//! **Co-evolutionary MAP-Elites** (feature `test-harness`) — the offline search.
//!
//! Three co-evolving populations — squad, swarm, and **world** (the game's own config: field propagation +
//! sim tuning, see `world_genome`) — each illuminated by its own MAP-Elites archive (Mouret & Clune,
//! arXiv:1504.04909), each supplying the others' selection pressure. Nothing optimises "win"; all optimise
//! **witnessed learnable-surprise** (`squad_ai::surprise`) subject to a **relational minimal criterion** —
//! a candidate is admitted only if a real encounter happened against the (squad, swarm, world) it was
//! paired with (Wang et al., POET, arXiv:1901.01753; Baker et al. autocurricula, arXiv:1909.07528;
//! minimal-criterion coevolution).
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

use super::evaluate::rollout;
use super::genome::{decode, encode, is_feasible, is_feasible_creature, mutate, Genome};
use super::qd::{BehaviorDescriptor, MapElitesArchive};
use super::surprise::{
    fitness, minimal_criterion, ActorKind, EpisodeOutcome, EpisodeTrace, FearBucket, Fitness, ModePrior,
};
use super::world_genome::{self, WorldGenome};

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
/// behaviour the baseline prior is swept from.
#[derive(Clone)]
pub struct Templates {
    /// Role repertoires in `RoleId::ALL` order.
    pub roles: Vec<Vec<Behavior>>,
    pub crab: Vec<Behavior>,
    pub scout: Vec<Behavior>,
    pub smiley: Vec<Behavior>,
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
}

impl SquadGenome {
    pub fn authored(t: &Templates) -> Self {
        SquadGenome(t.roles.iter().map(|b| encode(b)).collect())
    }
}

