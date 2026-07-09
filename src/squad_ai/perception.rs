//! Squad **perception + decision** — the unit-side twin of `ai::brain::think` (which is `Without<Unit>`).
//! Each tick it builds a [`Perception`] for every unit (fog LOS + examinables + threats + wounded
//! allies + squad anchor), routes it through the selected [`SquadPolicy`], and caches the chosen mode +
//! movement goal. Decisions are throttled per unit (a seed-staggered [`ThinkTimer`], as for creatures)
//! so the squad doesn't re-decide in lock-step; the *goal* is re-resolved every tick so cohesion tracks
//! the moving anchor without waiting for the next think (Dill Ch.3: decide at decision points, steer
//! continuously).
//!
//! Perception is grounded in the game's own signals: line of sight (`fog::FogGrid`), placed furniture
//! (`placement::PlacedIn`), scavengable bodies (`gore::Carryable`), and creature transforms — exactly
//! the affordance/state grounding that keeps AI behaviour (and later, generated dialogue) tied to the
//! world (Gallotta et al., "Large Language Models and Games: A Survey and Roadmap", 2024).

use bevy::prelude::*;

use crate::ai::brain::{ActiveBehavior, ThinkTimer};
use crate::ai::drives::{DriveId, Drives};
use crate::ai::utility::{Mode, Perception, SquadFields};
use crate::crab::Crab;
use crate::dungeon::Dungeon;
use crate::enemy::Enemy;
use crate::fog::FogGrid;
use crate::gore::Carryable;
use crate::health::Health;
use crate::placement::PlacedIn;
use crate::squad::{SquadMember, Unit};
use crate::util::nearest_planar;

use super::cohesion::{DesiredMove, SquadAnchor, SquadControlMode};
use super::policy::ActivePolicy;
use super::role::{RoleId, RoleBrains, LEASH};

/// Marks an entity a squad member has already studied/secured, so perception stops offering it as a
/// fresh examinable (one-way, like the fog reveal). Inserted by the Examine/SecureDoor actions.
#[derive(Component)]
pub struct Examined;

/// How far a unit perceives examinables (furniture / bodies) — the squad vision radius.
const EXAMINE_SIGHT: f32 = 8.0;
/// How far a unit perceives a hostile creature as a threat (line-of-sight bounded in the reveal grid).
const THREAT_SIGHT: f32 = 12.0;
/// How far the Psionic senses the watcher's anomaly signature — through walls (psi, not LOS).
const PSI_SIGHT: f32 = 16.0;
/// An ally at or below this health fraction is "down" and draws the Medic.
const WOUNDED_FRAC: f32 = 0.5;

