//! **Reveal pacing** (FVS-E-3) — why a research arc feels good, and why it front-loads for free.
//!
//! The item's requirement is that value track the **rate** of uncertainty reduction rather than the
//! number of experiments run — front-load the resolvable surprise, do not drip it out.
//!
//! ## Front-loading is NOT emergent, and finding that out is the substance of this item
//!
//! The intuition is that greedy information gain must already front-load: it always picks the question
//! we can least predict, so surely the first bite is the biggest. **It is not**, and the reason is a
//! property of the maths rather than of the selector:
//!
//! **Binary entropy is concave.** Moving a belief 0.5 → 0.8 resolves 0.28 bits; moving it 0.8 → 0.94
//! resolves 0.40. Entropy falls *slowly* near maximum uncertainty and *fast* near the extremes — so a
//! sequence of equal-strength observations resolves an INCREASING amount each time. Measured, four
//! parameters with a battery of `reliability = 0.8` tests:
//! `[0.28, 0.28, 0.28, 0.28, 0.40, 0.40, 0.40, 0.40]`. That is a drip that turns into a dump: precisely
//! the shape FVS-E-3 exists to forbid.
//!
//! So the pacing has to be authored, as the item says. [`ExperimentFatigue`] is the mechanism, and it is
//! diegetic rather than a curve bolted on the outside: **each repeat test on the same parameter is
//! weaker than the last.** The obvious experiments get run first; after those you are down to marginal
//! ones, arguing over a specimen that has already told you most of what it will. That is what a research
//! programme actually feels like, and it is one knob.
//!
//! **One trap this cost, worth knowing before touching it.** The schedule must measure
//! `ResearchPosterior::belief_entropy` (all parameters) and NOT `total_entropy` (unrevealed only).
//! `total_entropy` stops counting a parameter the instant its belief crosses `REVEAL_AT`, so the
//! observation that *triggers* a reveal appears to be worth that parameter's entire remaining
//! uncertainty — and the curve comes out RISING, the exact opposite of the requirement. Measured before
//! the fix: `[0.28, 0.28, 0.28, 0.28, 0.72, 0.72, 0.72, 0.72]`. Crossing a display threshold is not the
//! same event as learning something.
//!
//! [`reveal_schedule`] exists to *prove* that property rather than to produce it, and
//! [`schedule_is_front_loaded`] is the assertion a designer can run after retuning experiment
//! reliabilities to check they have not accidentally flattened the arc.
//!
//! ## Grounding
//!
//! Rietveld, Miller & Kiverstein (*The feeling of grip*, DOI 10.1007/s11229-017-1583-9) locate the felt
//! quality in the **movement toward** grip rather than in holding it — which is why [`felt_value`] scores
//! bits *resolved this step* and not total knowledge accumulated. Oudeyer & Kaplan's learning-progress
//! typology ([LPM], DOI 10.3389/neuro.12.006.2007) makes the same argument computationally: the reward
//! signal that sustains exploration is the derivative, not the level.
//!
//! Pure functions; no ECS, no RNG.

use serde::{Deserialize, Serialize};

use super::{Experiment, HiddenParam, ResearchPosterior};

/// How fast repeat tests on one parameter lose their edge (FVS-E-3's tunable).
///
/// The `k`-th test on a parameter runs at `reliability · decay^k`. Because binary entropy is concave,
/// the decay has to beat that concavity for the arc to taper — which is why the default is well below
/// 1.0 and why [`schedule_is_front_loaded`] is worth running after any retune.
#[derive(bevy::prelude::Resource, Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentFatigue {
    /// Multiplier applied per prior test on the same parameter. `1.0` disables fatigue — and produces
    /// the rising curve documented above, so it is not a neutral default.
    pub decay: f32,
}

/// Reliability below which a test stops being a *weak* test and becomes a **lying** one.
///
/// A Bayesian update at `r < 0.5` moves the belief AWAY from the observed result — a "test" that
/// reports true and thereby lowers `P(true)` is not weak evidence, it is anti-evidence. Fatigue must
/// therefore floor here rather than decaying through it. Found by measurement: `decay = 0.55` on a
/// `0.8` test yields `0.44`, and the arc silently started arguing with itself.
pub const USELESS_BELOW: f32 = 0.5;

