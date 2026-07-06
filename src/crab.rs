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
use crate::health::{Health, NoHealthBar};
use crate::squad::{Prey, Unit};
use crate::surface_nav::{SurfaceField, SurfaceGraph};
use crate::util::{hash01_u32, rand01};

/// Total crabs across the level, split into `CRAB_CLUSTERS` nests in far rooms.
const CRAB_COUNT: usize = 40;
const CRAB_CLUSTERS: usize = 4;
/// The first this-many clusters are seeded directly on wall faces, so wall-climbing is always visible
/// (the rest start on the floor and mount walls opportunistically as the field pulls them).
const CRAB_WALL_CLUSTERS: usize = 2;
/// Nests spawn at least this far (tiles) from the squad spawn, and clusters at least this far apart.
const CRAB_MIN_SPAWN_DIST: f32 = 8.0;
const CRAB_CLUSTER_SEP: f32 = 5.0;

/// Weak individually: 1–2 laser hits (see `laser::LASER_DAMAGE`).
const CRAB_HP: f32 = 25.0;
/// Base bite DPS, scaled up **super-linearly** by how many crabs pile on one unit (see
/// `crab_contact_damage`): total = `CRAB_CONTACT_DPS * count^DAMAGE_EXPONENT`. One crab is a nuisance
/// (~3 DPS, ~33 s to kill), but a pile becomes a feeding frenzy — the more crabs, the faster it climbs.
const CRAB_CONTACT_DPS: f32 = 3.0;
/// Growth curve for stacked bites — >1 makes a pile hurt disproportionately more than linear stacking
/// (≈33 DPS at 5 crabs, ≈95 at 10, ≈270 at 20). Tune this to taste.
const DAMAGE_EXPONENT: f32 = 1.5;
/// Planar bite margin beyond the unit's body radius — a crab this close (in XZ) to a unit is feeding.
const CRAB_CONTACT_RADIUS: f32 = 0.2;
/// Critical mass: this many crabs must be biting one target before the swarm deals ANY damage. Below it
/// the swarm is milling/building up (it can't overcome the squad); at/above it the super-linear bite
/// kicks in. A nest under attack (berserk) waives this. Playtest dial for how big a swarm must gather.
const MASS_MIN: usize = 5;
/// Pounce tuning. A crab lunges at a unit within `JUMP_LEN` (≈10 body lengths; a crab renders ~0.19 wide,
/// so ~1.9 world units) but not one already in its face (`JUMP_MIN`). It hunkers for `JUMP_HUNKER`s, then
/// arcs over `JUMP_AIR`s to `JUMP_ARC` peak height, biting `JUMP_DAMAGE` on landing, then waits out
/// `JUMP_COOLDOWN` before pouncing again.
const JUMP_LEN: f32 = 1.9;
/// Don't pounce at a unit already in biting range — only commit to a real leap (~6–10 body lengths),
/// so the jump reads as a dramatic lunge rather than a tiny hop.
const JUMP_MIN: f32 = 1.15;
const JUMP_HUNKER: f32 = 0.3;
const JUMP_AIR: f32 = 0.35;
const JUMP_ARC: f32 = 0.9;
const JUMP_COOLDOWN: f32 = 2.5;
const JUMP_DAMAGE: f32 = 8.0;

/// Scuttle speed along the surface (world units/s). The base pace every crab movement mode scales off
/// (scout roam ×`SCOUT_SPEED_MUL`, flee ×`FLEE_SPEED_MUL`, rally/seek at 1×), so tuning it slows the
/// whole swarm uniformly.
const CRAB_SPEED: f32 = 2.1; // 3/4 of the earlier 2.8 — a calmer, less frantic scuttle
/// Uniform render scale for the child model (native height ~3.06 → ~0.46 m ≈ 1.5 ft tall, sized to
/// the ~6 ft squad and 8 ft ceilings). Seat constants below scale with it.
const CRAB_RENDER_SCALE: f32 = 0.15;
/// Root body-centre height above the surface, along the surface normal (also seats the collider).
const CRAB_BODY_CENTER: f32 = 0.125;
/// Local Y offset of the scaled model under the root so its body rests on the surface (the glb origin
/// sits near the model's top). Calibrated by eye via devshot, scaled with `CRAB_RENDER_SCALE`.
const CRAB_MODEL_Y: f32 = 0.275;
/// Radius of the invisible collider sphere (the laser raycast target); world-size since the root is
/// unscaled. Sized to hug the *visible* crab (rendered span ≈0.19 → radius ≈0.1) so a bolt only draws
/// blood on a real hit — a near-miss now passes cleanly instead of registering on an oversized hitbox.
const CRAB_COLLIDER_R: f32 = 0.12;

/// Reynolds separation: crabs within this centre distance push apart, so the swarm actively spreads
/// out instead of stacking (≈2× the crab footprint → they hold a visible gap). Applied as a real
/// displacement (not just a steering nudge) so the spacing actually holds. Reynolds, "Steering
/// Behaviors For Autonomous Characters", GDC 1999.
const CRAB_SEP_RADIUS: f32 = 0.45;
const CRAB_SEP_STRENGTH: f32 = 7.0;

/// Per-crab path jitter: a small side-to-side wander perpendicular to the travel direction, so crabs
/// sharing one flow-field path don't converge into a single stacked line. Each crab's phase is offset
/// by its `angle_bias`, so they weave out of sync and fan across the corridor.
const CRAB_JITTER_STRENGTH: f32 = 0.6;
const CRAB_JITTER_FREQ: f32 = 2.3;

/// Scout recon tuning (the swarm's ~20% roaming recruiters; see [`Scout`] / `scout_sense_and_report`).
/// Fraction of crabs tagged as scouts at spawn (deterministic by spatial hash, so newborns split the
/// same way). ~1 in 5 gives recon coverage while leaving ~80% as the assault mass (`MASS_MIN`).
const SCOUT_FRACTION: f32 = 0.20;
/// A roaming scout "spots" any prey within this planar range (world units) — it then tracks and marks it.
const SCOUT_SIGHT: f32 = 5.0;
/// Scouts roam faster than the swarm forages (aggressive ranging to cover ground).
const SCOUT_SPEED_MUL: f32 = 1.35;
/// Seconds a roaming scout holds a wander heading before re-rolling it (mirrors `enemy::WANDER_INTERVAL`,
/// but longer — a scout commits to a direction to actually cover distance rather than jitter in place).
const SCOUT_WANDER_INTERVAL: f32 = 3.0;
/// Minimum seconds between a marking scout's rally-pheromone deposits. Keeps the beacon tracking the
/// moving prey (a vectorial pheromone; Tang et al. 2019) without saturating the field frame-by-frame.
const RALLY_DEPOSIT_COOLDOWN: f32 = 0.2;
/// Strength of each rally-pheromone deposit (the intermediate-vector magnitude before accumulation).
const RALLY_MARK_STRENGTH: f32 = 4.0;

