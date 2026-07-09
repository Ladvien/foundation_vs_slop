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
mod control;
mod field;
mod material;
mod pipeline;

use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::{ShaderType, TextureFormat};
use bevy::render::storage::ShaderBuffer;
use bevy::render::RenderApp;
use serde::Deserialize;

use crate::dungeon::Wall;

pub use material::{MoldFloorMaterial, MoldWallMaterial};

/// Compute workgroup edge (8×8 = 64 threads), matching the Bevy game-of-life reference. `field_size`
/// must be a whole multiple of this so the dispatch covers every texel exactly (see [`validate_config`]).
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

/// Side length of the CPU-written control texture — one texel per dungeon cell (the dungeon is
/// `192×192` tiles), which is all the resolution the world-state hooks need.
pub const CONTROL_SIZE: u32 = 192;

/// Control-texture format. CPU-written each `Update`, compute-read. Channels:
/// `R` = chemoattractant (blood pools, nests) · `G` = light/gaze repellent (fog-visible cells, attenuated
/// by habituation) · `B` = disturbance (squad proximity) · `A` = walkable mask (1 on floor, 0 over void).
pub const CONTROL_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// Fixed-point factor for the integer deposit accumulator: agents `atomicAdd(deposit_amount * SCALE)`, the
/// diffuse pass reads back `/ SCALE`. Large enough to preserve fractional deposits under heavy overlap.
/// Not a knob — a numerical detail of the atomic accumulator.
const DEPOSIT_SCALE: f32 = 1024.0;

/// Every tunable knob of the mold, from the `mycelia:` slice of `assets/config/config.ron`.
///
/// Structural constants ([`WORKGROUP_SIZE`], [`WORLD_ORIGIN`]/[`WORLD_EXTENT`], [`CONTROL_SIZE`], the
/// texture formats) stay in code — they are wired into the dispatch geometry and the world mapping, not
/// aesthetics. Everything here is an aesthetic or behavioural dial you can retune by editing the RON.
///
/// One path, no fallback: there is deliberately **no `Default` impl**. A missing or malformed slice is a
/// loud startup panic via [`validate_config`], never a silent default mold.
///
/// # The three layers
/// **Transport (the "mind").** Distances/angles are in field texels (1024² over 192 m ≈ 5.3 texels/m) and
/// radians. Jones' three-sensor model: arXiv 1503.06579 / 10.1162/artl.2010.16.2.16202.
///
/// **Field (the "flesh").** Gray-Scott: U (substrate) is consumed by V (biomass) via the autocatalytic
/// `U + 2V → 3V`; U is replenished at `feed`, V removed at `feed + kill`. `(feed, kill) = (0.036, 0.060)`
/// sits in the coral-growth regime — blooms creep outward rather than freezing into static spots. V is
/// nucleated by the trail, so blooms grow *along the veins*.
/// Refs: Turk (1991) SIGGRAPH 10.1145/122718.122749 (RD as surface texture synthesis, precisely this use);
/// Leppänen et al. (2004) 10.1590/S0103-97332004000300006; Maini & Painter (1997) 10.1039/a702602a;
/// Pearson (1993) Science 261 (the canonical `(F, k)` regime map). Flow-Lenia (arXiv 2212.07906) is the
/// mass-conserving generalization, deferred past v1.
///
/// **Reactivity (the "sentience").** Each gain is in trail units, so it competes directly with the scent an
/// agent senses: an attractant of 6.0 outweighs a mid-strength vein, a repellent of 9.0 overrides a strong
/// one. Photophobia stands in for Physarum's light-avoidance — the game has no dynamic lights, so
/// fog-of-war "a unit can see this cell" is the light/gaze proxy. Habituation follows Boisseau, Vogel &
/// Dussutour (2016), 10.1098/rspb.2016.0446: *P. polycephalum* learns to ignore a repeatedly-presented
/// *harmless* repellent, showing both responsiveness decline AND spontaneous recovery once it is withheld.
#[derive(Resource, Deserialize, Clone, Debug)]
pub struct MyceliaConfig {
    // ── Field geometry ────────────────────────────────────────────────────────────────────────────────
    /// Side length (texels) of the square world-space mold field. Must be a multiple of [`WORKGROUP_SIZE`].
    /// 1024² over the 192 m footprint ≈ 5.3 texels/tile. The dominant perf dial (cost scales with area).
    pub field_size: u32,
    /// Number of walking agents. Sparse on purpose (≈0.05/texel at 1024²) so the trail forms legible
    /// foraging *channels* rather than flooding to uniform saturation. An aesthetic ceiling, not a
    /// performance one — the GPU handles far more.
    pub agent_count: u32,