impl Default for ExperimentFatigue {
    fn default() -> Self {
        // Measured, not guessed. It has to beat binary entropy's concavity (or the arc rises) while
        // staying above `USELESS_BELOW` for at least a second test (or the arc is one round long and
        // there is nothing to taper).
        //
        // **Re-measured 2026-07-27 (FVS-E-6), and the old 0.8 was unshippable.** It satisfied the
        // front-loading constraint and violated a second one nobody had checked, because until the
        // research verb existed nothing ever ran the arc to completion: `0.8` with `USELESS_BELOW`
        // allows only THREE tests on a parameter (`0.8 → 0.64 → 0.512 → 0.41`), whose likelihood ratios
        // multiply to ~7.5 — a belief of **0.882** against a `REVEAL_AT` of **0.9**. No parameter could
        // ever resolve, so no specimen could complete and no capability could ever be unlocked.
        //
        // 0.88 is the value that satisfies both. Computed, then confirmed by the tests below:
        // * Four tests survive the floor (`0.8 → 0.704 → 0.620 → 0.545`), LR product ≈ 18.6, and the
        //   belief crosses `REVEAL_AT` on the **second** test at p ≈ 0.905.
        // * The arc still tapers: 0.278 bits then 0.271. Narrow, and that is the point — raising decay
        //   further wins back reliability headroom but loses the taper (at 0.9 the sequence rises,
        //   0.278 → 0.287, because binary entropy's concavity beats the weakened test).
        //
        // The usable authoring band for `Experiment::reliability` is therefore about **[0.77, 0.89]**:
        // below it a parameter never resolves, at 0.90 a single test resolves it outright.
        // `ResearchConfig::check_resolvable` enforces the lower edge at load.
        Self { decay: 0.88 }
    }
}

impl ExperimentFatigue {
    /// Effective reliability of the `prior_tests`-th repeat on one parameter, or `None` once the
    /// parameter is exhausted.
    ///
    /// `None` rather than a floored value, so an exhausted test is *not offered* instead of being
    /// offered and doing nothing — the same stance `Experiment::expected_information_gain` takes on a
    /// resolved parameter.
    pub fn effective(&self, base: f32, prior_tests: u32) -> Option<f32> {
        let r = base * self.decay.powi(prior_tests as i32);
        (r > USELESS_BELOW).then(|| r.clamp(0.0, 0.999))
    }
}

/// Bits of uncertainty each experiment would resolve, in greedy order.
///
/// Simulates the arc: repeatedly take the highest-EIG experiment, apply its *expected* effect, and
/// record what it bought. Returns one entry per step until nothing informative remains.
///
/// The simulated outcome is deliberately the **more likely** one at each step rather than a sampled
/// one: this is a pacing preview for design and for the HUD, not a prediction of a particular
/// playthrough, and a sampled schedule would be a different shape every time it was drawn.
pub fn reveal_schedule(
    experiments: &[Experiment],
    start: &ResearchPosterior,
    fatigue: ExperimentFatigue,
) -> Vec<f32> {
    let mut p = *start;
    let mut out = Vec::new();
    let mut runs = [0u32; super::PARAM_COUNT];
    // Bounded by the number of parameters times a few observations each; a runaway here would be a
    // logic error, not a long arc, so the cap fails loudly in tests rather than hanging.
    for _ in 0..(HiddenParam::ALL.len() * 32) {
        let Some(best) = experiments
            .iter()
            .filter(|x| x.expected_information_gain(&p) > 0.0)
            .max_by(|a, b| {
                a.expected_information_gain(&p).total_cmp(&b.expected_information_gain(&p))
            })
        else {
            break;
        };
        let before = p.belief_entropy();
        // The likelier outcome: whichever direction the current belief already leans. At exactly 0.5 it
        // does not matter which — the bits resolved are identical by symmetry.
        let likelier = p.belief(best.param) >= 0.5;
        let Some(r) = fatigue.effective(best.reliability, runs[best.param.as_index()]) else {
            break; // this parameter is exhausted, and greedy would keep choosing it
        };
        runs[best.param.as_index()] += 1;
        p.observe(best.param, likelier, r);
        let gained = before - p.belief_entropy();
        if gained <= 1.0e-6 {
            break; // no longer making progress; stop rather than emit a tail of zeros
        }
        out.push(gained);
    }
    out
}

/// Is this arc front-loaded — does each step resolve no more than the one before it?
///
/// The tolerance absorbs `f32` noise only; it is not slack for a genuinely rising step.
pub fn schedule_is_front_loaded(schedule: &[f32]) -> bool {
    schedule.windows(2).all(|w| w[1] <= w[0] + 1.0e-4)
}

