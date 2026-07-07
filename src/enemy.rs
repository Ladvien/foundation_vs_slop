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
use crate::impact_fx::ImpactQueue;
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
/// `SIGHT_NEAR` tiles, and falls to a frown (0) beyond `SIGHT_FAR`. The face "lights up" on prey — but
/// as uncanny *fixation*, not warmth: a smile that swells the closer it gets reads as a predator locking
/// on, not a greeting (direct gaze as a threat cue: Trevisan et al., PLoS ONE 2017,
/// DOI 10.1371/journal.pone.0188446; the uncanny valley: Mori).
const SIGHT_NEAR: f32 = 3.0;
const SIGHT_FAR: f32 = 12.0;
/// Clamp per-frame dt so a hitch can't tunnel an enemy through a wall (mirrors squad movement).
const MAX_FRAME_DT: f32 = 1.0 / 30.0;
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

/// --- Uncanny-watcher reflex tuning (the README rework) ---
/// It does NOT hunt the squad to kill (see the removed `enemy_contact_damage`): it wants to keep you
/// around to watch. It drifts toward the nearest unit but stops at this standoff (tiles) to stare rather
/// than pinning — a lonely observer, not a predator on contact.
const OBSERVE_DIST: f32 = 3.0;
/// A unit is "looking directly at" the watcher when the watcher sits within this cosine of the unit's
/// forward AND within `LOOK_RANGE` with a clear line (see `unit_is_facing`). cos(28°) ≈ 0.88 — a tight,
/// deliberate gaze, tighter than the gun's 75° front arc (`laser::FRONT_ARC_COS`). So a unit can be
/// shooting it from an oblique angle *without* "looking directly at it" — which is exactly the moment it
/// unleashes. The audience effect keys on *believed direct gaze*, not mere presence (Hamilton &
/// Cañigueral, "The Role of Eye Gaze During Natural Social Interactions", Front. Psychol. 2019,
/// DOI 10.3389/fpsyg.2019.00560).
const LOOK_COS: f32 = 0.88;
/// Range (tiles) a unit's gaze reaches the watcher (mirrors the fog vision radius, `fog::VISION_RADIUS`).
const LOOK_RANGE: f32 = 8.0;
/// Seconds after a hit the watcher stays "attacked", so the reaction persists past the single damage
/// tick. Short — this is a reflex, not a mood.
const HIT_MEMORY: f32 = 0.6;
/// Seconds it keeps fleeing after the last hit while a unit is watching it.
const SCARED_TIME: f32 = 1.6;
/// Lightning cadence: seconds between instakill bolts while unleashing (one victim per bolt, "if that
/// was the last enemy" it relaxes).
const ZAP_CADENCE: f32 = 0.35;
/// Range (world units) within which it can smite an attacker with lightning.
const ZAP_RANGE: f32 = 16.0;
/// Reach (world units) at which a crab counts as *biting* the watcher — used to attribute a hit to the
/// swarm (zap the crab) vs. a unit's laser (zap the oblique shooter). Matches the crab contact reach.
const BITE_REACH: f32 = 0.9;
/// How fast the attack-sphere "true form" fades in (charge units/s) once unleashing.
const CHARGE_RATE: f32 = 6.0;
/// Flee speed while scared — faster than its lumber so it actually breaks away.
const FLEE_SPEED: f32 = 3.2;
/// Seconds a lightning-beam VFX stays on screen.
const LIGHTNING_LIFE: f32 = 0.12;

/// Marker for an enemy root entity (also the raycast collider — carries the capsule mesh).
#[derive(Component)]
pub struct Enemy;

/// The watcher's reflex state machine (owned by `smiley_reflex`, driven every fixed tick). This is the
/// README's core mechanic: it conceals its power, revealing it only when unobserved. The face visuals
/// (`update_smiley_faces`) and movement (`enemy_seek`) read `mood`; `smiley_zap` fires lightning while
/// `Unleashing`. Grounded in the audience effect (behaviour changes under believed observation: Hamilton
/// & Cañigueral, Front. Psychol. 2019).
#[derive(Component)]
pub struct SmileyState {
    mood: SmileyMood,
    /// Last observed health, to detect a *drop* this tick (order-independent, unlike change-detection):
    /// `current < last_hp` ⇒ it was just attacked. Seeded to spawn HP.
    last_hp: f32,
    /// Seconds since the last detected hit; "attacked" while `< HIT_MEMORY` (a reflex window).
    since_hit: f32,
    /// Countdown between lightning bolts while unleashing.
    zap_cooldown: f32,
    /// Remaining flee time while scared (re-armed each hit-while-watched).
    flee_timer: f32,
}