/// The unit's body approximated as a vertical cylinder the crabs cling to (radius, climbable height).
const UNIT_BODY_RADIUS: f32 = 0.33;
const UNIT_BODY_HEIGHT: f32 = 1.0;
/// Speed while climbing onto / crawling over a unit's body.
const CRAB_CLIMB_SPEED: f32 = 2.89; // 3/4 of the earlier 3.85 — matches the slower scuttle
/// Distance to its chosen body slot at which a latched crab counts as "on" and biting (a bit generous
/// so it plays the attack animation while feeding, not just at the exact point).
const EAT_RANGE: f32 = 0.3;
/// Latched crabs prefer the unit's BACK (where the host's own gun can't reach), fanned across this
/// arc (radians) via a per-crab bias; separation pushes the overflow toward the sides/front when the
/// rear is full. The slot is body-relative, so a crab rides along as its host turns and walks.
const BACK_SPREAD: f32 = 2.6;
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
    /// Per-crab preferred angular slot `[0,1]` around the body (mapped across `BACK_SPREAD`).
    angle_bias: f32,
    /// Whether the crab is currently latched onto a unit (vs. free-roaming the floor/walls).
    latched: bool,
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

/// A crab's pounce state: crabs lunge ~`JUMP_LEN` (≈10 body lengths) at a nearby unit, hunkering down
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