impl SwarmGenome {
    pub fn authored(t: &Templates) -> Self {
        SwarmGenome { crab: encode(&t.crab), scout: encode(&t.scout), smiley: encode(&t.smiley) }
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
/// its job, and roughly half of all children trip it at `SIGMA = 0.25`. Screening costs *no simulation*,
/// so bounded rejection sampling is the right constraint-handling move: redraw until feasible.
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

// ── Behaviour descriptors ────────────────────────────────────────────────────────────────────────

/// Modes that read, to a watching player, as *pressing the fight*.
fn is_squad_combat(mode: Mode) -> bool {
    // One definition, shared with the minimal criterion's agency clause.
    super::surprise::is_squad_offensive(mode)
}

/// Modes that read as the swarm *committing* rather than milling or fleeing.
fn is_swarm_aggression(mode: Mode) -> bool {
    matches!(mode, Mode::Latch | Mode::Rally | Mode::Muster | Mode::Chase)
}

/// Both descriptor axes must be things a *player perceives*, because the archive's whole job is to hold
/// visibly different playstyles apart. `aggression` is the share of decisions that press the fight;
/// `exploration` is how much of the reachable map the squad actually walked.
pub fn squad_descriptor(trace: &EpisodeTrace, outcome: &EpisodeOutcome) -> BehaviorDescriptor {
    let unit_decisions: Vec<Mode> = trace
        .decisions
        .iter()
        .filter(|d| matches!(d.context.actor, super::surprise::ActorKind::Role(_)))
        .map(|d| d.mode)
        .collect();
    let aggression = share(&unit_decisions, is_squad_combat);
    let exploration = if outcome.reachable_cells > 0 {
        outcome.cells_covered as f32 / outcome.reachable_cells as f32
    } else {
        0.0
    };
    BehaviorDescriptor::new(aggression, exploration)
}

/// The swarm's axes: how much it commits, and how much it holds together rather than routing. Both are
/// read straight off the decision trace, so no extra instrumentation is needed.
///
/// The second axis is **persistence** — the complement of the flee share. A swarm that scatters at the
/// first shot occupies a different niche from one that presses through fire on the ALARM pheromone. It
/// reuses `BehaviorDescriptor::exploration` as a generic second coordinate; the archive only ever needs
/// two numbers in `[0,1]`, and giving the swarm its own type would duplicate the grid for no gain.
pub fn swarm_descriptor(trace: &EpisodeTrace) -> BehaviorDescriptor {
    let creature_decisions: Vec<Mode> = trace
        .decisions
        .iter()
        .filter(|d| !matches!(d.context.actor, super::surprise::ActorKind::Role(_)))
        .map(|d| d.mode)
        .collect();
    let aggression = share(&creature_decisions, is_swarm_aggression);
    let persistence = 1.0 - share(&creature_decisions, |m| m == Mode::Flee);
    BehaviorDescriptor::new(aggression, persistence)
}

/// The **world's** axes, both player-perceptible: how much DREAD it produces (mean unit fear over the
/// episode) × how FEROCIOUS a fight (the swarm's aggression share). Read off the trace, no new
/// instrumentation. Tune the axes after inspecting the first world archive.
pub fn world_descriptor(trace: &EpisodeTrace) -> BehaviorDescriptor {
    BehaviorDescriptor::new(mean_unit_fear(trace), swarm_descriptor(trace).aggression)
}

/// Mean unit fear over the episode, from the coarse `FearBucket` on each squad (`Role`) decision
/// (Calm = 0, Wary = 0.5, Panicked = 1). Value-sorted `mean` for order-independence.
fn mean_unit_fear(trace: &EpisodeTrace) -> f32 {
    let fears: Vec<f32> = trace
        .decisions
        .iter()
        .filter(|d| matches!(d.context.actor, ActorKind::Role(_)))
        .map(|d| match d.context.fear {
            FearBucket::Calm => 0.0,
            FearBucket::Wary => 0.5,
            FearBucket::Panicked => 1.0,
        })
        .collect();
    if fears.is_empty() {
        0.0
    } else {
        mean(&fears)
    }
}

fn share(modes: &[Mode], pred: impl Fn(Mode) -> bool) -> f32 {
    if modes.is_empty() {
        return 0.0;
    }
    modes.iter().filter(|m| pred(**m)).count() as f32 / modes.len() as f32
}

// ── Populations ──────────────────────────────────────────────────────────────────────────────────

/// A MAP-Elites archive plus the genomes its elites refer to. `qd::Elite` stores an opaque `u64` handle;
/// here that handle is an index into `store`, so the archive math stays exactly as unit-tested.
pub struct Population<G> {
    pub archive: MapElitesArchive,
    store: Vec<G>,
}

impl<G: Clone> Population<G> {
    pub fn new(resolution: usize) -> Self {
        Population { archive: MapElitesArchive::new(resolution), store: Vec::new() }
    }

    /// Insert if this genome fills an empty niche or beats the incumbent. Returns whether it landed.
    ///
    /// The genome is stored **only on acceptance**. A rejected candidate's handle was never written into
    /// any `Elite`, so keeping its slot would leak one genome per proposal — over 98% of `store` on a real
    /// run. Accepted handles are still never recycled, so an incumbent's handle stays valid when it is
    /// later displaced.
    pub fn insert(&mut self, descriptor: BehaviorDescriptor, fitness: f32, genome: G) -> bool {
        let handle = self.store.len() as u64;
        if self.archive.insert(descriptor, fitness, handle) {
            self.store.push(genome);
            true
        } else {
            false
        }
    }

    /// Insert a challenger, resolving a contested cell by a **common-opponent** comparison instead of the
    /// archive's stored fitness — the fix for MAP-Elites' stationary-fitness assumption (Mouret & Clune
    /// rest their argument on a stationary `f(genome)`; ours is not — `W`, `L`, and the descriptor all
    /// depend on the opponents a candidate drew). Comparing a challenger's fitness to an incumbent's scored
    /// against *different* opponents is apples-to-oranges, so when the cell is occupied `reeval_incumbent`
    /// re-scores the incumbent against the challenger's **exact** opponents and seeds (POET's
    /// `EVALUATE_CANDIDATES`, arXiv:1901.01753); the challenger wins unless the incumbent still scores `>=`
    /// it under those identical conditions. On a hold the incumbent's stored fitness is refreshed to the
    /// fresh common-opponent value, so the cell stays comparable going forward.
    ///
    /// `reeval_incumbent` returns `None` when the incumbent produces no real encounter on any of the
    /// challenger's conditions (inadmissible there) — then the challenger, which did, wins. It consumes no
    /// fresh RNG (it replays recorded seeds), so the whole run stays reproducible from `cfg.seed`.
    pub fn try_insert_with_reeval(
        &mut self,
        descriptor: BehaviorDescriptor,
        challenger_fitness: f32,
        challenger: G,
        reeval_incumbent: impl FnOnce(&G) -> Result<Option<f32>, String>,
    ) -> Result<bool, String> {
        match self.archive.incumbent(descriptor) {
            None => {
                let handle = self.store.len() as u64;
                self.store.push(challenger);
                self.archive.place(descriptor, challenger_fitness, handle);
                Ok(true)
            }
            Some(inc) => {
                let incumbent_genome = self.get(inc.genome).ok_or("dangling elite handle")?.clone();
                match reeval_incumbent(&incumbent_genome)? {
                    // Incumbent holds under the common opponents; refresh its fitness to the fresh value.
                    Some(s) if s >= challenger_fitness => {
                        self.archive.place(inc.descriptor, s, inc.genome);
                        Ok(false)
                    }
                    // Incumbent inadmissible under these conditions (`None`) or worse: the challenger wins.
                    _ => {
                        let handle = self.store.len() as u64;
                        self.store.push(challenger);
                        self.archive.place(descriptor, challenger_fitness, handle);
                        Ok(true)
                    }
                }
            }
        }
    }

    pub fn get(&self, handle: u64) -> Option<&G> {
        self.store.get(handle as usize)
    }

    /// Uniform draw from the occupied niches — the MAP-Elites selection rule. Uniform over *cells*, not
    /// over fitness: that is what keeps the search expanding into empty regions of the behaviour space
    /// rather than piling onto the current best.
    pub fn sample_parent(&self, rng: &mut ChaCha8Rng) -> Option<&G> {
        if self.archive.is_empty() {
            return None;
        }
        let n = self.archive.iter().count();
        let pick = rng.below(n);
        let handle = self.archive.iter().nth(pick).map(|(_, e)| e.genome)?;
        self.get(handle)
    }

    /// `k` opponents drawn (with replacement) from across the archive. Sampling the whole archive rather
    /// than its incumbent is the anti-cycling rule; with replacement so a sparse archive still yields `k`.
    pub fn sample_opponents(&self, k: usize, rng: &mut ChaCha8Rng) -> Vec<&G> {
        (0..k).filter_map(|_| self.sample_parent(rng)).collect()
    }
}

// ── The search ───────────────────────────────────────────────────────────────────────────────────

/// Everything one run needs. `episode_ticks` at 60 Hz: 7200 ≈ 120 s of simulated time.
///
/// 120 s is a **floor**, not a preference, and it was measured (`train probe`) rather than chosen. The
/// evaluation alternates 5 s of player-ordered advance with 5 s of AI-controlled engagement (see
/// `evaluate`), so only half the episode moves the squad toward the nests. At 60 s under that schedule the
/// authored squad takes 0–1 damage on two of three worlds and fails the criterion's "nothing was at stake"
/// clause — the shipped game's own brains. At 120 s it takes 2–15 damage, records 162–373 duty decisions,
/// and passes on all three worlds. A shorter episode does not make the search cheaper; it makes it empty.
pub struct SearchConfig {
    pub seed: u64,
    pub generations: u32,
    /// Children proposed per side per generation.
    pub batch: u32,
    pub episode_ticks: u32,
    /// Held-in dungeon seeds. Each candidate's two rollouts draw two *different* seeds from this set, so
    /// learnability measures behaviour that generalises across worlds rather than a memorised map.
    pub dungeon_seeds: Vec<u64>,
    pub resolution: usize,
    /// How many worker **processes** evaluate rollouts in parallel. `1` (the default) runs every rollout
    /// inline in this process — the reference path. `N > 1` spawns `N` `train worker` subprocesses and
    /// fans the `OPPONENTS` independent triples of each candidate across them. Parallelism must be across
    /// *processes*, never threads: `sim_harness` holds a process-wide lock and pins the compute pool to one
    /// thread for determinism (see `evaluate` module doc). Because `score_triple` draws no search RNG and a
    /// rollout is a pure function of its `(brains, world, seed, ticks)`, the fan-out reduces in the exact
    /// input order and the archives are **byte-identical** to `jobs = 1` (proved by
    /// `tests/search_parallel.rs`). The ceiling is `OPPONENTS`: children are sequential (each reads the
    /// archive the previous one just mutated), so raising `jobs` past `OPPONENTS` adds no speedup.
    pub jobs: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            seed: 0xC0FFEE,
            generations: 8,
            batch: 4,
            episode_ticks: 7200,
            dungeon_seeds: vec![0x5C09191, 0xA11CE, 0xBEEF],
            resolution: 8,
            jobs: 1,
        }
    }
}

