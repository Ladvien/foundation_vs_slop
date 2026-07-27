//! **`ResearchPosterior`** (FVS-E-1) — what the Foundation believes about one captured specimen.
//!
//! A belief over each hidden parameter, plus the fog-of-war reveal set the stat sheet reads.
//!
//! ## Why a Bernoulli per parameter and not something richer
//!
//! Every hidden parameter here is a *question the player asks in words*: is it lethal, is it
//! contagious, can it be contained by observation. A yes/no belief with a confidence is the smallest
//! representation that answers those legibly, and legibility is the requirement — FVS-L-2 has to render
//! this, and "68% lethal" is a sentence while a Dirichlet over five categories is a diagram.
//!
//! When a parameter genuinely needs more than two answers, the right move is to *split it into more
//! parameters*, not to widen this type: two binary questions are two things the player can research and
//! two things the HUD can state, which is strictly better than one question with a shrug.
//!
//! ## The distinction from `knowledge::Belief` (Push 10)
//!
//! Superficially similar, deliberately not shared. **A posterior converges on the truth**: it is
//! institutional, it is updated only by evidence, and 0.5 here honestly means "the evidence is
//! balanced". **A belief can be wrong**, is personal to an operative, and — per [EPISTEMIC] and the
//! Fisher argument the backlog quotes — must distinguish *no belief* from *an uncertain belief*, which
//! is why that type will use `Option` and this one does not need to. A specimen on the slab always has
//! a posterior; an operative may simply never have heard of the thing.

use bevy::prelude::Component;
use serde::{Deserialize, Serialize};

/// The hidden parameters research can resolve.
///
/// Append-only: the reveal set is a bitset keyed on `as_index`, so reordering these would silently
/// reinterpret a saved posterior once FVS-G-2 lands. Same discipline as `squad_ai`'s `ActorKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub enum HiddenParam {
    /// Will it kill an operative who engages it?
    Lethality,
    /// Does it spread — to operatives, to other anomalies?
    Contagion,
    /// Which stigmergy basin contains it? Resolving this is what makes a rule legible.
    CaptureBasin,
    /// Does it get stronger, or produce more of itself, if left alone?
    Proliferation,
}

impl HiddenParam {
    pub const ALL: [HiddenParam; 4] = [
        HiddenParam::Lethality,
        HiddenParam::Contagion,
        HiddenParam::CaptureBasin,
        HiddenParam::Proliferation,
    ];

    pub fn as_index(self) -> usize {
        match self {
            HiddenParam::Lethality => 0,
            HiddenParam::Contagion => 1,
            HiddenParam::CaptureBasin => 2,
            HiddenParam::Proliferation => 3,
        }
    }
}

/// How many parameters a posterior tracks.
pub const PARAM_COUNT: usize = HiddenParam::ALL.len();

/// Confidence at which a parameter counts as **resolved** and reveals itself on the stat sheet.
///
/// Not 1.0: Bayesian updating approaches certainty asymptotically, so a threshold of 1.0 is a reveal
/// that never fires. 0.9 is "the Foundation will write this down as fact".
pub const REVEAL_AT: f32 = 0.9;

/// The Foundation's belief about one captured specimen.
///
/// `belief[i]` is `P(param_i is true)`. Starts at `0.5` — maximum entropy, no evidence either way.
/// A `Component`, and it rides the **`Specimen`** entity — which carries no `Transform` and no
/// `Health`, so attaching it cannot reach `sim_harness::snapshot_hash`. That property is why the
/// posterior can be harness-visible without touching the pinned core.
#[derive(Component, Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct ResearchPosterior {
    belief: [f32; PARAM_COUNT],
    /// Bitset of parameters written up as fact. A parameter is revealed once, and never un-revealed —
    /// the records office does not retract, it supersedes.
    revealed: u32,
}

impl Default for ResearchPosterior {
    fn default() -> Self {
        Self::unknown()
    }
}

impl ResearchPosterior {
    /// A fresh capture: everything at maximum entropy, nothing revealed.
    pub fn unknown() -> Self {
        Self { belief: [0.5; PARAM_COUNT], revealed: 0 }
    }

    /// `P(param is true)`.
    pub fn belief(&self, param: HiddenParam) -> f32 {
        self.belief[param.as_index()]
    }

    /// Has this parameter been written up as fact?
    pub fn is_revealed(&self, param: HiddenParam) -> bool {
        self.revealed & (1 << param.as_index()) != 0
    }

    /// Force a reveal (used by tests and by a completed arc).
    pub fn reveal(&mut self, param: HiddenParam) {
        self.revealed |= 1 << param.as_index();
    }

    /// Shannon entropy of one parameter, in **bits**: `1.0` at total uncertainty, `0.0` at certainty.
    pub fn entropy(&self, param: HiddenParam) -> f32 {
        let p = self.belief(param);
        binary_entropy(p)
    }

    /// Total remaining uncertainty across every unresolved parameter, in bits.
    ///
    /// The completion metric: research is finished when this reaches zero. Summing rather than
    /// averaging on purpose — adding a fifth parameter should make a specimen take *longer* to research,
    /// not dilute the average and make it look nearly done.
    pub fn total_entropy(&self) -> f32 {
        HiddenParam::ALL
            .iter()
            .filter(|p| !self.is_revealed(**p))
            .map(|p| self.entropy(*p))
            .sum()
    }

    /// Is every parameter resolved?
    pub fn is_complete(&self) -> bool {
        HiddenParam::ALL.iter().all(|p| self.is_revealed(*p))
    }

