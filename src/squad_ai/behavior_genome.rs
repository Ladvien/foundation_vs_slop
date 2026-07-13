//! The **behaviour genome**: the game's evolvable *creature/squad physical-behaviour* config viewed as a
//! flat parameter vector.
//!
//! Fourth sibling of [`super::world_genome`] (world dynamics), [`super::level_genome`] (level generation),
//! and [`super::audio_genome`] (acoustics). Where `world_genome` searches field propagation + sim
//! dynamics, this one searches the per-agent behaviour knobs lifted into the `behavior:` config slice
//! ([`crate::behavior_tuning::BehaviorTuning`]) — locomotion speeds, steering/boids weights, senses,
//! combat cadence. It is a **separate** surface, so `world_genome`'s frozen 79-knob encoding (and its saved
//! `elites_world.ron`) are untouched.
//!
//! Two properties, mirroring the other genomes:
//!
//! 1. **Readable elites.** A [`BehaviorGenome`] decodes to a [`BehaviorTuning`] that serialises to the same
//!    RON a designer edits — an elite is a *diff of behaviour dials*, the reward-hacking guard (Skalse et
//!    al., "Defining and Characterizing Reward Hacking", arXiv:2209.13085).
//! 2. **Overlay onto a base (from `level_genome`).** The genome carries only a curated *subset* of the
//!    ~110 behaviour knobs — the highest-leverage continuous ones. [`decode`] overlays them onto a shipped
//!    [`BehaviorTuning`] base, so un-searched knobs pass through untouched and the search dimensionality
//!    stays tractable (~55, not ~110). Growing the subset later = add a [`BOUNDS`] row + one `encode`/
//!    `decode` line; config and runtime are untouched.
//!
//! **Feasibility by construction.** Each knob is clamped to a hard per-parameter [`BOUNDS`] range. Every
//! bound is chosen so the decoded config passes [`crate::behavior_tuning::validate_tuning`] *for any point
//! in the box* — in particular the ordering constraints (`boss.max_speed ≥ min_speed`,
//! `boss.sight_far ≥ sight_near`, Schmitt release ≥ trigger) hold because only the *upper* member of each
//! pair is searched and its lower bound sits above the fixed lower member. So `mutate` is one clamp, no
//! rejection loop (Skalse's "restrict the admissible set"). Mutation is the same scale-relative Gaussian
//! kernel as `world_genome` (`super::genome::flat_mutate`).

use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::genome::{flat_mutate, flat_range_check};
use crate::behavior_tuning::BehaviorTuning;

/// Number of searched knobs. The curated first tranche (54) plus a second tranche (35) of
/// tactical/combat-feel knobs (ORCA cohesion, boss senses, laser ballistics, crab pounce/carry,
/// parasite leap shape, mycelia coupling) appended as one block so the original 54 indices — and the
/// committed `elites_behavior.ron` written against them — are untouched.
pub const N: usize = 89;