/// Draw two DIFFERENT held-in dungeon seeds (the second differs whenever the set allows), so learnability
/// measures behaviour that generalises across worlds rather than a memorised map. Split out of the old
/// `evaluate_pair` so a challenger's *exact* worlds can be replayed against an incumbent in the Phase-5
/// common-opponent re-evaluation.
fn draw_two_seeds(seeds: &[u64], rng: &mut ChaCha8Rng) -> (u64, u64) {
    let i = rng.below(seeds.len());
    let j = if seeds.len() > 1 {
        let mut j = rng.below(seeds.len() - 1);
        if j >= i {
            j += 1;
        }
        j
    } else {
        i
    };
    (seeds[i], seeds[j])
}

/// The result of scoring one `(squad, swarm, world)` triple across a rollout pair.
struct Triple {
    fitness: Fitness,
    squad: BehaviorDescriptor,
    swarm: BehaviorDescriptor,
    world: BehaviorDescriptor,
}

/// Evaluate one `(squad, swarm, world)` triple: two rollouts on two given worlds (`seed_a`, `seed_b`), with
/// the evolved world config installed on both. `None` when either rollout fails the behavioural minimal
/// criterion — no real encounter, so nothing to score. Fitness is the unchanged `W·S·L` against the frozen
/// prior; each population reads its own descriptor off the first trace.
fn score_triple(
    t: &Templates,
    squad: &SquadGenome,
    swarm: &SwarmGenome,
    world: &WorldGenome,
    prior: &ModePrior,
    seed_a: u64,
    seed_b: u64,
    episode_ticks: u32,
) -> Result<Option<Triple>, String> {
    let wc = world_genome::decode(world)?;
    let a = rollout(brains_of(t, squad, swarm)?, Some(wc), seed_a, episode_ticks);
    if minimal_criterion(&a.outcome).is_err() {
        return Ok(None);
    }
    let b = rollout(brains_of(t, squad, swarm)?, Some(wc), seed_b, episode_ticks);
    if minimal_criterion(&b.outcome).is_err() {
        return Ok(None);
    }
    Ok(Some(Triple {
        fitness: fitness(&a.trace, &b.trace, prior),
        squad: squad_descriptor(&a.trace, &a.outcome),
        swarm: swarm_descriptor(&a.trace),
        world: world_descriptor(&a.trace),
    }))
}

/// One triple to evaluate: the three genomes and the seed pair. The unit of parallel work — a worker
/// process needs nothing else (it rebuilds `Templates` and holds the frozen prior), and a rollout is a
/// pure function of `(brains, world, seed, ticks)`, so this is a self-contained, order-independent job.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TripleJob {
    pub squad: SquadGenome,
    pub swarm: SwarmGenome,
    pub world: WorldGenome,
    pub seed_a: u64,
    pub seed_b: u64,
    pub ticks: u32,
}

/// The wire-friendly result of a [`TripleJob`]: the scalar fitness plus all three descriptors (cheap —
/// four `f32` pairs). The heavy `EpisodeTrace` is reduced worker-side and never crosses the process
/// boundary. `None` (at the `Option` layer above) means the triple failed the minimal criterion.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub(crate) struct TripleScore {
    pub score: f32,
    pub squad: BehaviorDescriptor,
    pub swarm: BehaviorDescriptor,
    pub world: BehaviorDescriptor,
}

/// [`score_triple`] reduced to the compact [`TripleScore`] — the exact computation a worker performs, and
/// the inline path performs too, so both routes are provably identical.
pub(crate) fn score_triple_compact(
    t: &Templates,
    squad: &SquadGenome,
    swarm: &SwarmGenome,
    world: &WorldGenome,
    prior: &ModePrior,
    seed_a: u64,
    seed_b: u64,
    episode_ticks: u32,
) -> Result<Option<TripleScore>, String> {
    Ok(score_triple(t, squad, swarm, world, prior, seed_a, seed_b, episode_ticks)?.map(|tr| TripleScore {
        score: tr.fitness.score(),
        squad: tr.squad,
        swarm: tr.swarm,
        world: tr.world,
    }))
}

/// Where a batch of [`TripleJob`]s is evaluated. `Inline` runs them in this process (the reference path,
/// and what the tests and single-core runs use); `Pool` fans them across worker processes. Both call the
/// identical [`score_triple_compact`], and both return results in input order, so the choice never changes
/// what the search computes — only how fast.
pub(crate) enum Evaluator<'a> {
    Inline { t: &'a Templates, prior: &'a ModePrior },
    Pool(super::parallel::WorkerPool),
}

impl Evaluator<'_> {
    /// Evaluate `jobs`, preserving order. Any error (a decode/feasibility bug, or a worker dying) is fatal
    /// and propagates — never a silent skip, which would corrupt the archive with a candidate scored on
    /// fewer opponents than it was credited for.
    fn eval(&self, jobs: &[TripleJob]) -> Result<Vec<Option<TripleScore>>, String> {
        match self {
            Evaluator::Inline { t, prior } => jobs
                .iter()
                .map(|j| {
                    score_triple_compact(t, &j.squad, &j.swarm, &j.world, prior, j.seed_a, j.seed_b, j.ticks)
                })
                .collect(),
            Evaluator::Pool(pool) => pool.eval(jobs),
        }
    }
}

/// Both archives after a run.
pub struct SearchResult {
    pub squad: Population<SquadGenome>,
    pub swarm: Population<SwarmGenome>,
    pub world: Population<WorldGenome>,
    pub evaluations: u32,
    pub rejected_infeasible: u32,
    pub rejected_by_criterion: u32,
}

