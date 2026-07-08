//! Gore: liquid blood and the squad "crunch" on death.
//!
//! Death and flesh hits are gory. Three composited effects, all triggered through one decoupled
//! queue ([`GoreQueue`]) so any system can request gore by pushing a [`GoreEvent`] (mirrors the
//! `impact_fx` queue pattern — that module keeps doing wall *sparks*; flesh is handled here, one
//! job per queue):
//!
//!   1. **Blood spray** — a camera-facing quad of liquid droplets flung out and arcing down under
//!      gravity (custom [`BloodSprayMaterial`] + `assets/shaders/blood_spray.wgsl`). Every hit/death.
//!   2. **Blood pool** — a flat SDF-splatter decal on the floor that grows then holds as a permanent
//!      stain, capped in count so it can't grow unbounded ([`BloodPoolMaterial`] +
//!      `assets/shaders/blood_pool.wgsl`). Every hit/death. A death also stamps the same decal,
//!      stood upright, onto any walls bounding the death cell (blood splatter on walls).
//!   3. **Gib chunks** — squad units (real 3D figurines) shatter into small primitive chunks with
//!      hand-integrated ballistics (bounce + settle). Squad `UnitCrunch` only; transient (shrink out).
//!   4. **Meat chunks** — fleshy blobs flung out on *any* death (squad + enemy) that settle and then
//!      stay lying on the floor as permanent viscera, capped in count like the blood pools. They are
//!      `Gib`s with an infinite lifetime, so they reuse the same ballistics as the transient gibs.
//!
//! We deliberately do *not* run true runtime mesh fracture. Real-time destruction methods
//! (Müller, Chentanez & Kim, "Real-Time Dynamic Fracture with Volumetric Approximate Convex
//! Decompositions", ACM TOG 2013, DOI 10.1145/2461912.2461934; Sellán et al., "Breaking Good:
//! Fracture Modes for Realtime Destruction", ACM TOG 2022, DOI 10.1145/3549540) need a VACD/Voronoi
//! decomposition and a physics solver this project has neither of. Instead we use the cheap
//! shipped-game analog those papers contrast against: a small burst of pre-authored gib primitives
//! with simple ballistics. The blood shaders reuse the texture-free hash noise cited there and in
//! `impact_fx.wgsl` (Lagae et al. 2010, DOI 10.1111/j.1467-8659.2010.01827.x).

use std::collections::VecDeque;
use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use serde::{Deserialize, Serialize};

use bevy::time::Real;

use avian3d::prelude::*;

use crate::autogib::AutogibCache;
use crate::util::hash_f32;
use crate::blood_lens::BloodLens;
use crate::dungeon::Dungeon;
use crate::juice::{Hitstop, Trauma};

/// What kind of gore to spawn — scales counts/sizes and gates the gib shatter.
#[derive(Clone, Copy)]
pub enum GoreKind {
    /// A bolt bit flesh: a small spray + spatter.
    FleshHit,
    /// A squad unit died: full spray + pool + flying gib chunks (the "crunch").
    UnitCrunch,
    /// A billboard enemy died: full spray + pool, but no gibs (it has no mesh to shatter).
    EnemySplat,
}

/// For a [`GoreKind::UnitCrunch`]: which baked character to gib and how to place it. Kept separate
/// from `pos` (which stays at chest height for the blood layers) so fragments spawn from the
/// figurine's true foot origin at its render scale. `None` for hits and billboard enemies.
pub struct GibSource {
    /// The character's source scene asset id — the key into [`AutogibCache`].
    pub source: AssetId<WorldAsset>,
    /// The figurine's foot origin in world space (fragments are placed relative to this).
    pub origin: Vec3,
    /// The unit's uniform render scale (figurine-local fragments are scaled by this at spawn).
    pub scale: f32,
}

/// A request for gore at a world position. Anything can push one into [`GoreQueue`].
pub struct GoreEvent {
    pub pos: Vec3,
    pub kind: GoreKind,
    /// Tint for gib chunks (a unit's outfit color); ignored for kinds without gibs.
    pub tint: Color,
    /// Character to fracture into fragment gibs (only `UnitCrunch` sets this; see [`GibSource`]).
    pub gib: Option<GibSource>,
    /// Feel-layer scale in `[0, 1]` for THIS death's screen-shake, hitstop, and on-screen blood —
    /// proportional to the dead thing's "mass" ([`death_intensity`]) so a giant boss kicks the camera
    /// hard while a swarm crab barely registers (40 of them must not read as one huge explosion). The
    /// gib/pool visuals below are NOT scaled by this (the chunks are the same regardless of who died).
    pub intensity: f32,
}

/// Feel intensity for a death from the dead thing's max HP plus a weight on its damage output,
/// normalized so the smiley boss (the heaviest threat) ≈ 1.0 and a lone swarm crab barely nudges the
/// camera. The `0.03` floor keeps a kill from being *completely* inert without letting chaff stack into
/// an explosion.
pub fn death_intensity(hp_max: f32, dps: f32) -> f32 {
    const REFERENCE_MASS: f32 = 2400.0 + 72.0 * 4.0; // the smiley boss: 2400 HP + weighted 72 DPS
    ((hp_max + dps * 4.0) / REFERENCE_MASS).clamp(0.03, 1.0)
}

/// Blood-pool size factor so a pool relates to the dead thing's size/weight (README): a swarm crab
/// leaves a small mark, the boss a wide slick — instead of every death stamping the same 1.7-unit disc.
/// Driven by [`GoreEvent::intensity`] (a normalized mass proxy in `[0.03, 1.0]`, present for **every**
/// kind), with a small nudge from the unit's render `gib.scale` when present (a direct mesh-*size*
/// signal, only on `UnitCrunch`). Clamped so a crab still marks the floor and the boss can't overflow.
fn pool_scale(intensity: f32, gib_scale: Option<f32>) -> f32 {
    // Mass → multiplier. Linear (not eased) so the swarm's crabs stay genuinely small — 40+ of them
    // must not flood the floor — while heavier deaths grow toward MAX.
    let t = intensity.clamp(0.0, 1.0);
    let mut f = POOL_SCALE_MIN + (POOL_SCALE_MAX - POOL_SCALE_MIN) * t;
    // A larger-than-standard figurine widens its own pool a touch; a smaller one tightens it.
    if let Some(s) = gib_scale {
        f *= 1.0 + (s - 1.0) * POOL_GIB_SCALE_WEIGHT;
    }
    f.clamp(POOL_SCALE_MIN, POOL_SCALE_MAX)
}

/// Blood-pool size multiplier at the extremes of [`pool_scale`]: `MIN` at a featherweight crab
/// (`intensity ≈ 0.03`), `MAX` at the boss (`intensity = 1`). `WEIGHT` is how strongly a `UnitCrunch`'s
/// render scale nudges the factor around 1.0. Tuning taste — eyeball a crab vs a boss death.
const POOL_SCALE_MIN: f32 = 0.35;
const POOL_SCALE_MAX: f32 = 2.0;
const POOL_GIB_SCALE_WEIGHT: f32 = 0.5;

/// World gore requests to service this frame (drained by [`drain_gore`]).
#[derive(Resource, Default)]
pub struct GoreQueue(pub Vec<GoreEvent>);

// ---------------------------------------------------------------------------------------------
// Materials — same recipe as `impact_fx.rs`/`health.rs`: an `AsBindGroup` uniform whose field
// order and types byte-match the WGSL `struct`.
// ---------------------------------------------------------------------------------------------

