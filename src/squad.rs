//! The player's squad: five controllable `Unit` characters (replacing the old single `Player`).
//! Units are selected/commanded by the mouse (see `selection`), path via A\* (see `pathfinding`),
//! and follow their waypoints with **Reynolds separation steering** so they fan out instead of
//! stacking — the pragmatic local-avoidance layer over a global planner, exactly the split Pettré
//! et al. ("Synthetic-Vision Based Steering", SIGGRAPH 2010, DOI 10.1145/1778765.1778860) and
//! Menge (Curtis/Best/Manocha 2016, DOI 10.17815/cd.2016.1) describe (path planner → waypoints →
//! local collision avoidance; Reynolds, "Steering Behaviors for Autonomous Characters", 1999).

use bevy::prelude::*;

use crate::dungeon::Dungeon;

/// Marker for a squad member (the RTS unit; replaces the old single-agent `Player`).
#[derive(Component)]
pub struct Unit;

/// A unit's 0-based slot, so number keys 1–5 map to `UnitIndex(0)`..`UnitIndex(4)` (see `selection`).
#[derive(Component)]
pub struct UnitIndex(pub u8);

/// Ground-plane movement speed, world units per second.
#[derive(Component)]
pub struct MoveSpeed(pub f32);

/// The unit's team/outfit color, applied to its figurine once the model loads.
#[derive(Component)]
pub struct Outfit(pub Color);

/// Marks a unit as currently selected (drawn with a green ring, obeys move orders).
#[derive(Component)]
pub struct Selected;

/// An active move order: remaining waypoints (grid cells) the unit walks through in order.
#[derive(Component)]
pub struct MoveOrder {
    pub path: Vec<IVec2>,
}

/// Marks the gun sub-model so the outfit recolor skips it (the blaster keeps its own colors).
#[derive(Component)]
struct GunModel;

/// Marks a unit whose figurine has already been recolored (so the one-shot recolor runs once).
#[derive(Component)]
struct Recolored;

/// Scale the figurine so a unit reads clearly (0.7 × 1.4 ≈ 1.0 ≈ wall height).
const FIGURINE_SCALE: f32 = 1.4;
const COLLISION_MARGIN: f32 = 0.08;
/// Square collision footprint (arm span + margin), unchanged from the single-agent tuning.
const UNIT_HALF_EXTENTS: Vec2 = Vec2::splat(0.25 * FIGURINE_SCALE + COLLISION_MARGIN);
/// RTS walk speed (a touch slower than the old run pace — these are commanded, not twitch-driven).
const UNIT_SPEED: f32 = 6.0;
/// How fast a unit turns to face its travel direction (per-second slerp rate, clamped per frame).
const TURN_SPEED: f32 = 14.0;
const MAX_FRAME_DT: f32 = 1.0 / 30.0;
/// A waypoint is "reached" within this ground distance.
const ARRIVE_EPS: f32 = 0.18;
/// Separation steering: units within this radius repel each other so a group fans out.
const SEPARATION_RADIUS: f32 = 0.95;
/// Weight of the separation push relative to the goal-seeking direction.
const SEPARATION_WEIGHT: f32 = 0.7;

const FIGURINE_GLB: &str = "kenney_prototype-kit/Models/GLB format/figurine.glb";

/// Compact blaster carried in the figurine's hand (CC0, Kenney Blaster Kit 2.1).
const BLASTER_GLB: &str = "kenney_blaster-kit_2.1/Models/GLB format/blaster-a.glb";
const GUN_OFFSET: Vec3 = Vec3::new(0.18, 0.3, -0.2);
const GUN_SCALE: f32 = 0.35;
const GUN_YAW: f32 = 0.0;
/// The gun's barrel tip in figurine-local space — laser bolts spawn here (see `laser`).
pub const MUZZLE_LOCAL: Vec3 = Vec3::new(GUN_OFFSET.x, GUN_OFFSET.y, GUN_OFFSET.z - 0.35);

/// Five distinct outfit colors, one per squad member (index-matched to `UnitIndex`).
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
        app.add_systems(Startup, spawn_squad)
            .add_systems(Update, (recolor_units, unit_movement));
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
                UnitIndex(i as u8),
                MoveSpeed(UNIT_SPEED),
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

/// Advance each unit along its `MoveOrder` waypoints, steering around fellow units (separation).
fn unit_movement(
    mut commands: Commands,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    mut units: Query<(Entity, &mut Transform, &MoveSpeed, Option<&mut MoveOrder>), With<Unit>>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    // Snapshot every unit's position first (idle units must still be avoided).
    let positions: Vec<(Entity, Vec3)> = units.iter().map(|(e, t, _, _)| (e, t.translation)).collect();

    for (entity, mut transform, speed, order) in &mut units {
        let Some(mut order) = order else {
            continue; // no active order → hold position
        };
        // Drop reached waypoints; finish the order when the path is empty.
        while let Some(&next) = order.path.first() {
            let target = dungeon.cell_center(next);
            let mut to = target - transform.translation;
            to.y = 0.0;
            if to.length() < ARRIVE_EPS {
                order.path.remove(0);
                continue;
            }
            break;
        }
        let Some(&next) = order.path.first() else {
            commands.entity(entity).remove::<MoveOrder>();
            continue;
        };

        // Goal-seeking direction toward the next waypoint.
        let mut to = dungeon.cell_center(next) - transform.translation;
        to.y = 0.0;
        let seek = to.normalize_or_zero();

        // Reynolds separation: sum of pushes away from nearby units.
        let mut separation = Vec3::ZERO;
        for (other, opos) in &positions {
            if *other == entity {
                continue;
            }
            let mut away = transform.translation - *opos;
            away.y = 0.0;
            let dist = away.length();
            if dist > 1e-3 && dist < SEPARATION_RADIUS {
                separation += away / dist * (SEPARATION_RADIUS - dist) / SEPARATION_RADIUS;
            }
        }

        let steer = (seek + separation * SEPARATION_WEIGHT).normalize_or_zero();
        if steer == Vec3::ZERO {
            continue;
        }
        let delta = steer * speed.0 * dt;
        transform.translation = dungeon.resolve_move(transform.translation, delta, UNIT_HALF_EXTENTS);

        // Face travel direction (local -Z toward the step), slerped for a smooth turn.
        let facing = Transform::from_translation(transform.translation)
            .looking_at(transform.translation + steer, Vec3::Y)
            .rotation;
        transform.rotation = transform.rotation.slerp(facing, (TURN_SPEED * dt).min(1.0));
    }
}