impl SmileyState {
    fn new(start_hp: f32) -> Self {
        SmileyState {
            mood: SmileyMood::Watching,
            last_hp: start_hp,
            since_hit: HIT_MEMORY, // start un-attacked
            zap_cooldown: 0.0,
            flee_timer: 0.0,
        }
    }
}

/// The watcher's three reflex moods. `Watching` is the default sad-lonely-observer; the other two are
/// the concealed-power reflex, split by whether a squad member is *looking directly at it* when hit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmileyMood {
    /// Idle/observe: sad when alone, drifts to a standoff and stares when it sees a unit.
    Watching,
    /// Attacked *while watched*: plays harmless — panicked face, flees. It won't reveal its power to an
    /// onlooker (self-presentation).
    Scared,
    /// Attacked *while unobserved*: the mask comes off — flips to the fractal-sphere "true form" and
    /// smites the attacker with instant-kill lightning, then relaxes.
    Unleashing,
}

/// Pure transition function for the reflex (unit-tested). `attacked` = took damage inside the memory
/// window; `looked_at` = some unit is gazing directly at it (see `unit_is_facing`); `fleeing` = a scared
/// flight is still in progress. The switch is the whole point: identical stimulus (being hit), opposite
/// response, gated only on whether it is watched.
///
/// "Relax after the last enemy" is emergent, not encoded here: while attackers keep biting they refresh
/// the hit-memory window (`attacked` stays true → stays `Unleashing`), and ~`HIT_MEMORY` after the last
/// one dies the window closes, so it falls back to `Watching` on its own.
fn next_mood(attacked: bool, looked_at: bool, fleeing: bool) -> SmileyMood {
    if looked_at {
        // Watched: it can NEVER unleash (concealment). A hit (or an in-progress flight) makes it cower;
        // otherwise it just watches. A gaze landing mid-unleash snaps it straight back to innocence.
        if attacked || fleeing {
            SmileyMood::Scared
        } else {
            SmileyMood::Watching
        }
    } else if attacked {
        // Unobserved and under attack → drop the mask.
        SmileyMood::Unleashing
    } else {
        SmileyMood::Watching
    }
}

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

/// Marker on the billboard child that renders the smiley face (shown while Watching/Scared).
#[derive(Component)]
struct SmileyFace;

/// Marker on the sibling billboard child that renders the fractal-sphere "true form" (shown while
/// Unleashing). Kept as a second child toggled by `Visibility` rather than swapping the material on one
/// quad — a hard cut that reads as the mask snapping off/on.
#[derive(Component)]
struct AttackSphereFace;

/// Endpoints of lightning bolts to draw this frame — `(from, to)`. `smiley_zap` (pinned) pushes; the
/// cosmetic `drain_lightning` (`Update`) drains and spawns the beam VFX. Decoupled like `ImpactQueue`
/// so the pinned kill stays render-free (the beam has no `Health`, so it never enters `snapshot_hash`).
#[derive(Resource, Default)]
struct LightningQueue(Vec<(Vec3, Vec3)>);

/// Shared beam mesh + emissive material for lightning VFX, built once.
#[derive(Resource)]
struct LightningAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// A live lightning-beam VFX entity; despawns once the clock passes `despawn_at`.
#[derive(Component)]
struct LightningBolt {
    despawn_at: f32,
}

/// GPU uniform — mirrors `SmileySettings` in `smiley.wgsl` (field order + types must match).
#[derive(Clone, ShaderType)]
struct SmileyUniform {
    look: Vec2,
    smile: f32,
    menace: f32,
    /// 0 = normal, 1 = full panic (pin-prick pupils + cold pallor) — the *scared* face.
    panic: f32,
    /// 0 = neutral, 1 = full "saddish" idle (desaturated, dimmed, cooled) — the lonely watcher at rest.
    sad: f32,
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

/// GPU uniform for the attack-sphere "true form" — mirrors `AttackSphereSettings` in
/// `attack_sphere.wgsl`. Padded to 16 bytes (the health-bar pattern).
#[derive(Clone, ShaderType)]
struct AttackSphereUniform {
    /// 0 = invisible, 1 = fully powered up. Ramped by `update_smiley_faces` while unleashing.
    charge: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

/// The "true form" material: a ray-marched fractal sphere (`attack_sphere.wgsl`, ported CC0 from Otavio
/// Good). The watcher's face **flips to this** the instant it is attacked while unobserved — the concealed
/// power revealed only when no one is watching (audience effect: Hamilton & Cañigueral 2019).
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct AttackSphereMaterial {
    #[uniform(0)]
    settings: AttackSphereUniform,
}

impl Material for AttackSphereMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/attack_sphere.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        // Coverage-as-alpha: the orb composites over the scene, the square quad's corners vanish.
        AlphaMode::Blend
    }
}

