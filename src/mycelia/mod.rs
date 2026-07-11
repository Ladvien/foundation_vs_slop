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
//! The firewall is a **plugin boundary**, not a property of the systems: `MyceliaPlugin` is registered
//! **only** in `lib::run`, never in the headless `sim_harness` (mirroring `UiPlugin`/`DialoguePlugin`), and
//! it **no-ops if the `RenderApp` sub-app is absent**. The replay harness therefore spawns no fruit bodies
//! and runs none of this code.
//!
//! Nearly everything here is cosmetic and lives on **`Update`**; no mold entity carries `Health`, so
//! `snapshot_hash` (which queries `(Transform, Health)`) never sees one. The exception is [`grazing`], whose
//! two systems are on `FixedUpdate` because they steer crabs — they drain `DriveId::HUNGER` and deposit into
//! `FieldId::MEAT`, both of which move a crab's `Transform`, which `snapshot_hash` *does* read. That is
//! pinned state, and it is only safe because of the plugin boundary above. It is also why those systems live
//! here and not in `crab.rs`: `CrabPlugin` **is** registered in the harness.
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
pub(crate) mod habitat;
mod grazing;
pub mod fruit;
mod material;
mod measure;
pub mod perceptual;
mod pipeline;
pub mod species;
mod testbed;

