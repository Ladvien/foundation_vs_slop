//! Role **action effects** — turn the cached decision ([`ActiveBehavior::mode`]) into world change and
//! dialogue. The Researcher/Engineer studying a subject marks it [`Examined`] (one-way, like fog
//! reveal) and narrates a finding; the Medic heals a wounded ally; the Psionic wards (damps its own
//! fear) and senses the watcher; the Gunman calls a threat. Each notable act emits a [`SquadUtterance`]
//! (throttled per unit) so interactions are *driven by what the unit does*, not scripted.
//!
//! Effects that change pinned state (health, drives, `Examined`) run on `FixedUpdate` after the squad
//! decides (`AiSet::Think`); dialogue generation from the emitted utterances is cosmetic (`Update`).

use bevy::prelude::*;

use crate::ai::brain::ActiveBehavior;
use crate::ai::drives::{DriveId, Drives};
use crate::ai::utility::Mode;
use crate::gore::Carryable;
use crate::health::Health;
use crate::placement::PlacedIn;
use crate::squad::{MoveOrder, Unit};
use crate::util::nearest_planar;

use super::dialogue::{ObsEvent, SquadUtterance};
use super::perception::Examined;
use super::role::RoleId;

/// Per-unit throttle on spontaneous utterances (seconds), so a held action doesn't spam one line every
/// tick. One-shot acts (Examine, gated by the `Examined` marker) don't use it.
#[derive(Component, Default)]
pub struct UtterCooldown(pub f32);

/// Seconds between repeated barks from one unit.
const UTTER_COOLDOWN: f32 = 4.0;
/// How close a unit must be to study / secure a subject.
const STUDY_RANGE: f32 = 1.6;
/// How close a medic must be to heal an ally.
const HEAL_RANGE: f32 = 1.6;
/// Medic heal rate (HP per second).
const HEAL_RATE: f32 = 20.0;
/// An ally at or below this fraction is a heal target (matches the perception threshold).
const WOUNDED_FRAC: f32 = 0.5;