/// Run the co-evolutionary search. `report` is called once per generation so a driver can log or
/// checkpoint; nothing here writes to disk.
pub fn search(
    t: &Templates,
    prior: &ModePrior,
    cfg: &SearchConfig,
    mut report: impl FnMut(u32, &SearchResult),
) -> Result<SearchResult, String> {
    prior.validate()?;
    if cfg.dungeon_seeds.is_empty() {
        return Err("search needs at least one dungeon seed".into());
    }

    let mut rng = seeded(cfg.seed);
    let authored_squad = SquadGenome::authored(t);
    let authored_swarm = SwarmGenome::authored(t);
    let authored_world = world_genome::authored();

    let mut result = SearchResult {
        squad: Population::new(cfg.resolution),
        swarm: Population::new(cfg.resolution),
        world: Population::new(cfg.resolution),
        evaluations: 0,
        rejected_infeasible: 0,
        rejected_by_criterion: 0,
    };

    // Where rollouts run. `jobs = 1` (default) evaluates inline in this process — the reference path the
    // tests pin. `jobs > 1` spawns a worker pool; it changes only *where* each triple is scored, never the
    // result (see `parallel` module doc). The pool lives for the whole search and is torn down on return.
    let evaluator = if cfg.jobs <= 1 {
        Evaluator::Inline { t, prior }
    } else {
        Evaluator::Pool(super::parallel::WorkerPool::spawn(cfg.jobs, prior)?)
    };

    // Each generation mutates one child per population and scores it against `OPPONENTS` pairs drawn from
    // the OTHER two archives (three-way co-evolution in the spirit of POET, arXiv:1901.01753, and multi-
    // agent autocurricula, Baker et al. arXiv:1909.07528): a squad child fights (swarm, world) pairs, a
    // swarm child fights (squad, world), and a world child is judged by the (squad, swarm) it induces.
    for generation in 0..cfg.generations {
        for _ in 0..cfg.batch {
            // ── squad child vs (swarm, world) opponents ──
            let parent = result.squad.sample_parent(&mut rng).cloned().unwrap_or_else(|| authored_squad.clone());
            let child = propose_squad(t, &parent, &mut rng, &mut result.rejected_infeasible)?;
            let swarm_opps = sample_or_authored(&result.swarm, &authored_swarm, &mut rng);
            let world_opps = sample_or_authored(&result.world, &authored_world, &mut rng);
            score_and_insert_squad(t, cfg, &mut rng, &mut result, &evaluator, &child, &swarm_opps, &world_opps)?;

            // ── swarm child vs (squad, world) opponents ──
            let parent = result.swarm.sample_parent(&mut rng).cloned().unwrap_or_else(|| authored_swarm.clone());
            let child = propose_swarm(t, &parent, &mut rng, &mut result.rejected_infeasible)?;
            let squad_opps = sample_or_authored(&result.squad, &authored_squad, &mut rng);
            let world_opps = sample_or_authored(&result.world, &authored_world, &mut rng);
            score_and_insert_swarm(t, cfg, &mut rng, &mut result, &evaluator, &child, &squad_opps, &world_opps)?;

            // ── world child vs (squad, swarm) opponents ──
            let parent = result.world.sample_parent(&mut rng).cloned().unwrap_or_else(|| authored_world.clone());
            let child = propose_world(&parent, &mut rng)?;
            let squad_opps = sample_or_authored(&result.squad, &authored_squad, &mut rng);
            let swarm_opps = sample_or_authored(&result.swarm, &authored_swarm, &mut rng);
            score_and_insert_world(t, cfg, &mut rng, &mut result, &evaluator, &child, &squad_opps, &swarm_opps)?;
        }
        report(generation, &result);
    }
    Ok(result)
}

/// Sample `OPPONENTS` opponents from an archive, falling back to `OPPONENTS` copies of the authored genome
/// while the archive is still empty — so an opponent set is always exactly `OPPONENTS` long and two sets
/// (one per other population) can be paired index-by-index into `(a, b)` opponents for one triple.
fn sample_or_authored<G: Clone>(pop: &Population<G>, authored: &G, rng: &mut ChaCha8Rng) -> Vec<G> {
    let sampled = pop.sample_opponents(OPPONENTS, rng);
    if sampled.is_empty() {
        vec![authored.clone(); OPPONENTS]
    } else {
        // `sample_opponents` draws `OPPONENTS` with replacement, so this is already that length.
        sampled.into_iter().cloned().collect()
    }
}

fn score_and_insert_squad(
    t: &Templates,
    cfg: &SearchConfig,
    rng: &mut ChaCha8Rng,
    result: &mut SearchResult,
    evaluator: &Evaluator,
    child: &SquadGenome,
    swarm_opps: &[SwarmGenome],
    world_opps: &[WorldGenome],
) -> Result<(), String> {
    // Phase 1 — sequential: screen each pairing and draw its seed pair, in opponent order. `draw_two_seeds`
    // is the only RNG here, so building the whole job list up front leaves the RNG stream byte-identical to
    // scoring the triples one at a time (`score_triple` draws none).
    let mut jobs = Vec::with_capacity(swarm_opps.len());
    // Each triple's exact (opponents, seeds), so a surviving incumbent can be re-scored on identical
    // conditions in the common-opponent comparison.
    let mut recorded: Vec<(SwarmGenome, WorldGenome, u64, u64)> = Vec::with_capacity(swarm_opps.len());
    for (swarm, world) in swarm_opps.iter().zip(world_opps) {
        // Feasible by construction (`propose_*` screens children; archive members were screened on entry).
        // A failure here is a bug, not a candidate to skip.
        feasible(t, child, swarm)?;
        world_genome::is_feasible(world)?;
        result.evaluations += 1;
        let (sa, sb) = draw_two_seeds(&cfg.dungeon_seeds, rng);
        jobs.push(TripleJob {
            squad: child.clone(),
            swarm: swarm.clone(),
            world: world.clone(),
            seed_a: sa,
            seed_b: sb,
            ticks: cfg.episode_ticks,
        });
        recorded.push((swarm.clone(), world.clone(), sa, sb));
    }

    // Phase 2 — parallel or inline: evaluate every triple, order preserved.
    let outcomes = evaluator.eval(&jobs)?;

    // Phase 3 — sequential: reduce in opponent order, keeping only PASSING triples' descriptors + records.
    let mut scores = Vec::new();
    let mut descriptors = Vec::new();
    let mut kept: Vec<(SwarmGenome, WorldGenome, u64, u64)> = Vec::new();
    for (outcome, rec) in outcomes.into_iter().zip(recorded) {
        match outcome {
            Some(s) => {
                scores.push(s.score);
                descriptors.push(s.squad);
                kept.push(rec);
            }
            None => result.rejected_by_criterion += 1,
        }
    }
    if scores.is_empty() {
        return Ok(());
    }
    let descriptor = mean_descriptor(&descriptors);
    let challenger_fitness = mean(&scores);
    result.squad.try_insert_with_reeval(descriptor, challenger_fitness, child.clone(), |incumbent| {
        reeval_squad(evaluator, incumbent, &kept, cfg.episode_ticks)
    })?;
    Ok(())
}

