//! **Human-interest proxies** — computable measures of what a *human watching* finds engaging, grounded in
//! games/interest psychology rather than only the information-theoretic `W·S·L` of [`super::surprise`].
//!
//! The realisation from the literature is that the existing witnessed-learnable-surprise fitness is already
//! a computational model of **interest** in Silvia's sense (interest = an appraisal of novelty/complexity ×
//! an appraisal of comprehensibility/coping — precisely the `S × L` structure; Silvia, "Interest — The
//! Curious Emotion", 2009, DOI 10.1111/j.1751-9004.2009.00210.x). What it is *missing* is the single
//! most-cited engagement driver in games research — **uncertainty of outcome / suspense** — plus a
//! *player-level* competence signal. This module adds three proxies, each computed from one cheap signal the
//! rollout already samples per checkpoint: the squad's **survival belief** `b_t ∈ [0,1]` (aggregate squad
//! health as a fraction of what it started with — it falls as units are bitten, rises as the medic heals).
//!
//! 1. **Suspense** — outcome uncertainty over the episode. The RMS of the belief change,
//!    `sqrt(E[(b_{t+1} − b_t)²])`: high for a back-and-forth fight whose result stays live, ~0 for a
//!    blowout. This is the Ely–Frankel–Kamenica formalisation of suspense (expected variance of belief
//!    change) and matches Kumari, Deterding & Freeman's (2019, DOI 10.1145/3311350.3347148) empirically
//!    grounded *outcome uncertainty* as the dominant moment-to-moment motive.
//!
//! 2. **Outcome surprise** — the magnitude of the *terminal* belief revision, `|b_T − b_{T−k}|`: a comeback
//!    or an upset. Distinct from the mode-distribution Bayesian surprise in `S` — this is surprise about
//!    *who wins*, not *what the agent does*.
//!
//! 3. **Effectance / competence** — "doing better than expected" (Deterding, Andersen, Kiverstein & Miller,
//!    2022, "Mastering uncertainty", DOI 10.3389/fpsyg.2022.924953; Koster, *Theory of Fun*). Their
//!    predictive-processing account pins the exact functional: fun is not the belief *level* ("doing well")
//!    but the **positive part of the belief-improvement rate above an adaptive expected baseline**,
//!    precision-weighted and summed — `Σ prec_t · max(0, v_t − v_exp_t)` with `v_t = b_t − b_{t−1}` and
//!    `v_exp` an EMA of recent rate (the paper's "hedonic treadmill": sustained constant progress decays to
//!    ~0 reward; only bursts *above trend* pay out). Because `v_exp` is a moving average, `v − v_exp` is a
//!    high-pass / **acceleration** detector — the player-level analogue of the compression-progress `L`
//!    term. The precision weight (inverse std of recent rate) reproduces the "Soulslike euphoria" spike: a
//!    confidently-held low trajectory that suddenly swings up pays out large.
//!
//! These are exposed as three terms (for calibration via `train probe`) and as a two-axis descriptor
//! (suspense × effectance) so a MAP-Elites archive can illuminate a *range* of engaging encounter shapes.
//! Framing competence as suspense/effectance — not raw "win" — is deliberate: balanced-uncertainty
//! challenge, not maximal winning, is what the psychology finds motivating, and it sidesteps the reward-
//! hacking trap (Skalse et al., arXiv:2209.13085) that a naive win-maximiser would walk into.

/// How many checkpoints back the terminal belief shift ([`Interest::outcome_surprise`]) looks. Three is a
/// short tail — a late comeback, not a whole-episode drift.
const OUTCOME_WINDOW: usize = 3;

/// EMA rate for the "expected" belief-improvement baseline (Deterding's hedonic treadmill). `0.5` weights
/// the last handful of checkpoints — fast enough that sustained progress stops paying, slow enough that a
/// one-off jolt still reads as better-than-expected.
const EMA_ALPHA: f64 = 0.5;

/// Precision floor for the effectance weight. Caps the inverse-std weight at `1/PREC_EPS` so an early,
/// perfectly-flat baseline cannot make one jolt dominate the whole term.
const PREC_EPS: f64 = 0.05;

/// The three human-interest proxies, each in `[0,1]`, kept separate so a low interest score can be
/// *explained* (was it a blowout? a walkover? predictable?) rather than merely observed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interest {
    /// Outcome uncertainty — RMS of the survival-belief change over the episode.
    pub suspense: f32,
    /// Terminal belief revision — a comeback or upset in the closing checkpoints.
    pub outcome_surprise: f32,
    /// "Doing better than expected" — precision-weighted belief improvement above an adaptive baseline.
    pub effectance: f32,
}

