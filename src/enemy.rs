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
use crate::sim::SimTuning;
use crate::ai::brain::ActiveBehavior;
use crate::ai::utility::Mode;
use crate::squad::Unit;
use crate::util::{rand01, smoothstep};

/// How many enemies to place at startup. Exactly one — a single boss smiley that is as strong as a
/// whole pack combined (see `START_HP` / `CONTACT_DPS`). There is no respawn, so this is one for the
/// whole run.
const ENEMY_COUNT: usize = 1;
// Momentum charge (steering with acceleration + heading persistence; Wang, Kearney, Cremer &
// Willemsen, "Steering Behaviors for Autonomous Vehicles in Virtual Environments", IEEE VR 2006,
// DOI 10.1109/vr.2005.69). An enemy crawls at `boss.min_speed`, and while it holds a roughly-straight
// heading it accelerates by `boss.accel` toward `boss.max_speed`; any heading change beyond
// `boss.turn_cos` (heading "unchanged" within ~30°) drops it back to a crawl — so a committed
// straight-line pursuer becomes fast, a wanderer stays slow. Lifted to `behavior.boss`
// (src/behavior_tuning.rs), read via `Res<BehaviorTuning>`.
/// Centre distance at which the boss counts as "in contact" — NO LONGER a damage radius (the contact-gnaw
/// was removed for the watcher rework), it now only tunes the separation/ring behaviour in `enemy_seek`
/// (so arrivers ring the standoff instead of stacking to a point).
const CONTACT_RADIUS: f32 = 1.2;
/// Legacy "bite" DPS — the boss does NOT deal contact damage any more (it's a neutral watcher). This
/// value survives ONLY as the "mass" weight for its own death camera-kick (`gore::death_intensity` in
/// `despawn_dead`): the boss is the heaviest thing in the level, so its death should kick the camera hard.
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
/// Only floor cells at least this far (tiles) from the squad spawn are candidate enemy positions,
/// and accepted enemies are kept at least `SPAWN_SEP` apart so they don't stack.
const MIN_SPAWN_DIST: f32 = 4.0;
const SPAWN_SEP: f32 = 3.0;
// Glance strength (`boss.look_amount`) and grin-on-sight band (`boss.sight_near`/`boss.sight_far`):
// `smile` ramps to a big toothy grin (≈1) as the nearest unit closes inside `sight_near` tiles and
// falls to a frown (0) beyond `sight_far` — uncanny *fixation*, not warmth (direct gaze as a threat cue:
// Trevisan et al., PLoS ONE 2017, DOI 10.1371/journal.pone.0188446; the uncanny valley: Mori). Lifted to
// `behavior.boss` (src/behavior_tuning.rs).
/// Clamp per-frame dt so a hitch can't tunnel an enemy through a wall (mirrors squad movement).
const MAX_FRAME_DT: f32 = 1.0 / 30.0;
// Wander hold (`boss.wander_interval`), and Reynolds separation (`boss.sep_radius`/`boss.sep_strength`):
// enemies closer than sep_radius push apart so a crowd chasing one unit never collapses to a point
// (Reynolds, "Steering Behaviors For Autonomous Characters", GDC 1999). Only the lateral part is applied
// while pursuing (see `enemy_seek`). Lifted to `behavior.boss` (src/behavior_tuning.rs).

