//! **Co-evolutionary MAP-Elites** (feature `test-harness`) — the offline search.
//!
//! Two populations, squad and swarm, each illuminated by its own MAP-Elites archive (Mouret & Clune,
//! arXiv:1504.04909), each supplying the other's selection pressure. Neither optimises "win"; both
//! optimise **witnessed learnable-surprise** (`squad_ai::surprise`) subject to a **relational minimal
//! criterion** — a candidate is admitted only if a real encounter happened against the opponent it was
//! paired with (Wang et al., POET, arXiv:1901.01753; minimal-criterion coevolution).
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
//! # KNOWN DEFECT: fitness is non-stationary, and MAP-Elites assumes it is not
//!
//! An elite's recorded fitness is the mean of `W·S·L` over the **three opponents it happened to be paired
//! with at insertion time**, and it is never re-measured. But fitness is not a function of the genome
//! alone: `W`, `L`, and even the descriptor all depend on the opponent (a squad's `aggression` is the share
//! of combat modes, and combat modes gate on `ThreatBearingKnown` — i.e. on whether the swarm showed up).
//!
//! So `MapElitesArchive::insert`'s elitism test (`incumbent.fitness >= challenger.fitness`) compares scores
//! measured under different conditions. Mouret & Clune state the assumption their predictability argument
//! rests on: "the only thing that changes over time is the number of cells that are filled and their
//! performance" (arXiv:1504.04909) — i.e. a stationary `f(genome)`. Freezing the *prior* fixes the
//! reference of `S`; it does not make the rollout opponent-independent.
//!
//! Consequences: an early elite scored against a weak, monoculture archive can enshrine itself and lock out
//! a genuinely better later genome; the fitnesses written into the committed RON are not comparable across
//! cells.
//!
//! POET — cited above for the minimal criterion — solves exactly this with continual re-evaluation and
//! transfer (its `EVALUATE_CANDIDATES` re-runs every agent in the target environment and keeps the best).
//! This module does not implement it. The fix is to re-evaluate an incumbent against the challenger's
//! opponent set before comparing (a common-opponent comparison), at the cost of one extra rollout pair per
//! contested cell. **Until that lands, treat the archive as a set of interesting candidates to read, not as
//! a ranked optimum.**

use std::collections::HashMap;

use serde::Serialize;

use rand_chacha::ChaCha8Rng;

use crate::ai::brain::{authored_brains, BrainSource, CandidateBrains};
use crate::ai::utility::{Behavior, Mode};
use crate::rng::{seeded, DetRng};
use crate::squad_ai::role::{RoleBrains, RoleId};

use super::evaluate::rollout;
use super::genome::{decode, encode, is_feasible, is_feasible_creature, mutate, Genome};
use super::qd::{BehaviorDescriptor, MapElitesArchive};
use super::surprise::{
    fitness, minimal_criterion, EpisodeOutcome, EpisodeTrace, Fitness, ModePrior,
};

/// Mutation strength (fraction of each parameter's authored scale). Large enough to leave the authored
/// basin within a few generations, small enough that most children stay feasible.
const SIGMA: f32 = 0.25;
/// Probability that a child also transposes two behaviour ranks.
const RANK_SWAP_P: f64 = 0.15;
/// How many opponents a candidate is evaluated against, drawn from across the opponent archive.
const OPPONENTS: usize = 3;

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
#[derive(Clone, Debug, PartialEq)]
pub struct SquadGenome(pub Vec<Genome>);

/// One swarm candidate: the three creature repertoires that co-adapt as a unit. They are carried
/// together because they share a world — a scout that marks prey is only meaningful beside crabs that
/// rally on the mark.
#[derive(Clone, Debug, PartialEq)]
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
        }
    }
}

/// The result of scoring one candidate against one opponent across a rollout pair.
struct Pairing {
    fitness: Fitness,
    squad: BehaviorDescriptor,
    swarm: BehaviorDescriptor,
}

/// Evaluate one (squad, swarm) pairing: two rollouts on two different worlds. `None` when the pairing
/// fails the behavioural minimal criterion — no real encounter happened, so there is nothing to score.
fn evaluate_pair(
    t: &Templates,
    squad: &SquadGenome,
    swarm: &SwarmGenome,
    prior: &ModePrior,
    cfg: &SearchConfig,
    rng: &mut ChaCha8Rng,
) -> Result<Option<Pairing>, String> {
    let seeds = &cfg.dungeon_seeds;
    let i = rng.below(seeds.len());
    // A *different* world for the second rollout whenever the set allows one.
    let j = if seeds.len() > 1 {
        let mut j = rng.below(seeds.len() - 1);
        if j >= i {
            j += 1;
        }
        j
    } else {
        i
    };

    let a = rollout(brains_of(t, squad, swarm)?, seeds[i], cfg.episode_ticks);
    if let Err(_why) = minimal_criterion(&a.outcome) {
        return Ok(None);
    }
    let b = rollout(brains_of(t, squad, swarm)?, seeds[j], cfg.episode_ticks);
    if let Err(_why) = minimal_criterion(&b.outcome) {
        return Ok(None);
    }

    Ok(Some(Pairing {
        fitness: fitness(&a.trace, &b.trace, prior),
        squad: squad_descriptor(&a.trace, &a.outcome),
        swarm: swarm_descriptor(&a.trace),
    }))
}

