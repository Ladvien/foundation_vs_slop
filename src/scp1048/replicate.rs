//! Replication — the benign original assembling hostile copies of itself out of scavenged material.
//!
//! This is the mechanic the whole module exists for. The original cannot be shot (it carries no
//! `Hostile`), so the player's only lever on the hostile population is **observation**: the brain gates
//! `Mode::Build` on *not* being seen, so a squad that keeps eyes on the bear stops it building at all.
//! Amplify-where-you-are, no central plan, is the shape of stigmergic construction (Khuong et al.,
//! "Stigmergic construction and topochemical information shape ant nest architecture", PNAS 2016,
//! doi:10.1073/pnas.1509829113).
//!
//! Both halves the design calls for are here: a **material economy** (`scavenge_rate` toward
//! `build_cost`) and a **cooldown** (`build_cooldown`), with `max_bears` as the firm cap — exactly the
//! role `parasite.manca_count_max` plays for the burst→brood→infest loop. All four are genome knobs, so
//! the QD search can evolve how fast the bear breeds and how hard the ceiling bites.

use bevy::prelude::*;

use super::{
    anim::Scp1048Anim, Scp1048, Scp1048Build, Scp1048Seed, Scp1048SpawnSeq, Scp1048Variant,
};
use crate::ai::brain::ActiveBehavior;
use crate::ai::utility::Mode;
use crate::dungeon::Dungeon;
use crate::sim::SimTuning;
use crate::util::hash01_u32;

/// Salt mixed into the per-build variant draw, so two builds by one parent do not come out identical.
const VARIANT_SALT: u32 = 0x9E37_79B1;

/// How far a fresh copy is offset from its parent's cell centre, in world units. Small — the copy is
/// assembled *by* the bear, so it should appear beside it, not across the room.
const SPAWN_JITTER: f32 = 0.35;

/// Which copy gets built, as a pure function of the parent's immortal seed and how many it has already
/// made.
///
/// **Never a shared RNG.** A draw from a `Res`-held generator advanced in query order would make the
/// variant depend on ECS iteration order — the exact class of bug the determinism rules exist for.
/// Salting with `builds_done` is what stops one parent producing an endless run of the same copy.
///
/// The three weights are `[w_a, w_b, max(0, 1 - w_a - w_b)]`. For any `w_a, w_b` in `[0,1]` that triple
/// sums to **at least 1**: if `w_a + w_b <= 1` it is exactly 1, and otherwise it is `w_a + w_b > 1`. So
/// the draw can never divide by zero and `world_genome::decode` needs no clamp on these two knobs — a
/// deliberate contrast with `brood_max.max(brood_min)`, which does.
pub(crate) fn copy_variant(seed: u32, builds_done: u32, w_a: f32, w_b: f32) -> Scp1048Variant {
    let w_a = w_a.clamp(0.0, 1.0);
    let w_b = w_b.clamp(0.0, 1.0);
    let w_c = (1.0 - w_a - w_b).max(0.0);
    let total = w_a + w_b + w_c; // provably >= 1, so never zero
    let r = hash01_u32(seed ^ builds_done.wrapping_mul(VARIANT_SALT)) * total;
    if r < w_a {
        Scp1048Variant::EarCopy
    } else if r < w_a + w_b {
        Scp1048Variant::InfantArm
    } else {
        Scp1048Variant::Scrap
    }
}

/// Accrue scavenged material while the original is building, and run its cooldown down.
///
/// **Determinism.** Purely per-entity: each bear reads and writes only its own [`Scp1048Build`]. No
/// shared state, no ordering decision, so no sort.
pub(crate) fn scp1048_scavenge(
    time: Res<Time>,
    sim: Res<SimTuning>,
    mut bears: Query<(&ActiveBehavior, &mut Scp1048Build)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let t = &sim.scp1048;
    for (active, mut build) in &mut bears {
        build.cooldown = (build.cooldown - dt).max(0.0);
        if active.mode == Mode::Build {
            build.materials = (build.materials + t.scavenge_rate * dt).min(t.build_cost);
        }
    }
}

