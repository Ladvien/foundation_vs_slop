//! Dimensional-crab swarm — a wall-climbing infestation enemy.
//!
//! Where the smiley boss (`crate::enemy`) is one slow bullet-sponge, the crabs are a ~40-strong swarm:
//! individually weak (a shot or two kills one) but lethal when several pile onto a unit. Their defining
//! trait is that they navigate the dungeon's *walls* as freely as its floors — mounting a wall from the
//! floor, crawling up the face and along the run, rounding corners, and dropping back down — chasing the
//! squad across the 2.5D surface manifold built by `crate::surface_nav`.
//!
//! Locomotion mirrors `enemy::enemy_seek`'s snapshot-then-move shape, but on surfaces: one `Arc`-shared
//! surface flow field (rebuilt only when the squad crosses cell boundaries) tells every crab which
//! neighbouring patch descends toward the nearest unit; the crab steers along its current surface's
//! tangent plane toward that patch's gate, transfers patch when it reaches the boundary, and re-orients
//! flat to the new surface (the `Quat::from_rotation_arc` trick `gore` uses for wall decals). Reynolds
//! separation (Reynolds, "Steering Behaviors For Autonomous Characters", GDC 1999) keeps the swarm from
//! collapsing to a point; it is evaluated through a per-frame spatial hash so 40 agents stay O(n·k), not
//! O(n²) (the smiley's pairwise loop is fine for one enemy, not a swarm).
//!
//! Entity shape (root scale 1 so the raycast collider keeps world size; the model is a scaled child):
//!   root  = `Crab` + `Hostile` + `Health` + `CrabMotion`/`CrabState` + a material-less `Sphere` `Mesh3d`
//!           (the invisible laser hit target, exactly like the smiley capsule).
//!   child = `WorldAssetRoot` of `dimensional_crab.glb#Scene0`, scaled to ~0.5 m and seated on the body.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use avian3d::prelude::{AngularVelocity, LinearVelocity, RigidBody};
use bevy::prelude::*;

use crate::audio::Sfx;
use crate::dungeon::Dungeon;
use crate::enemy::Hostile;
use crate::gore::{GoreEvent, GoreKind, GoreQueue};
use crate::health::{Biological, Health, NoHealthBar};
use crate::behavior_tuning::{BehaviorTuning, CrabTuning};
use crate::sim::SimTuning;
use crate::squad::{Prey, Unit};
use crate::surface_nav::{clamp_to_patch, project_tangent, surface_orientation, SurfaceField, SurfaceGraph};
use crate::util::{hash01_u32, rand01, unit_is_facing};

/// Total crabs across the level, split into `CRAB_CLUSTERS` nests in far rooms.
const CRAB_COUNT: usize = 40;
const CRAB_CLUSTERS: usize = 4;
/// The first this-many clusters are seeded directly on wall faces, so wall-climbing is always visible
/// (the rest start on the floor and mount walls opportunistically as the field pulls them).
const CRAB_WALL_CLUSTERS: usize = 2;
/// Nests spawn at least this far (tiles) from the squad spawn, and clusters at least this far apart.
const CRAB_MIN_SPAWN_DIST: f32 = 8.0;
const CRAB_CLUSTER_SEP: f32 = 5.0;

// Crab locomotion, boids, pounce, scout, feeding, and caste BEHAVIOUR constants moved to the `behavior:`
// config slice (`behavior.crab`, src/behavior_tuning.rs) so they are hand-tunable and searchable by
// `squad_ai::behavior_genome`. Only the render/collider/body-geometry constants stay here in code.

/// Uniform render scale for the child model (native height ~3.06 → ~0.46 m ≈ 1.5 ft tall, sized to
/// the ~6 ft squad and 8 ft ceilings). Seat constants below scale with it.
const CRAB_RENDER_SCALE: f32 = 0.15;
/// Root body-centre height above the surface, along the surface normal (also seats the collider).
const CRAB_BODY_CENTER: f32 = 0.125;
/// Local Y offset of the scaled model under the root so its body rests on the surface (the glb origin
/// sits near the model's top). Calibrated by eye via devshot, scaled with `CRAB_RENDER_SCALE`.
const CRAB_MODEL_Y: f32 = 0.275;
/// Radius of the invisible collider sphere (the laser raycast target); world-size since the root is
/// unscaled. Sized to hug the *visible* crab (rendered span ≈0.46 → radius ≈0.3) so a bolt only draws
/// blood on a real hit — a near-miss now passes cleanly instead of registering on an oversized hitbox.
/// Scales in lockstep with `CRAB_RENDER_SCALE` (2.5× when the model grew 0.06→0.15).
pub(crate) const CRAB_COLLIDER_R: f32 = 0.30;

/// The unit's body approximated as a vertical cylinder the crabs cling to (radius, climbable height).
const UNIT_BODY_RADIUS: f32 = 0.33;
const UNIT_BODY_HEIGHT: f32 = 1.0;
/// Reach the flow gate this close to commit the patch transfer.
const TRANSFER_RADIUS: f32 = 0.22;
/// How fast the crab's surface normal eases toward the new patch's normal on a transfer (per second).
const NORMAL_EASE: f32 = 12.0;
/// Frame-dt clamp so a hitch can't fling a crab off its surface (mirrors `enemy::MAX_FRAME_DT`).
const MAX_FRAME_DT: f32 = 1.0 / 30.0;
/// Cross-fade between animation clips.
const CROSSFADE: Duration = Duration::from_millis(150);
/// Clip playback-rate multipliers. The authored clips are extremely long (walk ≈ 10.5 s/loop, attack
/// ≈ 2.5 s), so at 1× the legs crawl through one cycle over many seconds — playing them several times
/// faster turns it into a frantic scuttle / rapid chomp. Tuned by eye.
const WALK_ANIM_SPEED: f32 = 7.0;
const ATTACK_ANIM_SPEED: f32 = 4.0;

const CRAB_GLB: &str = "dimensional_crab/dimensional_crab.glb";

/// Marker on a crab root entity (also the raycast collider).
#[derive(Component)]
pub struct Crab;

/// A crab's position on the surface manifold and its facing/seed state.
#[derive(Component)]
struct CrabMotion {
    /// Current [`SurfaceGraph`] patch id.
    patch: u32,
    /// World-space point ON the current surface (pre-seat).
    pos: Vec3,
    /// Current surface normal (eased toward the patch normal across transfers).
    normal: Vec3,
    /// Last travel heading (for smooth facing).
    heading: Vec3,
    /// Per-crab preferred height fraction `[0,1]` on a unit's body when latched, so the swarm spreads
    /// over the whole body (piranha) instead of piling at the feet.
    climb_bias: f32,
    /// Per-crab preferred angular slot `[0,1]` around the body (mapped across `bc.back_spread`).
    angle_bias: f32,
    /// Body-relative angle (radians) of the crab's slot around its host, `0` = dead-centre back. Set
    /// once on latching so the crab holds that spot and rides along as the host turns and walks.
    latch_rel: f32,
}

/// Public link from a latched crab to the unit it's feeding on (`None` when free-roaming). Read by the
/// laser so a bolt that shoots this crab off can roll to also wound its host (friendly fire).
#[derive(Component)]
pub struct CrabAttached {
    pub host: Option<Entity>,
}

/// A crab's foraging/carrying state: how much it can lift, the specific gib it's committed to (a
/// `Vec3`-only `ActiveBehavior.target` can't hold an entity), and whether its gib is currently lifted.
#[derive(Component)]
pub struct CrabCarry {
    pub capacity: f32,
    pub target: Option<Entity>,
    pub hauling: bool,
}

/// Animation state, chosen from movement/contact each frame.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum CrabState {
    Idle,
    Walk,
    Attack,
}

/// A crab's pounce state: crabs lunge ~`bc.jump_len` (≈10 body lengths) at a nearby unit, hunkering down
/// briefly before launching on a ballistic arc that bites on landing. `Ready` = grounded (normal
/// locomotion runs); `Hunker`/`Air` = the jump owns the crab's transform, so `crab_locomotion` skips it.
#[derive(Component)]
struct CrabJump {
    phase: JumpPhase,
    /// Time left in the current `Hunker`/`Air` phase.
    timer: f32,
    /// Cooldown before the next pounce (counts down while `Ready`).
    cooldown: f32,
    from: Vec3,
    to: Vec3,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JumpPhase {
    Ready,
    Hunker,
    Air,
}

/// A scout's recon state machine: roam hunting for prey, then track a sighting and mark it with the
/// vectorial rally pheromone (Tang et al. 2019) so the assault swarm converges on the live prey.
#[derive(Clone, Copy)]
enum ScoutState {
    /// Ranging the map looking for prey (aggressive wander).
    Roaming,
    /// Tracking spotted prey — approaching `prey_pos` (refreshed each frame it still senses the prey) and
    /// depositing rally-pheromone vectors that point toward it.
    Tracking { prey_pos: Vec3 },
}

/// Marks the ~[`bc.scout_fraction`] of crabs that are scouts (recon recruiters). Holds the roam/track state
/// and this scout's private wander heading + RNG. Models ant scout-recruitment foraging by minimalist
/// agents (Talamali et al., Swarm Intelligence 2019, DOI 10.1007/s11721-019-00176-9): roam → spot →
/// track-and-mark → the swarm converges
/// via the vectorial rally pheromone (Tang et al. 2019). Read by `think` (to drive the Scout/Mark modes)
/// and `scout_mark_prey` (detection + rally deposit); its wander state is advanced by `crab_locomotion`.
#[derive(Component)]
pub struct Scout {
    state: ScoutState,
    /// Current roam heading (world dir, re-rolled on `wander_timer`); mirrors `EnemyMotion.wander_dir`.
    wander_dir: Vec3,
    wander_timer: f32,
    /// Throttle between successive rally-pheromone deposits (seconds). Keeps a marking scout from
    /// saturating a cell every frame while it tracks the prey; the pheromone's own decay handles call-off.
    report_cooldown: f32,
    /// Per-scout LCG state for heading re-rolls (seeded from spawn hash so scouts diverge).
    rng: u32,
}

impl Scout {
    fn new(rand_seed: u32) -> Self {
        Self {
            state: ScoutState::Roaming,
            wander_dir: Vec3::ZERO,
            wander_timer: 0.0,
            report_cooldown: 0.0,
            // Salt distinct from the spawn-bundle draws so a scout's roam RNG is independent of its role
            // draw (see `CrabSpawnSeq`); `| 1` keeps the LCG state odd/non-zero.
            rng: (hash01_u32(rand_seed.wrapping_mul(0x9E37_79B1).wrapping_add(11)) * 4_000_000.0) as u32
                | 1,
        }
    }

    /// 1.0-equivalent gate for the brain: this scout is tracking a live sighting (latches the Mark mode).
    pub fn prey_spotted(&self) -> bool {
        matches!(self.state, ScoutState::Tracking { .. })
    }

    /// The prey a marking scout is tracking (the Mark behaviour's aim point — it approaches this to keep
    /// the rally pheromone fresh toward the prey's current position).
    pub fn tracked_prey(&self) -> Option<Vec3> {
        match self.state {
            ScoutState::Tracking { prey_pos } => Some(prey_pos),
            ScoutState::Roaming => None,
        }
    }
}

/// Link from a crab to its (asynchronously-spawned) `AnimationPlayer`, plus which state's clip is
/// currently playing (so `drive_crab_animation` only re-triggers on a real change).
#[derive(Component)]
struct CrabAnimPlayer {
    player: Entity,
    playing: Option<CrabState>,
}

/// The shared surface pursuit field (analog of `enemy::EnemyField`): rebuilt only when the set of unit
/// cells changes, shared read-only by the whole swarm.
#[derive(Resource, Default)]
struct CrabField {
    field: Option<Arc<SurfaceField>>,
    last_cells: Vec<IVec2>,
}

/// Monotonic spawn counter — a unique, ever-increasing seed handed to each crab at birth. Per-crab
/// randomization (scout/assault role, think-stagger, jump cadence, carry capacity, climb/angle biases,
/// RNG) is derived from THIS, never from the spawn *position*: nest-bred crabs all seat on the one
/// delivery cell, so a position hash would make every sibling a byte-identical clone (collapsing the
/// scout split to per-nest all-or-nothing). One counter, incremented once per spawn, keeps them distinct.
#[derive(Resource, Default)]
struct CrabSpawnSeq(u64);

/// The one shared animation graph + node handles for the three crab clips.
#[derive(Resource)]
struct CrabAnim {
    graph: Handle<AnimationGraph>,
    idle: AnimationNodeIndex,
    walk: AnimationNodeIndex,
    attack: AnimationNodeIndex,
}

pub struct CrabPlugin;

impl Plugin for CrabPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CrabField>()
            .init_resource::<CrabSpawnSeq>()
            // Graph and anim resources must exist before crabs are spawned/snapped to patches.
            .add_systems(Startup, (build_surface_graph, build_crab_anim, spawn_crabs).chain())
            // Pinned crab simulation on `FixedUpdate` — locomotion, jumping, carry economy, combat,
            // deposits, and reproduction. All the `.after(AiSet::…)` / inter-system orderings stay valid
            // because `AiSet` and every one of these systems now live on `FixedUpdate` together.
            .add_systems(
                FixedUpdate,
                (
                    rebuild_crab_field,
                    // Enlist foraging crabs onto specific gibs before locomotion reads their targets.
                    assign_meat_targets
                        .after(crate::ai::AiSet::Think)
                        .before(crab_locomotion),
                    // A crab that left SeekMeat/Carry drops its load before the carry machine
                    // re-evaluates the crew (fleeing, latching, or re-foraging all release).
                    release_uncommitted_carriers
                        .after(crate::ai::AiSet::Think)
                        .before(carry_gibs),
                    // Move after the brain has chosen this tick's mode (see `crate::ai`).
                    crab_locomotion
                        .after(rebuild_crab_field)
                        .after(crate::ai::AiSet::Think)
                        // Read the light field the tick it was baked (mirrors the `fog::LosWritten` gate).
                        .after(crate::light::LightFieldWritten)
                        // Likewise read the Almond Water field the tick it was updated + drunk down, so the
                        // forage nudge steers on the current level (post-heal), not last tick's.
                        .after(crate::almond_water::AlmondWaterWritten),
                    // Pounce owns the transform of mid-jump crabs; runs after locomotion set the rest.
                    crab_jump.after(crab_locomotion).in_set(crate::health::HealthDamage),
                    // Cooperative lift/haul/deliver — runs after crabs have moved and any fleer released.
                    carry_gibs
                        .after(crab_locomotion)
                        .after(assign_meat_targets),
                    crab_contact_damage.in_set(crate::health::HealthDamage),
                    // Flood the local ALARM channel when a crab is wounded, before the deposits drain so
                    // the muster bloom is live this tick (mirrors `scout_mark_prey`'s ordering).
                    crab_alarm_on_damage.before(crate::ai::AiSet::Deposits),
                    // Sate HUNGER after the brain has consumed this tick's drive values.
                    crab_feeding_sates_hunger.after(crate::ai::AiSet::Think),
                    crab_despawn_dead.in_set(CrabDespawn),
                    deposit_crab_fields,
                    deposit_meat_scent,
                    // Scout detection + rally-pheromone deposit: before the rally deposits drain so the
                    // beacon is live this tick, and before Think so the tracking state feeds the scout brain.
                    scout_mark_prey.before(crate::ai::AiSet::Deposits),
                    // Dynamic castes: re-role scouts↔assault off the fresh fields, before the brains think.
                    re_role_crabs
                        .after(crate::ai::AiSet::FieldUpdate)
                        .before(crate::ai::AiSet::Think),
                    nest_reproduce,
                ),
            )
            // Cosmetic: skeletal animation attach/drive stays on `Update`.
            .add_systems(Update, (attach_crab_animation, drive_crab_animation));
    }
}