fn score_and_insert_swarm(
    t: &Templates,
    cfg: &SearchConfig,
    rng: &mut ChaCha8Rng,
    result: &mut SearchResult,
    evaluator: &Evaluator,
    child: &SwarmGenome,
    squad_opps: &[SquadGenome],
    world_opps: &[WorldGenome],
) -> Result<(), String> {
    // Phases as in `score_and_insert_squad`: draw all seeds sequentially (RNG order preserved), evaluate
    // the batch, reduce in order.
    let mut jobs = Vec::with_capacity(squad_opps.len());
    let mut recorded: Vec<(SquadGenome, WorldGenome, u64, u64)> = Vec::with_capacity(squad_opps.len());
    for (squad, world) in squad_opps.iter().zip(world_opps) {
        feasible(t, squad, child)?;
        world_genome::is_feasible(world)?;
        result.evaluations += 1;
        let (sa, sb) = draw_two_seeds(&cfg.dungeon_seeds, rng);
        jobs.push(TripleJob {
            squad: squad.clone(),
            swarm: child.clone(),
            world: world.clone(),
            seed_a: sa,
            seed_b: sb,
            ticks: cfg.episode_ticks,
        });
        recorded.push((squad.clone(), world.clone(), sa, sb));
    }

    let outcomes = evaluator.eval(&jobs)?;

    let mut scores = Vec::new();
    let mut descriptors = Vec::new();
    let mut kept: Vec<(SquadGenome, WorldGenome, u64, u64)> = Vec::new();
    for (outcome, rec) in outcomes.into_iter().zip(recorded) {
        match outcome {
            Some(s) => {
                scores.push(s.score);
                descriptors.push(s.swarm);
                kept.push(rec);
            }
            None => result.rejected_by_criterion += 1,
        }
    }
    if scores.is_empty() {
        return Ok(());
    }
    let descriptor = mean_descriptor(&descriptors);
    let challenger_fitness = mean(&scores);
    result.swarm.try_insert_with_reeval(descriptor, challenger_fitness, child.clone(), |incumbent| {
        reeval_swarm(evaluator, incumbent, &kept, cfg.episode_ticks)
    })?;
    Ok(())
}

fn score_and_insert_world(
    t: &Templates,
    cfg: &SearchConfig,
    rng: &mut ChaCha8Rng,
    result: &mut SearchResult,
    evaluator: &Evaluator,
    child: &WorldGenome,
    squad_opps: &[SquadGenome],
    swarm_opps: &[SwarmGenome],
) -> Result<(), String> {
    world_genome::is_feasible(child)?;
    let mut jobs = Vec::with_capacity(squad_opps.len());
    let mut recorded: Vec<(SquadGenome, SwarmGenome, u64, u64)> = Vec::with_capacity(squad_opps.len());
    for (squad, swarm) in squad_opps.iter().zip(swarm_opps) {
        feasible(t, squad, swarm)?;
        result.evaluations += 1;
        let (sa, sb) = draw_two_seeds(&cfg.dungeon_seeds, rng);
        jobs.push(TripleJob {
            squad: squad.clone(),
            swarm: swarm.clone(),
            world: child.clone(),
            seed_a: sa,
            seed_b: sb,
            ticks: cfg.episode_ticks,
        });
        recorded.push((squad.clone(), swarm.clone(), sa, sb));
    }

    let outcomes = evaluator.eval(&jobs)?;

    let mut scores = Vec::new();
    let mut descriptors = Vec::new();
    let mut kept: Vec<(SquadGenome, SwarmGenome, u64, u64)> = Vec::new();
    for (outcome, rec) in outcomes.into_iter().zip(recorded) {
        match outcome {
            Some(s) => {
                scores.push(s.score);
                descriptors.push(s.world);
                kept.push(rec);
            }
            None => result.rejected_by_criterion += 1,
        }
    }
    if scores.is_empty() {
        return Ok(());
    }
    let descriptor = mean_descriptor(&descriptors);
    let challenger_fitness = mean(&scores);
    result.world.try_insert_with_reeval(descriptor, challenger_fitness, child.clone(), |incumbent| {
        reeval_world(evaluator, incumbent, &kept, cfg.episode_ticks)
    })?;
    Ok(())
}

// ── Common-opponent re-evaluation (the Phase-5 non-stationarity fix) ────────────────────────────────
//
// Each `reeval_*` re-scores an incumbent on a challenger's EXACT recorded opponents and seeds, so the two
// are compared under identical conditions before `try_insert_with_reeval`'s elitism test. `None` means the
// incumbent produced no real encounter on any of them (inadmissible here) — the challenger, which did, wins.
// No fresh RNG is drawn (recorded seeds are replayed), so the run stays reproducible.
//
// SERIAL_GUARD: each `rollout` inside `score_triple` acquires the non-reentrant `HARNESS_LOCK` itself and
// releases it before the next, so these sequential re-eval rollouts are safe. `search()` must therefore
// NEVER hold `serial_guard` around the generation loop — doing so (e.g. to "reuse one lock") would deadlock
// the very first re-eval on the lock the loop already holds.

fn reeval_squad(
    evaluator: &Evaluator,
    incumbent: &SquadGenome,
    recorded: &[(SwarmGenome, WorldGenome, u64, u64)],
    episode_ticks: u32,
) -> Result<Option<f32>, String> {
    let jobs: Vec<TripleJob> = recorded
        .iter()
        .map(|(swarm, world, sa, sb)| TripleJob {
            squad: incumbent.clone(),
            swarm: swarm.clone(),
            world: world.clone(),
            seed_a: *sa,
            seed_b: *sb,
            ticks: episode_ticks,
        })
        .collect();
    let scores: Vec<f32> = evaluator.eval(&jobs)?.into_iter().flatten().map(|s| s.score).collect();
    Ok(if scores.is_empty() { None } else { Some(mean(&scores)) })
}