    /// Fold in one experimental result.
    ///
    /// Standard Bayesian update for a noisy binary observation: with reliability `r`, a positive result
    /// has likelihood `r` under "true" and `1 − r` under "false".
    ///
    /// **Reliability is clamped below 1.0 on purpose.** At exactly 1.0 a single observation drives the
    /// belief to 0 or 1, and a subsequent contradicting result would then divide by zero — a specimen
    /// that produced one anomalous reading would poison its own record permanently. Keeping evidence
    /// short of absolute means the posterior can always be argued with, which is both numerically safe
    /// and the right epistemics for an organisation that writes things down.
    pub fn observe(&mut self, param: HiddenParam, result: bool, reliability: f32) {
        if self.is_revealed(param) || !reliability.is_finite() {
            return;
        }
        let r = reliability.clamp(0.0, 0.999);
        let prior = self.belief(param);
        let (l_true, l_false) = if result { (r, 1.0 - r) } else { (1.0 - r, r) };
        let num = prior * l_true;
        let denom = num + (1.0 - prior) * l_false;
        if denom <= 0.0 || !denom.is_finite() {
            return; // impossible evidence; leave the belief untouched rather than write a NaN
        }
        let posterior = (num / denom).clamp(0.0, 1.0);
        self.belief[param.as_index()] = posterior;
        if posterior >= REVEAL_AT || posterior <= 1.0 - REVEAL_AT {
            self.reveal(param);
        }
    }
}

/// Shannon entropy of a Bernoulli, in bits. `0` at `p ∈ {0,1}`, `1` at `p = 0.5`.
fn binary_entropy(p: f32) -> f32 {
    if p <= 0.0 || p >= 1.0 {
        return 0.0;
    }
    -(p * p.log2() + (1.0 - p) * (1.0 - p).log2())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_capture_is_maximally_uncertain_and_reveals_nothing() {
        let p = ResearchPosterior::unknown();
        for q in HiddenParam::ALL {
            assert_eq!(p.belief(q), 0.5);
            assert!(!p.is_revealed(q));
            assert!((p.entropy(q) - 1.0).abs() < 1.0e-5, "0.5 must be exactly one bit");
        }
        assert!(!p.is_complete());
        assert!((p.total_entropy() - PARAM_COUNT as f32).abs() < 1.0e-4);
    }

    #[test]
    fn evidence_moves_the_belief_and_reduces_uncertainty() {
        let mut p = ResearchPosterior::unknown();
        let before = p.entropy(HiddenParam::Lethality);
        p.observe(HiddenParam::Lethality, true, 0.8);
        assert!(p.belief(HiddenParam::Lethality) > 0.5, "a positive result must raise the belief");
        assert!(p.entropy(HiddenParam::Lethality) < before, "and must reduce uncertainty");
    }

    #[test]
    fn contradicting_evidence_can_still_move_a_confident_belief() {
        // THE numerical trap this type is built around: at reliability 1.0 the update drives the belief
        // to exactly 0 or 1, and the next contradicting observation divides by zero. One anomalous
        // reading would poison the record forever. `observe` clamps below 1.0 so a posterior can always
        // be argued with.
        let mut p = ResearchPosterior::unknown();
        for _ in 0..12 {
            p.observe(HiddenParam::Contagion, true, 1.0);
        }
        let confident = p.belief(HiddenParam::Contagion);
        assert!(confident.is_finite(), "belief must never become NaN, got {confident}");
        // It reveals on the way up, and a revealed parameter is settled — the records do not retract.
        assert!(p.is_revealed(HiddenParam::Contagion));
        p.observe(HiddenParam::Contagion, false, 0.9);
        assert_eq!(p.belief(HiddenParam::Contagion), confident, "a revealed parameter is final");
    }

    #[test]
    fn a_negative_result_resolves_just_as_well_as_a_positive_one() {
        // "It is definitely NOT contagious" is knowledge. An implementation that only reveals on high
        // belief would leave a fully-researched harmless specimen looking permanently unfinished.
        let mut p = ResearchPosterior::unknown();
        for _ in 0..8 {
            p.observe(HiddenParam::Contagion, false, 0.85);
        }
        assert!(p.belief(HiddenParam::Contagion) < 0.1);
        assert!(p.is_revealed(HiddenParam::Contagion), "certainty of absence is still certainty");
    }

    #[test]
    fn research_completes_only_when_every_parameter_is_resolved() {
        let mut p = ResearchPosterior::unknown();
        for q in HiddenParam::ALL {
            assert!(!p.is_complete());
            p.reveal(q);
        }
        assert!(p.is_complete());
        assert_eq!(p.total_entropy(), 0.0, "a finished posterior has no uncertainty left to spend");
    }

    #[test]
    fn total_entropy_sums_rather_than_averages() {
        // Adding a parameter must make a specimen take LONGER, not dilute the average into looking
        // nearly done. Pins the choice so a later "normalise this" does not quietly invert it.
        let mut p = ResearchPosterior::unknown();
        let all_open = p.total_entropy();
        p.reveal(HiddenParam::Lethality);
        assert!(
            (all_open - p.total_entropy() - 1.0).abs() < 1.0e-4,
            "resolving one parameter must remove exactly its bit"
        );
    }

    #[test]
    fn observing_a_revealed_parameter_is_a_no_op() {
        let mut p = ResearchPosterior::unknown();
        p.reveal(HiddenParam::Lethality);
        p.observe(HiddenParam::Lethality, true, 0.9);
        assert_eq!(p.belief(HiddenParam::Lethality), 0.5, "settled findings are not re-litigated");
    }
}