/// Build perception, decide (throttled), resolve the movement goal, and cache both — for every unit.
#[allow(clippy::type_complexity)]
pub fn squad_think(
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    fog: Res<FogGrid>,
    anchor: Res<SquadAnchor>,
    control: Res<SquadControlMode>,
    brains: Res<RoleBrains>,
    policy: Res<ActivePolicy>,
    threats: Query<&Transform, (Or<(With<Crab>, With<Enemy>)>, Without<Unit>)>,
    bosses: Query<&Transform, With<Enemy>>,
    furniture: Query<&Transform, (With<PlacedIn>, Without<Examined>)>,
    bodies: Query<&Transform, (With<Carryable>, Without<Examined>)>,
    mut units: Query<
        (
            Entity,
            &Transform,
            &RoleId,
            &mut Drives,
            &mut ActiveBehavior,
            &mut ThinkTimer,
            &mut DesiredMove,
            &Health,
            Option<&crate::squad::MoveOrder>,
            &SquadMember,
        ),
        With<Unit>,
    >,
) {
    // World signals gathered once (planar positions), reused across all units.
    let threat_pos: Vec<Vec3> = threats.iter().map(|t| t.translation).collect();
    let boss_pos: Vec<Vec3> = bosses.iter().map(|t| t.translation).collect();
    let examinable_pos: Vec<Vec3> = furniture
        .iter()
        .chain(bodies.iter())
        .map(|t| t.translation)
        .collect();

    // Snapshot every unit's position + health so the Medic can find a wounded ally without borrowing
    // the mutable unit query twice.
    let unit_snapshot: Vec<(Entity, Vec3, f32)> = units
        .iter()
        .map(|(e, t, _, _, _, _, _, h, _, _)| (e, t.translation, health_frac(h)))
        .collect();

    let dt = time.delta_secs();

    for (entity, tf, role, mut drives, mut active, mut timer, mut desired, health, order, member) in
        &mut units
    {
        let pos = tf.translation;

        // Nearest hostile (for threat-driven behaviours). Bounded by THREAT_SIGHT.
        let threat = nearest_planar(pos, threat_pos.iter().map(|&p| ((), p)))
            .filter(|(_, _, d)| *d <= THREAT_SIGHT);
        // Nearest examinable within sight (furniture / body not yet studied).
        let examinable = nearest_planar(pos, examinable_pos.iter().map(|&p| ((), p)))
            .filter(|(_, _, d)| *d <= EXAMINE_SIGHT);
        // Nearest OTHER wounded ally within support sight.
        let wounded = nearest_planar(
            pos,
            unit_snapshot
                .iter()
                .filter(|(e, _, hf)| *e != entity && *hf <= WOUNDED_FRAC)
                .map(|&(_, p, _)| ((), p)),
        )
        .filter(|(_, _, d)| *d <= THREAT_SIGHT);
        // The watcher's anomaly signature — sensed through walls within PSI range.
        let anomaly = nearest_planar(pos, boss_pos.iter().map(|&p| ((), p)))
            .is_some_and(|(_, _, d)| d <= PSI_SIGHT);

        let anchor_dist = if anchor.valid {
            (anchor.pos - pos).length()
        } else {
            999.0
        };

        // Feed perception-derived squad drives (curiosity near study targets, cohesion when strayed).
        drives.set(DriveId::CURIOSITY, if examinable.is_some() { 0.8 } else { 0.0 });
        drives.set(DriveId::COHESION, (anchor_dist / LEASH).clamp(0.0, 1.0));

        let squad = SquadFields {
            anchor: anchor.valid.then_some(anchor.pos),
            anchor_dist,
            nearest_examinable: examinable.map(|(_, p, _)| p),
            examinable_dist: examinable.map(|(_, _, d)| d).unwrap_or(999.0),
            has_unexamined: if examinable.is_some() { 1.0 } else { 0.0 },
            nearest_wounded_ally: wounded.map(|(_, p, _)| p),
            wounded_ally_dist: wounded.map(|(_, _, d)| d).unwrap_or(999.0),
            ally_down: if wounded.is_some() { 1.0 } else { 0.0 },
            tracked_threat: threat.map(|(_, p, _)| p),
            threat_bearing_known: if threat.is_some() { 1.0 } else { 0.0 },
            anomaly_residue: if anomaly { 1.0 } else { 0.0 },
        };

        let perc = Perception {
            pos,
            nearest_unit: threat.map(|(_, p, _)| p),
            nearest_dist: threat.map(|(_, _, d)| d).unwrap_or(999.0),
            health_frac: health_frac(health),
            drives: drives.v,
            scent_hotspot: Vec3::ZERO,
            scent_val: 0.0,
            meat_hotspot: Vec3::ZERO,
            meat_val: 0.0,
            carrying: 0.0,
            prey_spotted: 0.0,
            rally_val: 0.0,
            alarm_val: 0.0,
            seen_by_squad: if fog.visible_at(dungeon.world_to_cell(pos)) { 1.0 } else { 0.0 },
            squad,
        };

        // Decide (throttled): re-run the policy only at decision points; keep the cached mode between.
        timer.0 -= dt;
        if timer.0 <= 0.0 {
            timer.0 = THINK_INTERVAL;
            let brain = brains.get(*role);
            let idx = policy.0.choose(&perc, &brain.behaviors, &mut active.rng);
            active.mode = brain.behaviors[idx].mode;
        }

        // Resolve the movement goal from the (possibly cached) mode + fresh perception every tick, so
        // cohesion tracks the moving anchor without waiting for the next think.
        let goal = resolve_goal(active.mode, &perc, &anchor);
        active.target = goal;

        // The player order is authoritative: a unit under a `MoveOrder` ignores the AI goal (its
        // `unit_movement` path is untouched). The control mode selects whether the AI plans for an
        // order-less unit at all.
        let ai_drives_this_unit = order.is_none()
            && match *control {
                SquadControlMode::Autonomous | SquadControlMode::BetweenOrders => true,
                // ControlOne hands the leader — spawn member 0 — to the player: it plans no autonomous
                // goal, so only the player's `MoveOrder` moves it, while the other four run the AI. The
                // documented "player drives the leader, four AI teammates" mode. Keyed on the
                // determinism-neutral `SquadMember` index (on EVERY unit, so it never splits the hashed
                // archetype), NOT the windowed-only `Leader` marker (which would — see `SquadPlugin`).
                SquadControlMode::ControlOne => member.0 != 0,
            };
        desired.goal = if ai_drives_this_unit { goal } else { None };
    }
}

