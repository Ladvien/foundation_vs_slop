//! Smiley-face slop enemies.
//!
//! Each enemy is a procedural Shadertoy face (see `assets/shaders/smiley.wgsl`) rendered on a
//! camera-facing billboard quad, riding on an invisible **capsule collision mesh** — that capsule
//! is the raycast target the laser uses for hits (see `laser`). The AI is deliberately minimal:
//! shamble slowly toward the nearest squad unit, sliding along dungeon walls with the same
//! collision solver the squad uses ([`Dungeon::resolve_move`]). The face glances at whichever unit
//! it is chasing.
//!
//! Entity shape (one parent per enemy):
//!   root  = `Enemy` + `Health` + a **material-less** `Capsule3d` `Mesh3d`. A mesh with no material
//!           is never drawn, but still carries the `Aabb`/`GlobalTransform` a ray cast needs — so it
//!           is an invisible collider, and the ray hit entity is the enemy itself (no parent lookup).
//!   child = `SmileyFace` billboard quad with the custom [`SmileyMaterial`] (the visible face).
//! Keeping the collider on the root (not `Visibility::Hidden`, which would propagate and hide the
//! child face) is what lets the face show while the capsule stays invisible.

use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use std::sync::Arc;

use crate::audio::Sfx;
use crate::dungeon::Dungeon;
use crate::flowfield::FlowField;
use crate::fog::FogGrid;
use crate::gore::{GoreEvent, GoreKind, GoreQueue};
use crate::health::Health;
use crate::ai::brain::ActiveBehavior;
use crate::ai::utility::Mode;
use crate::squad::Unit;
use crate::util::rand01;

/// How many enemies to place at startup. Exactly one — a single boss smiley that is as strong as a
/// whole pack combined (see `START_HP` / `CONTACT_DPS`). There is no respawn, so this is one for the
/// whole run.
const ENEMY_COUNT: usize = 1;
/// Momentum charge (steering with acceleration + heading persistence; Wang, Kearney, Cremer &
/// Willemsen, "Steering Behaviors for Autonomous Vehicles in Virtual Environments", IEEE VR 2006,
/// DOI 10.1109/vr.2005.69). An enemy crawls at `MIN_SPEED`, and while it holds a roughly-straight
/// heading it accelerates by `ACCEL` toward `MAX_SPEED`; any heading change beyond `TURN_COS` drops
/// it back to a crawl — so a committed straight-line pursuer becomes fast and dangerous, a wanderer
/// stays slow.
const MIN_SPEED: f32 = 0.4; // a slow crawl — the boss lumbers
const MAX_SPEED: f32 = 2.5; // deliberately well under a unit's 6.0 cruise: a lumbering boss you can
                            // kite, but that flattens anything it corners (see `CONTACT_DPS`)