    // ── Transport layer (Physarum) ────────────────────────────────────────────────────────────────────
    /// Half-angle between the centre sensor and each side sensor (radians).
    pub sense_angle: f32,
    /// How far ahead (texels) the sensors sample.
    pub sense_dist: f32,
    /// How sharply an agent turns toward the stronger signal each tick (radians).
    pub rotate_angle: f32,
    /// Texels an agent advances per tick.
    pub step_size: f32,
    /// Scent laid down per agent per tick (pre-scale), in trail units.
    pub deposit_amount: f32,
    /// How far the trail is lerped toward its 3×3 mean each tick — the *diffusion rate*, NOT a full blur.
    /// Replacing the trail outright with its mean (weight 1.0) divides every deposit spike by ~9 each tick,
    /// so no channel can ever accumulate and the network never persists. A small weight lets scent spread
    /// just enough to attract neighbouring agents while the ridge stays sharp.
    pub diffuse_weight: f32,
    /// Multiplicative trail persistence per tick (`<1` so trails fade). Slow, so a route that keeps getting
    /// walked accumulates into a bright durable channel while a route walked once fades back to dark.
    /// With `diffuse_weight` this is the Jones/Lague diffuse→decay formulation.
    pub decay: f32,
    /// Upper clamp on trail intensity. Decay alone bounds the steady state at ≈ `deposit/(1-decay)`; this
    /// guards against transient spikes / NaNs.
    pub trail_max: f32,

    // ── Field layer (Gray-Scott) ──────────────────────────────────────────────────────────────────────
    /// Integration step. Gray-Scott with `d_u = 0.16` is stable at `dt = 1` on a unit grid.
    pub dt: f32,
    /// Substrate replenishment rate `F`.
    pub feed: f32,
    /// Biomass removal rate `k`.
    pub kill: f32,
    /// Diffusion rate of the substrate `U`.
    pub d_u: f32,
    /// Diffusion rate of the biomass `V` (half of `U` — the ratio is what makes the Turing instability).
    pub d_v: f32,
    /// How strongly a strong vein nucleates biomass beneath it each tick, keeping blooms tethered to the
    /// network instead of appearing at random.
    pub bloom_seed: f32,

    // ── Reactivity (the "sentience") ──────────────────────────────────────────────────────────────────
    /// How strongly a cell a unit currently *sees* repels agents.
    pub photophobia: f32,
    /// How strongly blood pools and nests attract foraging agents.
    pub chemo_gain: f32,
    /// How strongly a squad unit's immediate presence disturbs the mold, scattering agents away.
    pub disturbance_gain: f32,
    /// How strongly non-walkable void repels agents. Rather than hard-blocking movement — which would
    /// strand every agent that seeded inside a wall, forever — the void is merely *unattractive*. Agents on
    /// the floor hug it and thread down corridors; the few that start in the void steer themselves onto the
    /// nearest floor within a second. One path, self-healing.
    pub wall_repel: f32,
    /// How strongly the damp, dark, sheltered floor beside a wall *attracts* foraging agents. Real mold
    /// pools in corners; without this the `wall_repel` term alone drives it toward corridor centres, which
    /// is exactly backwards. Note this is an attraction to wall-adjacent *floor*, not to the wall cell
    /// itself — the two terms together make agents hug the wall face rather than enter or flee it.
    pub wall_affinity: f32,
    /// How far (in cells) the wall's influence reaches out across the floor. `1` = only the cells touching a
    /// wall; larger values pull mold in from further out.
    pub wall_reach: f32,
    /// Habituation gained per second while a cell is watched.
    pub hab_rate: f32,
    /// Habituation lost per second while unwatched — the "spontaneous recovery" of the 2016 result. Slower
    /// than `hab_rate`, so the mold's fear returns gradually.
    pub hab_recover: f32,
    /// Ceiling on how much habituation can blunt the gaze. Below 1.0 so a watched cell is never *fully*
    /// ignored — the mold gets bolder, not blind.
    pub hab_strength: f32,

