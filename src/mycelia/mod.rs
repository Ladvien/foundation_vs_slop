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
//! `(Transform, Health)`) never sees it. `MyceliaPlugin` is registered **only** in `lib::run`, never in the
//! headless `sim_harness` (mirroring `UiPlugin`/`DialoguePlugin`), and it **no-ops if the `RenderApp`
//! sub-app is absent** — belt-and-suspenders so a headless build can never touch the render world.
//!
//! There is **exactly one GPU→CPU edge**: `fruit.rs` reads back the coarse biomass grid that the `pin_scan`
//! pass writes, to decide where mushrooms erupt (see [`COARSE_SIZE`]). GPU floats are not bit-reproducible
//! across hardware, so this puts fruit-body positions in the same non-determinism class as the Avian
//! physics and FX layers. That is safe for the replay oracle for the same reason `gore::GibChunk` is: a
//! `FruitBody` carries a `Transform` but never a `Health`. Everything else is still strictly CPU→GPU.
//!
//! ## References (home-still corpus)
//! Jones multi-agent Physarum (arXiv 1503.06579; 10.1080/17445760.2015.1085535); foraging survey
//! (10.1007/s10462-021-10112-1). Field growth: Gray-Scott / Turing reaction-diffusion; Flow-Lenia
//! (arXiv 2212.07906) for the mass-conserving multi-species extension (deferred past v1).

mod agents;
mod control;
mod field;
pub mod fruit;
mod material;
mod measure;
pub mod perceptual;
mod pipeline;

use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::{ShaderType, TextureFormat};
use bevy::render::storage::ShaderBuffer;
use bevy::render::RenderApp;
use serde::Deserialize;

use crate::dungeon::Wall;

pub use fruit::FruitBody;
pub use material::{MoldFloorMaterial, MoldFruitMaterial, MoldWallMaterial};

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