const ACCEL: f32 = 0.4; // world units / s² — a long, low ramp so it never lunges
/// Heading is "unchanged" while the new desired direction is within ~30° of the current one.
const TURN_COS: f32 = 0.87;
/// Centre distance at which an enemy is "in contact" with a unit and starts gnawing on it. A little
/// longer than a unit's so the slow boss can still land hits before it's kited out of reach.
const CONTACT_RADIUS: f32 = 1.2;
/// Damage per second an in-contact enemy deals to a unit (a unit has 100 HP → ~1.4 s to kill). This
/// is the whole pack's bite on one body: being cornered by the boss is near-instant death, so the
/// squad must keep it at range and never let it close.
const CONTACT_DPS: f32 = 72.0;
/// Collision box half-extents for wall-sliding ([`Dungeon::resolve_move`]). Matched to the squad's
/// `0.27` so an enemy fits every corridor a unit fits (a 1-wide walled cell has only
/// `TILE - 2·WALL_THICKNESS = 0.6` clear width) — otherwise a wider enemy wedges at corridor mouths
/// and its flow-field pursuit stalls there. Independent of the raycast collider (how small a target
/// it is) and the face billboard (how big it looks).
const ENEMY_HALF: Vec2 = Vec2::splat(0.27);
/// **Raycast collider** capsule: a deliberately thin, tall core so most jittered bolts sail past —
/// enemies are hard to hit (evasion is a difficulty lever; McKay et al., IEEE Trans. Games 2018,
/// DOI 10.1109/tg.2018.2791019). This is much smaller than the visible face, so you must land a shot
/// on the small center to connect. Total height = length + 2·radius.
const CAPSULE_RADIUS: f32 = 0.18;
const CAPSULE_LENGTH: f32 = 0.9;
/// Y of the capsule center so its base rests on the floor (Y=0). = radius + length/2.
const ENEMY_Y: f32 = CAPSULE_RADIUS + CAPSULE_LENGTH * 0.5;
/// Billboard face size (world units) and its local offset up the capsule toward the "head".
const FACE_SIZE: f32 = 1.6;
const FACE_LOCAL: Vec3 = Vec3::new(0.0, 1.0, 0.0);
/// Starting hit points — a serious bullet sponge. This lone boss carries a whole pack's health on one
/// body (6 × the old 400), so with per-shot spread making most bolts miss it is a long, real fight —
/// not a 3-shot kill (enemy strength as a difficulty variable, McKay et al. 2018).
const START_HP: f32 = 2400.0;
/// Only floor cells at least this far (tiles) from the squad spawn are candidate enemy positions,
/// and accepted enemies are kept at least `SPAWN_SEP` apart so they don't stack.
const MIN_SPAWN_DIST: f32 = 4.0;
const SPAWN_SEP: f32 = 3.0;
/// Glance strength: how far the eyes/head lean toward the tracked unit (shader `look` range ≈ 0.4).
const LOOK_AMOUNT: f32 = 0.35;
/// Grin-on-sight: `smile` ramps to a big toothy grin (≈1) as the nearest unit closes inside
/// `SIGHT_NEAR` tiles, and falls to a frown (0) beyond `SIGHT_FAR`. The face "lights up" on prey.
const SIGHT_NEAR: f32 = 3.0;
const SIGHT_FAR: f32 = 12.0;
/// Hostile red tint always on (see `smiley.wgsl`).
const ENEMY_MENACE: f32 = 0.3;
/// Clamp per-frame dt so a hitch can't tunnel an enemy through a wall (mirrors squad movement).
const MAX_FRAME_DT: f32 = 1.0 / 30.0;
/// Aggro leash: an enemy actively pursues while its nearest unit is within this planar distance
/// Seconds a wandering enemy holds a random heading before re-rolling a new one.
const WANDER_INTERVAL: f32 = 2.5;
/// Enemies closer than this (centre distance) push apart, so a crowd chasing one unit never
/// collapses onto a single point (a killed enemy then can't "reveal a full-health twin" behind it).
/// Reynolds separation steering (Reynolds, "Steering Behaviors For Autonomous Characters", GDC 1999).
const SEP_RADIUS: f32 = 1.2;
/// Weight of the separation push relative to the (unit-length) pursuit/wander direction. Only the
/// lateral part is applied while pursuing (see `enemy_seek`), so this can be strong enough to ring a
/// target without stalling forward progress through corridors.
const SEP_STRENGTH: f32 = 2.5;

/// Marker for an enemy root entity (also the raycast collider — carries the capsule mesh).
#[derive(Component)]
pub struct Enemy;

/// Shared marker for anything the squad can shoot and the fog conceals — the smiley boss *and* the
/// crab swarm (`crate::crab`). Cross-cutting systems (laser hit-scan, fog hiding, combat-music trigger)
/// key on `Hostile` so they treat every threat uniformly through one code path, while type-specific AI
/// stays on `Enemy` / `Crab`.
#[derive(Component)]
pub struct Hostile;

/// An enemy's momentum: its current ground speed and heading. Speed builds while `heading` is held
/// (see the `MIN_SPEED`/`MAX_SPEED`/`ACCEL`/`TURN_COS` charge model) and resets on a turn. Also
/// carries wander state (used only when disengaged) and a per-enemy LCG seed for it.
#[derive(Component)]
struct EnemyMotion {
    speed: f32,
    heading: Vec3,
    /// Current wander heading (ground plane); re-rolled every `WANDER_INTERVAL` while disengaged.
    wander_dir: Vec3,
    /// Countdown to the next wander re-roll.
    wander_timer: f32,
    /// Per-enemy LCG state so wander headings differ between enemies without an RNG crate.
    rng: u32,
}

