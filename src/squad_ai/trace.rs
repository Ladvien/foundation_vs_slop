//! **Episode recording** — the bridge between the running simulation and `squad_ai::surprise`.
//!
//! Off by default. The shipped game pays one `if !enabled { return }` per fixed tick; a headless
//! evaluation (`squad_ai::evaluate`) flips it on and reads the [`EpisodeTrace`] + [`EpisodeOutcome`] out
//! at the end.
//!
//! # Sampling at decision points, not every tick
//!
//! Decisions are identified by `ActiveBehavior::decision`, a counter both think systems increment inside
//! their `ThinkTimer` gate.
//!
//! It is **not** `Changed<ActiveBehavior>`, and that distinction was a real bug. `squad_think` re-resolves
//! `active.target` from fresh perception on *every* tick (so cohesion tracks the moving anchor); the
//! `DerefMut` marks the component changed 18× per decision at a 0.3 s think interval. A `Changed`-keyed
//! recorder oversampled the squad ~18× with ~94% self-transitions, which made
//! `surprise::learnability` — whose entire purpose is to measure transition structure — read every brain
//! as maximally predictable. Creatures were unaffected (`think` `continue`s before touching `active`), so
//! the defect was invisible on the swarm side and would have silently destroyed the squad-side objective.
//!
//! # What "witnessed" means, and why the Psionic matters
//!
//! Hunicke & Chapman exploited change blindness so that dynamic difficulty would be *imperceptible*. The
//! objective here inverts that: behaviour only counts when the player could see **why** it happened.
//!
//! - A **creature** is witnessed when its own cell is inside the squad's live line of sight. A crab doing
//!   something ingenious in the fog is, to the player, something that did not happen.
//! - A **unit** is always on screen, so the question is whether its *cause* was. Every mode but `Flee`
//!   aims at something the player can see (an ally, a body, a door, the anchor). `Flee` is gated on the
//!   `FEAR` drive, which `ai::drives::TrackMaxFields` eases from the **stigmergy threat fields** — and
//!   those seep *through walls*. A unit bolting from a crab nobody can see reads as a bug, not as drama.
//!   So a `Flee` counts only when a threat is actually visible, **or a living Psionic is in the squad**:
//!   `psi_vision` paints those very threat channels as a heat wash across the floor, so the Psionic makes
//!   the invisible cause legible. The Psionic is the squad's *explicability organ*, and this is where that
//!   stops being a metaphor and starts being arithmetic.
//!
//! # Context fidelity
//!
//! [`Context`] is rebuilt here from components (`Drives`, `PerceptionLatch`) rather than captured inside
//! `think`. `PerceptionLatch` holds the *actual* hysteretic gate state the brain conditioned on, so the
//! three boolean flags are exact, not approximated. Fear is re-bucketed from `Drives`, which the think
//! systems read from the same component in the same tick.
//!
//! Even where a reconstruction were imperfect, it would be harmless: the baseline prior and every
//! candidate are recorded by *this same code*, so a systematic bias appears identically in `P` and `Q` and
//! cancels in the KL divergence. The context is a grouping key, never an input to a brain.

use bevy::prelude::*;

use crate::ai::brain::{ActiveBehavior, BrainId};
use crate::ai::drives::{DriveId, Drives};
use crate::ai::utility::Mode;
use crate::crab::{Crab, Scout};
use crate::dungeon::Dungeon;
use crate::enemy::Enemy;
use crate::fog::FogGrid;
use crate::health::Health;
use crate::squad::Unit;

use super::perception::PerceptionLatch;
use super::rl::Visitation;
use super::role::RoleId;
use super::surprise::{
    is_squad_duty, ActorKind, Context, Decision, EpisodeOutcome, EpisodeTrace, FearBucket,
};