fn build_surface_graph(mut commands: Commands, dungeon: Res<Dungeon>) {
    let graph = SurfaceGraph::build(&dungeon);
    let (floor, wall) = graph.patch_stats();
    info!(
        "crab: surface graph built — {} patches ({} floor, {} wall)",
        graph.len(),
        floor,
        wall
    );
    commands.insert_resource(graph);
}

fn build_crab_anim(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    // glb clips: 0 = attack, 1 = idle, 2 = walk.
    let (graph, nodes) = AnimationGraph::from_clips([
        assets.load(GltfAssetLabel::Animation(0).from_asset(CRAB_GLB)),
        assets.load(GltfAssetLabel::Animation(1).from_asset(CRAB_GLB)),
        assets.load(GltfAssetLabel::Animation(2).from_asset(CRAB_GLB)),
    ]);
    let handle = graphs.add(graph);
    commands.insert_resource(CrabAnim {
        graph: handle,
        attack: nodes[0],
        idle: nodes[1],
        walk: nodes[2],
    });
}

/// Place `CRAB_CLUSTERS` nests in far rooms and fill each with crabs; the first `CRAB_WALL_CLUSTERS`
/// seed their crabs onto wall faces so climbing is visible from the start.
fn spawn_crabs(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    graph: Res<SurfaceGraph>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut nest_mats: ResMut<Assets<crate::nest::NestMaterial>>,
    mut seq: ResMut<CrabSpawnSeq>,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let collider = meshes.add(Sphere::new(CRAB_COLLIDER_R));
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(CRAB_GLB));
    let dome = meshes.add(crate::nest::nest_dome_mesh()); // shared unit hemisphere → wall pimple per nest
    // Keep the shared handles so the reproduction system can birth new crabs at runtime.
    commands.insert_resource(CrabAssets {
        collider: collider.clone(),
        scene: scene.clone(),
    });

    // Greedily pick far, spread-apart nest seeds (deterministic, like `enemy::spawn_enemies`).
    let mut seeds: Vec<IVec2> = Vec::new();
    'scan: for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }
            if (cell - dungeon.spawn).as_vec2().length() < CRAB_MIN_SPAWN_DIST {
                continue;
            }
            if seeds
                .iter()
                .any(|c| (*c - cell).as_vec2().length() < CRAB_CLUSTER_SEP)
            {
                continue;
            }
            seeds.push(cell);
            if seeds.len() >= CRAB_CLUSTERS {
                break 'scan;
            }
        }
    }
    if seeds.is_empty() {
        warn!("crab: no floor cell far enough from spawn to place a nest");
        return;
    }

    // A dimensional nest portal near each cluster seed — the crabs' home + meat-delivery + birth anchor.
    // The dome sits ON a wall (bulging into the room); the delivery cell is the walled floor cell it
    // hangs over. Search the seed, then rings outward, for the nearest walled floor cell to seat it on.
    for &seed in &seeds {
        let mut placed = false;
        'search: for radius in 0i32..4 {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx.abs().max(dy.abs()) != radius {
                        continue; // only the shell of this ring (inner rings already tried)
                    }
                    let cell = seed + IVec2::new(dx, dy);
                    if !dungeon.is_floor(cell) {
                        continue;
                    }
                    let center = dungeon.cell_center(cell);
                    // Seat the nest only on a full-height wall. The camera-facing E/S edges are knee
                    // walls (squashed to `CAMERA_WALL_FRACTION`; their inner faces point -X / -Z — see
                    // `Dungeon::wall_faces_near`), and a dome seated mid-`WALL_HEIGHT` on one would
                    // float in the cutaway gap above the short wall. Prefer W/N faces (normals +X/+Z);
                    // a cell with only knee walls is skipped and the ring search moves on.
                    let full_face = dungeon.wall_faces_near(center).into_iter().find(|&(_, n)| {
                        !crate::dungeon::SHORT_CAMERA_WALLS || !crate::dungeon::is_camera_facing(n)
                    });
                    if let Some((face, normal)) = full_face {
                        if crate::nest::spawn_nest(
                            &mut commands,
                            &mut nest_mats,
                            dome.clone(),
                            face,
                            normal,
                            center,
                            &dungeon,
                        )
                        .is_some()
                        {
                            placed = true;
                            break 'search;
                        }
                    }
                }
            }
        }
        if !placed {
            warn!("crab: no wall face near cluster seed {seed:?} to seat a nest");
        }
    }

    let per_cluster = CRAB_COUNT.div_ceil(seeds.len());
    let ring = nest_offsets();
    let mut spawned = 0usize;

    for (ci, &seed) in seeds.iter().enumerate() {
        let on_wall = ci < CRAB_WALL_CLUSTERS;
        let mut in_cluster = 0usize;
        for &(dx, dy) in ring.iter() {
            if spawned >= CRAB_COUNT || in_cluster >= per_cluster {
                break;
            }
            let cell = seed + IVec2::new(dx, dy);
            if let Some(patch) = pick_patch(&graph, &dungeon, cell, on_wall) {
                let s = seq.0 as u32;
                seq.0 += 1;
                spawn_crab_on_patch(&mut commands, &graph, patch, &collider, &scene, s, &sim, beh.crab);
                spawned += 1;
                in_cluster += 1;
            }
        }
    }

    info!("crab: spawned {} crabs across {} nests", spawned, seeds.len());
}

/// Choose the patch a crab spawns on for `cell`: a wall face if `want_wall` and one exists, else the
/// cell's floor patch. Returns `None` if the cell is not a usable surface.
fn pick_patch(
    graph: &SurfaceGraph,
    dungeon: &Dungeon,
    cell: IVec2,
    want_wall: bool,
) -> Option<u32> {
    if !dungeon.is_floor(cell) {
        return None;
    }
    if want_wall {
        let center = dungeon.cell_center(cell);
        graph
            .wall_patch_at(dungeon, center)
            .or_else(|| graph.floor_patch_cell(cell))
    } else {
        graph.floor_patch_cell(cell)
    }
}

/// Spawn one crab seated on `patch`: an unscaled root (invisible collider + `Hostile`) with the scaled,
/// seated glTF model as a child.
fn spawn_crab_on_patch(
    commands: &mut Commands,
    graph: &SurfaceGraph,
    patch: u32,
    collider: &Handle<Mesh>,
    scene: &Handle<WorldAsset>,
    rand_seed: u32,
    sim: &SimTuning,
    bc: CrabTuning,
) {
    let p = graph.patch(patch);
    let pos = p.center;
    let normal = p.normal;
    let heading = p.tan_u;
    let seat = pos + normal * CRAB_BODY_CENTER;

    // Every per-crab random draw comes from the unique spawn seed, NOT the spawn position — bred crabs
    // share a delivery cell, so a position hash would clone them (see `CrabSpawnSeq`). Distinct salts
    // decorrelate the independent draws (role, capacity, jump cadence, biases).
    let draw = |salt: u32| hash01_u32(rand_seed.wrapping_mul(0x9E37_79B1).wrapping_add(salt));

    // ~bc.scout_fraction of crabs are scouts (recon recruiters); the rest run the assault brain. One path —
    // a plain conditional, no fallback.
    let is_scout = draw(1) < bc.scout_fraction;
    let brain_id = if is_scout {
        crate::ai::brain::BrainId::Scout
    } else {
        crate::ai::brain::BrainId::Crab
    };

    let mut ec = commands.spawn((
            Crab,
            Hostile,
            Health::new(sim.combat.crab_hp),
            NoHealthBar, // swarm chaff: no floating bar (40 would bury the screen)
            // Seed a per-crab starting HUNGER (salt 6, decorrelated from the other draws) so the swarm
            // begins differentiated — hungry crabs press to feed, sated ones forage — instead of a uniform
            // ramp where every crab hits HUNGER==1 in lockstep. Feeding sates it (`crab_feeding_sates_hunger`).
            crate::ai::drives::Drives::seeded(crate::ai::drives::DriveId::HUNGER, 0.2 + 0.6 * draw(6)),
            // Fear the squad's gunfire, never the swarm's own menace. Tagged here rather than at the two
            // call sites so runtime-bred crabs (`nest_reproduce`) inherit it too.
            crate::ai::faction::Faction::Crab,
            brain_id,
            crate::ai::brain::ActiveBehavior::new(rand_seed),
            crate::ai::brain::ThinkTimer::staggered(rand_seed),
            // Grouped so the spawn tuple stays within Bevy's 15-element Bundle limit. `Biological` rides
            // here (not as a 16th top-level element) — living flesh Almond Water can heal; tagged at spawn
            // so runtime-bred crabs (`nest_reproduce`) inherit it and no runtime archetype migration occurs.
            (
                Biological,
                // ~1 in 4 crabs can't smell the cyanide warning → walk into poison pools. On every crab.
                crate::health::CyanideSmell::from_seed(rand_seed as u64),
                CrabAttached { host: None },
                CrabCarry {
                    capacity: bc.carry_capacity * (0.8 + 0.4 * draw(2)),
                    target: None,
                    hauling: false,
                },
                CrabJump {
                    phase: JumpPhase::Ready,
                    timer: 0.0,
                    // Stagger initial cooldowns by seed so a fresh cluster doesn't pounce in lockstep.
                    cooldown: bc.jump_cooldown * draw(3),
                    from: Vec3::ZERO,
                    to: Vec3::ZERO,
                },
            ),
            CrabMotion {
                patch,
                pos,
                normal,
                heading,
                climb_bias: draw(4),
                angle_bias: draw(5),
                latch_rel: 0.0,
            },
            CrabState::Idle,
            // Sphere collider mesh paired with its CPU laser hit-volume (same radius, sphere = zero-height
            // capsule) so bolts test against the crab headlessly + deterministically.
            (
                Mesh3d(collider.clone()),
                crate::laser::LaserTarget {
                    radius: CRAB_COLLIDER_R,
                    half_height: 0.0,
                    id: crate::laser::target_id(crate::laser::TargetKind::Crab, rand_seed as u64),
                },
            ),
            // Render-only: smooth the crab's 60 Hz movement + surface rotation across the display refresh
            // (see `lib::run`). Grouped with `Transform` so the spawn tuple stays within Bevy's 15-element
            // Bundle limit.
            (
                Transform::from_translation(seat).with_rotation(surface_orientation(heading, normal)),
                // Component + plugin come from avian's `bevy_transform_interpolation` integration.
                avian3d::prelude::TransformInterpolation,
            ),
            Visibility::Inherited,
        ));
    ec.with_child((
        WorldAssetRoot(scene.clone()),
        Transform::from_translation(Vec3::Y * CRAB_MODEL_Y).with_scale(Vec3::splat(CRAB_RENDER_SCALE)),
    ));
    // Caste hysteresis timer + the immortal spawn seed, so `re_role_crabs` can flip this crab's role
    // deterministically as the swarm's needs shift (see that system's determinism note).
    ec.insert((Caste { cooldown: 0.0 }, CrabSeed(rand_seed)));
    // Crabs are photophobic — they steer down the `LightField` gradient toward shadow (see
    // `crab_locomotion` and `light::Photophobic`), so lit rooms become refuges and the swarm pools in the
    // dark. Added at spawn (stable archetype; `re_role_crabs` never touches it), so light response is a
    // fixed trait of the creature and can't churn the hashed sim actor's archetype at runtime.
    ec.insert(crate::light::Photophobic);
    // SCP-150 host state: a crab is also a parasitizable host (the three-body web — parasite ↔ crab ↔
    // squad). Always-present + inert until infested, added here so `nest_reproduce`'s bred crabs inherit
    // it too; a flipped field never splits the hashed crab archetype.
    ec.insert(crate::parasite::host_infestation_bundle());
    if is_scout {
        ec.insert(Scout::new(rand_seed));
    }
}

/// Cell offsets around a nest seed, sorted nearest-first, out to Chebyshev radius 3 (~49 cells) so a
/// cluster can fill even in a cramped room.
fn nest_offsets() -> Vec<(i32, i32)> {
    let mut v: Vec<(i32, i32)> = Vec::new();
    for dy in -3..=3 {
        for dx in -3..=3 {
            v.push((dx, dy));
        }
    }
    // SORT-OK: a fixed constant offset table, not an ECS query — the input order is source-code order.
    v.sort_by_key(|&(dx, dy)| dx * dx + dy * dy);
    v
}

/// Rebuild the shared surface field when the set of unit cells changes (copies
/// `enemy::rebuild_enemy_field`'s gate, over the surface graph).
fn rebuild_crab_field(
    graph: Option<Res<SurfaceGraph>>,
    units: Query<&Transform, With<Prey>>,
    dungeon: Res<Dungeon>,
    mut crab_field: ResMut<CrabField>,
) {
    let Some(graph) = graph else { return };

    let crab_field = &mut *crab_field;
    // `force` when the field isn't built yet, so a first run (or a graph that wasn't ready) still builds
    // even though the unit cells haven't moved — matches the old `&& field.is_some()` skip guard.
    let force = crab_field.field.is_none();
    crate::pathfind::rebuild_on_cell_change(
        units.iter().map(|t| dungeon.world_to_cell(t.translation)),
        &mut crab_field.last_cells,
        force,
        |cells| {
            let sources: Vec<u32> = cells
                .iter()
                .filter_map(|&c| graph.floor_patch_cell(c))
                .collect();
            crab_field.field = SurfaceField::build(&graph, &sources).map(Arc::new);
        },
    );
}

