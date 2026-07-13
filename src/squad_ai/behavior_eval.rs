//! Generate-and-measure evaluator for the **behaviour population**.
//!
//! Sibling of [`super::audio_eval`]. Like the audio and world populations, the objective is **emergent
//! agent behaviour** measured by the witnessed-learnable-surprise fitness (`super::surprise::fitness`, the
//! `W·S·L` of the world/brain/audio populations) over a two-rollout learnability pair, scored against the
//! frozen baseline prior. The only thing that changes between the baseline and a candidate is the installed
//! [`BehaviorTuning`] — authored brains, authored world, authored acoustics.
//!
//! The genome carries only a searched subset; [`super::behavior_genome::decode`] overlays it onto the
//! shipped base. That base is [`BehaviorTuning::default`], which the `behavior_default_equals_shipped_config`
//! test pins equal to the `behavior:` slice of `config.ron` — so a rollout's un-searched knobs are exactly
//! the shipped ones.
//!
//! One path, no fallback: an infeasible genome or a rollout that fails the minimal criterion is rejected
//! (`None`), never scored with a degraded value.

use crate::ai::brain::BrainSource;
use crate::behavior_tuning::BehaviorTuning;

use super::behavior_genome::{self, BehaviorGenome};
use super::coevolve::swarm_descriptor;
use super::evaluate::rollout;
use super::surprise::{fitness, minimal_criterion, ModePrior};

/// One behaviour evaluation: the `W·S·L` fitness and the two MAP-Elites descriptor axes.
pub struct BehaviorEvaluation {
    pub fitness: f32,
    /// `(aggression, persistence)` from `swarm_descriptor` — reused so the behaviour archive is comparable
    /// to the swarm/audio ones (the emergent story is a swarm regime shift).
    pub axes: (f32, f32),
}

/// Evaluate one behaviour genome. Decodes it onto the shipped base, installs the resulting
/// [`BehaviorTuning`] into two authored-brain rollouts on the first two (DIFFERENT) held-in seeds, gates
/// each on the minimal criterion, and scores the pair against `prior`.
pub fn evaluate(
    genome: &BehaviorGenome,
    prior: &ModePrior,
    seeds: &[u64],
    ticks: u32,
) -> Option<BehaviorEvaluation> {
    // The shipped base the genome's searched subset overlays onto (== the `behavior:` slice of config.ron,
    // pinned by `behavior_default_equals_shipped_config`).
    let base = BehaviorTuning::default();
    // Cheap feasibility gate before the (expensive) rollouts.
    behavior_genome::is_feasible(genome, &base).ok()?;
    let behavior = behavior_genome::decode(genome, &base).ok()?; // Copy — installed into both rollouts.

    // The learnability pair: a mode-transition model fitted on A must predict B. Needs two different worlds.
    let seed_a = *seeds.first()?;
    let seed_b = *seeds.get(1)?;

    let a = rollout(BrainSource::Authored, None, None, Some(behavior), seed_a, ticks);
    minimal_criterion(&a.outcome).ok()?;
    let b = rollout(BrainSource::Authored, None, None, Some(behavior), seed_b, ticks);
    minimal_criterion(&b.outcome).ok()?;

    let d = swarm_descriptor(&a.trace);
    Some(BehaviorEvaluation {
        fitness: fitness(&a.trace, &b.trace, prior).score(),
        axes: (d.aggression, d.exploration),
    })
}
