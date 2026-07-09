//! MYCELIA — a GPU-compute "sentient mold" ambience that colonizes the dungeon floor.
//!
//! A living skin of bioluminescent fungal intelligence creeps over the floor: a Jones multi-agent
//! Physarum transport network (the "veins") layered with a Gray-Scott reaction-diffusion field (the
//! organic "blooms"), all simulated GPU-resident in one world-space texture atlas and composited onto
//! the floor by a custom material. It reads the world one-way — foraging toward blood pools and nests,
//! recoiling from cells a unit currently sees (fog-of-war as a "light/gaze" proxy), blooming in the
//! unseen dark. It never influences gameplay; it is pure cosmetic ambience.
//!
//! # Why this design (right-sized to THIS game)
//! The world is a single fixed 192×192-tile dungeon (one flat floor at Y=0), generated once and never
//! streamed. So the mold is one **world-space field** indexed by world XZ (not mesh UV — every floor
//! tile shares one `Plane3d` with UV 0..1). Because the floor is planar there are **no UV seams**, and
//! the whole field fits in a single 1024² texture — no chunking/LOD machinery is needed.
//!
//! # Determinism firewall (see `TESTING.md`)
//! Everything here is cosmetic and lives on **`Update`**, never `FixedUpdate`. No mold entity carries
//! `Health` and no existing actor's `Transform`/`Health` is mutated, so `snapshot_hash` (which queries
//! `(Transform, Health)`) never sees it. Data flow is strictly **CPU→GPU, one-way** — no GPU→gameplay
//! readback (GPU floats are not bit-reproducible across hardware, the same non-determinism class as the
//! Avian physics and FX layers). `MyceliaPlugin` is registered **only** in `lib::run`, never in the
//! headless `sim_harness` (mirroring `UiPlugin`/`DialoguePlugin`), and it **no-ops if the `RenderApp`
//! sub-app is absent** — belt-and-suspenders so a headless build can never touch the render world.
//!
//! ## References (home-still corpus)
//! Jones multi-agent Physarum (arXiv 1503.06579; 10.1080/17445760.2015.1085535); foraging survey
//! (10.1007/s10462-021-10112-1). Field growth: Gray-Scott / Turing reaction-diffusion; Flow-Lenia
//! (arXiv 2212.07906) for the mass-conserving multi-species extension (deferred past v1).

mod material;
mod pipeline;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::{
    Extent3d, ShaderType, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::RenderApp;

pub use material::MoldFloorMaterial;

/// Side length (texels) of the square world-space mold field. 1024² over the 192 m footprint ≈ 5.3
/// texels/tile — plenty of resolution for veins, and a trivial GPU workload for one fixed world.
pub const FIELD_SIZE: u32 = 1024;

/// Compute workgroup edge (8×8 = 64 threads), matching the Bevy game-of-life reference. `FIELD_SIZE`
/// must be a whole multiple of this so the dispatch covers every texel exactly.
pub const WORKGROUP_SIZE: u32 = 8;

/// World-space footprint the field maps onto. The dungeon is `192×192` tiles at `TILE_SIZE = 1.0`, with
/// `Plane3d` tiles centered on integer cells, so floor world XZ spans `[-0.5, 191.5]`. The field's
/// texel (0,0) sits at `WORLD_ORIGIN`; texel (FIELD_SIZE,FIELD_SIZE) at `WORLD_ORIGIN + WORLD_EXTENT`.
pub const WORLD_ORIGIN: Vec2 = Vec2::new(-0.5, -0.5);
pub const WORLD_EXTENT: Vec2 = Vec2::splat(192.0);

/// Storage/sample format for the composited display texture. `Rgba16Float` is both storage-writable and
/// filterable-sampleable on Metal/wgpu29 (unlike `Rgba32Float`, which is not filterable), so the compute
/// pass can `textureStore` into it and the floor material can `textureSample` it with linear filtering.
pub const DISPLAY_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// The shared handle to the composited mold texture — the ONE texture that crosses the main↔render
/// boundary. The compute pass writes it (render world); the floor material samples it (also resolved in
/// the render world). Extracted so `prepare_bind_group` in the render world can find it.
#[derive(Resource, Clone, ExtractResource)]
pub struct MoldImages {
    /// `R` = vein/trail density, `G` = biomass, `B` = glow/flux, `A` = light — filled progressively
    /// across the implementation phases. In Phase A it just holds an animated plumbing-test gradient.
    pub display: Handle<Image>,
}

/// Simulation parameters shared (byte-identical) by the compute pass and the floor material. Kept in one
/// place so the world↔UV mapping can never drift between "where the sim writes" and "where the floor
/// reads". Extracted into the render world each frame.
///
/// Field order/types MUST byte-match the `MoldParams` struct in the WGSL shaders (std140 uniform layout:
/// each `vec2` aligns to 8 bytes → origin@0, extent@8, field_res@16, time@24, pad@28, size 32).
#[derive(Resource, Clone, ExtractResource, ShaderType)]
pub struct MoldParams {
    /// World XZ of field texel (0,0).
    pub world_origin: Vec2,
    /// World-space span the field covers (so `uv = (world_xz - origin) / extent`).
    pub world_extent: Vec2,
    /// Field resolution in texels (as float, for UV↔texel math in-shader).
    pub field_res: Vec2,
    /// Seconds since startup — drives the animated growth. Advanced on the main world each `Update`.
    pub time: f32,
    /// Explicit tail pad so Rust and WGSL agree on the 32-byte std140 size.
    pub _pad: f32,
}

pub struct MyceliaPlugin;

impl Plugin for MyceliaPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<MoldFloorMaterial>::default(),
            ExtractResourcePlugin::<MoldImages>::default(),
            ExtractResourcePlugin::<MoldParams>::default(),
        ))
        .add_systems(Startup, setup_mycelia)
        .add_systems(Update, advance_mold_time);

        // Render-world wiring. `get_sub_app_mut` returns `None` in a headless build with no `RenderApp`,
        // so the whole compute path is silently absent there (the determinism firewall) rather than
        // panicking like `sub_app_mut` would.
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            pipeline::build_render_app(render_app);
        }
    }
}

