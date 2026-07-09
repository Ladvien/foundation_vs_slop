//! The mold's *shading*. The compute chain produces raw simulation fields; everything about how the mold
//! looks — lighting, surface normal, wetness, bioluminescence — lives here.
//!
//! Both materials are `ExtendedMaterial<StandardMaterial, _>`, so the mold is a genuinely **lit PBR
//! surface** rather than a flat unlit decal. It picks up the scene's directional key and ambient fill, its
//! normal is perturbed by the biomass thickness (so it reads as a lumpy wet film), and its roughness drops
//! where it is thickest (a wet sheen). That is what stops it looking like paint.
//!
//! Two surfaces, one field:
//! - [`MoldFloorMaterial`] on the translucent floor overlay, sampling the field by world XZ.
//! - [`MoldWallMaterial`] on the wall slabs, sampling the *same* field at the wall's footprint XZ and
//!   fading with height, so the coating visibly creeps up out of the floor/wall corner.
//!
//! Only `fragment_shader()` is overridden, so the prepass keeps `StandardMaterial`'s default — no
//! `PREPASS_PIPELINE` branching is needed in the WGSL.

use bevy::asset::Asset;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, StandardMaterial};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use super::{MyceliaConfig, WORLD_EXTENT, WORLD_ORIGIN};

/// The lit floor coating.
pub type MoldFloorMaterial = ExtendedMaterial<StandardMaterial, MoldFloorExt>;
/// The lit coating creeping up the walls.
pub type MoldWallMaterial = ExtendedMaterial<StandardMaterial, MoldWallExt>;
/// The fruit body itself — the same organism, wearing flesh instead of film.
pub type MoldFruitMaterial = ExtendedMaterial<StandardMaterial, MoldFruitExt>;

/// GPU uniform shared by both surface shaders — must byte-match `MoldSurfaceParams` in
/// `mycelia_floor.wgsl` / `mycelia_wall.wgsl`. `vec2`s first so every following scalar is naturally aligned.
#[derive(Clone, ShaderType)]
pub struct MoldSurfaceParams {
    /// World XZ of field texel (0,0) — the same mapping the compute chain writes with.
    world_origin: Vec2,
    /// World-space span the field covers.
    world_extent: Vec2,
    /// Field resolution in texels (for the finite-difference normal).
    field_res: Vec2,
    /// Multiplier on the veins' emissive. The camera is LDR with no bloom, so this stays near 1.
    glow_gain: f32,
    /// Master opacity dial.
    intensity: f32,
    /// Trail value at which a vein begins to show.
    vein_lo: f32,
    /// Trail value at which a vein is fully lit.
    vein_hi: f32,
    /// Strength of the thickness-derived normal perturbation. `0` = flat.
    normal_strength: f32,
    /// Perceptual roughness in the **vein cores only**. The body of the mat stays matte.
    wet_roughness: f32,
    /// How far up a wall the mold climbs before fading out (world units).
    climb_height: f32,
    /// Spatial frequency of the hyphal filament noise, in cycles per world unit.
    fiber_scale: f32,
    /// How hard the filaments carve the surface normal.
    fiber_strength: f32,
    /// How much fbm breaks up the colony's outer contour. `0` = a smooth iso-contour meniscus (reads as
    /// liquid); higher = a feathery, dendritic advancing margin (reads as a fungus).
    margin_roughness: f32,
    /// Strength of the grazing-angle fuzz rim that stands in for a sheen lobe.
    sheen_strength: f32,
    /// Strength of the cavity ambient occlusion written into `diffuse_occlusion`. Without this the scene's
    /// bright uniform ambient fills every crevice and the filaments render flat regardless of the normal.
    ao_strength: f32,
}

impl MoldSurfaceParams {
    fn new(cfg: &MyceliaConfig) -> Self {
        Self {
            world_origin: WORLD_ORIGIN,
            world_extent: WORLD_EXTENT,
            field_res: Vec2::splat(cfg.field_size as f32),
            glow_gain: cfg.glow_gain,
            intensity: cfg.intensity,
            vein_lo: cfg.vein_lo,
            vein_hi: cfg.vein_hi,
            normal_strength: cfg.normal_strength,
            wet_roughness: cfg.wet_roughness,
            climb_height: cfg.climb_height,
            fiber_scale: cfg.fiber_scale,
            fiber_strength: cfg.fiber_strength,
            margin_roughness: cfg.margin_roughness,
            sheen_strength: cfg.sheen_strength,
            ao_strength: cfg.ao_strength,
        }
    }
}

