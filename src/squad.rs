//! The player's squad: controllable `Unit` characters commanded by the mouse (see `selection`).
//! Movement is the SOTA split of a **flow-field global navigator** (see `flowfield`) feeding a
//! **hand-rolled ORCA local-avoidance** layer (see `orca`): the flow field decides each unit's
//! preferred velocity toward the shared goal, ORCA turns that into a collision-free velocity around
//! the other units, and `Dungeon::resolve_move` keeps it out of walls. This is the planner →
//! preferred-velocity → reciprocal-avoidance pipeline of Treuille et al. (Continuum Crowds,
//! SIGGRAPH 2006, DOI 10.1145/1141911.1142008) and van den Berg et al. (ORCA, 2011,
//! DOI 10.1109/TRO.2011.2120810), and it replaces the earlier summed-force separation that let
//! units cancel to a standstill.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;

use crate::audio::Sfx;
use crate::crab::CrabAttached;
use crate::dungeon::Dungeon;
use crate::flowfield::FlowField;
use crate::gore::{GibSource, GoreEvent, GoreKind, GoreQueue};
use crate::health::Health;
use crate::orca::{self, Agent};
use crate::ai::brain::{ActiveBehavior, ThinkTimer};
use crate::ai::drives::Drives;
use crate::squad_ai::actions::UtterCooldown;
use crate::squad_ai::cohesion::DesiredMove;
use crate::squad_ai::dialogue::MemoryStream;
use crate::squad_ai::persona::load_personas;
use crate::squad_ai::role::RoleId;

/// Marker for a squad member (the RTS unit; replaces the old single-agent `Player`).
#[derive(Component)]
pub struct Unit;

/// Stable 0-based identity of a squad member (matches its `OUTFITS`/spawn index). Lets systems that
/// key on "who" — dialogue speakers, roster chips — resolve a member to its `Entity` without
/// comparing float colors. Assigned once at spawn; never reused.
#[derive(Component)]
pub struct SquadMember(pub usize);

/// Marks the one unit that anchors leader-facing UI (the choice bubbles of a dialogue exchange).
/// Exactly one living unit carries it — [`ensure_leader`] reassigns it if the current leader dies.
#[derive(Component)]
pub struct Leader;

/// Shared marker for anything the crab swarm treats as prey to swarm/latch/bite — squad units AND the
/// smiley boss (`crate::enemy`). Crab targeting keys on `Prey` (nearest wins), so the same forage/latch
/// code path drives crabs onto whichever prey is closest, without knowing its type.
#[derive(Component)]
pub struct Prey;

/// Ground-plane movement speed, world units per second.
#[derive(Component)]
pub struct MoveSpeed(pub f32);

/// The unit's team/outfit color, applied to its figurine once the model loads.
#[derive(Component)]
pub struct Outfit(pub Color);

/// Marks a unit as currently selected (drawn with a green ring, obeys move orders).
#[derive(Component)]
pub struct Selected;

/// An active move order: the shared flow field the unit follows toward the group's goal, plus a
/// small amount of follower state. One `FlowField` is built per command and shared (`Arc`) by every
/// unit in the selection, so hundreds of units cost one field build, not one A\* per unit.
#[derive(Component)]
pub struct MoveOrder {
    pub field: Arc<FlowField>,
    /// Closest the unit has ever gotten to the goal on this order (world distance).
    best_dist: f32,
    /// Seconds since `best_dist` last improved — a *progress*-based stall measure (a unit milling in
    /// place at non-zero speed still counts as stalled), driving packed-in and give-up arrival.
    no_progress_time: f32,
}

impl MoveOrder {
    pub fn new(field: Arc<FlowField>) -> Self {
        MoveOrder {
            field,
            best_dist: f32::MAX,
            no_progress_time: 0.0,
        }
    }
}