    // ── Appearance ────────────────────────────────────────────────────────────────────────────────────
    /// Multiplier on the veins' emissive bioluminescence. Held low because the camera is **LDR** (no `hdr`,
    /// no `Bloom`) and the scene is brightly lit (`AmbientLight` brightness 500 + a 2500-lux directional),
    /// so the default TonyMcMapface tonemapper clips anything much above mid-grey straight to white. Emissive
    /// above ~1.0 stops reading as sickly phosphorescence and becomes a flat white tube.
    pub glow_gain: f32,
    /// Master opacity dial for the whole coating (`0` = invisible, `1` = full).
    pub intensity: f32,
    /// Trail value at which a vein begins to show.
    pub vein_lo: f32,
    /// Trail value at which a vein is fully lit. Must exceed `vein_lo`.
    pub vein_hi: f32,
    /// Strength of the normal perturbation derived from the mold's thickness field. This is what stops the
    /// coating reading as a flat decal: the biomass becomes a lumpy, lit surface. `0` = perfectly flat.
    pub normal_strength: f32,
    /// Perceptual roughness where the mold is thickest. The carpet is `0.95`; a wet biofilm is far glossier,
    /// so a low value here is what sells "wet" under the directional light.
    pub wet_roughness: f32,
    /// How far (world units) the mold creeps up a wall from the floor before fading out. Walls are 2.4 tall.
    pub climb_height: f32,
}

