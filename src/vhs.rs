//! Full-screen **VHS** post-process pass. Most of the time the screen is clean; every
//! ~60 s (or the instant a real anomaly manifests, whichever comes first) the effect fades in for a brief
//! "tracking-error" glitch (chromatic split, tape-wave warp, a switching-noise band, scanlines, grain, a
//! chroma-bloom smear) and fades back out. A refractory window caps it at one glitch per `cycle_period`.
//!
//! Implementation uses Bevy 0.19's built-in [`FullscreenMaterial`] abstraction
//! (`bevy_core_pipeline::fullscreen_material`): the engine handles the render-graph wiring
//! (extract → upload uniform → format-specialize the pipeline → ping-pong bind groups → draw a
//! fullscreen triangle in the `Core3d` `PostProcess` set, before tonemapping). We only supply a
//! `ShaderType` settings component on the camera and the fragment shader (`assets/shaders/vhs.wgsl`).
//! The shader cross-fades base→VHS by `intensity`, so `intensity == 0` is an exact passthrough.
//!
//! Tuned via the `vhs:` slice of the unified `assets/config/config.ron`, read once at startup. Edit the
//! RON and relaunch to change the look — there is no in-game panel.

use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;
use serde::{Deserialize, Serialize};

/// Per-camera VHS uniform. Extracted to the render world and uploaded as the `@binding(2)`
/// uniform; the field order/types must byte-match `struct VhsSettings` in `vhs.wgsl`
/// (8 × f32 = 32 bytes, uniform-aligned). Two drive channels: `base` (always-on texture floor)
/// and `spike` (periodic glitch envelope). `time` is driven each frame; the five strengths are
/// copied from [`VhsConfig`].
#[derive(Component, Clone, Copy, ExtractComponent, ShaderType, Default)]
struct VhsSettings {
    base: f32,
    spike: f32,
    time: f32,
    chroma: f32,
    wave: f32,
    scanline: f32,
    noise_amt: f32,
    bloom: f32,
}

impl FullscreenMaterial for VhsSettings {
    fn fragment_shader() -> ShaderRef {
        "shaders/vhs.wgsl".into()
    }
    // Defaults: runs in `Core3d`, `Core3dSystems::PostProcess`, before tonemapping.
}

/// Human-facing, RON-persisted tunables: the fade cycle timing plus each effect's strength. A field of
/// the unified `crate::config::GameConfig` (the `vhs:` slice of `assets/config/config.ron`).
#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct VhsConfig {
    /// Always-on texture floor (0 = off) — the constant analog grain/scanline the picture always
    /// carries. Distortions spike on top of this; see `drive_fade`.
    base_level: f32,
    /// Seconds between glitches (start-to-start of the spike envelope).
    cycle_period: f32,
    /// Spike envelope: ramp up, hold at full, ramp down (seconds).
    fade_in: f32,
    hold: f32,
    fade_out: f32,
    /// Per-effect strengths (0 = off), fed into the shader uniform.
    chroma: f32,
    wave: f32,
    scanline: f32,
    noise_amt: f32,
    bloom: f32,
}

impl Default for VhsConfig {
    fn default() -> Self {
        Self {
            base_level: 0.15,
            cycle_period: 60.0,
            fade_in: 0.5,
            hold: 1.5,
            fade_out: 0.5,
            chroma: 1.0,
            wave: 1.0,
            scanline: 1.0,
            noise_amt: 1.0,
            bloom: 1.0,
        }
    }
}

pub struct VhsPlugin;

impl Plugin for VhsPlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback. The `vhs:` slice comes from the unified
        // `assets/config/config.ron`, loaded + validated once by `ConfigPlugin` (registered first).
        let config = app.world().resource::<crate::config::GameConfig>().vhs.clone();
        app.add_plugins(FullscreenMaterialPlugin::<VhsSettings>::default())
            .insert_resource(config)
            .init_resource::<VhsGlitch>()
            .add_systems(
                Update,
                (ensure_camera_settings, drive_glitch, drive_fade.after(drive_glitch)),
            );
    }
}

/// Attach the settings uniform to the 3D camera once (order-independent: the camera spawns in
/// its own `Startup` system, so we insert lazily in `Update` on any camera still lacking it).
fn ensure_camera_settings(
    mut commands: Commands,
    cameras: Query<Entity, (With<Camera3d>, Without<VhsSettings>)>,
) {
    for entity in &cameras {
        commands.entity(entity).insert(VhsSettings::default());
    }
}