impl Interest {
    /// Compute the three proxies from a per-checkpoint survival-belief series `b_t ∈ [0,1]`. A series with
    /// fewer than two points has no dynamics, so every term is `0.0`.
    pub fn from_belief(belief: &[f32]) -> Interest {
        let n = belief.len();
        if n < 2 {
            return Interest { suspense: 0.0, outcome_surprise: 0.0, effectance: 0.0 };
        }

        // ── suspense: RMS of the belief change (Ely–Frankel–Kamenica) ──
        let mut sq_sum = 0.0f64;
        for w in belief.windows(2) {
            let d = f64::from(w[1] - w[0]);
            sq_sum += d * d;
        }
        let suspense = ((sq_sum / (n - 1) as f64).sqrt() as f32).clamp(0.0, 1.0);

        // ── outcome surprise: terminal belief shift over the last OUTCOME_WINDOW checkpoints ──
        let k = OUTCOME_WINDOW.min(n - 1);
        let outcome_surprise = (belief[n - 1] - belief[n - 1 - k]).abs().clamp(0.0, 1.0);

        // ── effectance: precision-weighted positive rate above the EMA baseline (Deterding 2022) ──
        let mut v_exp = 0.0f64; // EMA of the rate — the "expected" (treadmill) baseline
        let mut m2 = 0.0f64; // EMA of rate² — for the precision (inverse-std) weight
        let mut eff = 0.0f64;
        for w in belief.windows(2) {
            let v = f64::from(w[1] - w[0]);
            // Score against the PRIOR expectation, then update the treadmill — so a jolt is measured before
            // the baseline absorbs it. Variance from the running second moment; `max(0)` guards float noise.
            let var = (m2 - v_exp * v_exp).max(0.0);
            let prec = 1.0 / (PREC_EPS + var.sqrt());
            let better = (v - v_exp).max(0.0);
            eff += prec * better;
            v_exp = (1.0 - EMA_ALPHA) * v_exp + EMA_ALPHA * v;
            m2 = (1.0 - EMA_ALPHA) * m2 + EMA_ALPHA * v * v;
        }
        let eff_mean = eff / (n - 1) as f64;
        // Squash the unbounded, non-negative accumulator into [0,1) monotonically (same 1 − e^{−x} as
        // `surprise::surprise_score`), so the term needs no arbitrary ceiling and never ties at 1.
        let effectance = (1.0 - (-eff_mean).exp()) as f32;

        Interest { suspense, outcome_surprise, effectance }
    }

    /// A single interest scalar — the **equal-weighted** blend of the three proxies. Equal weights are the
    /// least-assuming default; the relative importance is a calibration knob (`train probe` prints the three
    /// terms), deliberately not baked in here as a hidden design decision.
    pub fn score(&self) -> f32 {
        (self.suspense + self.outcome_surprise + self.effectance) / 3.0
    }

    /// The two MAP-Elites descriptor axes for illuminating engaging encounter *shapes*: suspense (how
    /// uncertain the outcome stayed) × effectance (how much better-than-expected the squad did). Both in
    /// `[0,1]`, ready for `qd::BehaviorDescriptor::new`.
    pub fn descriptor(&self) -> (f32, f32) {
        (self.suspense, self.effectance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_series_has_no_interest() {
        assert_eq!(Interest::from_belief(&[]), Interest { suspense: 0.0, outcome_surprise: 0.0, effectance: 0.0 });
        assert_eq!(Interest::from_belief(&[0.7]), Interest { suspense: 0.0, outcome_surprise: 0.0, effectance: 0.0 });
    }

    #[test]
    fn a_blowout_has_no_suspense() {
        // A monotone collapse with no back-and-forth: each step is the same small drop, so the RMS change is
        // small and, crucially, there is no *variance* of change — a walkover/steamroll, not a nail-biter.
        let steady = [1.0, 0.98, 0.96, 0.94, 0.92, 0.90];
        let i = Interest::from_belief(&steady);
        assert!(i.suspense < 0.05, "a steady drift is not suspenseful, got {}", i.suspense);
    }

    #[test]
    fn a_back_and_forth_fight_is_suspenseful() {
        // Belief swings up and down — the outcome stays live. Suspense (RMS change) must clear a blowout's.
        let swingy = [0.6, 0.2, 0.7, 0.15, 0.8, 0.25];
        let steady = [1.0, 0.98, 0.96, 0.94, 0.92, 0.90];
        let s = Interest::from_belief(&swingy).suspense;
        let b = Interest::from_belief(&steady).suspense;
        assert!(s > b, "a swingy fight ({s}) must out-suspense a blowout ({b})");
        assert!(s > 0.2, "large swings must register real suspense, got {s}");
    }

    #[test]
    fn a_comeback_registers_outcome_surprise() {
        // Belief was low and dropping, then a sharp recovery at the end — an upset in the closing window.
        let comeback = [0.5, 0.4, 0.3, 0.25, 0.2, 0.8];
        let flat_end = [0.5, 0.4, 0.3, 0.3, 0.3, 0.3];
        assert!(
            Interest::from_belief(&comeback).outcome_surprise
                > Interest::from_belief(&flat_end).outcome_surprise
        );
        assert!(Interest::from_belief(&comeback).outcome_surprise > 0.4);
    }

    #[test]
    fn efficient_recovery_beats_a_flat_walkover_on_effectance() {
        // "Doing better than expected": a run that dips then recovers faster than its recent trend should
        // score effectance above a flat, uneventful trajectory (where nothing beats expectation).
        let recover = [0.8, 0.5, 0.3, 0.5, 0.75, 0.95];
        let flat = [0.9, 0.9, 0.9, 0.9, 0.9, 0.9];
        let r = Interest::from_belief(&recover).effectance;
        let f = Interest::from_belief(&flat).effectance;
        assert!(r > f, "an efficient recovery ({r}) must beat a flat walkover ({f})");
        assert!((0.0..=1.0).contains(&r) && (0.0..=1.0).contains(&f));
    }

    #[test]
    fn all_terms_are_bounded_unit_interval() {
        for series in [
            vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0], // maximal swings
            vec![0.5; 8],
            vec![1.0, 0.0],
            vec![0.3, 0.31, 0.29, 0.32, 0.28],
        ] {
            let i = Interest::from_belief(&series);
            for v in [i.suspense, i.outcome_surprise, i.effectance, i.score()] {
                assert!((0.0..=1.0).contains(&v), "term {v} out of [0,1] for {series:?}");
            }
        }
    }
}
