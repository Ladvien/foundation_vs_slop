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

const PLAYER_SPEED: f32 = 5.0;
/// Collision radius kept below half a tile so the agent fits through 1-wide passages.
const PLAYER_RADIUS: f32 = 0.3;
/// Scale the figurine so the hero reads clearly (0.7 × 1.4 ≈ 1.0 ≈ wall height).
const FIGURINE_SCALE: f32 = 1.4;
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
        // Carried torch: a warm point light that pools around the explorer, so with
        // low ambient the nearby dungeon reads bright and remembered areas stay dim.
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

fn player_movement(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    player: Single<(&mut Transform, &MoveSpeed), With<Player>>,
) {
    let (mut transform, speed) = player.into_inner();

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

    if let Some(step) = direction.try_normalize() {
        let delta = step * speed.0 * time.delta_secs();
        // Wall collision is the dungeon's job — one authority for where the agent may go.
        transform.translation = dungeon.resolve_move(transform.translation, delta, PLAYER_RADIUS);
    }
}