pub struct EnemyPlugin;

impl Plugin for EnemyPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
                MaterialPlugin::<SmileyMaterial>::default(),
                MaterialPlugin::<AttackSphereMaterial>::default(),
            ))
            .init_resource::<EnemyField>()
            .init_resource::<LightningQueue>()
            .add_systems(Startup, (spawn_enemies, setup_lightning_assets))
            // Pinned sim (movement/reflex/AI-driven) on `FixedUpdate`; the `.after(AiSet::Think)` ordering
            // stays valid because `AiSet` is configured on `FixedUpdate` too.
            .add_systems(
                FixedUpdate,
                (
                    // Rebuild the pursuit field before enemies read it this tick.
                    rebuild_enemy_field,
                    // Reflex first so movement + the zap see this tick's mood.
                    smiley_reflex.after(crate::ai::AiSet::Think),
                    // Smite attackers while unleashing (instakill = pinned state → FixedUpdate).
                    smiley_zap.after(smiley_reflex),
                    // Move after the brain chose this tick's mode and the reflex set the mood.
                    enemy_seek
                        .after(rebuild_enemy_field)
                        .after(smiley_reflex)
                        .after(crate::ai::AiSet::Think),
                    despawn_dead,
                ),
            )
            // Cosmetic: fog toggle, face uniforms + form swap, and the lightning-beam VFX stay on `Update`.
            .add_systems(
                Update,
                (hide_enemies_in_fog, update_smiley_faces, drain_lightning, despawn_lightning),
            );
    }
}