/// Side length of the coarse biomass grid that `pin_scan` reduces the `field_size²` biomass field into, and
/// which is the module's **only** GPU→CPU channel (see `fruit.rs`).
///
/// Each coarse cell max-pools a `field_size / COARSE_SIZE` block and reports the winning texel's `(V, U)`
/// *and its exact field coordinates*, so a fruit body is placed at full field precision (0.19 world units)
/// even though the grid it was found on is coarse (1.5 world units per cell at 1024²/128²).
///
/// Every output slot is written by exactly one thread, so there are no atomics, no clear pass, and — unlike
/// an `atomicAdd`-appended candidate list — the readback's ordering is deterministic.
///
/// `field_size` must be a whole multiple of this (see [`validate_config`]), or the block reduction would
/// leave a ragged edge of unscanned texels.
pub const COARSE_SIZE: u32 = 128;

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
    /// How many simulation ticks per second the mold advances. **Not a performance dial — a biology dial**,
    /// and the single dial that sets how fast the mold visibly grows: every velocity in the chain (agent
    /// step, Gray-Scott advance) is per-tick, so all of them scale with it.
    ///
    /// The shipped value is **measured, not chosen**. `measure.rs` reads the display texture back and
    /// computes the biomass margin's normal speed by the level-set formula `|∂V/∂t| / |∇V|`, and the
    /// budget is [`perceptual::v_max`] at the tightest zoom. At 6 Hz the mold ran at 23.1 mm/s — nearly
    /// seven times the 3.33 mm/s object-relative motion threshold, i.e. plainly visible if you looked. At
    /// 1.5 Hz it runs at 2.92 mm/s, just below. See the `mycelia.sim_hz` comment in `config.ron` for the
    /// full sweep and how to reproduce it.
    pub sim_hz: f32,
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
    /// How strongly rock repels agents *at sensor range*. Movement into rock is hard-blocked in
    /// `agent_step`, but a sensor reaches ~1.7 cells ahead — this is what lets an agent see a wall coming
    /// and turn early rather than driving into the face and bouncing. It steers; it does not gate. Without
    /// it the mold piles against every wall.
    pub wall_repel: f32,
    /// How strongly the damp, dark, sheltered floor beside a wall *attracts* foraging agents. Real mold
    /// pools in corners; without this the `wall_repel` term alone drives it toward corridor centres, which
    /// is exactly backwards. Note this is an attraction to wall-adjacent *floor*, not to the wall cell
    /// itself — the two terms together make agents hug the wall face rather than enter or flee it.
    pub wall_affinity: f32,
    /// How violently biomass nucleates on carrion. Blood and nests merely *attract* the mold (`chemo_gain`
    /// steers agents); meat is FOOD, so where the chemoattractant is strong the flesh blooms directly,
    /// without waiting for a vein to establish. This is what makes a fresh gib erupt.
    pub carrion_bloom: f32,
    /// How far (in **world units**) the wall's influence reaches out across the floor. Measured from the
    /// slab surface by an exact distance transform at field resolution, so sub-tile values are meaningful:
    /// `0.6` keeps the pull to roughly the near half of the adjoining tile.
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
    /// Perceptual roughness in the **vein cores only** — the mat itself stays matte (~0.92). Mycelium is a
    /// fibrous, light-scattering felt, not a fluid: a low roughness applied across the whole sheet is
    /// precisely what makes it read as spilled liquid.
    pub wet_roughness: f32,
    /// How far (world units) the mold creeps up a wall from the floor before fading out. Walls are 2.4 tall.
    pub climb_height: f32,
    /// Spatial frequency of the hyphal filament noise, in cycles per world unit. Higher = finer strands.
    pub fiber_scale: f32,
    /// How hard the filaments carve the surface normal. This supplies the high-frequency structure that the
    /// smooth 1024² field cannot, so `normal_strength` can stay low and stop producing liquid meniscus lobes.
    pub fiber_strength: f32,
    /// How much fbm breaks up the colony's outer contour. `0` = a smooth iso-contour (a meniscus, i.e. a
    /// puddle); higher = the feathery dendritic advancing margin of a real fungal colony. Single strongest
    /// "that is a fungus" cue.
    pub margin_roughness: f32,
    /// Strength of the grazing-angle fuzz rim. Stands in for a sheen/fuzz BRDF lobe, which bevy's
    /// `StandardMaterial` does not have.
    pub sheen_strength: f32,
    /// Strength of the cavity ambient occlusion written into `diffuse_occlusion`. Load-bearing: the scene's
    /// ambient is a bright *uniform* fill, which ignores surface normals entirely, so without an occlusion
    /// term the filaments render flat no matter how hard the normal is perturbed.
    pub ao_strength: f32,

    // ── Fruiting (see `fruit.rs`) ─────────────────────────────────────────────────────────────────────
    /// Biomass `V` above which a texel is a candidate to pin a fruit body.
    ///
    /// Real Agaricomycetes fruit only once a colony has accumulated **critical mycelial mass** *and*
    /// **exhausted its nutrients** — nitrogen starvation is among the strongest maturation cues (Zhang et
    /// al. 2015, 10.1371/journal.pone.0123025; morphogenesis review: Kües & Navarro-González 2015,
    /// 10.1016/j.fbr.2015.05.001). Gray-Scott already integrates exactly those two quantities: `V` is the
    /// biomass and `U` the substrate it consumes. So "thick mat, spent substrate" is `V > v_fruit && U <
    /// u_exhausted` — free, with no new state.
    pub v_fruit: f32,
    /// Substrate `U` below which the patch counts as spent. See [`MyceliaConfig::v_fruit`].
    pub u_exhausted: f32,
    /// How long (seconds) a texel must hold the pin condition, unwatched, before it commits to a primordium.
    pub pin_dwell_secs: f32,
    /// Minimum separation (world units) between fruit bodies. Most hyphal knots never mature; neighbours
    /// compete for translocated nutrient (Kües & Navarro-González 2015). This is that competition.
    pub pin_min_spacing: f32,
    /// Hard ceiling on live fruit bodies. Reaching it is logged, never silently ignored.
    pub max_fruit_bodies: u32,
    /// Scale applied to the death cap mesh, whose native height is 13.9 cm. `2.5` gives a 35 cm mushroom —
    /// knee-high on a squad unit, legible at the RTS zoom the game actually plays at.
    pub body_scale: f32,
    /// Local biomass `V` below which a fruit body's patch has collapsed and the body reabsorbs, running its
    /// growth clock backwards. Primordium abortion, not a fallback branch: the same ODE with a negative
    /// sign. Must be below [`MyceliaConfig::v_fruit`], or a body would begin aborting the instant it pinned.
    pub maintain_v: f32,

    // ── Perception budget (see `perceptual.rs`) ───────────────────────────────────────────────────────
    /// Slowest motion (degrees of visual angle per second) a human reliably detects beside a stationary
    /// reference. Every *autonomous* motion the mold makes is held under this. Being eaten or crushed is
    /// exempt — that is meant to be seen. Leibowitz (1955), 10.1364/josa.45.000829.
    pub motion_threshold_deg_per_s: f32,
    /// Vertical visual angle the game window subtends at the player's eye (a 27" panel at ~60 cm ≈ 30°).
    /// The one number here that depends on the player's desk rather than on the game.
    pub screen_fov_deg_v: f32,
}

