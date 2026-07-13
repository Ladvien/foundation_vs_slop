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

use bevy::prelude::*;

use crate::ai::field::{sort_deposits, Deposit, FieldId, StigDeposits};
use crate::audio_tuning::AudioTuning;
use crate::audio::Sfx;
use crate::crab::CrabAttached;
use crate::dungeon::Dungeon;
use crate::enemy::{Hostile, LastAttacker, SmileyState};
use crate::fog::FogGrid;
use crate::gore::{GoreEvent, GoreKind, GoreQueue};
use crate::health::Health;
use crate::impact_fx::ImpactQueue;
use crate::behavior_tuning::BehaviorTuning;
use crate::sim::SimTuning;
use crate::squad::{AimTarget, Unit, Velocity};
use crate::util::rand01;

// Ballistics + aim cone (fire cadence, bolt speed/life, spread model, front arc) now live in the
// `behavior:` config slice (`BehaviorTuning::laser`); read via `Res<BehaviorTuning>` in `fire_laser` and
// (for the fire cadence) from `GameConfig` at plugin build. Damage/friendly-fire stay in `sim.combat`.

/// Half-width of a bolt for wall sweeps (thin — it's a bolt). Only used by `resolve_move` so the bolt
/// stops on the room-side wall face rather than passing through the slab. Pure bolt geometry — stays in code.
const LASER_HALF: f32 = 0.02;

/// A live laser bolt: its constant velocity, remaining lifetime (seconds), and the unit that fired it
/// (so a bolt that strikes the smiley watcher can attribute the hit to its real shooter — the watcher
/// only ever retaliates against who actually attacked it; see `enemy::LastAttacker`).
#[derive(Component)]
struct Laser {
    velocity: Vec3,
    life: f32,
    shooter: Entity,
}

/// Shared bolt mesh + emissive material, built once so every bolt is a cheap handle clone.
#[derive(Resource)]
struct LaserAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// Fixed-rate fire gate. Repeating: it ticks every frame and wraps every `behavior.laser.fire_interval`;
/// a shot is emitted on each wrap tick while Space is held.
#[derive(Resource)]
struct FireCooldown(Timer);

/// Deterministic laser RNG state, held as a resource rather than per-system `Local<u32>` so it is part
/// of the simulation state — snapshotable and reset per run (a `Local` lives outside every component and
/// cannot be captured, which would break replay). Two independent LCG streams so the aim-scatter draws
/// (in `fire_laser`) and the friendly-fire rolls (in `update_lasers`) never interleave regardless of
/// system order — the result is the same whichever runs first.
#[derive(Resource)]
pub struct LaserRng {
    /// Aim-cone scatter stream (`fire_laser`).
    aim: u32,
    /// Friendly-fire roll stream (`update_lasers`).
    friendly: u32,
}

impl Default for LaserRng {
    fn default() -> Self {
        // Fixed, non-zero seeds — deterministic across runs. (Distinct so the two streams decorrelate.)
        Self { aim: 0x1234_5678, friendly: 0x9ABC_DEF0 }
    }
}

/// A hostile's hit volume for the CPU bolt cast: a vertical capsule (a sphere when `half_height == 0`)
/// whose core is centred on the entity's `Transform.translation`. Sized to the entity's actual collider
/// (enemy capsule r=0.18/half-len 0.45; crab sphere r=0.30; nest dome r=0.40) so bolts connect the same
/// way the old `MeshRayCast` did — but on the CPU, with no render dependency and fully deterministic.
#[derive(Component)]
pub struct LaserTarget {
    pub radius: f32,
    pub half_height: f32,
}

pub struct LaserPlugin;

