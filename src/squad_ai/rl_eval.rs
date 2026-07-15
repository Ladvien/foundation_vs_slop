//! **Rollout evaluator for the policy (neuroevolution) population** (feature `test-harness`).
//!
//! Sibling of [`super::behavior_eval`] / [`super::audio_eval`]: decode a [`PolicyGenome`] into a
//! [`NeuralPolicy`], run two full headless rollouts on different worlds with that learned controller
//! installed at the squad policy seam ([`rollout_with_policy`]), gate each on the behavioural
//! [`minimal_criterion`], and score the pair with the same witnessed-learnable-surprise fitness
//! ([`fitness`]) as every other population. The objective is identical — emergent, *interesting* squad
//! behaviour — only the thing being evolved (the decision layer's weights) differs.

use std::sync::Arc;

use crate::ai::utility::MODE_COUNT;
use crate::config::WorldConfig;

use super::coevolve::squad_descriptor;
use super::evaluate::rollout_with_policy;
use super::fairness::{mode_concentration, survival_competence};
use super::policy::SquadPolicy;
use super::policy_genome::{self, PolicyGenome};
use super::surprise::{fitness, minimal_criterion, ActorKind, EpisodeTrace, ModePrior};

/// One policy genome's score: the fitness scalar plus the two MAP-Elites descriptor axes (the squad's
/// aggression × exploration, so the archive illuminates a range of playstyles, exactly as the squad
/// brain population does).
pub struct PolicyEvaluation {
    pub fitness: f32,
    pub axes: (f32, f32),
}

/// Evaluate a policy genome. `None` (a rejected candidate) when it is infeasible, fails to decode, or
/// either rollout fails the minimal criterion — one path, never a degraded fallback score.
pub fn evaluate(
    genome: &PolicyGenome,
    prior: &ModePrior,
    seeds: &[u64],
    ticks: u32,
) -> Option<PolicyEvaluation> {
    policy_genome::is_feasible(genome).ok()?;
    // Decode ONCE. `NeuralPolicy` is `Clone`, so the per-rollout factory clones the decoded net rather than
    // re-decoding — no repeated parse, and no `expect`/panic inside the factory (the one-path/no-panic rule).
    let policy = policy_genome::decode(genome).ok()?;
    let seed_a = *seeds.first()?;
    let seed_b = *seeds.get(1)?;

    let pa = policy.clone();
    let a = rollout_with_policy(
        Arc::new(move || Box::new(pa.clone()) as Box<dyn SquadPolicy>),
        None,
        None,
        None,
        seed_a,
        ticks,
    );
    minimal_criterion(&a.outcome).ok()?;

    let pb = policy.clone();
    let b = rollout_with_policy(
        Arc::new(move || Box::new(pb.clone()) as Box<dyn SquadPolicy>),
        None,
        None,
        None,
        seed_b,
        ticks,
    );
    minimal_criterion(&b.outcome).ok()?;

    // The squad descriptor (aggression × exploration) is the natural niche axis for a learned squad
    // controller — the same axes the authored squad brain population is illuminated over.
    let d = squad_descriptor(&a.trace, &a.outcome);
    Some(PolicyEvaluation { fitness: fitness(&a.trace, &b.trace, prior).score(), axes: (d.aggression, d.exploration) })
}

/// Squad-mode usage histogram (length [`MODE_COUNT`]) over a trace's **unit** decisions — the input to
/// `fairness::mode_concentration`. Creature decisions are excluded: exploitability is about how the *squad*
/// wins.
pub fn unit_mode_histogram(trace: &EpisodeTrace) -> [u32; MODE_COUNT] {
    let mut counts = [0u32; MODE_COUNT];
    for d in &trace.decisions {
        if matches!(d.context.actor, ActorKind::Role(_)) {
            counts[d.mode.index()] += 1;
        }
    }
    counts
}

/// One playtester genome's exploit signal on a config: the **competence** it achieves (mean survival
/// fraction across the seeds — the search's fitness, maximise it to find the strongest play) and the
/// **strategy concentration** of that play (Herfindahl over the squad mode histogram). Together they feed
/// `fairness::exploitability`.
pub struct PlaytesterEvaluation {
    /// Mean survival fraction across the seeds — the difficulty gauge the search maximises.
    pub competence: f32,
    /// Herfindahl concentration of the squad's mode usage — `1` = one dominant tactic.
    pub concentration: f32,
}

/// Evaluate a **playtester** policy: install the learned controller, run one rollout per seed against
/// `config`, and report how well it kept the squad alive and how concentrated its play was.
///
/// Unlike [`evaluate`], the objective is *competence* (survival), not witnessed-learnable-surprise — this is
/// the agent whose job is to *beat* the config, so that its best achievable play measures difficulty and its
/// style measures exploitability. It deliberately does **not** gate on `minimal_criterion`: a config a strong
/// player trivially survives is exactly the exploit we want surfaced, not discarded. `None` only when the
/// genome is infeasible / fails to decode, or no seed was supplied — one path, no degraded fallback score.
pub fn evaluate_playtester(
    genome: &PolicyGenome,
    config: Option<WorldConfig>,
    seeds: &[u64],
    ticks: u32,
) -> Option<PlaytesterEvaluation> {
    policy_genome::is_feasible(genome).ok()?;
    let policy = policy_genome::decode(genome).ok()?;
    if seeds.is_empty() {
        return None;
    }

    let mut competence_sum = 0.0f32;
    let mut hist = [0u32; MODE_COUNT];
    for &seed in seeds {
        let p = policy.clone();
        let r = rollout_with_policy(
            Arc::new(move || Box::new(p.clone()) as Box<dyn SquadPolicy>),
            config.clone(),
            None,
            None,
            seed,
            ticks,
        );
        competence_sum += survival_competence(r.outcome.survivors, r.outcome.squad_size);
        let seed_hist = unit_mode_histogram(&r.trace);
        for (h, c) in hist.iter_mut().zip(seed_hist.iter()) {
            *h += *c;
        }
    }
    Some(PlaytesterEvaluation {
        competence: competence_sum / seeds.len() as f32,
        concentration: mode_concentration(&hist),
    })
}
