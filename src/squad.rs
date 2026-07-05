//! The player's squad: controllable `Unit` characters commanded by the mouse (see `selection`).
//! Movement is the SOTA split of a **flow-field global navigator** (see `flowfield`) feeding a
//! **hand-rolled ORCA local-avoidance** layer (see `orca`): the flow field decides each unit's
//! preferred velocity toward the shared goal, ORCA turns that into a collision-free velocity around
//! the other units, and `Dungeon::resolve_move` keeps it out of walls. This is the planner →
//! preferred-velocity → reciprocal-avoidance pipeline of Treuille et al. (Continuum Crowds,
//! SIGGRAPH 2006, DOI 10.1145/1141911.1142008) and van den Berg et al. (ORCA, 2011,
//! DOI 10.1109/TRO.2011.2120810), and it replaces the earlier summed-force separation that let
//! units cancel to a standstill.

use std::sync::Arc;

use bevy::prelude::*;

use crate::audio::Sfx;
use crate::dungeon::Dungeon;
use crate::flowfield::FlowField;
use crate::gore::{GibSource, GoreEvent, GoreKind, GoreQueue};
use crate::health::Health;
use crate::orca::{self, Agent};

/// Marker for a squad member (the RTS unit; replaces the old single-agent `Player`).
#[derive(Component)]
pub struct Unit;

/// Shared marker for anything the crab swarm treats as prey to swarm/latch/bite — squad units AND the
/// smiley boss (`crate::enemy`). Crab targeting keys on `Prey` (nearest wins), so the same forage/latch
/// code path drives crabs onto whichever prey is closest, without knowing its type.
#[derive(Component)]
pub struct Prey;

/// Ground-plane movement speed, world units per second.
#[derive(Component)]
pub struct MoveSpeed(pub f32);

/// The unit's team/outfit color, applied to its figurine once the model loads.
#[derive(Component)]
pub struct Outfit(pub Color);

/// Marks a unit as currently selected (drawn with a green ring, obeys move orders).
#[derive(Component)]
pub struct Selected;

/// An active move order: the shared flow field the unit follows toward the group's goal, plus a
/// small amount of follower state. One `FlowField` is built per command and shared (`Arc`) by every
/// unit in the selection, so hundreds of units cost one field build, not one A\* per unit.
#[derive(Component)]
pub struct MoveOrder {
    pub field: Arc<FlowField>,
    /// Closest the unit has ever gotten to the goal on this order (world distance).
    best_dist: f32,
    /// Seconds since `best_dist` last improved — a *progress*-based stall measure (a unit milling in
    /// place at non-zero speed still counts as stalled), driving packed-in and give-up arrival.
    no_progress_time: f32,
}

impl MoveOrder {
    pub fn new(field: Arc<FlowField>) -> Self {
        MoveOrder {
            field,
            best_dist: f32::MAX,
            no_progress_time: 0.0,
        }
    }
}

/// A unit's current planar velocity (xz), advertised to ORCA so neighbors can reciprocate. Held on
/// every unit (zero while idle) since idle units are still avoided.
#[derive(Component)]
pub struct Velocity(pub Vec2);

/// Marks the gun sub-model so the outfit recolor skips it (the blaster keeps its own colors) and so
/// `autogib` can bake it as a separate intact chunk instead of folding it into the body fracture.
#[derive(Component)]
pub struct GunModel;

/// Marks a unit whose figurine has already been recolored (so the one-shot recolor runs once).
#[derive(Component)]
struct Recolored;

