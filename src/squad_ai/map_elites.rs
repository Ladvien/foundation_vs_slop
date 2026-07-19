//! The shared **single-population MAP-Elites (Quality-Diversity) loop** (Mouret & Clune 2015, MAP-Elites)
//! that both the level search ([`super::level_search`]) and the audio search ([`super::audio_search`])
//! drive: seed the archive with the authored genome, then `for generation { for batch { sample_parent →
//! mutate → feasibility-gate → evaluate → insert-or-reject } report }`. The two searches differ only in
//! their genome type and in *how* they mutate / feasibility-check / evaluate a candidate, so those parts
//! are supplied as closures and the propose → evaluate → archive skeleton lives here once.
//!
//! One path: an infeasible or criterion-failing child is counted (`rejected_infeasible` /
//! `rejected_by_criterion`) and dropped, never scored with a degraded fallback.

use rand_chacha::ChaCha8Rng;

use super::coevolve::Population;
use super::qd::BehaviorDescriptor;

/// The outcome of a single-population MAP-Elites search: the illuminated archive plus reject tallies.
/// The per-search result aliases (`LevelSearchResult`, `AudioSearchResult`) are this struct at their
/// genome type, so the searches share one shape and their `report` callbacks see the same fields.
pub struct MapElitesResult<G> {
    pub pop: Population<G>,
    pub evaluations: u32,
    pub rejected_infeasible: u32,
    pub rejected_by_criterion: u32,
}

/// Run the shared MAP-Elites loop into `result` (which must already hold an empty `Population` and zeroed
/// tallies). The caller supplies the authored seed genome plus the three varying steps as closures:
///
/// - `mutate(&parent, rng) -> Result<child, String>` — one proposal; the `rng` draw order here is
///   load-bearing (it is the *same* generator `sample_parent` just drew from, in that order).
/// - `is_feasible(&child) -> bool` — the cheap bounds gate before the (possibly expensive) evaluation.
/// - `evaluate(&genome) -> Option<(descriptor, fitness)>` — `None` is a minimal-criterion reject.
///
/// The authored genome is evaluated first to seed the archive; if it fails the criterion the loop returns
/// `Err(seed_criterion_err)`. `report(generation, &result)` fires after every generation. Behaviour is
/// identical to the two hand-written loops it replaces: same RNG draw order, same seed → mutate →
/// feasibility-reject → evaluate → insert sequence, same tally bookkeeping.
pub(crate) fn map_elites_loop<G, FM, FF, FE, FR>(
    rng: &mut ChaCha8Rng,
    result: &mut MapElitesResult<G>,
    authored_g: &G,
    generations: u32,
    batch: u32,
    patience: u32,
    seed_criterion_err: &str,
    mut mutate: FM,
    is_feasible: FF,
    mut evaluate: FE,
    mut report: FR,
) -> Result<(), String>
where
    G: Clone,
    FM: FnMut(&G, &mut ChaCha8Rng) -> Result<G, String>,
    FF: Fn(&G) -> bool,
    FE: FnMut(&G) -> Option<(BehaviorDescriptor, f32)>,
    FR: FnMut(u32, &MapElitesResult<G>),
{
    // Seed the archive with the authored config so `sample_parent` has somewhere to start.
    match evaluate(authored_g) {
        Some((d, fitness)) => {
            result.pop.insert(d, fitness, authored_g.clone());
            result.evaluations += 1;
        }
        None => return Err(seed_criterion_err.to_string()),
    }

    // Convergence early-stop on QD-score plateau (Mouret & Clune 2015 archive-property termination). A no-op
    // when `patience == 0`. Deterministic: stopping the outer loop leaves the gen-K archive byte-identical.
    let mut plateau = super::qd::PlateauStop::new(patience);
    for generation in 0..generations {
        for _ in 0..batch {
            let parent = result
                .pop
                .sample_parent(rng)
                .cloned()
                .unwrap_or_else(|| authored_g.clone());
            let child = mutate(&parent, rng)?;
            // Cheap feasibility gate before the evaluation.
            if !is_feasible(&child) {
                result.rejected_infeasible += 1;
                continue;
            }
            match evaluate(&child) {
                Some((d, fitness)) => {
                    result.pop.insert(d, fitness, child);
                    result.evaluations += 1;
                }
                None => result.rejected_by_criterion += 1,
            }
        }
        report(generation, result);
        if plateau.should_stop(result.pop.archive.qd_score()) {
            break;
        }
    }
    Ok(())
}

/// Rank separation added to a *non-improving* candidate's CMA fitness so an archive-improving candidate
/// always sorts above it, regardless of the raw fitness scale (see [`map_elites_cma_loop`]). Chosen larger
/// than any single-generation fitness spread the searches produce (witnessed-surprise ∈ `[0,1]`; the
/// synthetic sphere test spans ~`[-50, 0]`), so the two improvement classes never interleave.
const IMPROVE_SEP: f32 = 1.0e6;

