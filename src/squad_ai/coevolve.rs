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

use super::evaluate::rollout;
use super::genome::{decode, encode, is_feasible, is_feasible_creature, mutate, Genome};
use super::qd::{BehaviorDescriptor, MapElitesArchive};
use super::surprise::{
    fitness, minimal_criterion, EpisodeOutcome, EpisodeTrace, ModePrior,
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

/// Half-saturation constant for the world's vitality axes: the cross-species death/life count that maps to
/// the descriptor midpoint `0.5`. **Calibrated by measurement, not guessed** (`train probe` on the shipped
/// worlds at 7200 ticks: ~11–17 total deaths and ~6–18 total lives across the held-in seeds — the same
/// discipline behind `surprise::MIN_COVERAGE` and the `FearBucket` bands). At `K = 25` the shipped game sits
/// low-mid on both axes (~0.3 deaths, ~0.2–0.4 lives), so the whole deadlier/teeming corner stays as
/// headroom for the search to illuminate.
///
/// The map is a **saturating** response `x / (x + K)` (Holling Type II / Michaelis–Menten), not a
/// hard-clamped linear scale: cross-species counts span a wide range as the breeding/lethality/parasite
/// knobs move (Gras et al. 2009 — individual counts co-vary strongly with the dynamics), and a saturating
/// map keeps descriptor resolution across that whole range instead of clipping the extremes into one bin.
const VITALITY_HALF_SCALE: f32 = 25.0;

/// The **world's** axes — the two dials the search was pointed at: how many creatures DIE and how many are
/// alive (LIVES) at episode end, summed across every species the headless sim observes (squad units, crabs,
/// SCP-150 mancae, boss; mushrooms are GPU-only and shaped by the separate `train levels` search). This is
/// the "deaths and lives across all species" archive: MAP-Elites spreads worlds from graveyard to teeming
/// and a human picks the regime. Fitness stays `W·S·L` — these axes carry *diversity*, not quality.
///
/// Grounding: predator–prey turnover and biodiversity are canonical signals of a living ecosystem (Gras et
/// al., Artificial Life 15(4) 2009; Yang, arXiv:1003.5288). NOTE: the LIVES axis is total abundance (a
/// headcount sum), NOT the Shannon diversity index (which needs species proportions / evenness).
/// Each count is mapped into `[0,1)` by the saturating [`VITALITY_HALF_SCALE`] response; the
/// breeding-vs-lethality knobs (now including the parasite's brood/gestation) give the search 2-D freedom,
/// so deadly-yet-teeming and deadly-yet-depleted worlds land in different niches rather than collapsing onto
/// a diagonal.
pub fn world_descriptor(outcome: &EpisodeOutcome) -> BehaviorDescriptor {
    // Saturating (Holling Type II) map so a wide count range keeps descriptor resolution; see the constant.
    let softsat = |x: u32| x as f32 / (x as f32 + VITALITY_HALF_SCALE);
    BehaviorDescriptor::new(softsat(outcome.total_deaths()), softsat(outcome.total_lives()))
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

/// The **held-in dungeon seeds** every search evaluates against. Defined once, on purpose: a re-selection
/// must not be able to leave a stale copy behind. It already has — `0xA11CE` and `0xBEEF` were retired when
/// the mold landed (see `mold::MoldConfig`'s `Default`), and survived for months in docs and test comments
/// that still called them "held-in", long enough to mislead a later reader into re-tuning the episode floor
/// against a world the search no longer runs.
///
/// Chosen so the shipped squad produces a real encounter on each: it survives with margin AND the swarm
/// survives, so neither side is wiped.
pub const HELD_IN_SEEDS: [u64; 3] = [0x5C09191, 0x1CE5, 0xB0BA];

/// Everything one run needs. `episode_ticks` at 60 Hz: 7200 ≈ 120 s of simulated time.
///
/// 120 s is a **floor**, not a preference, and it is measured (`train probe`) rather than chosen. The
/// evaluation alternates player-ordered advance with AI-controlled engagement (see `evaluate`), so only part
/// of the episode moves the squad toward the nests.
///
/// **Measured 2026-07-17** — authored brains on the default world, `train probe --ticks N`, reporting
/// `unit_damage_taken` per held-in seed:
///
/// | seed | 1800 | 3600 | 5400 | 7200 |
/// |---|---|---|---|---|
/// | `0x5C09191` | 46 | 46 | 46 | 46 |
/// | `0x1CE5` | 0\* | 0\* | 0\* | 91 |
/// | `0xB0BA` | 1 | 1 | 207 | 489 |
///
/// \* `train probe` prints damage with `{:.0}`. These cells PASS the `unit_damage_taken > 0.0` clause, so
/// they are non-zero — but under half a hit point.
///
/// **Every cell passes `minimal_criterion`.** So — contrary to what this comment claimed before it was
/// re-measured — a short episode does not outright *reject* the shipped game. What it does is leave the
/// criterion balanced on a knife's edge: below 7200, `0x1CE5` clears "nothing was at stake" by less than one
/// hit point and `0xB0BA` by one. A candidate marginally less aggressive than the authored brain is rejected
/// there, so the admitted fraction collapses and the archives come back thin — the same empty-archive
/// failure as before, arriving probabilistically rather than absolutely. `0x1CE5` is the binding world: it
/// only acquires real stakes between 5400 and 7200. Cross-seed replayability tracks the same edge — 0.049 /
/// 0.026 / 0.111 / 0.114 at 1800 / 3600 / 5400 / 7200.
///
/// A shorter episode does not make the search cheaper; it makes it thin.
///
/// **Re-measure with `train probe` after anything that moves the deterministic trajectory.** These numbers
/// are a snapshot, not a law. The previous snapshot went stale silently and was later read as authoritative.
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
    /// fans a whole generation's `batch × OPPONENTS` independent triples (per population) across them — the
    /// batch MAP-Elites emitter (`batch_population`). Parallelism must be across *processes*, never threads:
    /// `sim_harness` holds a process-wide lock and pins the compute pool to one thread for determinism (see
    /// `evaluate` module doc). Because `score_triple_compact` draws no search RNG and a rollout is a pure
    /// function of its `(brains, world, seed, ticks)`, the fan-out reduces in the exact input order and the
    /// archives are **byte-identical** to `jobs = 1` (proved by `tests/search_parallel.rs`). The useful
    /// ceiling is now `batch × OPPONENTS` (per population per generation) — a whole batch is scored at once —
    /// so `jobs` scales to the box; raise `batch` for more width.
    pub jobs: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            seed: 0xC0FFEE,
            generations: 8,
            batch: 4,
            episode_ticks: 7200,
            dungeon_seeds: HELD_IN_SEEDS.to_vec(),
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

/// Evaluate one `(squad, swarm, world)` triple, reduced to the compact [`TripleScore`]: two rollouts on two
/// given worlds (`seed_a`, `seed_b`), with the evolved world config installed on both. `None` when either
/// rollout fails the behavioural minimal criterion — no real encounter, so nothing to score. Fitness is the
/// unchanged `W·S·L` against the frozen prior; each population reads its own descriptor off the first trace.
/// This is the exact computation a worker performs, and the inline path performs too, so both routes are
/// provably identical.
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
    let wc = world_genome::decode(world)?;
    let a = rollout(brains_of(t, squad, swarm)?, Some(wc), None, None, seed_a, episode_ticks);
    if minimal_criterion(&a.outcome).is_err() {
        return Ok(None);
    }
    let b = rollout(brains_of(t, squad, swarm)?, Some(wc), None, None, seed_b, episode_ticks);
    if minimal_criterion(&b.outcome).is_err() {
        return Ok(None);
    }
    Ok(Some(TripleScore {
        score: fitness(&a.trace, &b.trace, prior).score(),
        squad: squad_descriptor(&a.trace, &a.outcome),
        swarm: swarm_descriptor(&a.trace),
        world: world_descriptor(&a.outcome),
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
        Evaluator::Pool(super::parallel::WorkerPool::spawn(cfg.jobs, prior, t)?)
    };

    // Each generation mutates one child per population and scores it against `OPPONENTS` pairs drawn from
    // the OTHER two archives (three-way co-evolution in the spirit of POET, arXiv:1901.01753, and multi-
    // agent autocurricula, Baker et al. arXiv:1909.07528): a squad child fights (swarm, world) pairs, a
    // swarm child fights (squad, world), and a world child is judged by the (squad, swarm) it induces.
    for generation in 0..cfg.generations {
        // Each population's whole generation is proposed and scored as ONE batch (see `batch_population`):
        // `cfg.batch` children against a frozen archive snapshot, every child's `OPPONENTS` triples flattened
        // into a single `eval` call so `--jobs` scales to `batch × OPPONENTS` workers, inserted in a pinned
        // order. Sub-phases stay ordered squad→swarm→world within a generation (a swarm child still fights
        // this generation's freshly-inserted squad elites), which keeps the three-way autocurriculum tight.

        // ── squad children vs (swarm, world) opponents ──
        batch_population(
            cfg,
            &mut rng,
            &evaluator,
            &mut result.squad,
            &result.swarm,
            &result.world,
            &authored_squad,
            &authored_swarm,
            &authored_world,
            &mut result.evaluations,
            &mut result.rejected_infeasible,
            &mut result.rejected_by_criterion,
            |parent, rng, rejected| propose_squad(t, parent, rng, rejected),
            |_child| Ok(()),
            |child, swarm, world| {
                feasible(t, child, swarm)?;
                world_genome::is_feasible(world)
            },
            |c, swarm, world, sa, sb| TripleJob {
                squad: c.clone(),
                swarm: swarm.clone(),
                world: world.clone(),
                seed_a: sa,
                seed_b: sb,
                ticks: cfg.episode_ticks,
            },
            |s| s.squad,
        )?;

        // ── swarm children vs (squad, world) opponents ──
        batch_population(
            cfg,
            &mut rng,
            &evaluator,
            &mut result.swarm,
            &result.squad,
            &result.world,
            &authored_swarm,
            &authored_squad,
            &authored_world,
            &mut result.evaluations,
            &mut result.rejected_infeasible,
            &mut result.rejected_by_criterion,
            |parent, rng, rejected| propose_swarm(t, parent, rng, rejected),
            |_child| Ok(()),
            |child, squad, world| {
                feasible(t, squad, child)?;
                world_genome::is_feasible(world)
            },
            |c, squad, world, sa, sb| TripleJob {
                squad: squad.clone(),
                swarm: c.clone(),
                world: world.clone(),
                seed_a: sa,
                seed_b: sb,
                ticks: cfg.episode_ticks,
            },
            |s| s.swarm,
        )?;

        // ── world children vs (squad, swarm) opponents ──
        batch_population(
            cfg,
            &mut rng,
            &evaluator,
            &mut result.world,
            &result.squad,
            &result.swarm,
            &authored_world,
            &authored_squad,
            &authored_swarm,
            &mut result.evaluations,
            &mut result.rejected_infeasible,
            &mut result.rejected_by_criterion,
            |parent, rng, _rejected| propose_world(parent, rng),
            |child| world_genome::is_feasible(child),
            |_child, squad, swarm| feasible(t, squad, swarm),
            |c, squad, swarm, sa, sb| TripleJob {
                squad: squad.clone(),
                swarm: swarm.clone(),
                world: c.clone(),
                seed_a: sa,
                seed_b: sb,
                ticks: cfg.episode_ticks,
            },
            |s| s.world,
        )?;

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

/// Propose and score ONE population's whole generation as a batch, then insert — the **batch variant of
/// MAP-Elites** (Mouret & Clune 2015, *Illuminating search spaces by mapping elites*, arXiv:1504.04909,
/// §"batch": "a batch of `b` individuals is generated and evaluated in parallel before the map is updated";
/// parallel-scaling rationale: Colas, Madhavan, Huizinga & Clune 2020, *Scaling MAP-Elites to Deep
/// Neuroevolution*, doi:10.1145/3377930.3390217). The three co-evolving populations share this exact
/// Predraw/Eval/Insert structure and differ only in where the child sits in a triple, so it is parameterized:
///
/// - `propose` mutates a sampled parent into a feasible child (the brains rejection-sample; the world child
///   is feasible by construction);
/// - `pre_check` gates the child itself (the world child screens its own knobs; the brains pass `|_| Ok(())`);
/// - `check` screens one opponent pair (the two feasibility calls, which draw no RNG);
/// - `make_job` places a child-role genome into the correct triple slot beside its two opponents — used both
///   for the forward jobs (with `child`) and, inside `try_insert_with_reeval`, to re-score a surviving
///   incumbent on the challenger's exact recorded conditions (with `incumbent`);
/// - `select` picks this population's descriptor axis off a [`TripleScore`].
///
/// **Determinism + parallelism-invariance.** All RNG (parent pick, the variable-length `propose` redraws, the
/// two opponent samples, and each triple's `draw_two_seeds`) is consumed serially in child-then-opponent
/// order during PREDRAW, before any rollout — and a rollout draws none. So the whole generation's `batch ×
/// OPPONENTS` triples can be flattened into ONE `evaluator.eval` call: it reduces in input order, and inserts
/// are applied in the pinned predraw order (so the `>=` elitism tie-break in `try_insert_with_reeval` and the
/// contested-cell re-evals are reproducible). `jobs=1` (inline) and `jobs=N` (pool) therefore produce
/// bit-identical archives — the `--jobs` ceiling rises from `OPPONENTS` (3) to `batch × OPPONENTS`. Children
/// are proposed against the archive as it stands at the start of this sub-phase (inserts deferred) — the
/// standard online→batch trade.
#[allow(clippy::too_many_arguments)]
fn batch_population<C: Clone, O1: Clone, O2: Clone>(
    cfg: &SearchConfig,
    rng: &mut ChaCha8Rng,
    evaluator: &Evaluator,
    population: &mut Population<C>,
    opp_pop1: &Population<O1>,
    opp_pop2: &Population<O2>,
    authored_child: &C,
    authored_opp1: &O1,
    authored_opp2: &O2,
    evaluations: &mut u32,
    rejected_infeasible: &mut u32,
    rejected_by_criterion: &mut u32,
    propose: impl Fn(&C, &mut ChaCha8Rng, &mut u32) -> Result<C, String>,
    pre_check: impl Fn(&C) -> Result<(), String>,
    check: impl Fn(&C, &O1, &O2) -> Result<(), String>,
    make_job: impl Fn(&C, &O1, &O2, u64, u64) -> TripleJob,
    select: impl Fn(&TripleScore) -> BehaviorDescriptor,
) -> Result<(), String> {
    // One child's forward conditions, carried from predraw to the deferred insert.
    struct Pending<C, O1, O2> {
        child: C,
        recorded: Vec<(O1, O2, u64, u64)>,
    }

    // Phase 1 — PREDRAW: propose every child against the frozen (start-of-sub-phase) archives and build all
    // their triples. This is the only place RNG is consumed, in a fixed serial order.
    let mut pending: Vec<Pending<C, O1, O2>> = Vec::with_capacity(cfg.batch as usize);
    let mut all_jobs: Vec<TripleJob> = Vec::with_capacity(cfg.batch as usize * OPPONENTS);
    for _ in 0..cfg.batch {
        let parent = population.sample_parent(rng).cloned().unwrap_or_else(|| authored_child.clone());
        let child = propose(&parent, rng, rejected_infeasible)?;
        let opps1 = sample_or_authored(opp_pop1, authored_opp1, rng);
        let opps2 = sample_or_authored(opp_pop2, authored_opp2, rng);
        // A one-time gate on the child itself (the world child screens its own knobs). A failure is a bug.
        pre_check(&child)?;
        let mut recorded: Vec<(O1, O2, u64, u64)> = Vec::with_capacity(opps1.len());
        for (o1, o2) in opps1.iter().zip(&opps2) {
            // Feasible by construction; a failure here is a bug, not a candidate to skip.
            check(&child, o1, o2)?;
            *evaluations += 1;
            let (sa, sb) = draw_two_seeds(&cfg.dungeon_seeds, rng);
            all_jobs.push(make_job(&child, o1, o2, sa, sb));
            recorded.push((o1.clone(), o2.clone(), sa, sb));
        }
        pending.push(Pending { child, recorded });
    }

    // Phase 2 — EVAL the whole generation's triples in one flattened call (up to `batch × OPPONENTS`,
    // order preserved). This is where `--jobs` now scales past `OPPONENTS`.
    let outcomes = evaluator.eval(&all_jobs)?;

    // Phase 3 — INSERT in the pinned predraw order. Splitting `outcomes` by each child's job count keeps the
    // reduce identical to the per-child path; the fixed order makes the elitism tie-break reproducible.
    let mut cursor = 0usize;
    for p in pending {
        let n = p.recorded.len();
        let slice = &outcomes[cursor..cursor + n];
        cursor += n;

        let mut scores = Vec::new();
        let mut descriptors = Vec::new();
        let mut kept: Vec<(O1, O2, u64, u64)> = Vec::new();
        for (outcome, rec) in slice.iter().zip(p.recorded) {
            match outcome {
                Some(s) => {
                    scores.push(s.score);
                    descriptors.push(select(s));
                    kept.push(rec);
                }
                None => *rejected_by_criterion += 1,
            }
        }
        if scores.is_empty() {
            continue;
        }
        let descriptor = mean_descriptor(&descriptors);
        let challenger_fitness = mean(&scores);
        population.try_insert_with_reeval(descriptor, challenger_fitness, p.child.clone(), |incumbent| {
            reeval_on_recorded(evaluator, &kept, |rec| make_job(incumbent, &rec.0, &rec.1, rec.2, rec.3))
        })?;
    }
    Ok(())
}

// ── Common-opponent re-evaluation (the Phase-5 non-stationarity fix) ────────────────────────────────
//
// `reeval_on_recorded` re-scores an incumbent on a challenger's EXACT recorded opponents and seeds, so the two
// are compared under identical conditions before `try_insert_with_reeval`'s elitism test. `None` means the
// incumbent produced no real encounter on any of them (inadmissible here) — the challenger, which did, wins.
// No fresh RNG is drawn (recorded seeds are replayed), so the run stays reproducible.
//
// SERIAL_GUARD: each `rollout` inside `score_triple_compact` acquires the non-reentrant `HARNESS_LOCK` itself
// and releases it before the next, so these sequential re-eval rollouts are safe. `search()` must therefore
// NEVER hold `serial_guard` around the generation loop — doing so (e.g. to "reuse one lock") would deadlock
// the very first re-eval on the lock the loop already holds.
//
// The `to_job` closure (supplied by `score_and_insert`'s `make_job`) drops the incumbent into whichever triple
// slot this population owns, beside the two opponents pulled from each recorded tuple.
fn reeval_on_recorded<R>(
    evaluator: &Evaluator,
    recorded: &[R],
    to_job: impl Fn(&R) -> TripleJob,
) -> Result<Option<f32>, String> {
    let jobs: Vec<TripleJob> = recorded.iter().map(to_job).collect();
    let scores: Vec<f32> = evaluator.eval(&jobs)?.into_iter().flatten().map(|s| s.score).collect();
    Ok(if scores.is_empty() { None } else { Some(mean(&scores)) })
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
    // SORT-OK: bare f32 bits about to be summed (`mean`) — ties are identical terms.
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

/// One world elite, decoded back into **all four** of its config slices — a readable RON diff of the
/// shipped world's dials (the reward-hacking guard: a human reads what the search found before it ships).
///
/// Every slice `world_genome` encodes must appear here. The rollout scores the whole [`WorldConfig`], so a
/// slice omitted from this doc is a knob the search optimised and the game can never ship — the elite's
/// reported fitness would not be reproducible from the config it bakes. (`mold` + `almond` were exactly
/// that until they were added here: 23 of 102 knobs evaluated, then dropped on write.)
#[derive(Serialize)]
pub struct WorldEliteDoc {
    pub cell: (usize, usize),
    /// The archive's axes carry the world's descriptor (`world_descriptor`): total cross-species deaths ×
    /// total cross-species lives, each normalised into `[0,1]`. `BehaviorDescriptor`'s generic
    /// `aggression`/`exploration` fields hold them respectively.
    pub total_deaths: f32,
    pub total_lives: f32,
    pub fitness: f32,
    pub ai: crate::ai::tuning::AiTuning,
    pub sim: crate::sim::SimTuning,
    pub mold: crate::mold::MoldConfig,
    /// The evolvable gameplay subset of `almond_water` (`AlmondWaterDynamics`), not the full config: the
    /// structural + visual knobs are not evolved and stay shipped.
    pub almond: crate::almond_water::AlmondWaterDynamics,
    /// The evolvable gameplay subset of `lighting` (`LightingDynamics`) — likewise not the full config.
    pub lighting: crate::light::LightingDynamics,
}

/// Decode every world elite for review/commit — each is a readable diff of the shipped world's dials.
pub fn world_archive_doc(pop: &Population<WorldGenome>) -> Result<ArchiveDoc<WorldEliteDoc>, String> {
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let g = pop.get(elite.genome).ok_or("dangling elite handle")?;
        let wc = world_genome::decode(g)?;
        elites.push(WorldEliteDoc {
            cell: *cell,
            total_deaths: elite.descriptor.aggression,
            total_lives: elite.descriptor.exploration,
            fitness: elite.fitness,
            ai: wc.ai,
            sim: wc.sim,
            mold: wc.mold,
            almond: wc.almond,
            lighting: wc.lighting,
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
        let r = rollout(brains_of(t, &squad, &swarm)?, None, None, None, seed, episode_ticks);
        prior.observe(&r.trace);
    }
    Ok(prior)
}
