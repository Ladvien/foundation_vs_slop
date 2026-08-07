//! **Tone / experience-shape proxies** — computable measures of the *SCP-Foundation × Backrooms* feel a
//! human watching a full encounter should get, grounded in games/affect research. These are the companion
//! to `surprise`'s information-theoretic `W·S·L` (what the *agents* do) and `interest`'s
//! suspense/effectance (how the *outcome* swings): this module scores the **emotional shape of the episode
//! over time** — its dread, its liminal emptiness, and its pacing arc.
//!
//! Like [`super::interest`], every proxy is derived from one cheap signal the rollout already samples per
//! checkpoint — the squad's **survival belief** `b_t ∈ [0,1]` (aggregate squad health / the episode's
//! starting health). Danger is its complement, the **intensity** `i_t = 1 − b_t`: 0 when the squad is
//! whole, 1 at the brink. Deriving everything from one series keeps this module pure and fully testable
//! without the sim, exactly as `interest.rs` is.
//!
//! Three proxies, each in `[0,1]`:
//!
//! 1. **Dread** — sustained, *unresolved* tension. Gowler & Iacovides ("Horror, Guilt and Shame:
//!    Uncomfortable Experiences in Digital Games", CHI PLAY 2019, DOI 10.1145/3311350.3347179) find that
//!    productive discomfort comes from high-pressure, *uncertain-outcome* situations — and that it follows
//!    an **inverted-U**: too little is inert, too much (a hopeless, pinned-at-zero slog) tips from engaging
//!    into disengaging. So dread rewards passages held in danger for a *while* without resolving, but with
//!    an inverted-U over how long each passage lasts — a well-sustained held breath scores; an endless
//!    death-march decays. The "attention held by an unresolved state" framing is Itti & Baldi's surprise/
//!    salience account (DOI 10.1016/j.visres.2008.09.007).
//!
//! 2. **Loneliness / liminality** — the Backrooms rhythm: *long emptiness punctuated by sudden intensity*.
//!    Scored as `emptiness × punctuation` — the episode must be *mostly calm* (long stretches at full
//!    strength, nothing happening) **and** contain at least one *sharp* spike of danger. A relentlessly
//!    intense episode is not liminal (no emptiness); a serenely calm one is not either (no punctuation) —
//!    only the empty-corridor-then-sudden-bite profile scores. This is the proxy the level search
//!    (`level_genome`, the `dungeon.liminality` dial) will pull toward when it becomes an objective: a more
//!    liminal, sparse map *produces* this belief profile.
//!
//! 3. **Pacing / flow-arc** — the encounter should have a *shape*: tension **rises**, **climaxes**, and at
//!    least partly **resolves**, rather than being flat or a one-way blowout. Sweetser & Wyeth's GameFlow
//!    (DOI 10.1145/1077246.1077253) makes challenge-with-relief a core enjoyment element. Scored as an
//!    interior climax (a peak away from the very start/end) that both built from a calmer opening and
//!    released before the close — a monotone collapse (belief 1→0, peak at the buzzer, no relief) scores ~0.
//!
//! These are exposed as three terms (for calibration via `train probe`) and a two-axis descriptor
//! (dread × pacing) so a MAP-Elites archive can illuminate a *range* of encounter tones. As in `interest`,
//! [`Experience::score`] is a deliberately **equal-weighted** blend: the relative importance of dread vs.
//! liminality vs. pacing is a calibration knob (the Phase-5 human-audition gate re-fits it against rated
//! runs), not a hidden design decision baked in here.
//!
//! The fourth Phase-1 proxy named in the roadmap — **fairness / exploitability** — is deliberately *not*
//! here: it is a property of a *learned playtester's* behaviour, not of one belief series, so it is filled
//! in by Phase 4 (the neuroevolution playtester) and lives with that machinery.

/// Below this survival belief the squad counts as "in danger" for [`Experience::dread`]. `0.6` = the squad
/// has lost a noticeable share of its strength; above it, a scrape is not yet dread. Calibratable
/// (`train probe` prints the term); an initial value from the belief distribution's meaning, not tuned.
const DREAD_BAND: f32 = 0.6;