/// A unit's current planar velocity (xz), advertised to ORCA so neighbors can reciprocate. Held on
/// every unit (zero while idle) since idle units are still avoided.
#[derive(Component)]
pub struct Velocity(pub Vec2);

/// The world position this unit's gun is currently aimed at — the nearest enemy it can shoot, written
/// every tick by `laser::fire_laser` (`None` when holding fire). `unit_facing` turns the figurine to look
/// at it, so a unit visibly faces what it shoots (combat readability) and the smiley watcher's gaze test
/// (`enemy::unit_is_facing`) matches what the player sees — body facing == aim (Rabin, "Vision Zones",
/// GameAIPro2 Ch.4).
#[derive(Component, Default)]
pub struct AimTarget(pub Option<Vec3>);

/// Marks the gun sub-model so the outfit recolor skips it (the blaster keeps its own colors) and so
/// `autogib` can bake it as a separate intact chunk instead of folding it into the body fracture.
#[derive(Component)]
pub struct GunModel;

/// Marks a unit whose figurine has already been recolored (so the one-shot recolor runs once).
#[derive(Component)]
struct Recolored;

/// Scale the figurine to a ~6 ft squad member: the base mesh is 0.7 m tall, so 0.7 × 2.6 ≈ 1.82 m —
/// about three-quarters of the 2.4 m (~8 ft) ceiling, a believable human proportion. Uniform, so the
/// carried gun and the autogib fragments stay proportional. Collision (`UNIT_HALF_EXTENTS`) stays
/// narrower than the visual on purpose — see below.
const FIGURINE_SCALE: f32 = 2.6;
/// Square collision half-extent. Sized well under the narrowest walkable channel so units don't
/// wedge/catch in 1-tile doorways: a doorway walled on both sides has `TILE - 2·WALL_THICKNESS = 0.6`
/// of clear width, and 0.44-wide unit leaves ~0.08 m of slack per side to slide through cleanly. Well
/// under the figurine's visual radius on purpose — reaching the goal reliably beats pixel-exact
/// contact, and the visual is far wider anyway.
const UNIT_HALF_EXTENTS: Vec2 = Vec2::splat(0.22);
/// RTS walk speed (a touch slower than the old run pace — these are commanded, not twitch-driven).
/// Public so the laser can scale fire spread by how fast a unit is moving (accuracy penalty).
pub const UNIT_SPEED: f32 = 6.0;
/// Encumbrance: each crab clinging to a unit (a `CrabAttached` whose host is that unit) drags its
/// movement down. Effective speed = `UNIT_SPEED / (1 + crabs * CRAB_DRAG)`, floored at `MIN_ENCUMBER`
/// so a heavily-swarmed unit crawls (piranha weight) but is never frozen solid. Diminishing per crab:
/// 3 ≈ 0.7×, 5 ≈ 0.6×, 10 ≈ 0.4×, 20 ≈ 0.25×.
const CRAB_DRAG: f32 = 0.15;
const MIN_ENCUMBER: f32 = 0.15;
/// Squad member hit points (drives the floating health bar; units take no damage yet).
const UNIT_HP: f32 = 100.0;
/// How fast a unit turns to face its travel direction (per-second slerp rate, clamped per frame).
const TURN_SPEED: f32 = 14.0;
const MAX_FRAME_DT: f32 = 1.0 / 30.0;

/// ORCA personal-space disc radius. Slightly larger than the collision half-extent so units keep
/// their AABBs from overlapping, while still small enough for two to pass abreast in a 2-wide
/// corridor (2·0.30 = 0.60 centre spacing fits the 1.6 clear width).
const ORCA_RADIUS: f32 = 0.30;
/// Seconds ahead unit↔unit collisions are anticipated. Larger = earlier, gentler avoidance but more
/// anticipatory braking (which gridlocks dense junctions); kept low so units squeeze through crowds.
const ORCA_TIME_HORIZON: f32 = 1.0;
/// Only units within this centre distance are fed to a unit's ORCA solve (the rest can't interact
/// within the horizon). Bounds the per-frame neighbor work.
const ORCA_QUERY_RADIUS: f32 = 4.0;

