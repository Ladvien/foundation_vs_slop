//! The **behaviour search**: a single-population MAP-Elites (Quality-Diversity) loop that evolves the
//! [`BehaviorGenome`] under the witnessed-learnable-surprise objective (`super::behavior_eval`).
//!
//! Structurally identical to [`super::audio_search`] — the level search's plain propose → evaluate →
//! archive loop (Mouret & Clune 2015, MAP-Elites), reusing [`super::coevolve::Population`] + [`ArchiveDoc`]
//! — but its fitness is a *behavioural* full-simulation rollout, so it takes the frozen baseline `prior`
//! rather than being self-contained. It is NOT co-evolutionary: the brains, world, and acoustics are the
//! authored baseline, and only the `behavior:` slice moves, so there is no opponent sampling and no
//! non-stationarity — one config knob set, illuminated across the swarm-behaviour descriptor.
//!
//! Feasible-by-construction mutation means most children generate; the loud reject paths
//! (`rejected_infeasible` / `rejected_by_criterion`) count what the bounds and the minimal criterion turn
//! away, so a run that fills nothing is visible rather than silent.

use serde::Serialize;

use crate::behavior_tuning::BehaviorTuning;
use crate::rng::seeded;

use super::behavior_eval;
use super::behavior_genome::{self, authored, mutate, BehaviorGenome};
use super::coevolve::{ArchiveDoc, Population};
use super::map_elites::{map_elites_loop, MapElitesResult};
use super::qd::BehaviorDescriptor;
use super::surprise::ModePrior;

/// Knobs for a behaviour search. `dungeon_seeds` are the worlds each genome is scored across (the
/// learnability pair uses the first two, which must differ); `resolution` is the MAP-Elites grid side over
/// the swarm-behaviour descriptor (aggression × persistence).
#[derive(Clone, Debug)]
pub struct BehaviorSearchConfig {
    pub seed: u64,
    pub generations: u32,
    pub batch: u32,
    pub sigma: f32,
    pub resolution: usize,
    pub dungeon_seeds: Vec<u64>,
    pub episode_ticks: u32,
}

impl Default for BehaviorSearchConfig {
    fn default() -> Self {
        BehaviorSearchConfig {
            seed: 0xBE4A_010_5EED,
            generations: 40,
            batch: 32,
            sigma: 0.3,
            resolution: 8,
            dungeon_seeds: crate::squad_ai::coevolve::HELD_IN_SEEDS.to_vec(),
            episode_ticks: 1800,
        }
    }
}

/// The outcome of a behaviour search: the illuminated archive plus reject tallies. Aliases the shared
/// [`MapElitesResult`] at the behaviour genome, so all the rollout searches share one result shape.
pub type BehaviorSearchResult = MapElitesResult<BehaviorGenome>;

/// Run the behaviour search. `report(generation, &result)` is called after each generation; `search` itself
/// writes nothing to disk (the `train.rs` driver does). One path: an infeasible or criterion-failing child
/// is counted and dropped, never scored with a degraded fallback.
pub fn search(
    prior: &ModePrior,
    cfg: &BehaviorSearchConfig,
    mut report: impl FnMut(u32, &BehaviorSearchResult),
) -> Result<BehaviorSearchResult, String> {
    if cfg.dungeon_seeds.len() < 2 {
        return Err(
            "behaviour search needs >= 2 dungeon seeds: the learnability pair must run on DIFFERENT \
             worlds, or fitness measures a memorised map rather than a behaviour"
                .into(),
        );
    }
    let mut rng = seeded(cfg.seed);
    // The shipped base the genome's searched subset overlays onto (== the `behavior:` slice of config.ron).
    let base = BehaviorTuning::default();
    let authored_g = authored(&base);
    // The shipped behaviour config must itself be feasible — a loud failure here means the base is broken.
    behavior_genome::is_feasible(&authored_g, &base)
        .map_err(|e| format!("the shipped behaviour config is infeasible: {e}"))?;

    let mut result = BehaviorSearchResult {
        pop: Population::new(cfg.resolution),
        evaluations: 0,
        rejected_infeasible: 0,
        rejected_by_criterion: 0,
    };

    // The propose → evaluate → archive loop is shared with the audio/level searches; only mutate /
    // feasibility / evaluate vary, supplied here as closures. The criterion gate is the (expensive)
    // full-sim behavioural rollout.
    map_elites_loop(
        &mut rng,
        &mut result,
        &authored_g,
        cfg.generations,
        cfg.batch,
        "the shipped behaviour config failed the minimal criterion on the held-in seeds",
        |parent, rng| mutate(parent, &authored_g, cfg.sigma, rng),
        |child| behavior_genome::is_feasible(child, &base).is_ok(),
        |g| {
            behavior_eval::evaluate(g, prior, &cfg.dungeon_seeds, cfg.episode_ticks)
                .map(|ev| (BehaviorDescriptor::new(ev.axes.0, ev.axes.1), ev.fitness))
        },
        &mut report,
    )?;
    Ok(result)
}

/// One archived behaviour config, decoded to the exact `behavior:` RON a designer authors by hand — so an
/// elite is a readable diff of behaviour dials, the reward-hacking guard (Skalse et al., arXiv:2209.13085).
/// Copy the slice into `config.ron` to ship it.
#[derive(Serialize)]
pub struct BehaviorEliteDoc {
    pub cell: (usize, usize),
    /// Descriptor axis 1 — swarm aggression.
    pub aggression: f32,
    /// Descriptor axis 2 — swarm persistence.
    pub persistence: f32,
    pub fitness: f32,
    pub behavior: BehaviorTuning,
}

/// Build the serializable archive document — every elite decoded back to a readable `BehaviorTuning` slice
/// (overlaid onto the shipped base).
pub fn behavior_archive_doc(
    pop: &Population<BehaviorGenome>,
) -> Result<ArchiveDoc<BehaviorEliteDoc>, String> {
    let base = BehaviorTuning::default();
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let g = pop.get(elite.genome).ok_or("dangling elite handle")?;
        let behavior = behavior_genome::decode(g, &base)?;
        elites.push(BehaviorEliteDoc {
            cell: *cell,
            aggression: elite.descriptor.aggression,
            persistence: elite.descriptor.exploration,
            fitness: elite.fitness,
            behavior,
        });
    }
    Ok(ArchiveDoc {
        resolution: pop.archive.resolution(),
        coverage: pop.archive.coverage(),
        qd_score: pop.archive.qd_score(),
        elites,
    })
}