/// Scale the figurine so a unit stands a bit taller than the 1.0 walls and reads at the enemies'
/// scale (0.7 × 1.8 ≈ 1.3, vs the smiley's ~1.26 capsule). Uniform, so the carried gun and the
/// autogib fragments stay proportional. Collision (`UNIT_HALF_EXTENTS`) stays narrower than the
/// visual on purpose — see below.
const FIGURINE_SCALE: f32 = 1.8;
/// Square collision half-extent. Sized so a unit (0.54 wide) fits the narrowest walkable channel:
/// a corridor cell walled on both sides has only `TILE - 2·WALL_THICKNESS = 0.6` of clear width, so
/// a wider box would physically wedge in 1-wide passages regardless of steering. Deliberately a
/// touch under the figurine's visual radius — a unit reaching its goal reliably matters more than
/// pixel-exact wall contact.
const UNIT_HALF_EXTENTS: Vec2 = Vec2::splat(0.27);
/// RTS walk speed (a touch slower than the old run pace — these are commanded, not twitch-driven).
/// Public so the laser can scale fire spread by how fast a unit is moving (accuracy penalty).
pub const UNIT_SPEED: f32 = 6.0;
/// Squad member hit points (drives the floating health bar; units take no damage yet).
const UNIT_HP: f32 = 100.0;
/// How fast a unit turns to face its travel direction (per-second slerp rate, clamped per frame).
const TURN_SPEED: f32 = 14.0;
const MAX_FRAME_DT: f32 = 1.0 / 30.0;

/// ORCA personal-space disc radius. Slightly larger than the collision half-extent so units keep
/// their AABBs from overlapping, while still small enough for two to pass abreast in a 2-wide
/// corridor (2·0.30 = 0.60 centre spacing fits the 1.6 clear width).
const ORCA_RADIUS: f32 = 0.30;
/// Seconds ahead unit↔unit collisions are anticipated. Larger = earlier, gentler avoidance but more
/// anticipatory braking (which gridlocks dense junctions); kept low so units squeeze through crowds.
const ORCA_TIME_HORIZON: f32 = 1.0;
/// Only units within this centre distance are fed to a unit's ORCA solve (the rest can't interact
/// within the horizon). Bounds the per-frame neighbor work.
const ORCA_QUERY_RADIUS: f32 = 4.0;

/// Arrived once this close to the goal cell centre.
const ARRIVE_RADIUS: f32 = 0.6;
/// Within this distance of the goal, a unit that can no longer make progress is "packed in" and
/// treated as arrived (others fill the exact goal cell), instead of jittering forever.
const PACK_RADIUS: f32 = 2.5;
/// A stuck unit also arrives if an already-settled unit sits within this distance *between it and
/// the goal* — the settled blob then grows outward from the goal so a large crowd all packs in
/// rather than piling up short of a single goal cell.
const BLOB_RADIUS: f32 = 1.3;
/// A unit counts as "progressing" when it gets at least this much closer to the goal; smaller
/// improvements are treated as a stall (avoids float jitter resetting the stall timer forever).
const PROGRESS_EPS: f32 = 0.05;
/// No-progress duration that ends a packed-in order when the unit is near the goal or wedged behind
/// the settled goal blob. There is deliberately no plain time-based give-up: initial spawn
/// congestion always stalls the back of the crowd for seconds, so a timer alone would settle units
/// at the spawn. Settling is gated on *what* blocks a unit — a settled neighbor between it and the
/// goal (permanent) vs. moving neighbors (transient) — see `blocked_by_settled`.
const PACK_STUCK_TIME: f32 = 0.5;

const FIGURINE_GLB: &str = "kenney_prototype-kit/Models/GLB format/figurine.glb";

/// Compact blaster carried in the figurine's hand (CC0, Kenney Blaster Kit 2.1).
const BLASTER_GLB: &str = "kenney_blaster-kit_2.1/Models/GLB format/blaster-a.glb";
const GUN_OFFSET: Vec3 = Vec3::new(0.18, 0.3, -0.2);
const GUN_SCALE: f32 = 0.35;
const GUN_YAW: f32 = 0.0;
/// The gun's barrel tip in figurine-local space — laser bolts spawn here (see `laser`).
pub const MUZZLE_LOCAL: Vec3 = Vec3::new(GUN_OFFSET.x, GUN_OFFSET.y, GUN_OFFSET.z - 0.35);