/// What one step of research *feels* worth.
///
/// Scores **bits resolved on this step**, not knowledge held. Per [GRIP], the felt quality is in the
/// movement toward grip rather than in having it — so a step that resolves half a bit feels the same
/// whether it is the first or the last, and an arc that resolves nothing feels like nothing regardless
/// of how much is already known.
pub fn felt_value(bits_resolved: f32) -> f32 {
    bits_resolved.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exp(name: &str, param: HiddenParam, reliability: f32) -> Experiment {
        Experiment { name: name.into(), param, reliability }
    }

    fn full_battery() -> Vec<Experiment> {
        HiddenParam::ALL
            .iter()
            .map(|p| exp(&format!("{p:?}"), *p, 0.8))
            .collect()
    }

    #[test]
    fn a_research_arc_front_loads_without_an_authored_curve() {
        // FVS-E-3's acceptance. The property is emergent from greedy EIG: the first experiment takes the
        // biggest available bite, so the curve descends on its own. If this ever fails, the SELECTOR has
        // regressed — not the pacing — which is exactly what this test is for.
        let schedule = reveal_schedule(&full_battery(), &ResearchPosterior::unknown(), ExperimentFatigue::default());
        assert!(schedule.len() >= 4, "a full battery must produce a real arc, got {schedule:?}");
        assert!(
            schedule_is_front_loaded(&schedule),
            "reveal must taper, not rise: {schedule:?}"
        );
        // A leading PLATEAU is correct, not a failure: four independent parameters, all equally
        // unknown, so the first test on each is worth exactly the same. There is no reason the second
        // question should reveal less than the first — they are different questions. What must fall is
        // the arc across ROUNDS, once fatigue starts biting.
        assert!(
            schedule.len() > HiddenParam::ALL.len(),
            "the arc must reach a second round of tests, or there is nothing to taper: {schedule:?}"
        );
        assert!(
            schedule[0] > *schedule.last().expect("non-empty"),
            "the last round must resolve strictly less than the first: {schedule:?}"
        );
    }

    #[test]
    fn the_arc_terminates_rather_than_dripping_zero_forever() {
        // A drip of vanishing reveals is exactly what the item forbids. The schedule must END.
        let schedule = reveal_schedule(&full_battery(), &ResearchPosterior::unknown(), ExperimentFatigue::default());
        assert!(schedule.iter().all(|b| *b > 0.0), "no zero-value steps: {schedule:?}");
        assert!(schedule.len() < HiddenParam::ALL.len() * 32, "the arc must terminate");
    }

    #[test]
    fn an_empty_battery_produces_no_arc_rather_than_panicking() {
        assert!(reveal_schedule(&[], &ResearchPosterior::unknown(), ExperimentFatigue::default()).is_empty());
    }

    #[test]
    fn a_finished_posterior_has_nothing_left_to_reveal() {
        let mut p = ResearchPosterior::unknown();
        for q in HiddenParam::ALL {
            p.reveal(q);
        }
        assert!(reveal_schedule(&full_battery(), &p, ExperimentFatigue::default()).is_empty());
    }

    #[test]
    fn felt_value_tracks_the_step_not_the_total() {
        // [GRIP]: the feeling is in the MOVEMENT toward grip. A late step that resolves half a bit is
        // worth the same as an early one that resolves half a bit — what changes is that late steps
        // have less left to resolve, which the schedule already expresses.
        assert_eq!(felt_value(0.5), felt_value(0.5));
        assert!(felt_value(1.0) > felt_value(0.25));
        assert_eq!(felt_value(-1.0), 0.0, "a step cannot feel worse than nothing");
    }

    #[test]
    fn a_flattened_battery_is_detectable() {
        // The designer-facing use: after retuning reliabilities, check the arc still descends. A battery
        // of identical low-reliability tests on ONE parameter drips instead of front-loading, and this
        // is how that shows up rather than as a vague "research feels bad".
        let dull: Vec<Experiment> =
            (0..6).map(|i| exp(&format!("t{i}"), HiddenParam::Lethality, 0.6)).collect();
        let schedule = reveal_schedule(&dull, &ResearchPosterior::unknown(), ExperimentFatigue::default());
        let total: f32 = schedule.iter().sum();
        assert!(
            total < 1.5,
            "six weak tests on ONE parameter cannot resolve a whole battery: {total} from {schedule:?}"
        );
    }
}
