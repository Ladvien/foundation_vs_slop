//! The humanoid locomotion blend space — pure math, no ECS and no assets, so it runs in the
//! deterministic-core `cargo test` layer and can be swept exhaustively.
//!
//! A moving character is described by two continuous parameters, not by a state:
//!
//! * **speed** (world units/second), and
//! * **θ**, the direction of travel *in the character's own frame* — `0` straight ahead, `+π/2` to its
//!   right, `±π` backwards. Squad units yaw to face what they are shooting at
//!   (`squad::unit_facing`), so travel and facing routinely disagree and θ is not a formality.
//!
//! [`locomotion_weights`] turns that pair into a weight per clip, and the weights are a **partition of
//! unity by construction** — every branch multiplies shares that already sum to one, so nothing has to
//! renormalise and nothing can drift.
//!
//! Two ideas from Shroff, "Realizing NPCs", Game AI Pro 2 ch. 36 shape it:
//!
//! * **Overlapping speed ranges** (§36.2.5): *"we allow the speed ranges to overlap, so that there are
//!   transition areas where we change from one animation to another as the speed smoothly moves up or
//!   down."* [`RUN_BAND`] is that overlap. It replaces the old hard `RUN_SPEED_FRAC` threshold, which a
//!   unit hovering at the boundary would flap across — and every flap restarted a clip.
//! * **Keeping the pure clips pure** — the bands are sized around the clips' measured authored ground
//!   speeds so each one plays alone through most of its range and the blend only spans the gap between
//!   them, which is §36.2.5's "minimize the amount of time that the character remains within an
//!   overlapping range".

use crate::util::smoothstep;

/// Directional lobes of the blend space, in the character's own frame.
pub const DIR_FWD: usize = 0;
pub const DIR_RIGHT: usize = 1;
pub const DIR_BACK: usize = 2;
pub const DIR_LEFT: usize = 3;

/// Slot indices of the locomotion half of a humanoid blend set. The action slots (aim, fire) follow
/// these, so a creature's slot table is `[..LOCO_SLOTS] ++ [action slots]`.
pub const SLOT_IDLE: usize = 0;
pub const SLOT_IDLE_ALERT: usize = 1;
pub const SLOT_WALK: usize = 2;
pub const SLOT_RUN: usize = 3;
pub const SLOT_WALK_BACK: usize = 4;
pub const SLOT_RUN_BACK: usize = 5;
/// Sidestep toward the character's **left**, and toward its **right**. These name the direction of
/// travel, not a clip — `squad` wires whichever glb clip actually moves that way, which for the
/// VALKYRIE rig is not the one whose name says so (see `STRAFE_LEFTWARD`/`STRAFE_RIGHTWARD` there).
pub const SLOT_STRAFE_L: usize = 6;
pub const SLOT_STRAFE_R: usize = 7;
/// How many slots [`locomotion_weights`] fills.
pub const LOCO_SLOTS: usize = 8;

/// Speed band over which a character reads as moving at all, world units/second. Below the low edge it
/// is purely idle; above the high edge purely locomoting.
///
/// It has to be a band rather than an epsilon because `squad::unit_movement` slams `Velocity` to zero
/// the tick a unit arrives — top speed to standstill in one step. The band, together with the
/// cosmetic speed smoothing in the driver, is what turns that cliff into a settle.
pub const MOVE_BAND: (f32, f32) = (0.10, 0.50);

/// The §36.2.5 overlap between the walk and run tiers, world units/second. Sized from the *measured*
/// authored ground speeds of the VALKYRIE clips — 0.98 u/s for the walk, 2.85 u/s for the run (see
/// `squad`'s gait table) — so the band straddles the gap between them and each clip still plays alone
/// over its own range. A 50/50 mixture at the band's midpoint comes out at ~1.03× cadence, so the feet
/// stay planted *through* the crossover, not just at its ends.
pub const RUN_BAND: (f32, f32) = (1.3, 2.4);

/// Travel direction, in the character's own frame, from a planar direction expressed in that frame.
///
/// Bevy characters face local `−Z` (that is where `Transform::looking_at` points them), and with `+Y`
/// up the right-hand side is local `+X`. So forward is `−dz` and rightward is `+dx`, and
/// `atan2(right, forward)` gives the convention [`dir_weights`] wants: `0` ahead, `+π/2` to the right.
pub fn travel_angle(local_dir: bevy::math::Vec2) -> f32 {
    // `local_dir` is (x, z) in the character's frame.
    local_dir.x.atan2(-local_dir.y)
}

