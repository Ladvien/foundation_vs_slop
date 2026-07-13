//! **POET** — Paired Open-Ended Trailblazer (Wang, Lehman, Clune & Stanley 2019, arXiv:1901.01753): an
//! open-ended outer loop that co-generates *environments* and the *agents* that solve them, keeping each
//! new environment in the "neither too easy nor too hard" band for the current agents (a minimal-criterion
//! coevolution) and **transferring** agents between environments so a skill learned in one seeds progress in
//! another. It is the natural home for the pieces this project already has: the world/level genome is the
//! environment, the squad genome is the agent, `surprise::minimal_criterion` is the MCC gate, and
//! `interest`/`surprise` supply the "is this pairing engaging" score.
//!
//! Two additions over the base loop:
//!
//! - **Learning-progress curriculum** (Schmidhuber's compression-progress / Oudeyer & Kaplan's
//!   learning-progress motivation): optimisation budget is allocated in proportion to each niche's *recent
//!   improvement*, so compute flows to environments still yielding progress and away from converged ones —
//!   rather than spreading uniformly and re-polishing solved niches.
//! - The admission band is scored on the pairing's **fitness**, so "not too easy / not too hard" is measured
//!   against the current best agents, exactly the relational criterion of POET §3.
//!
//! The core is generic over the environment genome `E` and agent genome `A`, with mutation and evaluation
//! supplied as closures — so it is unit-testable on a synthetic difficulty/skill problem (no simulation),
//! and the `train poet` driver instantiates it with `WorldGenome` / `SquadGenome` and a real rollout.
//! Determinism: every draw goes through the seeded [`ChaCha8Rng`], never entropy.

use rand_chacha::ChaCha8Rng;

/// Knobs for a POET run.
#[derive(Clone, Debug)]
pub struct PoetConfig {
    pub seed: u64,
    /// Outer iterations (each: optimise all niches, then periodically reproduce + transfer).
    pub iterations: u32,
    /// Cap on simultaneously active niches.
    pub max_niches: usize,
    /// Base agent proposals per niche per iteration (the learning-progress curriculum adds more on top,
    /// steered toward the niches still improving).
    pub agent_proposals: u32,
    /// Extra proposals distributed across niches in proportion to their learning progress.
    pub curriculum_bonus: u32,
    /// Attempt to spawn a new niche every this-many iterations.
    pub reproduce_every: u32,
    /// Attempt cross-niche agent transfer every this-many iterations.
    pub transfer_every: u32,
    /// Admission band (on the pairing fitness) for a *new* niche measured against the parent's best agent:
    /// below `admit_min` it is trivially solved (too easy), and an unsolvable niche returns `None` from
    /// `evaluate` (too hard). Both ends implement POET's "neither too easy nor too hard".
    pub admit_min: f32,
    pub admit_max: f32,
    /// Window (in iterations) over which a niche's learning progress is measured.
    pub lp_window: usize,
}

impl Default for PoetConfig {
    fn default() -> Self {
        PoetConfig {
            seed: 0x9_0E7_5EED,
            iterations: 40,
            max_niches: 8,
            agent_proposals: 4,
            curriculum_bonus: 8,
            reproduce_every: 3,
            transfer_every: 4,
            admit_min: 0.05,
            admit_max: 0.8,
            lp_window: 4,
        }
    }
}

/// One active (environment, agent) pairing plus its progress bookkeeping.
pub struct Niche<E, A> {
    pub env: E,
    pub agent: A,
    pub best_fitness: f32,
    pub best_interest: f32,
    /// Recent learning progress — the best-fitness gain over the last `lp_window` iterations. Drives the
    /// curriculum's budget allocation; a fresh niche starts high so it gets attention.
    pub lp: f32,
    history: Vec<f32>,
}

/// The result of a POET run: the surviving niches (each an environment + the best agent found for it) and
/// run-wide tallies for the `report` callback and the driver's summary.
pub struct PoetResult<E, A> {
    pub niches: Vec<Niche<E, A>>,
    pub created: u32,
    pub rejected: u32,
    pub transfers: u32,
    pub evaluations: u32,
}

/// Learning progress = best-fitness improvement across the last `window` recorded values (clamped ≥ 0). A
/// niche with fewer than two records is treated as maximally progressing so it is not starved before it has
/// had a chance to move.
fn learning_progress(history: &[f32], window: usize) -> f32 {
    if history.len() < 2 {
        return 1.0;
    }
    let last = history[history.len() - 1];
    let back = history[history.len() - 1 - window.min(history.len() - 1)];
    (last - back).max(0.0)
}

