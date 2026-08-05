//! **Co-evolutionary MAP-Elites** (feature `test-harness`) — the offline search.
//!
//! Three co-evolving populations — squad, swarm, and **world** (the game's own config: field propagation +
//! sim tuning, see `world_genome`) — each illuminated by its own MAP-Elites archive (Mouret & Clune,
//! arXiv:1504.04909), each supplying the others' selection pressure — the mutual-pressure autocurriculum
//! of Baker et al. (arXiv:1909.07528, whose own setup is win-driven and treats intrinsic motivation as a
//! rival baseline). Nothing optimises "win"; all optimise
//! **witnessed learnable-surprise** (`squad_ai::surprise`) subject to a **relational minimal criterion** —
//! a candidate is admitted only if a real encounter happened against the (squad, swarm, world) it was
//! paired with. The no-win objective and minimal criterion here are POET / minimal-criterion coevolution
//! (Wang et al., POET, arXiv:1901.01753), not Baker's.
//!
//! Three design commitments, each earned from the literature:
//!
//! - **Opponents are sampled from across the archive, not from its incumbent.** Coevolving against only
//!   the current best is how you get Ficici & Pollack's *mediocre stable states* and cyclic forgetting.
//!   POET's transfer step and Bansal et al.'s opponent sampling both exist for this reason.
//!
//! - **Surprise is measured against a frozen prior, never against the current opponent.** If the
//!   reference drifted with the population, "surprising" would mean only "different from last
//!   generation", and the archive would chase its own tail. The prior is the shipped brain — what the
//!   *player* expects — and it never moves.
//!
//! - **The minimal criterion is a hard gate, not a penalty.** Skalse et al. (arXiv:2209.13085) show a
//!   hackable proxy stays hackable when you subtract a penalty from it; the remedy is to restrict the
//!   admissible set. An episode that fails any clause is discarded, never scored low.
//!
//! Everything is seeded (`crate::rng`), so a whole run is reproducible from one `u64`.
//!
//! # Non-stationary fitness — handled by common-opponent re-evaluation
//!
//! An elite's fitness is the mean of `W·S·L` over the opponents it was paired with, and fitness is **not** a
//! function of the genome alone: `W`, `L`, and even the descriptor all depend on the opponent (a squad's
//! `aggression` is the share of combat modes, which gate on whether the swarm showed up). So a naive
//! `incumbent.fitness >= challenger.fitness` elitism test would compare scores measured under *different*
//! conditions. Mouret & Clune's predictability argument rests on a stationary `f(genome)` (arXiv:1504.04909);
//! freezing the *prior* fixes the reference of `S` but not the rollout's opponent-dependence — and with three
//! co-adapting populations that non-stationarity is load-bearing, not a rounding error.
//!
//! The fix (POET's `EVALUATE_CANDIDATES`, arXiv:1901.01753): when a challenger contests a filled cell, the
//! **incumbent** is re-evaluated against the challenger's *exact* recorded opponents and seeds — a
//! common-opponent comparison — before the elitism test (`Population::try_insert_with_reeval`). It costs up
//! to `OPPONENTS` extra rollout pairs per *contested* cell (most proposals fill an empty niche, so amortized
//! cost is modest) and draws no fresh RNG, so a whole run stays reproducible from one `u64`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use rand_chacha::ChaCha8Rng;

use crate::ai::brain::{authored_brains, BrainSource, CandidateBrains};
use crate::ai::utility::{Behavior, Mode};
use crate::rng::{seeded, DetRng};
use crate::squad_ai::role::{RoleBrains, RoleId};

use crate::squad_ai::evaluate::rollout;
use crate::squad_ai::genome::{decode, encode, is_feasible, is_feasible_creature, mutate, Genome};
use crate::squad_ai::qd::{BehaviorDescriptor, MapElitesArchive};
use crate::squad_ai::surprise::{
    fitness, minimal_criterion, EpisodeOutcome, EpisodeTrace, ModePrior,
};
use crate::squad_ai::world_genome::{self, WorldGenome};

/// Mutation strength (fraction of each parameter's authored scale). Large enough to leave the authored
/// basin within a few generations, small enough that most children stay feasible.
const SIGMA: f32 = 0.25;
/// Probability that a child also transposes two behaviour ranks.
const RANK_SWAP_P: f64 = 0.15;
/// How many opponents a candidate is evaluated against, drawn from across the opponent archive.
const OPPONENTS: usize = 3;
/// Mutation strength for the world genome (fraction of each knob's authored scale). Gentler than the brain
/// `SIGMA`: the config knobs span a wide range of magnitudes, and a smaller kick keeps a child world near
/// its parent so the archive fills a smooth spread rather than scattering.
const WORLD_SIGMA: f32 = 0.15;