/// Shared pursuit field: a multi-source [`FlowField`] seeded from every unit's cell, so each enemy
/// paths toward its nearest unit around walls. Rebuilt only when the set of unit cells changes.
#[derive(Resource, Default)]
struct EnemyField {
    field: Option<Arc<FlowField>>,
    /// Sorted unit cells from the last build — skip the rebuild when nothing crossed a boundary.
    last_cells: Vec<IVec2>,
}

/// Marker on the billboard child that renders the smiley face.
#[derive(Component)]
struct SmileyFace;

/// GPU uniform — mirrors `SmileySettings` in `smiley.wgsl` (field order + types must match).
#[derive(Clone, ShaderType)]
struct SmileyUniform {
    look: Vec2,
    smile: f32,
    menace: f32,
}

/// The custom face material (fragment-only; uses Bevy's default mesh vertex pipeline).
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct SmileyMaterial {
    #[uniform(0)]
    settings: SmileyUniform,
}

impl Material for SmileyMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/smiley.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        // Coverage-as-alpha: the round face composites over the scene, the square quad vanishes.
        AlphaMode::Blend
    }
}

pub struct EnemyPlugin;

impl Plugin for EnemyPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SmileyMaterial>::default())
            .init_resource::<EnemyField>()
            .add_systems(Startup, spawn_enemies)
            .add_systems(
                Update,
                (
                    // Rebuild the pursuit field before enemies read it this frame.
                    rebuild_enemy_field,
                    // Move after the brain has chosen this frame's mode (see `crate::ai`).
                    enemy_seek
                        .after(rebuild_enemy_field)
                        .after(crate::ai::AiSet::Think),
                    enemy_contact_damage,
                    hide_enemies_in_fog,
                    update_smiley_faces,
                    despawn_dead,
                ),
            );
    }
}

/// Choose spread-out floor cells away from the squad spawn and place a smiley enemy on each.
fn spawn_enemies(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SmileyMaterial>>,
) {
    let capsule = meshes.add(Capsule3d::new(CAPSULE_RADIUS, CAPSULE_LENGTH));
    let quad = meshes.add(Rectangle::new(FACE_SIZE, FACE_SIZE));

    // Greedily pick floor cells far from spawn and spread apart (deterministic — no RNG).
    let mut chosen: Vec<IVec2> = Vec::new();
    'scan: for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }
            if (cell - dungeon.spawn).as_vec2().length() < MIN_SPAWN_DIST {
                continue;
            }
            if chosen
                .iter()
                .any(|c| (*c - cell).as_vec2().length() < SPAWN_SEP)
            {
                continue;
            }
            chosen.push(cell);
            if chosen.len() >= ENEMY_COUNT {
                break 'scan;
            }
        }
    }

    if chosen.is_empty() {
        warn!("enemy: no floor cell far enough from spawn to place any enemy");
        return;
    }

    for cell in chosen {
        let mut pos = dungeon.cell_center(cell);
        pos.y = ENEMY_Y;
        let material = materials.add(SmileyMaterial {
            settings: SmileyUniform {
                look: Vec2::ZERO,
                smile: 0.0, // updated each frame by `update_smiley_faces` (grin-on-sight)
                menace: ENEMY_MENACE,
            },
        });
        commands
            .spawn((
                Enemy,
                Hostile,
                Health::new(START_HP),
                crate::ai::drives::Drives::new(), // the boss weighs its own drives (bloodlust, …)
                crate::ai::brain::BrainId::Smiley,
                crate::ai::brain::ActiveBehavior::new(pos),
                crate::ai::brain::ThinkTimer::staggered(pos),
                EnemyMotion {
                    speed: MIN_SPEED,
                    heading: Vec3::ZERO,
                    wander_dir: Vec3::ZERO,
                    wander_timer: 0.0,
                    // Deterministic per-spawn seed from the cell coords (odd, nonzero).
                    rng: ((cell.x as u32).wrapping_mul(73_856_093)
                        ^ (cell.y as u32).wrapping_mul(19_349_663))
                        | 1,
                },
                // Material-less capsule: invisible, but a valid ray-cast collider.
                Mesh3d(capsule.clone()),
                Transform::from_translation(pos),
                // Explicit so `hide_enemies_in_fog` can toggle it; Hidden propagates to the face child.
                Visibility::Inherited,
            ))
            .with_child((
                SmileyFace,
                Mesh3d(quad.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(FACE_LOCAL),
            ));
    }
}

