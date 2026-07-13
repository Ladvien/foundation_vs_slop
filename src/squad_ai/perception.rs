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
use crate::ai::utility::{Mode, Perception, SquadFields, NO_TARGET_DIST};
use crate::crab::Crab;
use crate::dungeon::Dungeon;
use crate::enemy::Enemy;
use crate::fog::FogGrid;
use crate::gore::Carryable;
use crate::health::Health;
use crate::light::{light_push, LightField};
use crate::parasite::Infestation;
use crate::placement::PlacedIn;
use crate::sim::SimTuning;
use crate::squad::{SquadMember, Unit};
use crate::util::nearest_planar;

use super::cohesion::{DesiredMove, SquadAnchor, SquadControlMode};
use super::policy::ActivePolicy;
use super::role::{RoleId, RoleBrains};

/// Marks an entity a squad member has already studied/secured, so perception stops offering it as a
/// fresh examinable (one-way, like the fog reveal). Inserted by the Examine/SecureDoor actions.
#[derive(Component)]
pub struct Examined;

/// Per-unit **Schmitt latches** for the boolean perception gates.
///
/// Every gate a role brain reads is a hard threshold fed through `Curve::Step`, and a `Step` zeroes the
/// behaviour's product-score outright. So a threat hovering at exactly `THREAT_SIGHT` used to flip
/// `threat_bearing_known` 1↔0 on consecutive thinks, and the Gunman flip-flopped Overwatch↔FollowAnchor
/// every 0.3 s. The `ThinkTimer` bounds how *fast* that thrash runs; it cannot stop it.
///
/// A mode-commitment bonus in `decide` would not help either — a gated-off behaviour scores zero no matter
/// how committed the unit is — and `decide` is shared with the crab and boss brains, so changing it would
/// detune the swarm. The flicker has to die at the *fact*, which is a squad-only perception concern. Hence
/// an enter/exit band per gate, latched here (Schmitt trigger; the same idea as `FOLLOW_DEADZONE`).
///
/// Determinism: this is per-unit state, so it is inserted on **every** unit at spawn (a component on only
/// some units would split the hashed archetype — see the `Leader` note in `squad.rs`). Its update is a pure
/// function of the previous latch and this tick's scalars.
#[derive(Component, Default)]
pub struct PerceptionLatch {
    pub threat: bool,
    pub examinable: bool,
    pub anomaly: bool,
    pub ally_down: bool,
    pub past_leash: bool,
    /// A light-averse creature is within the Researcher's warding range (latches `Mode::Ward`).
    pub photophobe: bool,
}

/// The planar distance of a [`nearest_planar`] hit, or [`NO_TARGET_DIST`] when there is no candidate at
/// all. "No candidate" and "candidate far away" must read the same to the latches, so an emptied query
/// (the last crab died) releases a gate exactly as a retreating one does.
fn dist_of<T>(hit: Option<(T, Vec3, f32)>) -> f32 {
    hit.map_or(NO_TARGET_DIST, |(_, _, d)| d)
}

/// A latch that turns **on** when `value` falls to `enter` and off only once it climbs back past `exit`.
/// Requires `exit >= enter`; the gap is the dead band that kills boundary thrash.
///
/// Pure, so the anti-thrash property is unit-testable without an ECS.
fn latch_when_below(prev: bool, value: f32, enter: f32, exit: f32) -> bool {
    debug_assert!(exit >= enter, "hysteresis band must be non-negative");
    if prev {
        value <= exit
    } else {
        value <= enter
    }
}

/// The mirror of [`latch_when_below`]: on when `value` rises to `enter`, off only once it drops below
/// `exit`. Requires `exit <= enter`.
fn latch_when_above(prev: bool, value: f32, enter: f32, exit: f32) -> bool {
    debug_assert!(exit <= enter, "hysteresis band must be non-negative");
    if prev {
        value >= exit
    } else {
        value >= enter
    }
}

