//! **The research economy** (Push 4) — turning a captured specimen into knowledge.
//!
//! A capture banks a `containment::Specimen`. This is what makes that worth having: the anomaly has
//! **hidden parameters** the player does not know, research is the act of reducing uncertainty about
//! them, and a completed posterior is what pays out as an unlock.
//!
//! ## Two things kept deliberately separate
//!
//! This is **not** `knowledge::Belief` (Push 10, not yet built). A posterior is what the *Foundation*
//! knows about a captured specimen — institutional, objective, and it converges on the truth. A belief
//! is what one *operative* thinks about a kind of thing — personal, transmissible, and it can be wrong.
//! Collapsing them would lose the whole point of O-5, where false hearsay is the antagonist's weapon.
//!
//! ## Grounding
//!
//! Experiment selection is **greedy information gain**, and the form used here is exactly the one
//! Tiwari, Radhakrishna, Gulwani & Perelman give (*Information-theoretic User Interaction*,
//! DOI 10.48550/arXiv.2006.12638): with the chain rule `En(Pr(bb | q)) = En(Pr(bb)) − En(Pr(q))`, the
//! greedily-best question is `argmax_q En(Pr(q))` — or in their words, *"to greedily seek knowledge, we
//! should ask the question about which we know the least."*
//!
//! That identity is why [`Experiment::expected_information_gain`] scores the entropy of an experiment's
//! **predicted answer distribution** rather than simulating each outcome and averaging the posterior
//! entropies. The two are equal, and the cheap one is also the legible one: "how unsure am I what this
//! test will say" is something the HUD can state in words.
//!
//! ## Determinism
//!
//! Pure `f32` math over fixed-width arrays; no ECS, no RNG, no `App`. Every function here is a pure
//! function of its inputs, so it is unit-testable without a harness and cannot introduce an ordering
//! hazard. The ECS layer that will drive it (FVS-E-4's `Researched` hook) is not built yet.

use serde::{Deserialize, Serialize};

pub mod pacing;
pub mod posterior;

pub use pacing::{felt_value, reveal_schedule, schedule_is_front_loaded, ExperimentFatigue};
pub use posterior::{HiddenParam, ResearchPosterior, PARAM_COUNT};

/// A test the researchers can run on a held specimen.
///
/// Deliberately data, not a trait object: the whole set is small, authored, and must be rankable
/// cheaply every frame the research HUD is open.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    /// Player-facing name — this is read aloud in the records office.
    pub name: String,
    /// Which hidden parameter this test bears on.
    pub param: HiddenParam,
    /// How sharply the test discriminates, in `[0,1]`.
    ///
    /// `1.0` is a perfect test: its answer identifies the parameter's value outright. `0.0` is a test
    /// that tells you nothing, and is rejected by [`Self::validate`] — an experiment that cannot inform
    /// is a content bug, not a weak option, and offering it would make the EIG ranking a lie.
    pub reliability: f32,
}

impl Experiment {
    /// Reject a malformed authored experiment. One path, no fallback.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("experiment has no name".into());
        }
        if !(self.reliability.is_finite() && self.reliability > 0.0 && self.reliability <= 1.0) {
            return Err(format!(
                "experiment '{}': reliability must be in (0,1], got {}",
                self.name, self.reliability
            ));
        }
        Ok(())
    }

    /// **Expected information gain**, in bits.
    ///
    /// Per the chain-rule identity above this is the entropy of the *answer* distribution: the test we
    /// are least able to predict is the one that teaches us most. A perfectly reliable test's answer
    /// distribution is the posterior itself; an unreliable one blurs toward 50/50, which *raises* raw
    /// answer entropy for the wrong reason — so the blur is applied to the belief first and the result
    /// scaled by reliability, keeping "a coin-flip test is worthless" true.
    pub fn expected_information_gain(&self, posterior: &ResearchPosterior) -> f32 {
        if posterior.is_revealed(self.param) {
            // Nothing left to learn. Not merely low-value: offering a resolved question would make the
            // ranking read as broken to a player who can see the answer already.
            return 0.0;
        }
        posterior.entropy(self.param) * self.reliability
    }
}