/// GPU uniform — mirrors `BloodSettings` in `blood_spray.wgsl`.
#[derive(Clone, ShaderType)]
struct BloodSprayUniform {
    color_a: Vec4,
    color_b: Vec4,
    intensity: f32,
    spread: f32,
    speed: f32,
    particle_size: f32,
    gravity: f32,
    spawn_time: f32,
    duration: f32,
    seed: f32,
    particle_count: i32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct BloodSprayMaterial {
    #[uniform(0)]
    settings: BloodSprayUniform,
}

impl Material for BloodSprayMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/blood_spray.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}

/// GPU uniform — mirrors `PoolSettings` in `blood_pool.wgsl`.
#[derive(Clone, ShaderType)]
struct BloodPoolUniform {
    color: Vec4,
    /// Per-axis clip in quad-`p` units `(+X, -X, +Z, -Z)` — the pool is masked past a wall face so
    /// it can't seep through walls. Large values (wall splats) disable clipping.
    clip: Vec4,
    /// Per-diagonal clip `(+X+Z, -X+Z, +X-Z, -X-Z)`, same units, so corners can't leak past a wall.
    clip_diag: Vec4,
    spawn_time: f32,
    grow_time: f32,
    gloss: f32,
    seed: f32,
    /// Seconds for blood to fully "dry" — it darkens to matte maroon and loses its wet glint over this.
    dry_time: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct BloodPoolMaterial {
    #[uniform(0)]
    settings: BloodPoolUniform,
    #[texture(1)]
    #[sampler(2)]
    base: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    normal: Handle<Image>,
}

impl Material for BloodPoolMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/blood_pool.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}

// ---------------------------------------------------------------------------------------------
// Components + shared assets.
// ---------------------------------------------------------------------------------------------

/// A short-lived gore entity (blood spray); despawns when the clock passes `despawn_at`.
#[derive(Component)]
struct GoreFx {
    despawn_at: f32,
}

/// A permanent floor stain. Capped in number by [`cap_blood_pools`] via [`PoolRing`].
#[derive(Component)]
struct BloodPool;


/// Marker for every physics gib chunk (autogib fragment, flung gun, or meat blob). Motion is owned by
/// avian3d — the chunk is a `RigidBody::Dynamic` with a box collider, launched with an initial
/// linear + angular velocity, that tumbles, bounces off the floor/walls/other chunks, and settles.
/// All chunks are registered in [`GibRing`] so the total count stays bounded.
#[derive(Component)]
pub struct GibChunk;

/// The last *confined* ground position (XZ, y=0) of a gib. `confine_gibs` sweeps each frame's motion
/// from here with [`Dungeon::resolve_move`] so a chunk that drifts into — or arcs over — a wall is
/// stopped at the room-side wall face and pulled back into the walkable area (walls are only
/// `WALL_HEIGHT` tall, so without this a launched chunk clears them onto the unbounded floor plane).
#[derive(Component)]
struct GibConfine(Vec3);

/// Half-extent used when sweeping a gib against walls (thin — chunks are small debris).
pub const GIB_CONFINE_HALF: f32 = 0.05;

/// A meat chunk the crabs can scavenge and haul to their nest. `weight = density × mesh_volume`; a crab
/// has a finite carry capacity, so a heavy chunk needs several crabs (Σ capacities ≥ weight) to lift it
/// (cooperative transport). The carry state machine (see `crab::carry_gibs`) owns `carriers`/`phase`.
#[derive(Component)]
pub struct Carryable {
    pub weight: f32,
    /// Crabs committed to carrying this chunk (may exceed the minimum once a crew completes).
    pub carriers: Vec<Entity>,
    pub phase: CarryPhase,
    /// The nest chosen as the destination once the chunk is lifted.
    pub nest: Option<Entity>,
    /// Seconds spent in `Crewing` without managing to lift — releases a stuck crew after a timeout so
    /// its crabs disband and forage elsewhere (prevents a too-heavy chunk deadlocking its haulers).
    pub crew_timer: f32,
}

/// Lifecycle of a carryable chunk.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CarryPhase {
    /// On the ground, no committed crabs.
    Resting,
    /// Crabs are gathering but the crew can't lift it yet (Σ capacity < weight).
    Crewing,
    /// Lifted and moving to the nest (kinematic, gib-led).
    Hauling,
}

/// Density used to turn a chunk's mesh volume into a carry weight (`weight = density × volume`). Tuned
/// against `crab::CRAB_CARRY_CAPACITY` so small chunks go solo and big ones need a crew.
const MEAT_DENSITY: f32 = 900.0;
/// Fill fraction applied to an autogib fragment's *bounding-box* volume when pricing it as a carry
/// weight (the severed mesh fills only part of its box). Chosen so a body part lands in the same
/// haulable range as a meat chunk — light bits go solo, a torso needs a few crabs.
const FRAG_FILL: f32 = 0.12;
/// Carry-weight bounds for a scavengable body part (kept haulable so a crew always forms).
const FRAG_WEIGHT_MIN: f32 = 0.08;
const FRAG_WEIGHT_MAX: f32 = 1.5;

/// Shared meshes/materials for every gore entity.
#[derive(Resource)]
struct GoreAssets {
    /// 1×1 quad reused (billboard for spray, floor decal for pools) exactly like `impact_fx`.
    quad: Handle<Mesh>,
    /// Unit cube reused for gib chunks (scaled per-gib via its transform).
    gib: Handle<Mesh>,
    /// The low-poly meat-chunk meshes from `assets/meat_chunks/meatpack.glb` (normalized to ~1 unit
    /// and centered at origin). Two distinct shapes; a chunk picks one by hash for variety.
    meat_meshes: Vec<Handle<Mesh>>,
    /// One shared raw-meat textured material for every meat chunk. Shared (not per-chunk) so the
    /// *permanent* meat can't grow the material-asset count without bound.
    meat_mat: Handle<StandardMaterial>,
    /// Small sphere for airborne blood droplets (scaled per-droplet).
    droplet: Handle<Mesh>,
    /// Shared glossy dark-red material for droplets (a touch emissive so fresh blood glistens).
    droplet_mat: Handle<StandardMaterial>,
    /// Real blood PBR maps (an atlas of splatters + its normal) sampled by the pool decal for
    /// photo-real surface detail and wet micro-glints.
    blood_base: Handle<Image>,
    blood_normal: Handle<Image>,
}

/// One airborne blood droplet: a ballistic bit that arcs out and, on hitting floor/wall, stamps a
/// small blood decal there and vanishes (the codex's particle → OnCollision → decal loop).
#[derive(Component)]
struct Droplet {
    velocity: Vec3,
    /// Radius (world units) — sets resting/impact height and whether it's heavy enough to leave a mark.
    size: f32,
    /// Safety fallback despawn (a droplet that somehow never lands).
    despawn_at: f32,
}

/// Precomputed unit volumes of the meat-chunk meshes (parallel to `GoreAssets.meat_meshes`), so a
/// chunk's carry weight can be computed at spawn without re-reading the mesh. Empty until the GLB loads.
#[derive(Resource, Default)]
struct MeatVolumes {
    unit: Vec<f32>,
}

/// FIFO of live blood-pool entities so the oldest can be recycled once the cap is exceeded.
#[derive(Resource, Default)]
struct PoolRing(VecDeque<Entity>);

/// FIFO of live physics gib chunks (fragments, guns, meat) so the oldest can be recycled once the
/// total exceeds the cap — bounds how many rigid bodies the solver ever tracks.
#[derive(Resource, Default)]
pub struct GibRing(pub VecDeque<Entity>);