/// Arrived once this close to the goal cell centre.
const ARRIVE_RADIUS: f32 = 0.6;
/// Within this distance of the goal, a unit that can no longer make progress is "packed in" and
/// treated as arrived (others fill the exact goal cell), instead of jittering forever.
const PACK_RADIUS: f32 = 2.5;
/// A stuck unit also arrives if an already-settled unit sits within this distance *between it and
/// the goal* — the settled blob then grows outward from the goal so a large crowd all packs in
/// rather than piling up short of a single goal cell.
const BLOB_RADIUS: f32 = 1.3;
/// A unit counts as "progressing" when it gets at least this much closer to the goal; smaller
/// improvements are treated as a stall (avoids float jitter resetting the stall timer forever).
const PROGRESS_EPS: f32 = 0.05;
/// No-progress duration that ends a packed-in order when the unit is near the goal or wedged behind
/// the settled goal blob. There is deliberately no plain time-based give-up: initial spawn
/// congestion always stalls the back of the crowd for seconds, so a timer alone would settle units
/// at the spawn. Settling is gated on *what* blocks a unit — a settled neighbor between it and the
/// goal (permanent) vs. moving neighbors (transient) — see `blocked_by_settled`.
const PACK_STUCK_TIME: f32 = 0.5;

const FIGURINE_GLB: &str = "kenney_prototype-kit/Models/GLB format/figurine.glb";

/// Compact blaster carried in the figurine's hand (CC0, Kenney Blaster Kit 2.1).
const BLASTER_GLB: &str = "kenney_blaster-kit_2.1/Models/GLB format/blaster-a.glb";
const GUN_OFFSET: Vec3 = Vec3::new(0.18, 0.3, -0.2);
const GUN_SCALE: f32 = 0.35;
const GUN_YAW: f32 = 0.0;
/// The gun's barrel tip in figurine-local space — laser bolts spawn here (see `laser`).
pub const MUZZLE_LOCAL: Vec3 = Vec3::new(GUN_OFFSET.x, GUN_OFFSET.y, GUN_OFFSET.z - 0.35);

/// Five distinct outfit colors, one per squad member (index-matched to spawn order).
const OUTFITS: [Color; 5] = [
    Color::srgb(0.85, 0.22, 0.20), // red
    Color::srgb(0.22, 0.45, 0.90), // blue
    Color::srgb(0.25, 0.75, 0.32), // green
    Color::srgb(0.92, 0.76, 0.16), // gold
    Color::srgb(0.66, 0.32, 0.82), // purple
];

/// Spiral of cell offsets from the spawn point; the first five that are floor become unit spawns.
const SPAWN_SPIRAL: [(i32, i32); 13] = [
    (0, 0),
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (2, 0),
    (-2, 0),
    (0, 2),
    (0, -2),
];

pub struct SquadPlugin;

impl Plugin for SquadPlugin {
    fn build(&self, app: &mut App) {
        // `unit_movement` and death are PINNED sim → `FixedUpdate` (fixed dt, frame-rate independent).
        // `command_input` stays on `Update` (it reads mouse/cursor input, which is a per-frame concern);
        // the `MoveOrder` it inserts is simply picked up by the next fixed tick — a sub-frame latency the
        // player can't perceive. `recolor_units` is cosmetic and stays on `Update`.
        app.add_systems(Startup, spawn_squad)
            // `unit_facing` after `unit_movement` so it turns units (moving OR idle) toward their aim/travel
            // once this tick's velocity is settled. Pinned (rotation feeds the smiley's gaze test).
            .add_systems(FixedUpdate, (unit_movement, unit_facing.after(unit_movement), despawn_dead_units))
            .add_systems(Update, recolor_units);
        // NOTE: leader tracking (`ensure_leader` + the `Leader` marker) is deliberately NOT registered
        // here. The `Leader` marker sits on exactly one `Unit`, which would split the hashed squad into
        // two archetypes and make the pinned iteration order (ORCA in `unit_movement`, crab nearest-prey
        // tiebreaks) archetype-dependent — breaking `deterministic_core_is_bit_identical`. It's a
        // windowed-only, dialogue-facing concern, so `DialoguePlugin` owns it (registered in `lib::run`
        // only, never in the headless harness). `SquadMember` stays here: it's on *every* unit, so it
        // keeps them in one archetype and is determinism-neutral. See TESTING.md.
    }
}