/// The episode recorder. `enabled` is off in the shipped game; `evaluate` turns it on.
#[derive(Resource, Default)]
pub struct Recording {
    pub enabled: bool,
    pub trace: EpisodeTrace,
    pub outcome: EpisodeOutcome,
    /// Last seen health per unit, so damage can be accumulated as a sum of decreases. Keyed by
    /// `Entity::to_bits()` (index + generation), which is unique for the life of the entity.
    prev_unit_health: std::collections::HashMap<u64, f32>,
    /// Last recorded `ActiveBehavior::decision` per actor, so a decision is sampled exactly once.
    last_decision: std::collections::HashMap<u64, u32>,
}

impl Recording {
    /// Has this actor decided since we last recorded it? Advances the stored counter as a side effect.
    /// A fresh actor (first decision, or a crab bred mid-episode) is always new.
    fn is_new_decision(&mut self, entity: Entity, decision: u32) -> bool {
        let key = entity.to_bits();
        match self.last_decision.insert(key, decision) {
            Some(previous) => previous != decision,
            None => true,
        }
    }

    /// Begin a fresh episode.
    pub fn start(&mut self) {
        self.enabled = true;
        self.trace = EpisodeTrace::default();
        self.outcome = EpisodeOutcome::default();
        self.prev_unit_health.clear();
        self.last_decision.clear();
    }
}

/// Record every decision taken this tick. Runs on `FixedUpdate` after `AiSet::Think`, so both the
/// creature and squad think systems have written `ActiveBehavior`.
#[allow(clippy::too_many_arguments)]
pub fn record_decisions(
    mut rec: ResMut<Recording>,
    fog: Res<FogGrid>,
    dungeon: Res<Dungeon>,
    units: Query<
        (Entity, &Transform, &RoleId, &Drives, &ActiveBehavior, &PerceptionLatch),
        With<Unit>,
    >,
    creatures: Query<
        (Entity, &Transform, &BrainId, &Drives, &ActiveBehavior, Option<&Scout>),
        Without<Unit>,
    >,
    all_units: Query<&RoleId, With<Unit>>,
    threats: Query<&Transform, (Or<(With<Crab>, With<Enemy>)>, Without<Unit>)>,
) {
    if !rec.enabled {
        return;
    }

    // Is any hostile currently inside the squad's live line of sight? This is what makes a unit's flight
    // legible to the player.
    let threat_visible = threats
        .iter()
        .any(|t| fog.visible_at(dungeon.world_to_cell(t.translation)));
    // A living Psionic paints the danger fields across the floor (`psi_vision`), so a flight from an
    // unseen threat still has a cause the player can watch. Units despawn on death, so presence is life.
    let psionic_alive = all_units.iter().any(|r| *r == RoleId::Psionic);

    for (entity, _tf, role, drives, active, latch) in &units {
        if !rec.is_new_decision(entity, active.decision) {
            continue;
        }
        // `Flee` is the only mode whose cause can sit outside the player's perception (the FEAR drive
        // reads threat fields through walls). Everything else aims at something on screen.
        let witnessed = if active.mode == Mode::Flee {
            threat_visible || psionic_alive
        } else {
            true
        };
        if is_squad_duty(active.mode) {
            rec.outcome.squad_duty_decisions += 1;
        }
        rec.trace.decisions.push(Decision {
            actor_id: entity.to_bits(),
            context: Context {
                actor: ActorKind::Role(*role),
                fear: FearBucket::of(drives.v[DriveId::FEAR.0]),
                threat_known: latch.threat,
                ally_down: latch.ally_down,
                past_leash: latch.past_leash,
            },
            mode: active.mode,
            witnessed,
        });
    }

    for (entity, tf, brain, drives, active, scout) in &creatures {
        if !rec.is_new_decision(entity, active.decision) {
            continue;
        }
        let actor = match brain {
            BrainId::Smiley => ActorKind::Smiley,
            // The `Scout` marker is the authority; `BrainId` and it are assigned together at spawn.
            BrainId::Crab | BrainId::Scout => {
                if scout.is_some() {
                    ActorKind::Scout
                } else {
                    ActorKind::Crab
                }
            }
        };
        // A creature is witnessed exactly when the player can see it act.
        let witnessed = fog.visible_at(dungeon.world_to_cell(tf.translation));
        rec.trace.decisions.push(Decision {
            actor_id: entity.to_bits(),
            context: Context {
                actor,
                fear: FearBucket::of(drives.v[DriveId::FEAR.0]),
                // Creatures carry no squad perception; `SquadFields::neutral()` is all-false, and the
                // prior is recorded through this same branch, so the two agree by construction.
                threat_known: false,
                ally_down: false,
                past_leash: false,
            },
            mode: active.mode,
            witnessed,
        });
    }
}

