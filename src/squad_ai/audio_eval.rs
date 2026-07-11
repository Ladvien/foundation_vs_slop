//! Generate-and-measure evaluator for the **audio population**.
//!
//! Unlike the level population (whose fitness is a static structural metric), the audio population's
//! objective is **emergent agent behaviour**: does an acoustic-stimulus config make the swarm do something
//! a player has not seen — converge on a firefight, panic-scatter at a wider radius, hesitate at a kill?
//! So it reuses the same **witnessed-learnable-surprise** fitness (`super::surprise::fitness`, the `W·S·L`
//! of the world/brain populations, Schmidhuber's compression-progress surprise, arXiv:0812.4360) over a
//! two-rollout learnability pair, scored against the frozen baseline prior. The only thing that changes
//! between the baseline and a candidate is the installed [`AudioTuning`] — authored brains, authored world.
//!
//! One path, no fallback: an infeasible genome or a rollout that fails the minimal criterion is rejected
//! (`None`), never scored with a degraded value.

use crate::ai::brain::BrainSource;

use super::audio_genome::{self, AudioGenome};
use super::coevolve::swarm_descriptor;
use super::evaluate::rollout;
use super::surprise::{fitness, minimal_criterion, ModePrior};

/// One audio evaluation: the `W·S·L` fitness and the two MAP-Elites descriptor axes.
pub struct AudioEvaluation {
    pub fitness: f32,
    /// `(aggression, persistence)` from `swarm_descriptor` — the swarm-behaviour axes. Chosen because the
    /// emergent story the audio search illuminates is a *swarm* regime shift (scatter-from vs converge-on
    /// the din), and reusing the validated descriptor keeps the audio archive comparable to the swarm one.
    pub axes: (f32, f32),
}

/// Evaluate one audio genome. Installs the decoded [`AudioTuning`] into two authored-brain rollouts on the
/// first two (DIFFERENT) held-in seeds — different worlds so the learnability fitness measures a behaviour,
/// not a memorised map — gates each on the minimal criterion, and scores the pair against `prior`.
pub fn evaluate(
    genome: &AudioGenome,
    prior: &ModePrior,
    seeds: &[u64],
    ticks: u32,
) -> Option<AudioEvaluation> {
    // Cheap feasibility gate before the (expensive) rollouts.
    audio_genome::is_feasible(genome).ok()?;
    let audio = audio_genome::decode(genome).ok()?; // AudioTuning is Copy — installed into both rollouts.

    // The learnability pair: a mode-transition model fitted on A must predict B. Needs two different worlds.
    let seed_a = *seeds.first()?;
    let seed_b = *seeds.get(1)?;

    let a = rollout(BrainSource::Authored, None, Some(audio), seed_a, ticks);
    minimal_criterion(&a.outcome).ok()?;
    let b = rollout(BrainSource::Authored, None, Some(audio), seed_b, ticks);
    minimal_criterion(&b.outcome).ok()?;

    let d = swarm_descriptor(&a.trace);
    Some(AudioEvaluation {
        fitness: fitness(&a.trace, &b.trace, prior).score(),
        axes: (d.aggression, d.exploration),
    })
}