impl GibRing {
    /// The ONE path that removes a live gib early (a crab carrying it home to the nest). It drops the
    /// id from the ring *first*, so `cap_gib_chunks` — which despawns ring entries unconditionally —
    /// can never double-despawn a consumed chunk. No other code may despawn a gib. (Used by `carry_gibs`
    /// when a crew delivers a chunk to the nest.)
    pub fn consume(&mut self, commands: &mut Commands, e: Entity) {
        self.0.retain(|&x| x != e);
        commands.entity(e).despawn();
    }
}

/// Human-facing, serializable knobs — the `gore:` slice of the unified `assets/config/config.ron`.
#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct GoreSettings {
    // Blood spray.
    spray_color_a: [f32; 3],
    spray_color_b: [f32; 3],
    spray_intensity: f32,
    spray_spread: f32,
    spray_speed: f32,
    spray_particle_size: f32,
    spray_gravity: f32,
    spray_duration: f32,
    spray_quad_size: f32,
    spray_count_hit: i32,
    spray_count_death: i32,
    // Blood pool.
    pool_color: [f32; 3],
    pool_grow_time: f32,
    pool_gloss: f32,
    pool_size_hit: f32,
    pool_size_death: f32,
    /// Size of a blood splat stamped on a wall at a death.
    wall_splat_size: f32,
    /// Seconds for a pool to dry (darken to matte); fresh blood is bright + glossy.
    dry_time: f32,
    max_pools: usize,
    // Gib physics launch + material (avian): shared by autogib fragments and meat chunks. Gravity is
    // global (see `main::GIB_GRAVITY`); these tune the throw and how a chunk bounces/slides.
    gib_speed: f32,
    /// Bounciness of a chunk on impact (avian `Restitution`; 0 = dead thud, 1 = perfectly elastic).
    chunk_restitution: f32,
    /// Surface friction of a chunk (avian `Friction`; higher = slides less and settles sooner).
    gib_friction: f32,
    // Autogib (unit crunch: the figurine mesh sliced into flying fragments; see `autogib`).
    /// Fragment count at `autogib_ref_extent`; scaled by the mesh's actual bounding size.
    pub autogib_pieces_base: i32,
    /// Reference character half-extent the base piece count is tuned for.
    pub autogib_ref_extent: f32,
    /// Clamp on the fragment count (lower / upper — the upper bounds mesh + entity growth).
    pub autogib_min_pieces: i32,
    pub autogib_max_pieces: i32,
    /// Stop cutting a piece once its extent drops below this fraction of the whole mesh's extent.
    pub autogib_min_fraction: f32,
    /// Fragment launch speed as a multiple of `gib_speed`.
    pub autogib_speed_mult: f32,
    // Meat chunks (any death) + the overall physics-chunk cap.
    meat_count: i32,
    meat_size: f32,
    /// Max live physics gib chunks (fragments + guns + meat) before the oldest is recycled.
    max_gibs: usize,
    // Airborne blood droplets (arc + splat on contact).
    droplet_count_hit: i32,
    droplet_count_death: i32,
    droplet_speed: f32,
    droplet_gravity: f32,
    droplet_size: f32,
    droplet_life: f32,
    droplet_splat_size: f32,
}

impl Default for GoreSettings {
    fn default() -> Self {
        GoreSettings {
            // Over-the-top arcade: dark arterial → bright oxygenated red, heavy droop so it arcs.
            spray_color_a: [0.45, 0.0, 0.0],
            spray_color_b: [0.85, 0.08, 0.05],
            spray_intensity: 1.0,
            spray_spread: 0.9,
            spray_speed: 1.0,
            spray_particle_size: 7.0,
            spray_gravity: 0.9,
            // Short + small now — a quick muzzle flash of mist; the droplets are the real airborne read.
            spray_duration: 0.28,
            spray_quad_size: 1.5,
            spray_count_hit: 8,
            spray_count_death: 30,
            pool_color: [0.42, 0.02, 0.0],
            pool_grow_time: 0.45,
            pool_gloss: 0.9,
            pool_size_hit: 0.7,
            pool_size_death: 1.7,
            wall_splat_size: 0.7,
            dry_time: 22.0,
            max_pools: 300,
            gib_speed: 4.0,
            chunk_restitution: 0.45,
            gib_friction: 0.7,
            autogib_pieces_base: 14,
            autogib_ref_extent: 0.5,
            autogib_min_pieces: 6,
            autogib_max_pieces: 40,
            autogib_min_fraction: 0.18,
            autogib_speed_mult: 0.8,
            meat_count: 5,
            meat_size: 0.17,
            max_gibs: 200,
            // Many tiny droplets — real blood splatter is a fine mist of small drops, not marbles.
            droplet_count_hit: 18,
            droplet_count_death: 90,
            droplet_speed: 5.5,
            droplet_gravity: 20.0,
            droplet_size: 0.022,
            droplet_life: 2.0,
            droplet_splat_size: 0.12,
        }
    }
}

impl GoreSettings {
    fn spray_uniform(&self, spawn_time: f32, seed: f32, count: i32) -> BloodSprayUniform {
        BloodSprayUniform {
            color_a: Vec4::new(self.spray_color_a[0], self.spray_color_a[1], self.spray_color_a[2], 1.0),
            color_b: Vec4::new(self.spray_color_b[0], self.spray_color_b[1], self.spray_color_b[2], 1.0),
            intensity: self.spray_intensity,
            spread: self.spray_spread,
            speed: self.spray_speed,
            particle_size: self.spray_particle_size,
            gravity: self.spray_gravity,
            spawn_time,
            duration: self.spray_duration,
            seed,
            particle_count: count.clamp(0, 64),
        }
    }

    fn pool_uniform(&self, spawn_time: f32, seed: f32, clip: Vec4, clip_diag: Vec4) -> BloodPoolUniform {
        BloodPoolUniform {
            color: Vec4::new(self.pool_color[0], self.pool_color[1], self.pool_color[2], 1.0),
            clip,
            clip_diag,
            spawn_time,
            grow_time: self.pool_grow_time,
            gloss: self.pool_gloss,
            dry_time: self.dry_time,
            seed,
        }
    }
}

pub struct GorePlugin;

impl Plugin for GorePlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback. The `gore:` slice comes from the unified
        // `assets/config/config.ron`, loaded + validated once by `ConfigPlugin` (registered first).
        let settings = app.world().resource::<crate::config::GameConfig>().gore.clone();
        app.add_plugins(MaterialPlugin::<BloodSprayMaterial>::default())
            .add_plugins(MaterialPlugin::<BloodPoolMaterial>::default())
            .init_resource::<GoreQueue>()
            .init_resource::<PoolRing>()
            .init_resource::<GibRing>()
            .init_resource::<MeatVolumes>()
            .insert_resource(settings)
            .add_systems(Startup, setup_gore_assets)
            .add_systems(
                Update,
                (
                    compute_meat_volumes,
                    drain_gore,
                    update_droplets,
                    confine_gibs,
                    cap_blood_pools,
                    cap_gib_chunks,
                    despawn_gore,
                ),
            );
    }
}

