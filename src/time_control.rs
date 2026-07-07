//! Windowed **game-speed + pause** control for inspection and RL observation.
//!
//! The pinned simulation runs on a fixed 60 Hz `FixedUpdate` (see `lib::run`), so "speed" is simply
//! how many fixed steps Bevy runs per rendered frame. Bevy accumulates `Time<Virtual>` and drains it
//! into the fixed-step accumulator, so raising the virtual clock's *relative speed* to N makes ~N
//! fixed steps run per frame — while each fixed step still sees a constant `1/60` dt. Determinism and
//! frame-rate independence are therefore preserved at any multiplier: the sim just steps more (or
//! fewer) times per frame, never with a different dt.
//!
//! **One writer rule.** The virtual clock's relative speed has exactly one writer:
//! `juice::tick_hitstop`. It composes *base game-speed × hitstop freeze × pause* (see `juice`). This
//! module only owns the `GameSpeed` resource + the keyboard input that sets it. It never calls
//! `set_relative_speed`.
//!
//! **On `Time<Virtual>::max_delta`.** We leave Bevy's 250 ms default alone. Its clamp applies to the
//! *raw real* frame delta *before* the speed multiply (bevy_time 0.19 `virt.rs::advance_with_raw_delta`:
//! `clamped = min(raw_delta, max_delta)`, then `× effective_speed`), so at 60 fps (raw ≈ 16.7 ms) it
//! never clamps and the high multipliers already reach their full step count (×64 ⇒ ≈64 steps/frame) —
//! there is no per-frame step cap to "unlock". `max_delta`'s only job is to bound the fixed-step
//! catch-up after a real stall (alt-tab, window drag, breakpoint); *raising* it would enlarge that
//! post-stall burst (at ×64 a 1.2 s stall would replay ≈4600 fixed steps in one frame), so don't.
//!
//! **Human input is speed-independent.** The camera controls (WASD pan, wheel zoom, middle-drag) must
//! feel identical at any multiplier, so they never read the sim clock: pan runs on `Time<Real>` and
//! zoom/drag use raw per-frame input deltas (see `camera::drive_camera`). Order/selection input
//! (`selection`) has no time coupling at all. Changing speed — or pausing — never alters how the mouse
//! or keyboard respond.
//!
//! **Accepted side effects at high speed** (this is a dev/inspection tool, not a shipping UX):
//! - Cosmetic *gameplay-feel* systems that read the generic `Time` do scale with the multiplier —
//!   trauma decay + screen-shake phase (`juice`/`camera` shake), audio timers. At ×64 the shake buzzes
//!   and SFX race; this is intentional (they track sim time, not wall time) and is not input.
//! - Render frame-rate drops at extreme multipliers because each rendered frame runs up to ~64 fixed
//!   steps. The simulation stays correct and deterministic per step; it just does more work per frame.
//!
//! The headless RL harness (`sim_harness`, feature `test-harness`) has its own, separate speed knob
//! (`SimConfig::speed`) that advances *real* time manually and never touches `Time<Virtual>`, so it
//! never collides with this path.

use bevy::prelude::*;

/// Player-selected simulation speed. `base` is the wall-speed multiplier the swarm/lasers run at;
/// `paused` overrides it to a full freeze. Read by `juice::tick_hitstop`, the sole writer of the
/// virtual clock's relative speed.
#[derive(Resource, Debug, Clone, Copy)]
pub struct GameSpeed {
    /// Wall-speed multiplier (`1.0` = real time). One of [`SPEED_LADDER`] when set from the keyboard,
    /// but an RL/inspection tool may write any value directly — `juice::tick_hitstop` clamps it to a
    /// finite, non-negative speed before it reaches the virtual clock, so a stray NaN/inf/negative
    /// can't panic Bevy's `set_relative_speed`.
    pub base: f32,
    /// When `true`, gameplay is frozen regardless of `base` (the virtual clock is driven to ~0).
    pub paused: bool,
}

impl Default for GameSpeed {
    fn default() -> Self {
        Self { base: 1.0, paused: false }
    }
}

/// Discrete speed presets bound to number keys `1..=9` (index 0..=8). Index 2 (`×1.0`) is real time;
/// left of it slows down, right of it speeds up.
pub const SPEED_LADDER: [f32; 9] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0];

/// Number keys `1..=9`, positionally aligned with [`SPEED_LADDER`]'s rungs (digit 1 → rung 0, …). Kept
/// as a parallel array rather than `(KeyCode, index)` pairs so the rung index is derived from position;
/// `zip` with `SPEED_LADDER` means a length mismatch simply ignores the extra entries — never an
/// out-of-bounds panic (repo no-panic rule).
const DIGIT_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

pub struct TimeControlPlugin;

impl Plugin for TimeControlPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameSpeed>()
            .add_systems(Update, read_speed_input);
    }
}

/// Number-key presets: `1..=9` pick a rung of [`SPEED_LADDER`] (and un-pause), `0` toggles pause.
/// Uses `just_pressed` so a held key changes speed once. Digits don't collide with the camera
/// controls (WASD / arrows / scroll / middle-drag — see `camera::drive_camera`).
fn read_speed_input(keys: Res<ButtonInput<KeyCode>>, mut speed: ResMut<GameSpeed>) {
    for (&mult, key) in SPEED_LADDER.iter().zip(DIGIT_KEYS) {
        if keys.just_pressed(key) {
            speed.base = mult;
            speed.paused = false;
        }
    }
    if keys.just_pressed(KeyCode::Digit0) {
        speed.paused = !speed.paused;
    }
}