/// Floor extension. Bindings start at 100 so they cannot collide with `StandardMaterial`'s own bindings in
/// the shared material bind group.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct MoldFloorExt {
    #[uniform(100)]
    params: MoldSurfaceParams,
    /// Simulation fields: `R` trail · `G` biomass · `B` wall contact · `A` coverage.
    #[texture(101)]
    #[sampler(102)]
    display: Handle<Image>,
    /// World state; the material reads `G` (light/gaze) so the mold can conceal its glow when watched.
    #[texture(103)]
    #[sampler(104)]
    control: Handle<Image>,
}

/// Wall extension — identical bindings, different fragment shader.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct MoldWallExt {
    #[uniform(100)]
    params: MoldSurfaceParams,
    #[texture(101)]
    #[sampler(102)]
    display: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    control: Handle<Image>,
}

/// Per-body state for the fruit shader. Separate from [`MoldSurfaceParams`] because every mushroom needs
/// its own copy while the surface dials are shared.
#[derive(Clone, ShaderType)]
pub struct MoldFruitParams {
    /// Rate-limited maturity. `0` = the pale universal veil stretched over a fresh primordium, `1` = the
    /// mat's own deep flesh showing through an expanded pileus. Not `growth`: the albedo shift is throttled
    /// so it can never complete faster than the slow-change-blindness window (see
    /// `perceptual::MIN_APPEARANCE_RAMP_SECS`).
    tint: f32,
}

/// The fruit body. Reuses [`MoldSurfaceParams`] so a mushroom inherits the mat's palette, hyphal fibre
/// noise, margin mottle, matte felt roughness, sheen and cavity AO — retuning the mat retunes the mushroom,
/// and the two visibly read as the *same organism*.
///
/// The mesh's `COLOR_0` is a **part mask**, not artwork: `R` = cap (pileus), `G` = flesh (stipe, gills,
/// annulus), `B` = volva. Bevy's `StandardMaterial` multiplies base colour by the vertex colour when the
/// mesh carries `COLOR_0`, which would tint the cap pure red — so this shader overwrites `base_color`
/// outright and reads the mask itself. There are no textures on this asset; the parts *are* the mask.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct MoldFruitExt {
    #[uniform(100)]
    params: MoldSurfaceParams,
    #[texture(101)]
    #[sampler(102)]
    display: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    control: Handle<Image>,
    #[uniform(105)]
    fruit: MoldFruitParams,
}

impl MoldFruitExt {
    pub fn new(
        cfg: &MyceliaConfig,
        display: Handle<Image>,
        control: Handle<Image>,
        tint: f32,
    ) -> Self {
        Self { params: MoldSurfaceParams::new(cfg), display, control, fruit: MoldFruitParams { tint } }
    }

    /// Publish this body's maturity to its shader. Called by `fruit::tint_fruit_bodies` when it changes.
    pub fn set_tint(&mut self, tint: f32) {
        self.fruit.tint = tint;
    }

    /// The tint currently uploaded, so the caller can skip a no-op write (which would otherwise emit an
    /// `AssetEvent::Modified` and re-upload the uniform every frame for every mature mushroom).
    pub fn tint(&self) -> f32 {
        self.fruit.tint
    }
}

impl MaterialExtension for MoldFruitExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/mycelia_fruit.wgsl".into()
    }
}

impl MoldFloorExt {
    pub fn new(cfg: &MyceliaConfig, display: Handle<Image>, control: Handle<Image>) -> Self {
        Self { params: MoldSurfaceParams::new(cfg), display, control }
    }
}

impl MoldWallExt {
    pub fn new(cfg: &MyceliaConfig, display: Handle<Image>, control: Handle<Image>) -> Self {
        Self { params: MoldSurfaceParams::new(cfg), display, control }
    }
}

impl MaterialExtension for MoldFloorExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/mycelia_floor.wgsl".into()
    }
}

impl MaterialExtension for MoldWallExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/mycelia_wall.wgsl".into()
    }
}

/// The `StandardMaterial` under the floor overlay: translucent (the carpet shows through where there is no
/// mold) and non-metallic. The extension fragment supplies base colour, normal, roughness and emissive.
pub fn floor_base() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        alpha_mode: AlphaMode::Blend,
        ..default()
    }
}