/// Rebuild the shared pursuit field when the squad crosses cell boundaries. One multi-source flow
/// field (seeded from every unit cell) lets all enemies path toward their nearest unit *around*
/// walls — the same global navigator the squad uses (see `flowfield`) — so an enemy never pins
/// against a wall the way the old greedy "walk straight at the target" seek did. Reuses the fog
/// system's "unit cells unchanged ⇒ skip" trick so it's one O(cells) build per boundary crossing.
fn rebuild_enemy_field(
    dungeon: Res<Dungeon>,
    units: Query<&Transform, With<Unit>>,
    mut enemy_field: ResMut<EnemyField>,
) {
    let mut cells: Vec<IVec2> = units
        .iter()
        .map(|t| dungeon.world_to_cell(t.translation))
        .collect();
    cells.sort_unstable_by_key(|c| (c.x, c.y));
    cells.dedup();
    if cells == enemy_field.last_cells {
        return;
    }
    enemy_field.field = FlowField::build_from(&dungeon, &cells).map(Arc::new);
    enemy_field.last_cells = cells;
}

/// Momentum chase with a leash. An enemy PURSUES its nearest unit — steering along the shared flow
/// field so it routes around walls — while that unit is within [`CHASE_TILES`] or the enemy stands
/// in the squad's live LOS; otherwise it drifts in a slow WANDER. In both modes a Reynolds
/// separation push keeps enemies from stacking onto one point (so a kill never reveals a
/// full-health "twin"). Speed builds on a held heading and resets to a crawl on a turn
/// (steering-with-acceleration; Wang, Kearney, Cremer & Willemsen, "Steering Behaviors for
/// Autonomous Vehicles in Virtual Environments", IEEE VR 2006, DOI 10.1109/vr.2005.69).
fn enemy_seek(
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    enemy_field: Res<EnemyField>,
    scent_nav: Res<crate::ai::brain::ScentNav>,
    units: Query<&Transform, (With<Unit>, Without<Enemy>)>,
    mut enemies: Query<(Entity, &mut Transform, &mut EnemyMotion, &ActiveBehavior), With<Enemy>>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    // Snapshot enemy positions up front for the separation term (read-only); the mutable pass below
    // then moves each enemy. One frame of staleness is fine for a gentle push-apart.
    let positions: Vec<(Entity, Vec2)> = enemies
        .iter()
        .map(|(e, tf, _, _)| (e, tf.translation.xz()))
        .collect();

    for (entity, mut tf, mut motion, active) in &mut enemies {
        let pos = tf.translation;
        let nearest = nearest_unit(pos, &units);
        let nearest_dist = nearest.map(|t| (t.xz() - pos.xz()).length());

        // The brain (see `crate::ai`) chose the mode; the momentum/charge/wall-slide mechanics below
        // are unchanged — only the *desired direction* comes from the decision now.
        let engaged = matches!(active.mode, Mode::Chase | Mode::HuntBlood);
        let base = match active.mode {
            // Drawn to the biggest blood frenzy — path there around walls via the scent pursuit field
            // (falls back to a straight line at the source cell where the flow is zero).
            Mode::HuntBlood => scent_nav
                .field
                .as_ref()
                .map(|f| f.steer(&dungeon, pos))
                .filter(|d| *d != Vec2::ZERO)
                .or_else(|| active.target.map(|t| (t.xz() - pos.xz()).normalize_or_zero()))
                .unwrap_or(Vec2::ZERO),
            // Pursue the nearest unit around walls via the shared flow field.
            Mode::Chase => enemy_field
                .field
                .as_ref()
                .map(|f| f.steer(&dungeon, pos))
                .unwrap_or(Vec2::ZERO),
            // Wander / anything else: slow drifting heading (re-rolled on a timer).
            _ => {
                motion.wander_timer -= dt;
                if motion.wander_timer <= 0.0 || motion.wander_dir == Vec3::ZERO {
                    motion.wander_timer = WANDER_INTERVAL;
                    let angle = rand01(&mut motion.rng) * std::f32::consts::TAU;
                    motion.wander_dir = Vec3::new(angle.cos(), 0.0, angle.sin());
                }
                motion.wander_dir.xz()
            }
        };

        // Separation: push away from every nearby enemy, stronger the closer it is.
        let mut sep = Vec2::ZERO;
        for (other, opos) in &positions {
            if *other == entity {
                continue;
            }
            let off = pos.xz() - *opos;
            let d = off.length();
            if d > 1e-4 && d < SEP_RADIUS {
                sep += off / d * (1.0 - d / SEP_RADIUS);
            }
        }
        // While *approaching*, strip the part of the push that opposes the pursuit heading so a
        // swarm funnels through corridors without shoving itself backward and stalling. Once at the
        // unit (in contact), apply the full radial push so arrivers ring it instead of clumping.
        let in_contact = nearest_dist.is_some_and(|d| d <= CONTACT_RADIUS + 0.5);
        if !in_contact {
            let fwd = base.normalize_or_zero();
            sep -= fwd * sep.dot(fwd).min(0.0);
        }

        let desired = (base + sep * SEP_STRENGTH).normalize_or_zero();
        if desired == Vec2::ZERO {
            continue; // no pull and no push (e.g. pinning the target) — hold position
        }
        let desired3 = Vec3::new(desired.x, 0.0, desired.y);

        // Charge at full speed only while actually closing on a unit; in contact (ringing/gnawing) or
        // while wandering, settle to a crawl — so arrivers hold the ring and gnaw instead of
        // rocketing around on the separation push.
        let target_speed = if engaged && !in_contact && base != Vec2::ZERO {
            MAX_SPEED
        } else {
            MIN_SPEED
        };
        let holding = motion.heading != Vec3::ZERO && desired3.dot(motion.heading) >= TURN_COS;
        motion.speed = if holding {
            (motion.speed + ACCEL * dt).min(target_speed)
        } else {
            MIN_SPEED
        };
        motion.heading = desired3;

        // Don't overshoot the pursued unit (pin instead of jittering in/out of contact).
        let mut travel = motion.speed * dt;
        if engaged && let Some(d) = nearest_dist {
            travel = travel.min(d);
        }
        let resolved = dungeon.resolve_move(pos, desired3 * travel, ENEMY_HALF);
        // Keep the fixed capsule height; only slide on the ground plane.
        tf.translation.x = resolved.x;
        tf.translation.z = resolved.z;
    }
}

