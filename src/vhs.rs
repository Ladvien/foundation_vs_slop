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
//! Tuned via the `vhs_fx.ron` file, read once at startup. Edit the RON and relaunch to change the
//! look — there is no in-game panel.

use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;
use serde::{Deserialize, Serialize};

const CONFIG_PATH: &str = "vhs_fx.ron";

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

/// Human-facing, RON-persisted tunables: the fade cycle timing plus each effect's strength.
#[derive(Resource, Serialize, Deserialize, Clone)]
struct VhsConfig {
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
        app.add_plugins(FullscreenMaterialPlugin::<VhsSettings>::default())
            .init_resource::<VhsConfig>()
            .add_systems(Startup, load_config)
            .add_systems(Update, (ensure_camera_settings, drive_fade));
    }
}

/// Load persisted config at startup if present; otherwise keep the defaults (one path, no
/// fallback file is written here).
fn load_config(mut config: ResMut<VhsConfig>) {
    if let Some(loaded) = read_config() {
        info!("vhs: loaded {CONFIG_PATH}");
        *config = loaded;
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
fn drive_fade(time: Res<Time>, config: Res<VhsConfig>, mut cameras: Query<&mut VhsSettings>) {
    let t = time.elapsed_secs();
    let span = config.fade_in + config.hold + config.fade_out;

    // Periodic glitch: fire the envelope once per `cycle_period`, at the start of each cycle.
    let period = config.cycle_period.max(span.max(0.1));
    let spike = envelope(t % period, &config);

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

fn read_config() -> Option<VhsConfig> {
    let text = match std::fs::read_to_string(CONFIG_PATH) {
        Ok(text) => text,
        // Optional override file; absence means "use the built-in defaults".
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            error!("vhs: {CONFIG_PATH} exists but could not be read: {e}");
            std::process::exit(1);
        }
    };
    match ron::from_str(&text) {
        Ok(config) => Some(config),
        // Fail loud on a malformed override rather than silently running on defaults (one-path rule).
        Err(e) => {
            error!("vhs: {CONFIG_PATH} is present but failed to parse — fix the RON: {e}");
            std::process::exit(1);
        }
    }
}
