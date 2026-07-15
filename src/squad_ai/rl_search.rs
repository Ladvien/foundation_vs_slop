//! The **policy (neuroevolution) search**: a single-population MAP-Elites loop that evolves a
//! [`PolicyGenome`] — the weights of a learned [`NeuralPolicy`] squad controller — under the
//! witnessed-learnable-surprise objective (`super::rl_eval`).
//!
//! Structurally identical to [`super::behavior_search`] — the shared propose → evaluate → archive loop
//! ([`map_elites_loop`], Mouret & Clune 2015) over [`super::coevolve::Population`] — but the thing evolved
//! is the *decision layer itself*, not a config dial. This is the concrete RL learner: Evolution Strategies
//! over policy weights (Salimans et al. 2017, arXiv:1703.03864), evaluated by the same headless rollout and
//! scored by the same fitness as every other population, so it reuses the whole engine. Not co-evolutionary
//! on its own: the swarm/world/acoustics are the authored baseline and only the squad policy moves; wiring
//! it into the three-way co-evolution is a later step.

use serde::Serialize;

use crate::rng::seeded;

use super::coevolve::{ArchiveDoc, Population};
use super::map_elites::{map_elites_cma_loop, map_elites_loop, MapElitesResult};
use super::policy_genome::{self, authored, mutate, PolicyGenome};
use super::qd::BehaviorDescriptor;
use super::rl_eval;
use super::surprise::ModePrior;

/// Knobs for a policy search. `dungeon_seeds` are the worlds each genome is scored across (the learnability
/// pair uses the first two, which must differ); `resolution` is the MAP-Elites grid side over the squad
/// descriptor (aggression × exploration).
#[derive(Clone, Debug)]
pub struct RlSearchConfig {
    pub seed: u64,
    pub generations: u32,
    pub batch: u32,
    pub sigma: f32,
    pub resolution: usize,
    pub dungeon_seeds: Vec<u64>,
    pub episode_ticks: u32,
    /// Use the CMA-ME adaptive emitter (`map_elites_cma_loop`) instead of the isotropic-Gaussian mutation.
    /// **Default `false`** so the isotropic path — and any archive committed from it — stays bit-reproducible;
    /// opt in with `train rl --cma`.
    pub use_cma: bool,
}

impl Default for RlSearchConfig {
    fn default() -> Self {
        RlSearchConfig {
            seed: 0x9EA5_0_5EED,
            generations: 40,
            batch: 32,
            sigma: 0.3,
            resolution: 8,
            dungeon_seeds: vec![0x5C09191, 0x1CE5, 0xB0BA],
            episode_ticks: 1800,
            use_cma: false,
        }
    }
}

/// The outcome of a policy search: the illuminated archive plus reject tallies. Aliases the shared
/// [`MapElitesResult`] at the policy genome.
pub type RlSearchResult = MapElitesResult<PolicyGenome>;

/// Run the policy (neuroevolution) search. `report(generation, &result)` is called after each generation;
/// `search` itself writes nothing to disk (the `train.rs` driver does). One path: an infeasible or
/// criterion-failing child is counted and dropped, never scored with a degraded fallback.
pub fn search(
    prior: &ModePrior,
    cfg: &RlSearchConfig,
    mut report: impl FnMut(u32, &RlSearchResult),
) -> Result<RlSearchResult, String> {
    if cfg.dungeon_seeds.len() < 2 {
        return Err(
            "policy search needs >= 2 dungeon seeds: the learnability pair must run on DIFFERENT worlds, \
             or fitness measures a memorised map rather than a behaviour"
                .into(),
        );
    }
    let mut rng = seeded(cfg.seed);
    // The seed policy — a fixed pseudo-random small-weight net — is the band origin and the archive-seeding
    // candidate. It must itself be feasible; a loud failure here means the genome layout is broken.
    let authored_g = authored();
    policy_genome::is_feasible(&authored_g)
        .map_err(|e| format!("the seed policy is infeasible: {e}"))?;

    let mut result = RlSearchResult {
        pop: Population::new(cfg.resolution),
        evaluations: 0,
        rejected_infeasible: 0,
        rejected_by_criterion: 0,
    };

    let seed_err = "the seed policy failed the minimal criterion on the held-in seeds — a random-weight net \
                    may never choose role work; widen the seed distribution or lengthen the episode";
    let is_feasible = |child: &PolicyGenome| policy_genome::is_feasible(child).is_ok();
    let evaluate = |g: &PolicyGenome| {
        rl_eval::evaluate(g, prior, &cfg.dungeon_seeds, cfg.episode_ticks)
            .map(|ev| (BehaviorDescriptor::new(ev.axes.0, ev.axes.1), ev.fitness))
    };
    // Same seed / feasibility / evaluation either way — only the *proposal* operator differs. (The closures
    // are moved into exactly one branch; the other never runs.)
    if cfg.use_cma {
        map_elites_cma_loop(
            &mut rng,
            &mut result,
            &authored_g,
            cfg.generations,
            cfg.batch,
            cfg.sigma,
            seed_err,
            |g: &PolicyGenome| g.0.clone(),
            policy_genome::from_vec_clamped,
            is_feasible,
            evaluate,
            &mut report,
        )?;
    } else {
        map_elites_loop(
            &mut rng,
            &mut result,
            &authored_g,
            cfg.generations,
            cfg.batch,
            seed_err,
            |parent, rng| mutate(parent, &authored_g, cfg.sigma, rng),
            is_feasible,
            evaluate,
            &mut report,
        )?;
    }
    Ok(result)
}

/// One archived policy, packed for the committed `elites_policy.ron`. Unlike the config genomes this is an
/// **opaque weight vector**, not a readable diff — so the readable-elite reward-hacking guard (Skalse et
/// al., arXiv:2209.13085) does not apply; the guard for a learned policy is the minimal criterion plus
/// watching it play. The weights are kept so the runtime can rebuild the exact [`NeuralPolicy`].
#[derive(Serialize)]
pub struct RlEliteDoc {
    pub cell: (usize, usize),
    /// Descriptor axis 1 — squad aggression.
    pub aggression: f32,
    /// Descriptor axis 2 — squad exploration.
    pub exploration: f32,
    pub fitness: f32,
    /// The flat MLP weight vector — feed to `NeuralPolicy::from_weights` (via `policy_genome::decode`).
    pub weights: Vec<f32>,
}

/// Build the serializable archive document — every elite carrying its weight vector, so the runtime can
/// reconstruct the learned controller and the search can be resumed/inspected.
pub fn rl_archive_doc(pop: &Population<PolicyGenome>) -> Result<ArchiveDoc<RlEliteDoc>, String> {
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let g = pop.get(elite.genome).ok_or("dangling elite handle")?;
        elites.push(RlEliteDoc {
            cell: *cell,
            aggression: elite.descriptor.aggression,
            exploration: elite.descriptor.exploration,
            fitness: elite.fitness,
            weights: g.0.clone(),
        });
    }
    Ok(ArchiveDoc {
        resolution: pop.archive.resolution(),
        coverage: pop.archive.coverage(),
        qd_score: pop.archive.qd_score(),
        elites,
    })
}
