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

use bevy::prelude::*;

use crate::audio::Sfx;
use crate::dungeon::Dungeon;
use crate::enemy::Hostile;
use crate::gore::{GoreEvent, GoreKind, GoreQueue};
use crate::health::{Health, NoHealthBar};
use crate::squad::Unit;
use crate::surface_nav::{SurfaceField, SurfaceGraph};

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
/// `crab_contact_damage`): total = `CRAB_CONTACT_DPS * count^DAMAGE_EXPONENT`. One crab barely
/// scratches (~17 s) but a swarm becomes a feeding frenzy that shreds a unit in a blink.
const CRAB_CONTACT_DPS: f32 = 6.0;
/// Growth curve for stacked bites — >1 makes a pile hurt disproportionately more than its members
/// would linearly (5 crabs ≈ 78 DPS, 10 ≈ 240 DPS).
const DAMAGE_EXPONENT: f32 = 1.6;
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

/// Piranha latch: within this planar distance of a unit a crab leaves the floor/wall field and climbs
/// onto the unit itself to feed.
const LATCH_RANGE: f32 = 1.1;
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
                    crab_locomotion.after(rebuild_crab_field),
                    attach_crab_animation,
                    drive_crab_animation,
                    crab_contact_damage,
                    crab_despawn_dead,
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
) {
    let collider = meshes.add(Sphere::new(CRAB_COLLIDER_R));
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(CRAB_GLB));

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
            CrabAttached { host: None },
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
    units: Query<(Entity, &Transform), (With<Unit>, Without<Crab>)>,
    mut crabs: Query<
        (
            &mut CrabMotion,
            &mut CrabState,
            &mut CrabAttached,
            &mut Transform,
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
    for (motion, _, _, _) in &crabs {
        hash.entry(dungeon.world_to_cell(motion.pos))
            .or_default()
            .push(motion.pos);
    }

    for (mut motion, mut state, mut attached, mut transform) in &mut crabs {
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

        // Nearest unit on the ground plane.
        let (ndist, nunit) = unit_data.iter().fold((f32::MAX, None), |(bd, bu), &(e, up, fwd)| {
            let d = (up.xz() - motion.pos.xz()).length();
            if d < bd {
                (d, Some((e, up, fwd)))
            } else {
                (bd, bu)
            }
        });
        let t = (NORMAL_EASE * dt).min(1.0);

        let want = match nunit {
            // --- PIRANHA MODE: climb onto the unit and cover its body, biting from a free slot. ---
            Some((host, u, fwd)) if ndist < LATCH_RANGE => {
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
            // --- SURFACE MODE: shared flow-field pursuit across floor + walls. ---
            _ => {
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

/// Cheap deterministic hash of a position to `[0, 1)` (classic sine hash) — gives each crab a stable,
/// varied preferred climb height so a swarm spreads over a unit's body instead of stacking at one spot.
fn hash01(v: Vec3) -> f32 {
    let n = (v.x * 12.9898 + v.z * 78.233 + v.y * 37.719).sin() * 43758.547;
    n - n.floor()
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
            sfx.write(Sfx::EnemyDeath);
            commands.entity(entity).despawn();
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