/// Once the meat-chunk GLB meshes have loaded, compute each one's unit volume (signed-tetrahedron sum)
/// so meat chunks can be weighed at spawn. Runs each frame until filled, then is a cheap no-op.
fn compute_meat_volumes(
    assets: Option<Res<GoreAssets>>,
    meshes: Res<Assets<Mesh>>,
    mut volumes: ResMut<MeatVolumes>,
) {
    let Some(assets) = assets else { return };
    if !volumes.unit.is_empty() || assets.meat_meshes.is_empty() {
        return;
    }
    // Only fill once every mesh has resolved (they load a few frames after startup, before any death).
    let mut unit = Vec::with_capacity(assets.meat_meshes.len());
    for handle in &assets.meat_meshes {
        let Some(mesh) = meshes.get(handle) else { return };
        let Some(v) = crate::autogib::mesh_signed_volume(mesh) else {
            return;
        };
        unit.push(v);
    }
    if crate::ai::diag::AI_DIAG {
        info!("gore: meat unit volumes = {unit:?}");
    }
    volumes.unit = unit;
}

fn setup_gore_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    // Raw-meat textured material shared by every meat chunk (permanent, so one shared handle keeps
    // the material-asset count bounded). The meshes carry UVs from the source model.
    let meat_mat = std_mats.add(StandardMaterial {
        base_color_texture: Some(asset_server.load("meat_chunks/Raw textured meat close-up.png")),
        perceptual_roughness: 0.8,
        ..default()
    });
    // The two distinct low-poly chunk shapes from the converted meatpack (indices 0 and 3 of the six
    // duplicated objects). Loaded as bare `Mesh` handles so we position/scale each instance ourselves.
    let meat_meshes = vec![
        asset_server.load("meat_chunks/meatpack.glb#Mesh0/Primitive0"),
        asset_server.load("meat_chunks/meatpack.glb#Mesh3/Primitive0"),
    ];
    // Droplets: a glossy dark-red bit, faintly emissive so fresh blood catches the eye in flight.
    let droplet_mat = std_mats.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.0, 0.0),
        emissive: LinearRgba::rgb(0.12, 0.0, 0.0),
        perceptual_roughness: 0.35,
        ..default()
    });
    commands.insert_resource(GoreAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        gib: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        meat_meshes,
        meat_mat,
        droplet: meshes.add(Sphere::new(0.5)),
        droplet_mat,
        blood_base: asset_server.load("textures/blood/blood_base.png"),
        blood_normal: asset_server.load("textures/blood/blood_normal.png"),
    });
}

/// Service every queued gore request: spray (all), pool (all), and gibs (unit crunch only).
#[allow(clippy::too_many_arguments)]
fn drain_gore(
    mut commands: Commands,
    time: Res<Time>,
    mut queue: ResMut<GoreQueue>,
    settings: Res<GoreSettings>,
    assets: Res<GoreAssets>,
    mut spray_mats: ResMut<Assets<BloodSprayMaterial>>,
    mut pool_mats: ResMut<Assets<BloodPoolMaterial>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut ring: ResMut<PoolRing>,
    mut gib_ring: ResMut<GibRing>,
    dungeon: Res<Dungeon>,
    real: Res<Time<Real>>,
    // Grouped into one tuple param to stay under Bevy's 16-param-per-system cap.
    (mut trauma, mut hitstop, mut blood_lens, cache, volumes, fog): (
        ResMut<Trauma>,
        ResMut<Hitstop>,
        ResMut<BloodLens>,
        Res<AutogibCache>,
        Res<MeatVolumes>,
        Res<crate::fog::FogGrid>,
    ),
    camera: Single<&GlobalTransform, With<Camera3d>>,
    mut seed: Local<u32>,
) {
    if queue.0.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    let now_real = real.elapsed_secs();
    let cam_rot = camera.rotation();
    let cam_pos = camera.translation();

    for ev in queue.0.drain(..) {
        *seed = seed.wrapping_add(1);
        let fseed = *seed as f32 * 0.618;

        // --- SIM viscera (kills only): the unit's own mesh sliced into flying fragment gibs (raw-meat
        //     cut faces, blaster flung off intact — see `autogib`) plus permanent meat chunks. The crab
        //     forage→haul→breed economy consumes these, so they spawn for EVERY kill regardless of the
        //     squad's line of sight. Gating a simulation resource on fog (a pure presentation concern)
        //     would starve nests of food from off-screen kills — e.g. the smiley devouring a pile of
        //     crabs, or being killed, in a room the squad isn't currently viewing.
        if let GoreKind::UnitCrunch = ev.kind {
            if let Some(g) = &ev.gib {
                spawn_fragments(
                    &mut commands,
                    &cache,
                    &assets,
                    &mut std_mats,
                    &settings,
                    &mut gib_ring,
                    g.source,
                    g.origin,
                    g.scale,
                    ev.tint,
                    *seed,
                );
            } else {
                warn!("gore: UnitCrunch without a gib source; no fragment gibs spawned");
            }
        }
        if let GoreKind::UnitCrunch | GoreKind::EnemySplat = ev.kind {
            spawn_meat_chunks(&mut commands, &assets, &settings, &mut gib_ring, &volumes, ev.pos, *seed);
        }

        // --- Presentation feedback below is fog-gated: a kill the squad can't currently see leaves NO
        //     visible trace — no blood spray/pool/splatter, no screen-shake/hitstop/lens (but the sim
        //     viscera above still spawned). `FleshHit` is never gated (the laser only fires at
        //     fog-visible enemies), and squad deaths always happen in view.
        if matches!(ev.kind, GoreKind::UnitCrunch | GoreKind::EnemySplat)
            && !fog.visible_at(dungeon.world_to_cell(ev.pos))
        {
            continue;
        }

        // Feel layer: kick the camera, freeze-frame, and splatter the lens — **on a kill only**.
        // Flesh hits get no shake (auto-fire lands many per second; shaking on each is nauseating).
        if let GoreKind::UnitCrunch | GoreKind::EnemySplat = ev.kind {
            let k = ev.intensity.clamp(0.0, 1.0);
            trauma.add(0.85 * k);
            hitstop.freeze(now_real, 0.11 * k);
            blood_lens.splash(0.7 * k);
        }

        // Per-kind sizing: a hit is a small tap, a death is the full show.
        let (spray_count, quad_size, pool_size, droplet_count) = match ev.kind {
            GoreKind::FleshHit => (
                settings.spray_count_hit,
                settings.spray_quad_size * 0.5,
                settings.pool_size_hit,
                settings.droplet_count_hit,
            ),
            GoreKind::UnitCrunch | GoreKind::EnemySplat => (
                settings.spray_count_death,
                settings.spray_quad_size,
                settings.pool_size_death,
                settings.droplet_count_death,
            ),
        };

        // --- Airborne blood droplets: real ballistic drops that arc out and splat on contact (the
        //     primary airborne read; the billboard spray below is just a quick muzzle flash now).
        spawn_droplets(&mut commands, &assets, &settings, ev.pos, droplet_count, now, *seed);

        // --- Airborne blood spray (camera-facing billboard). Nudge toward the camera so it isn't
        //     clipped by the wall/floor it landed against, exactly like `impact_fx::drain_impacts`.
        let toward = (cam_pos - ev.pos).normalize_or_zero();
        let spray = spray_mats.add(BloodSprayMaterial {
            settings: settings.spray_uniform(now, fseed, spray_count),
        });
        commands.spawn((
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(spray),
            Transform::from_translation(ev.pos + toward * 0.3)
                .with_rotation(cam_rot)
                .with_scale(Vec3::splat(quad_size)),
            GoreFx {
                despawn_at: now + settings.spray_duration,
            },
        ));

        // --- Blood pool on the floor (flat XZ decal, lifted to avoid z-fighting with floor tiles;
        //     same orientation as the selection ring in `selection.rs`). Permanent, but capped.
        // Scale the base pool by the dead thing's mass/size so a crab's mark ≠ the boss's slick.
        let pool_size = pool_size * pool_scale(ev.intensity, ev.gib.as_ref().map(|g| g.scale));
        let floor_pos = Vec3::new(ev.pos.x, 0.02, ev.pos.z);
        // Clip the pool to the surrounding walls so it can't seep through them. `p` spans the quad
        // in [-1,1], so a world clear-distance maps to p-units by dividing by the quad half-size.
        let pool_half = (pool_size * 0.5).max(0.0001);
        let (ext_axis, ext_diag) = dungeon.open_extents(floor_pos, pool_size * 0.5 + 0.1);
        let pool = pool_mats.add(BloodPoolMaterial {
            settings: settings.pool_uniform(now, fseed, ext_axis / pool_half, ext_diag / pool_half),
            base: assets.blood_base.clone(),
            normal: assets.blood_normal.clone(),
        });
        let pool_id = commands
            .spawn((
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(pool),
                Transform::from_translation(floor_pos)
                    .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
                    .with_scale(Vec3::splat(pool_size)),
                BloodPool,
            ))
            .id();
        ring.0.push_back(pool_id);

        // --- Wall splatter (any death): blood on the walls bounding the death cell. (The permanent
        //     meat chunks and fragment gibs already spawned above, before the fog gate — they feed the
        //     crab economy and so must not depend on line of sight.)
        if let GoreKind::UnitCrunch | GoreKind::EnemySplat = ev.kind {
            spawn_wall_splatters(
                &mut commands,
                &assets,
                &mut pool_mats,
                &settings,
                &dungeon,
                &mut ring,
                ev.pos,
                now,
                *seed,
            );
        }
    }
}