/// Rank experiments most-informative first.
///
/// **Returns indices, not references**, so the caller keeps ownership of its authored list and the HUD
/// can render in this order without cloning strings every frame.
///
/// The sort is **total**: ties break on the authored index, so two equally-informative experiments are
/// always offered in the same order. That matters more than it looks — an unstable order would make the
/// top-ranked suggestion flicker between frames while the player is reading it.
pub fn rank_by_information_gain(
    experiments: &[Experiment],
    posterior: &ResearchPosterior,
) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..experiments.len()).collect();
    // SORT-OK: the input is an authored list, never an ECS query, and the key is
    // `(−EIG bits, authored index)` — total by construction because the index is unique.
    idx.sort_by(|&a, &b| {
        let ga = experiments[a].expected_information_gain(posterior);
        let gb = experiments[b].expected_information_gain(posterior);
        gb.total_cmp(&ga).then(a.cmp(&b))
    });
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exp(name: &str, param: HiddenParam, reliability: f32) -> Experiment {
        Experiment { name: name.into(), param, reliability }
    }

    #[test]
    fn the_most_informative_experiment_is_the_one_we_can_least_predict() {
        // The paper's rule, stated as a test: "ask the question about which we know the least".
        let mut p = ResearchPosterior::unknown();
        // Sharpen Lethality so we are nearly certain about it, and leave Contagion wide open.
        for _ in 0..6 {
            p.observe(HiddenParam::Lethality, true, 0.9);
        }
        let xs = [
            exp("necropsy", HiddenParam::Lethality, 1.0),
            exp("culture", HiddenParam::Contagion, 1.0),
        ];
        let ranked = rank_by_information_gain(&xs, &p);
        assert_eq!(
            xs[ranked[0]].param,
            HiddenParam::Contagion,
            "the open question must outrank the nearly-settled one"
        );
    }

    #[test]
    fn a_resolved_parameter_offers_no_information() {
        let mut p = ResearchPosterior::unknown();
        p.reveal(HiddenParam::Lethality);
        let x = exp("necropsy", HiddenParam::Lethality, 1.0);
        assert_eq!(
            x.expected_information_gain(&p),
            0.0,
            "a question the player can already see the answer to must rank last, not merely low"
        );
    }

    #[test]
    fn an_unreliable_test_is_worth_less_than_a_sharp_one_on_the_same_question() {
        // The failure this guards: scoring raw answer entropy makes a coin-flip test look MAXIMALLY
        // informative, because its answer is the hardest to predict. Reliability has to scale the gain.
        let p = ResearchPosterior::unknown();
        let sharp = exp("assay", HiddenParam::Lethality, 1.0);
        let vague = exp("hunch", HiddenParam::Lethality, 0.2);
        assert!(
            sharp.expected_information_gain(&p) > vague.expected_information_gain(&p),
            "a sharper test on the same question must rank higher"
        );
    }

    #[test]
    fn the_ranking_is_total_so_the_top_suggestion_cannot_flicker() {
        // Two identical experiments on different-but-equally-open params: the order must be stable, or
        // the HUD's top row swaps under the player's cursor between frames.
        let p = ResearchPosterior::unknown();
        let xs = [
            exp("a", HiddenParam::Lethality, 1.0),
            exp("b", HiddenParam::Contagion, 1.0),
            exp("c", HiddenParam::CaptureBasin, 1.0),
        ];
        let first = rank_by_information_gain(&xs, &p);
        for _ in 0..8 {
            assert_eq!(rank_by_information_gain(&xs, &p), first, "the ranking must be deterministic");
        }
    }

    #[test]
    fn a_malformed_experiment_is_rejected_at_the_door() {
        assert!(exp("", HiddenParam::Lethality, 1.0).validate().is_err());
        assert!(exp("x", HiddenParam::Lethality, 0.0).validate().is_err(), "a test that cannot inform");
        assert!(exp("x", HiddenParam::Lethality, 1.5).validate().is_err());
        assert!(exp("x", HiddenParam::Lethality, f32::NAN).validate().is_err());
        assert!(exp("x", HiddenParam::Lethality, 0.5).validate().is_ok());
    }
}
