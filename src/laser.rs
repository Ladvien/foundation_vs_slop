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

use crate::ai::field::{Deposit, FieldId, StigDeposits};
use crate::audio::Sfx;
use crate::crab::CrabAttached;
use crate::dungeon::Dungeon;
use crate::enemy::Hostile;
use crate::fog::FogGrid;
use crate::gore::{GoreEvent, GoreKind, GoreQueue};
use crate::health::Health;
use crate::impact_fx::ImpactQueue;
use crate::squad::{Unit, Velocity, UNIT_SPEED};
use crate::util::rand01;

/// Seconds between shots while Space is held (fixed fire rate).
const FIRE_INTERVAL: f32 = 0.15;
/// Bolt travel speed, world units per second.
const LASER_SPEED: f32 = 22.0;
/// Bolt lifetime in seconds (a fallback despawn if it never meets a wall).
const LASER_LIFE: f32 = 1.2;
/// Hit points removed from an enemy per bolt.
const LASER_DAMAGE: f32 = 0.2; // 1/50 power so the swarm survives to be watched (restore 10.0 for real combat)
/// Aim-cone half-angle (radians) for a *stationary* unit — nonzero so even a still squad must work
/// for hits against the small enemy hitbox.
const BASE_SPREAD: f32 = 0.06;
/// Extra aim-cone half-angle added at full movement speed — dominates `BASE_SPREAD`, so a moving unit
/// sprays. Scaled by (unit speed / `UNIT_SPEED`).
const MOVE_SPREAD: f32 = 0.40;
/// A unit only shoots things in its FRONT arc (it faces its travel direction). Targets whose bearing is
/// more than this half-angle off the unit's forward are ignored — so a crab on a unit's back is safe
/// from that unit's own gun (only a teammate facing it can shoot it off). cos(75°) ≈ 0.26.
const FRONT_ARC_COS: f32 = 0.26;
/// When a bolt shoots a crab that's latched onto a squad member, this is the chance it *also* wounds
/// the host (a stray round through the crab into your own guy) and how much it hurts.
const FRIENDLY_FIRE_CHANCE: f32 = 0.2;
const FRIENDLY_FIRE_DAMAGE: f32 = 5.0;
/// THREAT deposited into the stigmergy field per shot fired / per bolt landed — the swarm reads this
/// as danger and (once it has a fear drive) scatters from sustained fire.
const THREAT_PER_SHOT: f32 = 0.6;

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
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    fog: Res<FogGrid>,
    mut cooldown: ResMut<FireCooldown>,
    mut rng: Local<u32>,
    assets: Res<LaserAssets>,
    mut sfx: MessageWriter<Sfx>,
    mut deposits: ResMut<StigDeposits>,
    shooters: Query<(&Transform, &Velocity), (With<Unit>, Without<Hostile>)>,
    enemies: Query<&Transform, (With<Hostile>, Without<Unit>)>,
) {
    // Auto-fire: units shoot on their own at the fixed fire rate — no key to hold. A unit with no
    // visible enemy still holds fire (see the per-unit loop below).
    cooldown.0.tick(time.delta());
    if !cooldown.0.just_finished() {
        return;
    }

    // Auto-aim: each unit locks the nearest enemy it can currently SEE (fog-hidden enemies aren't
    // targeted — RTS partial observability) and fires from its muzzle toward it, scattered by a cone
    // that widens with the unit's speed. A unit with no visible enemy holds fire — one path.
    for (unit, velocity) in &shooters {
        let muzzle = unit.transform_point(crate::squad::MUZZLE_LOCAL);
        // The unit faces its travel direction (local -Z); it can only shoot what's in front of it.
        let forward = (unit.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
        let mut best = f32::MAX;
        let mut target = None;
        for enemy in &enemies {
            if !fog.visible_at(dungeon.world_to_cell(enemy.translation)) {
                continue; // can't shoot what the squad can't see
            }
            // Front-arc gate: ignore anything behind the unit (a crab on its own back is unshootable
            // by itself; a teammate whose front arc covers it can still pick it off).
            let bearing = (enemy.translation - unit.translation).with_y(0.0);
            if bearing.normalize_or(forward).dot(forward) < FRONT_ARC_COS {
                continue;
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
        // Gunfire raises the THREAT field at the shooter — creatures read this as danger (stigmergy).
        deposits.0.push(Deposit {
            pos: unit.translation,
            field: FieldId::THREAT,
            amount: THREAT_PER_SHOT,
        });
    }
}

fn update_lasers(
    mut commands: Commands,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    mut impacts: ResMut<ImpactQueue>,
    mut gore: ResMut<GoreQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut ray_cast: MeshRayCast,
    enemy_ids: Query<Entity, With<Hostile>>,
    mut healths: Query<&mut Health, With<Hostile>>,
    attached: Query<&CrabAttached>,
    mut unit_healths: Query<&mut Health, (With<Unit>, Without<Hostile>)>,
    mut lasers: Query<(Entity, &mut Transform, &mut Laser)>,
    mut deposits: ResMut<StigDeposits>,
    mut rng: Local<u32>,
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
                // Friendly fire: shooting a crab that's latched onto a squad member risks putting the
                // round through it into your own guy (rule 4). Rolls per hit.
                if let Ok(att) = attached.get(*hit_entity)
                    && let Some(host) = att.host
                    && rand01(&mut rng) < FRIENDLY_FIRE_CHANCE
                    && let Ok(mut host_hp) = unit_healths.get_mut(host)
                {
                    host_hp.current -= FRIENDLY_FIRE_DAMAGE;
                }
                // Flesh bleeds: a small blood spray + spatter at the strike point (walls keep the
                // spark burst via `ImpactQueue` below — one job per queue, see `gore`).
                gore.0.push(GoreEvent {
                    pos: hit.point,
                    kind: GoreKind::FleshHit,
                    tint: Color::srgb(0.7, 0.05, 0.05),
                    gib: None,
                    intensity: 0.0, // a flesh hit never shakes the camera (see gore feel layer)
                });
                sfx.write(Sfx::ImpactFlesh);
                // A bolt landing on flesh spikes THREAT where it hit — danger the swarm can read.
                deposits.0.push(Deposit {
                    pos: hit.point,
                    field: FieldId::THREAT,
                    amount: THREAT_PER_SHOT,
                });
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