/// The dread inverted-U's peak, in **checkpoints**. A rollout samples belief every
/// `evaluate::LIVENESS_EVERY` = 300 ticks = 5 s, so `4` places the most-dread-inducing sustained passage at
/// ~20 s of held, unresolved danger. Shorter passages are mere scares; passages far longer than this are
/// the hopeless slog Gowler & Iacovides warn tips out of engagement, and decay. Calibratable.
const DREAD_PEAK_CHECKPOINTS: f32 = 4.0;

/// Above this survival belief a checkpoint counts as "calm / empty" for [`Experience::loneliness`]. `0.9` =
/// essentially full strength, no active threat — the empty-corridor state the liminal rhythm punctuates.
const CALM_BAND: f32 = 0.9;

/// The three tone proxies, each in `[0,1]`, kept separate so a low score can be *explained* (was it a flat
/// slog? relentless with no let-up? a blowout with no arc?) rather than merely observed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Experience {
    /// Sustained, unresolved tension, inverted-U over passage duration.
    pub dread: f32,
    /// Long emptiness punctuated by sudden danger — the liminal rhythm.
    pub loneliness: f32,
    /// A rise→climax→resolution arc rather than flat or a one-way blowout.
    pub pacing: f32,
}

impl Experience {
    /// Compute the three proxies from a per-checkpoint survival-belief series `b_t ∈ [0,1]`. A series with
    /// fewer than two points has no dynamics, so every term is `0.0` — matching [`super::interest`].
    pub fn from_belief(belief: &[f32]) -> Experience {
        let n = belief.len();
        if n < 2 {
            return Experience { dread: 0.0, loneliness: 0.0, pacing: 0.0 };
        }

        Experience {
            dread: dread(belief),
            loneliness: loneliness(belief),
            pacing: pacing(belief),
        }
    }

    /// A single tone scalar — the **equal-weighted** blend of the three proxies. Equal weights are the
    /// least-assuming default (as in [`super::interest::Interest::score`]); the real weighting is fit by the
    /// human-audition gate, deliberately not baked in here as a hidden design decision.
    pub fn score(&self) -> f32 {
        (self.dread + self.loneliness + self.pacing) / 3.0
    }

    /// The two MAP-Elites descriptor axes for illuminating a range of encounter *tones*: dread (how held-
    /// and-unresolved the tension stayed) × pacing (how well-shaped the arc was). Both `[0,1]`, ready for
    /// `qd::BehaviorDescriptor::new`.
    pub fn descriptor(&self) -> (f32, f32) {
        (self.dread, self.pacing)
    }
}

/// The inverted-U window over a dread passage's duration `l` (checkpoints): `(l/L0)·e^{1 − l/L0}`, the
/// normalised Ricker/gamma shape that is `0` at `l=0`, rises to exactly `1` at `l = L0`
/// ([`DREAD_PEAK_CHECKPOINTS`]), and decays for longer passages — the productive-discomfort inverted-U
/// (Gowler & Iacovides 2019). Never negative.
fn dread_window(l: f32) -> f32 {
    let x = l / DREAD_PEAK_CHECKPOINTS;
    (x * (1.0 - x).exp()).max(0.0)
}

/// **Dread**: sum over maximal runs of consecutive "in danger" checkpoints of `depth · window(length)`,
/// squashed to `[0,1]`. `depth` is the run's mean intensity (how deep the danger ran), `window` the
/// inverted-U over its length. A calm episode has no danger runs → `0`; a string of brief scares scores
/// low (short windows); a sustained, deep, unresolved passage scores high; an endless pinned-at-zero slog
/// decays (the inverted-U's falling arm). The `1 − e^{−x}` squash needs no arbitrary ceiling and never ties
/// at 1 — the same monotone bound `surprise::surprise_score` and `interest` use.
fn dread(belief: &[f32]) -> f32 {
    let mut raw = 0.0f64;
    let mut run_len = 0u32;
    let mut run_intensity_sum = 0.0f64;
    // A closing helper: fold a completed run into `raw`. Declared as a plain match below rather than a
    // closure so it can borrow `raw` mutably twice (mid-loop and after).
    for &b in belief {
        if b < DREAD_BAND {
            run_len += 1;
            run_intensity_sum += f64::from(1.0 - b); // intensity = 1 − belief
        } else if run_len > 0 {
            let depth = run_intensity_sum / f64::from(run_len);
            raw += depth * f64::from(dread_window(run_len as f32));
            run_len = 0;
            run_intensity_sum = 0.0;
        }
    }
    // Flush a run that reached the end of the episode still in danger.
    if run_len > 0 {
        let depth = run_intensity_sum / f64::from(run_len);
        raw += depth * f64::from(dread_window(run_len as f32));
    }
    (1.0 - (-raw).exp()) as f32
}