/// Create the shared display texture, seed the shared params, spawn the floor overlay that samples the
/// mold field by world XZ. Runs once at startup on the main world.
fn setup_mycelia(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<MoldFloorMaterial>>,
) {
    // One RGBA16F texture, render-world resident, usable as BOTH a compute storage-write target and a
    // sampled material texture. This is the single image shared across the main↔render boundary.
    let mut image = Image::new_uninit(
        Extent3d { width: FIELD_SIZE, height: FIELD_SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        DISPLAY_FORMAT,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::COPY_DST | TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING;
    let display = images.add(image);

    commands.insert_resource(MoldImages { display: display.clone() });
    commands.insert_resource(MoldParams {
        world_origin: WORLD_ORIGIN,
        world_extent: WORLD_EXTENT,
        field_res: Vec2::splat(FIELD_SIZE as f32),
        time: 0.0,
        _pad: 0.0,
    });

    // A single translucent overlay quad covering the whole floor footprint, sitting a hair above the
    // floor (Y=0) so it composites over the carpet without z-fighting. Sampling by world XZ (in the
    // shader) is what a per-tile floor material will do later; the overlay proves that path with one mesh.
    let mesh = meshes.add(Plane3d::default().mesh().size(WORLD_EXTENT.x, WORLD_EXTENT.y));
    let material = materials.add(MoldFloorMaterial::new(display));
    let center = Vec3::new(
        WORLD_ORIGIN.x + WORLD_EXTENT.x * 0.5,
        0.02,
        WORLD_ORIGIN.y + WORLD_EXTENT.y * 0.5,
    );
    commands.spawn((
        Name::new("mycelia_floor_overlay"),
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(center),
    ));
}

/// Advance the shared sim clock on the main world each frame; the value is extracted into the render
/// world for the compute pass and reused by the floor material. `Update` (cosmetic), never `FixedUpdate`.
///
/// Uses **real** time (not `Time<Virtual>`), so the mold keeps breathing while the game is paused or at
/// the menu — matching the repo's other cosmetic shaders (`nest`, `vhs`) which animate off `globals.time`.
/// The mold is ambience; it should never freeze with a gameplay pause.
fn advance_mold_time(time: Res<Time<Real>>, mut params: ResMut<MoldParams>) {
    params.time = time.elapsed_secs();
}

/// Guard against a compile-time drift between `FIELD_SIZE` and the workgroup tiling. Dispatch code
/// assumes exact coverage (`FIELD_SIZE / WORKGROUP_SIZE` workgroups per axis).
const _: () = assert!(FIELD_SIZE % WORKGROUP_SIZE == 0);