/// Validate the `mycelia:` slice. One path: any violation is an `Err` that [`crate::config`] surfaces as a
/// loud startup panic — there is no clamping to a "safe" value, because a silently-corrected knob is
/// exactly the kind of magic result that is hard to trace back.
pub fn validate_config(c: &MyceliaConfig) -> Result<(), String> {
    let positive = |name: &str, v: f32| -> Result<(), String> {
        if v > 0.0 && v.is_finite() { Ok(()) } else { Err(format!("mycelia.{name} must be > 0, got {v}")) }
    };
    let unit = |name: &str, v: f32| -> Result<(), String> {
        if (0.0..=1.0).contains(&v) { Ok(()) } else { Err(format!("mycelia.{name} must be in 0..=1, got {v}")) }
    };
    let non_negative = |name: &str, v: f32| -> Result<(), String> {
        if v >= 0.0 && v.is_finite() { Ok(()) } else { Err(format!("mycelia.{name} must be >= 0, got {v}")) }
    };

    if c.field_size == 0 || c.field_size % WORKGROUP_SIZE != 0 {
        return Err(format!(
            "mycelia.field_size must be a non-zero multiple of {WORKGROUP_SIZE}, got {}",
            c.field_size
        ));
    }
    if c.agent_count == 0 {
        return Err("mycelia.agent_count must be > 0".to_string());
    }
    // The deposit accumulator is one u32 per field texel; keep the allocation sane.
    if c.field_size > 4096 {
        return Err(format!("mycelia.field_size must be <= 4096, got {}", c.field_size));
    }

    positive("sense_dist", c.sense_dist)?;
    positive("step_size", c.step_size)?;
    positive("deposit_amount", c.deposit_amount)?;
    positive("trail_max", c.trail_max)?;
    positive("dt", c.dt)?;
    non_negative("sense_angle", c.sense_angle)?;
    non_negative("rotate_angle", c.rotate_angle)?;
    non_negative("bloom_seed", c.bloom_seed)?;
    non_negative("photophobia", c.photophobia)?;
    non_negative("chemo_gain", c.chemo_gain)?;
    non_negative("disturbance_gain", c.disturbance_gain)?;
    non_negative("wall_repel", c.wall_repel)?;
    non_negative("wall_affinity", c.wall_affinity)?;
    non_negative("glow_gain", c.glow_gain)?;
    non_negative("hab_rate", c.hab_rate)?;
    non_negative("hab_recover", c.hab_recover)?;
    non_negative("normal_strength", c.normal_strength)?;
    non_negative("climb_height", c.climb_height)?;
    positive("wall_reach", c.wall_reach)?;

    unit("hab_strength", c.hab_strength)?;
    unit("intensity", c.intensity)?;
    unit("diffuse_weight", c.diffuse_weight)?;
    // Bevy clamps roughness into [0.089, 1.0]; anything outside is a config mistake, not an intent.
    if !(0.089..=1.0).contains(&c.wet_roughness) {
        return Err(format!("mycelia.wet_roughness must be in 0.089..=1.0, got {}", c.wet_roughness));
    }
    // Climbing past the wall top is meaningless; walls are `dungeon::WALL_HEIGHT` tall.
    if c.climb_height > crate::dungeon::WALL_HEIGHT {
        return Err(format!(
            "mycelia.climb_height ({}) exceeds wall height ({})",
            c.climb_height,
            crate::dungeon::WALL_HEIGHT
        ));
    }

    // `decay >= 1` never fades: the trail saturates to `trail_max` everywhere and the network dissolves
    // into a flat flood. `decay <= 0` erases it every tick. Both are degenerate, not merely ugly.
    if !(c.decay > 0.0 && c.decay < 1.0) {
        return Err(format!("mycelia.decay must be in (0, 1) exclusive, got {}", c.decay));
    }
    // Gray-Scott is only a pattern-former with unequal diffusion; `d_v >= d_u` kills the Turing instability.
    positive("d_u", c.d_u)?;
    positive("d_v", c.d_v)?;
    if c.d_v >= c.d_u {
        return Err(format!("mycelia.d_v ({}) must be < d_u ({}) for Turing patterns", c.d_v, c.d_u));
    }
    positive("feed", c.feed)?;
    positive("kill", c.kill)?;
    if c.vein_hi <= c.vein_lo {
        return Err(format!("mycelia.vein_hi ({}) must exceed vein_lo ({})", c.vein_hi, c.vein_lo));
    }
    Ok(())
}

/// The mold's field textures. Only `display` crosses to the material; the trail and biomass pairs live
/// purely to feed the compute chain. All are extracted so `prepare_bind_group` can bind them.
#[derive(Resource, Clone, ExtractResource)]
pub struct MoldImages {
    /// The composited output the floor material samples: straight premultiplied-ish `RGB` colour plus an
    /// `A` coverage mask, written by the `field` pass (the last in the chain). Not channel-encoded state —
    /// the material samples it and blends it over the floor directly.
    pub display: Handle<Image>,
    /// Trail-scent ping-pong pair. Each tick one is the read source (sensed by agents + blurred by
    /// diffuse) and the other the write target; they swap by parity so diffusion never reads what it is
    /// concurrently writing. `R` channel holds trail intensity.
    pub trail_a: Handle<Image>,
    pub trail_b: Handle<Image>,
    /// Gray-Scott biomass ping-pong pair. `R` = substrate `U`, `G` = biomass `V`. Same parity swap as the
    /// trail — the reaction-diffusion stencil reads a 3×3 neighbourhood, so it cannot write in place.
    pub biomass_a: Handle<Image>,
    pub biomass_b: Handle<Image>,
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
    /// Control-texture resolution in texels (one per dungeon cell). Kept with the other `vec2`s so every
    /// following scalar stays naturally aligned.
    pub control_res: Vec2,
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
    /// Gray-Scott integration step.
    pub dt: f32,
    /// Gray-Scott substrate replenishment rate.
    pub feed: f32,
    /// Gray-Scott biomass removal rate.
    pub kill: f32,
    /// Gray-Scott diffusion rate of substrate `U`.
    pub d_u: f32,
    /// Gray-Scott diffusion rate of biomass `V`.
    pub d_v: f32,
    /// Per-tick biomass nucleation beneath a strong vein.
    pub bloom_seed: f32,
    /// Lerp factor toward the trail's 3×3 mean each tick (the diffusion rate).
    pub diffuse_weight: f32,
    /// Repulsion from cells a unit currently sees (control `G`).
    pub photophobia: f32,
    /// Attraction to blood pools and nests (control `R`).
    pub chemo_gain: f32,
    /// Repulsion from squad proximity (control `B`).
    pub disturbance_gain: f32,
    /// Repulsion from non-walkable void (inverse of control `A`).
    pub wall_repel: f32,
    /// Attraction to wall-adjacent floor (the static wall-proximity field).
    pub wall_affinity: f32,
    /// Trail value at which a vein begins to show (drives biomass nucleation).
    pub vein_lo: f32,
    /// Trail value at which a vein is fully lit.
    pub vein_hi: f32,
}

