//! The two SCP-1048 behaviour repertoires — data literals, exactly like the `*_brain()` builders in
//! `crate::ai::brain`, which is where these are called from ([`crate::ai::brain::authored_brains`]).
//!
//! **Two brains, not one and not four.** The benign original and the hostile copies have nearly
//! disjoint mode sets (wander/emote/build/flee vs rage/chase/strike/flee), so a single parameterised
//! brain would spend half its genome on behaviours that can never fire for a given bear. At the other
//! extreme, A/B/C differ *only* in which attack the executor plays — their ranks, gates and targets are
//! identical — so three copy-brains would triple the swarm genome's width to encode a distinction the
//! decision layer never makes.
//!
//! **Structure here, numbers in tuning.** The gate thresholds below are code constants, in the same
//! spirit as `CHASE_TILES` / `RALLY_MIN` / `ALARM_MIN` in `crate::ai::brain`: they shape *when the bear
//! decides* to do something. The numbers the executor then acts on — reach, cadence, damage, build
//! economy — live in `sim::Scp1048Tuning` and are evolved by `squad_ai::world_genome`.

use crate::ai::brain::Brain;
use crate::ai::drives::DriveId;
use crate::ai::utility::{Behavior, Consideration, Curve, Fact, Input, Mode, TargetKind};

/// How far a copy can be from a unit and still work itself into a threat display, in tiles. The
/// display falls off linearly with distance (the `smiley_brain` Chase shape), so a copy looms hardest
/// when it is close and drifts back to wandering when the squad is far away.
const RAGE_TILES: f32 = 14.0;

/// Inside this distance (world units) a copy stops merely looming and closes to contact.
const APPROACH_TILES: f32 = 8.0;

/// The distance at which a copy **commits** to attacking.
///
/// Deliberately a little longer than the shipped `sim.scp1048.strike_range` (1.2): this gate decides
/// *intent* — start the wind-up, raise the gun — while the executor's `strike_range` check decides
/// whether a blow actually connects. Keeping the two separate is what lets the reach evolve without
/// the brain thrashing between Chase and Strike at the boundary.
const STRIKE_COMMIT: f32 = 1.6;

/// The canonical soft flee gate used across this codebase's creature brains.
const FLEE_CURVE: Curve = Curve::Logistic { k: 10.0, x0: 0.45 };

/// A constant, low-scoring default. Every repertoire needs one that no perception can gate off, or
/// `decide` would find nothing eligible and the creature would freeze — `validate_unconditional_default`
/// rejects a brain without it. Copied from `smiley_brain`'s Wander.
fn unconditional_wander() -> Behavior {
    Behavior {
        mode: Mode::Wander,
        rank: 0,
        target: TargetKind::None,
        considerations: vec![Consideration {
            input: Input::Perc(Fact::SelfHealthFrac),
            curve: Curve::Linear { m: 0.0, b: 0.15 }, // constant low default
        }],
    }
}

