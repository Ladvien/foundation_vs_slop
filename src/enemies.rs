//! Slop entities: the uncanny, malformed monsters extruded by SCP-9191.
//!
//! Each is deliberately ugly — a bulbous sphere with an ill-fitting cube jammed
//! through it, in clashing muddy colors. They spawn on the arena rim and shamble
//! toward the Foundation agent.

use bevy::prelude::*;

use crate::player::Player;
use crate::world::ARENA_HALF_SIZE;

/// Marker for a slop entity.
#[derive(Component)]
pub struct Slop;

/// Slop shamble speed in world units per second.
#[derive(Component)]
pub struct SlopSpeed(pub f32);

/// Fires on an interval to extrude a new slop entity.
#[derive(Resource)]
struct SpawnTimer(Timer);

/// Cycles spawn points and color choices deterministically — no RNG dependency.
#[derive(Resource, Default)]
struct SpawnCounter(usize);

const SPAWN_INTERVAL_SECS: f32 = 1.5;
const SLOP_SPEED: f32 = 2.5;

/// Fixed ring of spawn points around the arena rim, cycled by [`SpawnCounter`].
const SPAWN_RING: [Vec2; 8] = [
    Vec2::new(1.0, 0.0),
    Vec2::new(0.7, 0.7),
    Vec2::new(0.0, 1.0),
    Vec2::new(-0.7, 0.7),
    Vec2::new(-1.0, 0.0),
    Vec2::new(-0.7, -0.7),
    Vec2::new(0.0, -1.0),
    Vec2::new(0.7, -0.7),
];

/// Muddy, clashing "slop" colors cycled by [`SpawnCounter`].
const SLOP_COLORS: [(f32, f32, f32); 4] = [
    (0.42, 0.45, 0.20), // sickly olive
    (0.55, 0.30, 0.38), // bruised mauve
    (0.30, 0.42, 0.40), // waterlogged teal
    (0.50, 0.40, 0.22), // rancid ochre
];

pub struct EnemyPlugin;

impl Plugin for EnemyPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SpawnTimer(Timer::from_seconds(
            SPAWN_INTERVAL_SECS,
            TimerMode::Repeating,
        )))
        .insert_resource(SpawnCounter::default())
        .add_systems(Update, (spawn_slop, slop_seek_player));
    }
}

fn spawn_slop(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut timer: ResMut<SpawnTimer>,
    mut counter: ResMut<SpawnCounter>,
    time: Res<Time>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    let n = counter.0;
    counter.0 = counter.0.wrapping_add(1);

    let rim = SPAWN_RING[n % SPAWN_RING.len()] * ARENA_HALF_SIZE;
    let (r, g, b) = SLOP_COLORS[n % SLOP_COLORS.len()];
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(r, g, b),
        perceptual_roughness: 1.0,
        ..default()
    });

    // Parent = bulbous body; child = an ill-fitting cube shoved through it, rotated
    // off-axis to read as "wrong". Despawning the parent takes the child with it.
    commands
        .spawn((
            Slop,
            SlopSpeed(SLOP_SPEED),
            Mesh3d(meshes.add(Sphere::new(0.6))),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(rim.x, 0.6, rim.y),
        ))
        .with_child((
            Mesh3d(meshes.add(Cuboid::new(0.9, 0.6, 0.5))),
            MeshMaterial3d(material),
            Transform::from_xyz(0.15, 0.25, 0.1)
                .with_rotation(Quat::from_rotation_y(0.6) * Quat::from_rotation_x(0.35)),
        ));
}

fn slop_seek_player(
    time: Res<Time>,
    player: Single<&Transform, (With<Player>, Without<Slop>)>,
    mut slops: Query<(&mut Transform, &SlopSpeed), (With<Slop>, Without<Player>)>,
) {
    let target = player.translation;
    for (mut transform, speed) in &mut slops {
        // Chase on the ground plane only; keep the slop's own height.
        let mut to_player = target - transform.translation;
        to_player.y = 0.0;
        if let Some(dir) = to_player.try_normalize() {
            transform.translation += dir * speed.0 * time.delta_secs();
        }
    }
}