/// Spawn one physics gib chunk and register it in the cap ring: a `RigidBody::Dynamic` with a box
/// collider of world half-extents `half`, launched at `velocity` (linear) and `spin` (angular).
/// avian3d owns its motion — it tumbles, bounces off the floor/walls/other chunks, and settles, then
/// sleeps (extended position-based dynamics; Müller, Macklin, Chentanez, Jeschke & Kim, "Detailed
/// Rigid Body Simulation with Extended Position Based Dynamics", SCA 2020). Returns the parent entity
/// so the caller can attach the visual mesh child(ren) at the render `scale`.
#[allow(clippy::too_many_arguments)]
/// Keep every gib chunk inside the walkable area. Gibs are dynamic bodies and the walls are only
/// `WALL_HEIGHT` (=1) tall, so a chunk launched upward can clear a wall and land on the infinite floor
/// plane out in the void. Each frame we sweep the chunk's ground motion with [`Dungeon::resolve_move`],
/// which stops a box at the **room-side wall face** (not the wall centre); if the chunk crossed a wall,
/// it's snapped back onto the floor and its horizontal velocity killed so it can't push through again.
/// avian 0.7 syncs `Transform` → `Position` before each sim step, so writing `Transform` here holds.
fn confine_gibs(
    dungeon: Res<Dungeon>,
    mut gibs: Query<
        (&mut Transform, &mut LinearVelocity, &mut GibConfine, Option<&Carryable>),
        With<GibChunk>,
    >,
) {
    for (mut tf, mut lv, mut prev, carry) in &mut gibs {
        // A chunk mid-haul is Kinematic and driven along the nest flow field by `crab::carry_gibs` —
        // the sole authority over its transform then. Skip confinement so the two systems don't fight
        // (confine would yank a hauled chunk back toward its last grounded XZ when the haul route skirts
        // a wall). Keep `prev` synced to the live position so that, if the crew drops it back to a
        // Dynamic body, confinement resumes from wherever the haul left it, not a stale pre-haul point.
        if carry.is_some_and(|c| c.phase == CarryPhase::Hauling) {
            prev.0 = Vec3::new(tf.translation.x, 0.0, tf.translation.z);
            continue;
        }
        let moved = Vec3::new(tf.translation.x - prev.0.x, 0.0, tf.translation.z - prev.0.z);
        let resolved = dungeon.resolve_move(prev.0, moved, Vec2::splat(GIB_CONFINE_HALF));
        if (resolved.x - tf.translation.x).abs() > 1.0e-4
            || (resolved.z - tf.translation.z).abs() > 1.0e-4
        {
            tf.translation.x = resolved.x;
            tf.translation.z = resolved.z;
            lv.0.x = 0.0;
            lv.0.z = 0.0;
        }
        prev.0 = Vec3::new(tf.translation.x, 0.0, tf.translation.z);
    }
}

fn spawn_gib_body(
    commands: &mut Commands,
    gib_ring: &mut GibRing,
    settings: &GoreSettings,
    pos: Vec3,
    half: Vec3,
    velocity: Vec3,
    spin: Vec3,
) -> Entity {
    let id = commands
        .spawn((
            // The body sits at world scale 1 (the render scale lives on the mesh children), so the
            // collider dimensions are the true world size — no Transform-scale/collider mismatch.
            RigidBody::Dynamic,
            Collider::cuboid(2.0 * half.x, 2.0 * half.y, 2.0 * half.z),
            LinearVelocity(velocity),
            AngularVelocity(spin),
            Restitution::new(settings.chunk_restitution),
            Friction::new(settings.gib_friction),
            Transform::from_translation(pos),
            Visibility::default(),
            GibChunk,
            GibConfine(pos.with_y(0.0)),
        ))
        .id();
    gib_ring.0.push_back(id);
    id
}