/// Whether the compute chain advances this frame. The mold runs on its own slow clock (`sim_hz`), not the
/// render clock; on frames where this is `false` the render node skips every pass and leaves the ping-pong
/// parity alone, so the display texture simply persists.
#[derive(Resource, Clone, Copy, ExtractResource, Default)]
pub struct MoldStep {
    pub step: bool,
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
    // `pin_scan` reduces the field in `field_size / COARSE_SIZE` blocks. A non-integer ratio would leave a
    // ragged strip of texels no coarse cell covers, so mushrooms could never pin there.
    if c.field_size % COARSE_SIZE != 0 {
        return Err(format!(
            "mycelia.field_size ({}) must be a multiple of COARSE_SIZE ({COARSE_SIZE}) so the pin scan's \
             block reduction covers every texel",
            c.field_size
        ));
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
    non_negative("fiber_strength", c.fiber_strength)?;
    non_negative("sheen_strength", c.sheen_strength)?;
    positive("wall_reach", c.wall_reach)?;
    positive("sim_hz", c.sim_hz)?;
    non_negative("carrion_bloom", c.carrion_bloom)?;
    // A zero fiber frequency collapses the filament noise to a single constant sample: no strands at all.
    positive("fiber_scale", c.fiber_scale)?;

    unit("hab_strength", c.hab_strength)?;
    unit("intensity", c.intensity)?;
    unit("diffuse_weight", c.diffuse_weight)?;
    unit("ao_strength", c.ao_strength)?;
    // Above 1.0 the margin noise swamps the coat term entirely and mold appears in bare corridors.
    unit("margin_roughness", c.margin_roughness)?;
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

    // ── Fruiting ──────────────────────────────────────────────────────────────────────────────────────
    // `v_fruit`/`u_exhausted`/`maintain_v` are Gray-Scott concentrations, clamped to 0..=1 in the shader.
    unit("v_fruit", c.v_fruit)?;
    unit("u_exhausted", c.u_exhausted)?;
    unit("maintain_v", c.maintain_v)?;
    positive("pin_dwell_secs", c.pin_dwell_secs)?;
    positive("pin_min_spacing", c.pin_min_spacing)?;
    positive("body_scale", c.body_scale)?;
    if c.max_fruit_bodies == 0 {
        return Err("mycelia.max_fruit_bodies must be > 0".to_string());
    }
    // A body pins at `v_fruit` and reabsorbs below `maintain_v`. If the two crossed, every primordium would
    // begin aborting on the frame it committed — a mushroom that flickers rather than one that grows.
    if c.maintain_v >= c.v_fruit {
        return Err(format!(
            "mycelia.maintain_v ({}) must be below v_fruit ({}), or every pin aborts the instant it commits",
            c.maintain_v, c.v_fruit
        ));
    }
    // The pin condition is a conjunction: thick mat AND spent substrate. Gray-Scott's `U + 2V -> 3V` keeps
    // `U + V` near 1 in the reacting region, so demanding `V > v_fruit` while `U < u_exhausted` is only
    // satisfiable when the two thresholds leave room between them. `v_fruit + u_exhausted <= 1` guarantees
    // a texel can hold both at once; above that the mold would grow forever and never fruit, silently.
    if c.v_fruit + c.u_exhausted > 1.0 {
        return Err(format!(
            "mycelia.v_fruit ({}) + u_exhausted ({}) exceeds 1.0; no texel can satisfy both, so nothing \
             would ever fruit",
            c.v_fruit, c.u_exhausted
        ));
    }

    // ── Perception budget ─────────────────────────────────────────────────────────────────────────────
    // Both feed a division in `perceptual::v_max`; zero or negative means an infinite or reversed budget.
    positive("motion_threshold_deg_per_s", c.motion_threshold_deg_per_s)?;
    positive("screen_fov_deg_v", c.screen_fov_deg_v)?;
    // A threshold above ~1 deg/s is no longer "below the ability to notice" — it is plainly visible drift.
    // This is a guard against a fat-fingered decimal point, not a taste boundary.
    if c.motion_threshold_deg_per_s > 1.0 {
        return Err(format!(
            "mycelia.motion_threshold_deg_per_s ({}) is far above the ~0.02 deg/s object-relative motion \
             threshold (Leibowitz 1955); the mold would visibly crawl",
            c.motion_threshold_deg_per_s
        ));
    }
    if !(1.0..=180.0).contains(&c.screen_fov_deg_v) {
        return Err(format!(
            "mycelia.screen_fov_deg_v ({}) must be a plausible vertical field of view in degrees",
            c.screen_fov_deg_v
        ));
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
    /// `array<vec4<f32>, COARSE_SIZE²>` — the mold's only reading back to the CPU. Written by the `pin_scan`
    /// pass, one slot per thread; each entry is `(max V in block, U at that texel, texel x, texel y)`.
    /// `fruit.rs` attaches a [`bevy::render::gpu_readback::Readback`] to it and grows mushrooms from it.
    pub coarse: Handle<ShaderBuffer>,
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
    /// Direct biomass nucleation rate on carrion (control `R`). See [`MyceliaConfig::carrion_bloom`].
    pub carrion_bloom: f32,
    /// Trail value at which a vein begins to show (drives biomass nucleation).
    pub vein_lo: f32,
    /// Trail value at which a vein is fully lit.
    pub vein_hi: f32,
    /// Side length of the coarse biomass grid the `pin_scan` pass max-pools into. Structural, not a dial —
    /// see [`COARSE_SIZE`].
    pub coarse_res: u32,
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
            ExtractResourcePlugin::<MoldStep>::default(),
            ExtractResourcePlugin::<control::MoldControlImage>::default(),
        ))
        // `setup_mycelia` binds the control texture into the floor material, so the control textures must
        // exist first.
        .init_resource::<MoldStep>()
        .add_systems(Startup, (control::setup_control, setup_mycelia).chain())
        .init_resource::<CoatedFurniture>()
        .add_systems(Update, (advance_mold_time, control::write_control, coat_walls, coat_furniture));

