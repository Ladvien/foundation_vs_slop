//! The **level evaluator**: decode a [`LevelGenome`], run the real generation pipeline, and score it.
//!
//! Deliberately *not* a headless rollout. The three artefacts the level-quality objective needs —
//! the walkability mask, the placed furniture, and the mould habitat mask — are all produced by pure,
//! GPU-free functions the game already ships:
//!   1. `Dungeon::generate` (architecture),
//!   2. `placement::furnish::furnish_all` (furniture amount),
//!   3. `mycelia::habitat::build` (mushroom amount — the CPU habitat mask, no GPU field sim).
//! So a level is scored by *generating and measuring* it, with no Bevy `App`, no physics, and no GPU.
//! That is what makes evolving all three targets affordable and deterministic (Smith & Whitehead 2010's
//! expressive-range analysis is a static measurement; we make it the search's fitness).
//!
//! Every genome is evaluated across a **held-in seed set** and must clear the minimal criterion on *all*
//! of them: a level config that only produces a connected, playable dungeon on one seed is not robust,
//! so it is rejected. Fitness and the descriptor axes are averaged over the seeds. One path — a genome
//! that fails generation, a validator, or the criterion on any seed returns `None`, never a degraded score.

use crate::dungeon::Dungeon;
use crate::placement::manifest::FurnitureManifest;

use super::level_genome::{self, LevelBase, LevelGenome};
use super::level_quality;

/// The result of scoring one genome: its mean fitness and the mean descriptor axes (openness ×
/// infestation) used to place it in the MAP-Elites archive.
#[derive(Clone, Copy, Debug)]
pub struct LevelEvaluation {
    pub fitness: f32,
    pub axes: (f32, f32),
}

/// Decode and score a level genome across `seeds`. Returns `None` (a rejection, not a fallback) if the
/// genome is infeasible, generation fails to converge, or the level fails the minimal criterion on any
/// seed. On success, fitness and axes are the mean over the seed set.
pub fn evaluate(
    genome: &LevelGenome,
    base: &LevelBase,
    manifest: &FurnitureManifest,
    seeds: &[u64],
) -> Option<LevelEvaluation> {
    if seeds.is_empty() {
        return None;
    }
    let pheno = level_genome::decode(genome, base).ok()?;

    let mut sum_fit = 0.0f32;
    let mut sum_open = 0.0f32;
    let mut sum_infest = 0.0f32;
    for &seed in seeds {
        let mut dungeon_cfg = pheno.dungeon.clone();
        dungeon_cfg.seed = seed;
        // Generation can fail to converge (WFC restart budget) — that is an `Err`, handled as a rejection.
        let dungeon = Dungeon::generate(&dungeon_cfg).ok()?;
        let furniture = crate::placement::furnish::furnish_all(
            &dungeon,
            manifest,
            pheno.metropolis.clone(),
            &pheno.density,
        );
        // The CPU habitat mask (no GPU). Errors if the dungeon isn't 192² — the genome guarantees it is.
        let habitat = crate::mycelia::habitat::build(&dungeon, &pheno.mycelia).ok()?;

        let metrics = level_quality::measure(&dungeon, &furniture, &habitat);
        // Minimal criterion: any seed that fails rejects the whole genome (robustness across maps).
        let fitness = metrics.score()?;
        let (open, infest) = metrics.descriptor_axes();
        sum_fit += fitness;
        sum_open += open;
        sum_infest += infest;
    }

    let n = seeds.len() as f32;
    Some(LevelEvaluation {
        fitness: sum_fit / n,
        axes: (sum_open / n, sum_infest / n),
    })
}

/// A level's **playtest** score (feature `test-harness`): how *engaging* it is to play, not just how well
/// structured it is. This is the PCGRL move — a level scored by simulated play rather than static metrics
/// (Khalifa et al. 2020, DOI 10.1609/aiide.v16i1.7416; the experience-driven objective, Yannakakis &
/// Togelius 2011, DOI 10.1109/t-affc.2011.6).
#[cfg(feature = "test-harness")]
pub struct LevelPlaytestEvaluation {
    /// Mean engagement (experience + interest proxies) over the admitted seeds.
    pub fitness: f32,
    /// The static expressive-range descriptor axes (openness × infestation) — the MAP-Elites niche.
    pub axes: (f32, f32),
}

