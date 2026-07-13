//! **Rollout evaluator for the policy (neuroevolution) population** (feature `test-harness`).
//!
//! Sibling of [`super::behavior_eval`] / [`super::audio_eval`]: decode a [`PolicyGenome`] into a
//! [`NeuralPolicy`], run two full headless rollouts on different worlds with that learned controller
//! installed at the squad policy seam ([`rollout_with_policy`]), gate each on the behavioural
//! [`minimal_criterion`], and score the pair with the same witnessed-learnable-surprise fitness
//! ([`fitness`]) as every other population. The objective is identical — emergent, *interesting* squad
//! behaviour — only the thing being evolved (the decision layer's weights) differs.

use std::sync::Arc;

use super::coevolve::squad_descriptor;
use super::evaluate::rollout_with_policy;
use super::policy::SquadPolicy;
use super::policy_genome::{self, PolicyGenome};
use super::surprise::{fitness, minimal_criterion, ModePrior};

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