/// Five distinct outfit colors, one per squad member (index-matched to spawn order).
const OUTFITS: [Color; 5] = [
    Color::srgb(0.85, 0.22, 0.20), // red
    Color::srgb(0.22, 0.45, 0.90), // blue
    Color::srgb(0.25, 0.75, 0.32), // green
    Color::srgb(0.92, 0.76, 0.16), // gold
    Color::srgb(0.66, 0.32, 0.82), // purple
];

/// Spiral of cell offsets from the spawn point; the first five that are floor become unit spawns.
const SPAWN_SPIRAL: [(i32, i32); 13] = [
    (0, 0),
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (2, 0),
    (-2, 0),
    (0, 2),
    (0, -2),
];

pub struct SquadPlugin;

impl Plugin for SquadPlugin {
    fn build(&self, app: &mut App) {
        // `unit_movement` runs after `command_input` so a move order issued this frame is followed
        // the same frame (Bevy inserts a command sync point for the ordering), not one frame late.
        app.add_systems(Startup, spawn_squad).add_systems(
            Update,
            (
                recolor_units,
                unit_movement.after(crate::selection::command_input),
                despawn_dead_units,
            ),
        );
    }
}

fn spawn_squad(mut commands: Commands, dungeon: Res<Dungeon>, assets: Res<AssetServer>) {
    // Pick five distinct floor cells clustered around the dungeon spawn.
    let base = dungeon.spawn;
    let cells: Vec<IVec2> = SPAWN_SPIRAL
        .iter()
        .map(|&(dx, dy)| base + IVec2::new(dx, dy))
        .filter(|&c| dungeon.is_floor(c))
        .take(5)
        .collect();

    for (i, &cell) in cells.iter().enumerate() {
        let outfit = OUTFITS[i];
        commands
            .spawn((
                Unit,
                Prey, // crabs may swarm/bite units (nearest-prey targeting)
                MoveSpeed(UNIT_SPEED),
                Velocity(Vec2::ZERO),
                Health::new(UNIT_HP),
                Outfit(outfit),
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(FIGURINE_GLB))),
                Transform::from_translation(dungeon.cell_center(cell))
                    .with_scale(Vec3::splat(FIGURINE_SCALE)),
            ))
            // Carried blaster (kept from the shooter feature; fires from selected units, see `laser`).
            .with_child((
                GunModel,
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(BLASTER_GLB))),
                Transform::from_translation(GUN_OFFSET)
                    .with_scale(Vec3::splat(GUN_SCALE))
                    .with_rotation(Quat::from_rotation_y(GUN_YAW)),
            ));
    }
}

/// Debug safeguard: never let the whole squad wipe. At least [`MIN_LIVING_UNITS`] member always
/// survives — so of five units, four can die but the last cannot. (Remove this floor for real
/// lose conditions.)
const MIN_LIVING_UNITS: usize = 1;

/// Remove squad members whose health has run out (enemies gnaw them down — see `enemy`). Despawning
/// a unit takes its figurine + carried gun with it; its floating health bar is cleaned up as an
/// orphan by `health::update_health_bars`. A small burst at chest height marks the death. The last
/// [`MIN_LIVING_UNITS`] can't be despawned: a protected survivor has its health floored so it lingers
/// (bar shows a sliver) instead of dying.
fn despawn_dead_units(
    mut commands: Commands,
    mut gore: ResMut<GoreQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut units: Query<(Entity, &mut Health, &Transform, &Outfit, &WorldAssetRoot), With<Unit>>,
) {
    // How many dead units we may actually remove this frame while keeping the living floor.
    let mut removable = units.iter().count().saturating_sub(MIN_LIVING_UNITS);
    for (entity, mut hp, transform, outfit, root) in &mut units {
        if hp.current <= 0.0 {
            if removable > 0 {
                // The unit's real 3D figurine gets crunched: blood spray + a floor pool + its own
                // mesh sliced into flying meat chunks tinted to its outfit color (see `gore`/`autogib`).
                gore.0.push(GoreEvent {
                    pos: transform.translation + Vec3::Y * 0.5,
                    kind: GoreKind::UnitCrunch,
                    tint: outfit.0,
                    // The figurine's baked fracture set: spawn from its foot origin at its render scale.
                    gib: Some(GibSource {
                        source: root.0.id(),
                        origin: transform.translation,
                        scale: transform.scale.x,
                    }),
                    // Losing one of your own is a real gut-punch — a solid (but not boss-sized) kick.
                    intensity: 0.6,
                });
                sfx.write(Sfx::UnitDeath);
                commands.entity(entity).despawn();
                removable -= 1;
            } else {
                // Protected survivor — clamp so it can't die and its bar keeps a sliver.
                hp.current = hp.current.max(1.0);
            }
        }
    }
}