/// Marks the ~[`SCOUT_FRACTION`] of crabs that are scouts (recon recruiters). Holds the roam/track state
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
            .add_systems(
                Update,
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
                    // Move after the brain has chosen this frame's mode (see `crate::ai`).
                    crab_locomotion
                        .after(rebuild_crab_field)
                        .after(crate::ai::AiSet::Think),
                    // Pounce owns the transform of mid-jump crabs; runs after locomotion set the rest.
                    crab_jump.after(crab_locomotion),
                    // Cooperative lift/haul/deliver — runs after crabs have moved and any fleer released.
                    carry_gibs
                        .after(crab_locomotion)
                        .after(assign_meat_targets),
                    attach_crab_animation,
                    drive_crab_animation,
                    crab_contact_damage,
                    crab_despawn_dead,
                    deposit_crab_density,
                    deposit_meat_scent,
                    // Scout detection + rally-pheromone deposit: before the rally deposits drain so the
                    // beacon is live this frame, and before Think so the tracking state feeds the scout brain.
                    scout_mark_prey.before(crate::ai::AiSet::Deposits),
                    nest_reproduce,
                ),
            );
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
                        !crate::dungeon::SHORT_CAMERA_WALLS || (n != Vec3::NEG_X && n != Vec3::NEG_Z)
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
                spawn_crab_on_patch(&mut commands, &graph, patch, &collider, &scene, s);
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

    // ~SCOUT_FRACTION of crabs are scouts (recon recruiters); the rest run the assault brain. One path —
    // a plain conditional, no fallback.
    let is_scout = draw(1) < SCOUT_FRACTION;
    let brain_id = if is_scout {
        crate::ai::brain::BrainId::Scout
    } else {
        crate::ai::brain::BrainId::Crab
    };

    let mut ec = commands.spawn((
            Crab,
            Hostile,
            Health::new(CRAB_HP),
            NoHealthBar, // swarm chaff: no floating bar (40 would bury the screen)
            crate::ai::drives::Drives::new(), // needs the utility brain weighs (hunger/fear/…)
            brain_id,
            crate::ai::brain::ActiveBehavior::new(rand_seed),
            crate::ai::brain::ThinkTimer::staggered(rand_seed),
            // Grouped so the spawn tuple stays within Bevy's 15-element Bundle limit.
            (
                CrabAttached { host: None },
                CrabCarry {
                    capacity: CRAB_CARRY_CAPACITY * (0.8 + 0.4 * draw(2)),
                    target: None,
                    hauling: false,
                },
                CrabJump {
                    phase: JumpPhase::Ready,
                    timer: 0.0,
                    // Stagger initial cooldowns by seed so a fresh cluster doesn't pounce in lockstep.
                    cooldown: JUMP_COOLDOWN * draw(3),
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
                latched: false,
                latch_rel: 0.0,
            },
            CrabState::Idle,
            Mesh3d(collider.clone()),
            Transform::from_translation(seat).with_rotation(surface_orientation(heading, normal)),
            Visibility::Inherited,
        ));
    ec.with_child((
        WorldAssetRoot(scene.clone()),
        Transform::from_translation(Vec3::Y * CRAB_MODEL_Y).with_scale(Vec3::splat(CRAB_RENDER_SCALE)),
    ));
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

    let mut cells: Vec<IVec2> = units
        .iter()
        .map(|t| dungeon.world_to_cell(t.translation))
        .collect();
    cells.sort_by(|a, b| (a.x, a.y).cmp(&(b.x, b.y)));
    cells.dedup();

    if cells == crab_field.last_cells && crab_field.field.is_some() {
        return;
    }

    let sources: Vec<u32> = cells
        .iter()
        .filter_map(|&c| graph.floor_patch_cell(c))
        .collect();
    crab_field.field = SurfaceField::build(&graph, &sources).map(Arc::new);
    crab_field.last_cells = cells;
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
        ),
        With<Crab>,
    >,
) {
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

    // Spatial hash of crab positions (3D) keyed by floor cell, for O(n·k) separation.
    let mut hash: HashMap<IVec2, Vec<Vec3>> = HashMap::new();
    for (motion, _, _, _, _, _, _, _) in &crabs {
        hash.entry(dungeon.world_to_cell(motion.pos))
            .or_default()
            .push(motion.pos);
    }

    for (mut motion, mut state, mut attached, mut transform, active, carry, jump, mut scout) in
        &mut crabs
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
                        if d > 1.0e-4 && d < CRAB_SEP_RADIUS {
                            sep += away / d * (CRAB_SEP_RADIUS - d);
                        }
                    }
                }
            }
        }

        // Nearest unit on the ground plane (the brain decides *whether* to latch; this is *which* unit).
        let (_ndist, nunit) = unit_data.iter().fold((f32::MAX, None), |(bd, bu), &(e, up, fwd)| {
            let d = (up.xz() - motion.pos.xz()).length();
            if d < bd {
                (d, Some((e, up, fwd)))
            } else {
                (bd, bu)
            }
        });
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
        let gib_pos = carry
            .and_then(|c| c.target)
            .and_then(|e| gibs.get(e).ok())
            .map(|t| t.translation);
        let want = if latching && let Some((host, u, fwd)) = nunit {
            // --- PIRANHA MODE: climb onto the unit and cover its body, biting from a free slot. ---
            {
                // On first latching, claim a body-relative slot: fanned across the unit's REAR (where
                // the host's own forward-firing gun can't reach), spread by this crab's `angle_bias`.
                // Held thereafter, so the crab clings to that spot and rides along as the host walks.
                if !motion.latched {
                    motion.latched = true;
                    motion.latch_rel = (motion.angle_bias - 0.5) * BACK_SPREAD;
                }
                attached.host = Some(host);

                // World cling direction = the unit's back rotated by the crab's body-relative slot.
                let back_angle = (-fwd.z).atan2(-fwd.x);
                let ang = back_angle + motion.latch_rel;
                let radial = Vec3::new(ang.cos(), 0.0, ang.sin());
                let slot_y = 0.1 + motion.climb_bias * (UNIT_BODY_HEIGHT - 0.1);
                let target = u + radial * UNIT_BODY_RADIUS + Vec3::Y * slot_y;

                let to = target - motion.pos;
                let move_vec = to.normalize_or_zero() * CRAB_CLIMB_SPEED + sep * CRAB_SEP_STRENGTH;
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

                if to.length() < EAT_RANGE {
                    CrabState::Attack
                } else {
                    CrabState::Walk
                }
            }
        } else if fleeing {
            // --- FLEE: panic away from the THREAT gradient (the frenzy→scatter payoff). ---
            motion.latched = false;
            attached.host = None;
            crab_flee(&mut motion, &stig, &dungeon, &graph, sep, dt, t)
        } else if rallying {
            // --- RALLY: mass on the scout's marked sighting by following the local rally-pheromone
            // vector (Tang et al. 2019) — it already points at the (moving) prey, no gradient needed. ---
            motion.latched = false;
            attached.host = None;
            crab_rally(&mut motion, &rally, &dungeon, &graph, sep, dt, t)
        } else if scouting && let Some(scout) = scout.as_deref_mut() {
            // --- SCOUT ROAM: aggressive wander across floor + walls hunting for prey to mark. ---
            motion.latched = false;
            attached.host = None;
            crab_scout_roam(&mut motion, scout, &graph, &dungeon, sep, dt, t)
        } else if marking {
            // --- MARK: track the spotted prey — approach its position so the scout stays in sensing
            // range and `scout_mark_prey` keeps laying the rally pheromone toward its live cell. No
            // final-approach snap (home = None); falls back to holding heading if the sighting is gone. ---
            motion.latched = false;
            attached.host = None;
            let prey_pos = active.target;
            let desired = prey_pos.map(|p| p - motion.pos).unwrap_or(motion.heading);
            if steer_surface(&mut motion, &graph, &dungeon, desired, None, CRAB_SPEED * SCOUT_SPEED_MUL, sep, dt, t) {
                CrabState::Walk
            } else {
                CrabState::Idle
            }
        } else if seeking {
            // --- SEEK MEAT: steer to the committed gib, or climb the MEAT gradient toward a pile. ---
            motion.latched = false;
            attached.host = None;
            // Coarse fallback = the MEAT hotspot the brain aimed at; a hauling carrier hugs its chunk.
            let coarse = active.target;
            let hauling = carry.is_some_and(|c| c.hauling);
            crab_seek_meat(
                &mut motion, &stig, &dungeon, &graph, gib_pos, coarse, hauling, sep, dt, t,
            )
        } else {
            // --- SURFACE MODE: shared flow-field pursuit across floor + walls. ---
            {
                motion.latched = false;
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
                let phase = now * CRAB_JITTER_FREQ + motion.angle_bias * std::f32::consts::TAU;
                let jitter = side * (phase.sin() * CRAB_JITTER_STRENGTH);
                let move_vec = tangent * CRAB_SPEED
                    + jitter
                    + project_tangent(sep, p.normal) * CRAB_SEP_STRENGTH;
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

/// Nearest prey position + planar distance to `pos` (read-only over the pounce system's prey query).
fn nearest_prey_pos(
    prey: &Query<(&Transform, &mut Health), (With<Prey>, Without<Crab>)>,
    pos: Vec3,
) -> Option<(Vec3, f32)> {
    let mut best: Option<(Vec3, f32)> = None;
    for (ptf, _) in prey.iter() {
        let d = (ptf.translation.xz() - pos.xz()).length();
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((ptf.translation, d));
        }
    }
    best
}

/// Pounce attack: a grounded, hunting crab hunkers down, then leaps a ballistic arc (~10 body lengths)
/// onto a nearby unit and bites on landing. While hunkering/airborne this owns the crab's transform
/// (`crab_locomotion` skips it); on landing it re-homes onto the surface and starts a cooldown. A short
/// wind-up + high peak reads as a real pounce, not a glide.
fn crab_jump(
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    alarm: Res<crate::nest::NestAlarm>,
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
) {
    let Some(graph) = graph else { return };
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    let berserk = alarm.0 > 0.0;
    // Planar positions of every crab this frame, for the pounce-landing critical-mass check below
    // (immutable borrow of the same query before the mutable pass — the pouncer is included, matching
    // how `crab_contact_damage` counts). Mirrors that system's gate so a lone leaper can't chip a unit.
    let crab_cells: Vec<Vec2> = crabs.iter().map(|(m, _, _, _, _)| m.pos.xz()).collect();

    for (mut motion, mut state, mut jump, mut tf, active) in &mut crabs {
        match jump.phase {
            JumpPhase::Ready => {
                jump.cooldown = (jump.cooldown - dt).max(0.0);
                if jump.cooldown > 0.0 {
                    continue;
                }
                // Only pounce while hunting units (approaching prey), and only at a unit in the band.
                let aggressive = matches!(
                    active.mode,
                    crate::ai::utility::Mode::Latch | crate::ai::utility::Mode::Forage
                );
                if !aggressive {
                    continue;
                }
                if let Some((tpos, d)) = nearest_prey_pos(&prey, motion.pos) {
                    if d > JUMP_MIN && d < JUMP_LEN {
                        jump.phase = JumpPhase::Hunker;
                        jump.timer = JUMP_HUNKER;
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
                    if let Some((tpos, _)) = nearest_prey_pos(&prey, motion.pos) {
                        jump.to = tpos;
                    }
                    jump.from = motion.pos;
                    jump.phase = JumpPhase::Air;
                    jump.timer = JUMP_AIR;
                    if crate::ai::diag::AI_DIAG {
                        info!("crab: POUNCE dist={:.1}", (jump.to.xz() - jump.from.xz()).length());
                    }
                }
            }
            JumpPhase::Air => {
                jump.timer -= dt;
                let s = (1.0 - (jump.timer / JUMP_AIR)).clamp(0.0, 1.0);
                let ground = jump.from.lerp(jump.to, s);
                let height = JUMP_ARC * (std::f32::consts::PI * s).sin();
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
                    // Land: clamp onto the patch and bite the nearest prey in reach.
                    motion.pos = clamp_to_patch(motion.pos, graph.patch(motion.patch));
                    let reach_sq = (UNIT_BODY_RADIUS + CRAB_CONTACT_RADIUS + 0.2).powi(2);
                    let mass_sq = (UNIT_BODY_RADIUS + CRAB_CONTACT_RADIUS).powi(2);
                    for (ptf, mut hp) in &mut prey {
                        if (ptf.translation.xz() - motion.pos.xz()).length_squared() <= reach_sq {
                            // Critical-mass gate (same rule as `crab_contact_damage`): a pounce only
                            // bites if the swarm has reached MASS_MIN on this unit, or a nest is berserk.
                            // Below that the pounce lands but deals no damage — one path for "a handful
                            // of crabs can't overcome the squad", by contact OR by leap.
                            let count = crab_cells
                                .iter()
                                .filter(|c| (**c - ptf.translation.xz()).length_squared() <= mass_sq)
                                .count();
                            if count >= MASS_MIN || (berserk && count > 0) {
                                hp.current -= JUMP_DAMAGE;
                            }
                            break;
                        }
                    }
                    jump.phase = JumpPhase::Ready;
                    jump.cooldown = JUMP_COOLDOWN;
                }
            }
        }
    }
}

/// Feeding frenzy: damage to a unit grows **super-linearly** with how many crabs are on it
/// (`CRAB_CONTACT_DPS * count^DAMAGE_EXPONENT`), so one crab is a nuisance but a pile shreds it. Counts
/// by PLANAR distance so a crab clinging high on the body still feeds.
fn crab_contact_damage(
    time: Res<Time>,
    alarm: Res<crate::nest::NestAlarm>,
    crabs: Query<&Transform, (With<Crab>, Without<Prey>)>,
    mut prey: Query<(&Transform, &mut Health), (With<Prey>, Without<Crab>)>,
) {
    let dt = time.delta_secs();
    let berserk = alarm.0 > 0.0;
    // Reach = body radius + a little, so anything latched onto the cylinder counts (units and the boss).
    let reach_sq = (UNIT_BODY_RADIUS + CRAB_CONTACT_RADIUS).powi(2);
    for (unit_tf, mut hp) in &mut prey {
        let count = crabs
            .iter()
            .filter(|c| (c.translation.xz() - unit_tf.translation.xz()).length_squared() <= reach_sq)
            .count();
        // Mass gate: a handful of crabs can't overcome the squad — the swarm must reach critical mass on
        // a target before its (super-linear) bite bites. A nest under attack (berserk) waives the gate.
        if count >= MASS_MIN || (berserk && count > 0) {
            hp.current -= CRAB_CONTACT_DPS * (count as f32).powf(DAMAGE_EXPONENT) * dt;
        }
    }
}

/// Despawn dead crabs with a small blood burst + squelch (reuses the enemy-death VFX/SFX path).
fn crab_despawn_dead(
    mut commands: Commands,
    mut gore: ResMut<GoreQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    crabs: Query<(Entity, &Health, &Transform), With<Crab>>,
) {
    for (entity, hp, tf) in &crabs {
        if hp.current <= 0.0 {
            gore.0.push(GoreEvent {
                pos: tf.translation,
                kind: GoreKind::EnemySplat,
                tint: Color::srgb(0.35, 0.6, 0.15), // sickly green crab ichor
                gib: None,
                // Chaff: a crab death barely nudges the camera, so a whole swarm dying doesn't read as
                // one giant explosion (the gib chunks still pop — only the feel layer is scaled down).
                intensity: crate::gore::death_intensity(CRAB_HP, CRAB_CONTACT_DPS),
            });
            // Blood → SCENT: a fresh kill draws the swarm and the boss to the feeding site.
            deposits.0.push(crate::ai::field::Deposit {
                pos: tf.translation,
                field: crate::ai::field::FieldId::SCENT,
                amount: crate::ai::field::BLOOD_SCENT,
            });
            sfx.write(Sfx::EnemyDeath);
            commands.entity(entity).despawn();
        }
    }
}

/// Crabs flee a little faster than they forage — panic overrides the leisurely scuttle.
const FLEE_SPEED_MUL: f32 = 1.3;

/// Reproduction (positive feedback) + crowding cap. New crabs are born at a nest, paid for out of the
/// meat its crews have hauled in — never if the nest cell is already crowded or the global cap is hit.
const CRAB_COUNT_MAX: usize = 90;
/// Minimum inter-birth interval (seconds) — a rate limiter, NOT a free drip. A nest can only breed when
/// it has enough hauled meat (`MEAT_PER_CRAB`) *and* this timer has elapsed, so a well-fed nest births at
/// most this often (accelerated by `spawn_boost`) and a starved nest can't breed at all. Also capped by
/// `CRAB_COUNT_MAX` and suppressed while the nest cell is crowded.
const NEST_RESPAWN_INTERVAL: f32 = 5.0;
/// Hoarded meat one birth consumes. Breeding both *requires* and *spends* this much `hoard`, so the
/// forage→haul→deliver economy is the one gate on reinforcements: cut off the crabs' food (destroy the
/// gibs) and `hoard` drains to zero and births stop. Playtest dial — measured chunk weights run
/// ~0.10–1.5 (median ~0.45), so at 1.0 a birth costs roughly 2–3 delivered chunks.
const MEAT_PER_CRAB: f32 = 1.0;
/// Weight → spawn-boost conversion when a chunk is delivered. A heavy chunk (~1.5) alone roughly maxes
/// the boost; lighter chunks add proportionally, and deliveries accumulate. See [`SPAWN_BOOST_MAX`].
const FEED_GAIN: f32 = 6.0;
/// Cap on a nest's feeding surge. The effective respawn rate is `1 + spawn_boost`, so `9.0` = up to
/// **10×** faster births while well-fed (the "x10 based on chunk weight" the design calls for).
const SPAWN_BOOST_MAX: f32 = 9.0;
/// How fast the feeding surge fades (units/s). At the cap it takes ~9 s to decay to base, so a steady
/// stream of deliveries sustains fast spawning while a fed-then-starved nest relaxes back to the drip.
const SPAWN_BOOST_DECAY: f32 = 1.0;
const CROWD_CAP: f32 = 5.0; // local CRAB_DENSITY above this suppresses breeding (territorial)
/// Per-second density each crab lays into the CRAB_DENSITY field (≈ evaporation rate, so the field's
/// value at a cell tracks the local crab count).
const DENSITY_RATE: f32 = 0.4;
/// Per-second MEAT a gib lays into the field (≈ evaporation, so the field tracks current meat presence).
const MEAT_RATE: f32 = 0.5;
/// One crab's carry capacity (weight units). Measured meat weights (density × real GLB mesh volume) span
/// ~0.10–1.5, median ~0.45, so at 0.4 a light chunk goes solo, a mid chunk needs 2, and the heaviest
/// need 3–4 crabs cooperating (Σ capacity ≥ weight). Per-crab ±20% variance. Also self-limits pile-ups:
/// a gib stops accepting crew once full, so overflow foragers move on to uncrewed chunks.
const CRAB_CARRY_CAPACITY: f32 = 0.4;
/// How close a crab must get to a gib to grab it, and to the nest to deliver.
const GRAB_RANGE: f32 = 0.6;
/// Line-of-sight range: within this of the committed chunk the crab straight-lines onto it; beyond it,
/// it navigates by the wall-aware MEAT gradient. ~2 cells ≈ same room, so straight-line is unobstructed.
const LOS_RANGE: f32 = 2.0;
/// How tightly a *hauling* carrier hugs its chunk (mouth on it), vs `GRAB_RANGE` while merely gathering.
const CARRY_HOLD: f32 = 0.12;
/// Base speed a lifted chunk travels toward the nest — slower than a free crab (it's dragging a load).
/// Scaled down by weight and up by crew size in `carry_gibs` (see `WEIGHT_DRAG`).
const CARRY_SPEED: f32 = 1.6;
/// Divisor turning a chunk's weight into drag: haul speed ≈ `CARRY_SPEED * crew / (weight * WEIGHT_DRAG)`,
/// so a heavy chunk with a bare crew crawls and extra carriers speed it up (capped at `CARRY_SPEED`).
/// Tuned against measured weights (~0.1–1.5) and capacity 0.4 so a solo light chunk moves near base speed.
const WEIGHT_DRAG: f32 = 2.5;
/// World height a hauled chunk rides at — the crew's mouth height (crab seat ~0.05 + model ~0.11 + tooth
/// bone), so the chunk is gripped at the mouths rather than floating overhead.
const CARRY_HEIGHT: f32 = 0.15;
/// Horizontal distance from the nest at which a hauled chunk is delivered (nest dome radius + margin).
const DELIVER_RANGE: f32 = 1.2;
/// Seconds a crew may gather without lifting before it disbands (frees crabs stuck on a too-heavy chunk).
const CREW_TIMEOUT: f32 = 6.0;
/// A crab only enlists on a gib once it is genuinely NEAR it (roughly line-of-sight). Commitment uses a
/// straight-line distance, so a far gib may be behind a wall and unreachable; keeping the threshold
/// small means crabs commit only to chunks they can actually walk to. Farther foragers keep climbing the
/// wall-aware MEAT gradient until a chunk comes within reach, then commit. (Larger values re-introduce
/// the "committed to an across-wall gib → stuck against the wall" freeze.)
const MAX_COMMIT_DIST: f32 = 2.5;

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
) -> CrabState {
    // Flee down the THREAT gradient (away from danger); if the field is flat, keep the current heading
    // so the crab keeps moving rather than freezing. `steer_surface` routes this along the graph, so a
    // cornered crab climbs a wall to escape instead of clipping through it.
    let g = stig.gradient(crate::ai::field::FieldId::THREAT, dungeon, motion.pos);
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
        CRAB_SPEED * FLEE_SPEED_MUL,
        sep,
        dt,
        t,
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
) -> CrabState {
    let v = rally.sample(dungeon, motion.pos);
    let desired = if v.length_squared() > 1.0e-6 {
        Vec3::new(v.x, 0.0, v.y)
    } else {
        motion.heading
    };
    if steer_surface(motion, graph, dungeon, desired, None, CRAB_SPEED, sep, dt, t) {
        CrabState::Walk
    } else {
        CrabState::Idle
    }
}