/// Weight per directional lobe for a travel angle: a four-way angular blend with at most two non-zero
/// lobes, summing to exactly 1.
///
/// The fraction between two lobes is passed through `smoothstep`, which makes the weights C¹ across
/// the cardinal seams (a linear ramp has a visible kink in weight-vs-angle there) and, as a bonus,
/// holds each pure clip a little longer near its own direction.
pub fn dir_weights(theta: f32) -> [f32; 4] {
    let mut w = [0.0; 4];
    if !theta.is_finite() {
        // A non-finite angle can only come from a degenerate velocity; read it as "straight ahead"
        // rather than poisoning every weight with NaN.
        w[DIR_FWD] = 1.0;
        return w;
    }
    let u = theta.rem_euclid(std::f32::consts::TAU) / std::f32::consts::FRAC_PI_2; // [0, 4)
    let lobe = (u.floor() as usize).min(3);
    let s = smoothstep(0.0, 1.0, u - u.floor());
    w[lobe] += 1.0 - s;
    w[(lobe + 1) % 4] += s;
    w
}

/// `(moving, fast)` — how much of the character reads as locomoting at all, and how much of *that* is
/// the run tier rather than the walk tier. Both in `[0, 1]`.
pub fn tier_weights(speed: f32) -> (f32, f32) {
    if !speed.is_finite() {
        return (0.0, 0.0);
    }
    (
        smoothstep(MOVE_BAND.0, MOVE_BAND.1, speed),
        smoothstep(RUN_BAND.0, RUN_BAND.1, speed),
    )
}