/// Once a unit's figurine scene has spawned its mesh descendants, give it a flat outfit-colored
/// material (a new handle per unit so they don't share one asset). The gun subtree is skipped so
/// the blaster keeps its own look. Runs until the async scene load produces meshes, then tags the
/// unit `Recolored` so it never runs again.
fn recolor_units(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    units: Query<(Entity, &Outfit), (With<Unit>, Without<Recolored>)>,
    children: Query<&Children>,
    has_material: Query<(), With<MeshMaterial3d<StandardMaterial>>>,
    is_gun: Query<(), With<GunModel>>,
) {
    for (unit, outfit) in &units {
        let material = materials.add(StandardMaterial {
            base_color: outfit.0,
            perceptual_roughness: 0.7,
            ..default()
        });
        let mut recolored_any = false;
        let mut stack: Vec<Entity> = match children.get(unit) {
            Ok(c) => c.iter().collect(),
            Err(_) => continue,
        };
        while let Some(e) = stack.pop() {
            if is_gun.get(e).is_ok() {
                continue; // don't recurse into the gun sub-model
            }
            if has_material.get(e).is_ok() {
                commands.entity(e).insert(MeshMaterial3d(material.clone()));
                recolored_any = true;
            }
            if let Ok(ch) = children.get(e) {
                stack.extend(ch.iter());
            }
        }
        if recolored_any {
            commands.entity(unit).insert(Recolored);
        }
    }
}