fn spawn_squad(mut commands: Commands, dungeon: Res<Dungeon>, assets: Res<AssetServer>) {
    // Pick five distinct floor cells clustered around the dungeon spawn.
    let base = dungeon.spawn;
    let cells: Vec<IVec2> = SPAWN_SPIRAL
        .iter()
        .map(|&(dx, dy)| base + IVec2::new(dx, dy))
        .filter(|&c| dungeon.is_floor(c))
        .take(5)
        .collect();

    // The role + persona roster, index-matched to spawn order (member i plays role i). Loaded from
    // `assets/config/personas.ron` when present (validated), else the code-literal defaults — a
    // malformed/invalid override is a loud startup panic, never a silent default (mirrors roles.ron).
    let personas = load_personas().unwrap_or_else(|e| panic!("personas.ron: {e}"));

    for (i, &cell) in cells.iter().enumerate() {
        let outfit = OUTFITS[i];
        // Per-unit decision seed from the spawn index (deterministic; never from position, mirroring
        // the crab/scout convention in `ai::brain`).
        let seed = (i as u32).wrapping_add(1);
        let mut unit = commands.spawn((
                Unit,
                SquadMember(i),
                Prey, // crabs may swarm/bite units (nearest-prey targeting)
                MoveSpeed(UNIT_SPEED),
                Velocity(Vec2::ZERO),
                AimTarget(None), // set by `laser::fire_laser`; drives facing in `unit_facing`
                Health::new(UNIT_HP),
                Outfit(outfit),
                // Squad-AI kit: the role brain the unit runs, its dialogue persona, drives, the cached
                // decision + think throttle, and the autonomous movement goal (see `squad_ai`).
                (
                    RoleId::ALL[i],
                    personas[i].clone(),
                    Drives::new(),
                    ActiveBehavior::new(seed),
                    ThinkTimer::staggered(seed),
                    DesiredMove::default(),
                    UtterCooldown::default(),
                    MemoryStream::default(),
                ),
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(FIGURINE_GLB))),
                Transform::from_translation(dungeon.cell_center(cell))
                    .with_scale(Vec3::splat(FIGURINE_SCALE)),
                // Render-only: smooth this unit's 60 Hz movement across the display refresh (see `lib::run`).
                // Component + plugin come from avian's `bevy_transform_interpolation` integration.
                avian3d::prelude::TransformInterpolation,
            ));
        // The initial `Leader` marker is assigned windowed-only by `ensure_leader` (see `DialoguePlugin`),
        // not here — putting it on one unit in the headless core would split the hashed archetype.
        // Carried blaster (kept from the shooter feature; fires from selected units, see `laser`).
        unit.with_child((
            GunModel,
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(BLASTER_GLB))),
            Transform::from_translation(GUN_OFFSET)
                .with_scale(Vec3::splat(GUN_SCALE))
                .with_rotation(Quat::from_rotation_y(GUN_YAW)),
        ));
    }
}