/// Accumulate the episode's outcome — the evidence the *behavioural* minimal criterion rules on. Runs on
/// `FixedUpdate` after `AiSet::Think`.
///
/// This is also the first live call site of [`Visitation`], which has existed (with a `novelty_reward`
/// and a coverage counter) and been unit-tested since the RL scaffolding landed, without ever being
/// invoked.
pub fn record_outcome(
    mut rec: ResMut<Recording>,
    mut visits: ResMut<Visitation>,
    mut dead_crabs: RemovedComponents<Crab>,
    dungeon: Res<Dungeon>,
    units: Query<(Entity, &Transform, &Health), With<Unit>>,
    crabs: Query<(), With<Crab>>,
) {
    // `RemovedComponents` is a per-system event cursor: if we return early without draining it, the
    // events pile up and the *next* enabled episode counts deaths from before it began. Drain first.
    let crabs_died = dead_crabs.read().count() as u32;
    if !rec.enabled {
        return;
    }
    rec.outcome.crabs_killed += crabs_died;

    if rec.outcome.reachable_cells == 0 {
        rec.outcome.reachable_cells = (0..dungeon.height as i32)
            .flat_map(|y| (0..dungeon.width as i32).map(move |x| IVec2::new(x, y)))
            .filter(|c| dungeon.is_floor(*c))
            .count() as u32;
    }

    let mut squad_size = 0u32;
    for (entity, tf, health) in &units {
        squad_size += 1;
        visits.visit(dungeon.world_to_cell(tf.translation));
        // Damage is the sum of health *decreases*; healing (the Medic) must not offset it, or a tended
        // squad would read as one that was never in danger.
        let key = entity.to_bits();
        if let Some(prev) = rec.prev_unit_health.get(&key)
            && *prev > health.current
        {
            rec.outcome.unit_damage_taken += *prev - health.current;
        }
        rec.prev_unit_health.insert(key, health.current);
    }

    rec.outcome.squad_size = rec.outcome.squad_size.max(squad_size);
    rec.outcome.survivors = squad_size;
    rec.outcome.crabs_alive = crabs.iter().count() as u32;
    rec.outcome.cells_covered = visits.coverage() as u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_recording_is_off_and_empty() {
        let rec = Recording::default();
        assert!(!rec.enabled, "the shipped game must not record");
        assert!(rec.trace.is_empty());
    }

    #[test]
    fn start_clears_the_previous_episode() {
        let mut rec = Recording::default();
        rec.outcome.crabs_killed = 7;
        rec.prev_unit_health.insert(3, 42.0);
        rec.trace.decisions.push(Decision {
            actor_id: 0,
            context: Context {
                actor: ActorKind::Crab,
                fear: FearBucket::Calm,
                threat_known: false,
                ally_down: false,
                past_leash: false,
            },
            mode: Mode::Wander,
            witnessed: true,
        });
        rec.start();
        assert!(rec.enabled);
        assert!(rec.trace.is_empty(), "a new episode must not inherit decisions");
        assert_eq!(rec.outcome.crabs_killed, 0, "a new episode must not inherit an outcome");
        assert!(rec.prev_unit_health.is_empty(), "stale healths would fabricate damage");
    }
}