/// Enemies that reach a unit gnaw on it: every unit within [`CONTACT_RADIUS`] loses [`CONTACT_DPS`]
/// while an enemy touches it (stacking if several crowd one unit). This is what gives the squad's
/// health bars stakes — enemy damage output is a core "strength of enemies" difficulty lever (McKay
/// et al., "Implementing Adaptive Game Difficulty Balancing in Serious Games", IEEE Trans. Games
/// 2018, DOI 10.1109/tg.2018.2791019). Dead units are removed by `squad::despawn_dead_units`.
fn enemy_contact_damage(
    time: Res<Time>,
    enemies: Query<&Transform, (With<Enemy>, Without<Unit>)>,
    mut units: Query<(&Transform, &mut Health), (With<Unit>, Without<Enemy>)>,
) {
    let dt = time.delta_secs();
    let reach_sq = CONTACT_RADIUS * CONTACT_RADIUS;
    for enemy in &enemies {
        for (unit_tf, mut hp) in &mut units {
            let mut to = enemy.translation - unit_tf.translation;
            to.y = 0.0; // contact is on the ground plane (enemy capsule vs unit sit at different Y)
            if to.length_squared() <= reach_sq {
                hp.current -= CONTACT_DPS * dt;
            }
        }
    }
}

/// Hide enemies that aren't in the squad's live line of sight — fog of war conceals them, so the
/// player only sees slop that's actually in view. Driven every frame (enemies move in and out of LOS
/// even when the squad's own cell is unchanged, so this can't piggyback on the fog dirty flag).
/// Hiding the root propagates to the face child. This is the partial-observability that defines an
/// RTS (Yang, Xie & Peng, "Fuzzy Theory Based Single Belief State Generation for Partially Observable
/// Real-Time Strategy Games", IEEE Access 2019, DOI 10.1109/access.2019.2923419).
fn hide_enemies_in_fog(
    fog: Res<FogGrid>,
    dungeon: Res<Dungeon>,
    mut enemies: Query<(&Transform, &mut Visibility), With<Hostile>>,
) {
    for (tf, mut vis) in &mut enemies {
        let cell = dungeon.world_to_cell(tf.translation);
        let want = if fog.visible_at(cell) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
    }
}