pub struct MyceliaPlugin;

impl Plugin for MyceliaPlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback. The `mycelia:` slice comes from the unified
        // `GameConfig` that `ConfigPlugin` (registered first) has already validated.
        let config = app.world().resource::<crate::config::GameConfig>().mycelia.clone();
        app.insert_resource(config);

        app.add_plugins((
            MaterialPlugin::<MoldFloorMaterial>::default(),
            MaterialPlugin::<MoldWallMaterial>::default(),
            ExtractResourcePlugin::<MoldImages>::default(),
            ExtractResourcePlugin::<MoldBuffers>::default(),
            ExtractResourcePlugin::<MoldParams>::default(),
            ExtractResourcePlugin::<control::MoldControlImage>::default(),
        ))
        // `setup_mycelia` binds the control texture into the floor material, so the control textures must
        // exist first.
        .add_systems(Startup, (control::setup_control, setup_mycelia).chain())
        .add_systems(Update, (advance_mold_time, control::write_control, coat_walls));

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
    cfg: Res<MyceliaConfig>,
    control: Res<control::MoldControlImage>,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<MoldFloorMaterial>>,
) {
    let size = cfg.field_size;

    // Five RGBA16F field textures (composited display + trail and biomass ping-pong pairs), each usable as
    // both a compute storage-write target and a sampled read. `display` is the one shared with the material.
    let display = images.add(field::field_texture(size));
    let trail_a = images.add(field::field_texture(size));
    let trail_b = images.add(field::field_texture(size));
    let biomass_a = images.add(field::field_texture(size));
    let biomass_b = images.add(field::field_texture(size));

    // Agent population (seeded once) + zeroed deposit accumulator (one slot per field texel). Both are
    // `ShaderBuffer`s — the default usage (`STORAGE | COPY_DST`) is exactly what the compute chain needs.
    let agents = buffers.add(ShaderBuffer::from(agents::seed_agents(size as f32, cfg.agent_count)));
    let deposit = buffers.add(ShaderBuffer::from(vec![0u32; (size * size) as usize]));

    commands
        .insert_resource(MoldImages { display: display.clone(), trail_a, trail_b, biomass_a, biomass_b });
    commands.insert_resource(MoldBuffers { agents, deposit });
    commands.insert_resource(MoldParams {
        world_origin: WORLD_ORIGIN,
        world_extent: WORLD_EXTENT,
        field_res: Vec2::splat(size as f32),
        control_res: Vec2::splat(CONTROL_SIZE as f32),
        time: 0.0,
        agent_count: cfg.agent_count,
        sense_angle: cfg.sense_angle,
        sense_dist: cfg.sense_dist,
        rotate_angle: cfg.rotate_angle,
        step_size: cfg.step_size,
        deposit_amount: cfg.deposit_amount,
        decay: cfg.decay,
        trail_max: cfg.trail_max,
        deposit_scale: DEPOSIT_SCALE,
        dt: cfg.dt,
        feed: cfg.feed,
        kill: cfg.kill,
        d_u: cfg.d_u,
        d_v: cfg.d_v,
        bloom_seed: cfg.bloom_seed,
        diffuse_weight: cfg.diffuse_weight,
        photophobia: cfg.photophobia,
        chemo_gain: cfg.chemo_gain,
        disturbance_gain: cfg.disturbance_gain,
        wall_repel: cfg.wall_repel,
        wall_affinity: cfg.wall_affinity,
        vein_lo: cfg.vein_lo,
        vein_hi: cfg.vein_hi,
    });

    // A single translucent overlay quad covering the whole floor footprint, sitting a hair above the
    // floor (Y=0) so it composites over the carpet without z-fighting. One mesh, sampled by world XZ, so it
    // needs no per-tile material and is untouched by the fog's bright/dim floor swap.
    let mesh = meshes.add(Plane3d::default().mesh().size(WORLD_EXTENT.x, WORLD_EXTENT.y));
    let material = materials.add(MoldFloorMaterial {
        base: material::floor_base(),
        extension: material::MoldFloorExt::new(&cfg, display, control.dynamic.clone()),
    });
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

/// Swap every wall's `StandardMaterial` for a mold-aware [`MoldWallMaterial`], once, as soon as the dungeon
/// has spawned its tiles.
///
/// Doing it here rather than in `dungeon::spawn_tiles` keeps `dungeon` from having to know that `mycelia`
/// exists — the alternative would be an ordering dependency where the dungeon reads a `MoldImages` resource
/// at startup. The swap preserves the wall's original `StandardMaterial` (wallpaper texture, roughness) as
/// the `base` of the extension, so the wall still looks exactly like a wall wherever no mold has reached.
///
/// Safe because nothing else reads wall materials: the fog reveals walls via `Visibility`, and its
/// material-swap query is explicitly floor-only (`Without<Wall>`).
fn coat_walls(
    mut commands: Commands,
    mut done: Local<bool>,
    cfg: Res<MyceliaConfig>,
    images: Res<MoldImages>,
    control: Res<control::MoldControlImage>,
    std_materials: Res<Assets<StandardMaterial>>,
    mut wall_materials: ResMut<Assets<MoldWallMaterial>>,
    walls: Query<(Entity, &MeshMaterial3d<StandardMaterial>), With<Wall>>,
) {
    if *done {
        return;
    }
    // Every wall shares one `StandardMaterial` handle, so read the base off whichever we see first and
    // build a single extended material for all of them. If the tiles haven't spawned yet, try again next
    // frame — this system disables itself the moment it succeeds.
    let Some((_, first)) = walls.iter().next() else {
        return;
    };
    let Some(base) = std_materials.get(&first.0) else {
        return;
    };

    let coated = wall_materials.add(MoldWallMaterial {
        base: base.clone(),
        extension: material::MoldWallExt::new(
            &cfg,
            images.display.clone(),
            control.dynamic.clone(),
        ),
    });

    for (entity, _) in &walls {
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(coated.clone()));
    }
    *done = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A known-good config, matching the shipped `mycelia:` slice.
    fn valid() -> MyceliaConfig {
        MyceliaConfig {
            field_size: 1024,
            agent_count: 55_000,
            sense_angle: 0.40,
            sense_dist: 9.0,
            rotate_angle: 0.50,
            step_size: 1.0,
            deposit_amount: 1.0,
            diffuse_weight: 0.18,
            decay: 0.96,
            trail_max: 24.0,
            dt: 1.0,
            feed: 0.036,
            kill: 0.060,
            d_u: 0.16,
            d_v: 0.08,
            bloom_seed: 0.06,
            photophobia: 9.0,
            chemo_gain: 6.0,
            disturbance_gain: 5.0,
            wall_repel: 12.0,
            wall_affinity: 5.0,
            wall_reach: 2.5,
            hab_rate: 0.35,
            hab_recover: 0.08,
            hab_strength: 0.75,
            glow_gain: 1.0,
            intensity: 1.0,
            vein_lo: 3.0,
            vein_hi: 12.0,
            normal_strength: 6.0,
            wet_roughness: 0.22,
            climb_height: 0.85,
        }
    }

    #[test]
    fn shipped_defaults_validate() {
        assert!(validate_config(&valid()).is_ok());
    }

    /// `field_size` must tile the 8×8 workgroup exactly, or the 2D dispatch misses texels.
    #[test]
    fn field_size_must_tile_the_workgroup() {
        let mut c = valid();
        c.field_size = 1020; // not a multiple of 8 (1020 / 8 = 127.5)
        assert!(validate_config(&c).is_err());
        c.field_size = 0;
        assert!(validate_config(&c).is_err());
        c.field_size = 8192; // over the allocation cap
        assert!(validate_config(&c).is_err());
    }

    /// `decay >= 1` never fades (the trail floods to `trail_max` everywhere and the network dissolves);
    /// `decay <= 0` erases it every tick. Both are degenerate, so both must be rejected loudly.
    #[test]
    fn decay_must_be_strictly_between_zero_and_one() {
        for bad in [0.0, 1.0, 1.5, -0.1] {
            let mut c = valid();
            c.decay = bad;
            assert!(validate_config(&c).is_err(), "decay={bad} should be rejected");
        }
    }

    /// Gray-Scott only forms patterns with *unequal* diffusion: `d_v >= d_u` kills the Turing instability.
    #[test]
    fn biomass_must_diffuse_slower_than_substrate() {
        let mut c = valid();
        c.d_v = c.d_u;
        assert!(validate_config(&c).is_err());
        c.d_v = c.d_u + 0.01;
        assert!(validate_config(&c).is_err());
    }

    /// Climbing past the top of a wall is meaningless, and `wet_roughness` outside Bevy's clamp range is a
    /// config mistake rather than an intent.
    #[test]
    fn surface_dials_are_bounded_by_physical_reality() {
        let mut c = valid();
        c.climb_height = crate::dungeon::WALL_HEIGHT + 0.1;
        assert!(validate_config(&c).is_err());

        for bad in [0.0, 0.05, 1.5] {
            let mut c = valid();
            c.wet_roughness = bad;
            assert!(validate_config(&c).is_err(), "wet_roughness={bad} should be rejected");
        }

        let mut c = valid();
        c.wall_reach = 0.0; // a zero reach would divide by zero in the falloff
        assert!(validate_config(&c).is_err());
    }

    /// An inverted vein window would make `smoothstep` degenerate.
    #[test]
    fn vein_window_must_be_ordered() {
        let mut c = valid();
        c.vein_hi = c.vein_lo;
        assert!(validate_config(&c).is_err());
    }

    /// Unit-range dials are rejected outside `0..=1` rather than silently clamped.
    #[test]
    fn unit_range_dials_are_not_clamped() {
        for bad in [-0.1, 1.1] {
            let mut c = valid();
            c.intensity = bad;
            assert!(validate_config(&c).is_err(), "intensity={bad} should be rejected");

            let mut c = valid();
            c.diffuse_weight = bad;
            assert!(validate_config(&c).is_err(), "diffuse_weight={bad} should be rejected");

            let mut c = valid();
            c.hab_strength = bad;
            assert!(validate_config(&c).is_err(), "hab_strength={bad} should be rejected");
        }
    }

    /// NaN must not sneak past the comparisons (`v > 0.0` is false for NaN, but be explicit about it).
    #[test]
    fn nan_is_rejected() {
        let mut c = valid();
        c.sense_dist = f32::NAN;
        assert!(validate_config(&c).is_err());
    }
}