/// Trapezoidal fade envelope in [0, 1] for a time `x` seconds into a glitch, smoothed on the ramps.
fn envelope(x: f32, config: &VhsConfig) -> f32 {
    let up = config.fade_in.max(1e-4);
    let hold = config.hold.max(0.0);
    let down = config.fade_out.max(1e-4);
    let raw = if x < 0.0 {
        0.0
    } else if x < up {
        x / up
    } else if x < up + hold {
        1.0
    } else if x < up + hold + down {
        1.0 - (x - up - hold) / down
    } else {
        0.0
    };
    // Smoothstep the ramps (raw is already in [0, 1]) for a softer fade.
    raw * raw * (3.0 - 2.0 * raw)
}

/// Each frame: read the current glitch envelope (a single, refractory-gated pulse — see [`drive_glitch`])
/// and push it — plus the constant `base_level` texture floor and the effect strengths — into every
/// camera's `VhsSettings`.
fn drive_fade(time: Res<Time>, config: Res<VhsConfig>, glitch: Res<VhsGlitch>, mut cameras: Query<&mut VhsSettings>) {
    let t = time.elapsed_secs();
    // One driver for the spike: the trapezoidal envelope over the current glitch's `phase`. Idle between
    // glitches (`phase` runs past the envelope span → 0), so there is no always-on wash.
    let spike = envelope(glitch.phase, &config);

    for mut s in &mut cameras {
        s.base = config.base_level;
        s.spike = spike;
        s.time = t;
        s.chroma = config.chroma;
        s.wave = config.wave;
        s.scanline = config.scanline;
        s.noise_amt = config.noise_amt;
        s.bloom = config.bloom;
    }
}

/// The VHS glitch clock: a single tracking-error pulse that fires at most once per `cycle_period` and is
/// triggered by a REAL anomaly manifesting (the watcher unleashing, a chestburster erupting) OR the ambient
/// periodic metronome — whichever comes first. Windowed-only, like the rest of VHS (never perturbs the
/// deterministic core: it writes only this cosmetic resource + the camera's `VhsSettings`).
///
/// A refractory window (`cooldown`) is what caps the cadence: SCP-9191's biology now keeps a host swarm
/// perpetually erupting (almond water heals the crabs that host the parasite), so the old "corrupt while
/// ANY anomaly manifests" test pinned the effect ON forever. The cooldown lets a genuine anomaly still fire
/// the glitch — the found-footage tell — but never more than once per `cycle_period`.
#[derive(Resource)]
pub struct VhsGlitch {
    /// Seconds since the current glitch started; drives [`envelope`]. Idles past the envelope span between
    /// glitches, so the picture is clean until the next trigger.
    phase: f32,
    /// Refractory seconds remaining before a new glitch may start — caps the cadence at one per
    /// `cycle_period`.
    cooldown: f32,
}

impl Default for VhsGlitch {
    fn default() -> Self {
        // Start idle (`phase` past any envelope span) but with the refractory already clear, so the ambient
        // metronome can fire the first glitch shortly after boot rather than only after a full period.
        Self { phase: f32::MAX, cooldown: 0.0 }
    }
}

/// Advance the glitch clock and (re)trigger the pulse. Each frame: age `phase`, drain the `cooldown`, and
/// when the refractory has cleared start a fresh glitch if either a real anomaly is manifesting or a full
/// `cycle_period` has elapsed since the last one. `manifesting` stays a first-class trigger — a real
/// anomaly fires the glitch the instant the refractory clears — while the periodic term keeps the picture
/// glitching ambiently when the world is calm.
fn drive_glitch(
    time: Res<Time>,
    config: Res<VhsConfig>,
    mut glitch: ResMut<VhsGlitch>,
    watchers: Query<&crate::enemy::SmileyState>,
    infested: Query<&crate::parasite::Infestation>,
) {
    let dt = time.delta_secs();
    // Saturating age so `phase` cannot overflow across a long calm session while it idles at f32::MAX.
    glitch.phase = (glitch.phase + dt).min(f32::MAX);
    glitch.cooldown = (glitch.cooldown - dt).max(0.0);

    let manifesting =
        watchers.iter().any(|s| s.is_angry()) || infested.iter().any(|i| i.is_erupting());
    let span = config.fade_in + config.hold + config.fade_out;
    let period = config.cycle_period.max(span.max(0.1));

    // Trigger a new glitch only once the refractory has cleared — either on a live anomaly or the ambient
    // metronome. `cooldown == period`, so the observable cadence is one glitch per `cycle_period`.
    if glitch.cooldown <= 0.0 && (manifesting || glitch.phase >= period) {
        glitch.phase = 0.0;
        glitch.cooldown = period;
    }
}

