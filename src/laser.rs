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

use crate::ai::field::{Deposit, FieldId, StigDeposits};
use crate::audio::Sfx;
use crate::crab::CrabAttached;
use crate::dungeon::Dungeon;
use crate::enemy::{Hostile, LastAttacker, SmileyState};
use crate::fog::FogGrid;
use crate::gore::{GoreEvent, GoreKind, GoreQueue};
use crate::health::Health;
use crate::impact_fx::ImpactQueue;
use crate::squad::{AimTarget, Unit, Velocity, UNIT_SPEED};
use crate::util::rand01;

/// Seconds between shots while Space is held (fixed fire rate).
const FIRE_INTERVAL: f32 = 0.15;
/// Bolt travel speed, world units per second.
const LASER_SPEED: f32 = 22.0;
/// Bolt lifetime in seconds (a fallback despawn if it never meets a wall).
const LASER_LIFE: f32 = 1.2;
/// Half-width of a bolt for wall sweeps (thin — it's a bolt). Only used by `resolve_move` so the bolt
/// stops on the room-side wall face rather than passing through the slab.
const LASER_HALF: f32 = 0.02;
/// Hit points removed from an enemy per bolt.
const LASER_DAMAGE: f32 = 10.0; // full combat power (~3 hits to down a 25 HP crab); drop to ~0.2 (1/50) to keep the swarm alive for observation
/// Aim-cone half-angle (radians) for a *stationary* unit — nonzero so even a still squad must work
/// for hits against the small enemy hitbox.
const BASE_SPREAD: f32 = 0.06;
/// Extra aim-cone half-angle added at full movement speed — dominates `BASE_SPREAD`, so a moving unit
/// sprays. Scaled by (unit speed / `UNIT_SPEED`).
const MOVE_SPREAD: f32 = 0.40;
/// Extra aim-cone half-angle added at `DIST_SPREAD_RANGE` tiles of target range — distant crabs are
/// harder to hit (accuracy falls off linearly with distance up to this cap). A near target stays crisp.
const DIST_SPREAD: f32 = 0.30;
const DIST_SPREAD_RANGE: f32 = 14.0;
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