/// The authored repertoires: the fixed *structure* every genome lays values over, and the reference
/// behaviour the baseline prior is swept from. Serializable so the parallel evaluator can hand each worker
/// the *driver's* templates over the IPC handshake (see `parallel`), rather than the worker rebuilding
/// `authored()` — which would diverge from the inline path for any non-authored `t`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Templates {
    /// Role repertoires in `RoleId::ALL` order.
    pub roles: Vec<Vec<Behavior>>,
    pub crab: Vec<Behavior>,
    pub scout: Vec<Behavior>,
    pub smiley: Vec<Behavior>,
    pub bear: Vec<Behavior>,
    pub bear_copy: Vec<Behavior>,
}

impl Templates {
    /// The shipped brains. Note this reads `RoleBrains::defaults()` — the **code literals**, not any
    /// `roles.ron` overlay: the search must anchor its parameter bands and its prior to one fixed
    /// reference, or a hand-tuned override would silently move the origin of the whole search space.
    pub fn authored() -> Self {
        let roles = RoleBrains::defaults();
        let creatures = authored_brains();
        Templates {
            roles: RoleId::ALL.iter().map(|r| roles.get(*r).behaviors.clone()).collect(),
            crab: creatures.crab.behaviors,
            scout: creatures.scout.behaviors,
            smiley: creatures.smiley.behaviors,
            bear: creatures.bear.behaviors,
            bear_copy: creatures.bear_copy.behaviors,
        }
    }
}

/// One squad candidate: a genome per role, in `RoleId::ALL` order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SquadGenome(pub Vec<Genome>);

/// One swarm candidate: the three creature repertoires that co-adapt as a unit. They are carried
/// together because they share a world — a scout that marks prey is only meaningful beside crabs that
/// rally on the mark.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwarmGenome {
    pub crab: Genome,
    pub scout: Genome,
    pub smiley: Genome,
    /// SCP-1048, the benign original. Carried on the swarm side rather than the world side because it
    /// is a *repertoire*, not a dial — and because how eagerly the original builds co-adapts with how
    /// dangerous its copies are, which is the whole point of evolving the two together.
    pub bear: Genome,
    /// The shared A/B/C copy repertoire.
    pub bear_copy: Genome,
}

impl SquadGenome {
    pub fn authored(t: &Templates) -> Self {
        SquadGenome(t.roles.iter().map(|b| encode(b)).collect())
    }
}

impl SwarmGenome {
    pub fn authored(t: &Templates) -> Self {
        SwarmGenome {
            crab: encode(&t.crab),
            scout: encode(&t.scout),
            smiley: encode(&t.smiley),
            bear: encode(&t.bear),
            bear_copy: encode(&t.bear_copy),
        }
    }
}

/// Assemble the `BrainSource` a rollout runs. Decoding is fallible, and a decode failure here is a
/// programming error (a genome that does not fit its template), so it propagates rather than substituting
/// an authored brain — that would silently evaluate the wrong candidate.
pub fn brains_of(
    t: &Templates,
    squad: &SquadGenome,
    swarm: &SwarmGenome,
) -> Result<BrainSource, String> {
    if squad.0.len() != t.roles.len() {
        return Err(format!("squad genome has {} roles, expected {}", squad.0.len(), t.roles.len()));
    }
    let mut roles: HashMap<RoleId, Vec<Behavior>> = HashMap::new();
    for ((role, template), genome) in RoleId::ALL.iter().zip(&t.roles).zip(&squad.0) {
        roles.insert(*role, decode(template, genome)?);
    }
    Ok(BrainSource::Candidate(Box::new(CandidateBrains {
        roles,
        crab: decode(&t.crab, &swarm.crab)?,
        scout: decode(&t.scout, &swarm.scout)?,
        smiley: decode(&t.smiley, &swarm.smiley)?,
        bear: decode(&t.bear, &swarm.bear)?,
        bear_copy: decode(&t.bear_copy, &swarm.bear_copy)?,
    })))
}

/// Cheap, simulation-free screening of a squad candidate: would the shipped game load these brains?
pub fn squad_feasible(t: &Templates, squad: &SquadGenome) -> Result<(), String> {
    if squad.0.len() != t.roles.len() {
        return Err(format!("squad genome has {} roles, expected {}", squad.0.len(), t.roles.len()));
    }
    for ((role, template), genome) in RoleId::ALL.iter().zip(&t.roles).zip(&squad.0) {
        is_feasible(*role, template, genome)?;
    }
    Ok(())
}

/// The same, for a swarm candidate.
pub fn swarm_feasible(t: &Templates, swarm: &SwarmGenome) -> Result<(), String> {
    is_feasible_creature("crab_brain", &t.crab, &swarm.crab)?;
    is_feasible_creature("scout_brain", &t.scout, &swarm.scout)?;
    is_feasible_creature("smiley_brain", &t.smiley, &swarm.smiley)?;
    is_feasible_creature("bear_brain", &t.bear, &swarm.bear)?;
    is_feasible_creature("bear_copy_brain", &t.bear_copy, &swarm.bear_copy)?;
    Ok(())
}