fn reeval_swarm(
    evaluator: &Evaluator,
    incumbent: &SwarmGenome,
    recorded: &[(SquadGenome, WorldGenome, u64, u64)],
    episode_ticks: u32,
) -> Result<Option<f32>, String> {
    let jobs: Vec<TripleJob> = recorded
        .iter()
        .map(|(squad, world, sa, sb)| TripleJob {
            squad: squad.clone(),
            swarm: incumbent.clone(),
            world: world.clone(),
            seed_a: *sa,
            seed_b: *sb,
            ticks: episode_ticks,
        })
        .collect();
    let scores: Vec<f32> = evaluator.eval(&jobs)?.into_iter().flatten().map(|s| s.score).collect();
    Ok(if scores.is_empty() { None } else { Some(mean(&scores)) })
}

fn reeval_world(
    evaluator: &Evaluator,
    incumbent: &WorldGenome,
    recorded: &[(SquadGenome, SwarmGenome, u64, u64)],
    episode_ticks: u32,
) -> Result<Option<f32>, String> {
    let jobs: Vec<TripleJob> = recorded
        .iter()
        .map(|(squad, swarm, sa, sb)| TripleJob {
            squad: squad.clone(),
            swarm: swarm.clone(),
            world: incumbent.clone(),
            seed_a: *sa,
            seed_b: *sb,
            ticks: episode_ticks,
        })
        .collect();
    let scores: Vec<f32> = evaluator.eval(&jobs)?.into_iter().flatten().map(|s| s.score).collect();
    Ok(if scores.is_empty() { None } else { Some(mean(&scores)) })
}

/// Expose one squad mutation to the integration tests, which must prove that a candidate genome actually
/// reaches `utility::decide` — every other test runs the authored brains, so a silently-dropped candidate
/// would go unnoticed.
pub fn mutate_squad_for_test(
    t: &Templates,
    parent: &SquadGenome,
    sigma: f32,
    mut rng: ChaCha8Rng,
) -> SquadGenome {
    let mut out = Vec::with_capacity(parent.0.len());
    for (template, p) in t.roles.iter().zip(&parent.0) {
        out.push(mutate(template, p, sigma, 0.0, &mut rng).expect("mutate"));
    }
    SquadGenome(out)
}

/// Mutate every role's genome. The band origin is the *template*, derived inside `genome::mutate`, so it
/// cannot drift with the parent.
fn mutate_squad(
    t: &Templates,
    parent: &SquadGenome,
    rng: &mut ChaCha8Rng,
) -> Result<SquadGenome, String> {
    if parent.0.len() != t.roles.len() {
        return Err(format!("squad genome has {} roles, expected {}", parent.0.len(), t.roles.len()));
    }
    let mut out = Vec::with_capacity(parent.0.len());
    for (template, p) in t.roles.iter().zip(&parent.0) {
        out.push(mutate(template, p, SIGMA, RANK_SWAP_P, rng)?);
    }
    Ok(SquadGenome(out))
}

fn mutate_swarm(
    t: &Templates,
    parent: &SwarmGenome,
    rng: &mut ChaCha8Rng,
) -> Result<SwarmGenome, String> {
    Ok(SwarmGenome {
        crab: mutate(&t.crab, &parent.crab, SIGMA, RANK_SWAP_P, rng)?,
        scout: mutate(&t.scout, &parent.scout, SIGMA, RANK_SWAP_P, rng)?,
        smiley: mutate(&t.smiley, &parent.smiley, SIGMA, RANK_SWAP_P, rng)?,
    })
}

/// Propose a world child. `world_genome::mutate` clamps every knob into its hard `BOUNDS`, so a child is
/// feasible **by construction** — no rejection-sampling loop, unlike the brains (whose feasibility is a
/// value-space guard the mutation can violate). `is_feasible` is still asserted: a failure would be a
/// `BOUNDS` bug, and one path means surfacing it loudly rather than searching an infeasible world.
fn propose_world(parent: &WorldGenome, rng: &mut ChaCha8Rng) -> Result<WorldGenome, String> {
    let child = world_genome::mutate(parent, WORLD_SIGMA, rng)?;
    world_genome::is_feasible(&child)?;
    Ok(child)
}

/// Mean of a non-empty slice. Sorted before summing so the result does not depend on evaluation order —
/// float addition is not associative, and the whole run must be reproducible from its seed.
fn mean(xs: &[f32]) -> f32 {
    let mut sorted: Vec<u32> = xs.iter().map(|x| x.to_bits()).collect();
    sorted.sort_unstable();
    let sum: f32 = sorted.iter().map(|b| f32::from_bits(*b)).sum();
    sum / xs.len() as f32
}

