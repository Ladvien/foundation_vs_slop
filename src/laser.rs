//! Laser bolts fired from the player's blaster. Hold **Space** and the whole squad **auto-aims** at
//! the nearest enemy it can currently *see* (fog-hidden enemies are not targeted). Aim is imperfect:
//! every bolt scatters inside a random cone, and that cone widens sharply while the unit is moving —
//! so a maneuvering squad sprays wildly and enemies are hard to hit. Bolts despawn on a wall hit, a
//! lifetime timeout, or when they strike an enemy's (small) capsule collider (a `MeshRayCast` against
//! the enemy mesh — see `enemy`), which damages that enemy and spawns an impact burst.
//!
//! Accuracy-while-moving and cone-of-fire are standard shooter practice; the movement penalty and
//! evasive targets are difficulty levers (McKay et al., "Implementing Adaptive Game Difficulty
//! Balancing in Serious Games", IEEE Trans. Games 2018, DOI 10.1109/tg.2018.2791019). Only firing at
//! enemies in live line of sight follows from RTS partial observability (see `fog` / `enemy`).

use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::prelude::*;
use std::collections::HashSet;

use crate::audio::Sfx;
use crate::dungeon::Dungeon;
use crate::enemy::Enemy;
use crate::fog::FogGrid;
use crate::health::Health;
use crate::impact_fx::ImpactQueue;
use crate::squad::{Unit, Velocity, UNIT_SPEED};

/// Seconds between shots while Space is held (fixed fire rate).
const FIRE_INTERVAL: f32 = 0.15;
/// Bolt travel speed, world units per second.
const LASER_SPEED: f32 = 22.0;
/// Bolt lifetime in seconds (a fallback despawn if it never meets a wall).
const LASER_LIFE: f32 = 1.2;
/// Hit points removed from an enemy per bolt.
const LASER_DAMAGE: f32 = 10.0;
/// Aim-cone half-angle (radians) for a *stationary* unit — nonzero so even a still squad must work
/// for hits against the small enemy hitbox.
const BASE_SPREAD: f32 = 0.06;
/// Extra aim-cone half-angle added at full movement speed — dominates `BASE_SPREAD`, so a moving unit
/// sprays. Scaled by (unit speed / `UNIT_SPEED`).
const MOVE_SPREAD: f32 = 0.40;

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

#[allow(clippy::too_many_arguments)]
fn fire_laser(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    fog: Res<FogGrid>,
    mut cooldown: ResMut<FireCooldown>,
    mut rng: Local<u32>,
    assets: Res<LaserAssets>,
    mut sfx: MessageWriter<Sfx>,
    shooters: Query<(&Transform, &Velocity), (With<Unit>, Without<Enemy>)>,
    enemies: Query<&Transform, (With<Enemy>, Without<Unit>)>,
) {
    cooldown.0.tick(time.delta());
    if !keys.pressed(KeyCode::Space) || !cooldown.0.just_finished() {
        return;
    }

    // Auto-aim: each unit locks the nearest enemy it can currently SEE (fog-hidden enemies aren't
    // targeted — RTS partial observability) and fires from its muzzle toward it, scattered by a cone
    // that widens with the unit's speed. A unit with no visible enemy holds fire — one path.
    for (unit, velocity) in &shooters {
        let muzzle = unit.transform_point(crate::squad::MUZZLE_LOCAL);
        let mut best = f32::MAX;
        let mut target = None;
        for enemy in &enemies {
            if !fog.visible_at(dungeon.world_to_cell(enemy.translation)) {
                continue; // can't shoot what the squad can't see
            }
            let d = enemy.translation.distance_squared(muzzle);
            if d < best {
                best = d;
                target = Some(enemy.translation);
            }
        }
        let Some(target) = target else {
            continue;
        };
        let Ok(aim) = Dir3::new(target - muzzle) else {
            continue;
        };
        // Movement penalty: spread grows from BASE_SPREAD (still) toward BASE+MOVE (full speed).
        let move_frac = (velocity.0.length() / UNIT_SPEED).clamp(0.0, 1.0);
        let spread = BASE_SPREAD + MOVE_SPREAD * move_frac;
        let forward = scatter(*aim, spread, &mut rng);
        commands.spawn((
            Laser {
                velocity: forward * LASER_SPEED,
                life: LASER_LIFE,
            },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(muzzle).looking_to(forward, Vec3::Y),
        ));
        sfx.write(Sfx::Fire);
    }
}