/// The full locomotion weight vector: `(idle, idle_alert, walk, run, walk_back, run_back, strafe_l,
/// strafe_r)`, indexed by the `SLOT_*` constants above and summing to exactly 1.
///
/// `aiming` picks which idle the character settles into; it does not affect the moving clips, because
/// aiming while moving is layered onto the upper body instead (see `squad`'s masked action slots and
/// §36.4.1/§36.4.3).
pub fn locomotion_weights(speed: f32, theta: f32, aiming: bool) -> [f32; LOCO_SLOTS] {
    let (moving, fast) = tier_weights(speed);
    let d = dir_weights(theta);
    let mut w = [0.0; LOCO_SLOTS];
    w[SLOT_IDLE] = (1.0 - moving) * if aiming { 0.0 } else { 1.0 };
    w[SLOT_IDLE_ALERT] = (1.0 - moving) * if aiming { 1.0 } else { 0.0 };
    w[SLOT_WALK] = moving * (1.0 - fast) * d[DIR_FWD];
    w[SLOT_RUN] = moving * fast * d[DIR_FWD];
    w[SLOT_WALK_BACK] = moving * (1.0 - fast) * d[DIR_BACK];
    w[SLOT_RUN_BACK] = moving * fast * d[DIR_BACK];
    // The rig ships no run-tier strafe, so one pair covers both tiers and the shared gait phase
    // rate-corrects it to whatever speed the character is actually sidestepping at.
    w[SLOT_STRAFE_L] = moving * d[DIR_LEFT];
    w[SLOT_STRAFE_R] = moving * d[DIR_RIGHT];
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;

    const TAU: f32 = std::f32::consts::TAU;

    #[test]
    fn direction_lobes_are_a_partition_of_unity_everywhere() {
        for i in 0..=2000 {
            let theta = -TAU + 3.0 * TAU * (i as f32 / 2000.0);
            let w = dir_weights(theta);
            let sum: f32 = w.iter().sum();
            assert!((sum - 1.0).abs() < 1.0e-5, "dir_weights({theta}) summed to {sum}");
            assert!(w.iter().all(|x| (0.0..=1.0).contains(x)), "dir_weights({theta}) = {w:?}");
        }
    }

    #[test]
    fn each_cardinal_direction_selects_its_own_lobe() {
        let q = std::f32::consts::FRAC_PI_2;
        for (theta, lobe) in [(0.0, DIR_FWD), (q, DIR_RIGHT), (2.0 * q, DIR_BACK), (3.0 * q, DIR_LEFT)] {
            let w = dir_weights(theta);
            assert!(w[lobe] > 0.999, "theta {theta} should be pure lobe {lobe}, got {w:?}");
        }
    }

    /// Facing and travel agreeing means forward; travelling to the character's right means the right
    /// lobe. This pins the `−Z` forward / `+X` right convention against a silent axis flip.
    #[test]
    fn travel_angle_matches_the_bevy_forward_convention() {
        let w = dir_weights(travel_angle(Vec2::new(0.0, -1.0)));
        assert!(w[DIR_FWD] > 0.999, "local −Z must read as forward: {w:?}");
        let w = dir_weights(travel_angle(Vec2::new(1.0, 0.0)));
        assert!(w[DIR_RIGHT] > 0.999, "local +X must read as rightward: {w:?}");
        let w = dir_weights(travel_angle(Vec2::new(0.0, 1.0)));
        assert!(w[DIR_BACK] > 0.999, "local +Z must read as backward: {w:?}");
        let w = dir_weights(travel_angle(Vec2::new(-1.0, 0.0)));
        assert!(w[DIR_LEFT] > 0.999, "local −X must read as leftward: {w:?}");
    }

    #[test]
    fn a_degenerate_angle_reads_as_straight_ahead_not_as_nan() {
        let w = dir_weights(f32::NAN);
        assert_eq!(w[DIR_FWD], 1.0);
        assert!(w.iter().all(|x| x.is_finite()));
        let (m, f) = tier_weights(f32::NAN);
        assert_eq!((m, f), (0.0, 0.0));
    }

    #[test]
    fn tiers_are_monotone_and_span_the_bands() {
        assert_eq!(tier_weights(0.0), (0.0, 0.0));
        assert_eq!(tier_weights(MOVE_BAND.1).0, 1.0);
        assert_eq!(tier_weights(RUN_BAND.0).1, 0.0);
        assert_eq!(tier_weights(RUN_BAND.1).1, 1.0);
        let mut prev = (0.0, 0.0);
        for i in 0..=1000 {
            let s = 8.0 * i as f32 / 1000.0;
            let now = tier_weights(s);
            assert!(now.0 >= prev.0 - 1.0e-6 && now.1 >= prev.1 - 1.0e-6, "tiers must not go backwards at {s}");
            prev = now;
        }
    }

    #[test]
    fn locomotion_weights_are_a_partition_of_unity_over_the_whole_space() {
        for si in 0..=120 {
            let speed = 8.0 * si as f32 / 120.0;
            for ti in 0..=120 {
                let theta = -TAU + 2.0 * TAU * (ti as f32 / 120.0);
                for aiming in [false, true] {
                    let w = locomotion_weights(speed, theta, aiming);
                    let sum: f32 = w.iter().sum();
                    assert!(
                        (sum - 1.0).abs() < 1.0e-5,
                        "speed {speed} theta {theta} aiming {aiming} summed to {sum}: {w:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_standing_unit_is_pure_idle_and_a_sprinting_one_pure_run() {
        let w = locomotion_weights(0.0, 0.0, false);
        assert!(w[SLOT_IDLE] > 0.999 && w[SLOT_IDLE_ALERT] == 0.0);
        let w = locomotion_weights(0.0, 0.0, true);
        assert!(w[SLOT_IDLE_ALERT] > 0.999 && w[SLOT_IDLE] == 0.0);
        let w = locomotion_weights(6.0, 0.0, true);
        assert!(w[SLOT_RUN] > 0.999, "6 u/s straight ahead should be pure run: {w:?}");
    }

    /// Aiming while backpedalling must pick the backwards clip, not the forward one. This is the
    /// case the old forward-only state machine got visibly wrong.
    #[test]
    fn backpedalling_while_aiming_uses_the_backward_clips() {
        let theta = travel_angle(Vec2::new(0.0, 1.0)); // travelling along local +Z = backwards
        let w = locomotion_weights(1.0, theta, true);
        assert!(w[SLOT_WALK_BACK] > w[SLOT_WALK], "backpedal should out-weigh forward walk: {w:?}");
        let w = locomotion_weights(4.0, theta, true);
        assert!(w[SLOT_RUN_BACK] > w[SLOT_RUN], "fast backpedal should use run_back: {w:?}");
    }

    /// The property that "smooth" actually means here: the weight vector is *continuous* in both
    /// parameters. This is precisely what the state machine it replaces lacked — `RUN_SPEED_FRAC` was a
    /// step function, so a unit hovering at the boundary flipped its whole clip set back and forth.
    ///
    /// (These are the blend *targets*; a hard acceleration is still allowed to move them quickly. What
    /// bounds the rate of the weights actually handed to the skeleton is `anim`'s `FADE_TAU` ease,
    /// covered by `weights_ease_smoothly_and_reach_the_player`.)
    #[test]
    fn the_blend_space_is_continuous_in_speed_and_in_angle() {
        // Nudge the speed by 5 mm/s at a time across the whole range, at several travel angles.
        for ti in 0..8 {
            let theta = TAU * ti as f32 / 8.0;
            let mut prev = locomotion_weights(0.0, theta, false);
            for i in 1..=1600 {
                let w = locomotion_weights(0.005 * i as f32, theta, false);
                for k in 0..LOCO_SLOTS {
                    let jump = (w[k] - prev[k]).abs();
                    assert!(jump < 0.05, "slot {k} stepped {jump} at speed {}", 0.005 * i as f32);
                }
                prev = w;
            }
        }
        // And by 5 mrad at a time all the way round, at several speeds.
        for s in [0.3_f32, 1.0, 1.85, 3.0, 6.0] {
            let mut prev = locomotion_weights(s, -TAU, false);
            for i in 1..=2513 {
                let theta = -TAU + 0.005 * i as f32;
                let w = locomotion_weights(s, theta, false);
                for k in 0..LOCO_SLOTS {
                    let jump = (w[k] - prev[k]).abs();
                    assert!(jump < 0.05, "slot {k} stepped {jump} at speed {s}, theta {theta}");
                }
                prev = w;
            }
        }
    }
}