/// Spawn the unit's pre-baked mesh fragments as flying **physics** chunks (see `autogib`): each
/// fragment is a dynamic rigid body carrying two child meshes — the outfit-tinted outer skin and the
/// raw-meat cut face (`assets.meat_mat`) — so a crunched body reads as real severed pieces that
/// tumble and pile. The carried blaster is flung off as one intact chunk keeping its own material.
/// Chunks launch outward from the body center and let avian carry them the rest of the way. One path:
/// if the character wasn't baked we fail loudly (`warn!`) and spawn nothing — no fallback.
#[allow(clippy::too_many_arguments)]
fn spawn_fragments(
    commands: &mut Commands,
    cache: &AutogibCache,
    assets: &GoreAssets,
    std_mats: &mut Assets<StandardMaterial>,
    settings: &GoreSettings,
    gib_ring: &mut GibRing,
    source: AssetId<WorldAsset>,
    origin: Vec3,
    scale: f32,
    tint: Color,
    seed: u32,
) {
    let Some(frags) = cache.fragments(source) else {
        warn!("gore: no autogib bake for this character; skipping fragment gibs");
        return;
    };

    // One flat outfit material shared by every outer-skin piece of this death (bounded asset growth).
    // Cut faces reuse the shared raw-meat material.
    let outfit_mat = std_mats.add(StandardMaterial {
        base_color: tint,
        perceptual_roughness: 0.85,
        ..default()
    });

    let speed_mult = settings.autogib_speed_mult;
    for (i, frag) in frags.iter().enumerate() {
        let base = seed
            .wrapping_mul(2_246_822_519)
            .wrapping_add((i as u32).wrapping_mul(2_654_435_761));
        let h1 = hash_f32(base.wrapping_add(1));
        let h2 = hash_f32(base.wrapping_add(2));
        let h3 = hash_f32(base.wrapping_add(3));
        let h4 = hash_f32(base.wrapping_add(4));
        let h5 = hash_f32(base.wrapping_add(5));

        // Launch outward from the body center with an upward bias — pieces burst from where they sat.
        let out_dir = frag.center_local.normalize_or_zero();
        let angle = h1 * TAU;
        let jitter = Vec3::new(angle.cos(), 0.0, angle.sin()) * 0.5;
        let up = 0.6 + 0.8 * h3;
        let dir = (out_dir + jitter + Vec3::Y * up).normalize_or_zero();
        let velocity = dir * settings.gib_speed * speed_mult * (0.6 + 0.8 * h4);
        let spin = Vec3::new(h1 - 0.5, h2 - 0.5, h5 - 0.5).normalize_or_zero() * (8.0 + 8.0 * h4);
        let half = (frag.half_extents * scale).max(Vec3::splat(0.02));
        let pos = origin + frag.center_local * scale;

        let id = spawn_gib_body(commands, gib_ring, settings, pos, half, velocity, spin);
        // A severed body part is scavengable: crabs sense it via the MEAT field (see
        // `crab::deposit_meat_scent`, which keys on `Carryable`), crew up, and haul it back to their
        // nest exactly like a meat chunk — so the swarm carries off the player's remains instead of
        // milling around them. Weight ∝ the piece's (fractionally-filled) box volume, clamped haulable.
        let vol = 2.0 * half.x * 2.0 * half.y * 2.0 * half.z;
        let weight = (MEAT_DENSITY * vol * FRAG_FILL).clamp(FRAG_WEIGHT_MIN, FRAG_WEIGHT_MAX);
        commands.entity(id).insert(Carryable {
            weight,
            carriers: Vec::new(),
            phase: CarryPhase::Resting,
            nest: None,
            crew_timer: 0.0,
        });
        commands.entity(id).with_children(|parent| {
            let child_scale = Transform::from_scale(Vec3::splat(scale));
            if let Some(outer) = &frag.outer_mesh {
                parent.spawn((Mesh3d(outer.clone()), MeshMaterial3d(outfit_mat.clone()), child_scale));
            }
            if let Some(cap) = &frag.cap_mesh {
                parent.spawn((Mesh3d(cap.clone()), MeshMaterial3d(assets.meat_mat.clone()), child_scale));
            }
        });
    }

    // The blaster: one intact tumbling chunk that keeps its own material, flung a touch faster.
    if let Some(gun) = cache.gun(source) {
        let base = seed.wrapping_mul(40_507).wrapping_add(0x00C0_FFEE);
        let h1 = hash_f32(base.wrapping_add(1));
        let h2 = hash_f32(base.wrapping_add(2));
        let h3 = hash_f32(base.wrapping_add(3));
        let h4 = hash_f32(base.wrapping_add(4));
        let h5 = hash_f32(base.wrapping_add(5));

        let out_dir = gun.center_local.normalize_or_zero();
        let angle = h1 * TAU;
        let jitter = Vec3::new(angle.cos(), 0.0, angle.sin()) * 0.6;
        let up = 0.7 + 0.9 * h3;
        let dir = (out_dir + jitter + Vec3::Y * up).normalize_or_zero();
        let velocity = dir * settings.gib_speed * speed_mult * (1.0 + 0.6 * h4);
        let spin = Vec3::new(h1 - 0.5, h2 - 0.5, h5 - 0.5).normalize_or_zero() * (10.0 + 8.0 * h4);
        let half = (gun.half_extents * scale).max(Vec3::splat(0.02));
        let pos = origin + gun.center_local * scale;

        let id = spawn_gib_body(commands, gib_ring, settings, pos, half, velocity, spin);
        commands.entity(id).with_children(|parent| {
            parent.spawn((
                Mesh3d(gun.mesh.clone()),
                MeshMaterial3d(gun.material.clone()),
                Transform::from_scale(Vec3::splat(scale)),
            ));
        });
    }
}

/// Spawn fleshy meat chunks as physics bodies that fly out, tumble, bounce, and settle into a pile of
/// viscera on the floor. Registered in [`GibRing`] so [`cap_gib_chunks`] bounds the total. Runs for
/// any death (squad + enemy). The meat meshes are normalized to ~1 unit, so a chunk of half-extent
/// `half` renders at child scale `half*2` over a `half`-sized box collider.
#[allow(clippy::too_many_arguments)]
fn spawn_meat_chunks(
    commands: &mut Commands,
    assets: &GoreAssets,
    settings: &GoreSettings,
    gib_ring: &mut GibRing,
    volumes: &MeatVolumes,
    origin: Vec3,
    seed: u32,
) {
    for i in 0..settings.meat_count.max(0) {
        // Offset the seed base from the fragments so meat directions don't mirror the mesh chunks.
        let base = seed
            .wrapping_mul(2_246_822_519)
            .wrapping_add((i as u32).wrapping_mul(3_266_489_917))
            .wrapping_add(0xA5A5_A5A5);
        let h1 = hash_f32(base.wrapping_add(1));
        let h2 = hash_f32(base.wrapping_add(2));
        let h3 = hash_f32(base.wrapping_add(3));
        let h4 = hash_f32(base.wrapping_add(4));
        let h5 = hash_f32(base.wrapping_add(5));

        // Hemispherical up-and-out launch, like the fragments.
        let angle = h1 * TAU;
        let horiz = 0.4 + 0.7 * h2;
        let up = 0.7 + 0.9 * h3;
        let dir = Vec3::new(angle.cos() * horiz, up, angle.sin() * horiz).normalize_or_zero();
        let velocity = dir * settings.gib_speed * (0.6 + 0.8 * h4);
        let spin = Vec3::new(h1 - 0.5, h2 - 0.5, h5 - 0.5).normalize_or_zero() * (8.0 + 8.0 * h4);
        let half = 0.5 * settings.meat_size * (0.6 + 0.8 * h5);

        // Pick one of the meat-chunk shapes (guard the index in case the list is ever empty).
        let idx = if assets.meat_meshes.is_empty() {
            0
        } else {
            (base as usize) % assets.meat_meshes.len()
        };
        let mesh = if assets.meat_meshes.is_empty() {
            assets.gib.clone()
        } else {
            assets.meat_meshes[idx].clone()
        };
        // Carry weight = density × world mesh volume. The chunk mesh is the unit mesh scaled by 2*half,
        // so volume = unit_volume × (2*half)³. Falls back to the box volume until the GLB volumes load.
        let side = (2.0 * half).powi(3);
        // Sanitize the mesh volume at the door: a degenerate GLB can compute a NaN/inf/≤0 volume, and
        // `f32::clamp` passes a NaN operand straight through (NaN < min and NaN > max are both false),
        // so an unguarded NaN would yield a NaN weight that no crew's `Σcapacity ≥ weight` check can
        // ever satisfy — the chunk would litter the floor forever, denying the nest that food. Treat a
        // non-finite / non-positive volume exactly like an un-loaded one: fall back to the unit box.
        let unit_vol = volumes
            .unit
            .get(idx)
            .copied()
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(1.0);
        // Clamp to the same liftable band as fragments. Without it, an un-loaded `unit_vol` fallback
        // (1.0 vs the real ~0.1-0.5, before the GLB volumes finish computing) or an oversized chunk
        // yields a weight no crew can gather enough capacity to lift — the chunk then sits stuck until
        // the crew times out and the food is wasted.
        let weight = (MEAT_DENSITY * unit_vol * side).clamp(FRAG_WEIGHT_MIN, FRAG_WEIGHT_MAX);
        if crate::ai::diag::AI_DIAG {
            info!("gore: meat chunk idx={idx} half={half:.3} weight={weight:.2}");
        }
        let pos = origin + Vec3::Y * 0.2;
        let id = spawn_gib_body(commands, gib_ring, settings, pos, Vec3::splat(half), velocity, spin);
        commands.entity(id).insert(Carryable {
            weight,
            carriers: Vec::new(),
            phase: CarryPhase::Resting,
            nest: None,
            crew_timer: 0.0,
        });
        commands.entity(id).with_children(|parent| {
            parent.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(assets.meat_mat.clone()),
                Transform::from_scale(Vec3::splat(half * 2.0)),
            ));
        });
    }
}

