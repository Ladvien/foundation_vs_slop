//! **Replayability / expressive-range** — how *different* one candidate's playthroughs are from each other.
//!
//! The other proxies (`surprise`, `interest`, `experience`) each score a *single* episode. Replayability is
//! a property of a **set** of episodes — the same candidate config run on different dungeon seeds — and it is
//! the quantity the roadmap makes a first-class objective: we want to ship a *generator* whose runs vary, not
//! one tuned point that plays the same every time.
//!
//! The measure is **Expressive Range Analysis** (Smith & Whitehead, "Analyzing the Expressive Range of a
//! Level Generator", FDG 2010, DOI 10.1145/1814256.1814260; expanded by Summerville, "Expanding Expressive
//! Range", AIIDE 2018, DOI 10.1609/aiide.v14i1.13012), applied not to static level metrics but to the
//! *experienced* shape of each run: each episode is reduced to a fixed **run signature** in the
//! interest × tone descriptor space, and a generator's replayability is the **spread** of its signatures —
//! how much of that space its runs cover. Which axes to project onto is itself a choice with consequences
//! (Withington & Tokarchuk, "The Right Variety: Improving Expressive Range Analysis with Metric Selection
//! Methods", FDG 2023, DOI 10.1145/3582437.3582453); we use the six psychology-grounded axes already
//! computed per run rather than hand-picking a pair, so no axis choice is smuggled in here.
//!
//! Spread alone is not the objective — a generator that spreads by sometimes producing *broken* runs is not
//! replayable, it is unreliable. So the objective is spread **subject to every seed clearing the behavioural
//! minimal criterion** (`surprise::minimal_criterion`): the caller gates on admission, this module measures
//! variety over the admitted set. See [`replayability_gated`].

use super::experience::Experience;
use super::interest::Interest;

/// Number of axes in a [`RunSignature`] — the three `interest` proxies plus the three `experience` proxies.
pub const SIGNATURE_AXES: usize = 6;

/// The maximum possible Euclidean distance between two signatures whose axes are each in `[0,1]`:
/// `sqrt(SIGNATURE_AXES)`. Dividing pairwise distances by this normalises [`spread`] into `[0,1]`.
fn max_distance() -> f32 {
    (SIGNATURE_AXES as f32).sqrt()
}

/// One episode reduced to a point in the interest × tone descriptor space. Every axis is in `[0,1]`, so the
/// signature lives in the unit hypercube and pairwise distances normalise cleanly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunSignature {
    pub axes: [f32; SIGNATURE_AXES],
}

impl RunSignature {
    /// Reduce a run to its signature from the per-checkpoint survival-belief series — the same cheap signal
    /// every other episode-shape proxy reads. Order: `[suspense, outcome_surprise, effectance, dread,
    /// loneliness, pacing]`.
    pub fn from_belief(belief: &[f32]) -> RunSignature {
        let i = Interest::from_belief(belief);
        let e = Experience::from_belief(belief);
        RunSignature {
            axes: [i.suspense, i.outcome_surprise, i.effectance, e.dread, e.loneliness, e.pacing],
        }
    }

    /// Euclidean distance to another signature.
    fn distance(&self, other: &RunSignature) -> f32 {
        let mut sq = 0.0f32;
        for k in 0..SIGNATURE_AXES {
            let d = self.axes[k] - other.axes[k];
            sq += d * d;
        }
        sq.sqrt()
    }
}

/// **Spread**: the mean pairwise distance between a candidate's run signatures, normalised to `[0,1]`. This
/// is the expressive-range diversity measure — `0` when every run is identical (a generator that always
/// plays the same), rising as its runs cover more of the interest × tone space. Fewer than two runs have no
/// pairwise structure, so the result is `0`.
pub fn spread(signatures: &[RunSignature]) -> f32 {
    let n = signatures.len();
    if n < 2 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    let mut pairs = 0u32;
    for a in 0..n {
        for b in (a + 1)..n {
            sum += signatures[a].distance(&signatures[b]);
            pairs += 1;
        }
    }
    let mean = sum / pairs as f32;
    (mean / max_distance()).clamp(0.0, 1.0)
}