use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::gpu_readback::Readback;
use bevy::render::render_resource::{ShaderType, TextureFormat};
use bevy::render::storage::ShaderBuffer;
use bevy::render::RenderApp;
use serde::{Deserialize, Serialize};

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
#[derive(Resource, Deserialize, Serialize, Clone, Debug)]
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
    ///
    /// The shipped 0.075 Hz is a twentieth of that measured value. Front speed alone did not capture what
    /// the eye actually tracks — the *contour*, which a `smoothstep` slides by `ΔV / |∇V|` per tick, far
    /// further than the field itself moves. The interpolation pass (`mycelia_blend.wgsl`) fixed the stepping
    /// that caused; two rounds of playtest took the clock down for the residual creep.
    /// [`MyceliaConfig::warmup_ticks`] is what makes a clock this slow affordable.
    pub sim_hz: f32,
    /// Sim ticks dispatched behind the loading screen, before the player sees anything, so the colony is
    /// already mature at the moment of first sight. Advanced as fast as the GPU accepts them (one per
    /// rendered frame), not on the `sim_hz` clock — this is not time passing, it is history that already
    /// happened. `0` is legal and means "start from bare floor".
    pub warmup_ticks: u32,
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
    /// Minimum separation (world units) between fruit bodies of **different clusters**. Most hyphal knots
    /// never mature; neighbouring knots compete for translocated nutrient (Kües & Navarro-González 2015).
    /// That competition is between genets — inside one flush the only floor is volva geometry.
    ///
    /// Measured: at `1.5` a room the squad walked through grew ~45 bodies across 139 tiles — a lawn. `3.0`
    /// quarters that, which reads as a flush and leaves the floor clear for pathing and gore.
    pub cluster_spacing: f32,
    /// How far (world units) a flush's satellites may stand from its nucleus. Must exceed
    /// [`perceptual::min_sibling_spacing`], or there is no annulus to place them in and every bunch would
    /// silently collapse to a single body.
    pub cluster_radius: f32,
    /// Largest flush a nucleus may produce. Two is the structural minimum (a "cluster" of one is a solitary
    /// body), and the size distribution skews small — see [`perceptual::cluster_sites`].
    pub cluster_size_max: u32,
    /// Hard ceiling on live fruit bodies. Reaching it is logged, never silently ignored.
    pub max_fruit_bodies: u32,
    /// Scale applied to the death cap mesh, whose native height is 13.9 cm. `4.0` gives a 56 cm mushroom —
    /// knee-high on a squad unit, and ~50 px tall at the default zoom against a unit's ~150 px. At `2.5` it
    /// measured 31 px and read as floor debris rather than a prop.
    ///
    /// Growth time scales with it: the speed limit is on vertex *speed*, so a bigger body has further to
    /// travel. See [`perceptual::egg_to_adult_secs`].
    pub body_scale: f32,
    /// Local biomass `V` below which a fruit body's patch has collapsed and the body reabsorbs, running its
    /// growth clock backwards. Primordium abortion, not a fallback branch: the same ODE with a negative
    /// sign. Must be below [`MyceliaConfig::v_fruit`], or a body would begin aborting the instant it pinned.
    pub maintain_v: f32,

    // ── Habitat (see `habitat.rs`) ────────────────────────────────────────────────────────────────────
    /// Fraction of **walkable floor cells** (rooms + corridors alike) the colony may occupy. The mold has no
    /// business coating a whole dungeon: real fungal colonies are patchy because substrate, moisture and
    /// competition are patchy. `habitat::build` hits this by selecting rooms greedily, and reports what it
    /// actually achieved rather than silently clamping.
    pub habitat_coverage: f32,
    /// Blue-noise spacing (in cells) between patch nuclei inside an infested room. Reuses `geom::poisson_disk`.
    pub patch_spacing: f32,
    /// Smallest patch radius (cells). Must not exceed [`MyceliaConfig::patch_radius_max`].
    pub patch_radius_min: f32,
    /// Largest patch radius (cells).
    pub patch_radius_max: f32,
    /// Probability that a given corridor *run* (one adjacency edge, end to end) is fully infested. Rooms get
    /// patches; a corridor gets all of itself or none of it — a passage you dread is a passage, not a spot.
    pub corridor_infest_chance: f32,
    /// How hard fbm breaks up a patch's border. `0` yields circles; higher yields a ragged colony margin.
    pub edge_noise_amp: f32,
    /// Spatial frequency (cycles per cell) of that border noise.
    pub edge_noise_scale: f32,
    /// Habitat value at or above which an agent may stand. The GPU reads the mask as `u8`, so this threshold
    /// must be expressible there: the CPU seeder and the shader's hard block quantize identically, or an
    /// agent could be seeded on a texel the GPU then refuses to let it leave. Hence the `>= 2/255` floor.
    pub agent_hab_min: f32,
    /// Gray-Scott removal rate `k` on **barren** floor, blended toward [`MyceliaConfig::kill`] by habitat.
    ///
    /// Containment is a *reaction* property, not a masking one: masking the Laplacian would make every patch
    /// edge a no-flux wall and pile biomass into a hard rim. Instead barren floor sits in the regime where
    /// `(U, V) = (1, 0)` is the only stable homogeneous state, so `V` simply dies there. Linearizing the `V`
    /// equation about `V ≈ 0`, `U ≈ 1` gives a penetration length `λ = sqrt(d_v / (feed + kill_barren))` —
    /// sub-texel at the shipped values, so the mat cannot creep past the patch no matter how long it runs.
    /// Pearson (1993), *Science* 261:189, doi:10.1126/science.261.5118.189 — the canonical `(F, k)` map.
    pub kill_barren: f32,
    /// How readily each room type rots, keyed by the room-type tag `dungeon::pick_room` stamps into
    /// `Region::props`. Damp rooms (bathroom, kitchen) rot; dry ones (office, bedroom) rarely do.
    ///
    /// There is deliberately **no default weight** for an unlisted tag: a silent `1.0` would let a renamed or
    /// new room type slip in at middling susceptibility and go unnoticed. [`validate_config`] instead demands
    /// this table name exactly the tags the dungeon can emit.
    pub damp_weights: Vec<DampWeight>,

    // ── Perception budget (see `perceptual.rs`) ───────────────────────────────────────────────────────
    /// Slowest motion (degrees of visual angle per second) a human reliably detects beside a stationary
    /// reference. Every *autonomous* motion the mold makes is held under this. Being eaten or crushed is
    /// exempt — that is meant to be seen. Leibowitz (1955), 10.1364/josa.45.000829.
    pub motion_threshold_deg_per_s: f32,
    /// Vertical visual angle the game window subtends at the player's eye (a 27" panel at ~60 cm ≈ 30°).
    /// The one number here that depends on the player's desk rather than on the game.
    pub screen_fov_deg_v: f32,

    /// The mushroom species table. One row per species; the death cap is row 0. Each row carries its
    /// growth glb, native scale, and the measured geometry that feeds the perceptual speed limit. See
    /// [`species`]. There is deliberately no default row — the RON is the single source of truth, and a
    /// species referenced at spawn but absent here would be a loud out-of-range panic, not a silent one.
    pub species: Vec<species::SpeciesConfig>,
}

/// One row of [`MyceliaConfig::damp_weights`] — how readily a room type rots.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DampWeight {
    /// The room-type tag, as stamped into `Region::props.tags` by `dungeon::pick_room`.
    pub tag: String,
    /// Multiplier on the room's susceptibility score. Relative, not a probability.
    pub weight: f32,
}

impl MyceliaConfig {
    /// The susceptibility multiplier for a region's tags. Infallible **because** [`validate_config`] has
    /// already proved the table covers every tag the dungeon emits; a region carrying an unlisted tag is a
    /// contract violation between `dungeon` and this config, and it fails loudly there rather than here.
    pub fn damp_weight(&self, tags: &[String]) -> Result<f32, String> {
        for row in &self.damp_weights {
            if tags.iter().any(|t| t == &row.tag) {
                return Ok(row.weight);
            }
        }
        Err(format!("mycelia.damp_weights names no tag of region {tags:?}"))
    }
}

