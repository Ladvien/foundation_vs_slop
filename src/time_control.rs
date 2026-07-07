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
//! module only owns the `GameSpeed` resource + the keyboard input that sets it, plus a one-shot
//! startup tweak to `Time<Virtual>`'s `max_delta` (below). It never calls `set_relative_speed`.
//!
//! **Accepted side effects at high speed** (this is a dev/inspection tool, not a shipping UX):
//! - Cosmetic `Update` systems that read the generic `Time` resolve it to `Time<Virtual>` and so scale
//!   with the multiplier: camera pan (`camera::drive_camera`), trauma decay (`juice::decay_trauma`),
//!   audio timers. At ×64 the camera pans fast and SFX race — expected; decoupling them (switch to
//!   `Time<Real>`) is out of scope here.
//! - Render frame-rate drops at extreme multipliers because each rendered frame runs up to ~64 fixed
//!   steps. The simulation stays correct and deterministic per step; it just does more work per frame.
//!
//! The headless RL harness (`sim_harness`, feature `test-harness`) has its own, separate speed knob
//! (`SimConfig::speed`) that advances *real* time manually and never touches `Time<Virtual>`, so it
//! never collides with this path.

use bevy::prelude::*;
use bevy::time::Virtual;
use std::time::Duration;

/// Player-selected simulation speed. `base` is the wall-speed multiplier the swarm/lasers run at;
/// `paused` overrides it to a full freeze. Read by `juice::tick_hitstop`, the sole writer of the
/// virtual clock's relative speed.
#[derive(Resource, Debug, Clone, Copy)]
pub struct GameSpeed {
    /// Wall-speed multiplier (`1.0` = real time). One of [`SPEED_LADDER`] when set from the keyboard,
    /// but any positive value is valid (e.g. if an RL/inspection tool writes it directly).
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

/// Headroom for `Time<Virtual>::max_delta`. Bevy's default is 250 ms, which caps a 60 fps frame at
/// ~15 fixed steps — so ×16..×64 would silently saturate at ~×15. 1.2 s allows ~72 steps/frame, past
/// the ×64 ceiling (64 steps/frame). Kept finite (not raised absurdly high) so a genuine render hitch
/// after a stall catches up a bounded batch rather than spiraling.
const VIRTUAL_MAX_DELTA_SECS: f32 = 1.2;

pub struct TimeControlPlugin;

impl Plugin for TimeControlPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameSpeed>()
            .add_systems(Startup, raise_virtual_max_delta)
            .add_systems(Update, read_speed_input);
    }
}

/// Raise the virtual clock's per-frame clamp once at startup so the high multipliers actually run more
/// than ~15 fixed steps per frame (see [`VIRTUAL_MAX_DELTA_SECS`]). Mutates the `Time<Virtual>` that
/// Bevy's `TimePlugin` already owns — do not insert a fresh one.
fn raise_virtual_max_delta(mut vtime: ResMut<Time<Virtual>>) {
    vtime.set_max_delta(Duration::from_secs_f32(VIRTUAL_MAX_DELTA_SECS));
}

/// Number-key presets: `1..=9` pick a rung of [`SPEED_LADDER`] (and un-pause), `0` toggles pause.
/// Uses `just_pressed` so a held key changes speed once. Digits don't collide with the camera
/// controls (WASD / arrows / scroll / middle-drag — see `camera::drive_camera`).
fn read_speed_input(keys: Res<ButtonInput<KeyCode>>, mut speed: ResMut<GameSpeed>) {
    // (KeyCode, ladder index) for the nine speed presets.
    const PRESETS: [(KeyCode, usize); 9] = [
        (KeyCode::Digit1, 0),
        (KeyCode::Digit2, 1),
        (KeyCode::Digit3, 2),
        (KeyCode::Digit4, 3),
        (KeyCode::Digit5, 4),
        (KeyCode::Digit6, 5),
        (KeyCode::Digit7, 6),
        (KeyCode::Digit8, 7),
        (KeyCode::Digit9, 8),
    ];
    for (key, idx) in PRESETS {
        if keys.just_pressed(key) {
            speed.base = SPEED_LADDER[idx];
            speed.paused = false;
        }
    }
    if keys.just_pressed(KeyCode::Digit0) {
        speed.paused = !speed.paused;
    }
}