/// Seconds between squad re-decisions (steering runs every tick). Matches the creature `THINK_INTERVAL`.
const THINK_INTERVAL: f32 = 0.3;

/// FollowAnchor deadband: within this planar distance of the anchor a unit HOLDS (no goal) instead of
/// steering onto the exact centroid. Keeps an idle squad in a loose blob (ORCA spaces the members)
/// that actually comes to rest, rather than every unit converging on one point and micro-jittering
/// forever. Sits below the `Regroup` leash so the hard cohesion pull still owns the strayed band.
const FOLLOW_DEADZONE: f32 = 2.5;

/// How far ahead a fleeing unit aims its away-from-threat goal. The goal is re-resolved every tick, so
/// this only needs to sit past `ARRIVE_RADIUS` (0.6) — it sets a decisive retreat heading, not a
/// destination; steering re-points it down the threat gradient each tick.
const FLEE_DISTANCE: f32 = 8.0;

fn health_frac(h: &Health) -> f32 {
    if h.max > 0.0 {
        (h.current / h.max).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Map a chosen [`Mode`] to a world-space movement goal using current perception. Stationary actions
/// (scan/ward/overwatch/wander) return `None` (hold position); locomotion actions aim at their subject.
fn resolve_goal(mode: Mode, perc: &Perception, anchor: &SquadAnchor) -> Option<Vec3> {
    let anchor_goal = anchor.valid.then_some(anchor.pos);
    match mode {
        // Cohesion: a strayed unit heads straight for the group reference point.
        Mode::Regroup => anchor_goal,
        // Loose formation: only steer toward the anchor when OUTSIDE the deadband; within it, hold
        // (None) so the idle squad settles into a loose blob instead of all piling onto the identical
        // centroid and vibrating in place (the moving anchor still pulls the band along).
        Mode::FollowAnchor => {
            if perc.squad.anchor_dist > FOLLOW_DEADZONE {
                anchor_goal
            } else {
                None
            }
        }
        // Flee AWAY from the threat, not toward the squad. The anchor is the living squad's centroid,
        // which under a swarm sits *inside* the threat — steering there packs the squad onto what
        // frightened it. Reynolds "flee" / Fray context-steering danger gradient: desired heading
        // points from the threat to the agent (Reynolds 87/99; Game AI Pro 2 Ch.18). With no known
        // threat the fear is ambient, so regroup to the squad for safety.
        Mode::Flee => match perc.squad.tracked_threat {
            Some(threat) => {
                let d = perc.pos - threat;
                let away = Vec3::new(d.x, 0.0, d.z).normalize_or_zero();
                if away == Vec3::ZERO {
                    anchor_goal
                } else {
                    Some(perc.pos + away * FLEE_DISTANCE)
                }
            }
            None => anchor_goal,
        },
        // Study / secure the nearest examinable.
        Mode::Examine | Mode::SecureDoor => perc.squad.nearest_examinable,
        // Move to the wounded ally.
        Mode::TendWounded => perc.squad.nearest_wounded_ally,
        // Advance to contact on the threat (Overwatch holds — see below).
        Mode::Engage => perc.squad.tracked_threat,
        // Stationary actions: hold position and let the action/facing systems do the work.
        Mode::Overwatch
        | Mode::Suppress
        | Mode::PsiScan
        | Mode::Commune
        | Mode::Ward
        | Mode::DeploySensor
        | Mode::Wander => None,
        // Creature modes never chosen by a unit brain — hold.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::drives::DRIVE_COUNT;

    fn perc_with(squad: SquadFields) -> Perception {
        Perception {
            pos: Vec3::ZERO,
            nearest_unit: None,
            nearest_dist: 999.0,
            health_frac: 1.0,
            drives: [0.0; DRIVE_COUNT],
            scent_hotspot: Vec3::ZERO,
            scent_val: 0.0,
            meat_hotspot: Vec3::ZERO,
            meat_val: 0.0,
            carrying: 0.0,
            prey_spotted: 0.0,
            rally_val: 0.0,
            alarm_val: 0.0,
            seen_by_squad: 0.0,
            squad,
        }
    }

    fn anchor_at(p: Vec3) -> SquadAnchor {
        SquadAnchor { pos: p, vel: Vec3::ZERO, valid: true }
    }

    #[test]
    fn cohesion_modes_aim_at_the_anchor() {
        // With no tracked threat and a far anchor (neutral `anchor_dist` = 999 > the deadband), every
        // cohesion mode — including Flee, which falls back to regroup when the fear is ambient — heads
        // for the anchor.
        let a = anchor_at(Vec3::new(5.0, 0.0, 2.0));
        let p = perc_with(SquadFields::neutral());
        for mode in [Mode::FollowAnchor, Mode::Regroup, Mode::Flee] {
            assert_eq!(resolve_goal(mode, &p, &a), Some(a.pos), "{mode:?}");
        }
    }

    #[test]
    fn flee_steers_away_from_a_known_threat() {
        // A frightened unit with a tracked threat retreats AWAY from it, never onto the squad centroid
        // (which under a swarm sits inside the threat). Unit at origin, threat to the +x side → the
        // flee goal must lie on the −x side, past the unit.
        let anchor = anchor_at(Vec3::new(1.0, 0.0, 0.0)); // anchor near the threat side
        let p = perc_with(SquadFields {
            tracked_threat: Some(Vec3::new(5.0, 0.0, 0.0)),
            ..SquadFields::neutral()
        });
        let goal = resolve_goal(Mode::Flee, &p, &anchor).expect("flee yields a goal");
        assert!(goal.x < 0.0, "flee goal should be away from the +x threat, got {goal:?}");
    }

    #[test]
    fn followanchor_holds_inside_the_deadband() {
        // Within the loose-formation deadband a unit holds (no goal) so the idle squad settles instead
        // of piling onto the exact centroid; outside it, the anchor pulls it back.
        let a = anchor_at(Vec3::new(2.0, 0.0, 0.0));
        let near = perc_with(SquadFields { anchor_dist: 1.0, ..SquadFields::neutral() });
        assert_eq!(resolve_goal(Mode::FollowAnchor, &near, &a), None, "inside deadband → hold");
        let far = perc_with(SquadFields { anchor_dist: 5.0, ..SquadFields::neutral() });
        assert_eq!(resolve_goal(Mode::FollowAnchor, &far, &a), Some(a.pos), "outside deadband → pull");
    }

    #[test]
    fn examine_aims_at_the_examinable() {
        let target = Vec3::new(3.0, 0.0, 0.0);
        let p = perc_with(SquadFields { nearest_examinable: Some(target), ..SquadFields::neutral() });
        assert_eq!(resolve_goal(Mode::Examine, &p, &anchor_at(Vec3::ZERO)), Some(target));
        assert_eq!(resolve_goal(Mode::SecureDoor, &p, &anchor_at(Vec3::ZERO)), Some(target));
    }

    #[test]
    fn tend_aims_at_the_wounded_ally_and_engage_at_the_threat() {
        let ally = Vec3::new(-2.0, 0.0, 1.0);
        let threat = Vec3::new(7.0, 0.0, -3.0);
        let p = perc_with(SquadFields {
            nearest_wounded_ally: Some(ally),
            tracked_threat: Some(threat),
            ..SquadFields::neutral()
        });
        assert_eq!(resolve_goal(Mode::TendWounded, &p, &anchor_at(Vec3::ZERO)), Some(ally));
        assert_eq!(resolve_goal(Mode::Engage, &p, &anchor_at(Vec3::ZERO)), Some(threat));
    }

    #[test]
    fn stationary_actions_hold_position() {
        let p = perc_with(SquadFields::neutral());
        for mode in [Mode::Overwatch, Mode::PsiScan, Mode::Commune, Mode::Ward, Mode::Wander] {
            assert_eq!(resolve_goal(mode, &p, &anchor_at(Vec3::ZERO)), None, "{mode:?}");
        }
    }

    #[test]
    fn invalid_anchor_yields_no_cohesion_goal() {
        let p = perc_with(SquadFields::neutral());
        let no_anchor = SquadAnchor::default(); // valid = false
        assert_eq!(resolve_goal(Mode::FollowAnchor, &p, &no_anchor), None);
    }
}