/// Fixed-rate fire gate. Repeating: it ticks every frame and wraps every [`FIRE_INTERVAL`];
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
        app.insert_resource(FireCooldown(Timer::from_seconds(
            FIRE_INTERVAL,
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
    mut lrng: ResMut<LaserRng>,
    assets: Res<LaserAssets>,
    mut sfx: MessageWriter<Sfx>,
    mut deposits: ResMut<StigDeposits>,
    mut shooters: Query<(Entity, &Transform, &Velocity, &mut AimTarget), (With<Unit>, Without<Hostile>)>,
    // `Option<&SmileyState>` marks the smiley boss among hostiles: the squad leaves the neutral watcher
    // alone and only fires on it once it turns angry (crabs/nests have no `SmileyState` → always targeted).
    enemies: Query<(&Transform, Option<&SmileyState>), (With<Hostile>, Without<Unit>)>,
) {
    // Auto-fire: units shoot on their own at the fixed fire rate — no key to hold. Target selection runs
    // EVERY tick (so each unit's `AimTarget` — hence its facing, `squad::unit_movement` — stays fresh and
    // it visibly looks at what it shoots), but a bolt only *spawns* on the cooldown wrap tick.
    cooldown.0.tick(time.delta());
    let firing = cooldown.0.just_finished();

    // Auto-aim: each unit locks the nearest enemy it can currently SEE (fog-hidden enemies aren't
    // targeted — RTS partial observability) and fires from its muzzle toward it, scattered by a cone
    // that widens with the unit's speed. A unit with no visible enemy holds fire — one path.
    for (unit_entity, unit, velocity, mut aim_target) in &mut shooters {
        let muzzle = unit.transform_point(crate::squad::MUZZLE_LOCAL);
        // The unit faces its travel direction (local -Z); it can only shoot what's in front of it.
        let forward = (unit.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
        let mut best = f32::MAX;
        let mut target = None;
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
            if bearing.normalize_or(forward).dot(forward) < FRONT_ARC_COS {
                continue;
            }
            let d = enemy.translation.distance_squared(muzzle);
            if d < best {
                best = d;
                target = Some(enemy.translation);
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
        let move_frac = (velocity.0.length() / UNIT_SPEED).clamp(0.0, 1.0);
        let dist_frac = ((target - muzzle).length() / DIST_SPREAD_RANGE).clamp(0.0, 1.0);
        let spread = BASE_SPREAD + MOVE_SPREAD * move_frac + DIST_SPREAD * dist_frac;
        let forward = scatter(*aim, spread, &mut lrng.aim);
        commands.spawn((
            Laser {
                velocity: forward * LASER_SPEED,
                life: LASER_LIFE,
                shooter: unit_entity,
            },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(muzzle).looking_to(forward, Vec3::Y),
            // Render-only: smooth fast bolt motion across the display refresh (see `lib::run`). Translation
            // only — bolts don't rotate in flight. Component + plugin from avian's interpolation integration.
            avian3d::prelude::TranslationInterpolation,
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
    // Every hostile's hit volume (enemy capsule / crab & nest sphere) for the CPU bolt cast.
    // `With<Hostile>` keeps this provably disjoint from the `Without<Hostile>` `lasers` query below
    // (both touch `Transform`), which Bevy's borrow checker requires. `Option<&SmileyState>` marks the
    // boss: a NON-angry watcher is intangible to bolts (they pass through the neutral entity) so a stray
    // shot aimed at a crab can't provoke it — the "leave it alone" rule is enforced at the DAMAGE layer,
    // not just target selection (stimulus gating; GameAIPro2 Ch.27).
    targets: Query<(Entity, &Transform, &LaserTarget, Option<&SmileyState>), With<Hostile>>,
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
) {
    let dt = time.delta_secs();

    for (entity, mut transform, mut laser) in &mut lasers {
        let prev = transform.translation;
        transform.translation += laser.velocity * dt;
        laser.life -= dt;

        // Enemy hit: sweep this frame's motion segment against every hostile hit-volume on the CPU and
        // take the nearest pierced one (deterministic, render-free — replaces the old `MeshRayCast`). A
        // hit damages that hostile, sprays FX at the strike point, and consumes the bolt.
        let mut best: Option<(Entity, f32, Vec3)> = None;
        for (te, tt, tv, smiley) in &targets {
            // A neutral (non-angry) watcher is intangible — bolts pass through it (no damage, no provoke).
            if smiley.is_some_and(|s| !s.is_angry()) {
                continue;
            }
            if let Some((s, point)) =
                segment_capsule_hit(prev, transform.translation, tt.translation, tv.half_height, tv.radius)
                && best.is_none_or(|(_, bs, _)| s < bs)
            {
                best = Some((te, s, point));
            }
        }
        if let Some((hit_entity, _, hit_point)) = best {
            if let Ok(mut hp) = healths.get_mut(hit_entity) {
                hp.current -= LASER_DAMAGE;
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
                && rand01(&mut lrng.friendly) < FRIENDLY_FIRE_CHANCE
                && let Ok(mut host_hp) = unit_healths.get_mut(host)
            {
                host_hp.current -= FRIENDLY_FIRE_DAMAGE;
            }
            if !is_nest {
                // Flesh bleeds: a small blood spray + spatter at the strike point (walls keep the spark
                // burst via `ImpactQueue` below — one job per queue, see `gore`).
                gore.0.push(GoreEvent {
                    pos: hit_point,
                    kind: GoreKind::FleshHit,
                    tint: Color::srgb(0.7, 0.05, 0.05),
                    gib: None,
                    intensity: 0.0, // a flesh hit never shakes the camera (see gore feel layer)
                });
                sfx.write(Sfx::ImpactFlesh);
                // A bolt landing on flesh spikes THREAT where it hit — danger the swarm can read.
                deposits.0.push(Deposit {
                    pos: hit_point,
                    field: FieldId::THREAT,
                    amount: THREAT_PER_SHOT,
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