        // Fruit bodies: the mold reproducing. Registered here (not as a separate plugin) because it depends
        // on this plugin's textures, buffers and config, and shares its determinism firewall.
        fruit::build(app);
        // Dev calibration instrument. No-ops unless MYCELIA_MEASURE is set in the environment.
        measure::build(app);

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
///
/// Takes [`Dungeon`] because agents must be seeded on floor (see [`agents::seed_agents`]). That resource is
/// inserted in `DungeonPlugin::build`, before any schedule runs, so it is available to every `Startup`
/// system regardless of plugin order.
fn setup_mycelia(
    mut commands: Commands,
    cfg: Res<MyceliaConfig>,
    dungeon: Res<crate::dungeon::Dungeon>,
    control: Res<control::MoldControlImage>,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<MoldFloorMaterial>>,
) -> Result<(), BevyError> {
    let size = cfg.field_size;

    // The control texture is one texel per dungeon cell, and every texel<->cell map in the compute chain
    // assumes that exactly. A dungeon sized differently would silently sample a misaligned control texture,
    // so refuse to start rather than render a plausible lie.
    if dungeon.width != CONTROL_SIZE as usize || dungeon.height != CONTROL_SIZE as usize {
        return Err(format!(
            "mycelia: CONTROL_SIZE is {CONTROL_SIZE} but the dungeon is {}x{}; the control texture is one \
             texel per cell, so these must match",
            dungeon.width, dungeon.height
        )
        .into());
    }

    // Five RGBA16F field textures (composited display + trail and biomass ping-pong pairs), each usable as
    // both a compute storage-write target and a sampled read. `display` is the one shared with the material.
    let display = images.add(field::field_texture(size));
    let trail_a = images.add(field::field_texture(size));
    let trail_b = images.add(field::field_texture(size));
    let biomass_a = images.add(field::field_texture(size));
    let biomass_b = images.add(field::field_texture(size));

    // Agent population (seeded once, on floor only) + zeroed deposit accumulator (one slot per field texel).
    // Both are `ShaderBuffer`s — the default usage (`STORAGE | COPY_DST`) is exactly what the chain needs.
    let walkable: Vec<bool> = (0..dungeon.height)
        .flat_map(|y| {
            (0..dungeon.width).map(move |x| IVec2::new(x as i32, y as i32))
        })
        .map(|c| dungeon.is_floor(c))
        .collect();
    let seeded = agents::seed_agents(size, cfg.agent_count, &walkable, CONTROL_SIZE)?;
    let agents = buffers.add(ShaderBuffer::from(seeded));
    let deposit = buffers.add(ShaderBuffer::from(vec![0u32; (size * size) as usize]));
    // `vec4<f32>` per coarse cell: (max V, U at that texel, texel x, texel y). Zero-initialised, which reads
    // as "no biomass anywhere" — the true state before the first tick, not a placeholder.
    let coarse =
        buffers.add(ShaderBuffer::from(vec![0.0f32; (COARSE_SIZE * COARSE_SIZE * 4) as usize]));

    commands
        .insert_resource(MoldImages { display: display.clone(), trail_a, trail_b, biomass_a, biomass_b });
    commands.insert_resource(MoldBuffers { agents, deposit, coarse: coarse.clone() });
    // The mold's single GPU→CPU edge. `Readback` copies the buffer every frame it is present; `fruit.rs`
    // observes `ReadbackComplete` on this entity. Cosmetic-only, `Update`-only — see the module header.
    commands
        .spawn((
            Name::new("mycelia_coarse_readback"),
            bevy::render::gpu_readback::Readback::buffer(coarse),
            fruit::CoarseReadback,
        ))
        .observe(fruit::receive_coarse);
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
        carrion_bloom: cfg.carrion_bloom,
        vein_lo: cfg.vein_lo,
        vein_hi: cfg.vein_hi,
        coarse_res: COARSE_SIZE,
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

    Ok(())
}

/// Advance the shared sim clock on the main world each frame; the value is extracted into the render
/// world for the compute pass and reused by the floor material. `Update` (cosmetic), never `FixedUpdate`.
///
/// Uses **real** time (not `Time<Virtual>`), so the mold keeps breathing while the game is paused or at
/// the menu — matching the repo's other cosmetic shaders (`nest`, `vhs`) which animate off `globals.time`.
/// The mold is ambience; it should never freeze with a gameplay pause.
fn advance_mold_time(
    time: Res<Time<Real>>,
    cfg: Res<MyceliaConfig>,
    mut params: ResMut<MoldParams>,
    mut step: ResMut<MoldStep>,
    mut accum: Local<f32>,
) {
    params.time = time.elapsed_secs();

    // Fixed-rate sim clock, decoupled from the render clock. `Time<Real>` so the mold keeps breathing while
    // the game is paused (matching the rest of this module). One tick per period at most: if the frame rate
    // collapses we drop ticks rather than fast-forwarding the mold in a visible surge.
    let period = 1.0 / cfg.sim_hz;
    *accum += time.delta_secs();
    if *accum >= period {
        *accum = (*accum - period).min(period);
        step.step = true;
    } else {
        step.step = false;
    }
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
            sim_hz: 1.5,
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
            carrion_bloom: 0.30,
            wall_reach: 0.6,
            hab_rate: 0.35,
            hab_recover: 0.08,
            hab_strength: 0.75,
            glow_gain: 1.35,
            intensity: 1.0,
            vein_lo: 3.0,
            vein_hi: 12.0,
            normal_strength: 1.1,
            wet_roughness: 0.42,
            climb_height: 0.85,
            fiber_scale: 8.0,
            fiber_strength: 1.6,
            margin_roughness: 0.55,
            sheen_strength: 0.18,
            ao_strength: 0.75,
            v_fruit: 0.35,
            u_exhausted: 0.30,
            pin_dwell_secs: 6.0,
            pin_min_spacing: 1.5,
            max_fruit_bodies: 64,
            body_scale: 2.5,
            maintain_v: 0.20,
            motion_threshold_deg_per_s: 0.02,
            screen_fov_deg_v: 30.0,
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

    /// A body pins at `v_fruit` and reabsorbs below `maintain_v`. Crossed, every primordium would begin
    /// aborting on the frame it committed and the mold would flicker mushrooms rather than grow them.
    #[test]
    fn maintenance_threshold_must_sit_below_the_fruiting_threshold() {
        let mut c = valid();
        c.maintain_v = c.v_fruit;
        assert!(validate_config(&c).is_err());
        c.maintain_v = c.v_fruit + 0.1;
        assert!(validate_config(&c).is_err());
    }

    /// The pin condition is a conjunction — thick mat AND spent substrate. Thresholds that cannot both hold
    /// at once mean nothing ever fruits, which would look exactly like a bug in the scan pass.
    #[test]
    fn fruiting_thresholds_must_be_jointly_satisfiable() {
        let mut c = valid();
        c.v_fruit = 0.8;
        c.u_exhausted = 0.5; // 1.3 > 1.0: no texel can hold V > 0.8 while U < 0.5
        assert!(validate_config(&c).is_err());
    }

    /// The perception budget divides by `screen_fov_deg_v` and scales by `motion_threshold_deg_per_s`; a
    /// zero or absurd value silently produces an infinite growth rate rather than a visibly wrong one.
    #[test]
    fn perception_budget_is_bounded_by_psychophysics() {
        for bad in [0.0, -0.02] {
            let mut c = valid();
            c.motion_threshold_deg_per_s = bad;
            assert!(validate_config(&c).is_err(), "threshold={bad} should be rejected");
        }
        // 20 deg/s is a briskly moving object, not a subliminal one. Catch the misplaced decimal.
        let mut c = valid();
        c.motion_threshold_deg_per_s = 20.0;
        assert!(validate_config(&c).is_err());

        for bad in [0.0, 0.5, 200.0] {
            let mut c = valid();
            c.screen_fov_deg_v = bad;
            assert!(validate_config(&c).is_err(), "fov={bad} should be rejected");
        }
    }

    /// The fruiting dials are rejected outside their physical ranges rather than clamped.
    #[test]
    fn fruiting_dials_are_bounded() {
        let mut c = valid();
        c.max_fruit_bodies = 0;
        assert!(validate_config(&c).is_err());

        for bad in [0.0, -1.0] {
            let mut c = valid();
            c.pin_min_spacing = bad;
            assert!(validate_config(&c).is_err(), "pin_min_spacing={bad} should be rejected");

            let mut c = valid();
            c.body_scale = bad;
            assert!(validate_config(&c).is_err(), "body_scale={bad} should be rejected");

            let mut c = valid();
            c.pin_dwell_secs = bad;
            assert!(validate_config(&c).is_err(), "pin_dwell_secs={bad} should be rejected");
        }

        for bad in [-0.1, 1.1] {
            let mut c = valid();
            c.v_fruit = bad;
            assert!(validate_config(&c).is_err(), "v_fruit={bad} should be rejected");
        }
    }
}


/// Marks a mesh whose `StandardMaterial` has already been swapped for a mold-aware one, so `coat_furniture`
/// never reprocesses it.
#[derive(Component)]
struct MoldCoated;

/// Cache of `StandardMaterial` → coated `MoldWallMaterial`. A dungeon full of couches shares a handful of
/// glTF materials; without this we would mint one extended material per mesh instance.
#[derive(Resource, Default)]
struct CoatedFurniture(std::collections::HashMap<AssetId<StandardMaterial>, Handle<MoldWallMaterial>>);

/// Let the mold climb furniture, using the very same material that climbs walls.
///
/// The wall shader asks only two things of a surface: that it stands on the floor at `y = 0` (so world Y is
/// climb height) and that its outward normal points away from the mold pooled at its foot. A couch satisfies
/// both, so no new shader is needed — a table leg is a very short wall.
///
/// Furniture is instantiated from glTF **asynchronously**, so this cannot be a run-once startup system: it
/// polls, and each mesh is coated exactly once (guarded by [`MoldCoated`]).
#[allow(clippy::too_many_arguments)]
fn coat_furniture(
    mut commands: Commands,
    cfg: Res<MyceliaConfig>,
    images: Res<MoldImages>,
    control: Res<control::MoldControlImage>,
    roots: Query<Entity, With<crate::placement::PlacedIn>>,
    children: Query<&Children>,
    painted: Query<&MeshMaterial3d<StandardMaterial>, Without<MoldCoated>>,
    std_materials: Res<Assets<StandardMaterial>>,
    mut wall_materials: ResMut<Assets<MoldWallMaterial>>,
    mut cache: ResMut<CoatedFurniture>,
) {
    for root in &roots {
        for entity in children.iter_descendants(root) {
            let Ok(mat) = painted.get(entity) else {
                continue;
            };
            let id = mat.0.id();
            let coated = match cache.0.get(&id) {
                Some(handle) => handle.clone(),
                None => {
                    // The glTF material may not have finished loading; try again next frame.
                    let Some(base) = std_materials.get(&mat.0) else {
                        continue;
                    };
                    let handle = wall_materials.add(MoldWallMaterial {
                        base: base.clone(),
                        extension: material::MoldWallExt::new(
                            &cfg,
                            images.display.clone(),
                            control.dynamic.clone(),
                        ),
                    });
                    cache.0.insert(id, handle.clone());
                    handle
                }
            };
            commands
                .entity(entity)
                .remove::<MeshMaterial3d<StandardMaterial>>()
                .insert((MeshMaterial3d(coated), MoldCoated));
        }
    }
}