/// The replayability **objective**: the [`spread`] of the admitted runs, but **`0` unless every seed cleared
/// the minimal criterion**. A generator that achieves variety by occasionally emitting a broken (wiped,
/// extinct, un-encountered) run is not replayable — it is unreliable — and a hard gate, not a penalty, is the
/// only robust remedy (Skalse et al., "Defining and Characterizing Reward Hacking", arXiv:2209.13085),
/// mirroring how `surprise::minimal_criterion` gates the single-episode objective.
pub fn replayability_gated(signatures: &[RunSignature], all_seeds_admitted: bool) -> f32 {
    if !all_seeds_admitted {
        return 0.0;
    }
    spread(signatures)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(axes: [f32; SIGNATURE_AXES]) -> RunSignature {
        RunSignature { axes }
    }

    #[test]
    fn identical_runs_have_no_spread() {
        // A generator that plays the same every time — the thing replayability must score near zero.
        let s = sig([0.4, 0.3, 0.5, 0.2, 0.1, 0.6]);
        assert_eq!(spread(&[s, s, s]), 0.0);
    }

    #[test]
    fn fewer_than_two_runs_is_zero() {
        assert_eq!(spread(&[]), 0.0);
        assert_eq!(spread(&[sig([0.5; SIGNATURE_AXES])]), 0.0);
    }

    #[test]
    fn more_varied_runs_spread_more() {
        let tight = [
            sig([0.50, 0.50, 0.50, 0.50, 0.50, 0.50]),
            sig([0.52, 0.48, 0.51, 0.49, 0.50, 0.51]),
            sig([0.48, 0.51, 0.49, 0.51, 0.49, 0.50]),
        ];
        let varied = [
            sig([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            sig([1.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
            sig([1.0, 0.0, 1.0, 0.0, 1.0, 0.0]),
        ];
        assert!(spread(&varied) > spread(&tight), "varied runs must out-spread near-identical ones");
    }

    #[test]
    fn spread_is_bounded_unit_interval() {
        // The two extreme corners of the hypercube are the maximum-distance pair; normalisation caps it at 1.
        let corners = [sig([0.0; SIGNATURE_AXES]), sig([1.0; SIGNATURE_AXES])];
        let s = spread(&corners);
        assert!((s - 1.0).abs() < 1e-6, "opposite corners must be maximally spread, got {s}");
        for sigs in [
            vec![sig([0.0; SIGNATURE_AXES]), sig([1.0; SIGNATURE_AXES]), sig([0.5; SIGNATURE_AXES])],
            vec![sig([0.3, 0.7, 0.1, 0.9, 0.5, 0.2]), sig([0.8, 0.2, 0.6, 0.1, 0.4, 0.9])],
        ] {
            assert!((0.0..=1.0).contains(&spread(&sigs)));
        }
    }

    #[test]
    fn a_broken_seed_zeroes_replayability() {
        // High variety means nothing if one of the runs was inadmissible — the hard gate.
        let varied = [sig([0.0; SIGNATURE_AXES]), sig([1.0; SIGNATURE_AXES])];
        assert_eq!(replayability_gated(&varied, false), 0.0, "an inadmissible seed must veto replayability");
        assert!(replayability_gated(&varied, true) > 0.0, "all-admitted keeps the variety");
    }

    #[test]
    fn signature_from_belief_packs_all_six_axes() {
        // A swingy run should populate several axes; the packing order is interest then experience.
        let belief = [1.0, 0.3, 0.8, 0.2, 0.7, 0.95];
        let s = RunSignature::from_belief(&belief);
        let i = Interest::from_belief(&belief);
        let e = Experience::from_belief(&belief);
        assert_eq!(s.axes, [i.suspense, i.outcome_surprise, i.effectance, e.dread, e.loneliness, e.pacing]);
    }
}