/// Run the POET outer loop.
///
/// - `mutate_env(&env, rng) -> env` — perturb an environment genome (feasible by construction, like the
///   flat-genome mutators).
/// - `mutate_agent(&agent, rng) -> agent` — perturb an agent genome.
/// - `evaluate(&env, &agent) -> Option<(fitness, interest)>` — run the pairing; `None` is an MCC reject
///   (unsolvable / degenerate), `Some` carries the quality fitness and the human-interest score.
/// - `report(iteration, &result)` — progress callback after each iteration.
#[allow(clippy::too_many_arguments)]
pub fn poet_search<E, A, FEM, FAM, FEV, FR>(
    cfg: &PoetConfig,
    seed_env: E,
    seed_agent: A,
    mut mutate_env: FEM,
    mut mutate_agent: FAM,
    mut evaluate: FEV,
    mut report: FR,
) -> Result<PoetResult<E, A>, String>
where
    E: Clone,
    A: Clone,
    FEM: FnMut(&E, &mut ChaCha8Rng) -> Result<E, String>,
    FAM: FnMut(&A, &mut ChaCha8Rng) -> Result<A, String>,
    FEV: FnMut(&E, &A) -> Option<(f32, f32)>,
    FR: FnMut(u32, &PoetResult<E, A>),
{
    // The reproduce/transfer cadences are used as modulo divisors below — reject zero loudly rather than
    // panicking with a divide-by-zero on the first iteration (no-panic rule).
    if cfg.reproduce_every == 0 || cfg.transfer_every == 0 {
        return Err("poet: reproduce_every and transfer_every must be > 0".to_string());
    }
    let mut rng = crate::rng::seeded(cfg.seed);

    // Seed niche — the authored environment paired with the authored agent. It must clear the MCC, else the
    // whole run has no valid starting pairing (loud failure, no degraded start).
    let (seed_f, seed_i) = evaluate(&seed_env, &seed_agent)
        .ok_or("the seed (environment, agent) pairing failed the minimal criterion")?;
    let mut result = PoetResult {
        niches: vec![Niche {
            env: seed_env,
            agent: seed_agent,
            best_fitness: seed_f,
            best_interest: seed_i,
            lp: 1.0,
            history: vec![seed_f],
        }],
        created: 0,
        rejected: 0,
        transfers: 0,
        evaluations: 1,
    };

    for iteration in 0..cfg.iterations {
        // ── 1. Optimise each niche, with a learning-progress curriculum on the extra budget ──
        let total_lp: f32 = result.niches.iter().map(|n| n.lp).sum::<f32>().max(1e-6);
        for idx in 0..result.niches.len() {
            let share = result.niches[idx].lp / total_lp;
            let budget = cfg.agent_proposals + (cfg.curriculum_bonus as f32 * share).round() as u32;
            for _ in 0..budget {
                let cand = mutate_agent(&result.niches[idx].agent, &mut rng)?;
                if let Some((f, i)) = evaluate(&result.niches[idx].env, &cand) {
                    result.evaluations += 1;
                    if f > result.niches[idx].best_fitness {
                        result.niches[idx].agent = cand;
                        result.niches[idx].best_fitness = f;
                        result.niches[idx].best_interest = i;
                    }
                }
            }
            let n = &mut result.niches[idx];
            n.history.push(n.best_fitness);
            n.lp = learning_progress(&n.history, cfg.lp_window);
        }

        // ── 2. Reproduce: spawn a new niche whose difficulty sits in the admission band ──
        if iteration % cfg.reproduce_every == 0 && result.niches.len() < cfg.max_niches {
            // Parent = the niche with the most learning progress (the frontier of what's being solved).
            let parent_idx = result
                .niches
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.lp.partial_cmp(&b.1.lp).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let child_env = mutate_env(&result.niches[parent_idx].env, &mut rng)?;
            // MCC: score the child against the parent's best agent. `None` = too hard; outside the band =
            // too easy / degenerate. Only a moderate pairing (a genuinely new challenge) is admitted.
            match evaluate(&child_env, &result.niches[parent_idx].agent) {
                Some((f, ci)) if (cfg.admit_min..=cfg.admit_max).contains(&f) => {
                    result.evaluations += 1;
                    // Seed the child's agent by transfer: the best-performing existing agent in the new
                    // environment (POET's transfer-at-birth), so progress compounds across niches. Start from
                    // the parent's own agent with its REAL fitness+interest (`f`, `ci`) from the admission
                    // eval — not a 0.0 interest placeholder — and skip it in the loop so it is not re-scored.
                    let mut best = (result.niches[parent_idx].agent.clone(), f, ci);
                    for (i, n) in result.niches.iter().enumerate() {
                        if i == parent_idx {
                            continue;
                        }
                        if let Some((tf, ti)) = evaluate(&child_env, &n.agent) {
                            result.evaluations += 1;
                            if tf > best.1 {
                                best = (n.agent.clone(), tf, ti);
                            }
                        }
                    }
                    result.niches.push(Niche {
                        env: child_env,
                        agent: best.0,
                        best_fitness: best.1,
                        best_interest: best.2,
                        lp: 1.0,
                        history: vec![best.1],
                    });
                    result.created += 1;
                }
                _ => {
                    result.evaluations += 1;
                    result.rejected += 1;
                }
            }
        }

        // ── 3. Transfer: adopt another niche's agent wherever it beats the incumbent ──
        if iteration % cfg.transfer_every == 0 && result.niches.len() > 1 {
            let agents: Vec<A> = result.niches.iter().map(|n| n.agent.clone()).collect();
            for target in 0..result.niches.len() {
                for (src, agent) in agents.iter().enumerate() {
                    if src == target {
                        continue;
                    }
                    if let Some((f, i)) = evaluate(&result.niches[target].env, agent) {
                        result.evaluations += 1;
                        if f > result.niches[target].best_fitness {
                            result.niches[target].agent = agent.clone();
                            result.niches[target].best_fitness = f;
                            result.niches[target].best_interest = i;
                            result.transfers += 1;
                        }
                    }
                }
            }
        }

        report(iteration, &result);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Synthetic difficulty/skill world: environment = difficulty d, agent = skill s. ──
    // Performance is 0.5 + 0.5·(s − d): a matched pairing scores 0.5, a comfortable win → 1, and s far below
    // d → below 0; an agent more than 0.5 below the difficulty is "wiped" (None, too hard). Interest is a
    // flow triangle peaking when s ≈ d. This is the minimal model on which POET's progression is visible.
    fn eval(d: f32, s: f32) -> Option<(f32, f32)> {
        let gap = s - d;
        if gap < -0.5 {
            return None; // too hard — the pairing fails the criterion
        }
        let fitness = (0.5 + 0.5 * gap).clamp(0.0, 1.0);
        let interest = (1.0 - gap.abs()).max(0.0);
        Some((fitness, interest))
    }

    #[test]
    fn open_endedly_grows_harder_niches_and_more_skilled_agents() {
        // The POET signature: starting from (d=0, s=0), the loop should ratchet UP — spawning harder
        // environments and, via optimisation + transfer, more skilled agents to meet them. A closed
        // (non-open-ended) search would stay pinned at the seed difficulty.
        // Mutation scales are matched so the environment keeps pace with the agent: a big agent-proposal
        // budget would let skill overshoot niche 0 in one iteration, making every child read "too easy". A
        // modest per-iteration agent budget + a decent env step is the balanced regime POET is designed for.
        let cfg = PoetConfig {
            seed: 0xABCDEF,
            iterations: 100,
            max_niches: 12,
            agent_proposals: 2,
            curriculum_bonus: 2,
            reproduce_every: 2,
            transfer_every: 3,
            admit_min: 0.1,
            admit_max: 0.85,
            lp_window: 4,
        };
        let result = poet_search(
            &cfg,
            0.0f32, // seed difficulty
            0.0f32, // seed skill
            |&d, rng| Ok(d + (super::super::genome::gaussian(rng)).abs() * 0.5), // harder-only env mutation
            |&s, rng| Ok(s + super::super::genome::gaussian(rng) * 0.3),          // agent skill drift
            |&d, &s| eval(d, s),
            |_it, _r| {},
        )
        .expect("seed pairing is valid");

        assert!(result.created > 0, "POET must have spawned at least one new niche");
        assert!(result.niches.len() > 1, "the niche set must grow, got {}", result.niches.len());
        let max_d = result.niches.iter().map(|n| n.env).fold(0.0f32, f32::max);
        assert!(max_d > 0.8, "difficulty must ratchet up open-endedly, max d = {max_d}");
        let max_s = result.niches.iter().map(|n| n.agent).fold(0.0f32, f32::max);
        assert!(max_s > 0.8, "agents must grow in skill to meet the niches, max s = {max_s}");
        assert!(result.evaluations > cfg.iterations, "a real run must have many evaluations");
    }

    #[test]
    fn a_hopeless_seed_pairing_is_rejected_loudly() {
        // Seed agent far below seed difficulty → the pairing is unsolvable (None). POET must refuse to start
        // rather than run on a degenerate seed (one path, no degraded fallback).
        let cfg = PoetConfig::default();
        let r = poet_search(&cfg, 5.0f32, 0.0f32, |&d, _| Ok(d), |&s, _| Ok(s), |&d, &s| eval(d, s), |_, _| {});
        assert!(r.is_err(), "an unsolvable seed pairing must fail loudly");
    }

    #[test]
    fn learning_progress_tracks_recent_improvement() {
        assert_eq!(learning_progress(&[0.2], 4), 1.0, "a fresh niche is maximally progressing");
        assert_eq!(learning_progress(&[0.2, 0.5, 0.7], 4), 0.5, "improvement across the window");
        assert_eq!(learning_progress(&[0.7, 0.7, 0.7], 2), 0.0, "a converged niche has no progress");
    }
}