fn mean_descriptor(ds: &[BehaviorDescriptor]) -> BehaviorDescriptor {
    let x = mean(&ds.iter().map(|d| d.aggression).collect::<Vec<_>>());
    let y = mean(&ds.iter().map(|d| d.exploration).collect::<Vec<_>>());
    BehaviorDescriptor::new(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::squad_ai::surprise::{ActorKind, Context, Decision, FearBucket};

    fn decision(actor: ActorKind, mode: Mode) -> Decision {
        Decision {
            actor_id: 0,
            context: Context {
                actor,
                fear: FearBucket::Calm,
                threat_known: false,
                ally_down: false,
                past_leash: false,
            },
            mode,
            witnessed: true,
        }
    }

    #[test]
    fn the_authored_pairing_is_feasible() {
        // The search must admit the shipped brains, or it is measuring the wrong space.
        let t = Templates::authored();
        let squad = SquadGenome::authored(&t);
        let swarm = SwarmGenome::authored(&t);
        assert!(feasible(&t, &squad, &swarm).is_ok(), "{:?}", feasible(&t, &squad, &swarm));
        assert!(brains_of(&t, &squad, &swarm).is_ok());
    }

    #[test]
    fn brains_of_rejects_a_mis_shaped_squad_genome() {
        let t = Templates::authored();
        let mut squad = SquadGenome::authored(&t);
        squad.0.pop();
        let swarm = SwarmGenome::authored(&t);
        assert!(brains_of(&t, &squad, &swarm).is_err(), "a short genome must never silently decode");
    }

    #[test]
    fn descriptors_read_the_axes_a_player_perceives() {
        let trace = EpisodeTrace {
            decisions: vec![
                decision(ActorKind::Role(RoleId::Gunman), Mode::Engage),
                decision(ActorKind::Role(RoleId::Gunman), Mode::Overwatch),
                decision(ActorKind::Role(RoleId::Medic), Mode::TendWounded),
                decision(ActorKind::Role(RoleId::Medic), Mode::Wander),
                decision(ActorKind::Crab, Mode::Latch),
                decision(ActorKind::Crab, Mode::Flee),
                decision(ActorKind::Scout, Mode::Rally),
                decision(ActorKind::Smiley, Mode::Chase),
            ],
        };
        let outcome = EpisodeOutcome { cells_covered: 25, reachable_cells: 100, ..Default::default() };

        let squad = squad_descriptor(&trace, &outcome);
        assert!((squad.aggression - 0.5).abs() < 1e-6, "2 of 4 unit decisions press the fight");
        assert!((squad.exploration - 0.25).abs() < 1e-6);

        let swarm = swarm_descriptor(&trace);
        assert!((swarm.aggression - 0.75).abs() < 1e-6, "Latch/Rally/Chase of 4 creature decisions");
        assert!((swarm.exploration - 0.75).abs() < 1e-6, "1 of 4 fled");
    }

    #[test]
    fn descriptors_of_an_empty_trace_are_zero_not_nan() {
        let empty = EpisodeTrace::default();
        let outcome = EpisodeOutcome::default();
        let s = squad_descriptor(&empty, &outcome);
        assert_eq!((s.aggression, s.exploration), (0.0, 0.0));
        // A swarm that never decided has zero aggression — and `persistence` reads 1.0, since nothing fled.
        let w = swarm_descriptor(&empty);
        assert_eq!(w.aggression, 0.0);
        assert!(w.exploration.is_finite());
    }

    #[test]
    fn population_handles_stay_valid_when_an_insert_is_rejected() {
        // Handles index `store` and are never recycled, so a rejected insert must not invalidate an
        // incumbent's handle.
        let mut pop: Population<u32> = Population::new(4);
        let d = BehaviorDescriptor::new(0.5, 0.5);
        assert!(pop.insert(d, 0.8, 111));
        assert!(!pop.insert(d, 0.2, 222), "worse fitness must be rejected");
        let elite = pop.archive.best().expect("an elite");
        assert_eq!(pop.get(elite.genome), Some(&111), "the incumbent's handle still resolves");
    }

    #[test]
    fn reeval_insert_resolves_a_contested_cell_by_the_common_opponent_score() {
        // The Phase-5 elitism logic (no rollouts — the re-eval is a closure). An empty cell takes the
        // challenger without consulting the incumbent; a contest is decided by re-scoring the incumbent on
        // the challenger's conditions, and the incumbent's stored fitness is refreshed to that fresh value.
        let mut pop: Population<u32> = Population::new(4);
        let d = BehaviorDescriptor::new(0.5, 0.5);

        assert!(pop
            .try_insert_with_reeval(d, 0.8, 111, |_| panic!("no re-eval on an empty cell"))
            .unwrap());

        // Incumbent re-scores >= challenger under the common opponents → it holds, refreshed to 0.95.
        assert!(!pop
            .try_insert_with_reeval(d, 0.9, 222, |&g| {
                assert_eq!(g, 111, "the incumbent genome is re-evaluated");
                Ok(Some(0.95))
            })
            .unwrap());
        let inc = pop.archive.incumbent(d).expect("held");
        assert_eq!(pop.get(inc.genome), Some(&111));
        assert!((inc.fitness - 0.95).abs() < 1e-6, "fitness refreshed to the common-opponent score");

        // Incumbent re-scores lower → challenger wins.
        assert!(pop.try_insert_with_reeval(d, 0.5, 333, |_| Ok(Some(0.1))).unwrap());
        assert_eq!(pop.get(pop.archive.incumbent(d).unwrap().genome), Some(&333));

        // Incumbent inadmissible (produces no encounter) under the challenger's conditions → challenger wins.
        let d2 = BehaviorDescriptor::new(0.1, 0.1);
        assert!(pop.try_insert_with_reeval(d2, 0.4, 444, |_| panic!("empty")).unwrap());
        assert!(pop.try_insert_with_reeval(d2, 0.2, 555, |_| Ok(None)).unwrap());
        assert_eq!(pop.get(pop.archive.incumbent(d2).unwrap().genome), Some(&555));
    }

    #[test]
    fn sampling_is_deterministic_under_a_seed_and_empty_archives_yield_nothing() {
        let mut pop: Population<u32> = Population::new(4);
        assert!(pop.sample_parent(&mut seeded(1)).is_none(), "an empty archive has no parent");
        assert!(pop.sample_opponents(3, &mut seeded(1)).is_empty());

        pop.insert(BehaviorDescriptor::new(0.1, 0.1), 0.5, 7);
        pop.insert(BehaviorDescriptor::new(0.9, 0.9), 0.5, 9);
        let a: Vec<u32> = pop.sample_opponents(5, &mut seeded(42)).into_iter().copied().collect();
        let b: Vec<u32> = pop.sample_opponents(5, &mut seeded(42)).into_iter().copied().collect();
        assert_eq!(a, b, "opponent sampling must be reproducible from the seed");
        assert_eq!(a.len(), 5, "sampling is with replacement, so a sparse archive still yields k");
    }

    #[test]
    fn mean_is_order_independent() {
        // Float addition is not associative; the search's reproducibility depends on this.
        let xs = [0.1f32, 0.2, 0.3, 0.7, 0.9];
        let mut ys = xs;
        ys.reverse();
        assert_eq!(mean(&xs), mean(&ys));
    }

    #[test]
    fn mutation_yields_feasible_children_often_enough_for_rejection_sampling() {
        // A canary on SIGMA, and a regression test for a bug this test found: with the slope sign left
        // free, `Linear{m}` went negative half the time, `guaranteed_floor` lost both unconditional tails
        // (`wander`, `follow_anchor`, both authored `m = 0.0`), and **0 of 32** children were feasible —
        // the search would have spun forever. `ParamKind::SignLocked` fixed it.
        //
        // The residual rejection rate is the guard working as designed (`wander`'s intercept sits 0.02
        // above MIN_SCORE), and `propose_*` absorbs it by redrawing. The bar here only has to be high
        // enough that bounded rejection sampling terminates comfortably.
        let t = Templates::authored();
        let squad0 = SquadGenome::authored(&t);
        let swarm0 = SwarmGenome::authored(&t);
        let mut rng = seeded(99);
        let mut ok = 0;
        for _ in 0..64 {
            let squad = mutate_squad(&t, &squad0, &mut rng).expect("mutate");
            let swarm = mutate_swarm(&t, &swarm0, &mut rng).expect("mutate");
            if feasible(&t, &squad, &swarm).is_ok() {
                ok += 1;
            }
        }
        assert!(ok >= 16, "only {ok}/64 joint children feasible — SIGMA {SIGMA} is too large");
    }

    #[test]
    fn proposal_always_returns_a_feasible_child() {
        // `propose_*` is what makes "a child that reaches evaluation is always loadable" true.
        let t = Templates::authored();
        let squad0 = SquadGenome::authored(&t);
        let swarm0 = SwarmGenome::authored(&t);
        let mut rng = seeded(7);
        let mut rejected = 0;
        for _ in 0..16 {
            let squad = propose_squad(&t, &squad0, &mut rng, &mut rejected).expect("a feasible child");
            let swarm = propose_swarm(&t, &swarm0, &mut rng, &mut rejected).expect("a feasible child");
            assert!(squad_feasible(&t, &squad).is_ok());
            assert!(swarm_feasible(&t, &swarm).is_ok());
        }
        assert!(rejected > 0, "some draws should be rejected — else the guard is not binding");
    }
}

// ── Human-reviewable artifacts ───────────────────────────────────────────────────────────────────
//
// An elite is committed as RON in the same shape a designer authors. This is the reward-hacking guard
// (Skalse et al.): before an archive ships, a human reads the diff and can refuse it. Opaque weights
// would make that impossible, and the project's one-path rule forbids "magic results that are hard to
// debug".

/// One squad elite, decoded back into authored form.
#[derive(Serialize)]
pub struct SquadEliteDoc {
    pub cell: (usize, usize),
    pub aggression: f32,
    pub exploration: f32,
    pub fitness: f32,
    /// `(role, repertoire)` pairs in `RoleId::ALL` order — the `roles.ron` shape.
    pub roles: Vec<(RoleId, crate::squad_ai::role::RoleDef)>,
}

/// One swarm elite, decoded back into authored form.
#[derive(Serialize)]
pub struct SwarmEliteDoc {
    pub cell: (usize, usize),
    pub aggression: f32,
    pub persistence: f32,
    pub fitness: f32,
    pub crab: Vec<Behavior>,
    pub scout: Vec<Behavior>,
    pub smiley: Vec<Behavior>,
}

/// The archive as it lands on disk.
#[derive(Serialize)]
pub struct ArchiveDoc<E> {
    pub resolution: usize,
    pub coverage: usize,
    pub qd_score: f32,
    pub elites: Vec<E>,
}

/// Decode every squad elite for review/commit.
pub fn squad_archive_doc(t: &Templates, pop: &Population<SquadGenome>) -> Result<ArchiveDoc<SquadEliteDoc>, String> {
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let genome = pop.get(elite.genome).ok_or("dangling elite handle")?;
        let mut roles = Vec::new();
        for ((role, template), g) in RoleId::ALL.iter().zip(&t.roles).zip(&genome.0) {
            roles.push((*role, crate::squad_ai::role::RoleDef { behaviors: decode(template, g)? }));
        }
        elites.push(SquadEliteDoc {
            cell: *cell,
            aggression: elite.descriptor.aggression,
            exploration: elite.descriptor.exploration,
            fitness: elite.fitness,
            roles,
        });
    }
    Ok(ArchiveDoc {
        resolution: pop.archive.resolution(),
        coverage: pop.archive.coverage(),
        qd_score: pop.archive.qd_score(),
        elites,
    })
}