/// Advance each commanded unit: preferred velocity from the shared flow field → ORCA around the
/// other units → wall collision. Idle units hold position but are still avoided.
fn unit_movement(
    mut commands: Commands,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    mut units: Query<
        (
            Entity,
            &mut Transform,
            &MoveSpeed,
            &mut Velocity,
            Option<&mut MoveOrder>,
        ),
        With<Unit>,
    >,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    if dt <= 0.0 {
        return;
    }

    // Snapshot every unit as an ORCA agent using last frame's velocity (synchronous update: all
    // solves see the same prior state). A unit with an order `avoids` — it reciprocates; an idle
    // unit does not, so movers take full responsibility going around it.
    let agents: Vec<(Entity, Agent)> = units
        .iter()
        .map(|(e, t, _, v, order)| {
            (
                e,
                Agent {
                    pos: t.translation.xz(),
                    vel: v.0,
                    radius: ORCA_RADIUS,
                    avoids: order.is_some(),
                },
            )
        })
        .collect();

    for (entity, mut transform, speed, mut velocity, order) in &mut units {
        let Some(mut order) = order else {
            velocity.0 = Vec2::ZERO; // idle → at rest (still advertised to ORCA next frame)
            continue;
        };

        let pos = transform.translation;
        let self_pos = pos.xz();
        let goal_xz = dungeon.cell_center(order.field.goal()).xz();
        let goal_dist = (goal_xz - self_pos).length();

        // Preferred velocity: steer toward the flow-field look-ahead point on the cell centerline
        // (keeps the unit centered in corridors so its body fits through), at full speed.
        let pref = order.field.steer(&dungeon, pos) * speed.0;

        // ORCA neighbors, plus: is a *settled* unit (no order) sitting just ahead of me toward the
        // goal? If so and I can't progress, I've reached the back of the arrived blob and pack in.
        // Direction-based (settled unit within the goalward cone) so it propagates cleanly back from
        // the goal even across a room, and never fires at spawn where all neighbors still have orders.
        let to_goal = (goal_xz - self_pos).normalize_or_zero();
        let mut neighbors: Vec<Agent> = Vec::new();
        let mut blocked_by_settled = false;
        for (other, ag) in &agents {
            if *other == entity {
                continue;
            }
            let off = ag.pos - self_pos;
            if off.length_squared() <= ORCA_QUERY_RADIUS * ORCA_QUERY_RADIUS {
                neighbors.push(*ag);
            }
            if !ag.avoids
                && off.length_squared() <= BLOB_RADIUS * BLOB_RADIUS
                && off.normalize_or_zero().dot(to_goal) > 0.2
            {
                blocked_by_settled = true;
            }
        }

        // Nearby solid cells become hard ORCA wall constraints, so a unit dodging a neighbor is never
        // steered into a wall (where it would stall). Only walls the unit is actually *close* to bind
        // (gap < WALL_GATE); the allowed approach speed is the remaining gap ÷ dt, shrinking to zero at
        // contact. WALK_HALF is the walkable half-width of a walled cell (centre to wall face).
        const WALK_HALF: f32 = 0.5 * crate::dungeon::TILE_SIZE - crate::dungeon::WALL_THICKNESS;
        const WALL_GATE: f32 = 0.4;
        let cell = dungeon.world_to_cell(pos);
        let local = Vec2::new(pos.x - cell.x as f32, pos.z - cell.y as f32);
        let mut walls: Vec<(Vec2, f32)> = Vec::new();
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if dungeon.is_floor(cell + IVec2::new(dx, dy)) {
                continue;
            }
            let b = Vec2::new(dx as f32, dy as f32);
            let gap = WALK_HALF - (local.dot(b) + UNIT_HALF_EXTENTS.x);
            if gap < WALL_GATE {
                walls.push((b, (gap.max(0.0)) / dt));
            }
        }

        let me = Agent {
            pos: self_pos,
            vel: velocity.0,
            radius: ORCA_RADIUS,
            avoids: true,
        };
        let new_vel =
            orca::new_velocity(&me, pref, &neighbors, &walls, ORCA_TIME_HORIZON, dt, speed.0);
        velocity.0 = new_vel;

        // Integrate the ORCA velocity against walls (unit↔wall is the resolver's job, not ORCA's).
        let delta = Vec3::new(new_vel.x, 0.0, new_vel.y) * dt;
        transform.translation = dungeon.resolve_move(pos, delta, UNIT_HALF_EXTENTS);

        // Progress-based stall: the timer only resets when the unit gets genuinely closer to the
        // goal, so a unit shoved in circles at non-zero speed still eventually counts as stalled.
        let new_goal_dist = (goal_xz - transform.translation.xz()).length();
        if new_goal_dist < order.best_dist - PROGRESS_EPS {
            order.best_dist = new_goal_dist;
            order.no_progress_time = 0.0;
        } else {
            order.no_progress_time += dt;
        }

        // Arrival: reached the goal, or packed in — stalled *and* either right at the goal or wedged
        // behind the settled blob. Because settled units exist only at the goal (no mid-route give-up),
        // `blocked_by_settled` can only become true once a unit reaches the back of that blob, so the
        // blob grows outward from the goal and never nucleates a stall in the middle of a hallway.
        let packed = order.no_progress_time >= PACK_STUCK_TIME
            && (goal_dist < PACK_RADIUS || blocked_by_settled);
        let arrived = goal_dist < ARRIVE_RADIUS || packed;
        if arrived {
            commands.entity(entity).remove::<MoveOrder>();
            velocity.0 = Vec2::ZERO;
            continue;
        }

        // Face travel direction (local -Z toward the step), slerped for a smooth turn.
        let step = Vec3::new(new_vel.x, 0.0, new_vel.y);
        if step.length_squared() > 1e-6 {
            let facing = Transform::from_translation(transform.translation)
                .looking_at(transform.translation + step, Vec3::Y)
                .rotation;
            transform.rotation = transform.rotation.slerp(facing, (TURN_SPEED * dt).min(1.0));
        }
    }
}