/// Move every crab one step along the surface toward the nearest unit, transferring between patches and
/// re-orienting flat to each new surface.
fn crab_locomotion(
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    crab_field: Res<CrabField>,
    dungeon: Res<Dungeon>,
    stig: Res<crate::ai::field::Stig>,
    rally: Res<crate::ai::field::RallyField>,
    // The gameplay light field (baked in `light::LightFieldWritten`, ordered before this system) and the
    // config gains — for the photophobic/-philic light nudge below.
    light_field: Res<crate::light::LightField>,
    // The Almond Water field (written in `AlmondWaterWritten`, ordered before this system) — for the
    // wounded-forage nudge below (a wounded crab climbs the water gradient toward a seep).
    almond_water: Res<crate::almond_water::AlmondWater>,
    config: Res<crate::config::GameConfig>,
    units: Query<(Entity, &Transform), (With<Prey>, Without<Crab>)>,
    // Gib transforms, for a `SeekMeat`/`Carry` crab to steer to the specific chunk it's committed to.
    gibs: Query<&Transform, (With<crate::gore::GibChunk>, Without<Crab>, Without<Unit>)>,
    mut crabs: Query<
        (
            &mut CrabMotion,
            &mut CrabState,
            &mut CrabAttached,
            &mut Transform,
            &crate::ai::brain::ActiveBehavior,
            Option<&CrabCarry>,
            Option<&CrabJump>,
            Option<&mut Scout>,
            // Light response (`light::Photophobic`/`Photophilic`) — added at spawn; drives the light nudge.
            Option<&crate::light::Photophobic>,
            Option<&crate::light::Photophilic>,
            // Health gates the Almond Water forage nudge — only a wounded crab seeks the water.
            &Health,
            // Whether this crab can smell the cyanide warning — an anosmic crab can't tell a poison pool from
            // a heal pool, so it forages toward any water (and walks into cyanide).
            &crate::health::CyanideSmell,
        ),
        With<Crab>,
    >,
    // Reused across frames: a fresh HashMap + a Vec per occupied cell every frame (40-90 crabs on the
    // hottest per-crab path) churned dozens of small allocations. Held in a Local and cleared in place
    // (keys + Vec capacities retained, bounded by the fixed dungeon), so steady state is allocation-free.
    mut hash: Local<HashMap<IVec2, Vec<Vec3>>>,
) {
    let bc = config.behavior.crab;
    let Some(graph) = graph else { return };
    let Some(field) = crab_field.field.as_ref() else {
        return;
    };
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    let now = time.elapsed_secs(); // for per-crab path jitter (see CRAB_JITTER_*)

    // Per-unit: entity, foot position, and forward (local -Z) — its gun only reaches the front.
    let unit_data: Vec<(Entity, Vec3, Vec3)> = units
        .iter()
        .map(|(e, t)| {
            let fwd = (t.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
            (e, t.translation, fwd)
        })
        .collect();

    // Spatial hash of crab positions (3D) keyed by floor cell, for O(n·k) separation. Clear in place
    // (retain the buckets + each cell's Vec capacity) rather than reallocating the map every frame.
    for v in hash.values_mut() {
        v.clear();
    }
    for (motion, _, _, _, _, _, _, _, _, _, _, _) in &crabs {
        hash.entry(dungeon.world_to_cell(motion.pos))
            .or_default()
            .push(motion.pos);
    }
    // Sort each bucket by position bits so the separation SUM below (`sep += …`) is canonical. Float
    // addition is non-associative and the bucket is filled in crab QUERY order, which is NOT reproducible
    // across same-seed runs (documented at the carry logistics below, and see `util::nearest_planar`): an
    // unsorted bucket lets a query-order difference flip a rounding bit in `sep`, diverging a crab's
    // position and cascading into the physics-off replay hash (~1–3% of runs, pinned tick 549). Mirrors the
    // identical fix on the parasite swarm hash (`parasite.rs`, `manca_swarm`).
    for v in hash.values_mut() {
        // VALUE-CANONICAL: the bucket holds bare positions, so two coincident crabs contribute the
        // identical term to `sep` and their order cannot matter. (Contrast the drink/cull sorts, whose tied
        // elements carry identity.)
        crate::util::sort_value_canonical(v, |p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    }

    for (
        mut motion,
        mut state,
        mut attached,
        mut transform,
        active,
        carry,
        jump,
        mut scout,
        photophobic,
        photophilic,
        health,
        smell,
    ) in &mut crabs
    {
        // Mid-pounce crabs are owned by `crab_jump` (it drives their arc + transform) — skip them here.
        if jump.is_some_and(|j| j.phase != JumpPhase::Ready) {
            continue;
        }
        // Reynolds separation: raw 3D push away from nearby crabs (bounding-box spacing). Self is
        // skipped by the `d > eps` test; the per-cell hash keeps this O(n·k).
        let cell = dungeon.world_to_cell(motion.pos);
        let mut sep = Vec3::ZERO;
        for gy in -1..=1 {
            for gx in -1..=1 {
                if let Some(others) = hash.get(&(cell + IVec2::new(gx, gy))) {
                    for &o in others {
                        let away = motion.pos - o;
                        let d = away.length();
                        if d > 1.0e-4 && d < bc.sep_radius {
                            sep += away / d * (bc.sep_radius - d);
                        }
                    }
                }
            }
        }

        // Nearest unit on the ground plane (the brain decides *whether* to latch; this is *which* unit).
        // Payload carries the entity + precomputed forward vector; the shared ranking returns the winner.
        let nunit = crate::util::nearest_planar(
            motion.pos,
            unit_data.iter().map(|&(e, up, fwd)| ((e, fwd), up)),
        )
        .map(|((e, fwd), up, _d)| (e, up, fwd));
        let t = (NORMAL_EASE * dt).min(1.0);

        // The brain (see `crate::ai`) chose the mode; the surface/piranha *mechanics* below are
        // unchanged — Latch runs the piranha block, Flee is the new panic, everything else forages.
        let latching = matches!(active.mode, crate::ai::utility::Mode::Latch);
        let fleeing = matches!(active.mode, crate::ai::utility::Mode::Flee);
        // SeekMeat and Carry both steer to the crab's committed gib (Carry keeps formation while
        // `carry_gibs` drives the actual haul). The specific chunk lives in `CrabCarry.target`.
        let seeking = matches!(
            active.mode,
            crate::ai::utility::Mode::SeekMeat | crate::ai::utility::Mode::Carry
        );
        // Scout recon modes + the swarm's recruited rally (see `crate::ai::brain::scout_brain`).
        let scouting = matches!(active.mode, crate::ai::utility::Mode::Scout);
        let marking = matches!(active.mode, crate::ai::utility::Mode::Mark);
        let rallying = matches!(active.mode, crate::ai::utility::Mode::Rally);
        // Investigate: drawn toward the squad's audible din (`NOISE_SQUAD`) — steer to the noise hotspot
        // the brain aimed at (dormant unless the audio search turned it on; see `ai::brain::crab_brain`).
        let investigating = matches!(active.mode, crate::ai::utility::Mode::Investigate);
        // Muster: alarmed by a wounded neighbour — pursue the squad (same surface flow-field path as a
        // forage) but at a faster surge speed, so the retaliation reads as an aggressive charge.
        let mustering = matches!(active.mode, crate::ai::utility::Mode::Muster);
        let gib_pos = carry
            .and_then(|c| c.target)
            .and_then(|e| gibs.get(e).ok())
            .map(|t| t.translation);
        let want = if latching && let Some((host, u, fwd)) = nunit {
            // --- PIRANHA MODE: climb onto the unit and cover its body, biting from a free slot. ---
            {
                // On first latching (no host yet), claim a body-relative slot: fanned across the unit's
                // REAR (where the host's own forward-firing gun can't reach), spread by this crab's
                // `angle_bias`. Held thereafter, so the crab clings to that spot and rides along as the
                // host walks. `attached.host.is_none()` IS the "not yet latched" gate (host is the single
                // source of truth for latched-ness).
                if attached.host.is_none() {
                    motion.latch_rel = (motion.angle_bias - 0.5) * bc.back_spread;
                }
                attached.host = Some(host);

                // World cling direction = the unit's back rotated by the crab's body-relative slot.
                let back_angle = (-fwd.z).atan2(-fwd.x);
                let ang = back_angle + motion.latch_rel;
                let radial = Vec3::new(ang.cos(), 0.0, ang.sin());
                let slot_y = 0.1 + motion.climb_bias * (UNIT_BODY_HEIGHT - 0.1);
                let target = u + radial * UNIT_BODY_RADIUS + Vec3::Y * slot_y;

                let to = target - motion.pos;
                let move_vec = to.normalize_or_zero() * bc.climb_speed + sep * bc.sep_strength;
                motion.pos += move_vec * dt;
                motion.pos.y = motion.pos.y.max(0.0);

                // Keep the floor patch under the crab current, so it drops back into surface pathing
                // cleanly when this unit dies.
                if let Some(fp) = graph.floor_patch_cell(dungeon.world_to_cell(motion.pos)) {
                    motion.patch = fp;
                }

                // Cling flat to the body side (up = outward radial).
                motion.normal = motion.normal.lerp(radial, t).normalize_or(radial);
                let h = to.normalize_or(motion.heading);
                motion.heading = motion.heading.lerp(h, t).normalize_or(motion.heading);

                if to.length() < bc.eat_range {
                    CrabState::Attack
                } else {
                    CrabState::Walk
                }
            }
        } else if fleeing {
            // --- FLEE: panic away from the THREAT gradient (the frenzy→scatter payoff). ---
            attached.host = None;
            crab_flee(&mut motion, &stig, &dungeon, &graph, sep, dt, t, bc)
        } else if rallying {
            // --- RALLY: mass on the scout's marked sighting by following the local rally-pheromone
            // vector (Tang et al. 2019) — it already points at the (moving) prey, no gradient needed. ---
            attached.host = None;
            crab_rally(&mut motion, &rally, &dungeon, &graph, sep, dt, t, bc)
        } else if scouting && let Some(scout) = scout.as_deref_mut() {
            // --- SCOUT ROAM: aggressive wander across floor + walls hunting for prey to mark. ---
            attached.host = None;
            crab_scout_roam(&mut motion, scout, &graph, &dungeon, sep, dt, t, bc)
        } else if marking {
            // --- MARK: track the spotted prey — approach its position so the scout stays in sensing
            // range and `scout_mark_prey` keeps laying the rally pheromone toward its live cell. No
            // final-approach snap (home = None); falls back to holding heading if the sighting is gone. ---
            attached.host = None;
            let prey_pos = active.target;
            let desired = prey_pos.map(|p| p - motion.pos).unwrap_or(motion.heading);
            if steer_surface(&mut motion, &graph, &dungeon, desired, None, bc.speed * bc.scout_speed_mul, sep, dt, t, bc) {
                CrabState::Walk
            } else {
                CrabState::Idle
            }
        } else if investigating {
            // --- INVESTIGATE: steer toward the NOISE_SQUAD hotspot the brain aimed at — the swarm
            // converging on the sound of the guns. Same point-steering as Mark (no final-approach snap;
            // hold heading if the din is gone). Self-limiting: as the din evaporates the brain drops
            // Investigate and the crab reverts to foraging/fear. ---
            attached.host = None;
            let din_pos = active.target;
            let desired = din_pos.map(|p| p - motion.pos).unwrap_or(motion.heading);
            if steer_surface(&mut motion, &graph, &dungeon, desired, None, bc.speed, sep, dt, t, bc) {
                CrabState::Walk
            } else {
                CrabState::Idle
            }
        } else if seeking {
            // --- SEEK MEAT: steer to the committed gib, or climb the MEAT gradient toward a pile. ---
            attached.host = None;
            // Coarse fallback = the MEAT hotspot the brain aimed at; a hauling carrier hugs its chunk.
            let coarse = active.target;
            let hauling = carry.is_some_and(|c| c.hauling);
            crab_seek_meat(
                &mut motion, &stig, &dungeon, &graph, gib_pos, coarse, hauling, sep, dt, t, bc,
            )
        } else {
            // --- SURFACE MODE: shared flow-field pursuit across floor + walls. ---
            {
                attached.host = None;
                let p = graph.patch(motion.patch);
                let flow = field.flow(motion.patch);
                let tangent = match flow {
                    Some((_, gate)) => {
                        project_tangent(gate - motion.pos, p.normal).normalize_or_zero()
                    }
                    None => Vec3::ZERO,
                };
                // Weave side-to-side across the flow direction (on the surface plane) so crabs fan out
                // over the path instead of stacking; each crab's phase is offset by its bias.
                let side = tangent.cross(p.normal).normalize_or_zero();
                let phase = now * bc.jitter_freq + motion.angle_bias * std::f32::consts::TAU;
                let jitter = side * (phase.sin() * bc.jitter_strength);
                // Mustered (alarmed) crabs surge faster than a calm forage — the scary charge.
                let pursue_speed = if mustering { bc.speed * bc.muster_speed_mul } else { bc.speed };
                // Blind-side stalk: if the nearest unit is close and looking at this crab, arc around
                // toward its rear (tangential to the bearing, on the side that heads for its back) rather
                // than charging head-on — until the crab clears the facing cone and the pounce gate opens.
                let stalk = match nunit {
                    Some((_, upos, ufwd))
                        if {
                            let d = (upos - motion.pos).with_y(0.0).length();
                            d > bc.jump_min
                                && d < bc.stalk_band
                                && unit_is_facing(upos, ufwd, motion.pos, bc.pounce_blind_cos)
                        } =>
                    {
                        let bearing = (upos - motion.pos).with_y(0.0).normalize_or_zero();
                        let tang = Vec3::new(-bearing.z, 0.0, bearing.x); // perpendicular, ground plane
                        let sign = if tang.dot(ufwd) >= 0.0 { 1.0 } else { -1.0 }; // toward the unit's rear
                        project_tangent(tang * sign, p.normal).normalize_or_zero()
                            * (pursue_speed * bc.stalk_strength)
                    }
                    _ => Vec3::ZERO,
                };
                let move_vec = tangent * pursue_speed
                    + stalk
                    + jitter
                    + project_tangent(sep, p.normal) * bc.sep_strength;
                let moving = move_vec.length_squared() > 1.0e-6;
                motion.pos += move_vec * dt;
                motion.pos = clamp_to_patch(motion.pos, p);

                if let Some((next, gate)) = flow {
                    if motion.pos.distance(gate) < TRANSFER_RADIUS {
                        motion.patch = next;
                        motion.pos = clamp_to_patch(gate, graph.patch(next));
                    }
                }

                let target_n = graph.patch(motion.patch).normal;
                motion.normal = motion.normal.lerp(target_n, t).normalize_or(target_n);
                if moving {
                    let h = project_tangent(move_vec, motion.normal).normalize_or(motion.heading);
                    motion.heading = motion.heading.lerp(h, t).normalize_or(motion.heading);
                }
                if moving {
                    CrabState::Walk
                } else {
                    CrabState::Idle
                }
            }
        };

        // Light response: a photophobic (or photophilic) crab drifts down (or up) the LightField gradient
        // on top of whatever its mode is doing — light as a constant environmental force. This is local
        // photophobic/photophilic taxis (down/up the illuminance gradient); the avoidance direction is
        // consistent with Nakagaki et al. 2007's Physarum result, not their minimum-risk routing (a global
        // path integral between fixed endpoints, which this local step has none of). Skipped while latching:
        // a piranha crab rides a unit's body, off the floor light field. Deterministic (field + gradient +
        // config gain) on the pinned FixedUpdate path, so it folds into the replay hash like any crab
        // motion. `clamp_to_patch` keeps the nudge on the current surface patch (gate crossings stay with
        // the mode's flow-field).
        if !latching {
            // Aggression overrides light. A *committed* crab — one the swarm has recruited via the ALARM
            // (Muster) or rally (Rally) pheromone, one already climbing/feeding (Latch), or one hauling a
            // gib home (Carry) — drives THROUGH the light instead of being repelled by it. So the moment the
            // squad opens fire, the ALARM bloom flips nearby crabs to Muster and the swarm floods the lit
            // room; an *idle* forager still shies from the light, so lit ground stays tactical cover ("dark =
            // danger" holds). This is a per-mode gain scale on the existing photophobic taxis, NOT a second
            // path — one light-push, its strength gated by the crab's current decision. `ActiveBehavior.mode`
            // is written by `think` on the pinned FixedUpdate path, so this stays deterministic / replay-safe.
            use crate::ai::utility::Mode;
            let commit = matches!(
                active.mode,
                Mode::Muster | Mode::Rally | Mode::Latch | Mode::Carry
            );
            let light_scale = if commit { 0.0 } else { 1.0 };
            let signed_gain = if photophobic.is_some() {
                -config.lighting.photophobic_gain * light_scale
            } else if photophilic.is_some() {
                config.lighting.photophilic_gain * light_scale
            } else {
                0.0
            };
            let push = crate::light::light_push(&light_field, &dungeon, motion.pos, signed_gain);
            if push.length_squared() > 1.0e-9 {
                let p = graph.patch(motion.patch);
                motion.pos += project_tangent(push, p.normal) * dt;
                motion.pos = clamp_to_patch(motion.pos, p);
            }
        }

        // Almond Water foraging: a WOUNDED crab climbs the water gradient toward a richer seep, on top of
        // whatever its mode is doing — stigmergic foraging over a regenerating resource (Heylighen,
        // *Cognitive Systems Research* 2015). A healthy crab (`fraction` above the wounded threshold) ignores
        // the field — no cost when full — and the push is zero on flat water, so a crab nowhere near a
        // gradient is unbiased. Skipped while latching (a crab riding a unit's body is off the floor water).
        // Deterministic (field + gradient + belief + config gain) on the pinned FixedUpdate path, folding into
        // the replay hash like the light nudge above. This is what makes the seeps contested territory: the
        // same pools heal the squad, so a wounded crab and a wounded unit are drawn to the same water.
        //
        // Belief-modulated (the inversion mechanic): a crab that can smell seeks water it reads as heal and
        // FLEES water it reads as cyanide; an anosmic crab can't tell, so it seeks any water — and forages
        // straight into poison (emergent selection pressure against anosmia). The reading is gated by the
        // deadband so an unsettled pool draws no forage either way.
        if !latching && health.fraction() <= config.almond_water.forage_wounded_frac {
            let aw = &config.almond_water;
            let belief = almond_water.belief_at(dungeon.world_to_cell(motion.pos));
            let seek = if smell.anosmic || belief >= aw.belief_flip_hi {
                1.0 // seek water (heal pool, or can't smell the danger)
            } else if belief <= aw.belief_flip_lo {
                -1.0 // flee (a smelling crab avoids cyanide water)
            } else {
                0.0 // unsettled deadband — no forage
            };
            if seek != 0.0 {
                // The water gradient is on the scale of `capacity` (~100), not light's ~1, so normalise by
                // capacity to keep `forage_gain` on the same footing as the light gains — otherwise the push
                // is ~capacity× too strong and a wounded crab lurches across the map toward the nearest seep.
                let forage_gain = seek * aw.forage_gain / aw.capacity.max(1.0e-6);
                let push =
                    crate::almond_water::almond_push(&almond_water, &dungeon, motion.pos, forage_gain);
                if push.length_squared() > 1.0e-9 {
                    let p = graph.patch(motion.patch);
                    motion.pos += project_tangent(push, p.normal) * dt;
                    motion.pos = clamp_to_patch(motion.pos, p);
                }
            }
        }

        // Seat & orient flat to the current surface (floor, wall, or a unit's body).
        transform.translation = motion.pos + motion.normal * CRAB_BODY_CENTER;
        transform.rotation = surface_orientation(motion.heading, motion.normal);

        if *state != want {
            *state = want;
        }
    }
}


/// Wire the crab's asynchronously-spawned `AnimationPlayer` to the shared graph. Skips players that
/// don't belong to a crab (e.g. squad figurines) and tolerates the player not existing yet.
fn attach_crab_animation(
    mut commands: Commands,
    anim: Res<CrabAnim>,
    added: Query<Entity, Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    crabs: Query<(), With<Crab>>,
) {
    for player in &added {
        // Walk up the hierarchy to find the owning crab, if any.
        let mut cur = player;
        let owner = loop {
            if crabs.get(cur).is_ok() {
                break Some(cur);
            }
            match parents.get(cur) {
                Ok(child_of) => cur = child_of.parent(),
                Err(_) => break None,
            }
        };
        let Some(owner) = owner else { continue };

        commands
            .entity(player)
            .insert((AnimationGraphHandle(anim.graph.clone()), AnimationTransitions::new()));
        commands.entity(owner).insert(CrabAnimPlayer {
            player,
            playing: None,
        });
    }
}

/// Cross-fade each crab's clip to match its state; only acts on a real change (or first wiring). The
/// walk/attack clips play faster than authored so the leg cycle keeps pace with the scuttle rather than
/// foot-sliding.
fn drive_crab_animation(
    anim: Res<CrabAnim>,
    mut crabs: Query<(&CrabState, &mut CrabAnimPlayer)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (state, mut link) in &mut crabs {
        if link.playing == Some(*state) {
            continue;
        }
        let Ok((mut player, mut transitions)) = players.get_mut(link.player) else {
            continue; // transitions component not applied yet — retry next frame
        };
        let (node, speed) = match state {
            CrabState::Idle => (anim.idle, 1.0),
            CrabState::Walk => (anim.walk, WALK_ANIM_SPEED),
            CrabState::Attack => (anim.attack, ATTACK_ANIM_SPEED),
        };
        let active = transitions.play(&mut player, node, CROSSFADE);
        active.repeat().set_speed(speed);
        link.playing = Some(*state);
    }
}

/// Nearest prey: its position, its planar **forward** (`rotation * −Z`, for the blind-side pounce gate),
/// and the planar distance to `pos`. Read-only over the pounce system's prey query; a thin wrapper over
/// [`crate::util::nearest_planar`] (the shared ranking) carrying the forward vector as the payload.
fn nearest_prey(
    prey: &Query<(&Transform, &mut Health), (With<Prey>, Without<Crab>)>,
    pos: Vec3,
) -> Option<(Vec3, Vec3, f32)> {
    crate::util::nearest_planar(
        pos,
        prey.iter()
            .map(|(ptf, _)| (ptf.rotation * Vec3::NEG_Z, ptf.translation)),
    )
    .map(|(fwd, p, d)| (p, fwd, d))
}

/// Per-crab caste-swap cooldown (hysteresis) so a crab can't re-role again until it counts down —
/// stops castes chattering tick-to-tick. Not itself in `snapshot_hash` (crabs hash by Transform+Health);
/// see the determinism note on [`re_role_crabs`].
#[derive(Component)]
struct Caste {
    cooldown: f32,
}

/// The crab's immortal spawn seed, kept so a promotion re-seeds [`Scout::new`] deterministically and so
/// re-role's per-tick flip budget selects the SAME crabs regardless of ECS iteration order (sort key).
///
/// `pub` because it is the swarm's only stable identity, and the boss's cull
/// ([`crate::enemy::smiley_defense`]) needs it too: that swat picks WHICH crabs die by sorted order, and a
/// position-only key is not a total order — crabs piled on the boss sit at bit-identical coordinates
/// (measured: 6 fully-tied pairs on held-in world `0xA11CE`), so the tie decided a LETHAL pick by ECS query
/// order. A raw `Entity` cannot serve: ids are recycled and their order is not reproducible across
/// same-seed runs — that is the instability being guarded against, not a guard.
#[derive(Component, Clone, Copy)]
pub struct CrabSeed(pub u32);

// Dynamic-caste policy + bounds (README "let crabs re-role between scout and assault as swarm needs
// shift") moved to `behavior.crab` (caste_cooldown, caste_flips_per_tick, rally_live, alarm_high,
// promote_density, scout_min_frac, scout_max_frac). Live scouts are held in
// `[scout_min_frac, scout_max_frac]`; promote/demote signals + a per-crab cooldown give hysteresis.

/// One crab's re-role verdict this tick (pure; unit-tested).
#[derive(PartialEq, Debug)]
enum Rerole {
    Promote,
    Demote,
    Hold,
}

/// Pure caste decision from the local stigmergic picture (cooldown gating handled by the caller):
/// a scout demotes when the swarm is already converging (live beacon) or pressed (alarm); an assault
/// crab promotes when it's crowded with no beacon (recon is the marginal need). Everything else holds.
fn caste_decision(
    is_scout: bool,
    beacon: bool,
    density: f32,
    alarm: f32,
    alarm_high: f32,
    promote_density: f32,
) -> Rerole {
    if is_scout {
        if beacon || alarm > alarm_high {
            Rerole::Demote
        } else {
            Rerole::Hold
        }
    } else if !beacon && density > promote_density {
        Rerole::Promote
    } else {
        Rerole::Hold
    }
}

/// Dynamic castes: re-role crabs between scout and assault as the swarm's needs shift, instead of the
/// birth-fixed split. Runs on `FixedUpdate` after the stigmergic fields refresh and before the brains
/// think, so a flipped crab runs its new brain next tick.
///
/// **Determinism (per `TESTING.md`).** No RNG entropy: the decision is a pure function of deterministic
/// field samples + the fixed [`CrabSeed`]; the per-tick flip budget picks crabs in `CrabSeed` order
/// (order-independent of ECS iteration); a promotion re-seeds `Scout::new` from the stored seed. So
/// two same-seed runs make identical flips and `deterministic_core_is_bit_identical` holds by
/// construction (no committed crab hash to re-pin). `BrainId` and the `Scout` component are always
/// changed **together** so the brain (keys off `BrainId`) and the scout systems (key off `Scout`) never
/// desync.
fn re_role_crabs(
    time: Res<Time>,
    stig: Res<crate::ai::field::Stig>,
    rally: Res<crate::ai::field::RallyField>,
    dungeon: Res<Dungeon>,
    mut commands: Commands,
    mut crabs: Query<(Entity, &CrabMotion, &mut Caste, Option<&Scout>, &CrabSeed)>,
    beh: Res<BehaviorTuning>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    let bc = &beh.crab;
    let total = crabs.iter().count();
    if total == 0 {
        return;
    }

    let mut scouts = 0usize;
    let mut promotes: Vec<(Entity, u32)> = Vec::new();
    let mut demotes: Vec<(Entity, u32)> = Vec::new();
    for (e, motion, mut caste, scout, seed) in &mut crabs {
        caste.cooldown = (caste.cooldown - dt).max(0.0);
        let is_scout = scout.is_some();
        if is_scout {
            scouts += 1;
        }
        if caste.cooldown > 0.0 {
            continue; // hysteresis: recently flipped, leave it be
        }
        let density = stig.sample(crate::ai::field::FieldId::CRAB_DENSITY, &dungeon, motion.pos);
        let beacon = rally.sample(&dungeon, motion.pos).length() > bc.rally_live;
        let alarm = stig.sample(crate::ai::field::FieldId::ALARM, &dungeon, motion.pos);
        match caste_decision(is_scout, beacon, density, alarm, bc.alarm_high, bc.promote_density) {
            Rerole::Promote => promotes.push((e, seed.0)),
            Rerole::Demote => demotes.push((e, seed.0)),
            Rerole::Hold => {}
        }
    }

    let min_scouts = (total as f32 * bc.scout_min_frac).round() as usize;
    let max_scouts = (total as f32 * bc.scout_max_frac).round() as usize;
    // Deterministic tiebreak: same crabs flip regardless of iteration order.
    // `CrabSeed` is unique per crab, so these ARE total — and the check now proves it rather than trusting
    // the comment. Both feed a `take(flips_per_tick)` budget, so a tie would silently pick different crabs.
    crate::sort_total!(&mut promotes, |&(_, s): &(Entity, u32)| s);
    crate::sort_total!(&mut demotes, |&(_, s): &(Entity, u32)| s);

    let mut budget = bc.caste_flips_per_tick;
    for &(e, seed) in &promotes {
        if budget == 0 || scouts >= max_scouts {
            break;
        }
        // Promote in lockstep: assault brain → scout brain + insert the Scout component + arm cooldown.
        // `try_insert`: a targeted crab can be shot dead this same tick before the command applies —
        // skip it silently rather than panic on a despawned entity (the count stays deterministic since
        // the death is deterministic).
        commands.entity(e).try_insert((
            crate::ai::brain::BrainId::Scout,
            Scout::new(seed),
            Caste { cooldown: bc.caste_cooldown },
        ));
        scouts += 1;
        budget -= 1;
    }
    for &(e, _) in &demotes {
        if budget == 0 || scouts <= min_scouts {
            break;
        }
        // Demote in lockstep: drop the Scout component AND switch the brain back to assault. `try_*` for
        // the same same-tick-death reason as the promote path above.
        commands
            .entity(e)
            .try_remove::<Scout>()
            .try_insert((crate::ai::brain::BrainId::Crab, Caste { cooldown: bc.caste_cooldown }));
        scouts -= 1;
        budget -= 1;
    }
}

/// Pounce attack: a grounded, hunting crab hunkers down, then leaps a ballistic arc (~10 body lengths)
/// onto a nearby unit and bites on landing. While hunkering/airborne this owns the crab's transform
/// (`crab_locomotion` skips it); on landing it re-homes onto the surface and starts a cooldown. A short
/// wind-up + high peak reads as a real pounce, not a glide.
fn crab_jump(
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    mut crabs: Query<
        (
            &mut CrabMotion,
            &mut CrabState,
            &mut CrabJump,
            &mut Transform,
            &crate::ai::brain::ActiveBehavior,
        ),
        With<Crab>,
    >,
    mut prey: Query<(&Transform, &mut Health), (With<Prey>, Without<Crab>)>,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let Some(graph) = graph else { return };
    let bc = &beh.crab;
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    for (mut motion, mut state, mut jump, mut tf, active) in &mut crabs {
        match jump.phase {
            JumpPhase::Ready => {
                jump.cooldown = (jump.cooldown - dt).max(0.0);
                if jump.cooldown > 0.0 {
                    continue;
                }
                // Only pounce while hunting units (approaching prey), and only at a unit in the band.
                // Muster (alarm surge) and Rally (scout-recruited surge) are aggressive presses too — a
                // charging crab must be able to leap, or the surge reads as a plain walk-up. Without them
                // a mustering crab crosses the whole pounce band (bc.jump_min..bc.jump_len) before flipping to
                // Latch at dist<1.2 (already inside bc.jump_min), so it would never lunge.
                let aggressive = matches!(
                    active.mode,
                    crate::ai::utility::Mode::Latch
                        | crate::ai::utility::Mode::Forage
                        | crate::ai::utility::Mode::Muster
                        | crate::ai::utility::Mode::Rally
                );
                if !aggressive {
                    continue;
                }
                if let Some((tpos, tfwd, d)) = nearest_prey(&prey, motion.pos) {
                    // Blind-side gate: only commit the leap from outside the prey's facing cone, so a
                    // crab pounces when the unit isn't looking rather than lunging head-on into its guns.
                    let in_blind_spot = !unit_is_facing(tpos, tfwd, motion.pos, bc.pounce_blind_cos);
                    if d > bc.jump_min && d < bc.jump_len && in_blind_spot {
                        jump.phase = JumpPhase::Hunker;
                        jump.timer = bc.jump_hunker;
                        jump.from = motion.pos;
                        jump.to = tpos;
                    }
                }
            }
            JumpPhase::Hunker => {
                jump.timer -= dt;
                *state = CrabState::Attack;
                // Crouch: dip toward the surface during the wind-up.
                tf.translation = motion.pos + motion.normal * (CRAB_BODY_CENTER * 0.4);
                if jump.timer <= 0.0 {
                    // Launch toward the prey's CURRENT position.
                    if let Some((tpos, _, _)) = nearest_prey(&prey, motion.pos) {
                        jump.to = tpos;
                    }
                    jump.from = motion.pos;
                    jump.phase = JumpPhase::Air;
                    jump.timer = bc.jump_air;
                    if crate::ai::diag::AI_DIAG {
                        info!("crab: POUNCE dist={:.1}", (jump.to.xz() - jump.from.xz()).length());
                    }
                }
            }
            JumpPhase::Air => {
                jump.timer -= dt;
                let s = (1.0 - (jump.timer / bc.jump_air)).clamp(0.0, 1.0);
                let ground = jump.from.lerp(jump.to, s);
                let height = bc.jump_arc * (std::f32::consts::PI * s).sin();
                motion.pos = ground;
                // Re-home onto the surface beneath the arc so it lands on a real patch.
                if let Some(fp) = graph.floor_patch_cell(dungeon.world_to_cell(ground)) {
                    motion.patch = fp;
                    motion.normal = graph.patch(fp).normal;
                }
                let dir = jump.to.xz() - jump.from.xz();
                if dir.length_squared() > 1.0e-6 {
                    motion.heading = Vec3::new(dir.x, 0.0, dir.y).normalize_or(motion.heading);
                }
                tf.translation = ground + motion.normal * CRAB_BODY_CENTER + Vec3::Y * height;
                tf.rotation = surface_orientation(motion.heading, motion.normal);
                *state = CrabState::Attack;
                if jump.timer <= 0.0 {
                    // Land: clamp onto the patch and bite the nearest prey in reach. A pounce is a
                    // committed lunge, so it always bites on landing — a flat, reliable JUMP_DAMAGE hit
                    // (the super-linear pile bonus lives in `crab_contact_damage`). No critical-mass gate:
                    // the old MASS_MIN check made a lone leap deal zero, so a pouncing crab read as a
                    // harmless hop; a lunge that connects should hurt.
                    motion.pos = clamp_to_patch(motion.pos, graph.patch(motion.patch));
                    let reach_sq = (UNIT_BODY_RADIUS + bc.contact_radius + 0.2).powi(2);
                    // WHICH unit the lunge bites must not depend on prey query order. This used to take
                    // the first in-reach prey the ECS happened to yield and `break` — a
                    // keep-the-first-on-a-tie pick straight into `Health`, and query order is not stable
                    // across `App` instances. A crab landing between two units bit a different one run to
                    // run. `nearest_planar` ranks by `(distance bits, position bits)`, so the victim is a
                    // pure function of geometry — and biting the NEAREST is what the lunge meant anyway.
                    if let Some((_, tpos, _)) =
                        crate::util::nearest_planar(motion.pos, prey.iter().map(|(ptf, _)| ((), ptf.translation)))
                        && (tpos.xz() - motion.pos.xz()).length_squared() <= reach_sq
                    {
                        for (ptf, mut hp) in &mut prey {
                            if ptf.translation == tpos {
                                hp.current -= sim.combat.crab_jump_damage;
                                break;
                            }
                        }
                    }
                    jump.phase = JumpPhase::Ready;
                    jump.cooldown = bc.jump_cooldown;
                }
            }
        }
    }
}

/// Alarm-pheromone recruitment to defense: a wounded crab floods the local ALARM channel so every crab
/// within ~one room reads `Fact::AlarmHere`, musters (converges on the squad), and stops fleeing — the
/// fix for "shoot the crabs and they just scatter". This is the retaliatory, *local* twin of the nest's
/// own alarm (`nest::nest_alarm`): nest hit → a stronger, wider bloom, crab hit → a one-room alarm bloom
/// that self-limits as the field evaporates. Detection mirrors `nest_alarm`'s idiom — a crab whose
/// `Health` changed this frame and now sits below full was just hit (crabs never heal), so it deposits.
/// A stigmergic warning cry (Heylighen, "Stigmergy as a universal coordination mechanism", CSR 2016).
fn crab_alarm_on_damage(
    crabs: Query<(Ref<Health>, &Transform), With<Crab>>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    sim: Res<SimTuning>,
) {
    // Collect this tick's wounded-crab alarm deposits into a local batch, then sort it into the canonical
    // deposit order before appending to the shared queue. `drain_deposits` applies each with a
    // non-associative `f32 +=` in queue order, and the crab query order is NOT reproducible across runs
    // (async GLB load + entity-id reuse — see the carry logistics), so two wounded crabs whose ALARM
    // blooms overlap would otherwise sum to a query-order-dependent value, diverging the ALARM channel and
    // the physics-off replay hash (~1-3% of runs). Sorting the batch makes the drained field a pure
    // function of the deposits, exactly as `ai::field::sort_deposits` documents (the same class of fix as
    // the crab-separation bucket sort above).
    let mut batch: Vec<crate::ai::field::Deposit> = Vec::new();
    for (hp, tf) in &crabs {
        // `is_changed()` also fires on spawn; `is_added()` screens that out. `current < max` restricts to
        // an actual wound (a fresh full-health crab that was merely touched by the ECS won't deposit).
        if hp.is_changed() && !hp.is_added() && hp.current < hp.max {
            batch.push(crate::ai::field::Deposit {
                pos: tf.translation,
                field: crate::ai::field::FieldId::ALARM,
                amount: sim.deposit.alarm_crab,
            });
        }
    }
    crate::ai::field::sort_deposits(&mut batch);
    deposits.0.extend(batch);
}

/// Feeding sates hunger: an actively-biting crab (`CrabState::Attack`) drains its HUNGER drive, so a fed
/// crab's forage/latch/seek weighting falls and it peels off while hungrier crabs press. Without this,
/// HUNGER only ever rises (nothing consumed it), saturating every crab at 1.0 within ~33 s — a uniform
/// constant that cancels out of the utility maths and gives zero per-agent differentiation. Pairs with
/// the per-crab HUNGER seed at spawn.
fn crab_feeding_sates_hunger(
    time: Res<Time>,
    mut crabs: Query<(&CrabState, &mut crate::ai::drives::Drives), With<Crab>>,
    sim: Res<SimTuning>,
) {
    let dt = time.delta_secs();
    for (state, mut drives) in &mut crabs {
        if *state == CrabState::Attack {
            let h = drives.get(crate::ai::drives::DriveId::HUNGER);
            drives.set(crate::ai::drives::DriveId::HUNGER, h - sim.breeding.hunger_sate_rate * dt);
        }
    }
}

/// Feeding frenzy: damage to a unit grows **super-linearly** with how many crabs are on it
/// (`CRAB_CONTACT_DPS * count^DAMAGE_EXPONENT`), so one crab is a real nuisance and a pile shreds it —
/// a smooth ramp with NO critical-mass cliff (1 crab ≈ 3 DPS, 3 ≈ 15, 5 ≈ 33, 10 ≈ 95). The old hard
/// `MASS_MIN` gate made 1–4 crabs deal literally zero damage, so a thinned/split swarm played harmless;
/// the super-linear curve already makes a lone crab weak and a pile terrifying without a dead zone.
/// Counts by PLANAR distance so a crab clinging high on the body still feeds.
fn crab_contact_damage(
    time: Res<Time>,
    crabs: Query<(Entity, &Transform), (With<Crab>, Without<Prey>)>,
    // `Option<&mut LastAttacker>` is present only on the smiley watcher: a crab biting it records itself as
    // the attacker so the watcher retaliates against the swarm (not a bystander unit) — see `enemy::smiley_zap`.
    mut prey: Query<(&Transform, &mut Health, Option<&mut crate::enemy::LastAttacker>), (With<Prey>, Without<Crab>)>,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let dt = time.delta_secs();
    let bc = &beh.crab;
    // Reach = body radius + a little, so anything latched onto the cylinder counts (units and the boss).
    let reach_sq = (UNIT_BODY_RADIUS + bc.contact_radius).powi(2);
    for (prey_tf, mut hp, last_attacker) in &mut prey {
        // Count biters and attribute the bite to ONE of them for the boss's retaliation. WHICH biter is
        // recorded (`LastAttacker`, which `enemy::smiley_zap` instakills) must not depend on query order
        // — it is not reproducible across same-seed runs (see `util::nearest_planar`) — so pick the
        // lowest world position among the biters, a stable geometric key.
        let mut count = 0usize;
        let mut biter: Option<Entity> = None;
        let mut biter_key: Option<(u32, u32, u32)> = None;
        for (ce, ctf) in &crabs {
            if (ctf.translation.xz() - prey_tf.translation.xz()).length_squared() <= reach_sq {
                count += 1;
                let key = (
                    ctf.translation.x.to_bits(),
                    ctf.translation.y.to_bits(),
                    ctf.translation.z.to_bits(),
                );
                if biter_key.is_none_or(|bk| key < bk) {
                    biter = Some(ce);
                    biter_key = Some(key);
                }
            }
        }
        if count > 0 {
            hp.current -= sim.combat.crab_contact_dps * (count as f32).powf(sim.combat.crab_damage_exponent) * dt;
            if let Some(mut la) = last_attacker {
                la.entity = biter;
                la.age = 0.0;
            }
        }
    }
}

/// Despawn dead crabs with a small blood burst + squelch (reuses the enemy-death VFX/SFX path).
/// Tag set by `enemy::smiley_defense` on a crab it culls. Read by `crab_despawn_dead` (the single crab
/// despawn owner) to emit the boss-swat gore variant and — crucially — suppress the blood SCENT bloom a
/// normal death emits (a scent here would magnet more crabs into a feeding feedback loop).
#[derive(Component)]
pub struct Culled;

/// The set holding [`crab_despawn_dead`], the single owner of crab removal.
///
/// Anything that *tags* a crab for death rather than despawning it — `enemy::smiley_defense`, which writes
/// [`Culled`] — must be ordered `.before` this set. A sole despawn owner prevents a double-despawn, but it
/// does nothing about an `insert` command queued **after** the despawn command: applying it panics with
/// "Entity despawned". The two systems live in different plugins, so nothing but this set can express the
/// ordering they have always relied on.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CrabDespawn;

/// The ONE system that removes a crab at ≤ 0 HP, whatever zeroed it (laser, `smiley_zap`, or a boss cull
/// via the `Culled` tag). Being the sole despawn+gore owner is what prevents the double-despawn /
/// double-gore race with `smiley_defense`, which now only zeroes HP + tags instead of despawning itself.
fn crab_despawn_dead(
    mut commands: Commands,
    mut gore: ResMut<GoreQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    crabs: Query<(Entity, &Health, &Transform, Option<&Culled>, &CrabSeed), With<Crab>>,
    sim: Res<SimTuning>,
    audio: Res<crate::audio_tuning::AudioTuning>,
) {
    // Emit gore + despawn deaths in a STABLE order (by `CrabSeed`, unique+deterministic), NOT crab query
    // order — which is not reproducible across same-seed runs (see `util::nearest_planar`). The gore
    // drain stamps each meat chunk with a per-event seed counter, and the ECS entity free-list reuses
    // these just-freed ids; both depend on THIS order, so an unsorted pass gives meat chunks
    // nondeterministic spawn params AND a nondeterministic gib table/query order, which then makes the
    // crab foraging assignment (`assign_meat_targets`) diverge and breaks the physics-free replay hash.
    let mut dead: Vec<(u32, Entity, Vec3, bool)> = crabs
        .iter()
        .filter(|(_, hp, _, _, _)| hp.current <= 0.0)
        .map(|(e, _, tf, culled, seed)| (seed.0, e, tf.translation, culled.is_some()))
        .collect();
    crate::sort_total!(&mut dead, |(seed, _, _, _)| *seed);
    for (_, entity, pos, culled) in dead {
        if culled {
            // Boss cull (`smiley_defense`): green-ichor swat, and deliberately NO SCENT deposit — a
            // scent bloom here would magnet more crabs into a feeding feedback loop. No per-crab death
            // sfx either (the boss already played one batched swat for the whole cull).
            gore.0.push(GoreEvent {
                pos,
                kind: GoreKind::EnemySplat,
                tint: crate::palette::CRAB_ICHOR, // Type-Gray reanimated ichor (green)
                gib: None,
                intensity: 0.2,
            });
        } else {
            gore.0.push(GoreEvent {
                pos,
                kind: GoreKind::EnemySplat,
                tint: crate::palette::CRAB_ICHOR_DULL, // sickly green crab ichor
                gib: None,
                // Chaff: a crab death barely nudges the camera, so a whole swarm dying doesn't read as
                // one giant explosion (the gib chunks still pop — only the feel layer is scaled down).
                intensity: crate::gore::death_intensity(sim.combat.crab_hp, sim.combat.crab_contact_dps),
            });
            // Blood → SCENT: a fresh kill draws the swarm and the boss to the feeding site.
            deposits.0.push(crate::ai::field::Deposit {
                pos,
                field: crate::ai::field::FieldId::SCENT,
                amount: sim.deposit.blood_scent,
            });
            sfx.write(Sfx::EnemyDeath(pos));
            // The wet crunch of a crab death carries as swarm din (`NOISE_SWARM`); the *units* read it.
            deposits.0.push(crate::ai::field::Deposit {
                pos,
                field: crate::ai::field::FieldId::NOISE_SWARM,
                amount: audio.stimulus.enemy_death_loudness,
            });
        }
        commands.entity(entity).despawn();
    }
}

// Flee speed multiplier + the carry-crew tuning (carry_capacity, grab_range, los_range, carry_hold,
// carry_speed, weight_drag, deliver_range, crew_timeout, max_commit_dist) moved to `behavior.crab`. Only
// the render-height seat stays in code.

/// World height a hauled chunk rides at — the crew's mouth height (crab seat ~0.05 + model ~0.11 + tooth
/// bone), so the chunk is gripped at the mouths rather than floating overhead.
const CARRY_HEIGHT: f32 = 0.15;

/// Shared handles kept so `nest_reproduce` can spawn new crabs at runtime.
#[derive(Resource)]
struct CrabAssets {
    collider: Handle<Mesh>,
    scene: Handle<WorldAsset>,
}

/// Panic locomotion: crawl *down* the THREAT gradient (away from danger) across the floor, with free
/// cell-by-cell transfer (unlike surface pursuit, flight isn't along the field's gate). Returns the
/// animation state. This is the movement half of the emergent frenzy→scatter.
fn crab_flee(
    motion: &mut CrabMotion,
    stig: &crate::ai::field::Stig,
    dungeon: &Dungeon,
    graph: &crate::surface_nav::SurfaceGraph,
    sep: Vec3,
    dt: f32,
    t: f32,
    bc: CrabTuning,
) -> CrabState {
    // Flee down the THREAT gradient (away from danger); if the field is flat, keep the current heading
    // so the crab keeps moving rather than freezing. `steer_surface` routes this along the graph, so a
    // cornered crab climbs a wall to escape instead of clipping through it.
    let g = stig.gradient(crate::ai::field::FieldId::THREAT_GUN, dungeon, motion.pos);
    let away = Vec3::new(-g.x, 0.0, -g.y);
    let desired = if away.length_squared() > 1.0e-6 {
        away
    } else {
        motion.heading
    };
    if steer_surface(
        motion,
        graph,
        dungeon,
        desired,
        None,
        bc.speed * bc.flee_speed_mul,
        sep,
        dt,
        t,
        bc,
    ) {
        CrabState::Walk
    } else {
        CrabState::Idle
    }
}

/// Mass on a scout's marked sighting by following the **local vectorial rally pheromone** (Tang et al.
/// 2019): the sampled vector already points toward the (moving) prey — the swarm reads it and steers
/// straight along it, routed around walls by `steer_surface`. This is the paper's map-guided "tracking"
/// mode (robots move according to the pheromone map). A crab only enters Rally when the local magnitude
/// clears `RALLY_MIN`, so the vector is non-zero here; a vanishing vector just holds heading.
fn crab_rally(
    motion: &mut CrabMotion,
    rally: &crate::ai::field::RallyField,
    dungeon: &Dungeon,
    graph: &crate::surface_nav::SurfaceGraph,
    sep: Vec3,
    dt: f32,
    t: f32,
    bc: CrabTuning,
) -> CrabState {
    let v = rally.sample(dungeon, motion.pos);
    let desired = if v.length_squared() > 1.0e-6 {
        Vec3::new(v.x, 0.0, v.y)
    } else {
        motion.heading
    };
    if steer_surface(motion, graph, dungeon, desired, None, bc.speed, sep, dt, t, bc) {
        CrabState::Walk
    } else {
        CrabState::Idle
    }
}

/// Aggressive scout roam: range across floor + walls on a heading re-rolled every `bc.scout_wander_interval`
/// (copies `enemy::enemy_seek`'s wander, routed through the wall-aware `steer_surface` so scouts climb).
/// Faster than the swarm forages so scouts cover ground and find prey to report.
fn crab_scout_roam(
    motion: &mut CrabMotion,
    scout: &mut Scout,
    graph: &crate::surface_nav::SurfaceGraph,
    dungeon: &Dungeon,
    sep: Vec3,
    dt: f32,
    t: f32,
    bc: CrabTuning,
) -> CrabState {
    scout.wander_timer -= dt;
    if scout.wander_timer <= 0.0 || scout.wander_dir == Vec3::ZERO {
        scout.wander_timer = bc.scout_wander_interval;
        let angle = rand01(&mut scout.rng) * std::f32::consts::TAU;
        scout.wander_dir = Vec3::new(angle.cos(), 0.0, angle.sin());
    }
    if steer_surface(
        motion,
        graph,
        dungeon,
        scout.wander_dir,
        None,
        bc.speed * bc.scout_speed_mul,
        sep,
        dt,
        t,
        bc,
    ) {
        CrabState::Walk
    } else {
        CrabState::Idle
    }
}

/// Move a crab one step along the surface graph toward a `desired` world direction, transferring between
/// patches ONLY through graph gates (`SurfaceGraph::neighbors`) — the same wall-respecting mechanic as
/// flow-field pursuit. It therefore never clips through a wall and can climb onto a wall patch when that
/// is the best-aligned escape. `home` is an exact point the crab walks straight to when that point lies
/// on its current patch (the final approach onto / hold on a gib). Returns whether the crab moved.
#[allow(clippy::too_many_arguments)]
fn steer_surface(
    motion: &mut CrabMotion,
    graph: &crate::surface_nav::SurfaceGraph,
    dungeon: &Dungeon,
    desired: Vec3,
    home: Option<Vec3>,
    speed: f32,
    sep: Vec3,
    dt: f32,
    _t: f32,
    bc: CrabTuning,
) -> bool {
    // Final approach: if the homing point sits on THIS patch's cell, walk straight to it (no gate). Some
    // ⇒ the shared core skips the neighbour-gate scan and steers straight at this point (the on-patch
    // final approach onto / hold on a gib).
    let on_patch_home =
        home.filter(|h| graph.floor_patch_cell(dungeon.world_to_cell(*h)) == Some(motion.patch));

    // Reynolds separation, projected onto the current surface and scaled here (the core adds it as-is, so
    // the `project_tangent(sep, n) * bc.sep_strength` arithmetic stays bit-identical to the old copy).
    let n = graph.patch(motion.patch).normal;
    let push = project_tangent(sep, n) * bc.sep_strength;

    // `_t` is ignored: the shared core recomputes `(NORMAL_EASE * dt).min(1.0)` internally (same formula,
    // same `dt`), so the eased normal/heading are bit-identical. Kept in the signature so the 6 call sites
    // and the intermediate steer helpers (which also use `t` for their own lerps) stay untouched.
    crate::surface_nav::steer_surface_core(
        &mut motion.pos,
        &mut motion.patch,
        &mut motion.normal,
        &mut motion.heading,
        graph,
        desired,
        push,
        speed,
        dt,
        on_patch_home,
    )
}

/// Foraging locomotion: crawl toward meat along walkable floor. Long-range navigation follows the MEAT
/// stigmergy gradient, which — because the field lives only on floor cells and diffuses only between
/// them — flows *around* walls (a proper floor-topology potential field; ACO trail ascent, Dorigo).
/// Only within line-of-sight (`bc.los_range`) of the committed chunk does the crab straight-line home onto
/// the exact gib, then hold within `bc.grab_range` for the lift. A flat local field falls back to steering
/// at the coarse target (the MEAT hotspot for a forager, the chunk for a committed crab) so a crab out
/// of the field's reach still heads the right way instead of freezing. Free cell-by-cell floor transfer.
fn crab_seek_meat(
    motion: &mut CrabMotion,
    stig: &crate::ai::field::Stig,
    dungeon: &Dungeon,
    graph: &crate::surface_nav::SurfaceGraph,
    target: Option<Vec3>,
    coarse: Option<Vec3>,
    hauling: bool,
    sep: Vec3,
    dt: f32,
    t: f32,
    bc: CrabTuning,
) -> CrabState {
    // A hauling carrier hugs the chunk (mouth on it); a gathering crab holds a grab-range away.
    let hold = if hauling { bc.carry_hold } else { bc.grab_range };

    // Committed to a chunk that's within reach: hold position and keep the mouth turned onto it.
    if let Some(gp) = target {
        let to = gp - motion.pos;
        if to.length() < hold {
            let np = graph.patch(motion.patch);
            let h = project_tangent(to, np.normal).normalize_or(motion.heading);
            motion.heading = motion.heading.lerp(h, t).normalize_or(motion.heading);
            // Keep applying the Reynolds separation push while holding — this branch returns before
            // `steer_surface` (where separation now lives), so without it a converging crew clumps onto
            // one point and z-fights instead of ringing the chunk (Reynolds 1999, GDC).
            motion.pos += project_tangent(sep, np.normal) * bc.sep_strength * dt;
            return CrabState::Attack;
        }
    }

    // Desired travel direction. Near a committed chunk → straight at it (`steer_surface`'s `home` walks
    // it in once it's on the same patch). Far, or uncommitted → climb the MEAT gradient (wall-aware:
    // the field only lives on floor and routes around walls), falling back toward the coarse hotspot.
    let grad = {
        let g = stig.gradient(crate::ai::field::FieldId::MEAT, dungeon, motion.pos);
        Vec3::new(g.x, 0.0, g.y)
    };
    let desired = match target {
        Some(gp) if (gp - motion.pos).length() < bc.los_range => gp - motion.pos,
        Some(gp) => {
            if grad.length_squared() > 1.0e-6 {
                grad
            } else {
                gp - motion.pos
            }
        }
        None => {
            if grad.length_squared() > 1.0e-6 {
                grad
            } else {
                coarse.map(|c| c - motion.pos).unwrap_or(Vec3::ZERO)
            }
        }
    };
    if steer_surface(motion, graph, dungeon, desired, target, bc.speed, sep, dt, t, bc) {
        CrabState::Walk
    } else {
        CrabState::Idle
    }
}

/// Commit foraging crabs to specific gibs (the "recruitment" step of cooperative transport). For each
/// `SeekMeat` crab without a live target, pick the nearest chunk that still needs crew (Σ committed
/// carrier capacity < weight, not yet being hauled) and enlist it: push the crab into the gib's
/// `carriers`, point `CrabCarry.target` at the gib, and move the gib to `Crewing`. This and `carry_gibs`
/// are the ONLY mutators of `Carryable.carriers` (one-path ownership). Holland & Melhuish 1999
/// (stigmergic clustering); Dorigo ACO (recruitment by trail).
fn assign_meat_targets(
    mut crabs: Query<
        (
            Entity,
            &CrabMotion,
            &mut CrabCarry,
            &crate::ai::brain::ActiveBehavior,
            &CrabSeed,
        ),
        With<Crab>,
    >,
    mut gibs: Query<(Entity, &Transform, &mut crate::gore::Carryable, &crate::gore::GibKey)>,
    beh: Res<BehaviorTuning>,
) {
    let bc = &beh.crab;
    // Drop targets whose gib no longer exists (e.g. capped out of the ring mid-haul) so the crab
    // re-forages. Clearing `hauling` alongside `target` is essential: a lone `target = None` strands a
    // carrier that was mid-haul, because `hauling` keeps `Fact::CarryingMeat` — and thus the `Carry`
    // mode — latched with no chunk to carry. The brain then never leaves Carry, so it steers nowhere
    // (`target` is None), `release_uncommitted_carriers` can't recover it (Carry counts as
    // "committed"), and `carry_gibs` never touches it (the gib is gone) — the crab freezes forever.
    for (_, _, mut cc, _, _) in &mut crabs {
        if let Some(g) = cc.target {
            if gibs.get(g).is_err() {
                cc.target = None;
                cc.hauling = false;
            }
        }
    }

    // No meat on the floor → nothing to enlist crews for. Skip the caps/committed/snapshot/seeker
    // allocations entirely (the common case once a pile is cleared); the stale-target release above has
    // already run, so carriers are freed regardless.
    if gibs.is_empty() {
        return;
    }

    // Snapshot per-crab capacity — summing a gib's committed crew capacity needs every carrier's value.
    let caps: HashMap<Entity, f32> = crabs.iter().map(|(e, _, c, _, _)| (e, c.capacity)).collect();

    // Snapshot each gib: position, weight, whether it's already being hauled, and its current committed
    // capacity. `committed` is mutated as we enlist crabs this tick so several seekers don't over-crew.
    let mut committed: HashMap<Entity, f32> = HashMap::new();
    let mut gib_snap: Vec<(Entity, Vec3, f32, bool, u64)> = gibs
        .iter()
        .map(|(e, tf, c, key)| {
            // Sum the crew's capacities in a canonical (ascending) order, NOT `carriers` Vec order. The
            // sum feeds the `committed >= weight` lift/commit gate below, and float addition is
            // non-associative, so summing in enumeration order lets a carrier-order difference flip that
            // gate at the boundary — diverging which crab commits, and the physics-free replay hash.
            let mut caps_v: Vec<u32> = c.carriers.iter().filter_map(|x| caps.get(x)).map(|v| v.to_bits()).collect();
            // SORT-OK: bare f32 bits about to be summed — a tie is the same term twice. Interchangeable.
            caps_v.sort_unstable();
            let sum: f32 = caps_v.into_iter().map(f32::from_bits).sum();
            committed.insert(e, sum);
            (e, tf.translation, c.weight, c.phase == crate::gore::CarryPhase::Hauling, key.0)
        })
        .collect();
    // Determinism: gib enumeration follows entity spawn / ID-reuse order, which is NOT a stable
    // semantic ordering — two same-seed runs can produce the *same* set of chunks in a different query
    // order. The nearest-chunk pick below keeps the FIRST gib at the minimum distance (`d < bd`), so on
    // an exact distance tie it would commit a crab to a different chunk per run, diverging crab targets
    // and cascading into the physics-free replay hash (`deterministic_core_is_bit_identical`). Sort by
    // world position — a stable key independent of entity order — so the choice depends only on geometry.
    crate::sort_total!(&mut gib_snap, |&(_, p, _, _, key)| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), key));

    // Seeking crabs that still need a chunk. The enlist loop below is greedy and stateful (each commit
    // bumps `committed`, so who picks first decides who gets the last slot on a contested chunk), and it
    // pushes carriers whose per-crab (±20%) capacities are then summed non-associatively — so the
    // processing order must be a STABLE TOTAL order. Crab QUERY order isn't reproducible across same-seed
    // runs (see `util::nearest_planar`); world position alone still ties when two crabs sit on the exact
    // same point (early, pre-dispersal), and `sort_unstable` would then break that tie by unstable entity
    // order. Fall back to the stable, unique `CrabSeed` so the whole assignment is deterministic.
    let mut seekers: Vec<(Entity, Vec3, u32)> = crabs
        .iter()
        .filter(|(_, _, c, ab, _)| {
            matches!(ab.mode, crate::ai::utility::Mode::SeekMeat) && c.target.is_none()
        })
        .map(|(e, m, _, _, seed)| (e, m.pos, seed.0))
        .collect();
    crate::sort_total!(&mut seekers, |(_, p, seed)| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), *seed));

    for (crab_e, cpos, _) in seekers {
        let mut best: Option<(Entity, f32)> = None;
        for &(ge, gpos, weight, hauling, _) in &gib_snap {
            if hauling {
                continue;
            }
            if committed.get(&ge).copied().unwrap_or(0.0) >= weight {
                continue; // already has enough crew to lift
            }
            let d = gpos.distance(cpos);
            if d > bc.max_commit_dist {
                continue; // too far to reach by straight-line steering — gradient-forage toward it first
            }
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((ge, d));
            }
        }
        let Some((ge, _)) = best else { continue };

        // Commit: enlist on the gib and record the target on the crab.
        if let Ok((_, _, mut carry, _)) = gibs.get_mut(ge) {
            if !carry.carriers.contains(&crab_e) {
                carry.carriers.push(crab_e);
            }
            if carry.phase == crate::gore::CarryPhase::Resting {
                carry.phase = crate::gore::CarryPhase::Crewing;
            }
        }
        if let Ok((_, _, mut cc, _, _)) = crabs.get_mut(crab_e) {
            cc.target = Some(ge);
        }
        // Count this crab's capacity so later seekers this tick see the fuller crew.
        if let Some(c) = committed.get_mut(&ge) {
            *c += caps.get(&crab_e).copied().unwrap_or(0.0);
        }
    }
}