/// Hard `(min, max)` per knob, in the **same order** [`encode`]/[`decode`] walk the config. Each shipped
/// value sits inside its range; the extremes are playable-but-different, never degenerate. Ordering
/// constraints are respected by construction (see the module note). The round-trip test pins the layout.
static BOUNDS: [(f32, f32); N] = [
    // ── crab (17) ──
    (0.8, 4.0),   // crab.speed
    (0.2, 1.2),   // crab.sep_radius
    (1.0, 15.0),  // crab.sep_strength
    (0.0, 2.0),   // crab.jitter_strength
    (1.0, 8.0),   // crab.stalk_band
    (0.0, 2.5),   // crab.stalk_strength
    (0.8, 4.0),   // crab.jump_len
    (0.5, 8.0),   // crab.jump_cooldown
    (0.0, 0.95),  // crab.pounce_blind_cos
    (1.0, 2.5),   // crab.muster_speed_mul
    (1.0, 2.5),   // crab.flee_speed_mul
    (1.0, 6.0),   // crab.climb_speed
    (0.05, 0.5),  // crab.scout_fraction
    (2.0, 12.0),  // crab.scout_sight
    (0.02, 1.0),  // crab.rally_live
    (0.05, 2.0),  // crab.alarm_high
    (0.5, 8.0),   // crab.promote_density
    // ── parasite_swarm (18) ──
    (0.0, 3.0),   // parasite_swarm.align_strength
    (0.0, 1.0),   // parasite_swarm.align_floor
    (0.0, 2.5),   // parasite_swarm.seek_strength
    (0.0, 2.5),   // parasite_swarm.cohesion_strength
    (0.1, 1.0),   // parasite_swarm.mill_speed_factor
    (1.0, 3.0),   // parasite_swarm.charge_speed_factor
    (0.05, 3.0),  // parasite_swarm.commit_ramp
    (0.05, 3.0),  // parasite_swarm.commit_decay
    (0.5, 6.0),   // parasite_swarm.commit_spread
    (0.0, 8.0),   // parasite_swarm.flash_impulse
    (0.5, 6.0),   // parasite_swarm.flash_sep_boost
    (0.005, 0.5), // parasite_swarm.rouse_threat
    (1.0, 12.0),  // parasite_swarm.rouse_proximity
    (0.5, 8.0),   // parasite_swarm.rouse_contagion_r
    (0.5, 6.0),   // parasite_swarm.huddle_radius
    (2.0, 12.0),  // parasite_swarm.harborage_sep
    (0.1, 3.0),   // parasite_swarm.settle_speed
    (0.1, 1.0),   // parasite_swarm.embed_range
    // ── boss (7) ──
    (0.8, 6.0),   // boss.max_speed  (lower bound > fixed min_speed 0.4 → max >= min always)
    (0.1, 2.0),   // boss.accel
    (4.0, 20.0),  // boss.sight_far  (lower bound > fixed sight_near 3.0 → far >= near always)
    (0.5, 8.0),   // boss.wander_interval
    (0.5, 8.0),   // boss.observe_dist
    (0.0, 8.0),   // boss.sep_strength
    (0.1, 3.0),   // boss.hit_memory
    // ── laser (4) ──
    (0.05, 1.0),  // laser.fire_interval
    (0.0, 0.5),   // laser.base_spread
    (0.0, 1.5),   // laser.move_spread
    (-0.5, 0.9),  // laser.front_arc_cos
    // ── squad_move (4) ──
    (2.0, 12.0),  // squad_move.unit_speed
    (3.0, 30.0),  // squad_move.turn_speed
    (0.5, 8.0),   // squad_move.pack_radius
    (0.3, 5.0),   // squad_move.blob_radius
    // ── mycelia_coupling (3) ──
    (0.01, 1.0),  // mycelia_coupling.graze_bite_rate
    (0.01, 1.0),  // mycelia_coupling.fruit_meat_rate
    (0.02, 1.0),  // mycelia_coupling.gaze_seen
    // ── perception (1) ──
    // Lower bound == the fixed `leash_in` (4.0), so the leash Schmitt band (`leash` outer >= `leash_in`
    // inner) can never invert under mutation — feasible by construction (see `validate_tuning`).
    (4.0, 15.0),  // perception.leash
    // ── expanded subset (35): tactical / combat-feel knobs, all free of ordering constraints ──
    // squad_move (9)
    (0.05, 0.5),  // squad_move.min_encumber
    (0.15, 0.6),  // squad_move.orca_radius
    (0.3, 3.0),   // squad_move.orca_time_horizon
    (2.0, 8.0),   // squad_move.orca_query_radius
    (0.3, 1.5),   // squad_move.arrive_radius
    (1.0, 8.0),   // squad_move.anchor_ease
    (0.8, 4.0),   // squad_move.study_range
    (0.8, 4.0),   // squad_move.heal_range
    (5.0, 40.0),  // squad_move.heal_rate
    // boss (5)
    (0.0, 0.99),  // boss.turn_cos
    (0.3, 0.99),  // boss.gaze_cos
    (0.0, 1.0),   // boss.look_amount
    (1.0, 8.0),   // boss.flee_speed
    (0.4, 3.0),   // boss.sep_radius
    // laser (4)
    (8.0, 40.0),  // laser.laser_speed
    (0.4, 3.0),   // laser.laser_life
    (0.0, 1.0),   // laser.dist_spread
    (4.0, 30.0),  // laser.dist_spread_range
    // crab (8)
    (0.3, 2.0),   // crab.jump_arc
    (0.5, 6.0),   // crab.jitter_freq
    (1.0, 2.5),   // crab.scout_speed_mul
    (0.1, 1.0),   // crab.eat_range
    (0.5, 6.0),   // crab.back_spread
    (0.5, 4.0),   // crab.carry_speed
    (0.5, 4.0),   // crab.deliver_range
    (0.2, 2.0),   // crab.grab_range
    // parasite_swarm (5)
    (0.3, 2.0),   // parasite_swarm.leap_arc
    (0.0, 0.95),  // parasite_swarm.blind_cos
    (0.0, 3.0),   // parasite_swarm.harborage_bias
    (2.0, 20.0),  // parasite_swarm.rouse_calm_seconds
    (0.0, 3.0),   // parasite_swarm.huddle_sep_strength
    // mycelia_coupling (3)
    (0.1, 1.0),   // mycelia_coupling.graze_reach
    (0.1, 1.0),   // mycelia_coupling.mat_dense_v
    (0.005, 0.2), // mycelia_coupling.mat_meat_rate
    // perception (1)
    (0.1, 1.0),   // perception.squad_think_interval
];

