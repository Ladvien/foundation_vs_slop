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
use crate::squad::Unit;
use crate::surface_nav::{SurfaceField, SurfaceGraph};
use crate::util::hash01;

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

/// Scuttle speed along the surface (world units/s).
const CRAB_SPEED: f32 = 2.8; // 1.75× the earlier 1.6 — faster, more frantic vermin
/// Uniform render scale for the child model (native span ~3.2 → ~0.19 m). Small scuttling vermin —
/// ~1/2.5 the earlier size, which read as too big. Seat constants below scale down with it.
const CRAB_RENDER_SCALE: f32 = 0.06;
/// Root body-centre height above the surface, along the surface normal (also seats the collider).
const CRAB_BODY_CENTER: f32 = 0.05;
/// Local Y offset of the scaled model under the root so its body rests on the surface (the glb origin
/// sits near the model's top). Calibrated by eye via devshot, scaled with `CRAB_RENDER_SCALE`.
const CRAB_MODEL_Y: f32 = 0.11;
/// Radius of the invisible collider sphere (the laser raycast target); world-size since the root is
/// unscaled. Kept a touch generous so the now-small crabs stay hittable by the jittered auto-fire.
const CRAB_COLLIDER_R: f32 = 0.2;

/// Reynolds separation: crabs within this centre distance push apart (≈1.2× the crab footprint, so
/// they space out to roughly touching but *may* still overlap when crowded). Applied as a real
/// displacement (not just a steering nudge) so the spacing actually holds. Reynolds, "Steering
/// Behaviors For Autonomous Characters", GDC 1999.
const CRAB_SEP_RADIUS: f32 = 0.24;
const CRAB_SEP_STRENGTH: f32 = 6.0;