// --- Uncanny-watcher reflex tuning (the README rework) ---
// It does NOT hunt the squad to kill (see the removed `enemy_contact_damage`): it wants to keep you
// around to watch. It drifts toward the nearest unit but stops at the `boss.observe_dist` standoff (tiles)
// to stare rather than pinning — a lonely observer, not a predator on contact. Lifted to `behavior.boss`.
/// Observation is the PLAYER's gaze, not an NPC's: the watcher performs its uncanny "friendly" visage for
/// whoever is actually watching it — the human at the camera. [`WatchedByPlayer`] (set windowed-only by
/// [`snapshot_player_gaze`] from the live camera; always `false` in the headless deterministic core) gates
/// the mood ([`next_mood`]): hit WHILE the player is looking at it → Scared (it conceals under a believed
/// gaze); hit while the player's attention is elsewhere → Unleashing. The audience effect keys on *believed
/// direct gaze* (Hamilton & Cañigueral, "The Role of Eye Gaze During Natural Social Interactions", Front.
/// Psychol. 2019, DOI 10.3389/fpsyg.2019.00560). The OLD gaze keyed on whichever squad figurine's auto-aim
/// cone happened to point at it — invisible and arbitrary to the player; keying on the camera makes "if I
/// watch it, it hides" literally true and player-controlled.
///
/// PROVOKE: a squad bolt that STRIKES a Watching watcher now hits it (it is no longer intangible — see
/// [`crate::laser::update_lasers`]), recording the shooter as its [`LastAttacker`]. So a stray/missed shot
/// that lands on it while the player is NOT watching wakes it → it Unleashes and the instakill takes the
/// unit that fired the errant bolt. `fire_laser` still never AIMS at a Watching watcher (the squad won't
/// shoot it on purpose), so any hit is an accident the player pays for: watch it while fighting near it, or
/// a missed shot gets someone killed. A crab bite (`crab::crab_contact_damage`) still provokes it too.
/// Range (cells) the watcher's retaliation reaches — the SAME radius the squad can see (single source of
/// truth: `fog::VISION_RADIUS`), so it can only smite what is within the squad's vision.
const LOOK_RANGE: f32 = crate::fog::VISION_RADIUS as f32;
// Seconds after a hit the watcher stays "attacked" (`boss.hit_memory`), so the reaction persists past the
// single damage tick. Short — this is a reflex, not a mood. Lifted to `behavior.boss`.
/// Range (world units) within which it can smite an attacker with lightning. Clamped to the gaze/vision
/// range so it can never zap a unit beyond the distance at which that unit could ever see it (and thus
/// de-escalate it into cowering) — no deaths to an enemy you can neither see nor stop.
const ZAP_RANGE: f32 = LOOK_RANGE;
/// Duration (seconds) of the "true form" flash on each lightning strike — the ~180 ms glimpse of the
/// fractal sphere the player asked for. Shorter than `ZAP_CADENCE`, so between strikes the angry face
/// shows and only the strike reveals what it is.
const FLASH_TIME: f32 = 0.18;
// Flee speed while scared (`boss.flee_speed`) — faster than its lumber so it actually breaks away.
// Lifted to `behavior.boss`.
/// Seconds a lightning-beam VFX stays on screen.
const LIGHTNING_LIFE: f32 = 0.12;
// --- Bounded self-defence (a *coexisting god*, never a normal boss) ---
// It is intentionally un-killable-by-default and is NOT a required objective, so "leave it alone and it
// just watches you" is always a valid resolution — no soft-lock. Game balance tolerates deliberate
// unkillability (Jaffe et al., "Evaluating Game Balance with Restricted Play", AIIDE 2012) as long as
// nothing forces an inert *stuck* state (a logical bug that renders a game unplayable — Bergdahl et al.,
// "Augmenting Automated Game Testing with Deep RL", 2021). But the crab swarm treats it as `Prey`, so
// without a defence a pile would free-farm it to death (review finding #7). It therefore reflexively
// culls an over-committed crab pile — a mundane swat of bugs, NOT the concealed lightning, so it never
// reveals the true form — with NO heal (the old heal turned it into a crab-farming HP fountain). The
// swat's knobs (threshold/radius/max/cooldown) now live in the `sim:` config slice (`SimTuning::boss`).

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
    /// Counts down while the "true form" is flashed on a bolt: for `FLASH_TIME` after each lightning
    /// strike the face flips to the fractal sphere, then snaps back to the angry face. A ~180 ms glimpse
    /// of what it really is — the concealment cracking for a frame as it strikes.
    flash_timer: f32,
    /// Countdown between anti-swarm crab culls (see `smiley_defense` / `CULL_COOLDOWN`).
    cull_cooldown: f32,
}

impl SmileyState {
    fn new(start_hp: f32, hit_memory: f32) -> Self {
        SmileyState {
            mood: SmileyMood::Watching,
            last_hp: start_hp,
            since_hit: hit_memory, // start un-attacked (>= the hit-memory window)
            zap_cooldown: 0.0,
            flee_timer: 0.0,
            flash_timer: 0.0,
            cull_cooldown: 0.0,
        }
    }

    /// Is it in its hostile "angry" state right now? The squad only opens fire on the watcher once this is
    /// true (see `laser::fire_laser`) — otherwise it's a neutral entity you coexist with, not a target.
    pub fn is_angry(&self) -> bool {
        matches!(self.mood, SmileyMood::Unleashing)
    }

    /// Is it in its default sad-lonely-observer state — the *concealed* calm, before any mask cracks?
    /// The audio layer reads this to swell a legato "uncanny calm" pad while the watcher is quietly
    /// staring (van der Zwaag et al. 2011 — legato → tenderness/sadness), the counterpart to the sharp
    /// percussive reveal when it flips to `Unleashing`.
    pub fn is_watching(&self) -> bool {
        matches!(self.mood, SmileyMood::Watching)
    }
}