impl Plugin for LaserPlugin {
    fn build(&self, app: &mut App) {
        // Fire cadence comes from the `behavior:` config slice (same one-path `GameConfig` seam as the AI
        // tuning). Read once at build; `fire_laser` reads the rest of the ballistics per-tick.
        let fire_interval = app
            .world()
            .resource::<crate::config::GameConfig>()
            .behavior
            .laser
            .fire_interval;
        app.insert_resource(FireCooldown(Timer::from_seconds(
            fire_interval,
            TimerMode::Repeating,
        )))
        .init_resource::<LaserRng>()
        .add_systems(Startup, setup_laser_assets)
        // Pinned sim: firing + bolt motion/hits advance on the fixed timestep (deterministic, frame-rate
        // independent — the CPU raycast and `LaserRng` make this reproducible). `fire_laser` gates on the
        // LOS grid, so it must run after `update_los` writes it this tick (see `fog::LosWritten`).
        .add_systems(
            FixedUpdate,
            (fire_laser.after(crate::fog::LosWritten), update_lasers),
        );
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
        base_color: crate::palette::LASER_BOLT_BASE,
        emissive: crate::palette::LASER_BOLT_EMISSIVE, // red-dominant so it reads as a vivid bolt
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
    mut lrng: ResMut<LaserRng>,
    assets: Res<LaserAssets>,
    mut sfx: MessageWriter<Sfx>,
    mut deposits: ResMut<StigDeposits>,
    mut shooters: Query<
        (Entity, &Transform, &Velocity, &mut AimTarget, &crate::squad_ai::role::RoleId),
        (With<Unit>, Without<Hostile>),
    >,
    // `Option<&SmileyState>` marks the smiley boss among hostiles: the squad leaves the neutral watcher
    // alone and only fires on it once it turns angry (crabs/nests have no `SmileyState` → always targeted).
    enemies: Query<(&Transform, Option<&SmileyState>), (With<Hostile>, Without<Unit>)>,
    sim: Res<SimTuning>,
    audio: Res<AudioTuning>,
    beh: Res<BehaviorTuning>,
) {
    // Auto-fire: units shoot on their own at the fixed fire rate — no key to hold. Target selection runs
    // EVERY tick (so each unit's `AimTarget` — hence its facing, `squad::unit_movement` — stays fresh and
    // it visibly looks at what it shoots), but a bolt only *spawns* on the cooldown wrap tick.
    cooldown.0.tick(time.delta());
    let firing = cooldown.0.just_finished();

    // Auto-aim: each unit locks the nearest enemy it can currently SEE (fog-hidden enemies aren't
    // targeted — RTS partial observability) and fires from its muzzle toward it, scattered by a cone
    // that widens with the unit's speed. A unit with no visible enemy holds fire — one path.
    // Fire-din (`NOISE_SQUAD`) deposits are collected and sorted before queueing — the shooter query
    // order is not stable across App instances, and `drain_deposits` sums the din channel with a
    // non-associative `f32 +=` (see `field::sort_deposits`).
    let mut noise: Vec<Deposit> = Vec::new();
    for (unit_entity, unit, velocity, mut aim_target, role) in &mut shooters {
        // The Researcher (the "Scientist") carries a flashlight, not a blaster — it never fires. Its beam
        // repels light-averse creatures through the `LightField` instead of dealing damage (one path: no
        // "flashlight can also shoot" branch). Gate on the role *value*, which every unit carries, so the
        // hashed squad archetype stays uniform. Its `AimTarget` stays `None` (its spawn value), so
        // `unit_facing` falls through to the flashlight's `FacingOverride`, then to travel direction.
        // Ref: Björk & Michelsen, FDG 2014 — light as a non-lethal deterrent.
        if *role == crate::squad_ai::role::RoleId::Researcher {
            continue;
        }
        let muzzle = unit.transform_point(crate::squad::MUZZLE_LOCAL);
        // The unit faces its travel direction (local -Z); it can only shoot what's in front of it.
        let forward = (unit.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
        let mut best = f32::MAX;
        let mut target: Option<Vec3> = None;
        for (enemy, smiley) in &enemies {
            // Leave the uncanny watcher alone until it reveals itself: only target the smiley boss while
            // it is angry (unleashing). Crabs/nests have no `SmileyState`, so they stay fair game.
            if smiley.is_some_and(|s| !s.is_angry()) {
                continue;
            }
            if !fog.visible_at(dungeon.world_to_cell(enemy.translation)) {
                continue; // can't shoot what the squad can't see
            }
            // Front-arc gate: ignore anything behind the unit (a crab on its own back is unshootable
            // by itself; a teammate whose front arc covers it can still pick it off).
            let bearing = (enemy.translation - unit.translation).with_y(0.0);
            if bearing.normalize_or(forward).dot(forward) < beh.laser.front_arc_cos {
                continue;
            }
            let d = enemy.translation.distance_squared(muzzle);
            // Deterministic tie-break: on an exact distance tie prefer the lower world position, not
            // whichever enemy the query yielded first — query order isn't stable across same-seed runs
            // (see `util::nearest_planar`), and a flipped target aims the bolt at a different hostile.
            let et = enemy.translation;
            let better = match target {
                None => true,
                Some(bt) => {
                    (d.to_bits(), et.x.to_bits(), et.y.to_bits(), et.z.to_bits())
                        < (best.to_bits(), bt.x.to_bits(), bt.y.to_bits(), bt.z.to_bits())
                }
            };
            if better {
                best = d;
                target = Some(et);
            }
        }
        // Face what we shoot (drives the unit's facing in `squad::unit_movement`) — refreshed every tick.
        if aim_target.0 != target {
            aim_target.0 = target;
        }
        let Some(target) = target else {
            continue;
        };
        if !firing {
            continue; // aiming stays fresh, but only fire on the cooldown wrap tick
        }
        let Ok(aim) = Dir3::new(target - muzzle) else {
            continue;
        };
        // Spread grows with (a) the unit's own speed and (b) the target's range — a still unit firing
        // point-blank is crisp; a moving unit shooting a far crab sprays.
        let move_frac = (velocity.0.length() / beh.squad_move.unit_speed).clamp(0.0, 1.0);
        let dist_frac = ((target - muzzle).length() / beh.laser.dist_spread_range).clamp(0.0, 1.0);
        let spread =
            beh.laser.base_spread + beh.laser.move_spread * move_frac + beh.laser.dist_spread * dist_frac;
        let forward = scatter(*aim, spread, &mut lrng.aim);
        commands.spawn((
            Laser {
                velocity: forward * beh.laser.laser_speed,
                life: beh.laser.laser_life,
                shooter: unit_entity,
            },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(muzzle).looking_to(forward, Vec3::Y),
            // Render-only: smooth fast bolt motion across the display refresh (see `lib::run`). Translation
            // only — bolts don't rotate in flight. Component + plugin from avian's interpolation integration.
            avian3d::prelude::TranslationInterpolation,
        ));
        sfx.write(Sfx::Fire(muzzle));
        // Gunfire raises the THREAT field at the shooter — creatures read this as danger (stigmergy).
        deposits.0.push(Deposit {
            pos: unit.translation,
            field: FieldId::THREAT_GUN,
            amount: sim.deposit.threat_per_shot,
        });
        // …and its audible din (`NOISE_SQUAD`) at the same spot — the *sound* of the shot, which the
        // swarm reads as a stimulus (fear and/or investigate). Distinct channel from THREAT_GUN: it
        // propagates on the `audio:` slice's tuning and carries an evolvable perception sign.
        noise.push(Deposit {
            pos: unit.translation,
            field: FieldId::NOISE_SQUAD,
            amount: audio.stimulus.fire_loudness,
        });
    }
    sort_deposits(&mut noise);
    deposits.0.extend(noise);
}

fn update_lasers(
    mut commands: Commands,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    mut impacts: ResMut<ImpactQueue>,
    mut gore: ResMut<GoreQueue>,
    mut sfx: MessageWriter<Sfx>,
    // Every hostile's hit volume (enemy capsule / crab & nest sphere) for the CPU bolt cast.
    // `With<Hostile>` keeps this provably disjoint from the `Without<Hostile>` `lasers` query below
    // (both touch `Transform`), which Bevy's borrow checker requires. The Watching watcher is NO LONGER
    // intangible: a stray/missed squad bolt that strikes it hits + PROVOKES it (recording the shooter as
    // its `LastAttacker` below), so an errant shot landed while the player isn't watching it wakes it and
    // the instakill takes the shooter. `fire_laser` still never AIMS at a Watching watcher, so any hit on
    // it is an accident the player pays for (watch it, or a missed shot gets someone killed).
    targets: Query<(Entity, &Transform, &LaserTarget), With<Hostile>>,
    // The boss's `LastAttacker` working-memory fact — a bolt that hits it records its real shooter here so
    // `enemy::smiley_zap` retaliates against the actual attacker, never a bystander (Orkin 2005).
    mut attackers: Query<&mut LastAttacker>,
    // Nests are `Hostile` (siege-killable) but are stone structures, not flesh — used to suppress the
    // blood/squelch/THREAT reactions on a nest hit while still letting the bolt damage it.
    nests: Query<Entity, With<crate::nest::Nest>>,
    mut healths: Query<&mut Health, With<Hostile>>,
    attached: Query<&CrabAttached>,
    mut unit_healths: Query<&mut Health, (With<Unit>, Without<Hostile>)>,
    mut lasers: Query<(Entity, &mut Transform, &mut Laser), Without<Hostile>>,
    mut deposits: ResMut<StigDeposits>,
    mut lrng: ResMut<LaserRng>,
    // Bundled into one tuple param to stay within Bevy's 16-param system cap (adding `audio` alongside
    // the existing 16 would overflow). Both are read-only config slices.
    (sim, audio): (Res<SimTuning>, Res<AudioTuning>),
) {
    let dt = time.delta_secs();

    // Impact-din (`NOISE_SQUAD`) deposits — flesh and wall strikes — collected and sorted before queueing:
    // the bolt query order is not stable across App instances, and the din channel accumulates with a
    // non-associative `f32 +=` (see `field::sort_deposits`).
    let mut noise: Vec<Deposit> = Vec::new();
    for (entity, mut transform, mut laser) in &mut lasers {
        let prev = transform.translation;
        transform.translation += laser.velocity * dt;
        laser.life -= dt;

        // Enemy hit: sweep this frame's motion segment against every hostile hit-volume on the CPU and
        // take the nearest pierced one (deterministic, render-free — replaces the old `MeshRayCast`). A
        // hit damages that hostile, sprays FX at the strike point, and consumes the bolt.
        let mut best: Option<(Entity, f32, Vec3)> = None;
        for (te, tt, tv) in &targets {
            if let Some((s, point)) =
                segment_capsule_hit(prev, transform.translation, tt.translation, tv.half_height, tv.radius)
            {
                // Deterministic tie-break: equal parametric `s` on two overlapping hostiles resolves by
                // the strike point, not query order (unstable across same-seed runs — see
                // `util::nearest_planar`); a flip changes WHICH hostile the bolt damages.
                let better = match best {
                    None => true,
                    Some((_, bs, bp)) => {
                        (s.to_bits(), point.x.to_bits(), point.y.to_bits(), point.z.to_bits())
                            < (bs.to_bits(), bp.x.to_bits(), bp.y.to_bits(), bp.z.to_bits())
                    }
                };
                if better {
                    best = Some((te, s, point));
                }
            }
        }
        if let Some((hit_entity, _, hit_point)) = best {
            if let Ok(mut hp) = healths.get_mut(hit_entity) {
                hp.current -= sim.combat.laser_damage;
            }
            // If we hit the watcher, record WHO fired this bolt so it retaliates against the real shooter
            // (only the boss carries `LastAttacker`, so this no-ops for crabs/nests).
            if let Ok(mut la) = attackers.get_mut(hit_entity) {
                la.entity = Some(laser.shooter);
                la.age = 0.0;
            }
            // A nest is a stone structure, not flesh: it takes the damage above but must NOT emit the
            // blood spray, fleshy squelch, or a MEAT/THREAT feeding scent. The flesh reactions below are
            // gated on this so only a real creature bleeds.
            let is_nest = nests.contains(hit_entity);
            // Friendly fire: shooting a crab latched onto a squad member risks putting the round through
            // it into your own guy (rule 4). Rolls per hit. (A nest has no host.)
            if let Ok(att) = attached.get(hit_entity)
                && let Some(host) = att.host
                && rand01(&mut lrng.friendly) < sim.combat.friendly_fire_chance
                && let Ok(mut host_hp) = unit_healths.get_mut(host)
            {
                host_hp.current -= sim.combat.friendly_fire_damage;
            }
            if !is_nest {
                // Flesh bleeds: a small blood spray + spatter at the strike point (walls keep the spark
                // burst via `ImpactQueue` below — one job per queue, see `gore`).
                gore.0.push(GoreEvent {
                    pos: hit_point,
                    kind: GoreKind::FleshHit,
                    tint: crate::palette::LASER_SCORCH,
                    gib: None,
                    intensity: 0.0, // a flesh hit never shakes the camera (see gore feel layer)
                });
                sfx.write(Sfx::ImpactFlesh(hit_point));
                // A bolt landing on flesh spikes THREAT where it hit — danger the swarm can read.
                deposits.0.push(Deposit {
                    pos: hit_point,
                    field: FieldId::THREAT_GUN,
                    amount: sim.deposit.threat_per_shot,
                });
                // …and the wet impact's audible din (`NOISE_SQUAD`) at the strike point.
                noise.push(Deposit {
                    pos: hit_point,
                    field: FieldId::NOISE_SQUAD,
                    amount: audio.stimulus.impact_flesh_loudness,
                });
            }
            commands.entity(entity).despawn();
            continue;
        }

        // Wall block: sweep this frame's motion against the wall slabs with `resolve_move`, which stops
        // at the **room-side (inner) wall face** (±0.3 from cell centre) — not the coarse tile boundary,
        // and not the wall centre. If the bolt would have crossed into a wall, it's stopped on that
        // surface and the spark bursts there, in the room, instead of behind/inside the slab.
        let moved = Vec3::new(
            transform.translation.x - prev.x,
            0.0,
            transform.translation.z - prev.z,
        );
        let resolved = dungeon.resolve_move(prev, moved, Vec2::splat(LASER_HALF));
        let hit_wall = (resolved.x - transform.translation.x).abs() > 1.0e-4
            || (resolved.z - transform.translation.z).abs() > 1.0e-4;
        if hit_wall {
            transform.translation.x = resolved.x;
            transform.translation.z = resolved.z;
        }
        if laser.life <= 0.0 || hit_wall {
            // Only a real collision (not a mid-air timeout) spawns an impact burst (see `impact_fx`).
            if hit_wall {
                impacts.0.push(transform.translation);
                sfx.write(Sfx::ImpactWall(transform.translation));
                // The crack of a bolt on stone carries as din (`NOISE_SQUAD`) — quieter than a discharge
                // or a wet hit, but it still marks "a fight is happening here" for the swarm to read.
                noise.push(Deposit {
                    pos: transform.translation,
                    field: FieldId::NOISE_SQUAD,
                    amount: audio.stimulus.impact_wall_loudness,
                });
            }
            commands.entity(entity).despawn();
        }
    }
    sort_deposits(&mut noise);
    deposits.0.extend(noise);
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

/// Does the bolt's motion segment `p0 → p1` pierce a vertical capsule (a sphere when `half_h == 0`) of
/// the given `radius` centred on `center`? Returns `Some((entry, point))` where `entry ∈ [0,1]` is the
/// fraction along the bolt where it *first enters* the volume (surface entry — used to pick the frontmost
/// hostile, so overlapping volumes resolve to the one the bolt reaches first) and `point` is that entry
/// point for FX placement. Pure math: the closest distance between the bolt segment and the capsule's core
/// segment (Ericson, *Real-Time Collision Detection*, §5.1.9 closest-point of two segments) gives the hit
/// test; the entry fraction is the closest-approach parameter minus the half-chord (§5.3.2 ray-vs-sphere).
/// Deterministic and render-free — the CPU replacement for the old `MeshRayCast`.
fn segment_capsule_hit(p0: Vec3, p1: Vec3, center: Vec3, half_h: f32, radius: f32) -> Option<(f32, Vec3)> {
    const EPS: f32 = 1.0e-8;
    let q0 = center - Vec3::Y * half_h;
    let q1 = center + Vec3::Y * half_h;
    let d1 = p1 - p0; // bolt segment
    let d2 = q1 - q0; // capsule core
    let r = p0 - q0;
    let a = d1.dot(d1);
    let e = d2.dot(d2);
    let f = d2.dot(r);

    let (s, t);
    if a <= EPS {
        // Degenerate bolt (no motion this frame): treat as a point vs the core segment.
        s = 0.0;
        t = if e <= EPS { 0.0 } else { (f / e).clamp(0.0, 1.0) };
    } else {
        let c = d1.dot(r);
        if e <= EPS {
            // Sphere (zero-length core): closest point on the bolt to the centre.
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let s0 = if denom > EPS { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            let t0 = (b * s0 + f) / e;
            // Clamp `t` to the core segment and re-derive `s` for that end, per Ericson.
            if t0 < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            } else {
                t = t0;
                s = s0;
            }
        }
    }

    let closest_bolt = p0 + d1 * s;
    let closest_core = q0 + d2 * t;
    let dist2 = (closest_bolt - closest_core).length_squared();
    if dist2 <= radius * radius {
        // Order hits by SURFACE-ENTRY fraction, not closest approach: when two hostile hit-volumes both
        // overlap one bolt segment, the bolt must damage the one it geometrically reaches first. The entry
        // root of the ray-vs-sphere quadratic is the closest-approach parameter `s` minus the half-chord
        // `sqrt(radius² − dist²) / |d1|` (Ericson, §5.3.2). Exact for the sphere/cap branches and for a bolt
        // perpendicular to the capsule axis (this game's near-horizontal bolts vs vertical capsules).
        let entry = if a <= EPS {
            s // Degenerate bolt (no motion this frame): entry coincides with closest approach.
        } else {
            let half = (radius * radius - dist2).max(0.0).sqrt() / a.sqrt();
            (s - half).clamp(0.0, 1.0)
        };
        Some((entry, p0 + d1 * entry))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    // Pure CPU-raycast geometry — no App. Locks the segment-vs-capsule hit test the bolts use so the
    // headless/deterministic laser path can't silently regress (it replaced the render-coupled
    // `MeshRayCast`).
    use super::*;

    #[test]
    fn bolt_through_sphere_center_reports_surface_entry() {
        // Sphere r=0.3 at the origin; a bolt spanning x=-1..1 (length 2) passing through the centre must
        // report the SURFACE-ENTRY fraction (where it first crosses the sphere at x=-0.3 → 0.35), not the
        // mid-segment closest approach (0.5). The returned point is that near-face entry point.
        let hit = segment_capsule_hit(Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, 0.0, 0.3);
        let (entry, point) = hit.expect("a bolt through the centre must hit");
        assert!((entry - 0.35).abs() < 1.0e-4, "entry should be at the near surface, got {entry}");
        assert!((point.x + 0.3).abs() < 1.0e-4, "entry point should be the sphere's near face, got {}", point.x);
    }

    #[test]
    fn overlapping_targets_resolve_to_the_one_entered_first() {
        // Regression for the max-review finding: two hostiles straddle one bolt segment. The old code
        // ordered by closest-approach-to-CENTER and could pick the target whose centre projects earlier
        // even when the bolt's surface reaches the other first. `segment_capsule_hit` now returns the
        // ENTRY fraction, so the smaller value is the target the bolt truly pierces first (the selection
        // loop keeps the min). Bolt travels +X from the origin (x = 4·fraction).
        let p0 = Vec3::new(0.0, 0.0, 0.0);
        let p1 = Vec3::new(4.0, 0.0, 0.0);
        // Big target Y: bolt passes through its centre; near surface at x≈0.70 → entry ≈ 0.175.
        let (entry_y, _) = segment_capsule_hit(p0, p1, Vec3::new(1.6, 0.0, 0.0), 0.0, 0.9).expect("Y must hit");
        // Small target X: its centre projects earlier (x=1.3 → old `s`=0.325 < Y's 0.4, so the OLD metric
        // picked X), but its near surface (x≈1.19) is *behind* Y's → entry ≈ 0.298.
        let (entry_x, _) = segment_capsule_hit(p0, p1, Vec3::new(1.3, 0.28, 0.0), 0.0, 0.3).expect("X must hit");
        assert!(
            entry_y < entry_x,
            "bolt enters the big front target first (entry_y={entry_y}) despite the small target's centre \
             projecting earlier (entry_x={entry_x})"
        );
    }

    #[test]
    fn bolt_missing_sphere_returns_none() {
        // Same bolt, but the sphere is 1 unit off the line and only 0.3 across → clean miss.
        assert!(
            segment_capsule_hit(Vec3::new(-1.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::ZERO, 0.0, 0.3)
                .is_none()
        );
    }

    #[test]
    fn capsule_body_is_taller_than_a_sphere() {
        // Enemy capsule: r=0.18, half-height 0.45, centred at y=0.63 (core spans y 0.18..1.08). A bolt at
        // y=1.0 grazes the capsule but would miss a bare 0.18 sphere at the centre.
        let center = Vec3::new(0.0, 0.63, 0.0);
        let a = Vec3::new(-1.0, 1.0, 0.0);
        let b = Vec3::new(1.0, 1.0, 0.0);
        assert!(segment_capsule_hit(a, b, center, 0.45, 0.18).is_some(), "capsule flank should be hit");
        assert!(segment_capsule_hit(a, b, center, 0.0, 0.18).is_none(), "a point-sphere would miss high");
    }

    #[test]
    fn hit_is_deterministic() {
        let call =
            || segment_capsule_hit(Vec3::new(-1.0, 0.2, 0.1), Vec3::new(1.0, 0.1, -0.1), Vec3::ZERO, 0.3, 0.3);
        assert_eq!(call(), call());
    }
}