/// Keep exactly one living unit tagged [`Leader`]. Runs on the initial frame (no leader yet →
/// promotes [`SquadMember`] 0) and again whenever the leader dies (removed by [`despawn_dead_units`]),
/// promoting the surviving member with the lowest [`SquadMember`] index so leader-anchored UI
/// (dialogue choices) always has a target. Cheap: only acts when the tag is missing.
///
/// Windowed-only — registered by `crate::dialogue::DialoguePlugin`, never the headless harness. The
/// `Leader` marker splits the hashed `Unit` archetype, so it must stay out of the deterministic core
/// (see `SquadPlugin::build`).
pub(crate) fn ensure_leader(
    mut commands: Commands,
    leaders: Query<(), (With<Unit>, With<Leader>)>,
    members: Query<(Entity, &SquadMember), With<Unit>>,
) {
    if !leaders.is_empty() {
        return;
    }
    if let Some((entity, _)) = members.iter().min_by_key(|(_, m)| m.0) {
        commands.entity(entity).insert(Leader);
    }
}

/// Debug safeguard: never let the whole squad wipe. At least [`MIN_LIVING_UNITS`] member always
/// survives — so of five units, four can die but the last cannot. (Remove this floor for real
/// lose conditions.)
const MIN_LIVING_UNITS: usize = 1;

/// Remove squad members whose health has run out (enemies gnaw them down — see `enemy`). Despawning
/// a unit takes its figurine + carried gun with it; its floating health bar is cleaned up as an
/// orphan by `health::update_health_bars`. A small burst at chest height marks the death. The last
/// [`MIN_LIVING_UNITS`] can't be despawned: a protected survivor has its health floored so it lingers
/// (bar shows a sliver) instead of dying.
fn despawn_dead_units(
    mut commands: Commands,
    mut gore: ResMut<GoreQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut units: Query<(Entity, &mut Health, &Transform, &Outfit, &WorldAssetRoot), With<Unit>>,
) {
    // How many dead units we may actually remove this frame while keeping the living floor.
    let mut removable = units.iter().count().saturating_sub(MIN_LIVING_UNITS);
    for (entity, mut hp, transform, outfit, root) in &mut units {
        if hp.current <= 0.0 {
            if removable > 0 {
                // The unit's real 3D figurine gets crunched: blood spray + a floor pool + its own
                // mesh sliced into flying meat chunks tinted to its outfit color (see `gore`/`autogib`).
                gore.0.push(GoreEvent {
                    pos: transform.translation + Vec3::Y * 0.5,
                    kind: GoreKind::UnitCrunch,
                    tint: outfit.0,
                    // The figurine's baked fracture set: spawn from its foot origin at its render scale.
                    gib: Some(GibSource {
                        source: root.0.id(),
                        origin: transform.translation,
                        scale: transform.scale.x,
                    }),
                    // Losing one of your own is a real gut-punch — a solid (but not boss-sized) kick.
                    intensity: 0.6,
                });
                sfx.write(Sfx::UnitDeath(transform.translation));
                commands.entity(entity).despawn();
                removable -= 1;
            } else {
                // Protected survivor — clamp so it can't die and its bar keeps a sliver.
                hp.current = hp.current.max(1.0);
            }
        }
    }
}