/// A crab that stops participating in transport drops its load: clearing its target makes `carry_gibs`
/// prune it from the gib's crew next frame, so its capacity leaves and a stalled crew can re-evaluate.
/// A carrier is committed only while its mode is `SeekMeat` (crewing) or `Carry` (hauling); the moment
/// the brain flips it to anything else — Flee (scatter), but also Latch or Forage when a unit wanders
/// into range or the MEAT trace fades — it must release, or a seeker peeling off leaves a phantom
/// carrier that never reaches the pile and stalls the lift until `bc.crew_timeout`. Touches only
/// `CrabCarry` — the sole system besides the two carrier-mutators, and it never edits `carriers`.
fn release_uncommitted_carriers(
    mut crabs: Query<(&crate::ai::brain::ActiveBehavior, &mut CrabCarry), With<Crab>>,
) {
    for (active, mut cc) in &mut crabs {
        let committed = matches!(
            active.mode,
            crate::ai::utility::Mode::SeekMeat | crate::ai::utility::Mode::Carry
        );
        if !committed && cc.target.is_some() {
            cc.target = None;
            cc.hauling = false;
        }
    }
}

/// The cooperative-transport state machine — the SOLE authority over a lifted chunk's transform and
/// rigid-body mode. Each frame, per gib: prune dead/reassigned carriers, sum the live crew's capacity,
/// then take exactly one transition:
///   Resting/Crewing → Hauling   when the capacity *gathered at the chunk* ≥ weight
///                                (switch Dynamic→Kinematic, zero velocities, pick the nearest nest);
///   Crewing → Resting            after `bc.crew_timeout` (disband a crew that can't lift);
///   Hauling → delivered          within `bc.deliver_range` of the nest (hoard += weight, consume the gib);
///   Hauling → Crewing (drop)     if the crew's total capacity falls below weight (Kinematic→Dynamic).
/// The three rigid-body switches are mutually exclusive (never two in one frame) — the "kinematic
/// hand-off guard". During Hauling the gib LEADS along the nest's prebuilt `FlowField` (so it routes
/// around walls), riding at mouth height; the crew just chases and grips it via the `Carry` locomotion
/// branch, so there's no circular follow. Haul speed scales with crew size and inversely with weight.
/// Holland & Melhuish 1999 (stigmergic cooperative transport); avian3d kinematic bodies are moved by
/// transform, so zeroing Lin/Ang velocity on the switch prevents a residual-impulse launch.
#[allow(clippy::type_complexity)]
fn carry_gibs(
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    mut commands: Commands,
    mut gib_ring: ResMut<crate::gore::GibRing>,
    // `RigidBody` is an immutable component in avian3d — switch a body's type by re-inserting it via
    // `Commands`, not by mutating it in place. Velocities are mutable and zeroed on the switch.
    mut gibs: Query<
        (
            Entity,
            &mut crate::gore::Carryable,
            &mut Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
            // Carried solely to give the DELIVER pass below a stable total order — see there.
            &crate::gore::GibKey,
        ),
        With<crate::gore::GibChunk>,
    >,
    mut crabs: Query<
        (&Transform, &mut CrabCarry),
        (With<Crab>, Without<crate::gore::GibChunk>, Without<crate::nest::Nest>),
    >,
    mut nests: Query<
        (Entity, &Transform, &mut crate::nest::Nest),
        (Without<crate::gore::GibChunk>, Without<Crab>),
    >,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let dt = time.delta_secs();
    let bc = &beh.crab;

    // Deliveries are collected here and applied AFTER the loop, in `GibKey` order.
    //
    // The per-gib motion below is order-independent (each gib touches only its own Transform/velocities),
    // but a DELIVER is not: it accumulates `nest.hoard += weight` and `nest.spawn_boost += …` into a SHARED
    // nest with a non-associative `f32 +=`, and it despawns (feeding entity-id reuse). Gib query order is
    // not stable across `App` instances, so two chunks delivering to one nest on one tick summed to
    // different bits per run. That is not cosmetic: `nest_reproduce` gates on `hoard < meat_per_crab`, and
    // crossing that gate consumes a slot from the shared `CrabSpawnSeq`, which sets every subsequent crab's
    // caste, capacity and RNG. The sibling `assign_meat_targets` sorts both its lists for exactly this
    // reason; this loop was left raw (its INNER capacity sums were canonicalised, so only the outer pass
    // was missed). Mutant-armed: `deliver_range` ships 1.2 but the genome bounds reach 4.0 (~11× the
    // delivery area, so deliveries coincide instead of arriving one at a time).
    let mut delivered: Vec<(u64, Entity, Entity, f32)> = Vec::new(); // (GibKey, gib, nest, weight)
    for (ge, mut carry, mut gtf, mut lv, mut av, gib_key) in &mut gibs {
        // 1. Prune carriers down to crabs that still exist AND still point at this gib.
        carry
            .carriers
            .retain(|&c| crabs.get(c).map(|(_, cc)| cc.target == Some(ge)).unwrap_or(false));

        // 2. Sum crew capacity two ways: `cap_here` counts only carriers that have actually gathered at
        // the chunk (within `bc.grab_range`) — that's what can lift it; `cap_total` counts every committed
        // carrier — that's what sustains an in-progress haul (a chaser lagging a little mustn't drop it).
        // Requiring the gathered capacity (not the whole roster) to lift avoids a deadlock where one
        // straggler that can't path to the chunk keeps a full-strength crew from ever lifting.
        // Sum crew capacities in a canonical (ascending) order, NOT `carriers` Vec order: these feed the
        // `cap_here >= weight` lift gate, and float addition is non-associative, so an enumeration-order
        // difference would flip the lift at the boundary and diverge the replay hash (see
        // `assign_meat_targets`).
        let mut here_caps: Vec<u32> = Vec::new();
        let mut total_caps: Vec<u32> = Vec::new();
        for &c in &carry.carriers {
            if let Ok((ctf, cc)) = crabs.get(c) {
                total_caps.push(cc.capacity.to_bits());
                if ctf.translation.xz().distance(gtf.translation.xz()) <= bc.grab_range {
                    here_caps.push(cc.capacity.to_bits());
                }
            }
        }
        // SORT-OK: bare f32 bits about to be summed — ties are identical terms.
        total_caps.sort_unstable();
        here_caps.sort_unstable();
        let cap_total: f32 = total_caps.into_iter().map(f32::from_bits).sum();
        let cap_here: f32 = here_caps.into_iter().map(f32::from_bits).sum();
        let has_crew = !carry.carriers.is_empty();

        match carry.phase {
            crate::gore::CarryPhase::Resting | crate::gore::CarryPhase::Crewing => {
                // A gathered crew lifts only if it has enough capacity at the chunk AND a nest still
                // exists to receive it. Selecting the destination BEFORE committing means a chunk with
                // every nest razed never lifts into an undeliverable Kinematic haul (which used to
                // oscillate Crewing<->Hauling every fixed tick, resetting `crew_timer` each frame so the
                // crew never disbanded). With no nest it stays a Crewing crew and disbands at
                // bc.crew_timeout, cleanly re-foraging.
                let mut dest: Option<Entity> = None;
                if has_crew && cap_here >= carry.weight {
                    // Nearest nest, ranked by the floor delivery cell (`nest.pos`), NOT the wall-mounted
                    // dome `Transform`. Every other consumer (haul nav, deliver check, breeding, scout
                    // home) uses `nest.pos`; ranking by the dome could commit the haul to a nest whose
                    // delivery cell is across a wall, so the flow field returns nothing and the chunk
                    // gets dragged straight through it. Match the deliver check's horizontal distance.
                    // Determinism: break an exact distance tie by the nest's delivery position, not the
                    // nest query order (unstable across same-seed runs — see `util::nearest_planar`); the
                    // chosen nest sets the haul destination and steers the whole crew.
                    let mut best: Option<(f32, Vec3)> = None;
                    for (ne, _ntf, nest) in nests.iter() {
                        let d = (nest.pos - gtf.translation).with_y(0.0).length();
                        let better = match best {
                            None => true,
                            Some((bd, bp)) => {
                                (d.to_bits(), nest.pos.x.to_bits(), nest.pos.z.to_bits())
                                    < (bd.to_bits(), bp.x.to_bits(), bp.z.to_bits())
                            }
                        };
                        if better {
                            best = Some((d, nest.pos));
                            dest = Some(ne);
                        }
                    }
                }

                if let Some(nest_e) = dest {
                    // --- LIFT (Dynamic → Kinematic) ---
                    if crate::ai::diag::AI_DIAG {
                        let crew = carry
                            .carriers
                            .iter()
                            .filter(|&&c| {
                                crabs
                                    .get(c)
                                    .map(|(ctf, _)| {
                                        ctf.translation.xz().distance(gtf.translation.xz())
                                            <= bc.grab_range
                                    })
                                    .unwrap_or(false)
                            })
                            .count();
                        info!(
                            "carry: LIFT weight={:.2} crew={crew} cap_here={cap_here:.2}",
                            carry.weight
                        );
                    }
                    carry.phase = crate::gore::CarryPhase::Hauling;
                    carry.crew_timer = 0.0;
                    commands.entity(ge).insert(RigidBody::Kinematic);
                    lv.0 = Vec3::ZERO;
                    av.0 = Vec3::ZERO;
                    carry.nest = Some(nest_e);
                    for &c in &carry.carriers {
                        if let Ok((_, mut cc)) = crabs.get_mut(c) {
                            cc.hauling = true;
                        }
                    }
                } else if has_crew {
                    // Crewing: not enough gathered capacity yet, OR nowhere to deliver. Keep gathering;
                    // disband a crew that waits past bc.crew_timeout without lifting.
                    carry.phase = crate::gore::CarryPhase::Crewing;
                    carry.crew_timer += dt;
                    if carry.crew_timer >= bc.crew_timeout {
                        // Disband a crew that has waited too long without lifting.
                        for &c in &carry.carriers {
                            if let Ok((_, mut cc)) = crabs.get_mut(c) {
                                cc.target = None;
                                cc.hauling = false;
                            }
                        }
                        carry.carriers.clear();
                        carry.phase = crate::gore::CarryPhase::Resting;
                        carry.crew_timer = 0.0;
                    }
                } else {
                    carry.phase = crate::gore::CarryPhase::Resting;
                    carry.crew_timer = 0.0;
                }
            }
            crate::gore::CarryPhase::Hauling => {
                // Destination = the nest's floor delivery point + its walkway flow field (if it still
                // exists; a razed nest yields None → the haul aborts and the chunk drops).
                let nest_nav: Option<(Vec3, Arc<crate::flowfield::FlowField>)> = carry
                    .nest
                    .and_then(|n| nests.get(n).ok())
                    .map(|(_, _, nest)| (nest.pos, nest.flow.clone()));

                if nest_nav.is_none() {
                    // --- NEST RAZED MID-HAUL: full release (Kinematic → Dynamic) ---
                    // The destination nest no longer resolves, so there's nowhere to deliver. Drop the
                    // load and fully release the crew (clear carriers, back to Resting) — mirroring the
                    // bc.crew_timeout disband — instead of dropping to a Crewing limbo that can never
                    // re-lift (the LIFT gate now refuses a nestless chunk) and would only sit emitting
                    // MEAT scent until it timed out.
                    commands.entity(ge).insert(RigidBody::Dynamic);
                    for &c in &carry.carriers {
                        if let Ok((_, mut cc)) = crabs.get_mut(c) {
                            cc.target = None;
                            cc.hauling = false;
                        }
                    }
                    carry.carriers.clear();
                    carry.phase = crate::gore::CarryPhase::Resting;
                    carry.crew_timer = 0.0;
                    carry.nest = None;
                } else if cap_total < carry.weight {
                    // --- ABORT / DROP (Kinematic → Dynamic): crew shrank below liftable capacity ---
                    commands.entity(ge).insert(RigidBody::Dynamic);
                    carry.phase = crate::gore::CarryPhase::Crewing;
                    carry.crew_timer = 0.0;
                    for &c in &carry.carriers {
                        if let Ok((_, mut cc)) = crabs.get_mut(c) {
                            cc.hauling = false;
                        }
                    }
                } else if let Some((npos, flow)) = nest_nav {
                    let horiz = (npos - gtf.translation).with_y(0.0);
                    if horiz.length() <= bc.deliver_range {
                        // --- DELIVER (Kinematic → despawn) --- deferred to the sorted pass below; the
                        // hoard/boost accumulate and the despawn are the order-sensitive parts.
                        if let Some(n) = carry.nest {
                            delivered.push((gib_key.0, ge, n, carry.weight));
                        }
                        // Releasing this chunk's own carriers is order-independent (a crab hauls exactly
                        // one chunk), so it stays here.
                        for &c in &carry.carriers {
                            if let Ok((_, mut cc)) = crabs.get_mut(c) {
                                cc.target = None;
                                cc.hauling = false;
                            }
                        }
                    } else {
                        // Haul along the nest's flow field so the chunk threads walkways instead of
                        // beelining through walls (Item #3). Speed scales up with crew and down with
                        // weight (Item #6): a heavy chunk on a bare crew crawls; more carriers speed it.
                        let steer = flow.steer(&dungeon, gtf.translation);
                        let mut dir = Vec3::new(steer.x, 0.0, steer.y);
                        if dir.length_squared() <= 1.0e-6 {
                            dir = horiz; // at/near the goal cell but still outside bc.deliver_range: close in
                        }
                        let crew = carry.carriers.len() as f32;
                        let speed = (bc.carry_speed * crew / (carry.weight * bc.weight_drag))
                            .clamp(bc.carry_speed * 0.35, bc.carry_speed);
                        // Wall-confine the haul step. The flow field already threads walkways, but the
                        // `dir = horiz` straight-line fallback above (taken when the steer is ~0 right
                        // next to the wall-mounted nest) has no wall backstop, so a hauled chunk could
                        // be dragged through the wall/corner onto the void floor. Sweep the horizontal
                        // move against room walls with the same `resolve_move` the Dynamic path uses in
                        // `gore::confine_gibs`, so the chunk stops at the room-side wall face instead.
                        let step = dir.normalize_or_zero() * (speed * dt);
                        let resolved = dungeon.resolve_move(
                            gtf.translation.with_y(0.0),
                            Vec3::new(step.x, 0.0, step.z),
                            Vec2::splat(crate::gore::GIB_CONFINE_HALF),
                        );
                        gtf.translation.x = resolved.x;
                        gtf.translation.z = resolved.z;
                        gtf.translation.y = CARRY_HEIGHT; // ride at the crew's mouth height (Item #2)
                        lv.0 = Vec3::ZERO;
                        av.0 = Vec3::ZERO;
                    }
                }
            }
        }
    }

    // Apply the deliveries in `GibKey` order — the shared-nest accumulate and the despawn, i.e. exactly the
    // parts the raw gib query order was deciding. `GibKey` is unique by construction (it mixes a monotonic
    // `GibSeq`; before that fix it was derived from the death origin and COLLIDED for two creatures dying on
    // one coordinate — that was G0c), so this is a genuine total order and `sort_total!` proves it.
    crate::sort_total!(&mut delivered, |&(key, ..): &(u64, Entity, Entity, f32)| key);
    for (_, ge, n, weight) in delivered {
        if let Ok((_, _, mut nest)) = nests.get_mut(n) {
            nest.hoard += weight;
            // Feeding surge: heavier chunks accelerate births more, up to ~10×.
            nest.spawn_boost =
                (nest.spawn_boost + weight * sim.breeding.feed_gain).min(sim.breeding.spawn_boost_max);
        }
        // The ONE early-removal path (drops the id from the ring, then despawns).
        gib_ring.consume(&mut commands, ge);
    }
}