/// A **CMA-ME** improvement-emitter MAP-Elites loop (Fontaine et al. 2020, "Covariance Matrix Adaptation for
/// the Rapid Illumination of Behavior Space"). Instead of sampling an archive parent and applying an
/// isotropic kick ([`map_elites_loop`]), a [`super::cmaes::SepCmaEs`] emitter proposes each batch, is told an
/// *improvement* ranking — a candidate that adds a new cell ranks above one that only improved a cell, above
/// the rest — and adapts its distribution toward regions that illuminate the archive. On stagnation (a
/// generation adds nothing new, or the step size collapses) the emitter restarts from a random elite, so the
/// search keeps discovering rather than polishing one basin.
///
/// Operates on a flat `Vec<f32>` genome via `to_vec` / `from_vec` (the latter must clamp into the feasible
/// box), so it reuses the same feasibility + evaluate closures as [`map_elites_loop`]. Every search leaves it
/// **default-off**, so the committed archives (built with the isotropic path) stay bit-reproducible.
#[allow(clippy::too_many_arguments)]
pub(crate) fn map_elites_cma_loop<G, FF, FE, TV, FV, FR>(
    rng: &mut ChaCha8Rng,
    result: &mut MapElitesResult<G>,
    authored_g: &G,
    generations: u32,
    batch: u32,
    patience: u32,
    sigma: f32,
    seed_criterion_err: &str,
    to_vec: TV,
    from_vec: FV,
    is_feasible: FF,
    mut evaluate: FE,
    mut report: FR,
) -> Result<(), String>
where
    G: Clone,
    TV: Fn(&G) -> Vec<f32>,
    FV: Fn(&[f32]) -> G,
    FF: Fn(&G) -> bool,
    FE: FnMut(&G) -> Option<(BehaviorDescriptor, f32)>,
    FR: FnMut(u32, &MapElitesResult<G>),
{
    // Seed the archive with the authored genome (identical to the isotropic loop).
    match evaluate(authored_g) {
        Some((d, fitness)) => {
            result.pop.insert(d, fitness, authored_g.clone());
            result.evaluations += 1;
        }
        None => return Err(seed_criterion_err.to_string()),
    }

    let mut emitter = super::cmaes::SepCmaEs::new(to_vec(authored_g), sigma, batch as usize);

    // QD-score plateau early-stop (no-op when `patience == 0`), independent of the emitter's own restart rule.
    let mut plateau = super::qd::PlateauStop::new(patience);
    for generation in 0..generations {
        let mut told: Vec<(super::cmaes::Sample, f32)> = Vec::new();
        let mut added_new = false;
        for _ in 0..emitter.lambda() {
            let sample = emitter.ask(rng);
            let child = from_vec(&sample.x);
            // A rejected child is DROPPED, never told to the emitter — the same invariant the isotropic loop
            // holds (a criterion/infeasibility reject must not steer future proposals with a sentinel).
            if !is_feasible(&child) {
                result.rejected_infeasible += 1;
                continue;
            }
            // Re-derive the sample from the CLAMPED genome actually evaluated (`from_vec` box-clamps), so the
            // CMA update is driven by the in-bounds point, not the raw proposal — this is what stops the mean
            // marching past the box bound.
            let repaired = emitter.repair(&to_vec(&child));
            match evaluate(&child) {
                Some((d, fitness)) => {
                    let inserted = result.pop.insert(d, fitness, child);
                    result.evaluations += 1;
                    added_new |= inserted;
                    // Improvement ranking: an archive-improving candidate ranks STRICTLY above one that did
                    // not (the `IMPROVE_SEP` offset dominates any single-generation fitness spread), then by
                    // fitness within each class — CMA-ME's improvement emitter.
                    let rank = if inserted { fitness } else { fitness - IMPROVE_SEP };
                    told.push((repaired, rank));
                }
                None => result.rejected_by_criterion += 1,
            }
        }
        emitter.tell(told);

        // Restart rule: if the generation illuminated nothing new, or the step size collapsed, re-centre on a
        // random elite so the emitter explores a fresh region instead of polishing a filled basin.
        if !added_new || emitter.sigma() < 1e-4 {
            let restart_from =
                result.pop.sample_parent(rng).cloned().unwrap_or_else(|| authored_g.clone());
            emitter = super::cmaes::SepCmaEs::new(to_vec(&restart_from), sigma, batch as usize);
        }

        report(generation, result);
        if plateau.should_stop(result.pop.archive.qd_score()) {
            break;
        }
    }
    Ok(())
}