/// Once a unit's figurine scene has spawned its mesh descendants, give it a flat outfit-colored
/// material (a new handle per unit so they don't share one asset). The gun subtree is skipped so
/// the blaster keeps its own look. Runs until the async scene load produces meshes, then tags the
/// unit `Recolored` so it never runs again.
fn recolor_units(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    units: Query<(Entity, &Outfit), (With<Unit>, Without<Recolored>)>,
    children: Query<&Children>,
    has_material: Query<(), With<MeshMaterial3d<StandardMaterial>>>,
    is_gun: Query<(), With<GunModel>>,
) {
    for (unit, outfit) in &units {
        let mut stack: Vec<Entity> = match children.get(unit) {
            Ok(c) => c.iter().collect(),
            Err(_) => continue,
        };
        // Mint the outfit material lazily — only once we've actually found a mesh to recolor. Creating
        // it up-front orphaned a fresh `StandardMaterial` on every frame the scene was still streaming
        // (the guard above `continue`s) or had spawned meshes but no material yet, churning one throwaway
        // asset per unit per frame across the whole async-load window. `material.is_some()` also doubles
        // as the "did we recolor anything?" flag that gates the `Recolored` tag.
        let mut material: Option<Handle<StandardMaterial>> = None;
        while let Some(e) = stack.pop() {
            if is_gun.get(e).is_ok() {
                continue; // don't recurse into the gun sub-model
            }
            if has_material.get(e).is_ok() {
                let handle = material.get_or_insert_with(|| {
                    materials.add(StandardMaterial {
                        base_color: outfit.0,
                        perceptual_roughness: 0.7,
                        ..default()
                    })
                });
                commands.entity(e).insert(MeshMaterial3d(handle.clone()));
            }
            if let Ok(ch) = children.get(e) {
                stack.extend(ch.iter());
            }
        }
        if material.is_some() {
            commands.entity(unit).insert(Recolored);
        }
    }
}