/// Whether the compute chain advances this frame. The mold runs on its own slow clock (`sim_hz`), not the
/// render clock; on frames where this is `false` the render node skips every pass and leaves the ping-pong
/// parity alone, so the display texture simply persists.
#[derive(Resource, Clone, Copy, ExtractResource, Default)]
pub struct MoldStep {
    /// How many sim ticks to dispatch this frame. Usually `0` or `1`; more only when the game clock is
    /// running fast enough that a rendered frame spans several sim periods (see [`MAX_TICKS_PER_FRAME`]).
    pub ticks: u32,
    /// Phase through the current sim period, `0..1`. The `blend` pass lerps the previous tick's snapshot
    /// toward the newest by exactly this much, every rendered frame — see [`advance_mold_time`].
    pub alpha: f32,
}

/// Hard ceiling on sim ticks dispatched in one rendered frame.
///
/// Bounds the catch-up burst after a real stall (alt-tab, breakpoint, a slow first frame), exactly as
/// `Time<Virtual>::max_delta` bounds the fixed-timestep one — see the discussion in `time_control`. At the
/// top of the speed ladder (×64) the steady-state demand is `64 × sim_hz / fps` ticks per frame, far under
/// this; the cap only bites transients. When it does bite, it is **logged**, because a silently dropped tick
/// is a mold that quietly runs slower than the speed the player selected.
pub const MAX_TICKS_PER_FRAME: u32 = 8;

/// The habitat mask: **where the mold may live at all**, at field resolution, in row-major order.
///
/// Held as the quantized `u8` bytes that cross to the GPU in the static control texture's `G` channel, not as
/// `f32`. Two consumers must agree on this mask to the bit — `agents::seed_agents` on the CPU and
/// `agent_step`'s hard block on the GPU — and the only way to guarantee that is for both to read the same
/// bytes and apply the same threshold. Built once, at `Startup`, by [`habitat::build`]; the dungeon never
/// regenerates, so it never changes.
#[derive(Resource)]
pub struct MoldHabitat(pub Vec<u8>);

/// Decide the colony's footprint, once, before anything that depends on it exists.
///
/// Runs between `setup_control` (which allocates the texture the mask is uploaded into) and `setup_mycelia`
/// (which seeds the agents inside it). A failure here is a loud startup failure: a dungeon the mold cannot be
/// placed in is a generation bug, and there is no degraded colony worth rendering.
fn setup_habitat(
    mut commands: Commands,
    cfg: Res<MyceliaConfig>,
    dungeon: Res<crate::dungeon::Dungeon>,
) -> Result<(), BevyError> {
    commands.insert_resource(MoldHabitat(habitat::build(&dungeon, &cfg)?));
    Ok(())
}