/// Both archives after a run.
pub struct SearchResult {
    pub squad: Population<SquadGenome>,
    pub swarm: Population<SwarmGenome>,
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

    let mut result = SearchResult {
        squad: Population::new(cfg.resolution),
        swarm: Population::new(cfg.resolution),
        evaluations: 0,
        rejected_infeasible: 0,
        rejected_by_criterion: 0,
    };

    for generation in 0..cfg.generations {
        for _ in 0..cfg.batch {
            // ── squad child, evaluated against swarm opponents ──
            let parent = result.squad.sample_parent(&mut rng).cloned().unwrap_or_else(|| authored_squad.clone());
            let child = propose_squad(t, &parent, &mut rng, &mut result.rejected_infeasible)?;
            let opponents: Vec<SwarmGenome> = {
                let sampled = result.swarm.sample_opponents(OPPONENTS, &mut rng);
                if sampled.is_empty() {
                    vec![authored_swarm.clone()]
                } else {
                    sampled.into_iter().cloned().collect()
                }
            };
            score_and_insert_squad(t, prior, cfg, &mut rng, &mut result, &child, &opponents)?;

            // ── swarm child, evaluated against squad opponents ──
            let parent = result.swarm.sample_parent(&mut rng).cloned().unwrap_or_else(|| authored_swarm.clone());
            let child = propose_swarm(t, &parent, &mut rng, &mut result.rejected_infeasible)?;
            let opponents: Vec<SquadGenome> = {
                let sampled = result.squad.sample_opponents(OPPONENTS, &mut rng);
                if sampled.is_empty() {
                    vec![authored_squad.clone()]
                } else {
                    sampled.into_iter().cloned().collect()
                }
            };
            score_and_insert_swarm(t, prior, cfg, &mut rng, &mut result, &child, &opponents)?;
        }
        report(generation, &result);
    }
    Ok(result)
}

fn score_and_insert_squad(
    t: &Templates,
    prior: &ModePrior,
    cfg: &SearchConfig,
    rng: &mut ChaCha8Rng,
    result: &mut SearchResult,
    child: &SquadGenome,
    opponents: &[SwarmGenome],
) -> Result<(), String> {
    let mut scores = Vec::new();
    let mut descriptors = Vec::new();
    for opponent in opponents {
        // Both sides are feasible by construction (`propose_*` screens children; archive members were
        // screened when they entered). A failure here is a bug, not a candidate to skip.
        feasible(t, child, opponent)?;
        result.evaluations += 1;
        match evaluate_pair(t, child, opponent, prior, cfg, rng)? {
            Some(p) => {
                scores.push(p.fitness.score());
                descriptors.push(p.squad);
            }
            None => result.rejected_by_criterion += 1,
        }
    }
    if scores.is_empty() {
        return Ok(());
    }
    let fitness = mean(&scores);
    let descriptor = mean_descriptor(&descriptors);
    result.squad.insert(descriptor, fitness, child.clone());
    Ok(())
}

fn score_and_insert_swarm(
    t: &Templates,
    prior: &ModePrior,
    cfg: &SearchConfig,
    rng: &mut ChaCha8Rng,
    result: &mut SearchResult,
    child: &SwarmGenome,
    opponents: &[SquadGenome],
) -> Result<(), String> {
    let mut scores = Vec::new();
    let mut descriptors = Vec::new();
    for opponent in opponents {
        feasible(t, opponent, child)?;
        result.evaluations += 1;
        match evaluate_pair(t, opponent, child, prior, cfg, rng)? {
            Some(p) => {
                scores.push(p.fitness.score());
                descriptors.push(p.swarm);
            }
            None => result.rejected_by_criterion += 1,
        }
    }
    if scores.is_empty() {
        return Ok(());
    }
    let fitness = mean(&scores);
    let descriptor = mean_descriptor(&descriptors);
    result.swarm.insert(descriptor, fitness, child.clone());
    Ok(())
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
        let r = rollout(brains_of(t, &squad, &swarm)?, seed, episode_ticks);
        prior.observe(&r.trace);
    }
    Ok(prior)
}