/// **SCP-1048, the benign original.** The whole design is the `Emote` / `Build` pair, and it needs no
/// new perception plumbing at all — `seen_by_squad` is already computed for every agent in `think`.
///
/// Watched ⇒ it dances (the canon "dances while observed"). Unwatched *and* stocked with material ⇒ it
/// builds a copy (canon: no copy was ever seen being assembled). Neither ⇒ it wanders. That gives the
/// player a real, legible counter — keep eyes on the bear — instead of a timer they cannot influence.
///
/// Rank ladder: Flee(3) > Build(2) > Emote(1) > Wander(0). Fear beats everything, because a bear that
/// kept building through a firefight would read as scripted rather than alive.
pub fn bear_brain() -> Brain {
    Brain {
        behaviors: vec![
            unconditional_wander(),
            // Being looked at is the trigger, not a suppressor: the endearing display is what the
            // article is built around, and it is also the tell that the bear is aware of the squad.
            Behavior {
                mode: Mode::Emote,
                rank: 1,
                target: TargetKind::None,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::SeenBySquad),
                    curve: Curve::Step { threshold: 0.5, below: 0.0, above: 1.0 },
                }],
            },
            // **Gathering.** Gated ONLY on not being observed (the INVERTED step — `below: 1.0`).
            //
            // It must NOT also require `BuildReady`, and that is the subtle part: material accrues in
            // `replicate::scp1048_scavenge` only *while the bear is in* `Mode::Build`. Gating entry to
            // Build on already having the material is a deadlock — the bear could never take the first
            // step toward the thing it exists to do. `the_bear_can_start_gathering_from_nothing` locks
            // this; it is the bug that shipped in the first draft of this file.
            Behavior {
                mode: Mode::Build,
                rank: 2,
                target: TargetKind::None,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::SeenBySquad),
                    curve: Curve::Step { threshold: 0.5, below: 1.0, above: 0.0 },
                }],
            },
            Behavior {
                mode: Mode::Flee,
                rank: 3,
                target: TargetKind::None,
                considerations: vec![Consideration {
                    input: Input::Drive(DriveId::FEAR),
                    curve: FLEE_CURVE,
                }],
            },
            // **Assembling.** The same mode again at a higher rank, gated on having everything it
            // needs — the two-behaviours-one-mode shape `smiley_brain` already uses for its twin
            // `Chase`. This outranks `Flee`, so a bear that has gathered enough finishes the copy even
            // under fire: it is frightened of gunfire in general, but not enough to abandon a job it
            // has already done the work for. That is where `Fact::BuildReady` earns its place — and it
            // gives the search a real dial, since a candidate that drops this behaviour produces a
            // skittish bear that never quite finishes anything.
            Behavior {
                mode: Mode::Build,
                rank: 4,
                target: TargetKind::None,
                considerations: vec![
                    Consideration {
                        input: Input::Perc(Fact::SeenBySquad),
                        curve: Curve::Step { threshold: 0.5, below: 1.0, above: 0.0 },
                    },
                    Consideration {
                        input: Input::Perc(Fact::BuildReady),
                        curve: Curve::Step { threshold: 0.5, below: 0.0, above: 1.0 },
                    },
                ],
            },
        ],
    }
}

