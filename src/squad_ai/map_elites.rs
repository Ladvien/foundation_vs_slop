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
    }
    Ok(())
}