/// Advance each unit: preferred velocity → ORCA around the other units → wall collision. The
/// preferred velocity comes from *either* an authoritative player [`MoveOrder`] (flow-field steer,
/// the original path — unchanged) *or*, for an order-less unit, the squad AI's [`DesiredMove`] goal
/// (a straight steer toward the goal; walls are handled by `resolve_move`). A unit with neither holds
/// position. This is the single hook where the autonomous role/cohesion layer feeds the same ORCA
/// pipeline the player commands use (see `squad_ai::perception::squad_think`).
fn unit_movement(
    mut commands: Commands,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    mut units: Query<
        (
            Entity,
            &mut Transform,
            &MoveSpeed,
            &mut Velocity,
            Option<&mut MoveOrder>,
            Option<&mut crate::squad_ai::cohesion::DesiredMove>,
        ),
        With<Unit>,
    >,
    // Crabs clinging to units, for the encumbrance slowdown (a piranha pile bogs a unit down).
    attached: Query<&CrabAttached>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    if dt <= 0.0 {
        return;
    }

    // Count crabs latched onto each unit this frame → an encumbrance multiplier per host.
    let mut crab_load: HashMap<Entity, u32> = HashMap::new();
    for a in &attached {
        if let Some(host) = a.host {
            *crab_load.entry(host).or_default() += 1;
        }
    }

    // Snapshot every unit as an ORCA agent using last frame's velocity (synchronous update: all
    // solves see the same prior state). A unit that is moving — under a player order OR an *active* AI
    // goal — `avoids` (it reciprocates); a truly idle unit does not, so movers take full responsibility
    // going around it AND it reads as a "settled" neighbor for the `blocked_by_settled` arrival blob.
    // This hinges on `squad_think` giving an at-rest unit a `None` goal: the FollowAnchor deadband
    // (see `squad_ai::perception`) makes an idle unit near the anchor hold with `goal == None`, so it
    // is correctly `avoids: false` here. Without that deadband every idle unit carried a standing
    // FollowAnchor goal, was permanently `avoids: true`, and the arrival shortcut could never fire.
    let agents: Vec<(Entity, Agent)> = units
        .iter()
        .map(|(e, t, _, v, order, desired)| {
            let moving = order.is_some() || desired.is_some_and(|d| d.goal.is_some());
            (
                e,
                Agent {
                    pos: t.translation.xz(),
                    vel: v.0,
                    radius: ORCA_RADIUS,
                    avoids: moving,
                },
            )
        })
        .collect();

    for (entity, mut transform, speed, mut velocity, mut order, mut desired) in &mut units {
        let pos = transform.translation;
        let self_pos = pos.xz();

        // Encumbrance: crabs clinging to this unit drag its top speed down (never to a dead stop).
        let crabs = crab_load.get(&entity).copied().unwrap_or(0);
        let max_speed = speed.0 * (1.0 / (1.0 + crabs as f32 * CRAB_DRAG)).max(MIN_ENCUMBER);

        // Preferred velocity + goal from the authoritative source. Player order first (unchanged flow-
        // field steer); else the AI goal (straight steer); else hold.
        let (pref, goal_xz) = if let Some(order) = &order {
            // Flow-field look-ahead on the cell centerline (keeps the unit centered in corridors).
            let g = dungeon.cell_center(order.field.goal()).xz();
            (order.field.steer(&dungeon, pos) * max_speed, g)
        } else if let Some(goal) = desired.as_ref().and_then(|d| d.goal) {
            let g = goal.xz();
            ((g - self_pos).normalize_or_zero() * max_speed, g)
        } else {
            velocity.0 = Vec2::ZERO; // idle → at rest (still advertised to ORCA next frame)
            continue;
        };
        let goal_dist = (goal_xz - self_pos).length();

        // ORCA neighbors, plus: is a *settled* unit (no order) sitting just ahead of me toward the
        // goal? If so and I can't progress, I've reached the back of the arrived blob and pack in.
        // Direction-based (settled unit within the goalward cone) so it propagates cleanly back from
        // the goal even across a room, and never fires at spawn where all neighbors still have orders.
        let to_goal = (goal_xz - self_pos).normalize_or_zero();
        let mut neighbors: Vec<Agent> = Vec::new();
        let mut blocked_by_settled = false;
        for (other, ag) in &agents {
            if *other == entity {
                continue;
            }
            let off = ag.pos - self_pos;
            if off.length_squared() <= ORCA_QUERY_RADIUS * ORCA_QUERY_RADIUS {
                neighbors.push(*ag);
            }
            if !ag.avoids
                && off.length_squared() <= BLOB_RADIUS * BLOB_RADIUS
                && off.normalize_or_zero().dot(to_goal) > 0.2
            {
                blocked_by_settled = true;
            }
        }
        // Canonicalize neighbour order so ORCA is iteration-order-independent. `new_velocity` pushes one
        // half-plane per neighbour and solves an INCREMENTAL 2D linear program (`orca::new_velocity` →
        // `linear_program2/3`), whose float output depends on constraint ORDER. ECS query iteration
        // order is not guaranteed stable across runs (archetype membership shifts as components are
        // added/removed), so sorting neighbours by position — the value-sort determinism idiom of
        // `snapshot_hash`/`update_anchor` — makes the solve a pure function of the neighbour SET, a
        // cheap reproducibility guard on this ≤handful-of-neighbours hot path. `blocked_by_settled`
        // above is an order-independent OR, so it needs no sort.
        neighbors.sort_unstable_by_key(|a| (a.pos.x.to_bits(), a.pos.y.to_bits()));

        // Nearby solid cells become hard ORCA wall constraints, so a unit dodging a neighbor is never
        // steered into a wall (where it would stall). Only walls the unit is actually *close* to bind
        // (gap < WALL_GATE); the allowed approach speed is the remaining gap ÷ dt, shrinking to zero at
        // contact. WALK_HALF is the walkable half-width of a walled cell (centre to wall face).
        const WALK_HALF: f32 = 0.5 * crate::dungeon::TILE_SIZE - crate::dungeon::WALL_THICKNESS;
        const WALL_GATE: f32 = 0.4;
        let cell = dungeon.world_to_cell(pos);
        let local = Vec2::new(pos.x - cell.x as f32, pos.z - cell.y as f32);
        let mut walls: Vec<(Vec2, f32)> = Vec::new();
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if dungeon.is_floor(cell + IVec2::new(dx, dy)) {
                continue;
            }
            let b = Vec2::new(dx as f32, dy as f32);
            let gap = WALK_HALF - (local.dot(b) + UNIT_HALF_EXTENTS.x);
            if gap < WALL_GATE {
                walls.push((b, (gap.max(0.0)) / dt));
            }
        }

        let me = Agent {
            pos: self_pos,
            vel: velocity.0,
            radius: ORCA_RADIUS,
            avoids: true,
        };
        let new_vel =
            orca::new_velocity(&me, pref, &neighbors, &walls, ORCA_TIME_HORIZON, dt, max_speed);
        velocity.0 = new_vel;

        // Integrate the ORCA velocity against walls (unit↔wall is the resolver's job, not ORCA's).
        let delta = Vec3::new(new_vel.x, 0.0, new_vel.y) * dt;
        transform.translation = dungeon.resolve_move(pos, delta, UNIT_HALF_EXTENTS);
        let new_goal_dist = (goal_xz - transform.translation.xz()).length();

        if let Some(order) = order.as_mut() {
            // --- Player-order arrival (unchanged): progress-based stall + packed-in blob. ---
            // The timer only resets when the unit gets genuinely closer to the goal, so a unit shoved
            // in circles at non-zero speed still eventually counts as stalled.
            if new_goal_dist < order.best_dist - PROGRESS_EPS {
                order.best_dist = new_goal_dist;
                order.no_progress_time = 0.0;
            } else {
                order.no_progress_time += dt;
            }
            // Arrival: reached the goal, or packed in — stalled *and* either right at the goal or
            // wedged behind the settled blob. Because settled units exist only at the goal (no mid-
            // route give-up), `blocked_by_settled` can only become true once a unit reaches the back
            // of that blob, so the blob grows outward from the goal and never nucleates a stall mid-hall.
            let packed = order.no_progress_time >= PACK_STUCK_TIME
                && (goal_dist < PACK_RADIUS || blocked_by_settled);
            if goal_dist < ARRIVE_RADIUS || packed {
                commands.entity(entity).remove::<MoveOrder>();
                velocity.0 = Vec2::ZERO;
            }
        } else if new_goal_dist < ARRIVE_RADIUS {
            // --- AI-goal arrival: reached the cohesion/role goal → clear it and hold (no blob/stall
            // logic; autonomous goals are short-range and re-planned each think). ---
            if let Some(d) = desired.as_mut() {
                d.goal = None;
            }
            velocity.0 = Vec2::ZERO;
        }

        // Facing is handled centrally by `unit_facing` (below) for ALL units — moving OR idle — so a
        // stationary unit still turns to look at what it is shooting.
    }
}