/// A behaviour config's evolvable subset, flattened. Meaningless without [`BOUNDS`]/[`decode`], which pin
/// the layout and the base overlay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BehaviorGenome(pub Vec<f32>);

/// Flatten the searched subset of a [`BehaviorTuning`] into the fixed-order knob vector `BOUNDS`/`decode`
/// agree on.
pub fn encode(b: &BehaviorTuning) -> BehaviorGenome {
    let mut v = Vec::with_capacity(N);
    // crab
    v.push(b.crab.speed);
    v.push(b.crab.sep_radius);
    v.push(b.crab.sep_strength);
    v.push(b.crab.jitter_strength);
    v.push(b.crab.stalk_band);
    v.push(b.crab.stalk_strength);
    v.push(b.crab.jump_len);
    v.push(b.crab.jump_cooldown);
    v.push(b.crab.pounce_blind_cos);
    v.push(b.crab.muster_speed_mul);
    v.push(b.crab.flee_speed_mul);
    v.push(b.crab.climb_speed);
    v.push(b.crab.scout_fraction);
    v.push(b.crab.scout_sight);
    v.push(b.crab.rally_live);
    v.push(b.crab.alarm_high);
    v.push(b.crab.promote_density);
    // parasite_swarm
    v.push(b.parasite_swarm.align_strength);
    v.push(b.parasite_swarm.align_floor);
    v.push(b.parasite_swarm.seek_strength);
    v.push(b.parasite_swarm.cohesion_strength);
    v.push(b.parasite_swarm.mill_speed_factor);
    v.push(b.parasite_swarm.charge_speed_factor);
    v.push(b.parasite_swarm.commit_ramp);
    v.push(b.parasite_swarm.commit_decay);
    v.push(b.parasite_swarm.commit_spread);
    v.push(b.parasite_swarm.flash_impulse);
    v.push(b.parasite_swarm.flash_sep_boost);
    v.push(b.parasite_swarm.rouse_threat);
    v.push(b.parasite_swarm.rouse_proximity);
    v.push(b.parasite_swarm.rouse_contagion_r);
    v.push(b.parasite_swarm.huddle_radius);
    v.push(b.parasite_swarm.harborage_sep);
    v.push(b.parasite_swarm.settle_speed);
    v.push(b.parasite_swarm.embed_range);
    // boss
    v.push(b.boss.max_speed);
    v.push(b.boss.accel);
    v.push(b.boss.sight_far);
    v.push(b.boss.wander_interval);
    v.push(b.boss.observe_dist);
    v.push(b.boss.sep_strength);
    v.push(b.boss.hit_memory);
    // laser
    v.push(b.laser.fire_interval);
    v.push(b.laser.base_spread);
    v.push(b.laser.move_spread);
    v.push(b.laser.front_arc_cos);
    // squad_move
    v.push(b.squad_move.unit_speed);
    v.push(b.squad_move.turn_speed);
    v.push(b.squad_move.pack_radius);
    v.push(b.squad_move.blob_radius);
    // mycelia_coupling
    v.push(b.mycelia_coupling.graze_bite_rate);
    v.push(b.mycelia_coupling.fruit_meat_rate);
    v.push(b.mycelia_coupling.gaze_seen);
    // perception
    v.push(b.perception.leash);
    // ── expanded subset (35), in BOUNDS order ──
    // squad_move (9)
    v.push(b.squad_move.min_encumber);
    v.push(b.squad_move.orca_radius);
    v.push(b.squad_move.orca_time_horizon);
    v.push(b.squad_move.orca_query_radius);
    v.push(b.squad_move.arrive_radius);
    v.push(b.squad_move.anchor_ease);
    v.push(b.squad_move.study_range);
    v.push(b.squad_move.heal_range);
    v.push(b.squad_move.heal_rate);
    // boss (5)
    v.push(b.boss.turn_cos);
    v.push(b.boss.gaze_cos);
    v.push(b.boss.look_amount);
    v.push(b.boss.flee_speed);
    v.push(b.boss.sep_radius);
    // laser (4)
    v.push(b.laser.laser_speed);
    v.push(b.laser.laser_life);
    v.push(b.laser.dist_spread);
    v.push(b.laser.dist_spread_range);
    // crab (8)
    v.push(b.crab.jump_arc);
    v.push(b.crab.jitter_freq);
    v.push(b.crab.scout_speed_mul);
    v.push(b.crab.eat_range);
    v.push(b.crab.back_spread);
    v.push(b.crab.carry_speed);
    v.push(b.crab.deliver_range);
    v.push(b.crab.grab_range);
    // parasite_swarm (5)
    v.push(b.parasite_swarm.leap_arc);
    v.push(b.parasite_swarm.blind_cos);
    v.push(b.parasite_swarm.harborage_bias);
    v.push(b.parasite_swarm.rouse_calm_seconds);
    v.push(b.parasite_swarm.huddle_sep_strength);
    // mycelia_coupling (3)
    v.push(b.mycelia_coupling.graze_reach);
    v.push(b.mycelia_coupling.mat_dense_v);
    v.push(b.mycelia_coupling.mat_meat_rate);
    // perception (1)
    v.push(b.perception.squad_think_interval);
    debug_assert_eq!(v.len(), N, "encode walked the wrong number of knobs");
    BehaviorGenome(v)
}

