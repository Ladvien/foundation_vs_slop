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

mod agents;
mod field;
mod material;
mod pipeline;

use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::{ShaderType, TextureFormat};
use bevy::render::storage::ShaderBuffer;
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

// ── Physarum tuning (Jones three-sensor model) ─────────────────────────────────────────────────────
// Distances/angles are in field texels (1024² over 192 m ≈ 5.3 texels/m) and radians. These are the
// aesthetic knobs that decide how the veins forage and branch; they graduate to the RON config in Phase E.

/// Half-angle between the centre sensor and each side sensor. ~23°.
const SENSE_ANGLE: f32 = 0.40;
/// How far ahead (texels) the sensors sample the trail.
const SENSE_DIST: f32 = 9.0;
/// How sharply an agent turns toward the stronger scent each tick. ~29°.
const ROTATE_ANGLE: f32 = 0.50;
/// Texels an agent advances per tick.
const STEP_SIZE: f32 = 1.0;
/// Scent laid down per agent per tick (pre-scale), in trail units.
const DEPOSIT_AMOUNT: f32 = 1.0;
/// Multiplicative trail persistence per tick (`<1` so trails fade). Fast enough that lightly-trafficked
/// cells fade back toward dark between visits (so only reinforced routes stay bright and read as veins),
/// slow enough that an established channel holds its shape.
const DECAY: f32 = 0.82;
/// Upper clamp on trail intensity so reinforced hubs can't blow up (decay alone bounds the steady state,
/// this guards against transient spikes / NaNs).
const TRAIL_MAX: f32 = 12.0;
/// Fixed-point factor for the integer deposit accumulator: agents `atomicAdd(deposit_amount * SCALE)`, the
/// diffuse pass reads back `/ SCALE`. Large enough to preserve fractional deposits under heavy overlap.
const DEPOSIT_SCALE: f32 = 1024.0;

/// The mold's field textures. Only `display` crosses to the material; the trail pair lives purely to feed
/// the compute chain. All three are extracted so the render world's `prepare_bind_group` can bind them.
#[derive(Resource, Clone, ExtractResource)]
pub struct MoldImages {
    /// The composited output the floor material samples. `R` = vein/trail density, `G` = biomass (Phase C),
    /// `B` = glow/flux, `A` = light — filled progressively across phases. Written by the diffuse pass.
    pub display: Handle<Image>,
    /// Trail-scent ping-pong pair. Each tick one is the read source (sensed by agents + blurred by
    /// diffuse) and the other the write target; they swap by parity so diffusion never reads what it is
    /// concurrently writing. `R` channel holds trail intensity.
    pub trail_a: Handle<Image>,
    pub trail_b: Handle<Image>,
}

/// The mold's GPU storage buffers — the agent population and the per-texel deposit accumulator.
#[derive(Resource, Clone, ExtractResource)]
pub struct MoldBuffers {
    /// `array<Agent>` (`{ pos: vec2<f32>, heading: f32, _pad }`), updated in place each tick by the
    /// `agents` pass. Seeded once on the CPU (`agents::seed_agents`); GPU float drift after is cosmetic.
    pub agents: Handle<ShaderBuffer>,
    /// `array<atomic<u32>>`, one slot per field texel. The `agents` pass `atomicAdd`s fixed-point scent
    /// here; the `diffuse` pass reads it back and folds it into the trail; `clear_deposit` zeroes it each
    /// tick. A storage buffer (not a storage *texture*) because wgpu/Metal has no portable texture atomics.
    pub deposit: Handle<ShaderBuffer>,
}

/// Simulation parameters for the compute chain. Field order/types MUST byte-match the `MoldParams` struct
/// in `mycelia_sim.wgsl`. Laid out `vec2`-first so every field is naturally aligned (the three `vec2`s
/// occupy 0..24, all scalars follow on 4-byte boundaries) — `ShaderType`/encase computes the std140/std430
/// padding, and the WGSL struct mirrors the same field order so the layouts agree.
///
/// (The floor material has its own separate `MoldMatParams`; this uniform is compute-only.)
#[derive(Resource, Clone, ExtractResource, ShaderType)]
pub struct MoldParams {
    /// World XZ of field texel (0,0).
    pub world_origin: Vec2,
    /// World-space span the field covers (so `uv = (world_xz - origin) / extent`).
    pub world_extent: Vec2,
    /// Field resolution in texels (as float, for UV↔texel math in-shader).
    pub field_res: Vec2,
    /// Seconds since startup — seeds the agent-steering RNG. Advanced on the main world each `Update`.
    pub time: f32,
    /// Active agent count (agents beyond this in the buffer are idle).
    pub agent_count: u32,
    /// Half-angle between centre and side sensors (radians).
    pub sense_angle: f32,
    /// Sensor reach ahead of the agent (texels).
    pub sense_dist: f32,
    /// Turn magnitude per tick (radians).
    pub rotate_angle: f32,
    /// Advance per tick (texels).
    pub step_size: f32,
    /// Scent deposited per agent per tick (pre-scale).
    pub deposit_amount: f32,
    /// Trail persistence per tick (`<1`).
    pub decay: f32,
    /// Upper clamp on trail intensity.
    pub trail_max: f32,
    /// Fixed-point factor for the integer deposit accumulator.
    pub deposit_scale: f32,
}

pub struct MyceliaPlugin;

impl Plugin for MyceliaPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<MoldFloorMaterial>::default(),
            ExtractResourcePlugin::<MoldImages>::default(),
            ExtractResourcePlugin::<MoldBuffers>::default(),
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

/// Create the field textures + GPU buffers, seed the shared params, and spawn the floor overlay that
/// samples the mold field by world XZ. Runs once at startup on the main world.
fn setup_mycelia(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<MoldFloorMaterial>>,
) {
    // Three RGBA16F field textures (composited display + trail ping-pong pair), each usable as both a
    // compute storage-write target and a sampled read. `display` is the one shared with the material.
    let display = images.add(field::field_texture(FIELD_SIZE));
    let trail_a = images.add(field::field_texture(FIELD_SIZE));
    let trail_b = images.add(field::field_texture(FIELD_SIZE));

    // Agent population (seeded once) + zeroed deposit accumulator (one slot per field texel). Both are
    // `ShaderBuffer`s — the default usage (`STORAGE | COPY_DST`) is exactly what the compute chain needs.
    let agents = buffers.add(ShaderBuffer::from(agents::seed_agents(FIELD_SIZE as f32)));
    let deposit = buffers.add(ShaderBuffer::from(vec![0u32; (FIELD_SIZE * FIELD_SIZE) as usize]));

    commands.insert_resource(MoldImages { display: display.clone(), trail_a, trail_b });
    commands.insert_resource(MoldBuffers { agents, deposit });
    commands.insert_resource(MoldParams {
        world_origin: WORLD_ORIGIN,
        world_extent: WORLD_EXTENT,
        field_res: Vec2::splat(FIELD_SIZE as f32),
        time: 0.0,
        agent_count: agents::AGENT_COUNT,
        sense_angle: SENSE_ANGLE,
        sense_dist: SENSE_DIST,
        rotate_angle: ROTATE_ANGLE,
        step_size: STEP_SIZE,
        deposit_amount: DEPOSIT_AMOUNT,
        decay: DECAY,
        trail_max: TRAIL_MAX,
        deposit_scale: DEPOSIT_SCALE,
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