// The squad perception sight ranges + Schmitt hysteresis bands (EXAMINE_SIGHT/_RELEASE, THREAT_SIGHT/
// _RELEASE, PSI_SIGHT/_RELEASE, WARD_SIGHT/_RELEASE, WOUNDED_FRAC/_RELEASE) and the cohesion leash band
// (LEASH → `beh.perception.leash`, LEASH_IN) now live in the `behavior:` config slice
// (`BehaviorTuning::perception`), read as `Res<BehaviorTuning>` by `squad_think`. See src/behavior_tuning.rs.
// (`FOLLOW_DEADZONE` / `FLEE_DISTANCE` below stay in code: they are used only by the pure `resolve_goal`
// helper, which is kept config-free so it stays unit-testable.)

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
    // The illuminance grid — an SCP-150-infested unit is steered toward the darkest nearby cell (Phase 4
    // host manipulation). Read at this schedule slot; a possibly-1-tick-stale field is still deterministic.
    light: Res<LightField>,
    // Parasite manipulation strengths (`sim.parasite.manip_*`).
    sim: Res<SimTuning>,
    threats: Query<&Transform, (Or<(With<Crab>, With<Enemy>)>, Without<Unit>)>,
    bosses: Query<&Transform, With<Enemy>>,
    // Light-averse creatures the Researcher wards with its beam — crabs AND parasite mancas (mancas are
    // photophobic but neither `Crab` nor `Enemy`, so they are not in `threats`). Read-only, `Without<Unit>`.
    photophobes: Query<&Transform, (With<crate::light::Photophobic>, Without<Unit>)>,
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
            &mut PerceptionLatch,
            &Health,
            Option<&crate::squad::MoveOrder>,
            &SquadMember,
            // Always-present SCP-150 host state (Phase 4 manipulation reads `active`).
            &Infestation,
            // The Researcher aims its warding beam here; `None` for every other unit and mode.
            &mut crate::squad::FacingOverride,
        ),
        With<Unit>,
    >,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
) {
    // World signals gathered once (planar positions), reused across all units.
    let threat_pos: Vec<Vec3> = threats.iter().map(|t| t.translation).collect();
    let boss_pos: Vec<Vec3> = bosses.iter().map(|t| t.translation).collect();
    let photophobe_pos: Vec<Vec3> = photophobes.iter().map(|t| t.translation).collect();
    let examinable_pos: Vec<Vec3> = furniture
        .iter()
        .chain(bodies.iter())
        .map(|t| t.translation)
        .collect();

    // Snapshot every unit's position + health so the Medic can find a wounded ally without borrowing
    // the mutable unit query twice.
    let unit_snapshot: Vec<(Entity, Vec3, f32)> = units
        .iter()
        .map(|(e, t, _, _, _, _, _, _, h, _, _, _, _)| (e, t.translation, health_frac(h)))
        .collect();

    let dt = time.delta_secs();

    for (
        entity,
        tf,
        role,
        mut drives,
        mut active,
        mut timer,
        mut desired,
        mut latch,
        health,
        order,
        member,
        infestation,
        mut facing_override,
    ) in &mut units
    {
        let pos = tf.translation;

        // Each cue is resolved to its nearest candidate WITHOUT a range filter, then admitted through a
        // Schmitt band. Filtering first and latching second would be a bug: a threat that steps one
        // millimetre past the sight radius would vanish from the cue entirely, and the latch would have
        // nothing to hold on to.
        let nearest_threat = nearest_planar(pos, threat_pos.iter().map(|&p| ((), p)));
        let nearest_examinable = nearest_planar(pos, examinable_pos.iter().map(|&p| ((), p)));
        let nearest_boss = nearest_planar(pos, boss_pos.iter().map(|&p| ((), p)));
        let nearest_photophobe = nearest_planar(pos, photophobe_pos.iter().map(|&p| ((), p)));
        // Nearest OTHER ally hurt enough to count as "down". Hysteresis lives in the *threshold*, not in a
        // post-hoc test of the nearest ally's health: a Medic already treating someone keeps looking for
        // patients up to `WOUNDED_FRAC_RELEASE`, so the first tick of healing doesn't un-declare the
        // emergency and walk them away mid-treatment. (Filtering by distance first and health second would
        // let a healthy ally standing closer mask a wounded one further off.)
        let wounded_cut = if latch.ally_down {
            beh.perception.wounded_frac_release
        } else {
            beh.perception.wounded_frac
        };
        let nearest_wounded = nearest_planar(
            pos,
            unit_snapshot
                .iter()
                .filter(|(e, _, hf)| *e != entity && *hf <= wounded_cut)
                .map(|&(_, p, _)| ((), p)),
        )
        .filter(|(_, _, d)| *d <= beh.perception.threat_sight);

        let anchor_dist = if anchor.valid {
            (anchor.pos - pos).length()
        } else {
            NO_TARGET_DIST
        };

        // --- Latch every boolean gate through its dead band (pure; see `latch_when_below`). ---
        latch.threat = latch_when_below(
            latch.threat,
            dist_of(nearest_threat),
            beh.perception.threat_sight,
            beh.perception.threat_sight_release,
        );
        latch.examinable = latch_when_below(
            latch.examinable,
            dist_of(nearest_examinable),
            beh.perception.examine_sight,
            beh.perception.examine_sight_release,
        );
        latch.anomaly = latch_when_below(
            latch.anomaly,
            dist_of(nearest_boss),
            beh.perception.psi_sight,
            beh.perception.psi_sight_release,
        );
        latch.photophobe = latch_when_below(
            latch.photophobe,
            dist_of(nearest_photophobe),
            beh.perception.ward_sight,
            beh.perception.ward_sight_release,
        );
        // The band is already applied via `wounded_cut` above, so the latch just records the outcome for
        // the next tick's threshold.
        latch.ally_down = nearest_wounded.is_some();
        latch.past_leash =
            latch_when_above(latch.past_leash, anchor_dist, beh.perception.leash, beh.perception.leash_in);

        // A cue is only *admitted* to perception once its latch is on, so `resolve_goal` can never aim at a
        // subject the brain was not allowed to see.
        let threat = if latch.threat { nearest_threat } else { None };
        let examinable = if latch.examinable { nearest_examinable } else { None };
        let photophobe = if latch.photophobe { nearest_photophobe } else { None };
        let wounded = nearest_wounded;

        // Feed perception-derived squad drives (curiosity near study targets, cohesion when strayed).
        drives.set(DriveId::CURIOSITY, if latch.examinable { 0.8 } else { 0.0 });
        drives.set(DriveId::COHESION, (anchor_dist / beh.perception.leash).clamp(0.0, 1.0));

        // SCP-150 host manipulation (the extended phenotype, Heil 2016): an infested unit has its social
        // drives hijacked — COHESION forced low so it stops rejoining the squad, CURIOSITY forced high so it
        // wanders off. The parasite is isolating its host before the brood bursts (its movement goal is also
        // steered toward the dark, below). Applied AFTER the baseline `set()`s so it overrides them; this is
        // the sole drive/goal owner, so no competing writer.
        let infested = infestation.active;
        if infested {
            drives.set(DriveId::COHESION, sim.parasite.manip_cohesion_drop);
            drives.set(DriveId::CURIOSITY, sim.parasite.manip_curiosity_gain);
        }

        let squad = SquadFields {
            anchor: anchor.valid.then_some(anchor.pos),
            anchor_dist,
            nearest_examinable: examinable.map(|(_, p, _)| p),
            examinable_dist: dist_of(examinable),
            has_unexamined: if latch.examinable { 1.0 } else { 0.0 },
            nearest_wounded_ally: wounded.map(|(_, p, _)| p),
            wounded_ally_dist: wounded.map_or(NO_TARGET_DIST, |(_, _, d)| d),
            ally_down: if latch.ally_down { 1.0 } else { 0.0 },
            tracked_threat: threat.map(|(_, p, _)| p),
            threat_bearing_known: if latch.threat { 1.0 } else { 0.0 },
            anomaly_residue: if latch.anomaly { 1.0 } else { 0.0 },
            past_leash: if latch.past_leash { 1.0 } else { 0.0 },
            nearest_photophobe: photophobe.map(|(_, p, _)| p),
            photophobe_bearing_known: if latch.photophobe { 1.0 } else { 0.0 },
        };

        let perc = Perception {
            pos,
            nearest_unit: threat.map(|(_, p, _)| p),
            nearest_dist: dist_of(threat),
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
            // Units don't investigate the din (the acoustic Investigate behaviour is crab-only).
            noise_draw: 0.0,
            squad,
        };

        // Decide (throttled): re-run the policy only at decision points; keep the cached mode between.
        timer.0 -= dt;
        if timer.0 <= 0.0 {
            timer.0 = beh.perception.squad_think_interval;
            let brain = brains.get(*role);
            let idx = policy.0.choose(&perc, &brain.behaviors, *role, &mut active.rng);
            active.mode = brain.behaviors[idx].mode;
            // A real decision. `squad_ai::trace` samples on this, NOT on `Changed<ActiveBehavior>` — the
            // unconditional `active.target` write below marks the component changed every tick.
            active.decision = active.decision.wrapping_add(1);
        }

        // The Researcher aims its warding beam. When its AI holds `Mode::Ward` (a photophobe is in range),
        // turn the body to face that creature: `unit_facing` reads `FacingOverride` above aim/travel, and
        // the flashlight cone in `light::apply_dynamic_lights` follows the resulting facing, shoving the
        // creature down-light. The target refreshes every tick (fresh nearest) so the beam tracks a moving
        // crab smoothly even between throttled decisions. Role-gated so the Psionic's own `Mode::Ward`
        // (guard a downed ally) is untouched; `None` for every other unit and whenever not warding, so
        // facing falls back to aim then travel. Ref: Björk & Michelsen, FDG 2014 — light as a deterrent.
        facing_override.0 = if *role == RoleId::Researcher && active.mode == Mode::Ward {
            photophobe.map(|(_, p, _)| p)
        } else {
            None
        };

        // Resolve the movement goal from the (possibly cached) mode + fresh perception every tick, so
        // cohesion tracks the moving anchor without waiting for the next think.
        let mut goal = resolve_goal(active.mode, &perc, &anchor);
        // SCP-150 manipulation: override an infested unit's goal toward the darkest nearby cell (down the
        // `LightField` gradient), so it drifts into shadow to be isolated. Falls back to its normal goal
        // where the field is flat.
        if infested {
            if let Some(dark) = dark_goal(&light, &dungeon, pos, sim.parasite.manip_dark_gain) {
                goal = Some(dark);
            }
        }
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

// Seconds between squad re-decisions (`squad_think_interval`) now lives in the `behavior:` config slice
// (`BehaviorTuning::perception::squad_think_interval`), read as `Res<BehaviorTuning>`.

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

/// The darkest-nearby-cell goal for an infested unit: a point `lookahead` world units ahead down the
/// `LightField` gradient (toward shadow). `None` where the field is flat (no gradient to follow), so the
/// unit keeps its normal goal there. Reuses `light::light_push` (negative gain ⇒ toward dark), the same
/// primitive the photophobic crabs steer with. The manipulation strengths themselves live in
/// `sim::ParasiteTuning` (`manip_*`).
fn dark_goal(light: &LightField, dungeon: &Dungeon, pos: Vec3, lookahead: f32) -> Option<Vec3> {
    let toward_dark = light_push(light, dungeon, pos, -1.0);
    let dir = toward_dark.normalize_or_zero();
    (dir.length_squared() > 1.0e-6).then(|| pos + dir * lookahead)
}

/// Map a chosen [`Mode`] to a world-space movement goal using current perception. Stationary actions
/// (scan/ward/overwatch/wander) return `None` (hold position); locomotion actions aim at their subject.
fn resolve_goal(mode: Mode, perc: &Perception, anchor: &SquadAnchor) -> Option<Vec3> {
    let anchor_goal = anchor.valid.then_some(anchor.pos);
    match mode {
        // Both cohesion modes aim at the group reference point, and both stop short of it: outside the
        // deadband they steer to the anchor; within it they hold (None), so the squad settles into a loose
        // blob (ORCA spaces the members) instead of every unit converging on one identical point and
        // vibrating there. The moving anchor still pulls the band along.
        //
        // `Regroup` needs the deadband too, not just `FollowAnchor`. Modes are cached between thinks (up to
        // `THINK_INTERVAL`), so a unit that has just run home is still in `Regroup` for a fraction of a
        // second after arriving — without the deadband it would spend that time steering into the exact
        // centroid, and `avoids: true` would keep it from reading as settled to ORCA.
        Mode::Regroup | Mode::FollowAnchor => {
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
        // Creature modes, never chosen by a unit brain — hold. Enumerated rather than caught by a `_`
        // wildcard so that adding a `Mode` is a COMPILE ERROR here instead of a silent hold: a new squad
        // action that forgot its goal mapping would otherwise stand still and look like an AI bug.
        Mode::Forage
        | Mode::Latch
        | Mode::Chase
        | Mode::HuntBlood
        | Mode::SeekMeat
        | Mode::Carry
        | Mode::Scout
        | Mode::Mark
        | Mode::Rally
        | Mode::Muster
        | Mode::Investigate => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::drives::DRIVE_COUNT;

    /// The shipped perception knobs — the sight bands / leash the latch tests exercise now live in config.
    fn bp() -> crate::behavior_tuning::PerceptionTuning {
        crate::behavior_tuning::BehaviorTuning::default().perception
    }

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
            noise_draw: 0.0,
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
    fn both_cohesion_modes_hold_inside_the_deadband() {
        // Within the loose-formation deadband a unit holds (no goal) so the idle squad settles instead
        // of piling onto the exact centroid; outside it, the anchor pulls it back.
        //
        // `Regroup` is included deliberately: modes are cached between thinks, so a unit that has just run
        // home is still in Regroup for a fraction of a second after arriving. Without the deadband it
        // would steer into the exact centroid for that window, and never read as settled to ORCA.
        let a = anchor_at(Vec3::new(2.0, 0.0, 0.0));
        for mode in [Mode::FollowAnchor, Mode::Regroup] {
            let near = perc_with(SquadFields { anchor_dist: 1.0, ..SquadFields::neutral() });
            assert_eq!(resolve_goal(mode, &near, &a), None, "{mode:?} inside deadband → hold");
            let far = perc_with(SquadFields { anchor_dist: 5.0, ..SquadFields::neutral() });
            assert_eq!(resolve_goal(mode, &far, &a), Some(a.pos), "{mode:?} outside deadband → pull");
        }
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
    fn latch_enters_at_the_tight_bound_and_releases_only_past_the_band() {
        // The core Schmitt property. Acquire at 12.0, hold anywhere inside the band, release only beyond
        // 13.5 — and, coming back, re-acquire only at 12.0 again.
        assert!(!latch_when_below(false, 12.5, 12.0, 13.5), "must not acquire inside the band");
        assert!(latch_when_below(false, 12.0, 12.0, 13.5), "acquires at the tight bound");
        assert!(latch_when_below(true, 13.5, 12.0, 13.5), "holds to the far edge of the band");
        assert!(!latch_when_below(true, 13.6, 12.0, 13.5), "releases past the band");
    }

    #[test]
    fn a_subject_oscillating_inside_the_band_never_thrashes() {
        // THE regression lock for the Gunman flip-flopping Overwatch↔FollowAnchor. A crab pacing across
        // the raw sight radius used to toggle `threat_bearing_known` on every think; with the band, one
        // acquisition survives the whole oscillation, and one release survives the way back out.
        let mut on = false;
        for step in 0..40 {
            // Sweep 11.8 ↔ 13.2 — straddling the 12.0 trigger, entirely inside the 12.0–13.5 band.
            let d = if step % 2 == 0 { 11.8 } else { 13.2 };
            let next = latch_when_below(on, d, bp().threat_sight, bp().threat_sight_release);
            if step > 0 {
                assert_eq!(next, on, "latch flipped at step {step} (d={d}) — that is the thrash");
            }
            on = next;
        }
        assert!(on, "the sweep dips inside the trigger, so the threat should be acquired and held");
    }

    #[test]
    fn latch_when_above_is_the_mirror_image() {
        // The leash: strayed once past LEASH_OUT, un-strayed only once well back inside LEASH_IN.
        assert!(!latch_when_above(false, 5.9, bp().leash, bp().leash_in), "not yet strayed");
        assert!(latch_when_above(false, 6.0, bp().leash, bp().leash_in), "strays at the leash");
        assert!(latch_when_above(true, 4.0, bp().leash, bp().leash_in), "still coming home at the band edge");
        assert!(!latch_when_above(true, 3.9, bp().leash, bp().leash_in), "home once well inside");
    }

    #[test]
    fn an_absent_subject_reads_as_far_away_and_releases_the_latch() {
        // "No candidate at all" (the last crab died) must release a gate exactly as a retreating one does,
        // rather than freezing the latch on its last value.
        let none: Option<((), Vec3, f32)> = None;
        assert_eq!(dist_of(none), NO_TARGET_DIST);
        assert!(!latch_when_below(true, dist_of(none), bp().threat_sight, bp().threat_sight_release));
    }

    #[test]
    fn the_leash_band_sits_outside_the_follow_deadzone() {
        // A regrouping unit releases its leash latch at LEASH_IN and is then owned by FollowAnchor, which
        // holds inside FOLLOW_DEADZONE. If the release landed *inside* the deadzone the unit would arrive,
        // stop being strayed, hold, drift out, and re-stray — a slow oscillation instead of settling.
        assert!(bp().leash_in > FOLLOW_DEADZONE, "leash release must hand off to FollowAnchor, not to itself");
        assert!(bp().leash > bp().leash_in, "the leash band must have width");
    }

    #[test]
    fn invalid_anchor_yields_no_cohesion_goal() {
        let p = perc_with(SquadFields::neutral());
        let no_anchor = SquadAnchor::default(); // valid = false
        assert_eq!(resolve_goal(Mode::FollowAnchor, &p, &no_anchor), None);
    }
}