/// Execute self/subject effects for each unit's chosen mode, and emit throttled observations. Does not
/// mutate *other* units (see [`medic_heal`] for cross-unit healing), so it needs only one `Unit` query.
#[allow(clippy::type_complexity)]
pub fn unit_actions(
    time: Res<Time>,
    mut commands: Commands,
    mut utter: MessageWriter<SquadUtterance>,
    furniture: Query<(Entity, &Transform), (With<PlacedIn>, Without<Examined>)>,
    bodies: Query<(Entity, &Transform), (With<Carryable>, Without<Examined>)>,
    // A player `MoveOrder` is authoritative — an ordered unit is under FULL player control, so it must
    // not auto-examine (permanently marking furniture/bodies `Examined` and denying the Researcher the
    // find), auto-ward, or bark. `Without<MoveOrder>` excludes those units from all action effects.
    mut units: Query<
        (Entity, &Transform, &RoleId, &ActiveBehavior, &mut Drives, &mut UtterCooldown),
        (With<Unit>, Without<MoveOrder>),
    >,
) {
    let dt = time.delta_secs();
    for (entity, tf, role, active, mut drives, mut cd) in &mut units {
        cd.0 = (cd.0 - dt).max(0.0);
        let pos = tf.translation;

        // Examine / SecureDoor is a one-shot: mark the subject `Examined` (so it isn't re-offered) and
        // narrate immediately, bypassing the utterance cooldown.
        if matches!(active.mode, Mode::Examine | Mode::SecureDoor) {
            let body = nearest_planar(pos, bodies.iter().map(|(e, t)| (e, t.translation)))
                .filter(|(_, _, d)| *d <= STUDY_RANGE);
            let object = nearest_planar(pos, furniture.iter().map(|(e, t)| (e, t.translation)))
                .filter(|(_, _, d)| *d <= STUDY_RANGE);
            // A researcher prefers a corpse; anyone else studies whichever is present (object first).
            let choice = if *role == RoleId::Researcher && body.is_some() {
                body.map(|b| (b.0, ObsEvent::ExaminedBody))
            } else if let Some(o) = object {
                Some((o.0, ObsEvent::ExaminedObject))
            } else {
                body.map(|b| (b.0, ObsEvent::ExaminedBody))
            };
            if let Some((target, ev)) = choice {
                commands.entity(target).try_insert(Examined);
                utter.write(SquadUtterance { speaker: entity, role: *role, event: ev, subject: Some(pos) });
            }
            continue;
        }

        // Ward has a self-effect (damp own fear, lift morale) that applies every tick it's active,
        // independent of the throttled bark below.
        if active.mode == Mode::Ward {
            let fear = drives.get(DriveId::FEAR);
            drives.set(DriveId::FEAR, fear * 0.5);
            let morale = drives.get(DriveId::MORALE);
            drives.set(DriveId::MORALE, (morale + 0.2).min(1.0));
        }

        // The rest are throttled spontaneous barks: one line per `UTTER_COOLDOWN` per unit.
        let event = match active.mode {
            Mode::PsiScan => Some(ObsEvent::SensedAnomaly),
            Mode::Commune => Some(ObsEvent::Communed),
            Mode::Ward => Some(ObsEvent::Warded),
            Mode::Overwatch | Mode::Engage | Mode::Suppress => Some(ObsEvent::ThreatSpotted),
            Mode::TendWounded => Some(ObsEvent::HealedAlly),
            Mode::Regroup => Some(ObsEvent::Regrouped),
            // The alarm beat: a unit that breaks and flees now shouts it, so an emergent fear-spike rout
            // reads as the squad REACTING, not silently glitching. An autonomously fleeing unit carries no
            // `MoveOrder`, so it already passes this system's `Without<MoveOrder>` filter. Cosmetic
            // `SquadUtterance` + the un-hashed cooldown only — no pinned mutation — so this FixedUpdate arm
            // stays determinism-safe.
            Mode::Flee => Some(ObsEvent::Panicked),
            _ => None,
        };
        if let Some(event) = event
            && cd.0 <= 0.0
        {
            cd.0 = UTTER_COOLDOWN;
            utter.write(SquadUtterance { speaker: entity, role: *role, event, subject: Some(pos) });
        }
    }
}

/// Apply Medic healing: any unit at/below [`WOUNDED_FRAC`] within [`HEAL_RANGE`] of a medic in
/// `TendWounded` regenerates HP. Two plain queries suffice — the medic read touches `Transform` +
/// `ActiveBehavior`, the patient write touches `Transform` (shared read) + `&mut Health`, so their
/// component access doesn't conflict even though both match every unit.
pub fn medic_heal(
    time: Res<Time>,
    // A medic under a player `MoveOrder` is being marched by the player, not tending — exclude it so
    // player-commanded units are genuinely under full control (matches the `unit_actions` gate).
    medics: Query<(&Transform, &ActiveBehavior), (With<Unit>, Without<MoveOrder>)>,
    mut patients: Query<(&Transform, &mut Health), With<Unit>>,
) {
    // Positions of medics currently tending.
    let tending: Vec<Vec3> = medics
        .iter()
        .filter(|(_, a)| a.mode == Mode::TendWounded)
        .map(|(t, _)| t.translation)
        .collect();
    if tending.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    for (tf, mut health) in &mut patients {
        if health.max <= 0.0 || health.current > health.max * WOUNDED_FRAC {
            continue;
        }
        let pos = tf.translation;
        let near_medic = tending
            .iter()
            .any(|m| (*m - pos).length() <= HEAL_RANGE && (*m - pos).length() > 0.01);
        if near_medic {
            health.current = (health.current + HEAL_RATE * dt).min(health.max);
        }
    }
}