/// **Loneliness / liminality**: `emptiness × punctuation`. `emptiness` is the fraction of checkpoints at or
/// above [`CALM_BAND`] (long stretches of nothing); `punctuation` is the sharpest single-step *drop* in
/// belief (a sudden spike of danger), `max_t (b_t − b_{t+1})` clamped to `[0,1]`. High only when the
/// episode is mostly empty **and** something sudden happens in it — the Backrooms rhythm. A constantly-calm
/// episode has `punctuation ≈ 0`; a relentlessly-intense one has `emptiness ≈ 0`; both score ~0.
fn loneliness(belief: &[f32]) -> f32 {
    let n = belief.len();
    let calm = belief.iter().filter(|&&b| b >= CALM_BAND).count();
    let emptiness = calm as f32 / n as f32;
    let mut punctuation = 0.0f32;
    for w in belief.windows(2) {
        // A drop in belief is a spike of danger; a rise (recovery) is not punctuation.
        punctuation = punctuation.max(w[0] - w[1]);
    }
    (emptiness * punctuation.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

/// **Pacing / flow-arc**: `interiority × min(rise, resolution)`. `rise = i_peak − i_first` (tension built
/// from the opening), `resolution = i_peak − i_last` (tension released by the close), both over intensity
/// `i = 1 − b`; `min` demands **both** a build *and* a relief, so a monotone one-way blowout (which peaks at
/// the buzzer, `resolution = 0`) scores 0. `interiority = 4·p·(1−p)` with `p = peak_index/(n−1)` is a
/// parabola that is `1` for a climax at mid-episode and `0` for one pinned at the very start or end — an
/// arc, not a spike at the edge. All terms `[0,1]`.
fn pacing(belief: &[f32]) -> f32 {
    let n = belief.len();
    // Intensity peak (max danger = min belief) and its position. First occurrence wins (stable).
    let mut peak_idx = 0usize;
    let mut min_belief = belief[0];
    for (i, &b) in belief.iter().enumerate() {
        if b < min_belief {
            min_belief = b;
            peak_idx = i;
        }
    }
    let i_peak = 1.0 - min_belief;
    let i_first = 1.0 - belief[0];
    let i_last = 1.0 - belief[n - 1];
    let rise = (i_peak - i_first).clamp(0.0, 1.0);
    let resolution = (i_peak - i_last).clamp(0.0, 1.0);
    let p = peak_idx as f32 / (n - 1) as f32;
    let interiority = (4.0 * p * (1.0 - p)).clamp(0.0, 1.0);
    (interiority * rise.min(resolution)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_series_has_no_experience() {
        assert_eq!(Experience::from_belief(&[]), Experience { dread: 0.0, loneliness: 0.0, pacing: 0.0 });
        assert_eq!(Experience::from_belief(&[0.5]), Experience { dread: 0.0, loneliness: 0.0, pacing: 0.0 });
    }

    #[test]
    fn all_terms_are_bounded_unit_interval() {
        for series in [
            vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            vec![0.5; 8],
            vec![1.0, 0.0],
            vec![1.0, 0.9, 0.3, 0.2, 0.6, 0.95],
            vec![0.05, 0.04, 0.03, 0.02, 0.01, 0.0],
        ] {
            let e = Experience::from_belief(&series);
            for v in [e.dread, e.loneliness, e.pacing, e.score()] {
                assert!((0.0..=1.0).contains(&v), "term {v} out of [0,1] for {series:?}");
            }
        }
    }

    // ── dread ──────────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn a_calm_episode_has_no_dread() {
        // Belief never drops below the danger band: nothing was ever at stake.
        let calm = [1.0, 0.98, 0.97, 0.99, 1.0, 0.98];
        assert!(dread(&calm) < 1e-6, "a calm episode must have ~0 dread, got {}", dread(&calm));
    }

    #[test]
    fn a_sustained_unresolved_passage_out_dreads_a_brief_scare() {
        // A single-checkpoint dip (brief scare) vs. a passage held deep in danger for ~the peak duration.
        let brief = [1.0, 1.0, 0.3, 1.0, 1.0, 1.0];
        let sustained = [1.0, 0.5, 0.3, 0.25, 0.3, 1.0];
        let s = dread(&sustained);
        let b = dread(&brief);
        assert!(s > b, "a held, unresolved passage ({s}) must out-dread a brief scare ({b})");
        assert!(s > 0.2, "a genuine dread passage must register, got {s}");
    }

    #[test]
    fn an_endless_hopeless_slog_decays_below_a_well_shaped_passage() {
        // The inverted-U's falling arm: belief pinned at the brink for the whole (long) episode is a
        // disengaging death-march, and must score BELOW a well-sustained ~peak-length passage of the same
        // depth. Both are "always in danger"; only the duration differs.
        let well_shaped = [0.2, 0.2, 0.2, 0.2]; // ~L0 checkpoints deep in danger
        let endless = [0.2; 20]; // far past the inverted-U peak
        let w = dread(&well_shaped);
        let e = dread(&endless);
        assert!(w > e, "a well-shaped dread passage ({w}) must beat an endless slog ({e})");
    }

    // ── loneliness / liminality ──────────────────────────────────────────────────────────────────────

    #[test]
    fn empty_then_spike_is_lonelier_than_either_extreme() {
        // Mostly calm, then one sudden sharp bite — the Backrooms rhythm.
        let liminal = [1.0, 1.0, 1.0, 1.0, 0.2, 1.0];
        // Relentless danger: no emptiness.
        let relentless = [0.3, 0.25, 0.2, 0.3, 0.2, 0.25];
        // Serene: emptiness but no punctuation.
        let serene = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let l = loneliness(&liminal);
        assert!(l > loneliness(&relentless), "liminal ({l}) must beat relentless");
        assert!(l > loneliness(&serene), "liminal ({l}) must beat serene emptiness");
        assert!(l > 0.3, "a mostly-empty episode with a sharp bite must register, got {l}");
    }

    #[test]
    fn a_gentle_drift_is_not_lonely() {
        // Emptiness but no *sharp* spike — the danger seeps in gradually, so punctuation stays low.
        let drift = [1.0, 0.98, 0.9, 0.85, 0.8, 0.78];
        assert!(loneliness(&drift) < 0.2, "a gentle drift is not the liminal rhythm, got {}", loneliness(&drift));
    }

    // ── pacing / flow-arc ────────────────────────────────────────────────────────────────────────────

    #[test]
    fn a_rise_climax_resolution_arc_paces_well() {
        // Safe → tension climbs → climax mid-episode → relief. Belief: high, dips to a mid trough, recovers.
        let arc = [1.0, 0.7, 0.3, 0.2, 0.6, 0.95];
        let p = pacing(&arc);
        assert!(p > 0.4, "a clean rise-climax-resolution arc must pace well, got {p}");
    }

    #[test]
    fn a_flat_episode_has_no_pacing() {
        let flat = [0.8, 0.8, 0.8, 0.8, 0.8, 0.8];
        assert!(pacing(&flat) < 1e-6, "a flat episode has no arc, got {}", pacing(&flat));
    }

    #[test]
    fn a_one_way_blowout_has_no_pacing() {
        // Monotone collapse: belief falls to the buzzer, peak danger at the very end, no relief. The climax
        // is at the edge (interiority → 0) and resolution is 0 — a steamroll, not a paced encounter.
        let blowout = [1.0, 0.8, 0.6, 0.4, 0.2, 0.0];
        assert!(pacing(&blowout) < 0.05, "a one-way blowout must not pace, got {}", pacing(&blowout));
    }

    #[test]
    fn the_dread_window_peaks_at_its_calibrated_duration() {
        // The inverted-U is 0 at length 0, exactly 1 at the peak duration, and below 1 on either side.
        assert!(dread_window(0.0).abs() < 1e-9);
        assert!((dread_window(DREAD_PEAK_CHECKPOINTS) - 1.0).abs() < 1e-6);
        assert!(dread_window(DREAD_PEAK_CHECKPOINTS * 0.5) < 1.0);
        assert!(dread_window(DREAD_PEAK_CHECKPOINTS * 3.0) < dread_window(DREAD_PEAK_CHECKPOINTS));
    }

    #[test]
    fn score_is_the_equal_weighted_blend() {
        let e = Experience { dread: 0.3, loneliness: 0.6, pacing: 0.9 };
        assert!((e.score() - 0.6).abs() < 1e-6);
    }
}