/// Both sides of a pairing.
pub fn feasible(t: &Templates, squad: &SquadGenome, swarm: &SwarmGenome) -> Result<(), String> {
    squad_feasible(t, squad)?;
    swarm_feasible(t, swarm)
}

/// How many times a mutation may be redrawn before the search gives up on a parent.
///
/// A child is infeasible when it loses its unconditional default — e.g. `wander()`'s intercept
/// (`Linear { m: 0.0, b: 0.12 }`) drifts below `MIN_SCORE = 0.1`, only 0.02 away. That is the guard doing
/// its job. Screening costs *no simulation*, so bounded rejection sampling is the right
/// constraint-handling move: redraw until feasible.
///
/// **How often it trips, measured over 2000 draws (2026-08-05):** `squad` children are feasible
/// **0.902** of the time and `swarm` children **0.281** — so the swarm side is essentially the whole
/// rejection rate, and the previous claim here of "roughly half of all children" was wrong for both.
/// The two are drawn independently, so over 500 pairs none exhausted this budget and the worst side
/// needed 19 of 64 redraws (`P(exhaust) ≈ 0.72^64 ≈ 4e-10`).
/// `coevolve::tests::mutation_yields_feasible_children_often_enough_for_rejection_sampling` pins it.
///
/// Exhausting the budget is a loud error, never a silent skip. It means `SIGMA` is wrong for this parent,
/// and quietly evaluating the parent again (or an authored brain) would corrupt the archive with a
/// candidate nobody proposed.
const MAX_MUTATION_ATTEMPTS: u32 = 64;

/// Redraw a squad child until it is feasible.
fn propose_squad(
    t: &Templates,
    parent: &SquadGenome,
    rng: &mut ChaCha8Rng,
    rejected: &mut u32,
) -> Result<SquadGenome, String> {
    for _ in 0..MAX_MUTATION_ATTEMPTS {
        let child = mutate_squad(t, parent, rng)?;
        if squad_feasible(t, &child).is_ok() {
            return Ok(child);
        }
        *rejected += 1;
    }
    Err(format!(
        "no feasible squad child in {MAX_MUTATION_ATTEMPTS} draws at sigma {SIGMA}; the parent sits on \
         the feasibility boundary"
    ))
}

/// Public feasible squad mutation for the POET outer loop (`squad_ai::poet`) — redraw a child until it
/// passes `squad_feasible`, exactly as the co-evolution's internal [`propose_squad`] does. The rejection
/// count is discarded (POET keeps its own tallies).
pub fn mutate_squad_feasible(
    t: &Templates,
    parent: &SquadGenome,
    rng: &mut ChaCha8Rng,
) -> Result<SquadGenome, String> {
    let mut rejected = 0;
    propose_squad(t, parent, rng, &mut rejected)
}

/// Public feasible **swarm** mutation — the twin of [`mutate_squad_feasible`], and the seam
/// `replay::search_rollouts_of_MUTANTS_are_reproducible` needs.
///
/// It exists because the determinism guard must evaluate what the SEARCH evaluates. The older guard ran the
/// **authored** genome and was green while the search diverged: a mutant reaches code the authored config
/// never arms (a behaviour gated on a knob that ships clear of its threshold, a mode the shipped brains
/// never enter). Mutating the swarm is the half that moves crab behaviour, which is where every
/// order-dependence in this sim has so far lived.
pub fn mutate_swarm_feasible(
    t: &Templates,
    parent: &SwarmGenome,
    rng: &mut ChaCha8Rng,
) -> Result<SwarmGenome, String> {
    let mut rejected = 0;
    propose_swarm(t, parent, rng, &mut rejected)
}

/// Redraw a swarm child until it is feasible.
fn propose_swarm(
    t: &Templates,
    parent: &SwarmGenome,
    rng: &mut ChaCha8Rng,
    rejected: &mut u32,
) -> Result<SwarmGenome, String> {
    for _ in 0..MAX_MUTATION_ATTEMPTS {
        let child = mutate_swarm(t, parent, rng)?;
        if swarm_feasible(t, &child).is_ok() {
            return Ok(child);
        }
        *rejected += 1;
    }
    Err(format!(
        "no feasible swarm child in {MAX_MUTATION_ATTEMPTS} draws at sigma {SIGMA}; the parent sits on \
         the feasibility boundary"
    ))
}

// Glob re-exports so the split is invisible to the rest of the crate: every path that resolved against
// the old single-file `coevolve` module still resolves.
mod artifacts;
mod descriptors;
mod population;
mod search;
#[cfg(test)]
mod tests;

pub use artifacts::*;
pub use descriptors::*;
pub use population::*;
pub use search::*;