fn update_lasers(
    mut commands: Commands,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    mut impacts: ResMut<ImpactQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut ray_cast: MeshRayCast,
    enemy_ids: Query<Entity, With<Enemy>>,
    mut healths: Query<&mut Health, With<Enemy>>,
    mut lasers: Query<(Entity, &mut Transform, &mut Laser)>,
) {
    let dt = time.delta_secs();
    // Restrict ray hits to enemy collider meshes (the material-less capsules on enemy roots).
    let enemy_set: HashSet<Entity> = enemy_ids.iter().collect();
    let is_enemy = |e: Entity| enemy_set.contains(&e);

    for (entity, mut transform, mut laser) in &mut lasers {
        let prev = transform.translation;
        transform.translation += laser.velocity * dt;
        laser.life -= dt;

        // Enemy hit: cast a ray along exactly this frame's motion segment against enemy capsules.
        // `RayCastVisibility::Any` so the (undrawn) collider still registers. A hit damages that
        // enemy, spawns an impact burst at the strike point, and consumes the bolt.
        let step = transform.translation - prev;
        if let Ok(dir) = Dir3::new(step) {
            let travelled = step.length();
            let settings = MeshRayCastSettings::default()
                .with_visibility(RayCastVisibility::Any)
                .with_filter(&is_enemy)
                .always_early_exit();
            if let Some((hit_entity, hit)) = ray_cast.cast_ray(Ray3d::new(prev, dir), &settings).first()
                && hit.distance <= travelled
            {
                if let Ok(mut hp) = healths.get_mut(*hit_entity) {
                    hp.current -= LASER_DAMAGE;
                }
                impacts.0.push(hit.point);
                sfx.write(Sfx::ImpactFlesh);
                commands.entity(entity).despawn();
                continue;
            }
        }

        // Despawn on lifetime end or when it leaves the floor (into a wall/void cell). The
        // cell test is coarse vs the inset wall slab (≤0.2 tile) but imperceptible at bolt speed.
        let cell = dungeon.world_to_cell(transform.translation);
        let hit_wall = !dungeon.is_floor(cell);
        if laser.life <= 0.0 || hit_wall {
            // Only a real collision (not a mid-air timeout) spawns an impact burst (see `impact_fx`).
            if hit_wall {
                impacts.0.push(transform.translation);
                sfx.write(Sfx::ImpactWall);
            }
            commands.entity(entity).despawn();
        }
    }
}

/// Perturb an aim direction inside a cone of half-angle ≈ `spread` (radians): sample a uniform point
/// in a disc of radius `spread` on the plane ⟂ to `dir`, add it, and renormalize. For large `spread`
/// this sprays widely — the "moving = inaccurate" feel.
fn scatter(dir: Vec3, spread: f32, rng: &mut u32) -> Vec3 {
    if spread <= 0.0 {
        return dir;
    }
    let (u, v) = dir.any_orthonormal_pair();
    let r = spread * rand01(rng).sqrt(); // sqrt → uniform over the disc, not clustered at center
    let theta = std::f32::consts::TAU * rand01(rng);
    let offset = u * (r * theta.cos()) + v * (r * theta.sin());
    let jittered = (dir + offset).normalize_or_zero();
    if jittered == Vec3::ZERO {
        dir
    } else {
        jittered
    }
}

/// Cheap LCG (Numerical Recipes constants) producing a float in [0, 1). Full-period from any seed,
/// including the `Local<u32>` default of 0 — no RNG crate, matching the project's hand-rolled `Rng`.
fn rand01(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*state >> 8) as f32 / (1u32 << 24) as f32
}
