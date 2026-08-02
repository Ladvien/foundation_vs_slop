//! **The ASYNC aperture** — the material for the volume inside the door frame (FVS-G-5).
//!
//! The frame is a model; this is the thing that makes it a door onto somewhere else. Design doc §7
//! calls it the game's signature image and notes that the *model* already existed while this did not.
//!
//! The in-repo precedent is `nest.rs` + `assets/shaders/nest.wgsl` — literally "the portal's custom
//! fullscreen-fractal material" — and this follows it exactly: `AsBindGroup` + `impl Material` +
//! `MaterialPlugin`, with a uniform pushed on `Update`. The shader itself carries the three art
//! decisions and why each is load-bearing.
//!
//! Windowed-only. A `MaterialPlugin` in the harness would make the deterministic core depend on a GPU,
//! which is the stated reason `LightingPlugin` and `MyceliaPlugin` are excluded from `sim_harness`.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

/// Uniform for [`AsyncApertureMaterial`]. `ShaderType`-compatible field order and 16-byte padding.
#[derive(Clone, Copy, bevy::render::render_resource::ShaderType, Debug)]
pub struct ApertureUniform {
    pub depth: f32,
    pub warp: f32,
    /// `[0,1]`, eased up while an operative stands in the trigger.
    pub charge: f32,
    pub _pad: f32,
}

impl Default for ApertureUniform {
    fn default() -> Self {
        // Tuned blind (no GPU in this loop) — these are a starting point for iteration, not a verdict.
        Self {
            depth: 0.55,
            warp: 0.9,
            charge: 0.0,
            _pad: 0.0,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct AsyncApertureMaterial {
    #[uniform(0)]
    pub settings: ApertureUniform,
}

impl Material for AsyncApertureMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/async_aperture.wgsl".into()
    }
    /// **Opaque, deliberately.** The aperture must OCCLUDE — a blended quad reads as a window onto the
    /// wall behind it, which is precisely the thing it must not be.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }
    /// **Double-sided**, because the quad stands in the Site's OUTER perimeter and the camera turns.
    ///
    /// Bevy's default back-face culling made the aperture visible only from the hall: at the Q/E
    /// detents that look at the Site from outside, the ASYNC door was a see-through hole in the
    /// exterior wall with the hall floor visible through it. Found by screenshotting from behind on
    /// 2026-08-01. Rendering both faces keeps the door reading as a door from every angle; the
    /// corridor illusion simply runs the other way from the far side, which is the correct thing for
    /// an aperture that genuinely goes somewhere.
    ///
    /// There is no `cull_mode` hook on `Material` — the pipeline's primitive state is where culling
    /// lives, so it is set in `specialize`.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Marker so the charge system can find the aperture quad.
#[derive(Component)]
pub struct ApertureQuad;

/// The light the aperture throws into the ASYNC hall.
///
/// **The shader alone could not do this.** A custom `fragment` writes a colour and stops there — it
/// illuminates nothing around it, so before 2026-08-02 the portal lit its own pixels and left the hall
/// floor in front of it completely dark. A hole onto somewhere else that casts nothing reads as a
/// poster of a hole. This is the real light, sodium-tinted to match `WALL_TINT` in
/// `assets/shaders/async_aperture.wgsl`, and it breathes and charges on exactly the same terms the
/// shader does — see [`drive_aperture_charge`].
#[derive(Component)]
pub struct ApertureGlow;

/// Sodium spill colour. Matches `WALL_TINT` in the shader; the two are the same art fact, and if they
/// drift the portal's own light stops agreeing with the portal.
pub const APERTURE_LIGHT_TINT: Color = Color::linear_rgb(0.82, 0.74, 0.42);

/// Resting luminous power of the aperture's spill, in lumens.
pub const APERTURE_LIGHT_LUMENS: f32 = 90_000.0;

/// Extra lumens at full charge — the door brightening as it notices someone.
pub const APERTURE_LIGHT_CHARGE_LUMENS: f32 = 150_000.0;

/// Ease `charge` toward 1 while an operative is inside the door's trigger, and back down when not.
///
/// On `Update` and cosmetic: it writes only a material uniform, never `(Transform, Health)`.
pub fn drive_aperture_charge(
    time: Res<Time>,
    mut mats: ResMut<Assets<AsyncApertureMaterial>>,
    quads: Query<&MeshMaterial3d<AsyncApertureMaterial>, With<ApertureQuad>>,
    doors: Query<(&super::visuals::AsyncDoor, &Transform)>,
    avatars: Query<&Transform, With<super::visuals::SiteAvatar>>,
    mut glows: Query<&mut PointLight, With<ApertureGlow>>,
) {
    // A generous approach radius rather than the trigger itself: the door should notice you BEFORE you
    // are through it, or the effect only ever plays on the frame you leave.
    let near = avatars.iter().any(|a| {
        doors.iter().any(|(d, dt)| {
            let rel = (a.translation - dt.translation).abs();
            rel.x <= d.half_extents.x * 2.5 && rel.z <= d.half_extents.z * 4.0
        })
    });
    let target = if near { 1.0 } else { 0.0 };
    // Slow: an anomaly noticing you should not snap.
    let k = (time.delta_secs() * 1.4).clamp(0.0, 1.0);
    let mut charge = 0.0f32;
    for handle in &quads {
        if let Some(mut m) = mats.get_mut(&handle.0) {
            m.settings.charge += (target - m.settings.charge) * k;
            charge = m.settings.charge;
        }
    }
    // Drive the spill from the SAME charge the shader is showing, and breathe it on the same period
    // (`sin(t * 0.9)` there). Recomputing the phase from `elapsed_secs` rather than threading it
    // through keeps one number in one place; the two are in step because they read the same clock.
    let breath = 0.5 + 0.5 * (time.elapsed_secs() * 0.9).sin();
    let lumens = APERTURE_LIGHT_LUMENS * (0.82 + 0.18 * breath)
        + APERTURE_LIGHT_CHARGE_LUMENS * charge;
    for mut light in &mut glows {
        light.intensity = lumens;
    }
}