/// Decode every swarm elite for review/commit.
pub fn swarm_archive_doc(t: &Templates, pop: &Population<SwarmGenome>) -> Result<ArchiveDoc<SwarmEliteDoc>, String> {
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let g = pop.get(elite.genome).ok_or("dangling elite handle")?;
        elites.push(SwarmEliteDoc {
            cell: *cell,
            aggression: elite.descriptor.aggression,
            // The archive's second axis carries `persistence` for the swarm (see `swarm_descriptor`).
            persistence: elite.descriptor.exploration,
            fitness: elite.fitness,
            crab: decode(&t.crab, &g.crab)?,
            scout: decode(&t.scout, &g.scout)?,
            smiley: decode(&t.smiley, &g.smiley)?,
        });
    }
    Ok(ArchiveDoc {
        resolution: pop.archive.resolution(),
        coverage: pop.archive.coverage(),
        qd_score: pop.archive.qd_score(),
        elites,
    })
}

/// One world elite, decoded back into its two config slices — a readable RON diff of the shipped world's
/// dials (the reward-hacking guard: a human reads what the search found before it ships).
#[derive(Serialize)]
pub struct WorldEliteDoc {
    pub cell: (usize, usize),
    /// The archive's axes carry the world's descriptor (`world_descriptor`): mean unit fear × swarm
    /// aggression. `BehaviorDescriptor`'s generic `aggression`/`exploration` fields hold them respectively.
    pub mean_fear: f32,
    pub swarm_aggression: f32,
    pub fitness: f32,
    pub ai: crate::ai::tuning::AiTuning,
    pub sim: crate::sim::SimTuning,
}

/// Decode every world elite for review/commit — each is a readable diff of the shipped world's dials.
pub fn world_archive_doc(pop: &Population<WorldGenome>) -> Result<ArchiveDoc<WorldEliteDoc>, String> {
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let g = pop.get(elite.genome).ok_or("dangling elite handle")?;
        let wc = world_genome::decode(g)?;
        elites.push(WorldEliteDoc {
            cell: *cell,
            mean_fear: elite.descriptor.aggression,
            swarm_aggression: elite.descriptor.exploration,
            fitness: elite.fitness,
            ai: wc.ai,
            sim: wc.sim,
        });
    }
    Ok(ArchiveDoc {
        resolution: pop.archive.resolution(),
        coverage: pop.archive.coverage(),
        qd_score: pop.archive.qd_score(),
        elites,
    })
}

/// Sweep the **authored** brains to build the player's baseline expectation.
///
/// This is `P(mode | context)` for the game as shipped: the model every prior encounter has trained the
/// player on. Surprise is measured against it and it never moves during a search — a reference that
/// drifted with the population would make "surprising" mean only "different from last generation".
pub fn sweep_prior(t: &Templates, seeds: &[u64], episode_ticks: u32) -> Result<ModePrior, String> {
    let squad = SquadGenome::authored(t);
    let swarm = SwarmGenome::authored(t);
    let mut prior = ModePrior::default();
    for &seed in seeds {
        let r = rollout(brains_of(t, &squad, &swarm)?, None, seed, episode_ticks);
        prior.observe(&r.trace);
    }
    Ok(prior)
}
