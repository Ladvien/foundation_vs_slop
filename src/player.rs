//! The player-controlled Foundation agent, now a `figurine` model exploring the
//! dungeon with smooth WASD movement, wall collision, and a carried torch.

use bevy::prelude::*;

use crate::camera::{SCREEN_FORWARD, SCREEN_RIGHT};
use crate::dungeon::Dungeon;

/// Marker for the single player-controlled agent.
#[derive(Component)]
pub struct Player;

/// Ground-plane movement speed, in world units per second.
#[derive(Component)]
pub struct MoveSpeed(pub f32);

/// Base (walking) ground speed, world units per second.
const PLAYER_SPEED: f32 = 10.0;
/// Speed multiplier while a Shift key is held — the sprint/run button.
const RUN_MULTIPLIER: f32 = 1.8;
/// How fast the figurine turns to face its travel direction, as a per-second slerp rate
/// (clamped to 1 per frame). Higher = snappier turns.
const TURN_SPEED: f32 = 22.0;
/// Cap on the per-frame timestep for movement (~1/30 s). Bounds how far the hero can move
/// in a single frame, so a frame hitch can't lurch it forward past where it should be.
const MAX_FRAME_DT: f32 = 1.0 / 30.0;
/// Scale the figurine so the hero reads clearly (0.7 × 1.4 ≈ 1.0 ≈ wall height).
const FIGURINE_SCALE: f32 = 1.4;
/// Small gap the collision box keeps from every wall face. Stops the hero standing flush,
/// which (a) kills the ~1e-5 floating-point flush-contact artifact and (b) prevents the arm
/// from overhanging adjacent unrevealed (black) void at inside corners. Complements the
/// camera-side wall cutaway in `occlusion`.
const COLLISION_MARGIN: f32 = 0.08;
/// Collision box half-extents. Square, sized to the figurine's *widest* part (the arms,
/// ±0.25 × scale) plus [`COLLISION_MARGIN`], because the figurine now rotates to face its
/// travel direction — a square footprint keeps the arms clear of walls in every orientation.
/// Still < the 0.8 half-clearance of the 2-wide corridors, so it fits every passage.
const PLAYER_HALF_EXTENTS: Vec2 = Vec2::splat(0.25 * FIGURINE_SCALE + COLLISION_MARGIN);
/// The figurine's feet already sit at y = 0 in scene space (its meshes are lifted by
/// node transforms), so no vertical offset is needed — it rests directly on the floor.
const FIGURINE_FOOT_OFFSET: f32 = 0.0;
const FIGURINE_GLB: &str = "kenney_prototype-kit/Models/GLB format/figurine.glb";

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player)
            .add_systems(Update, player_movement);
    }
}

fn spawn_player(mut commands: Commands, dungeon: Res<Dungeon>, assets: Res<AssetServer>) {
    let mut origin = dungeon.spawn_world();
    origin.y = FIGURINE_FOOT_OFFSET;

    commands
        .spawn((
            Player,
            MoveSpeed(PLAYER_SPEED),
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(FIGURINE_GLB))),
            Transform::from_translation(origin).with_scale(Vec3::splat(FIGURINE_SCALE)),
        ))
        // Carried torch: a subtle warm hotspot around the explorer. The bright fluorescent
        // ambient (see `world.rs`) is the main light now; fog still hides unexplored tiles.
        .with_child((
            PointLight {
                color: Color::srgb(1.0, 0.85, 0.6),
                intensity: 300_000.0,
                range: 10.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(0.0, 1.6, 0.0),
        ));
}

pub fn player_movement(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    player: Single<(&mut Transform, &MoveSpeed), With<Player>>,
) {
    let (mut transform, speed) = player.into_inner();

    // WASD moves along the screen axes (see `camera`), so "up" is away from the camera
    // rather than a world axis — the intuitive mapping for an isometric view.
    let mut direction = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        direction += SCREEN_FORWARD;
    }
    if keys.pressed(KeyCode::KeyS) {
        direction -= SCREEN_FORWARD;
    }
    if keys.pressed(KeyCode::KeyD) {
        direction += SCREEN_RIGHT;
    }
    if keys.pressed(KeyCode::KeyA) {
        direction -= SCREEN_RIGHT;
    }

    // Clamp the frame delta so an occasional frame hitch (e.g. background CPU load)
    // can't teleport the hero a huge step in one frame — it slows briefly instead.
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    if let Some(step) = direction.try_normalize() {
        let running = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let run = if running { RUN_MULTIPLIER } else { 1.0 };
        let delta = step * speed.0 * run * dt;
        // Wall collision is the dungeon's job — one authority for where the agent may go.
        transform.translation =
            dungeon.resolve_move(transform.translation, delta, PLAYER_HALF_EXTENTS);

        // Turn to face the travel direction. `looking_at` orients local -Z toward the target;
        // slerp toward it (rather than snapping) so direction changes read as a smooth pivot.
        let target = Transform::from_translation(transform.translation)
            .looking_at(transform.translation + step, Vec3::Y)
            .rotation;
        transform.rotation = transform.rotation.slerp(target, (TURN_SPEED * dt).min(1.0));
    }
}