/// Assemble a copy for every original that is ready, up to the live-population cap.
///
/// **Determinism — the critical site in this module.** The visit order is *part of the result*: bears
/// draw from the shared monotonic [`Scp1048SpawnSeq`], and the `max_bears` cap is claimed first-come.
/// So the iteration must be a **total** order, which is what `sort_total!` enforces (and panics on a
/// tie under `debug_assertions`/`test-harness`, naming the site).
///
/// Position bits alone are **not** total — `parasite_burst` documents hosts sitting at bit-identical
/// coordinates as a routine occurrence. [`Scp1048Seed`] is the tiebreak precisely because it is never
/// position-derived: a copy is built in its parent's own cell, so a position-derived key could not
/// break a position tie. That is the `GibKey` trap named in the determinism rules — a tiebreak derived
/// from the very quantity it exists to disambiguate.
///
/// The child's decorrelation seed likewise comes from the monotonic counter, never from an `Entity` id
/// (recycled, and not reproducible across same-seed runs) and never from the spawn position (siblings
/// share a cell, so a position hash would clone them).
pub(crate) fn scp1048_replicate(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    assets: Res<AssetServer>,
    bear_anim: Res<Scp1048Anim>,
    mut seq: ResMut<Scp1048SpawnSeq>,
    all_bears: Query<(), With<Scp1048>>,
    mut builders: Query<(&Scp1048, &Scp1048Seed, &Transform, &mut Scp1048Build)>,
) {
    let t = &sim.scp1048;
    let mut live = all_bears.iter().count();
    if live >= t.max_bears {
        return;
    }

    // Snapshot the ready builders, then order them totally before any of them draws a seed.
    let mut order: Vec<((u32, u32, u32), u32, Vec3)> = builders
        .iter()
        .filter(|(bear, _, _, build)| {
            bear.variant == Scp1048Variant::Original && build.ready(t.build_cost)
        })
        .map(|(_, seed, tf, _)| {
            let p = tf.translation;
            ((p.x.to_bits(), p.y.to_bits(), p.z.to_bits()), seed.0, p)
        })
        .collect();
    if order.is_empty() {
        return;
    }
    crate::sort_total!(&mut order, |&(k, seed, _)| (k, seed));

    // Which parents actually built, so their economy can be debited in a second pass.
    let mut built: Vec<u32> = Vec::new();
    for (_, parent_seed, pos) in order {
        if live >= t.max_bears {
            break;
        }
        // `builds_done` is read from the parent below; look it up by its (unique) seed.
        let done = builders
            .iter()
            .find(|(_, s, _, _)| s.0 == parent_seed)
            .map_or(0, |(_, _, _, b)| b.builds_done);
        let child_seed = seq.0;
        seq.0 = seq.0.wrapping_add(1);
        let variant = copy_variant(parent_seed, done, t.copy_w_a, t.copy_w_b);
        // Deterministic offset from the parent's cell centre — the `MANCA_SPAWN_JITTER` idiom. Derived
        // from the child's own seed, so siblings do not stack.
        let cell = dungeon.world_to_cell(pos);
        let centre = dungeon.cell_center(cell);
        let a = hash01_u32(child_seed ^ 0x5BD1_E995) * std::f32::consts::TAU;
        let spawn = centre + Vec3::new(a.cos(), 0.0, a.sin()) * SPAWN_JITTER;
        super::spawn_scp1048_at(
            &mut commands,
            &assets,
            &bear_anim,
            &sim,
            child_seed,
            spawn,
            variant,
        );
        built.push(parent_seed);
        live += 1;
    }

    // Debit only the parents that actually built (a bear blocked by the cap keeps its material).
    for (_, seed, _, mut build) in &mut builders {
        if built.contains(&seed.0) {
            build.materials = 0.0;
            build.cooldown = t.build_cooldown;
            build.builds_done = build.builds_done.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_variant_is_deterministic() {
        for seed in 0u32..500 {
            let a = copy_variant(seed, 0, 0.34, 0.33);
            assert_eq!(a, copy_variant(seed, 0, 0.34, 0.33), "the draw must be reproducible");
        }
    }

    #[test]
    fn every_copy_is_reachable_at_the_shipped_weights() {
        let t = SimTuning::default().scp1048;
        let seen: std::collections::HashSet<Scp1048Variant> =
            (0u32..3000).map(|s| copy_variant(s, 0, t.copy_w_a, t.copy_w_b)).collect();
        assert_eq!(seen.len(), 3, "all three copies should be buildable, got {seen:?}");
    }

    #[test]
    fn consecutive_builds_by_one_parent_are_not_all_the_same() {
        // The `builds_done` salt exists so a single bear does not emit an endless run of one variant.
        let t = SimTuning::default().scp1048;
        let seen: std::collections::HashSet<Scp1048Variant> =
            (0u32..60).map(|n| copy_variant(7, n, t.copy_w_a, t.copy_w_b)).collect();
        assert!(seen.len() > 1, "one parent's successive builds must vary");
    }

    #[test]
    fn degenerate_weights_behave_and_never_divide_by_zero() {
        // Both zero ⇒ C takes the whole remainder.
        for seed in 0u32..200 {
            assert_eq!(copy_variant(seed, 0, 0.0, 0.0), Scp1048Variant::Scrap);
        }
        // Both one ⇒ the triple sums to 2 and C is unreachable; A and B split it.
        let seen: std::collections::HashSet<Scp1048Variant> =
            (0u32..600).map(|s| copy_variant(s, 0, 1.0, 1.0)).collect();
        assert!(!seen.contains(&Scp1048Variant::Scrap), "w_a=w_b=1 leaves no room for C");
        assert_eq!(seen.len(), 2, "A and B should share it");
    }

    #[test]
    fn the_weight_triple_always_sums_to_at_least_one() {
        // The property that makes the clamp-free decode sound. Checked across the whole bounds square.
        for i in 0..=20 {
            for j in 0..=20 {
                let (w_a, w_b) = (i as f32 / 20.0, j as f32 / 20.0);
                let total = w_a + w_b + (1.0 - w_a - w_b).max(0.0);
                assert!(total >= 1.0 - 1e-6, "w_a={w_a} w_b={w_b} summed to {total}");
            }
        }
    }
}