/// Stamp a blood splat on each wall bounding the death cell — a [`BloodPoolMaterial`] decal stood up
/// on the wall's inner face, oriented into the room. Reuses the pool material/marker/cap so wall
/// splats share the floor pools' recycling budget. See [`Dungeon::wall_faces_near`].
#[allow(clippy::too_many_arguments)]
fn spawn_wall_splatters(
    commands: &mut Commands,
    assets: &GoreAssets,
    pool_mats: &mut Assets<BloodPoolMaterial>,
    settings: &GoreSettings,
    dungeon: &Dungeon,
    ring: &mut PoolRing,
    pos: Vec3,
    now: f32,
    seed: u32,
) {
    for (idx, (face, normal)) in dungeon.wall_faces_near(pos).into_iter().enumerate() {
        // Skip camera-facing (E/S) walls while they're knee-high: a full-height splat would hang in the
        // air above the squashed wall. Same guard crab (crab.rs) and furnish (furnish.rs) apply to
        // wall_faces_near — gore was the one subsystem missing it.
        if crate::dungeon::SHORT_CAMERA_WALLS && crate::dungeon::is_camera_facing(normal) {
            continue;
        }
        let h = hash_f32(seed.wrapping_mul(97).wrapping_add(idx as u32 * 61 + 7));
        let h2 = hash_f32(seed.wrapping_mul(89).wrapping_add(idx as u32 * 41 + 3));
        // Splat height up the 1-unit wall; width varied a little per splat.
        let height = 0.25 + 0.55 * h;
        let w = settings.wall_splat_size * (0.75 + 0.5 * h2);
        let material = pool_mats.add(BloodPoolMaterial {
            // No wall-clipping for a splat that already lives on a wall.
            settings: settings.pool_uniform(
                now,
                seed as f32 * 0.618 + idx as f32,
                Vec4::splat(9.0),
                Vec4::splat(9.0),
            ),
            base: assets.blood_base.clone(),
            normal: assets.blood_normal.clone(),
        });
        let id = commands
            .spawn((
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(material),
                // `from_rotation_arc(Z, normal)` aims the quad face into the room; nudge off the wall
                // to avoid z-fighting; taller than wide so it reads as running down.
                Transform::from_translation(face + Vec3::Y * height + normal * 0.02)
                    .with_rotation(Quat::from_rotation_arc(Vec3::Z, normal))
                    .with_scale(Vec3::new(w, w * 1.4, w)),
                BloodPool,
            ))
            .id();
        ring.0.push_back(id);
    }
}

/// Fling out a spray of airborne blood droplets from a hit/death. Hemispherical up-and-out launch
/// with wide speed/size variance (fast fine mist + slow heavy gobs). Each is a ballistic sphere that
/// [`update_droplets`] arcs and splats.
fn spawn_droplets(
    commands: &mut Commands,
    assets: &GoreAssets,
    settings: &GoreSettings,
    origin: Vec3,
    count: i32,
    now: f32,
    seed: u32,
) {
    for i in 0..count.max(0) {
        let base = seed
            .wrapping_mul(2_654_435_761)
            .wrapping_add((i as u32).wrapping_mul(40_507));
        let h1 = hash_f32(base.wrapping_add(1));
        let h2 = hash_f32(base.wrapping_add(2));
        let h3 = hash_f32(base.wrapping_add(3));
        let h4 = hash_f32(base.wrapping_add(4));
        let h5 = hash_f32(base.wrapping_add(5));

        // Mostly-outward launch with only a modest upward bias, so drops arc down onto the floor and
        // lower walls instead of rocketing straight into the wall tops (walls are only 1.0 tall).
        let angle = h1 * TAU;
        let horiz = 0.7 + 0.6 * h2;
        let up = 0.15 + 0.55 * h3;
        let dir = Vec3::new(angle.cos() * horiz, up, angle.sin() * horiz).normalize_or_zero();
        let velocity = dir * settings.droplet_speed * (0.4 + 1.3 * h4);
        let size = settings.droplet_size * (0.4 + 1.4 * h5);

        // Spawn near floor-to-waist height (never above the wall top), so the spray starts low.
        let spawn_y = origin.y.min(0.5);
        commands.spawn((
            Mesh3d(assets.droplet.clone()),
            MeshMaterial3d(assets.droplet_mat.clone()),
            Transform::from_translation(Vec3::new(origin.x, spawn_y, origin.z))
                .with_scale(Vec3::splat(size * 2.0)),
            Droplet {
                velocity,
                size,
                despawn_at: now + settings.droplet_life,
            },
        ));
    }
}

/// Stamp a small blood decal where a droplet landed: flat on the floor (clipped to walls, like a
/// pool) or upright on a wall face (like a wall splat). Reuses the pool material + [`PoolRing`] cap.
#[allow(clippy::too_many_arguments)]
fn stamp_droplet_splat(
    commands: &mut Commands,
    assets: &GoreAssets,
    pool_mats: &mut Assets<BloodPoolMaterial>,
    settings: &GoreSettings,
    dungeon: &Dungeon,
    ring: &mut PoolRing,
    pos: Vec3,
    wall_normal: Option<Vec3>,
    now: f32,
    seed: f32,
) {
    let size = settings.droplet_splat_size;
    let (transform, clip, clip_diag) = match wall_normal {
        Some(n) => (
            Transform::from_translation(pos + n * 0.02)
                .with_rotation(Quat::from_rotation_arc(Vec3::Z, n))
                .with_scale(Vec3::splat(size)),
            Vec4::splat(9.0),
            Vec4::splat(9.0),
        ),
        None => {
            let floor = Vec3::new(pos.x, 0.021, pos.z);
            let half = (size * 0.5).max(0.0001);
            let (ea, ed) = dungeon.open_extents(floor, size * 0.5 + 0.1);
            (
                Transform::from_translation(floor)
                    .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
                    .with_scale(Vec3::splat(size)),
                ea / half,
                ed / half,
            )
        }
    };
    let material = pool_mats.add(BloodPoolMaterial {
        settings: settings.pool_uniform(now, seed, clip, clip_diag),
        base: assets.blood_base.clone(),
        normal: assets.blood_normal.clone(),
    });
    let id = commands
        .spawn((Mesh3d(assets.quad.clone()), MeshMaterial3d(material), transform, BloodPool))
        .id();
    ring.0.push_back(id);
}

