//! Laser bolts fired from the player's blaster. Hold **Space** to fire glowing bolts in the
//! direction the figurine faces (its move/turn facing, see `player`). Bolts travel in a straight
//! line and despawn on a wall hit or after a short lifetime — there are no enemies to hit yet.

use bevy::prelude::*;

use crate::dungeon::Dungeon;
use crate::impact_fx::ImpactQueue;
use crate::squad::{Selected, Unit};

/// Seconds between shots while Space is held (fixed fire rate).
const FIRE_INTERVAL: f32 = 0.15;
/// Bolt travel speed, world units per second.
const LASER_SPEED: f32 = 22.0;
/// Bolt lifetime in seconds (a fallback despawn if it never meets a wall).
const LASER_LIFE: f32 = 1.2;

/// A live laser bolt: its constant velocity and remaining lifetime (seconds).
#[derive(Component)]
struct Laser {
    velocity: Vec3,
    life: f32,
}

/// Shared bolt mesh + emissive material, built once so every bolt is a cheap handle clone.
#[derive(Resource)]
struct LaserAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// Fixed-rate fire gate. Repeating: it ticks every frame and wraps every [`FIRE_INTERVAL`];
/// a shot is emitted on each wrap tick while Space is held.
#[derive(Resource)]
struct FireCooldown(Timer);

pub struct LaserPlugin;

impl Plugin for LaserPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FireCooldown(Timer::from_seconds(
            FIRE_INTERVAL,
            TimerMode::Repeating,
        )))
        .add_systems(Startup, setup_laser_assets)
        .add_systems(Update, (fire_laser, update_lasers));
    }
}

fn setup_laser_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Thin, long-on-Z bolt so `looking_to(forward)` (which aligns local −Z) points it along travel.
    let mesh = meshes.add(Cuboid::new(0.06, 0.06, 0.5));
    // The project's first emissive material — a hot red-orange bolt. Values > 1 read as "glowing"
    // even without bloom; add an HDR camera + Bloom later for a halo.
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.1, 0.08),
        emissive: LinearRgba::rgb(7.0, 0.25, 0.1), // red-dominant so it reads as a vivid bolt
        ..default()
    });
    commands.insert_resource(LaserAssets { mesh, material });
}

fn fire_laser(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut cooldown: ResMut<FireCooldown>,
    assets: Res<LaserAssets>,
    shooters: Query<&Transform, (With<Unit>, With<Selected>)>,
) {
    cooldown.0.tick(time.delta());
    if !keys.pressed(KeyCode::Space) || !cooldown.0.just_finished() {
        return;
    }

    // Every selected unit fires a bolt from its own muzzle along its facing.
    for unit in &shooters {
        let forward = *unit.forward(); // the unit's facing (local −Z)
        let muzzle = unit.transform_point(crate::squad::MUZZLE_LOCAL);
        commands.spawn((
            Laser {
                velocity: forward * LASER_SPEED,
                life: LASER_LIFE,
            },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(muzzle).looking_to(forward, Vec3::Y),
        ));
    }
}

fn update_lasers(
    mut commands: Commands,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    mut impacts: ResMut<ImpactQueue>,
    mut lasers: Query<(Entity, &mut Transform, &mut Laser)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut laser) in &mut lasers {
        transform.translation += laser.velocity * dt;
        laser.life -= dt;
        // Despawn on lifetime end or when it leaves the floor (into a wall/void cell). The
        // cell test is coarse vs the inset wall slab (≤0.2 tile) but imperceptible at bolt speed.
        let cell = dungeon.world_to_cell(transform.translation);
        let hit_wall = !dungeon.is_floor(cell);
        if laser.life <= 0.0 || hit_wall {
            // Only a real collision (not a mid-air timeout) spawns an impact burst (see `impact_fx`).
            if hit_wall {
                impacts.0.push(transform.translation);
            }
            commands.entity(entity).despawn();
        }
    }
}