/// Aggressive scout roam: range across floor + walls on a heading re-rolled every `SCOUT_WANDER_INTERVAL`
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
) -> CrabState {
    scout.wander_timer -= dt;
    if scout.wander_timer <= 0.0 || scout.wander_dir == Vec3::ZERO {
        scout.wander_timer = SCOUT_WANDER_INTERVAL;
        let angle = rand01(&mut scout.rng) * std::f32::consts::TAU;
        scout.wander_dir = Vec3::new(angle.cos(), 0.0, angle.sin());
    }
    if steer_surface(
        motion,
        graph,
        dungeon,
        scout.wander_dir,
        None,
        CRAB_SPEED * SCOUT_SPEED_MUL,
        sep,
        dt,
        t,
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
    t: f32,
) -> bool {
    let p = graph.patch(motion.patch);

    // Final approach: if the homing point sits on THIS patch's cell, walk straight to it (no gate).
    let on_patch_home =
        home.filter(|h| graph.floor_patch_cell(dungeon.world_to_cell(*h)) == Some(motion.patch));

    // Otherwise pick the graph neighbour whose gate best matches the desired travel direction.
    let desired_t = project_tangent(desired, p.normal).normalize_or_zero();
    let mut best: Option<(u32, Vec3)> = None;
    let mut best_dot = 0.0f32;
    if on_patch_home.is_none() && desired_t.length_squared() > 1.0e-6 {
        for (to, gate) in graph.neighbors(motion.patch) {
            let g_dir = project_tangent(gate - motion.pos, p.normal).normalize_or_zero();
            let d = g_dir.dot(desired_t);
            if d > best_dot {
                best_dot = d;
                best = Some((to, gate));
            }
        }
    }

    // Steer toward the homing point, else the chosen gate, else drift in the desired direction (a
    // dead-end with no aligned neighbour → the crab slides along the wall, clamped to its patch).
    let steer_to = on_patch_home
        .or(best.map(|(_, g)| g))
        .unwrap_or(motion.pos + desired_t);
    let tangent = project_tangent(steer_to - motion.pos, p.normal).normalize_or_zero();
    let move_vec = tangent * speed + project_tangent(sep, p.normal) * CRAB_SEP_STRENGTH;
    motion.pos += move_vec * dt;
    motion.pos = clamp_to_patch(motion.pos, p);

    // Commit a patch transfer only on physically reaching the chosen gate — never across a wall.
    if let Some((to, gate)) = best {
        if motion.pos.distance(gate) < TRANSFER_RADIUS {
            motion.patch = to;
            motion.pos = clamp_to_patch(gate, graph.patch(to));
        }
    }

    // Ease onto the (possibly new) surface and turn toward travel.
    let np = graph.patch(motion.patch);
    motion.normal = motion.normal.lerp(np.normal, t).normalize_or(np.normal);
    let moved = move_vec.length_squared() > 1.0e-6;
    if moved {
        let h = project_tangent(move_vec, motion.normal).normalize_or(motion.heading);
        motion.heading = motion.heading.lerp(h, t).normalize_or(motion.heading);
    }
    moved
}

/// Foraging locomotion: crawl toward meat along walkable floor. Long-range navigation follows the MEAT
/// stigmergy gradient, which — because the field lives only on floor cells and diffuses only between
/// them — flows *around* walls (a proper floor-topology potential field; ACO trail ascent, Dorigo).
/// Only within line-of-sight (`LOS_RANGE`) of the committed chunk does the crab straight-line home onto
/// the exact gib, then hold within `GRAB_RANGE` for the lift. A flat local field falls back to steering
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
) -> CrabState {
    // A hauling carrier hugs the chunk (mouth on it); a gathering crab holds a grab-range away.
    let hold = if hauling { CARRY_HOLD } else { GRAB_RANGE };

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
            motion.pos += project_tangent(sep, np.normal) * CRAB_SEP_STRENGTH * dt;
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
        Some(gp) if (gp - motion.pos).length() < LOS_RANGE => gp - motion.pos,
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
    if steer_surface(motion, graph, dungeon, desired, target, CRAB_SPEED, sep, dt, t) {
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
    mut crabs: Query<(Entity, &CrabMotion, &mut CrabCarry, &crate::ai::brain::ActiveBehavior), With<Crab>>,
    mut gibs: Query<(Entity, &Transform, &mut crate::gore::Carryable)>,
) {
    // Drop targets whose gib no longer exists (e.g. capped out of the ring mid-haul) so the crab
    // re-forages. Clearing `hauling` alongside `target` is essential: a lone `target = None` strands a
    // carrier that was mid-haul, because `hauling` keeps `Fact::CarryingMeat` — and thus the `Carry`
    // mode — latched with no chunk to carry. The brain then never leaves Carry, so it steers nowhere
    // (`target` is None), `release_uncommitted_carriers` can't recover it (Carry counts as
    // "committed"), and `carry_gibs` never touches it (the gib is gone) — the crab freezes forever.
    for (_, _, mut cc, _) in &mut crabs {
        if let Some(g) = cc.target {
            if gibs.get(g).is_err() {
                cc.target = None;
                cc.hauling = false;
            }
        }
    }

    // Snapshot per-crab capacity — summing a gib's committed crew capacity needs every carrier's value.
    let caps: HashMap<Entity, f32> = crabs.iter().map(|(e, _, c, _)| (e, c.capacity)).collect();

    // Snapshot each gib: position, weight, whether it's already being hauled, and its current committed
    // capacity. `committed` is mutated as we enlist crabs this tick so several seekers don't over-crew.
    let mut committed: HashMap<Entity, f32> = HashMap::new();
    let gib_snap: Vec<(Entity, Vec3, f32, bool)> = gibs
        .iter()
        .map(|(e, tf, c)| {
            let sum: f32 = c.carriers.iter().filter_map(|x| caps.get(x)).sum();
            committed.insert(e, sum);
            (e, tf.translation, c.weight, c.phase == crate::gore::CarryPhase::Hauling)
        })
        .collect();

    // Seeking crabs that still need a chunk.
    let seekers: Vec<(Entity, Vec3)> = crabs
        .iter()
        .filter(|(_, _, c, ab)| {
            matches!(ab.mode, crate::ai::utility::Mode::SeekMeat) && c.target.is_none()
        })
        .map(|(e, m, _, _)| (e, m.pos))
        .collect();

    for (crab_e, cpos) in seekers {
        let mut best: Option<(Entity, f32)> = None;
        for &(ge, gpos, weight, hauling) in &gib_snap {
            if hauling {
                continue;
            }
            if committed.get(&ge).copied().unwrap_or(0.0) >= weight {
                continue; // already has enough crew to lift
            }
            let d = gpos.distance(cpos);
            if d > MAX_COMMIT_DIST {
                continue; // too far to reach by straight-line steering — gradient-forage toward it first
            }
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((ge, d));
            }
        }
        let Some((ge, _)) = best else { continue };

        // Commit: enlist on the gib and record the target on the crab.
        if let Ok((_, _, mut carry)) = gibs.get_mut(ge) {
            if !carry.carriers.contains(&crab_e) {
                carry.carriers.push(crab_e);
            }
            if carry.phase == crate::gore::CarryPhase::Resting {
                carry.phase = crate::gore::CarryPhase::Crewing;
            }
        }
        if let Ok((_, _, mut cc, _)) = crabs.get_mut(crab_e) {
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
/// carrier that never reaches the pile and stalls the lift until `CREW_TIMEOUT`. Touches only
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
///   Crewing → Resting            after `CREW_TIMEOUT` (disband a crew that can't lift);
///   Hauling → delivered          within `DELIVER_RANGE` of the nest (hoard += weight, consume the gib);
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
) {
    let dt = time.delta_secs();

    for (ge, mut carry, mut gtf, mut lv, mut av) in &mut gibs {
        // 1. Prune carriers down to crabs that still exist AND still point at this gib.
        carry
            .carriers
            .retain(|&c| crabs.get(c).map(|(_, cc)| cc.target == Some(ge)).unwrap_or(false));

        // 2. Sum crew capacity two ways: `cap_here` counts only carriers that have actually gathered at
        // the chunk (within `GRAB_RANGE`) — that's what can lift it; `cap_total` counts every committed
        // carrier — that's what sustains an in-progress haul (a chaser lagging a little mustn't drop it).
        // Requiring the gathered capacity (not the whole roster) to lift avoids a deadlock where one
        // straggler that can't path to the chunk keeps a full-strength crew from ever lifting.
        let mut cap_here = 0.0;
        let mut cap_total = 0.0;
        for &c in &carry.carriers {
            if let Ok((ctf, cc)) = crabs.get(c) {
                cap_total += cc.capacity;
                if ctf.translation.xz().distance(gtf.translation.xz()) <= GRAB_RANGE {
                    cap_here += cc.capacity;
                }
            }
        }
        let has_crew = !carry.carriers.is_empty();

        match carry.phase {
            crate::gore::CarryPhase::Resting | crate::gore::CarryPhase::Crewing => {
                if !has_crew {
                    carry.phase = crate::gore::CarryPhase::Resting;
                    carry.crew_timer = 0.0;
                } else if cap_here >= carry.weight {
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
                                            <= GRAB_RANGE
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
                    // Nearest nest becomes the destination — ranked by the floor delivery cell
                    // (`nest.pos`), NOT the wall-mounted dome `Transform`. Every other consumer (haul
                    // nav, deliver check, breeding, scout home) uses `nest.pos`; ranking by the dome
                    // here could commit the haul to a nest whose delivery cell is across a wall, so the
                    // flow field returns nothing and the chunk gets dragged straight through it. Match
                    // the deliver check's horizontal distance.
                    let mut best: Option<(Entity, f32)> = None;
                    for (ne, _ntf, nest) in nests.iter() {
                        let d = (nest.pos - gtf.translation).with_y(0.0).length();
                        if best.is_none_or(|(_, bd)| d < bd) {
                            best = Some((ne, d));
                        }
                    }
                    carry.nest = best.map(|(e, _)| e);
                    for &c in &carry.carriers {
                        if let Ok((_, mut cc)) = crabs.get_mut(c) {
                            cc.hauling = true;
                        }
                    }
                } else {
                    carry.phase = crate::gore::CarryPhase::Crewing;
                    carry.crew_timer += dt;
                    if carry.crew_timer >= CREW_TIMEOUT {
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
                }
            }
            crate::gore::CarryPhase::Hauling => {
                // Destination = the nest's floor delivery point + its walkway flow field (if it still
                // exists; a razed nest yields None → the haul aborts and the chunk drops).
                let nest_nav: Option<(Vec3, Arc<crate::flowfield::FlowField>)> = carry
                    .nest
                    .and_then(|n| nests.get(n).ok())
                    .map(|(_, _, nest)| (nest.pos, nest.flow.clone()));

                if cap_total < carry.weight || nest_nav.is_none() {
                    // --- ABORT / DROP (Kinematic → Dynamic) ---
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
                    if horiz.length() <= DELIVER_RANGE {
                        // --- DELIVER (Kinematic → despawn) ---
                        if let Some(n) = carry.nest {
                            if let Ok((_, _, mut nest)) = nests.get_mut(n) {
                                nest.hoard += carry.weight;
                                // Feeding surge: heavier chunks accelerate births more, up to ~10×.
                                nest.spawn_boost =
                                    (nest.spawn_boost + carry.weight * FEED_GAIN).min(SPAWN_BOOST_MAX);
                            }
                        }
                        for &c in &carry.carriers {
                            if let Ok((_, mut cc)) = crabs.get_mut(c) {
                                cc.target = None;
                                cc.hauling = false;
                            }
                        }
                        // The ONE early-removal path (drops the id from the ring, then despawns).
                        gib_ring.consume(&mut commands, ge);
                    } else {
                        // Haul along the nest's flow field so the chunk threads walkways instead of
                        // beelining through walls (Item #3). Speed scales up with crew and down with
                        // weight (Item #6): a heavy chunk on a bare crew crawls; more carriers speed it.
                        let steer = flow.steer(&dungeon, gtf.translation);
                        let mut dir = Vec3::new(steer.x, 0.0, steer.y);
                        if dir.length_squared() <= 1.0e-6 {
                            dir = horiz; // at/near the goal cell but still outside DELIVER_RANGE: close in
                        }
                        let crew = carry.carriers.len() as f32;
                        let speed = (CARRY_SPEED * crew / (carry.weight * WEIGHT_DRAG))
                            .clamp(CARRY_SPEED * 0.35, CARRY_SPEED);
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
}

/// Each crab lays into the CRAB_DENSITY field (a stigmergic crowding/recruitment substrate). With the
/// per-second rate ≈ the field's evaporation, the value at a cell tracks the local crab count.
fn deposit_crab_density(
    time: Res<Time>,
    crabs: Query<&Transform, With<Crab>>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
) {
    let amount = DENSITY_RATE * time.delta_secs();
    for tf in &crabs {
        deposits.0.push(crate::ai::field::Deposit {
            pos: tf.translation,
            field: crate::ai::field::FieldId::CRAB_DENSITY,
            amount,
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
) {
    let amount = MEAT_RATE * time.delta_secs();
    for (tf, carry) in &gibs {
        if carry.phase == crate::gore::CarryPhase::Hauling {
            continue; // in-transit chunk — don't trail meat scent to the nest
        }
        deposits.0.push(crate::ai::field::Deposit {
            pos: tf.translation,
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
/// - **Roaming → Tracking**: on sensing prey within `SCOUT_SIGHT` (planar), lock onto the nearest.
/// - **Tracking**: refresh the tracked prey and, throttled by `RALLY_DEPOSIT_COOLDOWN`, deposit a vector
///   toward it (strength eases with proximity). Losing sight drops back to Roaming; the pheromone then
///   evaporates on its own — the automatic "call off the attack".
fn scout_mark_prey(
    time: Res<Time>,
    mut scouts: Query<(&Transform, &mut Scout)>,
    prey: Query<&Transform, With<Prey>>,
    mut deposits: ResMut<crate::ai::field::RallyDeposits>,
) {
    let dt = time.delta_secs();
    for (tf, mut scout) in &mut scouts {
        let pos = tf.translation;
        scout.report_cooldown = (scout.report_cooldown - dt).max(0.0);

        // Nearest prey on the ground plane, within sight.
        let mut best = f32::MAX;
        let mut spotted = None;
        for pt in &prey {
            let d = (pt.translation.xz() - pos.xz()).length();
            if d < best {
                best = d;
                spotted = Some(pt.translation);
            }
        }

        match spotted.filter(|_| best <= SCOUT_SIGHT) {
            Some(prey_pos) => {
                scout.state = ScoutState::Tracking { prey_pos };
                // Deposit an intermediate-vector pointing at the prey (Tang's `s`), throttled so a cell
                // isn't saturated frame-by-frame. Strength eases with proximity so nearer marks weigh more.
                if scout.report_cooldown <= 0.0
                    && let Some(dir) = (prey_pos.xz() - pos.xz()).try_normalize()
                {
                    let strength =
                        RALLY_MARK_STRENGTH * ((SCOUT_SIGHT - best) / SCOUT_SIGHT).clamp(0.0, 1.0);
                    deposits.0.push(crate::ai::field::RallyDeposit {
                        pos,
                        vec: dir * strength,
                    });
                    scout.report_cooldown = RALLY_DEPOSIT_COOLDOWN;
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
    mut nests: Query<&mut crate::nest::Nest>,
    crabs: Query<(), With<Crab>>,
    mut seq: ResMut<CrabSpawnSeq>,
) {
    let (Some(graph), Some(crab_assets)) = (graph, crab_assets) else {
        return;
    };
    let dt = time.delta_secs();
    let mut total = crabs.iter().count();
    for mut nest in &mut nests {
        // Fade the feeding surge, then re-arm the next spawn at the boosted rate (up to 10× faster).
        nest.spawn_boost = (nest.spawn_boost - SPAWN_BOOST_DECAY * dt).max(0.0);
        nest.respawn_timer -= dt;
        if nest.respawn_timer > 0.0 {
            continue;
        }
        // Effective rate = 1 + spawn_boost (SPAWN_BOOST_MAX ⇒ ~10× faster). Re-arm even if this tick
        // can't spawn (cap/crowd), so a fed nest keeps its fast cadence.
        nest.respawn_timer = NEST_RESPAWN_INTERVAL / (1.0 + nest.spawn_boost);

        if total >= CRAB_COUNT_MAX {
            continue;
        }
        // Meat gate: breeding both requires and consumes hoarded meat. No hoard → no birth, so cutting
        // off the swarm's food halts reinforcements (the economy's one lever).
        if nest.hoard < MEAT_PER_CRAB {
            continue;
        }
        // Don't pile births onto a crowded nest cell (territorial self-limiting).
        let density = stig.sample(crate::ai::field::FieldId::CRAB_DENSITY, &dungeon, nest.pos);
        if density >= CROWD_CAP {
            continue;
        }
        let Some(patch) = graph.floor_patch_cell(dungeon.world_to_cell(nest.pos)) else {
            continue; // nest's delivery cell isn't floor — can't seat a newborn here
        };
        nest.hoard -= MEAT_PER_CRAB; // spend the meat this birth cost
        let s = seq.0 as u32;
        seq.0 += 1;
        spawn_crab_on_patch(
            &mut commands,
            &graph,
            patch,
            &crab_assets.collider,
            &crab_assets.scene,
            s,
        );
        total += 1;
        if crate::ai::diag::AI_DIAG {
            info!("nest: RESPAWN total={total}");
        }
    }
}

/// Project a world vector onto the plane with the given normal (drop the normal component).
fn project_tangent(v: Vec3, normal: Vec3) -> Vec3 {
    v - normal * v.dot(normal)
}

/// Clamp a world point onto a patch's rectangle (keeps a crab on its current surface).
fn clamp_to_patch(pos: Vec3, p: &crate::surface_nav::Patch) -> Vec3 {
    let d = pos - p.center;
    let u = d.dot(p.tan_u).clamp(-p.half.x, p.half.x);
    let v = d.dot(p.tan_v).clamp(-p.half.y, p.half.y);
    p.center + p.tan_u * u + p.tan_v * v
}

/// Orientation that lays the crab flat on a surface (model up → `normal`) facing `heading`. Uses
/// `look_to` (−Z toward the facing dir, +Y toward the normal); `heading` is projected perpendicular to
/// the normal first so the up axis is exact.
fn surface_orientation(heading: Vec3, normal: Vec3) -> Quat {
    let up = normal.normalize_or(Vec3::Y);
    let fwd = project_tangent(heading, up).normalize_or(up.any_orthonormal_vector());
    let mut t = Transform::IDENTITY;
    t.look_to(fwd, up);
    t.rotation
}
