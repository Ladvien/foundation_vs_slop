//! Game-feel "juice": **hitstop** (a brief near-freeze on a kill) and **trauma-based screen shake**.
//! These are technique-agnostic feel amplifiers — the single highest-ROI way to make impacts land
//! (see the gore codex). Impacts push [`Trauma`] and request a [`Hitstop`]; this plugin decays the
//! trauma and drives the virtual clock's speed. The camera reads [`Trauma`] and offsets the view
//! (see `camera`).
//!
//! Hitstop / impact-frames and the trauma² + noise shake model follow Squirrel Eiserloh,
//! "Math for Game Programmers: Juicing Your Cameras" (GDC 2016). Trauma is squared so small hits
//! barely nudge the camera while kills kick hard, and it decays linearly to zero.

use bevy::prelude::*;
use bevy::time::{Real, Virtual};

/// Accumulated screen-shake energy in `[0, 1]`. Combat systems (see `gore`) spike it on impacts; it
/// bleeds off each frame. The camera turns it into a view offset scaled by `trauma²`.
#[derive(Resource, Default)]
pub struct Trauma(pub f32);

impl Trauma {
    /// Add shake energy, clamped to 1.
    pub fn add(&mut self, amount: f32) {
        self.0 = (self.0 + amount).clamp(0.0, 1.0);
    }
}

/// The active hitstop window: gameplay (the virtual clock) is nearly frozen until `until_real`,
/// measured on the **real** clock so the freeze can end itself.
#[derive(Resource, Default)]
pub struct Hitstop {
    until_real: f32,
}

impl Hitstop {
    /// Freeze for `secs` starting at `now_real`; extends (never shortens) an in-progress freeze.
    pub fn freeze(&mut self, now_real: f32, secs: f32) {
        self.until_real = self.until_real.max(now_real + secs);
    }
}

/// How fast trauma bleeds off (units per second of gameplay time). Decays on the virtual clock, so a
/// hitstop freeze *holds* the shake at its peak for the frozen frames, then it releases — the classic
/// "impact frame, then shudder" feel.
const TRAUMA_DECAY: f32 = 1.7;
/// Virtual-clock speed during a hitstop freeze (near zero — not exactly zero, so timers still tick).
const FROZEN_SPEED: f32 = 0.02;

pub struct JuicePlugin;

impl Plugin for JuicePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Trauma>()
            .init_resource::<Hitstop>()
            .add_systems(Update, (tick_hitstop, decay_trauma));
    }
}

/// Drive the virtual clock's relative speed from the hitstop window, timed on the real clock so the
/// freeze releases itself (the virtual clock it controls would otherwise stall its own countdown).
fn tick_hitstop(real: Res<Time<Real>>, hitstop: Res<Hitstop>, mut vtime: ResMut<Time<Virtual>>) {
    let target = if real.elapsed_secs() < hitstop.until_real {
        FROZEN_SPEED
    } else {
        1.0
    };
    if (vtime.relative_speed() - target).abs() > 1e-4 {
        vtime.set_relative_speed(target);
    }
}

/// Bleed trauma toward zero. Uses the virtual clock so the shake holds through a hitstop freeze.
fn decay_trauma(time: Res<Time>, mut trauma: ResMut<Trauma>) {
    if trauma.0 > 0.0 {
        trauma.0 = (trauma.0 - TRAUMA_DECAY * time.delta_secs()).max(0.0);
    }
}