/// The watcher's memory of **who actually just hit it** — a working-memory fact (source + recency) in the
/// F.E.A.R. sense (Orkin, "Agent Architecture Considerations for Real-Time Planning in Games", AIIDE 2005:
/// sensors cache facts with a source and an update time). `smiley_zap` retaliates against *this* entity,
/// not against whatever is geometrically nearest — so it can only ever kill the unit or crab that truly
/// attacked it, never a bystander walking past. The two damage sites write it (guarded to the boss):
/// `laser::update_lasers` (the bolt's shooter) and `crab::crab_contact_damage` (the biting crab).
#[derive(Component, Default)]
pub struct LastAttacker {
    /// The entity that most recently damaged the watcher (`None` once the memory goes stale or is spent).
    pub entity: Option<Entity>,
    /// Seconds since it was recorded; cleared once older than `HIT_MEMORY` so a stale attacker isn't zapped.
    pub age: f32,
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

/// The "true form" material: a ray-marched fractal sphere (`attack_sphere.wgsl`, ported CC0 from Otavio
/// Good). The watcher's face **flips to this** the instant it is attacked while unobserved — the concealed
/// power revealed only when no one is watching (audience effect: Hamilton & Cañigueral 2019). It carries
/// no uniforms: the orb is a hard on/off keyed purely on `Visibility` (which also skips the expensive
/// ray-march while hidden), so there is nothing to fade (the old `charge` uniform was redundant with the
/// `Visibility` cut and never produced a partial value).
#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
struct AttackSphereMaterial {}

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
            // The player-gaze fact the reflex reads. Inited here (harness + windowed) so `smiley_reflex`
            // always has it; the WRITER (`snapshot_player_gaze`) is registered windowed-only in `lib::run`,
            // so the deterministic core reads a stable `WatchedByPlayer(false)`.
            .init_resource::<WatchedByPlayer>()
            .add_systems(Startup, (spawn_enemies, setup_lightning_assets))
            // Pinned sim (movement/reflex/AI-driven) on `FixedUpdate`; the `.after(AiSet::Think)` ordering
            // stays valid because `AiSet` is configured on `FixedUpdate` too.
            .add_systems(
                FixedUpdate,
                (
                    // The watcher's aura must reach the grid before it is drained/evaporated this tick.
                    deposit_anomaly_aura.before(crate::ai::AiSet::Deposits),
                    // Rebuild the pursuit field before enemies read it this tick.
                    rebuild_enemy_field,
                    // Reflex first so movement + the zap see this tick's mood.
                    smiley_reflex.after(crate::ai::AiSet::Think),
                    // Smite attackers while unleashing (instakill = pinned state → FixedUpdate).
                    smiley_zap.after(smiley_reflex).in_set(crate::health::HealthDamage),
                    // Bounded no-heal crab cull so the swarm can't free-farm the coexisting god. It TAGS
                    // its victims (`crab::Culled`) rather than despawning them, so it must run before the
                    // one despawn owner — an `insert` command applied after that despawn panics.
                    smiley_defense.before(crate::crab::CrabDespawn).in_set(crate::health::HealthDamage),
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
    sim: Res<SimTuning>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
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
        let attack_material = attack_materials.add(AttackSphereMaterial {});
        commands
            .spawn((
                Enemy,
                Hostile,
                crate::squad::Prey, // crabs swarm the boss too (nearest-prey targeting)
                SmileyState::new(sim.boss.start_hp, beh.boss.hit_memory),
                LastAttacker::default(), // who last hit it (written by the laser + crab damage sites)
                Health::new(sim.boss.start_hp),
                // Grouped so the spawn tuple stays within Bevy's 15-element Bundle limit.
                (
                    crate::ai::drives::Drives::new(), // the boss weighs its own drives (bloodlust, …)
                    // The watcher fears nothing — but it must still be tagged, because every `Drives`
                    // carrier needs a faction (`ai::faction::validate_factions`).
                    crate::ai::faction::Faction::Anomaly,
                ),
                crate::ai::brain::BrainId::Smiley,
                // Single boss → a stable per-spawn seed from its position bits (the seed just needs to be
                // deterministic and distinct; only the swarm needs the monotonic `CrabSpawnSeq`).
                crate::ai::brain::ActiveBehavior::new(pos.x.to_bits() ^ pos.z.to_bits()),
                crate::ai::brain::ThinkTimer::staggered(pos.x.to_bits() ^ pos.z.to_bits()),
                EnemyMotion {
                    speed: beh.boss.min_speed,
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


/// The watcher radiates dread simply by existing. This is the only writer of THREAT_ANOMALY — the channel
/// units read (through walls, via the Psionic) as the boss's presence.
///
/// Before the threat channels were split by emitter, the boss deposited nothing at all while alive: the
/// single `THREAT` channel was written only by the squad's own lasers, so "the boss's aura" that the
/// field's doc-comment advertised did not exist. Determinism: positions are value-sorted before emitting,
/// because overlapping deposit discs accumulate with non-associative float `+=` (the same reason
/// `crab::deposit_crab_fields` and `crab::deposit_meat_scent` sort).
fn deposit_anomaly_aura(
    time: Res<Time>,
    bosses: Query<&Transform, With<Enemy>>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    sim: Res<SimTuning>,
) {
    let amount = sim.deposit.anomaly_aura_rate * time.delta_secs();
    let mut positions: Vec<Vec3> = bosses.iter().map(|tf| tf.translation).collect();
    positions.sort_unstable_by_key(|p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    for pos in positions {
        deposits.0.push(crate::ai::field::Deposit {
            pos,
            field: crate::ai::field::FieldId::THREAT_ANOMALY,
            amount,
        });
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
    let enemy_field = &mut *enemy_field;
    crate::pathfind::rebuild_on_cell_change(
        units.iter().map(|t| dungeon.world_to_cell(t.translation)),
        &mut enemy_field.last_cells,
        false,
        |cells| {
            enemy_field.field = FlowField::build_from(&dungeon, cells).map(Arc::new);
        },
    );
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
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    let boss = &beh.boss;

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
                motion.speed = boss.min_speed;
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
                    motion.speed = boss.flee_speed;
                    let resolved = dungeon.resolve_move(pos, desired3 * (boss.flee_speed * dt), ENEMY_HALF);
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
                    motion.wander_timer = boss.wander_interval;
                    let angle = rand01(&mut motion.rng) * std::f32::consts::TAU;
                    motion.wander_dir = Vec3::new(angle.cos(), 0.0, angle.sin());
                }
                motion.wander_dir.xz()
            }
        };

        // Observation standoff: once within `OBSERVE_DIST` of the unit it's watching, stop closing and
        // just stare. It no longer gnaws on contact (see the removed `enemy_contact_damage`) — it wants
        // to keep you around to watch, so it holds at a creepy distance rather than pinning you. Applies
        // whenever it is *approaching a unit* — both plain `Chase` AND blood-drawn `HuntBlood` (which also
        // homes on a nearby unit), so blood near a unit doesn't make it forget the standoff and pin them.
        let (base, engaged) = if within_standoff(active.mode, nearest_dist, boss.observe_dist) {
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
            if d > 1e-4 && d < boss.sep_radius {
                sep += off / d * (1.0 - d / boss.sep_radius);
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

        let desired = (base + sep * boss.sep_strength).normalize_or_zero();
        if desired == Vec2::ZERO {
            continue; // no pull and no push (e.g. pinning the target) — hold position
        }
        let desired3 = Vec3::new(desired.x, 0.0, desired.y);

        // Charge at full speed only while actually closing on a unit; in contact (ringing/gnawing) or
        // while wandering, settle to a crawl — so arrivers hold the ring and gnaw instead of
        // rocketing around on the separation push.
        let target_speed = if engaged && !in_contact && base != Vec2::ZERO {
            boss.max_speed
        } else {
            boss.min_speed
        };
        let holding = motion.heading != Vec3::ZERO && desired3.dot(motion.heading) >= boss.turn_cos;
        motion.speed = if holding {
            (motion.speed + boss.accel * dt).min(target_speed)
        } else {
            boss.min_speed
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
    camera: Single<&GlobalTransform, With<Camera3d>>,
    enemies: Query<(&Transform, &Children, &SmileyState), With<Enemy>>,
    units: Query<&Transform, (With<Unit>, Without<Enemy>)>,
    mut faces: Query<
        (&mut Transform, &mut Visibility, &MeshMaterial3d<SmileyMaterial>),
        (With<SmileyFace>, Without<Enemy>, Without<Unit>, Without<AttackSphereFace>),
    >,
    mut orbs: Query<
        (&mut Transform, &mut Visibility),
        (With<AttackSphereFace>, Without<Enemy>, Without<Unit>, Without<SmileyFace>),
    >,
    mut face_mats: ResMut<Assets<SmileyMaterial>>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
) {
    let cam_rot = camera.rotation();
    // Face-space axes: since the quad takes the camera rotation, its local right/up are the
    // camera's right/up — project the world glance direction onto those to get the shader `look`.
    let right = camera.right();
    let up = camera.up();

    for (etf, children, state) in &enemies {
        let angry = state.is_angry();
        let scared = state.mood == SmileyMood::Scared;
        // The "true form" is a BRIEF FLASH synced to each strike (~FLASH_TIME), not a sustained orb — AND
        // gated on the *current* mood, so if a gaze snaps it back to Watching/Scared mid-flash the fractal
        // orb can't leak onto a boss the logic now treats as innocent (derive the visual state from the
        // authoritative gameplay state — GameAIPro2 Ch.12 "Separation of Concerns for AI & Animation"; the
        // "turn to look and it's just smiling at you" beat holds).
        let flashing = orb_visible(angry, state.flash_timer);

        // Watch the nearest unit: eyes glance toward it, and the grin SWELLS the closer it is — uncanny
        // fixation, a predator locking on, not warmth (Trevisan 2017; Mori's uncanny valley).
        let (mut look, grin) = match nearest_unit(etf.translation, &units) {
            Some(target) => {
                let mut to = target - etf.translation;
                let glance = Vec2::new(to.dot(*right), to.dot(*up)).normalize_or_zero() * beh.boss.look_amount;
                to.y = 0.0;
                (glance, smoothstep(beh.boss.sight_far, beh.boss.sight_near, to.length()))
            }
            None => (Vec2::ZERO, 0.0),
        };

        // Mood → face uniforms. Angry = a hostile red frown (between strikes). Scared = the panic face
        // (pin-prick pupils, cold pallor). Watching = sad/lonely with no one to fixate on, warming into
        // the swelling grin as a unit nears (eyes cast down when it has nothing to watch).
        let panic = if scared { 1.0 } else { 0.0 };
        let menace = if angry { 1.0 } else { 0.0 };
        let sad = if scared || angry { 0.0 } else { (1.0 - grin).clamp(0.0, 1.0) };
        if !scared && !angry && grin < 0.01 {
            look.y = -beh.boss.look_amount * 0.5;
        }
        let smile = if angry { 0.0 } else { grin * (1.0 - panic) };

        for &child in children {
            if let Ok((mut ftf, mut vis, mat)) = faces.get_mut(child) {
                ftf.rotation = cam_rot; // billboard
                // The face shows in every mood EXCEPT the brief real-form flash.
                let want = if flashing { Visibility::Hidden } else { Visibility::Inherited };
                if *vis != want {
                    *vis = want;
                }
                if let Some(mut m) = face_mats.get_mut(&mat.0) {
                    m.settings.look = look;
                    m.settings.smile = smile;
                    m.settings.panic = panic;
                    m.settings.menace = menace;
                    m.settings.sad = sad;
                }
            } else if let Ok((mut otf, mut vis)) = orbs.get_mut(child) {
                otf.rotation = cam_rot; // billboard
                // The orb shows ONLY during the flash — a hard `Visibility` cut on/off (which also skips
                // its expensive ray-march while hidden). No fade: the flash is only ~FLASH_TIME long.
                let want = if flashing { Visibility::Inherited } else { Visibility::Hidden };
                if *vis != want {
                    *vis = want;
                }
            }
        }
    }
}


/// Despawn enemies whose health has run out, with a parting impact burst (reuses the laser VFX).
fn despawn_dead(
    mut commands: Commands,
    mut gore: ResMut<GoreQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    enemies: Query<(Entity, &Health, &Transform), With<Enemy>>,
    sim: Res<SimTuning>,
) {
    for (entity, hp, tf) in &enemies {
        if hp.current <= 0.0 {
            // Billboard smiley — no mesh to shatter, so a red blood burst + floor pool, no gibs.
            gore.0.push(GoreEvent {
                pos: tf.translation,
                kind: GoreKind::EnemySplat,
                tint: crate::palette::ENEMY_SCORCH,
                gib: None,
                // The boss is the heaviest thing in the level: full camera kick on its death.
                intensity: crate::gore::death_intensity(sim.boss.start_hp, CONTACT_DPS),
            });
            // Blood → SCENT: a death marks a rich feeding site the swarm and boss are drawn to.
            deposits.0.push(crate::ai::field::Deposit {
                pos: tf.translation,
                field: crate::ai::field::FieldId::SCENT,
                amount: sim.deposit.blood_scent,
            });
            sfx.write(Sfx::EnemyDeath(tf.translation));
            commands.entity(entity).despawn();
        }
    }
}

/// Pure gate for the "true form" flash: the fractal orb is visible ONLY while the watcher is actually
/// angry (`Unleashing`) AND inside a strike's flash window. Deriving the visual from the authoritative
/// mood is what stops the orb leaking onto a boss a gaze has snapped back to innocence (Separation of
/// Concerns; GameAIPro2 Ch.12). Unit-tested.
fn orb_visible(angry: bool, flash_timer: f32) -> bool {
    angry && flash_timer > 0.0
}

/// Pure test: should the watcher hold its observation standoff this tick? True while *approaching a unit*
/// — plain `Chase` OR blood-drawn `HuntBlood` (both home on a nearby unit) — and already within
/// `observe_dist` (`behavior.boss.observe_dist`). Keeps it staring at a creepy distance instead of pinning
/// the unit. Unit-tested.
fn within_standoff(mode: Mode, nearest_dist: Option<f32>, observe_dist: f32) -> bool {
    matches!(mode, Mode::Chase | Mode::HuntBlood) && nearest_dist.is_some_and(|d| d <= observe_dist)
}

/// Planar (XZ) distance — the game's actors all sit on the ground plane, so this is the right metric.
fn planar_dist(a: Vec3, b: Vec3) -> f32 {
    (a.xz() - b.xz()).length()
}

/// Whether the PLAYER is currently looking at the watcher — the diegetic "human observer" it performs its
/// uncanny-friendly visage for. Set once per frame by [`snapshot_player_gaze`] (windowed-only, from the
/// live camera) and read by the pinned [`smiley_reflex`]. Defaults `false`; the deterministic harness never
/// registers the snapshot system (no player, and the windowed camera eases over WALL-CLOCK time, which is
/// not reproducible), so the core always reads a stable `false` — the watcher there is permanently
/// "unobserved", which keeps its reflex bit-reproducible.
#[derive(Resource, Default)]
pub struct WatchedByPlayer(pub bool);

// Cosine of the half-angle of the camera-view cone within which the watcher counts as "looked at": the
// player must roughly CENTRE it in view (deliberate attention), not merely have it at the screen edge.
// cos(22°) ≈ 0.927. Windowed-only tuning. Lifted to `behavior.boss.gaze_cos` (src/behavior_tuning.rs).

/// Pure: is `watcher` within the camera's central view cone (a dot-product proxy for "near screen
/// centre")? Split out so the gaze geometry is unit-testable without a live camera.
fn is_watcher_centered(cam_pos: Vec3, cam_fwd: Vec3, watcher: Vec3, cos: f32) -> bool {
    (watcher - cam_pos).normalize_or_zero().dot(cam_fwd) >= cos
}

/// Windowed-only: is the player centring the watcher in the camera view this frame? Writes
/// [`WatchedByPlayer`] for the pinned [`smiley_reflex`] to read. NEVER registered in the headless harness
/// (determinism depends on the core reading a fixed `false` — see [`WatchedByPlayer`]). Uses the camera's
/// view axis as a dot-product cone, so it needs neither the cursor nor the viewport size — just the live
/// camera transform and the watcher positions.
pub fn snapshot_player_gaze(
    mut watched: ResMut<WatchedByPlayer>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    smileys: Query<&Transform, With<Enemy>>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
) {
    // A single game camera in the windowed app; take the first (windowed-only, so iteration order is
    // irrelevant — nothing here is hashed).
    let Some(cam_tf) = camera.iter().next() else {
        watched.0 = false;
        return;
    };
    let cam_pos = cam_tf.translation();
    let cam_fwd = cam_tf.forward();
    watched.0 = smileys
        .iter()
        .any(|stf| is_watcher_centered(cam_pos, *cam_fwd, stf.translation, beh.boss.gaze_cos));
}

/// The watcher's reflex: detect being attacked, read whether the PLAYER is watching it, and set the mood
/// (`Watching`/`Scared`/`Unleashing`). The whole mechanic — concealment under observation —
/// lives here (audience effect: Hamilton & Cañigueral, Front. Psychol. 2019, DOI 10.3389/fpsyg.2019.00560).
fn smiley_reflex(
    time: Res<Time>,
    watched: Res<WatchedByPlayer>,
    mut smileys: Query<(&Health, &mut SmileyState, &mut LastAttacker), With<Enemy>>,
    sim: Res<SimTuning>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
) {
    let dt = time.delta_secs();
    let hit_memory = beh.boss.hit_memory;
    // "Looked at" is now the PLAYER's gaze — a single global fact this tick, set windowed-only by
    // `snapshot_player_gaze` and a stable `false` in the headless core (so the reflex stays
    // bit-reproducible). The watcher performs its friendly mask for the human at the camera, not for
    // whichever figurine's auto-aim happened to point at it.
    let looked_at = watched.0;
    for (hp, mut state, mut attacker) in &mut smileys {
        // Tick down the "true form" flash from the last strike (set by `smiley_zap`).
        state.flash_timer = (state.flash_timer - dt).max(0.0);
        // Age the last-attacker working-memory fact; forget it once stale so a long-gone attacker is never
        // retaliated against (Orkin 2005 — perceptions carry an update time and decay).
        attacker.age += dt;
        if attacker.age > hit_memory {
            attacker.entity = None;
        }
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
        let attacked = state.since_hit < hit_memory;

        // Scared flight persists for a beat after the last hit-while-watched.
        if looked_at && attacked {
            state.flee_timer = sim.boss.scared_time;
        } else {
            state.flee_timer = (state.flee_timer - dt).max(0.0);
        }
        let fleeing = state.flee_timer > 0.0;

        state.mood = next_mood(attacked, looked_at, fleeing);

        // Count the zap cadence down only while unleashing (so the first bolt fires promptly on the flip).
        if state.is_angry() {
            state.zap_cooldown = (state.zap_cooldown - dt).max(0.0);
        } else {
            state.zap_cooldown = 0.0;
        }
    }
}

/// While unleashing, smite the attacker with an instant-kill lightning bolt on a short cadence. The victim
/// is the boss's real `LastAttacker`: a crab that bit it (crab contact is what provokes it in the first
/// place), or a unit that shot it AFTER it was already unleashing (squad bolts land only once it is
/// angry — a Watching boss is intangible). The victim's own per-type despawn system (crab/unit) then
/// removes it with the correct death VFX; here we only set `hp = 0` (pinned) and queue the beam VFX
/// (cosmetic). Deterministic: single recorded attacker, no RNG.
#[allow(clippy::type_complexity)]
fn smiley_zap(
    mut lightning: ResMut<LightningQueue>,
    mut impacts: ResMut<ImpactQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut smileys: Query<(&Transform, &mut SmileyState, &LastAttacker), With<Enemy>>,
    mut crabs: Query<(&Transform, &mut Health), (With<crate::crab::Crab>, Without<Unit>, Without<Enemy>)>,
    mut units: Query<(&Transform, &mut Health), (With<Unit>, Without<crate::crab::Crab>, Without<Enemy>)>,
    sim: Res<SimTuning>,
) {
    for (stf, mut state, attacker) in &mut smileys {
        if !state.is_angry() || state.zap_cooldown > 0.0 {
            continue;
        }
        let spos = stf.translation;

        // Retaliate against the entity that ACTUALLY hit it (its `LastAttacker` working-memory fact;
        // Orkin 2005), never whatever is geometrically nearest — so a bystander unit is never struck. The
        // attacker is a crab or a unit; strike it iff it still exists and is within `ZAP_RANGE`.
        let Some(victim) = attacker.entity else {
            continue; // no recorded attacker → nothing to smite (relaxes as the hit-memory expires)
        };
        let struck = if let Ok((vtf, mut hp)) = crabs.get_mut(victim) {
            let vpos = vtf.translation;
            (planar_dist(vpos, spos) <= ZAP_RANGE).then(|| {
                hp.current = 0.0; // instant kill — the crab's own despawn does gore/SCENT/SFX next tick
                vpos
            })
        } else if let Ok((vtf, mut hp)) = units.get_mut(victim) {
            let vpos = vtf.translation;
            (planar_dist(vpos, spos) <= ZAP_RANGE).then(|| {
                hp.current = 0.0; // the unit that shot it once it was unleashing (its real attacker, not a bystander)
                vpos
            })
        } else {
            None // the attacker already despawned
        };
        let Some(vpos) = struck else {
            continue;
        };

        // Beam from up the watcher's body to the victim + a bright spark at the strike + a sharp report.
        lightning.0.push((spos + Vec3::Y * 0.9, vpos));
        impacts.0.push(vpos);
        sfx.write(Sfx::ImpactWall(vpos)); // a sharp crack — stands in for a dedicated thunder clip
        state.zap_cooldown = sim.boss.zap_cadence;
        state.flash_timer = FLASH_TIME; // flip to the "true form" for ~180 ms as the bolt strikes
    }
}

/// Bounded, no-heal self-defence so the crab swarm can't free-farm the coexisting god (review #7). When
/// an over-committed pile (≥ `CULL_THRESHOLD`) presses against it and its swat is off cooldown, it culls
/// up to `CULL_MAX` of them — a mundane devour of bugs, run regardless of observation (it is NOT the
/// concealed lightning, so it never reveals the true form or breaks concealment). NO heal: the old
/// per-crab heal turned it into a crab-farming HP fountain. Deterministic: crabs are taken in stable
/// query-iteration order, no RNG.
///
/// The cull zeroes each victim's HP and tags it `crate::crab::Culled`, then lets the ONE crab-death
/// despawner (`crab::crab_despawn_dead`) remove it. A single despawn owner means a crab that is also
/// killed by a laser or `smiley_zap` the same tick can't be double-despawned / double-gored. This system is
/// ordered `.before(crab::CrabDespawn)` because tagging is not despawning: a `Culled` insert queued after
/// the despawn command is applied to a dead entity and panics. The `Culled` tag tells that despawner to emit
/// the green-ichor swat gore but NO SCENT bloom — a scent here would just magnet more crabs into a loop.
fn smiley_defense(
    time: Res<Time>,
    mut commands: Commands,
    mut sfx: MessageWriter<Sfx>,
    mut smileys: Query<(&Transform, &mut SmileyState), With<Enemy>>,
    mut crabs: Query<(Entity, &Transform, &mut Health), With<crate::crab::Crab>>,
    sim: Res<SimTuning>,
) {
    let dt = time.delta_secs();
    for (stf, mut state) in &mut smileys {
        state.cull_cooldown = (state.cull_cooldown - dt).max(0.0);
        if state.cull_cooldown > 0.0 {
            continue;
        }
        let spos = stf.translation;
        // Only LIVE crabs are cull candidates — a crab already at 0 HP (a laser or `smiley_zap` kill this
        // tick) is left to `crab_despawn_dead`'s normal death path, so the two never both claim one crab.
        let mut biters: Vec<(Entity, Vec3)> = crabs
            .iter()
            .filter(|(_, ctf, hp)| hp.current > 0.0 && planar_dist(ctf.translation, spos) <= sim.boss.cull_radius)
            .map(|(e, ctf, _)| (e, ctf.translation))
            .collect();
        if biters.len() < sim.boss.cull_threshold {
            continue;
        }
        // Deterministic: cull the first `CULL_MAX` in a STABLE order (by world position), not query
        // order — WHICH crabs die feeds Health/despawn and must not depend on unstable entity ordering
        // (query order is not reproducible across same-seed runs; see `util::nearest_planar`).
        biters.sort_unstable_by_key(|(_, p)| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
        for (ce, _) in biters.into_iter().take(sim.boss.cull_max) {
            if let Ok((_, _, mut hp)) = crabs.get_mut(ce) {
                hp.current = 0.0; // lethal swat — the single crab-death despawner finishes it next tick
            }
            commands.entity(ce).insert(crate::crab::Culled);
        }
        sfx.write(Sfx::ImpactFlesh(spos));
        state.cull_cooldown = sim.boss.cull_cooldown;
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
        base_color: crate::palette::LIGHTNING_BASE,
        emissive: crate::palette::LIGHTNING_EMISSIVE, // electric blue-white, HDR-bright (reads as a bolt)
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

    // Gaze is now the PLAYER's camera (see `snapshot_player_gaze` / `is_watcher_centered`), not a unit body
    // cone. These pin the new camera-cone geometry; `next_mood` still gates on the resulting `looked_at`
    // bool, and the concealment invariant below is unchanged: identical stimulus, opposite response.
    #[test]
    fn gaze_watcher_dead_ahead_is_looked_at() {
        // Camera at origin looking down −Z; a watcher straight ahead is centred in view.
        let gaze_cos = crate::behavior_tuning::BehaviorTuning::default().boss.gaze_cos;
        assert!(is_watcher_centered(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(0.0, 0.0, -5.0), gaze_cos));
    }

    #[test]
    fn gaze_watcher_off_axis_is_not_looked_at() {
        // 45° off the view axis (dot = 0.707) is outside the ~22° central cone (cos ≈ 0.927).
        let gaze_cos = crate::behavior_tuning::BehaviorTuning::default().boss.gaze_cos;
        assert!(!is_watcher_centered(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(-5.0, 0.0, -5.0), gaze_cos));
    }

    #[test]
    fn gaze_watcher_behind_camera_is_not_looked_at() {
        let gaze_cos = crate::behavior_tuning::BehaviorTuning::default().boss.gaze_cos;
        assert!(!is_watcher_centered(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(0.0, 0.0, 5.0), gaze_cos));
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

    // The "true form" never leaks onto an innocent boss: the orb is gated on the CURRENT mood, so a stale
    // flash timer alone can't show it (review finding #4 — the concealment-cracking beat).
    #[test]
    fn orb_shows_only_while_angry_and_flashing() {
        assert!(orb_visible(true, 0.1)); // angry + mid-flash → the fractal sphere shows
        assert!(!orb_visible(false, 0.1)); // gaze snapped it back to innocence mid-flash → NO leak
        assert!(!orb_visible(true, 0.0)); // angry but between strikes → the angry face, not the orb
        assert!(!orb_visible(false, 0.0));
    }

    // The observation standoff holds while approaching a unit in BOTH Chase and HuntBlood (review #5), and
    // not in other modes / when still far.
    #[test]
    fn standoff_covers_chase_and_huntblood_when_close() {
        let od = crate::behavior_tuning::BehaviorTuning::default().boss.observe_dist;
        assert!(within_standoff(Mode::Chase, Some(od - 0.5), od));
        assert!(within_standoff(Mode::HuntBlood, Some(od - 0.5), od)); // the fix: HuntBlood too
        assert!(!within_standoff(Mode::HuntBlood, Some(od + 5.0), od)); // still far → keep closing
        assert!(!within_standoff(Mode::Wander, Some(0.5), od)); // not approaching a unit → no standoff
        assert!(!within_standoff(Mode::Chase, None, od)); // no unit at all
    }

    // The zap can never reach past the range at which a unit could see/de-escalate it (review #9).
    #[test]
    fn zap_range_never_exceeds_vision() {
        assert!(ZAP_RANGE <= LOOK_RANGE, "ZAP_RANGE {ZAP_RANGE} must not exceed gaze/vision {LOOK_RANGE}");
    }
}