/// The unit's body approximated as a vertical cylinder the crabs cling to (radius, climbable height).
const UNIT_BODY_RADIUS: f32 = 0.33;
const UNIT_BODY_HEIGHT: f32 = 1.0;
/// Speed while climbing onto / crawling over a unit's body.
const CRAB_CLIMB_SPEED: f32 = 3.85; // 1.75× the earlier 2.2
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
                    // A fleeing crab drops its load before the carry machine re-evaluates the crew.
                    release_on_flee
                        .after(crate::ai::AiSet::Think)
                        .before(carry_gibs),
                    // Move after the brain has chosen this frame's mode (see `crate::ai`).
                    crab_locomotion
                        .after(rebuild_crab_field)
                        .after(crate::ai::AiSet::Think),
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
) {
    let collider = meshes.add(Sphere::new(CRAB_COLLIDER_R));
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(CRAB_GLB));
    let dome = meshes.add(Sphere::new(1.0)); // shared unit sphere → floor-buried dome per nest
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

    // A dimensional nest portal at each cluster seed — the crabs' home + meat-delivery + birth anchor.
    for &seed in &seeds {
        crate::nest::spawn_nest(
            &mut commands,
            &mut nest_mats,
            dome.clone(),
            dungeon.cell_center(seed),
        );
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
                spawn_crab_on_patch(&mut commands, &graph, patch, &collider, &scene);
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
) {
    let p = graph.patch(patch);
    let pos = p.center;
    let normal = p.normal;
    let heading = p.tan_u;
    let seat = pos + normal * CRAB_BODY_CENTER;

    commands
        .spawn((
            Crab,
            Hostile,
            Health::new(CRAB_HP),
            NoHealthBar, // swarm chaff: no floating bar (40 would bury the screen)
            crate::ai::drives::Drives::new(), // needs the utility brain weighs (hunger/fear/…)
            crate::ai::brain::BrainId::Crab,
            crate::ai::brain::ActiveBehavior::new(pos),
            crate::ai::brain::ThinkTimer::staggered(pos),
            CrabAttached { host: None },
            CrabCarry {
                capacity: CRAB_CARRY_CAPACITY * (0.8 + 0.4 * hash01(pos)),
                target: None,
                hauling: false,
            },
            CrabMotion {
                patch,
                pos,
                normal,
                heading,
                climb_bias: hash01(pos),
                angle_bias: hash01(pos + Vec3::new(7.3, 1.9, 4.1)),
                latched: false,
                latch_rel: 0.0,
            },
            CrabState::Idle,
            Mesh3d(collider.clone()),
            Transform::from_translation(seat).with_rotation(surface_orientation(heading, normal)),
            Visibility::Inherited,
        ))
        .with_child((
            WorldAssetRoot(scene.clone()),
            Transform::from_translation(Vec3::Y * CRAB_MODEL_Y)
                .with_scale(Vec3::splat(CRAB_RENDER_SCALE)),
        ));
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
    units: Query<&Transform, With<Unit>>,
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
    units: Query<(Entity, &Transform), (With<Unit>, Without<Crab>)>,
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
        ),
        With<Crab>,
    >,
) {
    let Some(graph) = graph else { return };
    let Some(field) = crab_field.field.as_ref() else {
        return;
    };
    let dt = time.delta_secs().min(MAX_FRAME_DT);

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
    for (motion, _, _, _, _, _) in &crabs {
        hash.entry(dungeon.world_to_cell(motion.pos))
            .or_default()
            .push(motion.pos);
    }

    for (mut motion, mut state, mut attached, mut transform, active, carry) in &mut crabs {
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
        } else if seeking {
            // --- SEEK MEAT: steer to the committed gib, or climb the MEAT gradient toward a pile. ---
            motion.latched = false;
            attached.host = None;
            crab_seek_meat(&mut motion, &stig, &dungeon, &graph, gib_pos, sep, dt, t)
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
                let move_vec =
                    tangent * CRAB_SPEED + project_tangent(sep, p.normal) * CRAB_SEP_STRENGTH;
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

/// Feeding frenzy: damage to a unit grows **super-linearly** with how many crabs are on it
/// (`CRAB_CONTACT_DPS * count^DAMAGE_EXPONENT`), so one crab is a nuisance but a pile shreds it. Counts
/// by PLANAR distance so a crab clinging high on the body still feeds.
fn crab_contact_damage(
    time: Res<Time>,
    crabs: Query<&Transform, (With<Crab>, Without<Unit>)>,
    mut units: Query<(&Transform, &mut Health), (With<Unit>, Without<Crab>)>,
) {
    let dt = time.delta_secs();
    // Reach = body radius + a little, so anything latched onto the cylinder counts.
    let reach_sq = (UNIT_BODY_RADIUS + CRAB_CONTACT_RADIUS).powi(2);
    for (unit_tf, mut hp) in &mut units {
        let count = crabs
            .iter()
            .filter(|c| (c.translation.xz() - unit_tf.translation.xz()).length_squared() <= reach_sq)
            .count();
        if count > 0 {
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
/// Delivered meat (hoard units) consumed to birth one crab at a nest — the forage→carry→breed payoff.
/// Sized against measured delivery (chunks weigh ~0.1–1.5), so a nest births a crab every few hauls.
const MEAT_PER_CRAB: f32 = 3.0;
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
/// Speed a lifted chunk travels toward the nest — slower than a free crab (it's dragging a load).
const CARRY_SPEED: f32 = 1.6;
/// Height a hauled chunk floats at so it reads as carried (the crew scuttles beneath it).
const LIFT_HEIGHT: f32 = 0.3;
/// Horizontal distance from the nest at which a hauled chunk is delivered (nest dome radius + margin).
const DELIVER_RANGE: f32 = 1.2;
/// Seconds a crew may gather without lifting before it disbands (frees crabs stuck on a too-heavy chunk).
const CREW_TIMEOUT: f32 = 6.0;
/// A crab only enlists on a gib within this range — its `SeekMeat` steering is straight-line, so a
/// far chunk (likely behind a wall) can't be reached; those crabs keep climbing the MEAT gradient
/// (which routes around walls) until a chunk comes within reach, then commit.
const MAX_COMMIT_DIST: f32 = 9.0;

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
    let p = graph.patch(motion.patch);
    // Gradient points toward higher THREAT; flee down it. If the field is flat here, keep heading so
    // the crab keeps moving away rather than freezing.
    let g = stig.gradient(crate::ai::field::FieldId::THREAT, dungeon, motion.pos);
    let away = project_tangent(Vec3::new(-g.x, 0.0, -g.y), p.normal).normalize_or_zero();
    let dir = if away.length_squared() > 1.0e-6 {
        away
    } else {
        project_tangent(motion.heading, p.normal).normalize_or_zero()
    };
    let move_vec = dir * (CRAB_SPEED * FLEE_SPEED_MUL) + project_tangent(sep, p.normal) * CRAB_SEP_STRENGTH;
    motion.pos += move_vec * dt;

    // Free floor transfer: flight can head any direction, so re-home onto the floor cell under the crab.
    if let Some(fp) = graph.floor_patch_cell(dungeon.world_to_cell(motion.pos)) {
        motion.patch = fp;
        let np = graph.patch(fp);
        motion.pos = clamp_to_patch(motion.pos, np);
        motion.normal = motion.normal.lerp(np.normal, t).normalize_or(np.normal);
    } else {
        // Fled toward a wall/void — clamp back onto the current patch (can't flee through walls).
        motion.pos = clamp_to_patch(motion.pos, graph.patch(motion.patch));
    }
    if move_vec.length_squared() > 1.0e-6 {
        let h = project_tangent(move_vec, motion.normal).normalize_or(motion.heading);
        motion.heading = motion.heading.lerp(h, t).normalize_or(motion.heading);
    }
    CrabState::Walk
}

/// Foraging locomotion: crawl *up* toward meat. With a committed gib the crab steers straight to it and
/// holds within `GRAB_RANGE` (so `carry_gibs` can lift once the crew has gathered); without one it climbs
/// the MEAT gradient toward the nearest pile (ACO-style trail ascent; Dorigo). Free cell-by-cell floor
/// transfer, like `crab_flee`. Returns the animation state.
fn crab_seek_meat(
    motion: &mut CrabMotion,
    stig: &crate::ai::field::Stig,
    dungeon: &Dungeon,
    graph: &crate::surface_nav::SurfaceGraph,
    target: Option<Vec3>,
    sep: Vec3,
    dt: f32,
    t: f32,
) -> CrabState {
    let p = graph.patch(motion.patch);
    let dir = match target {
        // Committed to a specific chunk: bee-line to it, but stop once within grabbing range so the
        // crew can converge and lift (the crab holds its ground rather than jostling the gib).
        Some(gp) => {
            let to = gp - motion.pos;
            if to.length() < GRAB_RANGE {
                Vec3::ZERO
            } else {
                project_tangent(to, p.normal).normalize_or_zero()
            }
        }
        // No committed chunk yet: ascend the MEAT field toward higher concentration.
        None => {
            let g = stig.gradient(crate::ai::field::FieldId::MEAT, dungeon, motion.pos);
            project_tangent(Vec3::new(g.x, 0.0, g.y), p.normal).normalize_or_zero()
        }
    };
    let move_vec = dir * CRAB_SPEED + project_tangent(sep, p.normal) * CRAB_SEP_STRENGTH;
    motion.pos += move_vec * dt;

    // Free floor transfer (foraging can head any direction; re-home onto the floor cell beneath).
    if let Some(fp) = graph.floor_patch_cell(dungeon.world_to_cell(motion.pos)) {
        motion.patch = fp;
        let np = graph.patch(fp);
        motion.pos = clamp_to_patch(motion.pos, np);
        motion.normal = motion.normal.lerp(np.normal, t).normalize_or(np.normal);
    } else {
        motion.pos = clamp_to_patch(motion.pos, graph.patch(motion.patch));
    }
    if move_vec.length_squared() > 1.0e-6 {
        let h = project_tangent(move_vec, motion.normal).normalize_or(motion.heading);
        motion.heading = motion.heading.lerp(h, t).normalize_or(motion.heading);
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
    // Drop targets whose gib no longer exists (e.g. capped out of the ring) so the crab re-forages.
    for (_, _, mut cc, _) in &mut crabs {
        if let Some(g) = cc.target {
            if gibs.get(g).is_err() {
                cc.target = None;
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

/// A crab that panics (or dies) mid-carry drops its load: clearing its target makes `carry_gibs` prune
/// it from the gib's crew next frame, so a fleeing hauler removes its capacity and can tip the chunk
/// back to the ground (the emergent "gunfire scatters the crew, the chunk thuds down"). Touches only
/// `CrabCarry` — the sole system besides the two carrier-mutators, and it never edits `carriers`.
fn release_on_flee(mut crabs: Query<(&crate::ai::brain::ActiveBehavior, &mut CrabCarry), With<Crab>>) {
    for (active, mut cc) in &mut crabs {
        if matches!(active.mode, crate::ai::utility::Mode::Flee) && cc.target.is_some() {
            cc.target = None;
            cc.hauling = false;
        }
    }
}

/// The cooperative-transport state machine — the SOLE authority over a lifted chunk's transform and
/// rigid-body mode. Each frame, per gib: prune dead/reassigned carriers, sum the live crew's capacity,
/// then take exactly one transition:
///   Resting/Crewing → Hauling   when Σ capacity ≥ weight AND the whole crew is within `GRAB_RANGE`
///                                (switch Dynamic→Kinematic, zero velocities, pick the nearest nest);
///   Crewing → Resting            after `CREW_TIMEOUT` (disband a crew that can't lift);
///   Hauling → delivered          within `DELIVER_RANGE` of the nest (hoard += weight, consume the gib);
///   Hauling → Crewing (drop)     if the crew's capacity falls below weight (Kinematic→Dynamic).
/// The three rigid-body switches are mutually exclusive (never two in one frame) — the "kinematic
/// hand-off guard". During Hauling the gib LEADS (its transform advances toward the nest); the crabs
/// just chase it via the `Carry` locomotion branch, so there's no circular follow.
/// Holland & Melhuish 1999 (stigmergic cooperative transport); avian3d kinematic bodies are moved by
/// transform, so zeroing Lin/Ang velocity on the switch prevents a residual-impulse launch.
#[allow(clippy::type_complexity)]
fn carry_gibs(
    time: Res<Time>,
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
                    // Nearest nest becomes the destination.
                    let mut best: Option<(Entity, f32)> = None;
                    for (ne, ntf, _) in nests.iter() {
                        let d = ntf.translation.distance(gtf.translation);
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
                // Destination nest position (if it still exists).
                let nest_pos = carry
                    .nest
                    .and_then(|n| nests.get(n).ok())
                    .map(|(_, t, _)| t.translation);

                if cap_total < carry.weight || nest_pos.is_none() {
                    // --- ABORT / DROP (Kinematic → Dynamic) ---
                    commands.entity(ge).insert(RigidBody::Dynamic);
                    carry.phase = crate::gore::CarryPhase::Crewing;
                    carry.crew_timer = 0.0;
                    for &c in &carry.carriers {
                        if let Ok((_, mut cc)) = crabs.get_mut(c) {
                            cc.hauling = false;
                        }
                    }
                } else if let Some(np) = nest_pos {
                    let horiz = (np - gtf.translation).with_y(0.0);
                    if horiz.length() <= DELIVER_RANGE {
                        // --- DELIVER (Kinematic → despawn) ---
                        if let Some(n) = carry.nest {
                            if let Ok((_, _, mut nest)) = nests.get_mut(n) {
                                nest.hoard += carry.weight;
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
                        // Haul: the gib leads, gliding toward the nest at carry speed; crew chases it.
                        gtf.translation += horiz.normalize_or_zero() * (CARRY_SPEED * dt);
                        gtf.translation.y = LIFT_HEIGHT;
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

/// Each carryable meat gib lays into the MEAT field, so foraging crabs sense a pile from a distance and
/// climb its gradient (ACO-style trail-following; Dorigo). The field fades as gibs are hauled off.
fn deposit_meat_scent(
    time: Res<Time>,
    gibs: Query<&Transform, With<crate::gore::Carryable>>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
) {
    let amount = MEAT_RATE * time.delta_secs();
    for tf in &gibs {
        deposits.0.push(crate::ai::field::Deposit {
            pos: tf.translation,
            field: crate::ai::field::FieldId::MEAT,
            amount,
        });
    }
}

/// Positive-feedback breeding — the ONE reproduction path (there is no latched-crab breeding). A nest
/// spends the meat its crews have hauled in (`hoard`) to birth new crabs at its mouth: while the hoard
/// covers another crab and the global cap allows it, deduct `MEAT_PER_CRAB` and spawn one on a floor
/// patch at the nest. A crowded nest cell (local CRAB_DENSITY high) holds its hoard until the area
/// thins — territorial self-limiting. This closes the forage→carry→deliver→breed loop (Holland &
/// Melhuish 1999, stigmergic foraging; Dorigo, ACO positive feedback with a population ceiling).
fn nest_reproduce(
    mut commands: Commands,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    stig: Res<crate::ai::field::Stig>,
    crab_assets: Option<Res<CrabAssets>>,
    mut nests: Query<(&Transform, &mut crate::nest::Nest)>,
    crabs: Query<(), With<Crab>>,
) {
    let (Some(graph), Some(crab_assets)) = (graph, crab_assets) else {
        return;
    };
    let mut total = crabs.iter().count();
    for (tf, mut nest) in &mut nests {
        // Hold the hoard while the nest cell is crowded (don't pile births onto births).
        let density = stig.sample(crate::ai::field::FieldId::CRAB_DENSITY, &dungeon, tf.translation);
        if density >= CROWD_CAP {
            continue;
        }
        let cell = dungeon.world_to_cell(tf.translation);
        let Some(patch) = graph.floor_patch_cell(cell) else {
            continue; // nest not over a floor patch — can't seat a newborn here
        };
        while nest.hoard >= MEAT_PER_CRAB && total < CRAB_COUNT_MAX {
            nest.hoard -= MEAT_PER_CRAB;
            spawn_crab_on_patch(
                &mut commands,
                &graph,
                patch,
                &crab_assets.collider,
                &crab_assets.scene,
            );
            total += 1;
            if crate::ai::diag::AI_DIAG {
                info!("nest: BIRTH total={total} hoard_left={:.1}", nest.hoard);
            }
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
