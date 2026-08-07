//! The gait cadence policy — the one function that turns ground speed into cycles per second.
//!
//! This lives here, engine-free, and not in `emerge-anim` where the runtime consumes it, because two
//! programs must agree on it exactly: the game's pose blender drives every gait clip's seek time from
//! it, and the editor's animation bench predicts foot skate from it (`rig_check::skate_report`). A
//! bench that restated the clamp would measure a different game than the one that ships. `emerge-anim`
//! re-exports [`gait_cycles_per_sec`] so the game-side call sites are unchanged.

/// Weights below this snap to exactly `0.0` in the runtime's weight pass, because Bevy's
/// `animate_targets` skips a clip whose weight is *bit-exactly* zero. It doubles as the "no gait
/// weight" floor in [`gait_cycles_per_sec`] — one epsilon, one meaning: a mixture this faint carries
/// no cadence opinion.
pub const WEIGHT_EPS: f32 = 1.0e-3;

/// Clamp on the gait playback rate, as a multiple of the mixture's authored cadence, so a stalled or
/// sprinting outlier can neither freeze nor gabble the legs. The price is honest and measurable:
/// outside `authored_speed × [0.5, 2.0]` the feet slide, because the legs are pinned to the clamp
/// while the body keeps the sim's speed. The bench's skate check reports exactly that band.
pub const PHASE_RATE_CLAMP: (f32, f32) = (0.5, 2.0);

/// Cycles per second for a gait mixture.
///
/// `speed / mean_cycle_distance` is the cadence that keeps the feet planted at this ground speed — the
/// blend-space generalisation of the single-clip playback-rate correction of Game AI Pro 2 §36.2.5.
/// It is clamped to [`PHASE_RATE_CLAMP`] × the mixture's own authored cadence, so a unit moving far
/// outside the range its clips were authored for degrades to a fast-but-readable stride instead of a
/// blur. Returns `0.0` when no gait clip carries weight — the phase then simply holds, and because it
/// is never reset, resuming is seamless.
///
/// `weight_sum` = Σ wᵢ, `weighted_distance` = Σ wᵢ·cycle_distanceᵢ, `weighted_cadence` = Σ wᵢ/durationᵢ,
/// all over the gait slots.
pub fn gait_cycles_per_sec(
    speed: f32,
    weight_sum: f32,
    weighted_distance: f32,
    weighted_cadence: f32,
) -> f32 {
    if weight_sum <= WEIGHT_EPS || weighted_distance <= 1.0e-6 || weighted_cadence <= 1.0e-6 {
        return 0.0;
    }
    let mean_distance = weighted_distance / weight_sum;
    let nominal = weighted_cadence / weight_sum;
    let (lo, hi) = PHASE_RATE_CLAMP;
    (speed / mean_distance).clamp(lo * nominal, hi * nominal)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk covers 1.388 world units per 1.417 s cycle; run covers 2.135 per 0.750 s (measured from
    /// the planted foot's travel — see `squad`'s gait table). A pure clip at its own authored speed
    /// must come out at rate 1.0, i.e. its authored cadence.
    #[test]
    fn a_pure_clip_at_its_authored_speed_runs_at_cadence_one() {
        let walk_cps = gait_cycles_per_sec(1.388 / 1.417, 1.0, 1.388, 1.0 / 1.417);
        assert!(
            (walk_cps - 1.0 / 1.417).abs() < 1.0e-4,
            "walk at its authored speed should play at 1×: {walk_cps}"
        );
        let run_cps = gait_cycles_per_sec(2.135 / 0.750, 1.0, 2.135, 1.0 / 0.750);
        assert!(
            (run_cps - 1.0 / 0.750).abs() < 1.0e-4,
            "run at its authored speed should play at 1×: {run_cps}"
        );
    }

    /// A 50/50 walk/run mixture at the mixture's own authored speed must also land near cadence 1 —
    /// this is the property that keeps the feet planted *through* the overlap band, not just at its ends.
    #[test]
    fn a_mixture_at_its_own_authored_speed_stays_near_cadence_one() {
        let (ww, wr) = (0.5, 0.5);
        let weight_sum = ww + wr;
        let weighted_distance = ww * 1.388 + wr * 2.135;
        let weighted_cadence = ww / 1.417 + wr / 0.750;
        let nominal = weighted_cadence / weight_sum;
        let authored_speed = weighted_distance / weight_sum * nominal;
        let cps = gait_cycles_per_sec(authored_speed, weight_sum, weighted_distance, weighted_cadence);
        assert!((cps - nominal).abs() < 1.0e-4, "mixture cadence drifted: {cps} vs {nominal}");
    }

    #[test]
    fn the_gait_rate_is_clamped_at_both_ends() {
        let (lo, hi) = PHASE_RATE_CLAMP;
        let nominal = 1.0 / 1.417;
        let fast = gait_cycles_per_sec(1000.0, 1.0, 1.388, nominal);
        assert!((fast - hi * nominal).abs() < 1.0e-5, "runaway speed must clamp: {fast}");
        let slow = gait_cycles_per_sec(0.001, 1.0, 1.388, nominal);
        assert!((slow - lo * nominal).abs() < 1.0e-5, "a crawl must not freeze the legs: {slow}");
    }

    #[test]
    fn no_gait_weight_freezes_the_phase_instead_of_dividing_by_zero() {
        assert_eq!(gait_cycles_per_sec(6.0, 0.0, 0.0, 0.0), 0.0);
        assert_eq!(gait_cycles_per_sec(6.0, 1.0, 0.0, 1.0), 0.0);
    }
}