/// Score a level by **how it plays**: a cheap static pre-filter (reject structurally broken levels before
/// paying for a rollout — Khalifa's feasible/infeasible split), then a real headless rollout per held-in
/// seed scored by the Phase-1 experience + interest proxies, gated by the behavioural minimal criterion on
/// every seed. `None` (a rejection, not a fallback) if the genome is infeasible, fails static structure, or
/// produces no real encounter on any seed. Fitness is the mean engagement; the descriptor axes are the
/// static openness × infestation.
#[cfg(feature = "test-harness")]
pub fn evaluate_playtest(
    genome: &LevelGenome,
    base: &LevelBase,
    manifest: &FurnitureManifest,
    seeds: &[u64],
    ticks: u32,
) -> Option<LevelPlaytestEvaluation> {
    use super::evaluate::rollout_level;
    use super::experience::Experience;
    use super::interest::Interest;
    use super::surprise::minimal_criterion;

    // 1. Cheap static pre-filter (+ the expressive-range descriptor axes): a level that isn't connected /
    //    playable is rejected here without a rollout.
    let static_eval = evaluate(genome, base, manifest, seeds)?;
    let pheno = level_genome::decode(genome, base).ok()?;

    // 2. Playtest: run the real sim on the evolved level, on each held-in seed, and measure how engaging the
    //    play is (experience + interest from the survival-belief series), gated by the behavioural criterion.
    let mut sum_fit = 0.0f32;
    for &seed in seeds {
        let r = rollout_level(pheno.clone(), seed, ticks);
        minimal_criterion(&r.outcome).ok()?;
        let engagement =
            0.5 * (Experience::from_belief(&r.belief).score() + Interest::from_belief(&r.belief).score());
        sum_fit += engagement;
    }
    Some(LevelPlaytestEvaluation { fitness: sum_fit / seeds.len() as f32, axes: static_eval.axes })
}

/// Load the shipped config slices as a [`LevelBase`] — the mutation origin and decode base — plus the
/// furniture manifest the evaluator furnishes with. One path: a missing/malformed config is a loud `Err`.
pub fn load_base() -> Result<(LevelBase, FurnitureManifest), String> {
    let cfg = crate::config::load_game_config()?;
    let base = LevelBase {
        dungeon: cfg.dungeon,
        metropolis: cfg.placement.metropolis,
        density: cfg.placement.density,
        mycelia: cfg.mycelia,
    };
    Ok((base, cfg.placement.furniture))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::squad_ai::level_genome::{authored, mutate};

    #[test]
    fn shipped_level_scores_and_is_reproducible() {
        // The authored level, generated and measured on two seeds, clears the criterion and scores in
        // (0,1] — and the same genome+seeds evaluate identically (deterministic, GPU-free).
        let (base, manifest) = load_base().expect("shipped config");
        let g = authored(&base);
        let seeds = [0x5C09191u64, 0xA11CE];
        let a = evaluate(&g, &base, &manifest, &seeds).expect("shipped level passes the criterion");
        assert!(a.fitness > 0.0 && a.fitness <= 1.0, "fitness in (0,1], got {}", a.fitness);
        assert!((0.0..=1.0).contains(&a.axes.0) && (0.0..=1.0).contains(&a.axes.1));
        // The shipped config infests ~15% of the floor, so the mushroom (infestation) axis must be > 0 —
        // guards the habitat-mask resolution bug (the mask is 1024², not dungeon-cell resolution).
        assert!(a.axes.1 > 0.0, "shipped level must read as having mushrooms, got infestation {}", a.axes.1);
        // And it places furniture, so the clutter axis must be > 0 too.
        assert!(a.axes.0 > 0.0, "shipped level must read as having furniture, got clutter {}", a.axes.0);
        let b = evaluate(&g, &base, &manifest, &seeds).expect("again");
        assert_eq!(a.fitness, b.fitness, "evaluation must be deterministic");
        assert_eq!(a.axes, b.axes);
    }

    /// The playtest path (feature `test-harness`): the shipped level, run through the real sim, scores in
    /// `[0,1]` and is deterministic. Guards the `SimConfig::with_level` seam + `evaluate_playtest`.
    #[cfg(feature = "test-harness")]
    #[test]
    fn shipped_level_playtests_and_is_deterministic() {
        use super::evaluate_playtest;
        // Do NOT hold `serial_guard()` here — `evaluate_playtest`'s rollouts acquire it internally per App,
        // and the guard's mutex is non-reentrant, so holding it here would deadlock.
        let (base, manifest) = load_base().expect("shipped config");
        let g = authored(&base);
        let seeds = [0x5C09191u64];
        let a = evaluate_playtest(&g, &base, &manifest, &seeds, 1800).expect("shipped level plays");
        assert!((0.0..=1.0).contains(&a.fitness), "engagement fitness in [0,1], got {}", a.fitness);
        let b = evaluate_playtest(&g, &base, &manifest, &seeds, 1800).expect("again");
        assert_eq!(a.fitness, b.fitness, "playtest scoring must be deterministic");
        assert_eq!(a.axes, b.axes);
    }

    #[test]
    fn a_mutated_genome_evaluates_or_cleanly_rejects() {
        // Mutation never crashes the evaluator: each child either scores (Some) or is cleanly rejected
        // (None) — never a panic, never a bogus score. Exercises the generate→furnish→habitat path.
        let (base, manifest) = load_base().expect("shipped config");
        let mut rng = crate::rng::seeded(0x5EED_1EA5);
        let seeds = [0x5C09191u64, 0xA11CE];
        let mut scored = 0;
        for _ in 0..12 {
            let child = mutate(&authored(&base), 0.4, &mut rng);
            if let Some(e) = evaluate(&child, &base, &manifest, &seeds) {
                assert!(e.fitness >= 0.0 && e.fitness <= 1.0);
                scored += 1;
            }
        }
        assert!(scored > 0, "at least some mutated levels should be scorable");
    }
}