/// `true` once the colony has completed its [`MyceliaConfig::warmup_ticks`] and the mold is established.
///
/// Warmup ticks are dispatched one per rendered frame and ignore the clock entirely, so the colony grows
/// underneath the boot and title screens even though `SimBlocked` has frozen virtual time there. By the time
/// most players click through, this is already `true` and the warmup screen passes straight through.
/// `ui::warmup` waits on it so that a player who clicks fast still never sees bare carpet.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct MoldWarm(pub bool);

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

    // ── Habitat ───────────────────────────────────────────────────────────────────────────────────────
    unit("habitat_coverage", c.habitat_coverage)?;
    unit("corridor_infest_chance", c.corridor_infest_chance)?;
    unit("agent_hab_min", c.agent_hab_min)?;
    positive("patch_spacing", c.patch_spacing)?;
    positive("patch_radius_min", c.patch_radius_min)?;
    positive("edge_noise_scale", c.edge_noise_scale)?;
    non_negative("edge_noise_amp", c.edge_noise_amp)?;
    if c.patch_radius_max < c.patch_radius_min {
        return Err(format!(
            "mycelia.patch_radius_max ({}) must be >= patch_radius_min ({})",
            c.patch_radius_max, c.patch_radius_min
        ));
    }
    // The habitat mask crosses to the GPU as one `Rgba8Unorm` byte, so a threshold finer than a quantisation
    // step is a threshold the shader cannot honour. Below 2/255 a texel the CPU seeder judged habitable can
    // round to a byte the shader's hard block rejects — and that agent is frozen for the run.
    if c.agent_hab_min < 2.0 / 255.0 {
        return Err(format!(
            "mycelia.agent_hab_min must be >= 2/255 ({:.5}) so the u8 habitat mask can express it, got {}",
            2.0 / 255.0,
            c.agent_hab_min
        ));
    }
    if c.damp_weights.is_empty() {
        return Err("mycelia.damp_weights must not be empty".to_string());
    }
    for row in &c.damp_weights {
        non_negative(&format!("damp_weights[{}].weight", row.tag), row.weight)?;
    }
    if c.damp_weights.iter().all(|r| r.weight <= 0.0) {
        return Err("mycelia.damp_weights: at least one room type must be able to rot".to_string());
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
    // Barren floor must sit where `(U, V) = (1, 0)` is the only stable homogeneous state, or the mat merely
    // grows *slower* outside its patch instead of dying there. The non-trivial states solve
    // `(F+k)V² - F·V + F(F+k) = 0`, which has real roots only while `F + k <= sqrt(F)/2`; above that the
    // saddle-node has annihilated them and `V` decays unconditionally. Pearson (1993), Science 261:189.
    positive("kill_barren", c.kill_barren)?;
    let saddle_node = c.feed.sqrt() * 0.5;
    if c.feed + c.kill_barren <= saddle_node {
        return Err(format!(
            "mycelia.kill_barren ({}) leaves barren floor inside the Gray-Scott pattern regime: \
             feed + kill_barren = {} must exceed sqrt(feed)/2 = {saddle_node}, or biomass survives \
             outside its patch",
            c.kill_barren,
            c.feed + c.kill_barren
        ));
    }
    if c.kill_barren <= c.kill {
        return Err(format!(
            "mycelia.kill_barren ({}) must exceed kill ({}) — barren floor is where the mold dies",
            c.kill_barren, c.kill
        ));
    }
    if c.vein_hi <= c.vein_lo {
        return Err(format!("mycelia.vein_hi ({}) must exceed vein_lo ({})", c.vein_hi, c.vein_lo));
    }

    // ── Fruiting ──────────────────────────────────────────────────────────────────────────────────────
    // `v_fruit`/`u_exhausted`/`maintain_v` are Gray-Scott concentrations, clamped to 0..=1 in the shader.
    unit("v_fruit", c.v_fruit)?;
    unit("u_exhausted", c.u_exhausted)?;
    unit("maintain_v", c.maintain_v)?;
    positive("pin_dwell_secs", c.pin_dwell_secs)?;
    // The dwell is credited once per readback, and readbacks land once per sim tick. A dwell shorter than a
    // tick period is therefore indistinguishable from zero — every value in `(0, period]` commits on exactly
    // the same scan. Reject it rather than ship a dial that looks live and is not.
    let period = 1.0 / c.sim_hz;
    if c.pin_dwell_secs < period {
        return Err(format!(
            "mycelia.pin_dwell_secs ({}) must be >= one sim tick ({period} s at sim_hz {}), or the dwell \
             gate is quantised away and any positive value behaves the same",
            c.pin_dwell_secs, c.sim_hz,
        ));
    }
    positive("cluster_spacing", c.cluster_spacing)?;
    positive("cluster_radius", c.cluster_radius)?;
    positive("body_scale", c.body_scale)?;
    // A satellite is placed in the annulus between two volvas touching and the cluster radius. If the radius
    // does not clear that floor there is no annulus, every rejection-sample fails, and each "flush" is
    // silently just its nucleus — a feature that looks implemented and is not.
    let sibling_min = perceptual::min_sibling_spacing(c.body_scale);
    if c.cluster_radius <= sibling_min {
        return Err(format!(
            "mycelia.cluster_radius ({}) must exceed two volva radii at body_scale {} ({sibling_min}), or a \
             flush has nowhere to put its satellites and collapses to one body",
            c.cluster_radius, c.body_scale,
        ));
    }
    // Clusters must be tighter than the gap between them, or a "bunch" is indistinguishable from the lawn
    // `cluster_spacing` exists to prevent.
    if c.cluster_spacing <= c.cluster_radius {
        return Err(format!(
            "mycelia.cluster_spacing ({}) must exceed cluster_radius ({}), or neighbouring flushes merge",
            c.cluster_spacing, c.cluster_radius,
        ));
    }
    if c.cluster_size_max < 2 {
        return Err(format!(
            "mycelia.cluster_size_max ({}) must be >= 2; a cluster of one is a solitary body",
            c.cluster_size_max,
        ));
    }
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

/// Cross-slice check: `mycelia.damp_weights` must name **exactly** the room types `dungeon.room_types`
/// declares — no missing type, no stale one.
///
/// A missing tag has no safe answer. Defaulting it to `1.0` would quietly give a brand-new room type middling
/// susceptibility and nobody would ever notice; erroring at lookup time would fire only on the seeds that
/// happen to roll that type. Neither slice can see the other, so the check lives with the caller that holds
/// both (`crate::config::load_game_config`) and fires before a single frame runs.
pub fn validate_damp_coverage(
    c: &MyceliaConfig,
    room_types: &[crate::dungeon::RoomType],
) -> Result<(), String> {
    for rt in room_types {
        if !c.damp_weights.iter().any(|r| r.tag == rt.tag) {
            return Err(format!(
                "mycelia.damp_weights is missing room type {:?}; every type dungeon.room_types can emit \
                 needs a susceptibility, or the mold would silently treat it as average",
                rt.tag
            ));
        }
    }
    for row in &c.damp_weights {
        if !room_types.iter().any(|rt| rt.tag == row.tag) {
            return Err(format!(
                "mycelia.damp_weights names room type {:?}, which dungeon.room_types never emits",
                row.tag
            ));
        }
    }
    Ok(())
}

/// Validate the species table. One path, no silent defaults: a malformed row is a loud startup error,
/// not a mushroom that grows wrong on some seeds. Checks each row's geometry is well-formed (the speed
/// limit is undefined otherwise), colours are in gamut, its flush cannot overlap at `body_scale`, and
/// its room affinity names only room types the dungeon can emit.
pub fn validate_species(
    c: &MyceliaConfig,
    room_types: &[crate::dungeon::RoomType],
) -> Result<(), String> {
    const ARCHETYPES: [&str; 9] = [
        "veiled_egg", "gilled_ringed", "gilled_plain", "bolete", "funnel", "bracket", "globe",
        "cluster", "morel",
    ];
    if c.species.is_empty() {
        return Err("mycelia.species is empty; there must be at least the death cap (row 0)".into());
    }
    for (i, s) in c.species.iter().enumerate() {
        let who = format!("mycelia.species[{i}] {:?}", s.name);
        if s.name.is_empty() || s.growth_glb.is_empty() {
            return Err(format!("{who}: empty name or growth_glb"));
        }
        if !ARCHETYPES.contains(&s.archetype.as_str()) {
            return Err(format!("{who}: unknown archetype {:?}; one of {ARCHETYPES:?}", s.archetype));
        }
        if !(s.body_scale > 0.0) {
            return Err(format!("{who}: body_scale must be > 0, got {}", s.body_scale));
        }
        if !(0.0..=1.0).contains(&s.toxicity) || !(s.nutrition >= 0.0) {
            return Err(format!("{who}: toxicity must be 0..=1 and nutrition >= 0"));
        }
        let g = &s.geom;
        for (k, &d) in g.stage_max_disp.iter().enumerate() {
            if !(d > 0.0) {
                return Err(format!("{who}: stage_max_disp[{k}] must be > 0 (speed limit divides by it)"));
            }
        }
        for k in 0..6 {
            if g.stage_height_m[k + 1] < g.stage_height_m[k] - 0.002 {
                return Err(format!("{who}: stage_height_m drops at stage {k} (a body may not shrink)"));
            }
        }
        if !(g.egg_height_m > 0.0 && g.cap_radius_m > 0.0 && g.volva_radius_m > 0.0) {
            return Err(format!("{who}: egg/cap/volva radius must all be > 0"));
        }
        if !(g.bend_lo_m < g.bend_hi_m) {
            return Err(format!("{who}: bend_lo_m must be < bend_hi_m"));
        }
        // A flush of this species must have room to pack without its volvas interpenetrating.
        let min_sibling = 2.0 * g.volva_radius_m * s.body_scale;
        if s.archetype != "bracket" && min_sibling >= c.cluster_radius {
            return Err(format!(
                "{who}: sibling spacing {min_sibling:.3} >= cluster_radius {:.3}; the flush would collapse",
                c.cluster_radius
            ));
        }
        for col in [&s.colors.cap_young, &s.colors.cap_old, &s.colors.stipe, &s.colors.volva, &s.colors.substrate] {
            if col.iter().any(|&x| !(0.0..=1.0).contains(&x)) {
                return Err(format!("{who}: a part colour is out of the [0,1] range: {col:?}"));
            }
        }
        for a in &s.room_affinity {
            if !room_types.iter().any(|rt| rt.tag == a.tag) {
                return Err(format!(
                    "{who}: room_affinity names room type {:?}, which dungeon.room_types never emits",
                    a.tag
                ));
            }
        }
    }
    Ok(())
}

/// The mold's field textures. Only `display` crosses to the material; the trail and biomass pairs live
/// purely to feed the compute chain. All are extracted so `prepare_bind_group` can bind them.
#[derive(Resource, Clone, ExtractResource)]
pub struct MoldImages {
    /// The field the three mold materials sample: `R` trail · `G` biomass `V` · `B` wall contact. Written
    /// **every rendered frame** by the `blend` pass, which lerps [`MoldImages::snap_a`] and
    /// [`MoldImages::snap_b`] by `MoldParams::blend_alpha`. Raw simulation fields, not colour — shading is
    /// the material's job.
    pub display: Handle<Image>,
    /// Per-tick snapshot ping-pong pair, written by the `field` pass. One holds the newest tick's fields,
    /// the other the tick before it, and `blend` interpolates between them so the mold advances continuously
    /// instead of hopping once per `sim_hz` period. Same parity swap as the trail.
    pub snap_a: Handle<Image>,
    pub snap_b: Handle<Image>,
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
    /// Habitat (static field `G`) at or above which an agent may stand. See [`MyceliaConfig::agent_hab_min`].
    pub agent_hab_min: f32,
    /// Gray-Scott `k` on barren floor. See [`MyceliaConfig::kill_barren`].
    pub kill_barren: f32,
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
        // exist first; and it seeds the agents inside the habitat mask, so `setup_habitat` must run between
        // the two.
        .init_resource::<MoldStep>()
        .init_resource::<MoldWarm>()
        .add_systems(Startup, (control::setup_control, setup_habitat, setup_mycelia).chain())
        .init_resource::<CoatedFurniture>()
        .add_systems(Update, (advance_mold_time, control::write_control, coat_walls, coat_furniture))
        // Reads `MoldStep`, so it must observe the flag `advance_mold_time` set this frame.
        .add_systems(Update, gate_coarse_readback.after(advance_mold_time));

        // Fruit bodies: the mold reproducing. Registered here (not as a separate plugin) because it depends
        // on this plugin's textures, buffers and config, and shares its determinism firewall.
        fruit::build(app);
        // Crabs eating those fruit bodies. The one part of this module that touches pinned state, and it is
        // safe for the same reason everything else here is: this plugin never reaches the headless harness.
        grazing::build(app);
        // Dev calibration instruments. Both no-op unless their environment variable is set.
        measure::build(app);
        testbed::build(app);

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
    habitat: Res<MoldHabitat>,
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

    // Seven RGBA16F field textures (the blended display, the per-tick snapshot pair, and the trail and
    // biomass ping-pong pairs), each usable as both a compute storage-write target and a sampled read.
    // `display` is the one shared with the materials; the snapshots exist only so `blend` has two ticks to
    // interpolate between. Zero-filled, so before the first tick the mold is simply absent.
    let display = images.add(field::field_texture(size));
    let snap_a = images.add(field::field_texture(size));
    let snap_b = images.add(field::field_texture(size));
    let trail_a = images.add(field::field_texture(size));
    let trail_b = images.add(field::field_texture(size));
    let biomass_a = images.add(field::field_texture(size));
    let biomass_b = images.add(field::field_texture(size));

    // Agent population (seeded once, inside the habitat only) + zeroed deposit accumulator (one slot per
    // field texel). Both are `ShaderBuffer`s — the default usage (`STORAGE | COPY_DST`) is what the chain
    // needs. Seeding reads the *quantized* mask, the same bytes the shader's hard block will read; see the
    // habitat invariant in `agents.rs`.
    let seeded = agents::seed_agents(size, cfg.agent_count, &habitat.0, cfg.agent_hab_min)?;
    let agents = buffers.add(ShaderBuffer::from(seeded));
    let deposit = buffers.add(ShaderBuffer::from(vec![0u32; (size * size) as usize]));
    // `vec4<f32>` per coarse cell: (max V, U at that texel, texel x, texel y). Zero-initialised, which reads
    // as "no biomass anywhere" — the true state before the first tick, not a placeholder.
    let coarse =
        buffers.add(ShaderBuffer::from(vec![0.0f32; (COARSE_SIZE * COARSE_SIZE * 4) as usize]));

    commands.insert_resource(MoldImages {
        display: display.clone(),
        snap_a,
        snap_b,
        trail_a,
        trail_b,
        biomass_a,
        biomass_b,
    });
    commands.insert_resource(MoldBuffers { agents, deposit, coarse });
    // The mold's single GPU→CPU edge; `fruit.rs` observes `ReadbackComplete` on this entity. Cosmetic-only,
    // `Update`-only — see the module header. The `Readback` component itself is owned by
    // `gate_coarse_readback`, which holds it only on sim-tick frames.
    commands
        .spawn((Name::new("mycelia_coarse_readback"), fruit::CoarseReadback))
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
        agent_hab_min: cfg.agent_hab_min,
        kill_barren: cfg.kill_barren,
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

/// Seconds between sim ticks. Validated `> 0` at startup, so this never divides by zero.
fn period_secs(cfg: &MyceliaConfig) -> f32 {
    1.0 / cfg.sim_hz
}

/// Advance the shared sim clock on the main world each frame; the value is extracted into the render
/// world for the compute pass and reused by the floor material. `Update` (cosmetic), never `FixedUpdate`.
///
/// # The clock
///
/// `Time<Virtual>` — the same clock gameplay runs on, scaled by [`crate::time_control::GameSpeed`] and driven
/// to zero by a pause or a blocking menu. So the mold grows at ×1 exactly as slowly as
/// [`perceptual::v_max`] demands, and the speed ladder fast-forwards it along with everything else: at ×16 a
/// player who wants to *watch* the colony spread can.
///
/// That reverses an earlier decision to run this on `Time<Real>` so the mold kept breathing behind the pause
/// menu. Ambience is not worth two clocks. The one thing that decision bought — a colony already grown by the
/// time the player arrives — is now bought properly, by [`MyceliaConfig::warmup_ticks`].
///
/// # Two cadences, and the warmup is not one of them dressed up
///
/// Warmup is *history*: ticks the world already lived through before the player arrived, dispatched one per
/// rendered frame regardless of any clock (which is why it still runs behind the frozen title screen).
/// Afterwards the clock is the `sim_hz` one, and it is the only one.
fn advance_mold_time(
    time: Res<Time<Virtual>>,
    cfg: Res<MyceliaConfig>,
    coarse: Res<fruit::MoldCoarse>,
    mut warm: ResMut<MoldWarm>,
    mut params: ResMut<MoldParams>,
    mut step: ResMut<MoldStep>,
    mut accum: Local<f32>,
    mut baseline: Local<Option<u64>>,
) {
    params.time = time.elapsed_secs();

    // ── Warmup: run the colony's history before anyone is looking ─────────────────────────────────────
    //
    // A mold that has lived in these corridors for years must not be caught colonising them. So the chain is
    // dispatched every frame — no `sim_hz` gate — until `warmup_ticks` ticks have actually run.
    //
    // Counted against a baseline taken at the first readback that proves the chain dispatched at all
    // (`MoldCoarse::has_run`). A raw readback count would not do: `bevy_render` copies the coarse buffer on
    // every frame the `Readback` component is present, including the frames before the compute pipelines
    // have finished compiling, and the warmup budget would be spent on frames that dispatched nothing —
    // handing the player a bare floor and a `MoldWarm` that lied. Over-requesting by the readback's
    // one- or two-frame latency is harmless; under-delivering the colony is the whole failure.
    if !warm.0 {
        let done = match (coarse.has_run(), *baseline) {
            (false, _) => false,
            (true, None) => {
                *baseline = Some(coarse.ticks_elapsed());
                cfg.warmup_ticks == 0
            }
            (true, Some(base)) => coarse.ticks_elapsed() - base >= cfg.warmup_ticks as u64,
        };
        if done {
            warm.0 = true;
            info!("mycelia: colony warm after {} ticks", cfg.warmup_ticks);
            // Hand over to the slow clock without letting the mold flinch backwards. Warmup leaves the blend
            // showing the newest snapshot (`alpha = 1`); falling through with an empty accumulator would put
            // `alpha` at 0 next frame, which displays the snapshot *before* it — a one-tick step backwards.
            // A full accumulator fires a tick immediately instead, flipping the parity so that `alpha = 0`
            // means the snapshot we are already showing.
            *accum = period_secs(&cfg);
        } else {
            // Exactly one tick per frame, so `MoldCoarse::ticks_elapsed` (one readback per frame) counts
            // them one-for-one. Nothing to interpolate toward yet, and no eye to notice: show each
            // snapshot whole.
            step.ticks = 1;
            step.alpha = 1.0;
            *accum = 0.0;
            return;
        }
    }

    // Fixed-rate sim clock, decoupled from the render clock: the mold advances by whole `1 / sim_hz` periods
    // of *virtual* time, however many of those a rendered frame happens to span. At ×1 and 60 fps that is a
    // tick every few hundred frames; at ×64 it is several per frame, which is the point of the speed ladder.
    let period = period_secs(&cfg);
    *accum += time.delta_secs();
    let wanted = (*accum / period).floor().max(0.0) as u32;
    step.ticks = wanted.min(MAX_TICKS_PER_FRAME);
    if wanted > MAX_TICKS_PER_FRAME {
        // Loudly. A dropped tick means the mold ran slower than the speed the player asked for, and a mold
        // that silently ignores the speed ladder is exactly the kind of result that takes hours to trace.
        warn!(
            "mycelia: frame spanned {wanted} sim ticks, capped at {MAX_TICKS_PER_FRAME}; \
             the mold fell {} ticks behind the game clock",
            wanted - MAX_TICKS_PER_FRAME,
        );
    }
    // Consume the ticks we ran; discard the rest of the backlog rather than carry it into a surge on the
    // next frame. Never leave more than one period banked, or `alpha` would exceed 1.
    *accum = (*accum - step.ticks as f32 * period).clamp(0.0, period);

    // How far we are through the *current* period, `0..1`. The `blend` pass uses this to interpolate the
    // display texture between the last two tick snapshots, every rendered frame.
    //
    // Without it the mold advanced in 667 ms jumps. That is not a motion-threshold failure — the biomass
    // margin creeps at 2.92 mm/s, well under budget — but a *contour* one: the material resolves the field
    // through `smoothstep`, so wherever the biomass gradient is shallow, a small step in `V` slides the
    // rendered iso-contour a long way. The edge visibly hopped. Interpolating the field restores the
    // continuous motion the speed limit was derived for; the eye integrates it and sees nothing.
    //
    // Costs one tick of latency (the blend arrives at snapshot `k` just as `k+1` is computed). At 1.5 Hz
    // that is 667 ms of lag on an ambience layer nobody is timing.
    step.alpha = (*accum / period).clamp(0.0, 1.0);
}

/// Hold the coarse buffer's [`Readback`] only on sim-tick frames.
///
/// `bevy_render`'s `prepare_buffers` queues a `copy_buffer_to_buffer` for **every** entity carrying a
/// `Readback`, every rendered frame — there is no dirty flag and no one-shot variant, and the component's own
/// docs say "if this component is not removed, the readback will be attempted every frame". But `pin_scan`
/// only rewrites the buffer when [`MoldStep::ticks`] is non-zero, i.e. at `sim_hz`. Copying it at the display's
/// refresh rate would pay a GPU→CPU transfer and a `COARSE_SIZE²` decode ~80× per meaningful update.
///
/// Removing the component (rather than despawning the entity) keeps the `receive_coarse` observer and the
/// [`fruit::CoarseReadback`] marker alive. `GpuReadbackPlugin` installs `ExtractComponentPlugin<Readback>`,
/// whose `SyncComponentPlugin` registers an `on_remove` hook, so the removal reaches the render world.
///
/// `advance_mold_time` raises `ticks` on exactly the frames a tick runs, and `Update` runs before
/// `ExtractSchedule`, so this yields one copy per *frame that ticked* — which the warmup counter relies on
/// being one per tick, hence its one-tick-per-frame cadence.
fn gate_coarse_readback(
    mut commands: Commands,
    step: Res<MoldStep>,
    buffers: Res<MoldBuffers>,
    readback: Query<(Entity, Has<Readback>), With<fruit::CoarseReadback>>,
) -> Result<(), BevyError> {
    // Spawned unconditionally by `setup_mycelia`; its absence means the setup contract broke.
    let (entity, present) = readback.single()?;
    if step.ticks > 0 && !present {
        commands.entity(entity).insert(Readback::buffer(buffers.coarse.clone()));
    } else if step.ticks == 0 && present {
        commands.entity(entity).remove::<Readback>();
    }
    Ok(())
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
    /// Visible to the sibling submodules' tests (e.g. `habitat`), which need a valid config to build against.
    pub(super) fn valid() -> MyceliaConfig {
        MyceliaConfig {
            field_size: 1024,
            sim_hz: 1.5,
            warmup_ticks: 200,
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
            kill_barren: 0.09,
            d_u: 0.16,
            d_v: 0.08,
            bloom_seed: 0.06,
            habitat_coverage: 0.25,
            patch_spacing: 4.0,
            patch_radius_min: 2.0,
            patch_radius_max: 5.0,
            corridor_infest_chance: 0.12,
            edge_noise_amp: 0.35,
            edge_noise_scale: 0.15,
            agent_hab_min: 0.02,
            damp_weights: vec![
                DampWeight { tag: "bathroom".into(), weight: 3.0 },
                DampWeight { tag: "kitchen".into(), weight: 2.5 },
                DampWeight { tag: "hall".into(), weight: 1.2 },
                DampWeight { tag: "living".into(), weight: 1.0 },
                DampWeight { tag: "bedroom".into(), weight: 0.6 },
                DampWeight { tag: "office".into(), weight: 0.4 },
            ],
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
            cluster_spacing: 3.0,
            cluster_radius: 0.7,
            cluster_size_max: 8,
            max_fruit_bodies: 40,
            body_scale: 4.0,
            maintain_v: 0.20,
            motion_threshold_deg_per_s: 0.02,
            screen_fov_deg_v: 30.0,
            species: vec![species::death_cap_config_row()],
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
            c.cluster_spacing = bad;
            assert!(validate_config(&c).is_err(), "cluster_spacing={bad} should be rejected");

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
