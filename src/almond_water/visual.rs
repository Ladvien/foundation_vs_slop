//! The Almond Water puddle visual + the mold moisture-feed — cosmetic, windowed-only. Registered ONLY in
//! the game (never in `sim_harness`), so it can never perturb a golden. Uses the render clock, never the
//! sim clock.
//!
//! **The puddle** is one translucent overlay quad covering the whole floor footprint (the same recipe as
//! `mycelia`'s floor overlay, `src/mycelia/mod.rs`), driven by a 192² single-channel texture uploaded from
//! the gameplay [`super::AlmondWater`] `level` grid each frame. A custom [`AlmondWaterMaterial`] samples it
//! by world XZ and composites three layers — procedural bubble-up blooms, a physically-based thin-film
//! interference tint (oil-slick iridescence), and an almond base — in `assets/shaders/almond_water.wgsl`.
//!
//! **The mold moisture-feed** lives in [`crate::mycelia::control::write_control`], which reads this same
//! `AlmondWater` field and seeds the mold's chemoattractant (`R`) channel with local wetness so the mold
//! blooms richer on wet concrete. One-way and cosmetic — it never writes gameplay state.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::shader::ShaderRef;

use super::AlmondWater;
use crate::config::GameConfig;
use crate::dungeon::Dungeon;
use crate::mycelia::{WORLD_EXTENT, WORLD_ORIGIN};

/// Height above the floor (Y=0) the puddle quad sits at — a hair below the mold overlay (`0.02`) so a wet,
/// mold-colonised cell reads as mold *over* water, and low enough to avoid z-fighting the carpet.
const PUDDLE_Y: f32 = 0.015;

/// GPU uniform for the puddle shader. Packed into `vec4`s so there is no `vec3`/scalar std140 alignment
/// hazard — must byte-match `AlmondParams` in `assets/shaders/almond_water.wgsl`.
#[derive(Clone, ShaderType)]
struct AlmondParams {
    /// `(world_origin.xy, world_extent.xy)` — the world-XZ→UV map, same as the mold's field mapping.
    bounds: Vec4,
    /// `(field_res, min_visible_norm, film_thickness_nm, film_ior)`.
    params0: Vec4,
    /// `(iridescence_strength, almond_tint.rgb)`.
    params1: Vec4,
}

/// The puddle material: a plain alpha-blended [`Material`] sampling the level texture by world XZ.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct AlmondWaterMaterial {
    #[uniform(0)]
    params: AlmondParams,
    /// Normalised water level (0..1 of `capacity`), one texel per dungeon cell, uploaded each frame.
    #[texture(1)]
    #[sampler(2)]
    level: Handle<Image>,
}

impl Material for AlmondWaterMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/almond_water.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}

/// Handle to the 192² level texture the puddle material samples — mutated in place each frame by
/// [`upload_level`], which re-uploads it to the GPU.
#[derive(Resource)]
struct AlmondLevelImage(Handle<Image>);

/// Owns the cosmetic puddle render + drives the level-texture upload. Windowed-only.
pub struct AlmondWaterVisualPlugin;

impl Plugin for AlmondWaterVisualPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<AlmondWaterMaterial>::default())
            .add_systems(Startup, setup_puddle)
            .add_systems(Update, upload_level);
    }
}

/// Create the level texture, spawn the floor overlay quad with the puddle material, and stash the texture
/// handle for the per-frame upload. Runs once at startup; needs `Dungeon` (for the grid size) which
/// `DungeonPlugin` inserts before any schedule runs.
fn setup_puddle(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    config: Res<GameConfig>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<AlmondWaterMaterial>>,
) {
    let (w, h) = (dungeon.width as u32, dungeon.height as u32);
    let mut image = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0u8; (w * h) as usize], // R8Unorm = 1 byte/texel, zero-filled (dry) until the first upload
        TextureFormat::R8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage = TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING;
    image.sampler = ImageSampler::linear(); // smooth the per-cell field into a continuous pool
    let level = images.add(image);

    let cfg = &config.almond_water;
    let tint = cfg.almond_tint;
    let min_visible_norm = (cfg.min_visible_level / cfg.capacity).clamp(0.0, 1.0);
    let material = materials.add(AlmondWaterMaterial {
        params: AlmondParams {
            bounds: Vec4::new(WORLD_ORIGIN.x, WORLD_ORIGIN.y, WORLD_EXTENT.x, WORLD_EXTENT.y),
            params0: Vec4::new(w as f32, min_visible_norm, cfg.film_thickness_nm, cfg.film_ior),
            params1: Vec4::new(cfg.iridescence_strength, tint[0], tint[1], tint[2]),
        },
        level: level.clone(),
    });

    let mesh = meshes.add(Plane3d::default().mesh().size(WORLD_EXTENT.x, WORLD_EXTENT.y));
    let center = Vec3::new(
        WORLD_ORIGIN.x + WORLD_EXTENT.x * 0.5,
        PUDDLE_Y,
        WORLD_ORIGIN.y + WORLD_EXTENT.y * 0.5,
    );
    commands.spawn((
        Name::new("almond_water_puddle_overlay"),
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(center),
    ));
    commands.insert_resource(AlmondLevelImage(level));
}

/// Copy the gameplay `level` grid into the puddle texture each frame, normalised to `capacity` → 0..255.
/// `super::AlmondWater`'s fields are private but visible here (a child module sees its parent's privates),
/// so this reads `level` directly rather than sampling cell-by-cell. Cosmetic: it only mutates a GPU image.
fn upload_level(
    field: Res<AlmondWater>,
    config: Res<GameConfig>,
    image: Res<AlmondLevelImage>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(mut image) = images.get_mut(&image.0) else {
        return;
    };
    let Some(data) = image.data.as_mut() else {
        return;
    };
    let cap = config.almond_water.capacity.max(1.0e-6);
    // `level.len() == width*height == data.len()` (R8Unorm), so the row-major cell index is the texel index.
    if data.len() != field.level.len() {
        return;
    }
    for (byte, &lvl) in data.iter_mut().zip(field.level.iter()) {
        *byte = ((lvl / cap).clamp(0.0, 1.0) * 255.0) as u8;
    }
}
