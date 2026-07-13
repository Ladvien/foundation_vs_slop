//! Full-screen **VHS** post-process pass. Most of the time the screen is clean; every
//! ~45 s the effect fades in for a brief "tracking-error" glitch (chromatic split, tape-wave
//! warp, a switching-noise band, scanlines, grain, a chroma-bloom smear) and fades back out.
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
            cycle_period: 45.0,
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
            .init_resource::<AnomalyGlitch>()
            .add_systems(
                Update,
                (
                    ensure_camera_settings,
                    drive_anomaly_glitch,
                    drive_fade.after(drive_anomaly_glitch),
                ),
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

/// Each frame: compute the current spike envelope (periodic cycle) and push it — plus the constant
/// `base_level` texture floor and the effect strengths — into every camera's `VhsSettings`.
fn drive_fade(
    time: Res<Time>,
    config: Res<VhsConfig>,
    glitch: Res<AnomalyGlitch>,
    mut cameras: Query<&mut VhsSettings>,
) {
    let t = time.elapsed_secs();
    let span = config.fade_in + config.hold + config.fade_out;

    // Periodic glitch: fire the envelope once per `cycle_period`, at the start of each cycle.
    let period = config.cycle_period.max(span.max(0.1));
    let spike = envelope(t % period, &config);

    for mut s in &mut cameras {
        s.base = config.base_level;
        // The picture corrupts on a REAL anomaly manifesting (the watcher unleashing, a chestburster
        // erupting), folded on top of the ambient periodic metronome — a found-footage tell for anomalous
        // PRESENCE rather than a clock. `.max` so the stronger of {periodic, anomaly} wins; never a second
        // code path.
        s.spike = spike.max(glitch.0);
        s.time = t;
        s.chroma = config.chroma;
        s.wave = config.wave;
        s.scanline = config.scanline;
        s.noise_amt = config.noise_amt;
        s.bloom = config.bloom;
    }
}

/// The anomaly-driven glitch intensity: rises to [`ANOMALY_GLITCH_PEAK`] while a real anomaly is
/// manifesting (the watcher unleashing, or a chestburster erupting) and decays after. Folded into the VHS
/// `spike` by [`drive_fade`] so anomalous PRESENCE corrupts the picture — the found-footage grammar
/// SCP-9191's slop trades on — instead of the old fixed 45 s metronome that correlated with nothing.
#[derive(Resource, Default)]
pub struct AnomalyGlitch(pub f32);

/// Corruption target while an anomaly manifests (below 1.0 so the shader peak still reads as a deliberate
/// spike, not a permanent wash).
const ANOMALY_GLITCH_PEAK: f32 = 0.9;
/// Per-second decay once the anomaly settles (~0.6 s fade-out).
const ANOMALY_GLITCH_DECAY: f32 = 1.5;

/// Drive [`AnomalyGlitch`] from live anomaly state — windowed-only, like the rest of VHS (never runs in the
/// deterministic core). Instant rise on manifestation, smooth decay after, so the corruption tracks the
/// anomaly and tails off rather than snapping on/off.
fn drive_anomaly_glitch(
    time: Res<Time>,
    mut glitch: ResMut<AnomalyGlitch>,
    watchers: Query<&crate::enemy::SmileyState>,
    infested: Query<&crate::parasite::Infestation>,
) {
    let manifesting =
        watchers.iter().any(|s| s.is_angry()) || infested.iter().any(|i| i.is_erupting());
    glitch.0 = if manifesting {
        ANOMALY_GLITCH_PEAK
    } else {
        (glitch.0 - ANOMALY_GLITCH_DECAY * time.delta_secs()).max(0.0)
    };
}