/// Each crab lays into two channels, both at a per-second rate ≈ the channel's evaporation, so each
/// cell's value tracks the local crab count:
///
/// - CRAB_DENSITY — the swarm's own crowding/recruitment substrate (read by `nest_reproduce`).
/// - THREAT_CRAB — the menace the swarm radiates, read as FEAR by the *squad* (never by crabs; see
///   `ai::faction`). Separate from density because dread wants a wider radius and slower decay than
///   crowding, and because the two must be tunable apart.
///
/// Determinism: overlapping discs accumulate into shared cells and float `+=` is non-associative, so a
/// deposit emitted in entity-iteration order would make the summed field depend on that order (which can
/// shift between same-seed runs). Emit in a stable *position* order, exactly as `deposit_meat_scent` does.
fn deposit_crab_fields(
    time: Res<Time>,
    crabs: Query<&Transform, With<Crab>>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    sim: Res<SimTuning>,
) {
    let dt = time.delta_secs();
    let density = sim.deposit.crab_density_rate * dt;
    let menace = sim.deposit.crab_menace_rate * dt;
    let mut positions: Vec<Vec3> = crabs.iter().map(|tf| tf.translation).collect();
    // SORT-OK: bare positions — the position IS the whole value, so a tie means two identical deposits,
    // which contribute identical terms to the same sum. Interchangeable.
    positions.sort_unstable_by_key(|p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    for pos in positions {
        deposits.0.push(crate::ai::field::Deposit {
            pos,
            field: crate::ai::field::FieldId::CRAB_DENSITY,
            amount: density,
        });
        deposits.0.push(crate::ai::field::Deposit {
            pos,
            field: crate::ai::field::FieldId::THREAT_CRAB,
            amount: menace,
        });
    }
}

/// Each *resting/crewing* meat gib lays into the MEAT field, so foraging crabs sense a pile from a
/// distance and climb its gradient (ACO-style trail-following; Dorigo). A gib that is already lifted and
/// being hauled is skipped — otherwise it drags the moving MEAT hotspot (the SeekMeat target) toward the
/// nest, pulling uncommitted foragers onto an already-crewed chunk instead of dispersing to fresh piles.
fn deposit_meat_scent(
    time: Res<Time>,
    gibs: Query<(&Transform, &crate::gore::Carryable)>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    sim: Res<SimTuning>,
) {
    let amount = sim.deposit.meat_rate * time.delta_secs();
    // Determinism: gibs enumerate in unstable entity order (see `util::nearest_planar`). Each MEAT
    // deposit spreads over a disc, so overlapping chunks accumulate into shared field cells; pushing
    // them in enumeration order makes the summed gradient depend on that order (float `+=` is
    // non-associative), drifting swarm steering enough to break the replay hash. Emit in a stable
    // position order so the MEAT field depends only on WHERE the chunks are.
    let mut positions: Vec<Vec3> = gibs
        .iter()
        .filter(|(_, carry)| carry.phase != crate::gore::CarryPhase::Hauling)
        .map(|(tf, _)| tf.translation)
        .collect();
    // SORT-OK: bare positions — the position IS the whole value, so a tie means two identical deposits,
    // which contribute identical terms to the same sum. Interchangeable.
    positions.sort_unstable_by_key(|p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    for pos in positions {
        deposits.0.push(crate::ai::field::Deposit {
            pos,
            field: crate::ai::field::FieldId::MEAT,
            amount,
        });
    }
}

/// Scout recon + recruitment. Scouts roam to find prey (minimalist-agent foraging; Talamali, Bose, Haire,
/// Xu, Marshall & Reina, "Sophisticated collective foraging with minimalist agents", Swarm Intelligence
/// 2019, DOI 10.1007/s11721-019-00176-9) and
/// mark it with the **vectorial rally pheromone** (Tang, Xu, Yu, Zhang & Zhang, "Dynamic target searching
/// and tracking with swarm robots based on stigmergy", Robotics & Autonomous Systems 2019): while a scout
/// senses prey it deposits a direction vector (an "intermediate-vector") pointing at the prey's live
/// position, so the map continuously encodes the bearing to the moving target rather than a stale scalar
/// at where the prey once was. Runs before the rally deposits drain so the beacon is live this frame and
/// `think` reads a fresh Scout state:
/// - **Roaming → Tracking**: on sensing prey within `bc.scout_sight` (planar), lock onto the nearest.
/// - **Tracking**: refresh the tracked prey and, throttled by `bc.rally_deposit_cooldown`, deposit a vector
///   toward it (strength eases with proximity). Losing sight drops back to Roaming; the pheromone then
///   evaporates on its own — the automatic "call off the attack".
fn scout_mark_prey(
    time: Res<Time>,
    mut scouts: Query<(&Transform, &mut Scout)>,
    prey: Query<&Transform, With<Prey>>,
    mut deposits: ResMut<crate::ai::field::RallyDeposits>,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let dt = time.delta_secs();
    let bc = &beh.crab;
    // Rally marks are collected and sorted before queueing — the same idiom every SCALAR deposit producer
    // uses (`nest_alarm`, `crab_alarm_on_damage`, `deposit_crab_fields`, …), which this site never got
    // because `RallyDeposits` had no `sort_*` helper to call: `sort_deposits` is typed `&mut [Deposit]`.
    // The scout query order is not stable across `App` instances, and `RallyField::deposit` accumulates with
    // a non-associative `Vec2 +=` over a `deposit_radius`-wide disc, so two scouts within ~2·radius write
    // the same cells on one tick and raw push order set the low bits of that cell's vector. That is not
    // cosmetic: `re_role_crabs` gates a caste flip on `rally.sample(..).length() > bc.rally_live`, and while
    // the authored 0.15 sits clear of the field's noise floor, the genome's lower bound is **0.02** — right
    // on it. See `field::sort_rally_deposits`.
    let mut batch: Vec<crate::ai::field::RallyDeposit> = Vec::new();
    for (tf, mut scout) in &mut scouts {
        let pos = tf.translation;
        scout.report_cooldown = (scout.report_cooldown - dt).max(0.0);

        // Nearest prey on the ground plane, within sight (the shared ranking; payload is unit `()`).
        let hit = crate::util::nearest_planar(pos, prey.iter().map(|pt| ((), pt.translation)));
        match hit.filter(|(_, _, d)| *d <= bc.scout_sight) {
            Some(((), prey_pos, best)) => {
                scout.state = ScoutState::Tracking { prey_pos };
                // Deposit an intermediate-vector pointing at the prey (Tang's `s`), throttled so a cell
                // isn't saturated frame-by-frame. Strength eases with proximity so nearer marks weigh more.
                if scout.report_cooldown <= 0.0
                    && let Some(dir) = (prey_pos.xz() - pos.xz()).try_normalize()
                {
                    let strength =
                        sim.deposit.rally_mark * ((bc.scout_sight - best) / bc.scout_sight).clamp(0.0, 1.0);
                    batch.push(crate::ai::field::RallyDeposit { pos, vec: dir * strength });
                    scout.report_cooldown = bc.rally_deposit_cooldown;
                    if crate::ai::diag::AI_DIAG {
                        info!("scout: MARK prey@{:?} from@{:?}", prey_pos.xz(), pos.xz());
                    }
                }
            }
            None => {
                // Lost the prey — resume roaming; the pheromone evaporates on its own (call-off).
                scout.state = ScoutState::Roaming;
            }
        }
    }
    crate::ai::field::sort_rally_deposits(&mut batch);
    deposits.0.extend(batch);
}

/// Meat-fuelled breeding — the ONE reproduction path. A nest births a crab only when it has hoarded at
/// least `MEAT_PER_CRAB` of delivered meat AND its `NEST_RESPAWN_INTERVAL` rate-limiter has elapsed; each
/// birth *spends* that meat, so the forage→haul→deliver economy is the sole source of reinforcements —
/// starve the swarm (destroy its gibs) and the hoard drains and births stop. Also capped by
/// `CRAB_COUNT_MAX` and suppressed while the nest cell is crowded (local CRAB_DENSITY high) so births
/// don't pile onto births. Newborns seat on the nest's floor delivery cell; `spawn_boost` (from heavier
/// deliveries) shortens the interval for a well-fed nest.
fn nest_reproduce(
    time: Res<Time>,
    mut commands: Commands,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    stig: Res<crate::ai::field::Stig>,
    crab_assets: Option<Res<CrabAssets>>,
    mut nests: Query<(Entity, &mut crate::nest::Nest)>,
    crabs: Query<(), With<Crab>>,
    mut seq: ResMut<CrabSpawnSeq>,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let (Some(graph), Some(crab_assets)) = (graph, crab_assets) else {
        return;
    };
    let dt = time.delta_secs();
    let mut total = crabs.iter().count();

    // CANONICAL ORDER — load-bearing, and the same class of bug as `laser::fire_laser`'s shared aim draw.
    // This loop is greedy and stateful over TWO pieces of shared state, so the order the nests are visited
    // in is part of the result:
    //   * `seq` is a SHARED monotonic counter, and `CrabSpawnSeq`'s own doc spells out what rides on it —
    //     "scout/assault role, think-stagger, jump cadence, carry capacity, climb/angle biases, RNG". When
    //     two nests breed on the same tick, raw query order decided WHICH nest's newborn got seed N and
    //     which got N+1, so a crab's caste and capacity flipped between two same-seed runs.
    //   * `total` gates on `crab_count_max`, so at the cap the visit order decides WHICH nest gets the last
    //     slot — a keep-the-first-on-a-tie pick.
    // Nest query order is NOT stable across `App` instances (`sim_harness::nest_cells` was canonicalised
    // for exactly this reason; the breeding loop itself never was). `nest.pos` is assigned at spawn and
    // immortal, so its bits are a stable, total key — the `world_to_cell` quantisation is deliberately NOT
    // used here: two nests can share a cell.
    let mut order: Vec<(u32, u32, u32, Entity)> = nests
        .iter()
        .map(|(e, n)| (n.pos.x.to_bits(), n.pos.y.to_bits(), n.pos.z.to_bits(), e))
        .collect();
    crate::sort_total!(&mut order, |k: &(u32, u32, u32, Entity)| (k.0, k.1, k.2));

    for (.., nest_e) in order {
        let Ok((_, mut nest)) = nests.get_mut(nest_e) else {
            continue;
        };
        // Fade the feeding surge, then re-arm the next spawn at the boosted rate (up to 10× faster).
        nest.spawn_boost = (nest.spawn_boost - sim.breeding.spawn_boost_decay * dt).max(0.0);
        nest.respawn_timer -= dt;
        if nest.respawn_timer > 0.0 {
            continue;
        }
        // Effective rate = 1 + spawn_boost (SPAWN_BOOST_MAX ⇒ ~10× faster). Re-arm even if this tick
        // can't spawn (cap/crowd), so a fed nest keeps its fast cadence.
        nest.respawn_timer = sim.breeding.respawn_interval / (1.0 + nest.spawn_boost);

        if total >= sim.breeding.crab_count_max {
            continue;
        }
        // Meat gate: breeding both requires and consumes hoarded meat. No hoard → no birth, so cutting
        // off the swarm's food halts reinforcements (the economy's one lever).
        if nest.hoard < sim.breeding.meat_per_crab {
            continue;
        }
        // Don't pile births onto a crowded nest cell (territorial self-limiting).
        let density = stig.sample(crate::ai::field::FieldId::CRAB_DENSITY, &dungeon, nest.pos);
        if density >= sim.breeding.crowd_cap {
            continue;
        }
        let Some(patch) = graph.floor_patch_cell(dungeon.world_to_cell(nest.pos)) else {
            continue; // nest's delivery cell isn't floor — can't seat a newborn here
        };
        nest.hoard -= sim.breeding.meat_per_crab; // spend the meat this birth cost
        let s = seq.0 as u32;
        seq.0 += 1;
        spawn_crab_on_patch(
            &mut commands,
            &graph,
            patch,
            &crab_assets.collider,
            &crab_assets.scene,
            s,
            &sim,
            beh.crab,
        );
        total += 1;
        if crate::ai::diag::AI_DIAG {
            info!("nest: RESPAWN total={total}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dungeon::{Dungeon, TILE_SIZE, WALL_THICKNESS};
    use crate::surface_nav::SurfaceGraph;
    use crate::wfc::{E, N, S, W};

    #[test]
    fn caste_policy_promotes_and_demotes_on_the_right_signals() {
        // The caste thresholds are now config (behavior.crab); test against the shipped defaults.
        let bc = crate::behavior_tuning::BehaviorTuning::default().crab;
        let (ah, pd) = (bc.alarm_high, bc.promote_density);
        // Assault crab, crowded, no beacon → promote to scout (spare a body for recon).
        assert_eq!(
            caste_decision(false, false, pd + 1.0, 0.0, ah, pd),
            Rerole::Promote
        );
        // Assault crab but a beacon is live → hold (the swarm is already converging; stay a fighter).
        assert_eq!(
            caste_decision(false, true, pd + 1.0, 0.0, ah, pd),
            Rerole::Hold
        );
        // Assault crab, uncrowded → hold.
        assert_eq!(caste_decision(false, false, 0.0, 0.0, ah, pd), Rerole::Hold);
        // Scout with a live beacon → demote (recon done, become a fighter).
        assert_eq!(caste_decision(true, true, 0.0, 0.0, ah, pd), Rerole::Demote);
        // Scout under alarm → demote (defense press).
        assert_eq!(caste_decision(true, false, 0.0, ah + 0.1, ah, pd), Rerole::Demote);
        // Scout, calm, no beacon → hold (keep ranging).
        assert_eq!(caste_decision(true, false, 0.0, 0.0, ah, pd), Rerole::Hold);
    }

    /// The wall's inner (room-facing) face, from cell centre — the ground-truth threshold
    /// `Dungeon::is_solid` uses. A crab body must stay short of this by its radius.
    fn wall_inner_face() -> f32 {
        0.5 * TILE_SIZE - WALL_THICKNESS
    }

    /// Build a `Dungeon` whose only floor cells are `floors` (all else rock), on a `w×h` grid.
    fn dungeon_with(w: usize, h: usize, floors: &[IVec2]) -> Dungeon {
        let mut mask = vec![false; w * h];
        for c in floors {
            mask[c.y as usize * w + c.x as usize] = true;
        }
        Dungeon::from_walkable(w, h, mask)
    }

    /// A crab clamped onto a floor patch — even shoved hard past every walled edge — must never
    /// end up with its body inside a bounding wall slab. This is the exact bug that was reported
    /// (crabs clipping into walls) and the invariant the per-edge floor-patch inset restores.
    #[test]
    fn crab_cannot_be_clamped_into_a_wall() {
        // A single floor cell at (1,1) surrounded by rock ⇒ walled on all four edges.
        let cell = IVec2::new(1, 1);
        let dungeon = dungeon_with(3, 3, &[cell]);
        for dir in [N, E, S, W] {
            assert!(dungeon.walled(cell, dir), "fixture must wall every edge of the cell");
        }

        let graph = SurfaceGraph::build(&dungeon);
        let patch_id = graph
            .floor_patch_cell(cell)
            .expect("center cell must have a floor patch");
        let patch = graph.patch(patch_id);
        let center = dungeon.cell_center(cell);
        let r = CRAB_COLLIDER_R;

        // Sanity: the walls really are there — the FULL-tile corner (what the old, un-inset clamp
        // permitted) sits inside a wall slab. This is precisely what used to let crabs clip.
        assert!(
            dungeon.is_solid_test(center.x + 0.5 * TILE_SIZE, center.z),
            "the tile edge is inside a wall — the old full-tile clamp would embed the crab here"
        );

        // Shove a crab far past each edge (and past each corner), clamp, and assert its whole
        // body footprint (centre ± radius on X and Z) stays out of solid geometry.
        let far = 5.0;
        let pushes = [
            Vec3::new(far, 0.0, 0.0),
            Vec3::new(-far, 0.0, 0.0),
            Vec3::new(0.0, 0.0, far),
            Vec3::new(0.0, 0.0, -far),
            Vec3::new(far, 0.0, far),
            Vec3::new(-far, 0.0, far),
            Vec3::new(far, 0.0, -far),
            Vec3::new(-far, 0.0, -far),
        ];
        for push in pushes {
            let clamped = clamp_to_patch(center + push, patch);
            for (dx, dz) in [(0.0, 0.0), (r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
                assert!(
                    !dungeon.is_solid_test(clamped.x + dx, clamped.z + dz),
                    "crab body clipped a wall: push={push:?} clamped={clamped:?} offset=({dx},{dz})"
                );
            }
        }
    }

    /// The inset must apply ONLY to walled edges. An OPEN edge (floor neighbour) keeps the full
    /// half-tile so a crab can still reach the floor↔floor transfer gate at the cell boundary —
    /// insetting it would strand the crab and break pursuit. Uses a straight corridor so the
    /// middle cell is open E/W and walled N/S.
    #[test]
    fn open_edges_keep_full_extent_walled_edges_inset() {
        let cells = [IVec2::new(0, 1), IVec2::new(1, 1), IVec2::new(2, 1)];
        let mid = cells[1];
        let dungeon = dungeon_with(3, 3, &cells);
        assert!(!dungeon.walled(mid, E) && !dungeon.walled(mid, W), "E/W must be open");
        assert!(dungeon.walled(mid, N) && dungeon.walled(mid, S), "N/S must be walled");

        let graph = SurfaceGraph::build(&dungeon);
        let patch = graph.patch(graph.floor_patch_cell(mid).expect("floor patch"));
        let center = dungeon.cell_center(mid);

        // Open edges (±X): the crab reaches the full ±0.5 boundary (the transfer gate location).
        let east = clamp_to_patch(center + Vec3::new(5.0, 0.0, 0.0), patch);
        let west = clamp_to_patch(center + Vec3::new(-5.0, 0.0, 0.0), patch);
        assert!(
            (east.x - center.x - 0.5 * TILE_SIZE).abs() < 1e-5,
            "open E edge must keep full half-tile extent, got {}",
            east.x - center.x
        );
        assert!(
            (west.x - center.x + 0.5 * TILE_SIZE).abs() < 1e-5,
            "open W edge must keep full half-tile extent, got {}",
            west.x - center.x
        );

        // Walled edges (±Z): the crab is held short of the wall inner face by its radius.
        let bound = wall_inner_face() - CRAB_COLLIDER_R;
        let south = clamp_to_patch(center + Vec3::new(0.0, 0.0, 5.0), patch);
        let north = clamp_to_patch(center + Vec3::new(0.0, 0.0, -5.0), patch);
        assert!(
            (south.z - center.z - bound).abs() < 1e-5,
            "walled S edge must inset to inner_face - radius, got {}",
            south.z - center.z
        );
        assert!(
            (north.z - center.z + bound).abs() < 1e-5,
            "walled N edge must inset to inner_face - radius, got {}",
            north.z - center.z
        );
        // And that inset genuinely keeps the body off the slab.
        assert!(!dungeon.is_solid_test(south.x, south.z + CRAB_COLLIDER_R));
        assert!(!dungeon.is_solid_test(north.x, north.z - CRAB_COLLIDER_R));
    }
}