/// Rebuild a [`BehaviorTuning`] by overlaying the genome's searched subset onto `base` (the shipped config,
/// which carries the un-searched knobs). `Err` on wrong length — one path, no padding/truncation. The reads
/// bind in `encode` order; the round-trip test pins it.
pub fn decode(g: &BehaviorGenome, base: &BehaviorTuning) -> Result<BehaviorTuning, String> {
    if g.0.len() != N {
        return Err(format!("behaviour genome has {} knobs, expected {N}", g.0.len()));
    }
    let v = &g.0;
    let mut i = 0usize;
    macro_rules! f {
        () => {{
            let x = v[i];
            i += 1;
            x
        }};
    }
    let mut b = *base; // overlay: start from the shipped base, then stamp the searched knobs
    // crab
    b.crab.speed = f!();
    b.crab.sep_radius = f!();
    b.crab.sep_strength = f!();
    b.crab.jitter_strength = f!();
    b.crab.stalk_band = f!();
    b.crab.stalk_strength = f!();
    b.crab.jump_len = f!();
    b.crab.jump_cooldown = f!();
    b.crab.pounce_blind_cos = f!();
    b.crab.muster_speed_mul = f!();
    b.crab.flee_speed_mul = f!();
    b.crab.climb_speed = f!();
    b.crab.scout_fraction = f!();
    b.crab.scout_sight = f!();
    b.crab.rally_live = f!();
    b.crab.alarm_high = f!();
    b.crab.promote_density = f!();
    // parasite_swarm
    b.parasite_swarm.align_strength = f!();
    b.parasite_swarm.align_floor = f!();
    b.parasite_swarm.seek_strength = f!();
    b.parasite_swarm.cohesion_strength = f!();
    b.parasite_swarm.mill_speed_factor = f!();
    b.parasite_swarm.charge_speed_factor = f!();
    b.parasite_swarm.commit_ramp = f!();
    b.parasite_swarm.commit_decay = f!();
    b.parasite_swarm.commit_spread = f!();
    b.parasite_swarm.flash_impulse = f!();
    b.parasite_swarm.flash_sep_boost = f!();
    b.parasite_swarm.rouse_threat = f!();
    b.parasite_swarm.rouse_proximity = f!();
    b.parasite_swarm.rouse_contagion_r = f!();
    b.parasite_swarm.huddle_radius = f!();
    b.parasite_swarm.harborage_sep = f!();
    b.parasite_swarm.settle_speed = f!();
    b.parasite_swarm.embed_range = f!();
    // boss
    b.boss.max_speed = f!();
    b.boss.accel = f!();
    b.boss.sight_far = f!();
    b.boss.wander_interval = f!();
    b.boss.observe_dist = f!();
    b.boss.sep_strength = f!();
    b.boss.hit_memory = f!();
    // laser
    b.laser.fire_interval = f!();
    b.laser.base_spread = f!();
    b.laser.move_spread = f!();
    b.laser.front_arc_cos = f!();
    // squad_move
    b.squad_move.unit_speed = f!();
    b.squad_move.turn_speed = f!();
    b.squad_move.pack_radius = f!();
    b.squad_move.blob_radius = f!();
    // mycelia_coupling
    b.mycelia_coupling.graze_bite_rate = f!();
    b.mycelia_coupling.fruit_meat_rate = f!();
    b.mycelia_coupling.gaze_seen = f!();
    // perception
    b.perception.leash = f!();
    // ── expanded subset (35), in BOUNDS order ──
    // squad_move (9)
    b.squad_move.min_encumber = f!();
    b.squad_move.orca_radius = f!();
    b.squad_move.orca_time_horizon = f!();
    b.squad_move.orca_query_radius = f!();
    b.squad_move.arrive_radius = f!();
    b.squad_move.anchor_ease = f!();
    b.squad_move.study_range = f!();
    b.squad_move.heal_range = f!();
    b.squad_move.heal_rate = f!();
    // boss (5)
    b.boss.turn_cos = f!();
    b.boss.gaze_cos = f!();
    b.boss.look_amount = f!();
    b.boss.flee_speed = f!();
    b.boss.sep_radius = f!();
    // laser (4)
    b.laser.laser_speed = f!();
    b.laser.laser_life = f!();
    b.laser.dist_spread = f!();
    b.laser.dist_spread_range = f!();
    // crab (8)
    b.crab.jump_arc = f!();
    b.crab.jitter_freq = f!();
    b.crab.scout_speed_mul = f!();
    b.crab.eat_range = f!();
    b.crab.back_spread = f!();
    b.crab.carry_speed = f!();
    b.crab.deliver_range = f!();
    b.crab.grab_range = f!();
    // parasite_swarm (5)
    b.parasite_swarm.leap_arc = f!();
    b.parasite_swarm.blind_cos = f!();
    b.parasite_swarm.harborage_bias = f!();
    b.parasite_swarm.rouse_calm_seconds = f!();
    b.parasite_swarm.huddle_sep_strength = f!();
    // mycelia_coupling (3)
    b.mycelia_coupling.graze_reach = f!();
    b.mycelia_coupling.mat_dense_v = f!();
    b.mycelia_coupling.mat_meat_rate = f!();
    // perception (1)
    b.perception.squad_think_interval = f!();
    debug_assert_eq!(i, N, "decode read the wrong number of knobs");
    Ok(b)
}