/// Choose spread-out floor cells away from the squad spawn and place a smiley enemy on each.
fn spawn_enemies(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SmileyMaterial>>,
    mut attack_materials: ResMut<Assets<AttackSphereMaterial>>,
) {
    let capsule = meshes.add(Capsule3d::new(CAPSULE_RADIUS, CAPSULE_LENGTH));
    let quad = meshes.add(Rectangle::new(FACE_SIZE, FACE_SIZE));
    // The "true form" reads bigger than the face — a swelling orb when the mask drops.
    let orb_quad = meshes.add(Rectangle::new(FACE_SIZE * 1.6, FACE_SIZE * 1.6));

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
                menace: 0.0, // idle is *sad*, not hostile — see `update_smiley_faces`
                panic: 0.0,
                sad: 1.0, // starts lonely
            },
        });
        // The "true form" material, hidden until it unleashes (charge 0 = fully faded out).
        let attack_material = attack_materials.add(AttackSphereMaterial {
            settings: AttackSphereUniform { charge: 0.0, _pad0: 0.0, _pad1: 0.0, _pad2: 0.0 },
        });
        commands
            .spawn((
                Enemy,
                Hostile,
                crate::squad::Prey, // crabs swarm the boss too (nearest-prey targeting)
                SmileyState::new(START_HP),
                Health::new(START_HP),
                crate::ai::drives::Drives::new(), // the boss weighs its own drives (bloodlust, …)
                crate::ai::brain::BrainId::Smiley,
                // Single boss → a stable per-spawn seed from its position bits (the seed just needs to be
                // deterministic and distinct; only the swarm needs the monotonic `CrabSpawnSeq`).
                crate::ai::brain::ActiveBehavior::new(pos.x.to_bits() ^ pos.z.to_bits()),
                crate::ai::brain::ThinkTimer::staggered(pos.x.to_bits() ^ pos.z.to_bits()),
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
                // Material-less capsule: invisible, but a valid ray-cast collider. Paired with its CPU
                // hit-volume (same dimensions) so laser bolts test against it headlessly + deterministically.
                (
                    Mesh3d(capsule.clone()),
                    crate::laser::LaserTarget { radius: CAPSULE_RADIUS, half_height: CAPSULE_LENGTH * 0.5 },
                ),
                Transform::from_translation(pos),
                // Explicit so `hide_enemies_in_fog` can toggle it; Hidden propagates to BOTH face children.
                Visibility::Inherited,
                // Render-only: smooth the boss's 60 Hz movement across the display refresh (see `lib::run`).
                // Component + plugin come from avian's `bevy_transform_interpolation` integration.
                avian3d::prelude::TransformInterpolation,
            ))
            // The smiley face — shown while Watching/Scared. `Inherited` so `update_smiley_faces` can
            // hide it (and the root's fog toggle still hides it) via `Inherited`/`Hidden`, never `Visible`
            // (which would override the fog `Hidden` and leak the face through the fog).
            .with_child((
                SmileyFace,
                Mesh3d(quad.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(FACE_LOCAL),
                Visibility::Inherited,
            ))
            // The fractal-sphere "true form" — hidden until it unleashes.
            .with_child((
                AttackSphereFace,
                Mesh3d(orb_quad.clone()),
                MeshMaterial3d(attack_material),
                Transform::from_translation(FACE_LOCAL),
                Visibility::Hidden,
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

/// Momentum chase, driven by the brain. An enemy PURSUES its nearest unit — steering along the shared
/// flow field so it routes around walls — whenever the decision layer picks `Chase`/`HuntBlood`
/// (`smiley_brain`: within the distance leash OR standing in the squad's live LOS at any range);
/// otherwise it drifts in a slow WANDER. In both modes a Reynolds separation push keeps enemies from
/// stacking onto one point (so a kill never reveals a full-health "twin"). Speed builds on a held
/// heading and resets to a crawl on a turn
/// (steering-with-acceleration; Wang, Kearney, Cremer & Willemsen, "Steering Behaviors for
/// Autonomous Vehicles in Virtual Environments", IEEE VR 2006, DOI 10.1109/vr.2005.69).
fn enemy_seek(
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    enemy_field: Res<EnemyField>,
    scent_nav: Res<crate::ai::brain::ScentNav>,
    units: Query<&Transform, (With<Unit>, Without<Enemy>)>,
    mut enemies: Query<
        (
            Entity,
            &mut Transform,
            &mut EnemyMotion,
            &ActiveBehavior,
            &SmileyState,
        ),
        With<Enemy>,
    >,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    // Snapshot enemy positions up front for the separation term (read-only); the mutable pass below
    // then moves each enemy. One frame of staleness is fine for a gentle push-apart.
    let positions: Vec<(Entity, Vec2)> = enemies
        .iter()
        .map(|(e, tf, _, _, _)| (e, tf.translation.xz()))
        .collect();

    for (entity, mut tf, mut motion, active, state) in &mut enemies {
        let pos = tf.translation;
        let nearest = nearest_unit(pos, &units);
        let nearest_dist = nearest.map(|t| (t.xz() - pos.xz()).length());

        // The reflex mood overrides the brain's locomotion for the two concealed-power states.
        match state.mood {
            // Rooted while unleashing: it plants itself, sheds its face, and smites (see `smiley_zap`).
            SmileyMood::Unleashing => {
                motion.speed = MIN_SPEED;
                continue;
            }
            // Scared (attacked while watched): it "runs away" — flee straight from the nearest unit.
            SmileyMood::Scared => {
                let away = match nearest {
                    Some(t) => (pos.xz() - t.xz()).normalize_or_zero(),
                    None => Vec2::ZERO,
                };
                if away != Vec2::ZERO {
                    let desired3 = Vec3::new(away.x, 0.0, away.y);
                    motion.heading = desired3;
                    motion.speed = FLEE_SPEED;
                    let resolved = dungeon.resolve_move(pos, desired3 * (FLEE_SPEED * dt), ENEMY_HALF);
                    tf.translation.x = resolved.x;
                    tf.translation.z = resolved.z;
                }
                continue;
            }
            // Watching: fall through to the observe/approach logic below.
            SmileyMood::Watching => {}
        }

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

        // Observation standoff: once within `OBSERVE_DIST` of the unit it's watching, stop closing and
        // just stare. It no longer gnaws on contact (see the removed `enemy_contact_damage`) — it wants
        // to keep you around to watch, so it holds at a creepy distance rather than pinning you.
        let (base, engaged) = if matches!(active.mode, Mode::Chase)
            && nearest_dist.is_some_and(|d| d <= OBSERVE_DIST)
        {
            (Vec2::ZERO, false)
        } else {
            (base, engaged)
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

// NOTE: the watcher no longer gnaws the squad. The old `enemy_contact_damage` (a 72-DPS contact kill)
// was removed for the README rework: "it wants to keep you around, even though it could kill you
// instantly" — a thing that withholds its lethality can't also be chewing your units to death on touch.
// Its only lethality now is the concealed retaliation (`smiley_zap`). `CONTACT_DPS` survives solely as
// the "mass" weight for its own death camera-kick (see `despawn_dead`).

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

/// Billboard the face quads toward the (fixed) iso camera, glance at the nearest unit, and drive the
/// mood-appropriate look — including the **flip** between the smiley face and the fractal-sphere "true
/// form". Reads the reflex `SmileyState.mood` (`smiley_reflex`). Cosmetic → `Update`.
#[allow(clippy::type_complexity)]
fn update_smiley_faces(
    time: Res<Time>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    enemies: Query<(&Transform, &Children, &SmileyState), With<Enemy>>,
    units: Query<&Transform, (With<Unit>, Without<Enemy>)>,
    mut faces: Query<
        (&mut Transform, &mut Visibility, &MeshMaterial3d<SmileyMaterial>),
        (With<SmileyFace>, Without<Enemy>, Without<Unit>, Without<AttackSphereFace>),
    >,
    mut orbs: Query<
        (&mut Transform, &mut Visibility, &MeshMaterial3d<AttackSphereMaterial>),
        (With<AttackSphereFace>, Without<Enemy>, Without<Unit>, Without<SmileyFace>),
    >,
    mut face_mats: ResMut<Assets<SmileyMaterial>>,
    mut orb_mats: ResMut<Assets<AttackSphereMaterial>>,
) {
    let dt = time.delta_secs();
    let cam_rot = camera.rotation();
    // Face-space axes: since the quad takes the camera rotation, its local right/up are the
    // camera's right/up — project the world glance direction onto those to get the shader `look`.
    let right = camera.right();
    let up = camera.up();

    for (etf, children, state) in &enemies {
        let unleashing = state.mood == SmileyMood::Unleashing;
        let scared = state.mood == SmileyMood::Scared;

        // Watch the nearest unit: eyes glance toward it, and the grin SWELLS the closer it is — uncanny
        // fixation, a predator locking on, not warmth (Trevisan 2017; Mori's uncanny valley).
        let (mut look, grin) = match nearest_unit(etf.translation, &units) {
            Some(target) => {
                let mut to = target - etf.translation;
                let glance = Vec2::new(to.dot(*right), to.dot(*up)).normalize_or_zero() * LOOK_AMOUNT;
                to.y = 0.0;
                (glance, smoothstep(SIGHT_FAR, SIGHT_NEAR, to.length()))
            }
            None => (Vec2::ZERO, 0.0),
        };

        // Mood → face uniforms. Scared = the existing panic face (pin-prick pupils, cold pallor).
        // Watching = sad/lonely when it has no one to fixate on, warming into the swelling grin as a unit
        // nears. With nothing to watch, the eyes cast down (a desolate, lonely stare).
        let panic = if scared { 1.0 } else { 0.0 };
        let sad = if scared { 0.0 } else { (1.0 - grin).clamp(0.0, 1.0) };
        if !scared && grin < 0.01 {
            look.y = -LOOK_AMOUNT * 0.5;
        }
        let smile = grin * (1.0 - panic);

        for &child in children {
            if let Ok((mut ftf, mut vis, mat)) = faces.get_mut(child) {
                ftf.rotation = cam_rot; // billboard
                // Shown while Watching/Scared; hidden while the true form is out.
                let want = if unleashing { Visibility::Hidden } else { Visibility::Inherited };
                if *vis != want {
                    *vis = want;
                }
                if let Some(mut m) = face_mats.get_mut(&mat.0) {
                    m.settings.look = look;
                    m.settings.smile = smile;
                    m.settings.panic = panic;
                    m.settings.menace = 0.0; // idle reads *sad*, not hostile; the orb carries the menace
                    m.settings.sad = sad;
                }
            } else if let Ok((mut otf, mut vis, mat)) = orbs.get_mut(child) {
                otf.rotation = cam_rot; // billboard
                let want = if unleashing { Visibility::Inherited } else { Visibility::Hidden };
                if *vis != want {
                    *vis = want;
                }
                if let Some(mut m) = orb_mats.get_mut(&mat.0) {
                    // Fade the orb in as it powers up, snap it out on relax.
                    let target = if unleashing { 1.0 } else { 0.0 };
                    let step = CHARGE_RATE * dt;
                    m.settings.charge += (target - m.settings.charge).clamp(-step, step);
                }
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

/// cos of the gun's front arc — a unit could be shooting whatever sits within this of its forward.
/// Mirrors the private `laser::FRONT_ARC_COS` (cos 75° ≈ 0.26); kept local to avoid making that pub.
const SHOOT_ARC_COS: f32 = 0.26;

/// Pure gaze test: is `target` within `look_cos` of `unit_forward` (a tight "looking directly at it"
/// cone)? Planar (XZ). The caller adds the range + clear-line-of-sight checks. Unit-tested.
fn unit_is_facing(unit_pos: Vec3, unit_forward: Vec3, target_pos: Vec3, look_cos: f32) -> bool {
    let bearing = (target_pos - unit_pos).with_y(0.0).normalize_or_zero();
    if bearing == Vec3::ZERO {
        return true; // on top of the unit — treat as looked at
    }
    let fwd = unit_forward.with_y(0.0).normalize_or(Vec3::NEG_Z);
    bearing.dot(fwd) >= look_cos
}

/// Pure test: is the watcher in this unit's *firing* arc (it could be shooting it) but OUTSIDE its tight
/// gaze cone — i.e. an attacker plinking it from an oblique angle it isn't looking straight at? That is
/// exactly the unobserved attacker it retaliates against. Unit-tested.
fn is_oblique_shooter(unit_pos: Vec3, unit_forward: Vec3, smiley_pos: Vec3, look_cos: f32, arc_cos: f32) -> bool {
    let bearing = (smiley_pos - unit_pos).with_y(0.0).normalize_or_zero();
    if bearing == Vec3::ZERO {
        return false;
    }
    let fwd = unit_forward.with_y(0.0).normalize_or(Vec3::NEG_Z);
    let d = bearing.dot(fwd);
    d >= arc_cos && d < look_cos
}

/// Planar (XZ) distance — the game's actors all sit on the ground plane, so this is the right metric.
fn planar_dist(a: Vec3, b: Vec3) -> f32 {
    (a.xz() - b.xz()).length()
}

/// The watcher's reflex: detect being attacked, decide whether it is being *watched*, and set the mood
/// (`Watching`/`Scared`/`Unleashing`). The whole README mechanic — concealment under observation —
/// lives here (audience effect: Hamilton & Cañigueral, Front. Psychol. 2019, DOI 10.3389/fpsyg.2019.00560).
fn smiley_reflex(
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    units: Query<&Transform, (With<Unit>, Without<Enemy>)>,
    mut smileys: Query<(&Transform, &Health, &mut SmileyState), With<Enemy>>,
) {
    let dt = time.delta_secs();
    for (stf, hp, mut state) in &mut smileys {
        // Attacked = an HP drop since last tick (order-independent, unlike change-detection). The memory
        // window makes the reaction persist past the single damage tick — and, because ongoing bites keep
        // refreshing it, it stays `Unleashing` until ~HIT_MEMORY after the LAST attacker dies (that is the
        // "relax if that was the last enemy" beat, emergent rather than scripted).
        if hp.current < state.last_hp - 1.0e-3 {
            state.since_hit = 0.0;
        } else {
            state.since_hit = (state.since_hit + dt).min(1.0e6);
        }
        state.last_hp = hp.current;
        let attacked = state.since_hit < HIT_MEMORY;

        // Looked at = SOME unit gazes directly at it: tight cone + within range + a clear line (walls
        // break the gaze). Range/LoS in cell space (no tile-size constant needed).
        let scell = dungeon.world_to_cell(stf.translation);
        let looked_at = units.iter().any(|utf| {
            let ucell = dungeon.world_to_cell(utf.translation);
            let forward = utf.rotation * Vec3::NEG_Z;
            (ucell - scell).as_vec2().length() <= LOOK_RANGE
                && unit_is_facing(utf.translation, forward, stf.translation, LOOK_COS)
                && dungeon.line_of_sight(ucell, scell)
        });

        // Scared flight persists for a beat after the last hit-while-watched.
        if looked_at && attacked {
            state.flee_timer = SCARED_TIME;
        } else {
            state.flee_timer = (state.flee_timer - dt).max(0.0);
        }
        let fleeing = state.flee_timer > 0.0;

        state.mood = next_mood(attacked, looked_at, fleeing);

        // Count the zap cadence down only while unleashing (so the first bolt fires promptly on the flip).
        if state.mood == SmileyMood::Unleashing {
            state.zap_cooldown = (state.zap_cooldown - dt).max(0.0);
        } else {
            state.zap_cooldown = 0.0;
        }
    }
}

/// While unleashing, smite the attacker with an instant-kill lightning bolt on a short cadence — a crab
/// biting it, or (per the locked design) a unit plinking it from an unobserved angle. The victim's own
/// per-type despawn system (crab/unit) then removes it with the correct death VFX; here we only set
/// `hp = 0` (pinned) and queue the beam VFX (cosmetic). Deterministic: nearest-by-planar, first-on-tie,
/// no RNG.
#[allow(clippy::type_complexity)]
fn smiley_zap(
    dungeon: Res<Dungeon>,
    mut lightning: ResMut<LightningQueue>,
    mut impacts: ResMut<ImpactQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut smileys: Query<(&Transform, &mut SmileyState), With<Enemy>>,
    mut crabs: Query<(Entity, &Transform, &mut Health), (With<crate::crab::Crab>, Without<Unit>, Without<Enemy>)>,
    mut units: Query<(Entity, &Transform, &mut Health), (With<Unit>, Without<crate::crab::Crab>, Without<Enemy>)>,
) {
    for (stf, mut state) in &mut smileys {
        if state.mood != SmileyMood::Unleashing || state.zap_cooldown > 0.0 {
            continue;
        }
        let spos = stf.translation;
        let scell = dungeon.world_to_cell(spos);

        // Attribute the hit: a crab within bite reach ⇒ the swarm did it (zap the crab); otherwise it was
        // a unit's laser (zap the oblique shooter). This is the geometric stand-in for attacker tracking.
        let biting = crabs.iter().any(|(_, ctf, _)| planar_dist(ctf.translation, spos) <= BITE_REACH);

        let victim: Option<(Entity, Vec3)> = if biting {
            let cand: Vec<(Entity, Vec3)> = crabs
                .iter()
                .filter(|(_, ctf, _)| planar_dist(ctf.translation, spos) <= ZAP_RANGE)
                .map(|(e, ctf, _)| (e, ctf.translation))
                .collect();
            crate::util::nearest_planar(spos, cand.iter().map(|(e, p)| (*e, *p))).map(|(e, p, _)| (e, p))
        } else {
            let cand: Vec<(Entity, Vec3)> = units
                .iter()
                .filter(|(_, utf, _)| {
                    let fwd = utf.rotation * Vec3::NEG_Z;
                    let ucell = dungeon.world_to_cell(utf.translation);
                    planar_dist(utf.translation, spos) <= ZAP_RANGE
                        && is_oblique_shooter(utf.translation, fwd, spos, LOOK_COS, SHOOT_ARC_COS)
                        && dungeon.line_of_sight(ucell, scell)
                })
                .map(|(e, utf, _)| (e, utf.translation))
                .collect();
            crate::util::nearest_planar(spos, cand.iter().map(|(e, p)| (*e, *p))).map(|(e, p, _)| (e, p))
        };

        let Some((victim, vpos)) = victim else {
            continue; // nothing in range to smite this tick — hold (relaxes when the hit-memory expires)
        };

        // Instant kill: the victim's own despawn system does the gore/SCENT/SFX next tick.
        if biting {
            if let Ok((_, _, mut hp)) = crabs.get_mut(victim) {
                hp.current = 0.0;
            }
        } else if let Ok((_, _, mut hp)) = units.get_mut(victim) {
            hp.current = 0.0;
        }

        // Beam from up the watcher's body to the victim + a bright spark at the strike + a sharp report.
        lightning.0.push((spos + Vec3::Y * 0.9, vpos));
        impacts.0.push(vpos);
        sfx.write(Sfx::ImpactWall); // a sharp crack — stands in for a dedicated thunder clip
        state.zap_cooldown = ZAP_CADENCE;
    }
}

/// Build the shared lightning-beam mesh + emissive material once.
fn setup_lightning_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Unit-length on Z so a beam is placed with `looking_at(to)` + `scale.z = length`.
    let mesh = meshes.add(Cuboid::new(0.14, 0.14, 1.0));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.9, 1.0),
        emissive: LinearRgba::rgb(3.0, 6.0, 12.0), // electric blue-white, HDR-bright (reads as a bolt)
        ..default()
    });
    commands.insert_resource(LightningAssets { mesh, material });
}

/// Spawn a short-lived beam for each queued lightning strike (cosmetic → `Update`; no `Health`, so it
/// never enters `snapshot_hash`).
fn drain_lightning(
    mut commands: Commands,
    time: Res<Time>,
    mut queue: ResMut<LightningQueue>,
    assets: Res<LightningAssets>,
) {
    if queue.0.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    for (from, to) in queue.0.drain(..) {
        let len = (to - from).length();
        if len < 1.0e-3 {
            continue; // degenerate — no direction to orient the beam
        }
        let mid = (from + to) * 0.5;
        let mut tf = Transform::from_translation(mid).looking_at(to, Vec3::Y);
        tf.scale = Vec3::new(1.0, 1.0, len); // stretch the unit cuboid to span from→to
        commands.spawn((
            LightningBolt { despawn_at: now + LIGHTNING_LIFE },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            tf,
        ));
    }
}

/// Despawn lightning beams once their brief lifetime elapses.
fn despawn_lightning(mut commands: Commands, time: Res<Time>, bolts: Query<(Entity, &LightningBolt)>) {
    let now = time.elapsed_secs();
    for (entity, bolt) in &bolts {
        if now >= bolt.despawn_at {
            commands.entity(entity).despawn();
        }
    }
}

/// Nearest unit position to `from`, by planar (XZ) distance via the shared [`crate::util::nearest_planar`]
/// ranking. Planar is equivalent to full-3D here because every `Unit` sits at Y=0.0 (`Dungeon::cell_center`
/// seats them there and `squad::resolve_move` only slides X/Z), so the Y term is a constant added to every
/// candidate — it changes neither the argmin nor any tie. Enemies carry a non-floor Y, but an enemy is
/// never a *candidate* here (callers scan `With<Unit>`), only the `from` origin, where the constant lands.
/// NOTE: if units ever leave the floor (jump-pads, flying units), revisit — the metric would then matter.
fn nearest_unit<F: bevy::ecs::query::QueryFilter>(
    from: Vec3,
    units: &Query<&Transform, F>,
) -> Option<Vec3> {
    crate::util::nearest_planar(from, units.iter().map(|t| ((), t.translation))).map(|((), p, _)| p)
}

#[cfg(test)]
mod tests {
    // Pure reflex logic — no App, no ECS (the seed-in/assert-out convention of `ai/utility.rs` and
    // `laser.rs`). Locks the concealment mechanic: identical stimulus, opposite response, gated on gaze.
    use super::*;

    // Units face local −Z; a target straight ahead sits at −Z from the unit.
    #[test]
    fn gaze_directly_ahead_is_looked_at() {
        assert!(unit_is_facing(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(0.0, 0.0, -5.0), LOOK_COS));
    }

    #[test]
    fn gaze_45_degrees_off_is_not_looked_at() {
        // bearing (−1,0,−1)/√2 · forward (0,0,−1) = 0.707 < LOOK_COS (0.88) → not a direct gaze.
        assert!(!unit_is_facing(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(-5.0, 0.0, -5.0), LOOK_COS));
    }

    #[test]
    fn gaze_behind_is_not_looked_at() {
        assert!(!unit_is_facing(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(0.0, 0.0, 5.0), LOOK_COS));
    }

    #[test]
    fn oblique_shooter_sits_between_the_arcs() {
        // 45° off: 0.26 (arc/75°) ≤ 0.707 < 0.88 (gaze/28°) → shooting it, but not looking straight at it.
        assert!(is_oblique_shooter(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(-5.0, 0.0, -5.0), LOOK_COS, SHOOT_ARC_COS));
    }

    #[test]
    fn dead_on_shooter_is_not_oblique() {
        // Straight ahead is a *direct gaze* (it would flee), not an oblique shot.
        assert!(!is_oblique_shooter(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(0.0, 0.0, -5.0), LOOK_COS, SHOOT_ARC_COS));
    }

    #[test]
    fn behind_is_not_a_shooter() {
        // It can't even bring the gun to bear → not an attacker to retaliate against.
        assert!(!is_oblique_shooter(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(0.0, 0.0, 5.0), LOOK_COS, SHOOT_ARC_COS));
    }

    #[test]
    fn watched_and_hit_cowers_never_unleashes() {
        assert_eq!(next_mood(true, true, false), SmileyMood::Scared);
        assert_eq!(next_mood(true, true, true), SmileyMood::Scared);
        // Still fleeing while watched, even between hits.
        assert_eq!(next_mood(false, true, true), SmileyMood::Scared);
    }

    #[test]
    fn unobserved_and_hit_drops_the_mask() {
        assert_eq!(next_mood(true, false, false), SmileyMood::Unleashing);
        assert_eq!(next_mood(true, false, true), SmileyMood::Unleashing);
    }

    #[test]
    fn a_gaze_landing_snaps_it_back_to_innocence() {
        // Was unobserved; a unit now looks and it isn't being hit this instant → back to Watching (the
        // "spin around and it's just smiling at you" beat).
        assert_eq!(next_mood(false, true, false), SmileyMood::Watching);
    }

    #[test]
    fn idle_when_unobserved_and_unhit() {
        assert_eq!(next_mood(false, false, false), SmileyMood::Watching);
    }
}