/// Turn each unit to face what it is shooting (its `AimTarget`, set by `laser::fire_laser`), or its
/// travel direction when not engaging — slerped for a smooth turn. Runs for EVERY unit (unlike the old
/// facing in `unit_movement`, which only turned commanded/moving units), so a stationary unit visibly
/// pivots to aim. This is why the smiley watcher's "is a unit looking at it" gaze test (which reads body
/// facing) matches what the player sees: body facing == aim (Rabin, "Vision Zones", GameAIPro2 Ch.4).
fn unit_facing(time: Res<Time>, mut units: Query<(&mut Transform, &Velocity, &AimTarget), With<Unit>>) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    if dt <= 0.0 {
        return;
    }
    for (mut transform, velocity, aim) in &mut units {
        // Prefer the fire target (flattened to the unit's own height so it yaws, never pitches); else the
        // travel direction. `None` on both ⇒ hold the current facing.
        let target = aim
            .0
            .map(|t| Vec3::new(t.x, transform.translation.y, t.z))
            .or_else(|| {
                let v = Vec3::new(velocity.0.x, 0.0, velocity.0.y);
                (v.length_squared() > 1.0e-6).then_some(transform.translation + v)
            });
        if let Some(target) = target
            && (target - transform.translation).length_squared() > 1.0e-6
        {
            let facing = Transform::from_translation(transform.translation)
                .looking_at(target, Vec3::Y)
                .rotation;
            transform.rotation = transform.rotation.slerp(facing, (TURN_SPEED * dt).min(1.0));
        }
    }
}
