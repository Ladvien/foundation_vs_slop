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
use super::map_elites::{map_elites_cma_loop, map_elites_cma_mae_loop, map_elites_loop, MapElitesResult};
use super::policy_genome::{self, authored, mutate, PolicyGenome};
use super::qd::BehaviorDescriptor;
use super::rl_eval;
use super::surprise::ModePrior;

/// Knobs for a policy search. `dungeon_seeds` are the worlds each genome is scored across (the learnability
/// pair uses the first two, which must differ); `resolution` is the MAP-Elites grid side over the policy
/// descriptor (initiative × caretaking; see `rl_eval::policy_descriptor`).
#[derive(Clone, Debug)]
pub struct RlSearchConfig {
    pub seed: u64,
    pub generations: u32,
    pub batch: u32,
    pub sigma: f32,
    pub resolution: usize,
    pub dungeon_seeds: Vec<u64>,
    pub episode_ticks: u32,
    /// Convergence early-stop patience (generations without QD-score gain); `0` disables. See
    /// [`crate::squad_ai::qd::PlateauStop`].
    pub patience: u32,
    /// Which proposal operator generates children. See [`Emitter`].
    pub emitter: Emitter,
    /// CMA-MAE's archive-annealing rate, `0.0..=1.0`. Ignored by the other two emitters.
    ///
    /// `0.0` makes every cell's threshold its own current elite — i.e. CMA-ME's "improve the cell you
    /// landed in". `1.0` freezes thresholds at the seed fitness. Fontaine & Nikolaidis report the
    /// interesting behaviour in between, where a cell's bar rises *toward* its elite rather than
    /// snapping to it, so an emitter is rewarded for a run of near-misses instead of only for outright
    /// improvements — which is the whole reason CMA-MAE beats CMA-ME on deceptive landscapes.
    pub cma_mae_alpha: f32,
}

/// The proposal operator a policy search uses to generate children.
///
/// **FVS-H-2.** CMA-MAE (`map_elites::map_elites_cma_mae_loop`, Fontaine & Nikolaidis 2023) was
/// implemented and unit-tested but `pub(crate)` and referenced only by its own two tests — dead code
/// reachable from nothing. A boolean cannot name three emitters, which is why it stayed dead: there was
/// no place to put it. This enum is that place.
///
/// **The status quo is CMA-ME, not the isotropic emitter** — `train rl --cma` has been used in anger
/// (the 2026-07-23 island run) and is what `train all` already passes for the `rl` phase. So this widens
/// the choice rather than fixing a weakness.
///
/// ⚠️ **Scope, measured:** `rl_search` is the **only** consumer of any CMA emitter. `levels`, `audio`,
/// `behavior` and `evolve3` all use the isotropic `map_elites_loop`, so this improves the **policy**
/// archive alone unless that wiring is widened later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Emitter {
    /// Isotropic-Gaussian mutation of an archive parent. Bit-reproducible and the conservative default,
    /// so an archive committed from it does not depend on an adaptive covariance.
    #[default]
    Isotropic,
    /// CMA-ME (`map_elites_cma_loop`): one adaptive covariance, restarted on stagnation.
    CmaMe,
    /// CMA-MAE (`map_elites_cma_mae_loop`): CMA-ME plus per-cell **annealed** acceptance thresholds.
    CmaMae,
}

impl Emitter {
    /// What `train`'s banner prints.
    pub fn label(self) -> &'static str {
        match self {
            Emitter::Isotropic => "neuroevolution, isotropic",
            Emitter::CmaMe => "CMA-ME emitter",
            Emitter::CmaMae => "CMA-MAE emitter (annealed archive)",
        }
    }
}

impl Default for RlSearchConfig {
    fn default() -> Self {
        RlSearchConfig {
            seed: 0x9EA5_0_5EED,
            generations: 40,
            batch: 32,
            sigma: 0.3,
            resolution: 8,
            dungeon_seeds: crate::squad_ai::coevolve::HELD_IN_SEEDS.to_vec(),
            // Measured minimal-criterion floor (see `audio_search::AudioSearchConfig::default` and
            // `tests/search_calibration.rs`); below it feasible episodes are rejected and the archive stays empty.
            episode_ticks: 7200,
            patience: 0,
            emitter: Emitter::Isotropic,
            // Fontaine & Nikolaidis's reported default. Inert unless `emitter` is `CmaMae`.
            cma_mae_alpha: 0.1,
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
    match cfg.emitter {
        Emitter::CmaMe => map_elites_cma_loop(
            &mut rng,
            &mut result,
            &authored_g,
            cfg.generations,
            cfg.batch,
            cfg.patience,
            cfg.sigma,
            seed_err,
            |g: &PolicyGenome| g.0.clone(),
            policy_genome::from_vec_clamped,
            is_feasible,
            evaluate,
            &mut report,
        )?,
        Emitter::CmaMae => map_elites_cma_mae_loop(
            &mut rng,
            &mut result,
            &authored_g,
            cfg.generations,
            cfg.batch,
            cfg.patience,
            cfg.sigma,
            cfg.cma_mae_alpha,
            // The annealing floor `min_f`. `0.0` because policy fitness is `W·S·L`, a product of three
            // non-negative factors — so zero is the true infimum rather than a chosen constant, and an
            // empty cell's threshold starts where "nothing archived here yet" actually is. A negative
            // floor (as the unit tests use, for a landscape that goes below zero) would let a cell
            // accept a worse-than-nothing elite.
            0.0,
            seed_err,
            |g: &PolicyGenome| g.0.clone(),
            policy_genome::from_vec_clamped,
            is_feasible,
            evaluate,
            &mut report,
        )?,
        Emitter::Isotropic => map_elites_loop(
            &mut rng,
            &mut result,
            &authored_g,
            cfg.generations,
            cfg.batch,
            cfg.patience,
            seed_err,
            |parent, rng| mutate(parent, &authored_g, cfg.sigma, rng),
            is_feasible,
            evaluate,
            &mut report,
        )?,
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
    /// Descriptor axis 1 — **initiative** (non-`FollowAnchor` share); see `rl_eval::policy_descriptor`.
    pub initiative: f32,
    /// Descriptor axis 2 — **caretaking** (self-care/defence share of the squad's non-follow decisions).
    pub caretaking: f32,
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
            // BehaviorDescriptor's two generic slots carry (initiative, caretaking) for the policy archive.
            initiative: elite.descriptor.aggression,
            caretaking: elite.descriptor.exploration,
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