/// Billboard the face toward the (fixed) iso camera and point the eyes at the nearest unit.
fn update_smiley_faces(
    camera: Single<&GlobalTransform, With<Camera3d>>,
    enemies: Query<(&Transform, &Children), (With<Enemy>, Without<SmileyFace>)>,
    units: Query<&Transform, (With<Unit>, Without<Enemy>, Without<SmileyFace>)>,
    mut faces: Query<
        (&mut Transform, &MeshMaterial3d<SmileyMaterial>),
        (With<SmileyFace>, Without<Enemy>, Without<Unit>),
    >,
    mut materials: ResMut<Assets<SmileyMaterial>>,
) {
    let cam_rot = camera.rotation();
    // Face-space axes: since the quad takes the camera rotation, its local right/up are the
    // camera's right/up — project the world glance direction onto those to get the shader `look`.
    let right = camera.right();
    let up = camera.up();

    for (etf, children) in &enemies {
        // Watch the nearest unit: eyes glance toward it, and the face grins wider the closer it is.
        let (look, smile) = match nearest_unit(etf.translation, &units) {
            Some(target) => {
                let mut to = target - etf.translation;
                let glance = Vec2::new(to.dot(*right), to.dot(*up)).normalize_or_zero() * LOOK_AMOUNT;
                to.y = 0.0;
                // Big grin (→1) up close, frown (→0) far — the face lights up when it sees prey.
                let grin = smoothstep(SIGHT_FAR, SIGHT_NEAR, to.length());
                (glance, grin)
            }
            None => (Vec2::ZERO, 0.0),
        };
        for &child in children {
            let Ok((mut ftf, mat_handle)) = faces.get_mut(child) else {
                continue;
            };
            ftf.rotation = cam_rot;
            if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
                mat.settings.look = look;
                mat.settings.smile = smile;
            }
        }
    }
}


/// GLSL-style `smoothstep` (Hermite ramp), clamped. When `edge0 > edge1` the ramp is reversed, so
/// `smoothstep(FAR, NEAR, d)` rises from 0 at `d = FAR` to 1 at `d = NEAR` — closer ⇒ bigger grin.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Despawn enemies whose health has run out, with a parting impact burst (reuses the laser VFX).
fn despawn_dead(
    mut commands: Commands,
    mut gore: ResMut<GoreQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    enemies: Query<(Entity, &Health, &Transform), With<Enemy>>,
) {
    for (entity, hp, tf) in &enemies {
        if hp.current <= 0.0 {
            // Billboard smiley — no mesh to shatter, so a red blood burst + floor pool, no gibs.
            gore.0.push(GoreEvent {
                pos: tf.translation,
                kind: GoreKind::EnemySplat,
                tint: Color::srgb(0.7, 0.05, 0.05),
                gib: None,
                // The boss is the heaviest thing in the level: full camera kick on its death.
                intensity: crate::gore::death_intensity(START_HP, CONTACT_DPS),
            });
            // Blood → SCENT: a death marks a rich feeding site the swarm and boss are drawn to.
            deposits.0.push(crate::ai::field::Deposit {
                pos: tf.translation,
                field: crate::ai::field::FieldId::SCENT,
                amount: crate::ai::field::BLOOD_SCENT,
            });
            sfx.write(Sfx::EnemyDeath);
            commands.entity(entity).despawn();
        }
    }
}

/// Nearest unit position to `from` (XZ+Y distance is fine — units and enemies share the ground).
fn nearest_unit<F: bevy::ecs::query::QueryFilter>(
    from: Vec3,
    units: &Query<&Transform, F>,
) -> Option<Vec3> {
    let mut best = f32::MAX;
    let mut nearest = None;
    for u in units {
        let d = u.translation.distance_squared(from);
        if d < best {
            best = d;
            nearest = Some(u.translation);
        }
    }
    nearest
}