/// Integrate droplets: gravity + move, resolve against walls, and on the first floor/wall contact
/// stamp a splat (heavier drops only — fine mist just vanishes) and despawn. This is the codex's
/// particle → OnCollision → decal loop.
#[allow(clippy::too_many_arguments)]
fn update_droplets(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<GoreSettings>,
    dungeon: Res<Dungeon>,
    assets: Res<GoreAssets>,
    mut pool_mats: ResMut<Assets<BloodPoolMaterial>>,
    mut ring: ResMut<PoolRing>,
    mut droplets: Query<(Entity, &mut Transform, &mut Droplet)>,
    mut seed: Local<u32>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();

    for (entity, mut tf, mut d) in &mut droplets {
        if now >= d.despawn_at {
            commands.entity(entity).despawn();
            continue;
        }
        d.velocity.y -= settings.droplet_gravity * dt;
        tf.translation.y += d.velocity.y * dt;

        // Stretch the droplet along its velocity so it reads as a motion-blurred streak, not a ball.
        let speed = d.velocity.length();
        if speed > 0.01 {
            let dir = d.velocity / speed;
            let stretch = (1.0 + speed * 0.12).min(3.5);
            tf.rotation = Quat::from_rotation_arc(Vec3::Y, dir);
            tf.scale = Vec3::new(d.size * 2.0, d.size * 2.0 * stretch, d.size * 2.0);
        }

        // Floor contact.
        if tf.translation.y <= d.size {
            if d.size > settings.droplet_size * 0.75 {
                *seed = seed.wrapping_add(1);
                stamp_droplet_splat(
                    &mut commands,
                    &assets,
                    &mut pool_mats,
                    &settings,
                    &dungeon,
                    &mut ring,
                    Vec3::new(tf.translation.x, 0.0, tf.translation.z),
                    None,
                    now,
                    *seed as f32 * 0.618,
                );
            }
            commands.entity(entity).despawn();
            continue;
        }

        // Horizontal move + wall contact.
        let hstep = Vec3::new(d.velocity.x * dt, 0.0, d.velocity.z * dt);
        if hstep.x != 0.0 || hstep.z != 0.0 {
            let resolved = dungeon.resolve_move(tf.translation, hstep, Vec2::splat(d.size.max(0.02)));
            let blocked_x = (resolved.x - (tf.translation.x + hstep.x)).abs() > 1e-4;
            let blocked_z = (resolved.z - (tf.translation.z + hstep.z)).abs() > 1e-4;
            if blocked_x || blocked_z {
                if d.size > settings.droplet_size * 0.6 {
                    *seed = seed.wrapping_add(1);
                    let normal = if blocked_x {
                        Vec3::new(-d.velocity.x.signum(), 0.0, 0.0)
                    } else {
                        Vec3::new(0.0, 0.0, -d.velocity.z.signum())
                    };
                    let at = Vec3::new(resolved.x, tf.translation.y.clamp(0.12, 0.9), resolved.z);
                    stamp_droplet_splat(
                        &mut commands,
                        &assets,
                        &mut pool_mats,
                        &settings,
                        &dungeon,
                        &mut ring,
                        at,
                        Some(normal),
                        now,
                        *seed as f32 * 0.618,
                    );
                }
                commands.entity(entity).despawn();
                continue;
            }
            tf.translation.x = resolved.x;
            tf.translation.z = resolved.z;
        }
    }
}

/// Keep the number of permanent floor stains bounded: recycle the oldest once over the cap.
fn cap_blood_pools(mut commands: Commands, settings: Res<GoreSettings>, mut ring: ResMut<PoolRing>) {
    while ring.0.len() > settings.max_pools {
        if let Some(old) = ring.0.pop_front() {
            // Only this system despawns pools (they are otherwise permanent) and each id is popped
            // exactly once, so the entity is always still alive here.
            commands.entity(old).despawn();
        }
    }
}

/// Keep the number of physics gib chunks bounded: recycle the oldest *idle* chunk once over the cap.
/// This is what stops the rigid-body count (and the pile of viscera) from growing without limit.
///
/// Class-aware recycling (cf. Weber, Mateas & Jhala, "A Particle Model for State Estimation in
/// Real-Time Strategy Games", AIIDE 2011 — age particles out *by class*, never blindly, so in-use
/// state isn't discarded). Off-screen kills legitimately push chunks into this shared ring (they feed
/// nests the squad can't see — see the spawn note above), so the ring can overflow with fresh viscera
/// while real haul jobs are in flight. A blind FIFO pop could then evict a meat chunk a crew is
/// mid-haul to a nest (it vanishes from under the carriers, so the nest is never fed) or one a crew is
/// gathering to lift. So protect chunks actively in the crab economy (`Hauling`/`Crewing`) and recycle
/// only idle ones — `Resting`, or pure decoration with no `Carryable` (guns) — oldest first.
fn cap_gib_chunks(
    mut commands: Commands,
    settings: Res<GoreSettings>,
    mut ring: ResMut<GibRing>,
    carry: Query<&Carryable>,
) {
    // Scan from the oldest end; skip in-economy chunks, despawn the first recyclable one, repeat. The
    // ring is small (cap ~200) and this only does work when over cap, so the O(n) `remove` is cheap.
    // If every over-cap chunk is in-flight (pathological), we briefly exceed the cap rather than yank a
    // live haul — the excess drains as soon as those chunks deliver (`GibRing::consume`) or time out.
    let mut idx = 0;
    while ring.0.len() > settings.max_gibs && idx < ring.0.len() {
        let e = ring.0[idx];
        let in_economy = carry
            .get(e)
            .is_ok_and(|c| matches!(c.phase, CarryPhase::Hauling | CarryPhase::Crewing));
        if in_economy {
            idx += 1; // protected — try the next-oldest
            continue;
        }
        // Only this system despawns idle gib chunks (otherwise permanent) and each id appears once, so
        // the entity is always still alive here. Don't advance `idx`: the next entry shifts into it.
        ring.0.remove(idx);
        commands.entity(e).despawn();
    }
}

fn despawn_gore(mut commands: Commands, time: Res<Time>, fx: Query<(Entity, &GoreFx)>) {
    let now = time.elapsed_secs();
    for (entity, f) in &fx {
        if now >= f.despawn_at {
            commands.entity(entity).despawn();
        }
    }
}

/// Validate an already-deserialized [`GoreSettings`]. Called by the unified config loader
/// (`crate::config::load_game_config`) on the `gore:` slice of the master `GameConfig` — one path, no
/// fallback. `bake_autogib` feeds these straight into `i32::clamp(min, max)`, which panics when
/// `min > max`; reject an inverted pair loudly at the door instead of crashing later mid-combat.
pub fn validate_settings(settings: &GoreSettings) -> Result<(), String> {
    if settings.autogib_min_pieces > settings.autogib_max_pieces {
        return Err(format!(
            "gore: autogib_min_pieces ({}) > autogib_max_pieces ({}) — fix the RON",
            settings.autogib_min_pieces, settings.autogib_max_pieces
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{pool_scale, POOL_SCALE_MAX, POOL_SCALE_MIN};

    #[test]
    fn pool_scale_grows_with_mass_and_stays_bounded() {
        // A featherweight crab (intensity floor 0.03) still leaves a visible mark, but a small one.
        let crab = pool_scale(0.03, None);
        assert!(crab > 0.0, "crab pool must not vanish, got {crab}");
        assert!(crab < 0.6, "crab pool should stay small, got {crab}");

        // The boss (intensity 1.0) hits the ceiling.
        let boss = pool_scale(1.0, None);
        assert!((boss - POOL_SCALE_MAX).abs() < 1e-6, "boss should be MAX, got {boss}");

        // Monotonic in intensity.
        assert!(pool_scale(0.1, None) < pool_scale(0.5, None));
        assert!(pool_scale(0.5, None) < pool_scale(0.9, None));

        // Always clamped to [MIN, MAX], even with an extreme gib scale.
        for &(i, g) in &[(0.0, Some(0.0)), (1.0, Some(10.0)), (0.5, Some(-5.0))] {
            let f = pool_scale(i, g);
            assert!(
                (POOL_SCALE_MIN..=POOL_SCALE_MAX).contains(&f),
                "factor {f} out of [{POOL_SCALE_MIN}, {POOL_SCALE_MAX}] for intensity {i}, gib {g:?}"
            );
        }

        // A bigger figurine widens its own pool vs the standard scale=1.0.
        assert!(pool_scale(0.5, Some(1.5)) > pool_scale(0.5, Some(1.0)));
    }
}