/// **SCP-1048-A / B / C, the hostile copies.** One repertoire for all three; the executor reads
/// `Scp1048Variant` to pick the attack.
///
/// Rank ladder: Flee(4) > Strike(3) > Rage/Chase(2) > Wander(0).
///
/// `Rage` and `Chase` deliberately **share rank 2**, the documented creature idiom in
/// `crate::ai::brain` ("creatures deliberately SHARE ranks... which reads as swarm variety rather than
/// as thrash") — and the reason the role rank-ladder invariant is correctly not applied to creatures.
/// With a shared rank the two distance curves decide which pull is stronger, so a copy looms while it
/// is still far and switches to closing as the gap shuts, rather than snapping between the two.
///
/// Escalating from a low-intensity display to contact — rather than charging straight in — is the
/// pattern animal-contest work reports: the individuals that win escalate readily to physical contact
/// while spending less time posturing (Bubak et al., "Assessment strategies and fighting patterns in
/// animal contests", Current Zoology 2016, doi:10.1093/cz/zow040).
pub fn bear_copy_brain() -> Brain {
    Brain {
        behaviors: vec![
            unconditional_wander(),
            // The threat display, falling off with distance — `smiley_brain`'s Chase shape.
            Behavior {
                mode: Mode::Rage,
                rank: 2,
                target: TargetKind::NearestUnit,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::NearestUnitDist),
                    curve: Curve::Linear { m: -1.0 / RAGE_TILES, b: 1.0 },
                }],
            },
            // Close to contact once the squad is within reach of a short rush.
            Behavior {
                mode: Mode::Chase,
                rank: 2,
                target: TargetKind::NearestUnit,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::NearestUnitDist),
                    curve: Curve::Step { threshold: APPROACH_TILES, below: 1.0, above: 0.0 },
                }],
            },
            Behavior {
                mode: Mode::Strike,
                rank: 3,
                target: TargetKind::NearestUnit,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::NearestUnitDist),
                    curve: Curve::Step { threshold: STRIKE_COMMIT, below: 1.0, above: 0.0 },
                }],
            },
            // Fear outranks the attack, but ONLY at range: the second consideration zeroes this out
            // once the copy is already in strike reach, so a cornered bear commits instead of bolting
            // mid-swing. Same shape as the crab's rally/alarm flee-suppression gate.
            Behavior {
                mode: Mode::Flee,
                rank: 4,
                target: TargetKind::None,
                considerations: vec![
                    Consideration { input: Input::Drive(DriveId::FEAR), curve: FLEE_CURVE },
                    Consideration {
                        input: Input::Perc(Fact::NearestUnitDist),
                        curve: Curve::Step { threshold: STRIKE_COMMIT, below: 0.0, above: 1.0 },
                    },
                ],
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::utility::validate_unconditional_default;

    #[test]
    fn both_bear_brains_have_an_unconditional_default() {
        // The same gate `validated_creatures` applies at startup. Without it `decide` could find no
        // eligible behaviour and the bear would stand frozen.
        validate_unconditional_default(&bear_brain().behaviors, "bear_brain").expect("bear_brain");
        validate_unconditional_default(&bear_copy_brain().behaviors, "bear_copy_brain")
            .expect("bear_copy_brain");
    }

    #[test]
    fn the_original_never_fights_and_the_copies_never_build() {
        // The split that justifies two brains rather than one parameterised repertoire: if these mode
        // sets ever overlap, collapsing them into a single brain becomes the cheaper design.
        let original: Vec<Mode> = bear_brain().behaviors.iter().map(|b| b.mode).collect();
        let copy: Vec<Mode> = bear_copy_brain().behaviors.iter().map(|b| b.mode).collect();
        for hostile in [Mode::Rage, Mode::Strike] {
            assert!(!original.contains(&hostile), "the benign original must not {hostile:?}");
        }
        for benign in [Mode::Build, Mode::Emote] {
            assert!(!copy.contains(&benign), "a hostile copy must not {benign:?}");
        }
    }

    #[test]
    fn the_bear_can_start_gathering_from_nothing() {
        // **The deadlock lock.** Material only accrues while the bear is already in `Mode::Build`, so
        // at least one Build behaviour must be reachable with `BuildReady == 0` — otherwise the bear
        // can never take the first step and replication never happens at all in a real game (only in
        // tests that hand it materials directly, which is exactly how this shipped unnoticed once).
        let cold = crate::ai::utility::Perception {
            pos: bevy::math::Vec3::ZERO,
            nearest_unit: None,
            nearest_dist: 999.0,
            health_frac: 1.0,
            drives: [0.0; crate::ai::drives::DRIVE_COUNT],
            scent_hotspot: bevy::math::Vec3::ZERO,
            scent_val: 0.0,
            meat_hotspot: bevy::math::Vec3::ZERO,
            meat_val: 0.0,
            carrying: 0.0,
            prey_spotted: 0.0,
            rally_val: 0.0,
            alarm_val: 0.0,
            seen_by_squad: 0.0, // unobserved
            noise_draw: 0.0,
            build_ready: 0.0, // ...and with nothing banked yet
            squad: crate::ai::utility::SquadFields::neutral(),
            water: crate::ai::utility::WaterObs::default(),
        };
        let brain = bear_brain();
        let mut rng = 1u32;
        let chosen = brain.behaviors[crate::ai::utility::decide(&brain.behaviors, &cold, &mut rng)].mode;
        assert_eq!(
            chosen,
            Mode::Build,
            "an unobserved bear with no material must still choose Build so it can start gathering"
        );
    }

    #[test]
    fn a_ready_bear_finishes_the_copy_even_while_frightened() {
        // The higher-ranked assembling behaviour must outrank Flee, or a bear that has done all the
        // gathering abandons the copy the moment a rifle goes off nearby.
        let brain = bear_brain();
        let flee_rank = brain
            .behaviors
            .iter()
            .find(|b| b.mode == Mode::Flee)
            .map(|b| b.rank)
            .expect("the original must be able to flee");
        let top_build = brain
            .behaviors
            .iter()
            .filter(|b| b.mode == Mode::Build)
            .map(|b| b.rank)
            .max()
            .expect("the original must be able to build");
        assert!(top_build > flee_rank, "a ready bear ({top_build}) must outrank Flee ({flee_rank})");
    }

    #[test]
    fn only_the_original_can_reach_mode_build() {
        // `Mode::Build` is what drives replication. A copy that could pick it would breed a second
        // generation, and `max_bears` is the only thing that would stop the family exploding.
        assert!(bear_brain().behaviors.iter().any(|b| b.mode == Mode::Build));
        assert!(!bear_copy_brain().behaviors.iter().any(|b| b.mode == Mode::Build));
    }

    #[test]
    fn a_copy_in_reach_commits_instead_of_fleeing() {
        // Flee outranks Strike, so without the distance gate on Flee a frightened copy would bolt
        // mid-swing every time the squad opened fire — and the copies would never land a blow.
        let flee = bear_copy_brain()
            .behaviors
            .into_iter()
            .find(|b| b.mode == Mode::Flee)
            .expect("the copy brain must be able to flee");
        assert_eq!(flee.considerations.len(), 2, "Flee must be gated on distance as well as FEAR");
    }
}