/// The shipped behaviour config as a genome — the band origin for [`mutate`] and the search baseline. Reads
/// the origin out of the loaded `GameConfig` (one source of truth), like `level_genome::authored(base)`.
pub fn authored(base: &BehaviorTuning) -> BehaviorGenome {
    encode(base)
}

/// Perturb a behaviour genome: every knob gets a scale-relative Gaussian kick (scale from the **authored**
/// value so bands can't ratchet across generations), clamped to its hard [`BOUNDS`]. Children are feasible
/// by construction — `propose` is one `mutate`, no rejection loop. Reuses `genome::flat_mutate`.
pub fn mutate(
    parent: &BehaviorGenome,
    authored: &BehaviorGenome,
    sigma: f32,
    rng: &mut ChaCha8Rng,
) -> Result<BehaviorGenome, String> {
    if parent.0.len() != N {
        return Err(format!("behaviour genome has {} knobs, expected {N}", parent.0.len()));
    }
    if authored.0.len() != N {
        return Err(format!("authored behaviour genome has {} knobs, expected {N}", authored.0.len()));
    }
    Ok(BehaviorGenome(flat_mutate(&parent.0, &authored.0, &BOUNDS, sigma, rng)))
}

/// The genome-level feasibility gate: right length, every knob finite and within [`BOUNDS`], and the
/// decoded `BehaviorTuning` passes `behavior_tuning::validate_tuning`. `mutate` guarantees this by
/// construction; the check exists for genomes built any other way (e.g. loaded from a committed archive).
pub fn is_feasible(g: &BehaviorGenome, base: &BehaviorTuning) -> Result<(), String> {
    if g.0.len() != N {
        return Err(format!("behaviour genome has {} knobs, expected {N}", g.0.len()));
    }
    flat_range_check(&g.0, &BOUNDS, "behaviour")?;
    let decoded = decode(g, base)?;
    crate::behavior_tuning::validate_tuning(&decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::seeded;

    #[test]
    fn authored_round_trips_exactly() {
        // encode ∘ decode is the identity on the shipped config (the base overlays onto itself), so the
        // encode/decode field walks agree and the round trip is loss-free.
        let base = BehaviorTuning::default();
        let decoded = decode(&authored(&base), &base).expect("authored decodes");
        assert_eq!(decoded, base);
    }

    #[test]
    fn authored_has_n_knobs_and_sits_inside_bounds() {
        let base = BehaviorTuning::default();
        let g = authored(&base);
        assert_eq!(g.0.len(), N, "authored genome length");
        for (i, &x) in g.0.iter().enumerate() {
            let (lo, hi) = BOUNDS[i];
            assert!(
                (lo..=hi).contains(&x),
                "shipped knob {i} = {x} is not within its BOUNDS [{lo}, {hi}] — the search could never \
                 reach the shipped value, or the bounds are miscalibrated"
            );
        }
        is_feasible(&g, &base).expect("the shipped behaviour must be feasible");
    }

    #[test]
    fn mutation_stays_within_bounds_and_feasible() {
        let base = BehaviorTuning::default();
        let authored = authored(&base);
        let mut rng = seeded(0x5EED);
        for _ in 0..500 {
            let child = mutate(&authored, &authored, 0.3, &mut rng).expect("mutate");
            assert_eq!(child.0.len(), N);
            for (i, &x) in child.0.iter().enumerate() {
                let (lo, hi) = BOUNDS[i];
                assert!(x.is_finite(), "knob {i} became non-finite");
                assert!((lo..=hi).contains(&x), "knob {i} = {x} escaped BOUNDS [{lo}, {hi}]");
            }
            is_feasible(&child, &base).expect("a clamped child must be feasible by construction");
        }
    }

    #[test]
    fn a_mutation_actually_moves_something() {
        let base = BehaviorTuning::default();
        let authored = authored(&base);
        let mut rng = seeded(0xC0DE);
        let child = mutate(&authored, &authored, 0.3, &mut rng).expect("mutate");
        assert_ne!(child, authored, "mutation changed nothing");
    }

    #[test]
    fn decode_rejects_a_wrong_length_genome() {
        let base = BehaviorTuning::default();
        assert!(decode(&BehaviorGenome(vec![0.0; N - 1]), &base).is_err());
        assert!(is_feasible(&BehaviorGenome(vec![0.0; N + 1]), &base).is_err());
    }
}