/// A **CMA-MAE** loop (Fontaine & Nikolaidis 2023, "Covariance Matrix Adaptation MAP-Annealing",
/// DOI 10.1145/3583131.3590389) — the SOTA upgrade to [`map_elites_cma_loop`]. CMA-ME ranks a proposal by a
/// *binary* improved/not-improved flag, which fails on flat or deceptive objectives (nothing "improves", so
/// the emitter gets no gradient) and abandons the objective too readily. CMA-MAE gives every cell a **soft
/// annealing threshold** `t_e` (init `min_f`): a proposal's rank is the *continuous* improvement `f − t_e`,
/// and on acceptance the threshold anneals `t_e ← t_e + α·(f − t_e)`. `α = 0` recovers a pure optimiser
/// (thresholds never rise), `α = 1` recovers CMA-ME-like hard elitism; intermediate `α` smoothly trades
/// optimisation for exploration and is robust to the failure modes above.
///
/// The output archive (`result.pop`) still keeps the best genome per cell for the readable elite handoff;
/// the thresholds are a *separate* side-structure driving only the emitter's ranking. Additive and
/// default-off, so the committed archives (isotropic / CMA-ME) stay bit-reproducible.
// Staged CMA-MAE (Fontaine & Nikolaidis 2023) emitter — implemented and unit-tested, but not yet wired
// into `rl_search`'s emitter selection (that still picks isotropic vs CMA-ME on `--cma`). Kept ready for
// the next increment; allow dead_code until it's selected.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn map_elites_cma_mae_loop<G, FF, FE, TV, FV, FR>(
    rng: &mut ChaCha8Rng,
    result: &mut MapElitesResult<G>,
    authored_g: &G,
    generations: u32,
    batch: u32,
    patience: u32,
    sigma: f32,
    alpha: f32,
    min_f: f32,
    seed_criterion_err: &str,
    to_vec: TV,
    from_vec: FV,
    is_feasible: FF,
    mut evaluate: FE,
    mut report: FR,
) -> Result<(), String>
where
    G: Clone,
    TV: Fn(&G) -> Vec<f32>,
    FV: Fn(&[f32]) -> G,
    FF: Fn(&G) -> bool,
    FE: FnMut(&G) -> Option<(BehaviorDescriptor, f32)>,
    FR: FnMut(u32, &MapElitesResult<G>),
{
    use std::collections::BTreeMap;

    let res = result.pop.archive.resolution();
    // Per-cell annealing thresholds (BTreeMap for deterministic iteration, matching the archive).
    let mut thresholds: BTreeMap<(usize, usize), f32> = BTreeMap::new();

    // Seed the archive + the seeded cell's threshold with the authored genome.
    match evaluate(authored_g) {
        Some((d, fitness)) => {
            result.pop.insert(d, fitness, authored_g.clone());
            thresholds.insert(d.cell(res), min_f + alpha * (fitness - min_f));
            result.evaluations += 1;
        }
        None => return Err(seed_criterion_err.to_string()),
    }

    let mut emitter = super::cmaes::SepCmaEs::new(to_vec(authored_g), sigma, batch as usize);

    // QD-score plateau early-stop (no-op when `patience == 0`), independent of the emitter's own restart rule.
    let mut plateau = super::qd::PlateauStop::new(patience);
    for generation in 0..generations {
        let mut told: Vec<(super::cmaes::Sample, f32)> = Vec::new();
        let mut any_improved = false;
        for _ in 0..emitter.lambda() {
            let sample = emitter.ask(rng);
            let child = from_vec(&sample.x);
            if !is_feasible(&child) {
                result.rejected_infeasible += 1;
                continue;
            }
            let repaired = emitter.repair(&to_vec(&child));
            match evaluate(&child) {
                Some((d, fitness)) => {
                    let cell = d.cell(res);
                    let t = thresholds.get(&cell).copied().unwrap_or(min_f);
                    let improvement = fitness - t;
                    // The output archive keeps best-per-cell regardless (the readable elite).
                    result.pop.insert(d, fitness, child);
                    // Soft acceptance: beat the annealing threshold → anneal it up and count it as illumination.
                    if improvement > 0.0 {
                        thresholds.insert(cell, t + alpha * improvement);
                        any_improved = true;
                    }
                    // CMA-MAE ranks EVERY proposal by its continuous improvement (even negative), so the
                    // emitter gets a gradient on flat/deceptive objectives where CMA-ME's binary flag is stuck.
                    result.evaluations += 1;
                    told.push((repaired, improvement));
                }
                None => result.rejected_by_criterion += 1,
            }
        }
        emitter.tell(told);

        // Restart when a generation cleared no threshold (nothing new illuminated) or the step size collapsed.
        if !any_improved || emitter.sigma() < 1e-4 {
            let restart_from =
                result.pop.sample_parent(rng).cloned().unwrap_or_else(|| authored_g.clone());
            emitter = super::cmaes::SepCmaEs::new(to_vec(&restart_from), sigma, batch as usize);
        }

        report(generation, result);
        if plateau.should_stop(result.pop.archive.qd_score()) {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::seeded;

    /// Squash a real to `[0,1]` for a synthetic descriptor axis.
    fn unit(x: f32) -> f32 {
        1.0 / (1.0 + (-x).exp())
    }

    #[test]
    fn cma_loop_illuminates_the_archive() {
        // A synthetic QD problem over a 4-D `Vec<f32>` genome: descriptor = (unit(x0), unit(x1)), fitness =
        // -||x - target||². The CMA-ME loop must fill several archive cells (illumination), not collapse to
        // one — the whole point of a QD emitter over a plain optimiser.
        let target = [0.8f32, -0.6, 0.2, -0.3];
        let mut result = MapElitesResult { pop: Population::new(8), evaluations: 0, rejected_infeasible: 0, rejected_by_criterion: 0 };
        let authored = vec![0.0f32; 4];
        let mut rng = seeded(0xC0A5_E11E);
        map_elites_cma_loop(
            &mut rng,
            &mut result,
            &authored,
            30,
            12,
            0, // patience: 0 = no early-stop (test the full run)
            0.6,
            "seed failed",
            |g: &Vec<f32>| g.clone(),
            |v: &[f32]| v.to_vec(),
            |_g| true,
            |g: &Vec<f32>| {
                let fitness = -g.iter().zip(target).map(|(a, b)| (a - b) * (a - b)).sum::<f32>();
                Some((BehaviorDescriptor::new(unit(g[0]), unit(g[1])), fitness))
            },
            |_gen, _r| {},
        )
        .expect("cma loop");
        assert!(result.pop.archive.coverage() >= 3, "CMA-ME must illuminate several cells, got {}", result.pop.archive.coverage());
        assert!(result.evaluations > 30, "it must have evaluated a full run, got {}", result.evaluations);
    }

    #[test]
    fn cma_mae_illuminates_the_archive() {
        // The same synthetic QD problem the CMA-ME test uses: CMA-MAE must also fill several cells.
        let target = [0.8f32, -0.6, 0.2, -0.3];
        let mut result = MapElitesResult { pop: Population::new(8), evaluations: 0, rejected_infeasible: 0, rejected_by_criterion: 0 };
        let authored = vec![0.0f32; 4];
        let mut rng = seeded(0xC0A5_E11E);
        map_elites_cma_mae_loop(
            &mut rng, &mut result, &authored, 30, 12, 0, 0.6, 0.5, -100.0, "seed failed",
            |g: &Vec<f32>| g.clone(),
            |v: &[f32]| v.to_vec(),
            |_g| true,
            |g: &Vec<f32>| {
                let fitness = -g.iter().zip(target).map(|(a, b)| (a - b) * (a - b)).sum::<f32>();
                Some((BehaviorDescriptor::new(unit(g[0]), unit(g[1])), fitness))
            },
            |_gen, _r| {},
        )
        .expect("cma-mae loop");
        assert!(result.pop.archive.coverage() >= 3, "CMA-MAE must illuminate several cells, got {}", result.pop.archive.coverage());
        assert!(result.evaluations > 30);
    }

    #[test]
    fn cma_mae_still_illuminates_a_flat_objective() {
        // The failure mode CMA-MAE exists for: a FLAT objective (constant fitness). CMA-ME's binary
        // improved/not flag gives the emitter almost no gradient once cells are filled; CMA-MAE's annealing
        // threshold keeps `f - t` informative, so it must still spread across the descriptor space.
        let authored = vec![0.0f32; 4];
        let mut result = MapElitesResult { pop: Population::new(8), evaluations: 0, rejected_infeasible: 0, rejected_by_criterion: 0 };
        let mut rng = seeded(0xF1A7_C0DE);
        map_elites_cma_mae_loop(
            &mut rng, &mut result, &authored, 40, 12, 0, 0.8, 0.5, 0.0, "seed",
            |g: &Vec<f32>| g.clone(),
            |v: &[f32]| v.to_vec(),
            |_g| true,
            |g: &Vec<f32>| Some((BehaviorDescriptor::new(unit(g[0]), unit(g[1])), 1.0f32)),
            |_gen, _r| {},
        )
        .expect("cma-mae flat");
        assert!(
            result.pop.archive.coverage() >= 3,
            "CMA-MAE must keep illuminating on a flat objective, got {}",
            result.pop.archive.coverage()
        );
    }
}
