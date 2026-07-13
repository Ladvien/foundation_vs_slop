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
use crate::sim::SimTuning;
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

/// An explicit world point this unit's body should turn to face, overriding both `AimTarget` and the
/// travel-direction fallback in `unit_facing`. Used by the Researcher's flashlight: when its AI enters
/// `Mode::Ward` it aims the beam at the light-averse threat it is herding (`squad_ai::perception`).
/// `None` on every other unit and on the Researcher when not warding — then facing falls back to aim,
/// then to travel direction. Present on EVERY unit (spawned `None`) so it never splits the hashed squad
/// archetype, exactly like `AimTarget`.
#[derive(Component, Default)]
pub struct FacingOverride(pub Option<Vec3>);

/// Marks the gun sub-model so the outfit recolor skips it (the blaster keeps its own colors) and so
/// `autogib` can bake it as a separate intact chunk instead of folding it into the body fracture. The
/// Researcher's flashlight carries this marker too — it is the unit's held item, so it inherits the same
/// recolor-skip and death-fling behavior for free (see the spawn branch below).
#[derive(Component)]
pub struct GunModel;

/// Marks the Researcher's flashlight sub-model (a sibling of [`GunModel`]'s role) so the windowed-only
/// cosmetic `SpotLight` system (`light::attach_flashlight_spots`) can find it and give it a real beam.
/// Gameplay light goes through the `LightField` cone instead (see `light::apply_dynamic_lights`); this
/// marker is purely for the rendered glow.
#[derive(Component)]
pub struct FlashlightModel;

/// Marks the figurine child entity that carries the unit's async body scene (`WorldAssetRoot`). The
/// figurine lives on a *child*, not the `Unit` sim entity, so the scene spawner's async
/// `Children`/scene-instance insertion (and the `Recolored` tag) churns *this* cosmetic entity's
/// archetype at a wall-clock-dependent tick — never the `Unit`'s. Keeping the `Unit` archetype fixed
/// at spawn is what lets the deterministic replay gate run the squad AI (see issue #18 / `sim_harness`).
#[derive(Component)]
pub struct FigurineModel;

/// The unit's figurine scene asset, carried on the `Unit` itself as a stable, spawn-time id so death
/// (`despawn_dead_units`) and fracture baking (`autogib::bake_autogib`) can key the gib source without
/// reading the async `WorldAssetRoot` (which now lives on the [`FigurineModel`] child). One handle is
/// loaded once and cloned into both the child's `WorldAssetRoot` and this component — one asset, one path.
#[derive(Component)]
pub struct FigurineSource(pub Handle<WorldAsset>);

/// Marks a [`FigurineModel`] child whose meshes have already been recolored (so the one-shot recolor
/// runs once). Tagged on the figurine child, never the `Unit`, so recoloring never churns the sim archetype.
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
const MAX_FRAME_DT: f32 = 1.0 / 30.0;

// Unit locomotion / ORCA / pack-cohesion knobs — UNIT_SPEED, MIN_ENCUMBER, TURN_SPEED, ORCA_RADIUS,
// ORCA_TIME_HORIZON, ORCA_QUERY_RADIUS, ARRIVE_RADIUS, PACK_RADIUS, BLOB_RADIUS, PROGRESS_EPS,
// PACK_STUCK_TIME — now live in the `behavior:` config slice (`BehaviorTuning::squad_move`), read as
// `Res<BehaviorTuning>`. See src/behavior_tuning.rs. (The laser scales fire spread by unit speed via the
// same slice's `squad_move.unit_speed`.)

const FIGURINE_GLB: &str = "kenney_prototype-kit/Models/GLB format/figurine.glb";

/// Compact blaster carried in the figurine's hand (CC0, Kenney Blaster Kit 2.1).
const BLASTER_GLB: &str = "kenney_blaster-kit_2.1/Models/GLB format/blaster-a.glb";
const GUN_OFFSET: Vec3 = Vec3::new(0.18, 0.3, -0.2);
const GUN_SCALE: f32 = 0.35;
const GUN_YAW: f32 = 0.0;
/// The gun's barrel tip in figurine-local space — laser bolts spawn here (see `laser`).
pub const MUZZLE_LOCAL: Vec3 = Vec3::new(GUN_OFFSET.x, GUN_OFFSET.y, GUN_OFFSET.z - 0.35);

/// The Researcher carries this handheld flashlight instead of the blaster (CC0, authored via BlenderMCP;
/// see `/mnt/codex_fs/game_assets/low_poly_flashlight`). The lens faces the model's local +Y after the
/// Y-up glTF export, and the model stands ~2.5 units tall on its tail cap, so we scale it down and pitch
/// it so the beam points forward out of the figurine's hand. These transform constants are cosmetic only
/// — the gameplay light cone points along the unit's facing, not the model (see `light`).
const FLASHLIGHT_GLB: &str = "low_poly_flashlight/low_poly_flashlight.glb";
const FLASHLIGHT_OFFSET: Vec3 = Vec3::new(0.18, 0.3, -0.2);
const FLASHLIGHT_SCALE: f32 = 0.12;
/// Pitch that tips the model's local +Y (lens up) forward to the unit's −Z; tuned by screenshot.
const FLASHLIGHT_PITCH: f32 = -std::f32::consts::FRAC_PI_2;

/// Five distinct outfit colors, one per squad member (index-matched to spawn order = `RoleId::ALL`).
/// See [`crate::palette`].
const OUTFITS: [Color; 5] = crate::palette::OUTFITS;

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
            // `unit_movement` CONSUMES the `DesiredMove` goal that `squad_ai::squad_think` produces in
            // `AiSet::Think`, so that edge is pinned explicitly rather than left to registration order.
            // Both are on `FixedUpdate`, so without the constraint Bevy is free to run them in either
            // order — an ambiguity that would silently cost a tick of latency (or shift the replay hash)
            // in a codebase that value-sorts ORCA neighbours to keep the sim reproducible.
            //
            // `unit_facing` after `unit_movement` so it turns units (moving OR idle) toward their aim/travel
            // once this tick's velocity is settled. Pinned (rotation feeds the smiley's gaze test).
            .add_systems(
                FixedUpdate,
                (
                    unit_movement.after(crate::ai::AiSet::Think),
                    unit_facing.after(unit_movement),
                    despawn_dead_units,
                ),
            )
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

fn spawn_squad(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    assets: Res<AssetServer>,
    sim: Res<SimTuning>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
) {
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
        // The figurine body scene: loaded once, then referenced from two places — a stable
        // `FigurineSource` on the `Unit` (spawn-time id for gib/death code) and the async
        // `WorldAssetRoot` on the cosmetic `FigurineModel` child below. Attaching the scene to a child
        // (not the `Unit`) keeps the sim entity's archetype fixed across the async load, so ECS
        // iteration order is stable between same-seed runs — the fix that lets the squad AI into the
        // deterministic replay gate (issue #18). Crabs already do this (`crate::crab`, `with_child`).
        let figurine: Handle<WorldAsset> =
            assets.load(GltfAssetLabel::Scene(0).from_asset(FIGURINE_GLB));
        let mut unit = commands.spawn((
                Unit,
                SquadMember(i),
                Prey, // crabs may swarm/bite units (nearest-prey targeting)
                MoveSpeed(beh.squad_move.unit_speed),
                Velocity(Vec2::ZERO),
                AimTarget(None), // set by `laser::fire_laser`; drives facing in `unit_facing`
                FacingOverride(None), // set by `squad_think` when the Researcher wards; wins over aim

                Health::new(sim.combat.unit_hp),
                Outfit(outfit),
                // Squad-AI kit: the role brain the unit runs, its dialogue persona, drives, the cached
                // decision + think throttle, and the autonomous movement goal (see `squad_ai`).
                (
                    RoleId::ALL[i],
                    personas[i].clone(),
                    Drives::new(),
                    // Units fear the creatures (THREAT_CRAB / THREAT_ANOMALY), never their own gunfire.
                    crate::ai::faction::Faction::Foundation,
                    ActiveBehavior::new(seed),
                    ThinkTimer::staggered(seed),
                    DesiredMove::default(),
                    // On EVERY unit, never a subset: a component present on only some units would split
                    // the hashed squad archetype and make iteration order run-dependent (see the `Leader`
                    // note below).
                    crate::squad_ai::perception::PerceptionLatch::default(),
                    UtterCooldown::default(),
                    MemoryStream::default(),
                    crate::squad_ai::dialogue::SpokenLines::default(),
                ),
                FigurineSource(figurine.clone()),
                // The `Unit` carries no mesh of its own (the figurine is a cosmetic child), so nothing
                // auto-inserted `Visibility` here. Two things needed it and quietly went without:
                // the cosmetic children logged `B0004: parent without InheritedVisibility` every frame,
                // and `dialogue::bubble::track_bubbles` — whose owner query is `(&Transform, &Visibility)`
                // — never matched a unit, so it despawned every speech bubble on the frame it spawned.
                // Squad dialogue could not render at all. Uniform across all five units, so it does not
                // split the hashed archetype, and `snapshot_hash` reads only Transform + Health.
                Visibility::default(),
                Transform::from_translation(dungeon.cell_center(cell))
                    .with_scale(Vec3::splat(FIGURINE_SCALE)),
                // Render-only: smooth this unit's 60 Hz movement across the display refresh (see `lib::run`).
                // Component + plugin come from avian's `bevy_transform_interpolation` integration.
                avian3d::prelude::TransformInterpolation,
            ));
        // SCP-150 host state: every unit is a parasitizable host with an (initially inert) infestation
        // slot. Always-present (never a runtime insert/remove) so it can't split the hashed squad
        // archetype — the same invariant the `PerceptionLatch` note above relies on.
        unit.insert(crate::parasite::host_infestation_bundle());
        // The initial `Leader` marker is assigned windowed-only by `ensure_leader` (see `DialoguePlugin`),
        // not here — putting it on one unit in the headless core would split the hashed archetype.
        // The figurine body scene, on a cosmetic child with an identity transform: it inherits the
        // unit's position + `FIGURINE_SCALE`, so it renders exactly where the old root-attached scene
        // did, but the async `Children`/scene-instance churn lands on this child, not the `Unit`.
        unit.with_child((
            FigurineModel,
            WorldAssetRoot(figurine),
            Transform::default(),
        ));
        // Held item. The Researcher (the "Scientist" archetype) trades the blaster for a flashlight and
        // becomes a light-based crowd-control unit — its beam repels photophobic creatures rather than
        // dealing damage (see `laser::fire_laser`, which skips it, and `light::apply_dynamic_lights`).
        // Ref: Björk & Michelsen, FDG 2014 — the flashlight as a vision-limiting, non-lethal deterrent.
        // Branch on the role *value* (every unit already carries `RoleId`), never a marker component, so
        // the hashed squad archetype stays uniform across the five members. Both items keep the `GunModel`
        // marker: it is "the held item" for the recolor-skip and the autogib death-fling, which apply to
        // the flashlight identically.
        if RoleId::ALL[i] == RoleId::Researcher {
            unit.with_child((
                GunModel,
                FlashlightModel,
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(FLASHLIGHT_GLB))),
                Transform::from_translation(FLASHLIGHT_OFFSET)
                    .with_scale(Vec3::splat(FLASHLIGHT_SCALE))
                    .with_rotation(Quat::from_rotation_x(FLASHLIGHT_PITCH)),
            ));
        } else {
            unit.with_child((
                GunModel,
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(BLASTER_GLB))),
                Transform::from_translation(GUN_OFFSET)
                    .with_scale(Vec3::splat(GUN_SCALE))
                    .with_rotation(Quat::from_rotation_y(GUN_YAW)),
            ));
        }
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

/// Remove squad members whose health has run out (enemies gnaw them down — see `enemy`). Despawning
/// a unit takes its figurine + carried gun with it; its floating health bar is cleaned up as an
/// orphan by `health::update_health_bars`. A small burst at chest height marks the death.
///
/// Every unit can die, including the last: a total wipe is a real outcome. `cohesion::update_anchor`
/// clears `SquadAnchor::valid` on an empty squad and `pick_leader` no-ops, so the zero-unit world is
/// well-defined rather than a state the sim is protected from. This matters beyond lose conditions:
/// the offline behaviour search (`squad_ai::qd`) scores `survivors` and gates on "the squad was not
/// wiped", and a floor that silently resurrects the last member would make both signals a lie.
fn despawn_dead_units(
    mut commands: Commands,
    mut gore: ResMut<GoreQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    audio: Res<crate::audio_tuning::AudioTuning>,
    units: Query<(Entity, &Health, &Transform, &Outfit, &FigurineSource), With<Unit>>,
) {
    // Death-din (`NOISE_SQUAD`) deposits are collected here and sorted before queueing: the query order
    // over dead units is not stable across App instances (async GLB load + entity-id reuse), so an
    // unsorted batch would smear the din channel order-dependently (see `field::sort_deposits`). Every
    // sibling deposit site already sorts (e.g. `crab_despawn_dead` by `Seed`); this one did not.
    let mut noise: Vec<crate::ai::field::Deposit> = Vec::new();
    for (entity, hp, transform, outfit, figurine) in &units {
        if hp.current > 0.0 {
            continue;
        }
        // The unit's real 3D figurine gets crunched: blood spray + a floor pool + its own
        // mesh sliced into flying meat chunks tinted to its outfit color (see `gore`/`autogib`).
        gore.0.push(GoreEvent {
            pos: transform.translation + Vec3::Y * 0.5,
            kind: GoreKind::UnitCrunch,
            tint: outfit.0,
            // The figurine's baked fracture set: spawn from its foot origin at its render scale.
            gib: Some(GibSource {
                source: figurine.0.id(),
                origin: transform.translation,
                scale: transform.scale.x,
            }),
            // Losing one of your own is a real gut-punch — a solid (but not boss-sized) kick.
            intensity: 0.6,
        });
        sfx.write(Sfx::UnitDeath(transform.translation));
        // A unit's death is the loudest squad acoustic event: its din (`NOISE_SQUAD`) marks where the
        // fight turned costly, so the swarm keeps reading the spot even after the guns fall silent.
        noise.push(crate::ai::field::Deposit {
            pos: transform.translation,
            field: crate::ai::field::FieldId::NOISE_SQUAD,
            amount: audio.stimulus.unit_death_loudness,
        });
        commands.entity(entity).despawn();
    }
    crate::ai::field::sort_deposits(&mut noise);
    deposits.0.extend(noise);
}

/// Once a unit's figurine scene has spawned its mesh descendants, give it a flat outfit-colored
/// material (a new handle per unit so they don't share one asset). Runs until the async scene load
/// produces meshes, then tags the `FigurineModel` child `Recolored` so it never runs again.
///
/// Keyed on the figurine *child* (not the `Unit`) so the one-shot `Recolored` tag churns the cosmetic
/// child's archetype, never the sim entity's — the async-load isolation that lets the squad AI into
/// the deterministic replay gate (issue #18). The gun is a *sibling* child of the `Unit`, outside the
/// figurine subtree walked here, so it keeps its own colors without an explicit skip.
fn recolor_units(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    figurines: Query<(Entity, &ChildOf), (With<FigurineModel>, Without<Recolored>)>,
    outfits: Query<&Outfit, With<Unit>>,
    children: Query<&Children>,
    has_material: Query<(), With<MeshMaterial3d<StandardMaterial>>>,
) {
    for (figurine, child_of) in &figurines {
        let Ok(outfit) = outfits.get(child_of.parent()) else {
            continue; // parent isn't a unit (or despawned) — nothing to color
        };
        let mut stack: Vec<Entity> = match children.get(figurine) {
            Ok(c) => c.iter().collect(),
            Err(_) => continue, // scene not instantiated yet — retry next frame
        };
        // Mint the outfit material lazily — only once we've actually found a mesh to recolor. Creating
        // it up-front orphaned a fresh `StandardMaterial` on every frame the scene was still streaming
        // (the guard above `continue`s) or had spawned meshes but no material yet, churning one throwaway
        // asset per unit per frame across the whole async-load window. `material.is_some()` also doubles
        // as the "did we recolor anything?" flag that gates the `Recolored` tag.
        let mut material: Option<Handle<StandardMaterial>> = None;
        while let Some(e) = stack.pop() {
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
            commands.entity(figurine).insert(Recolored);
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
            // Read-only on purpose: `squad_ai::squad_think` is the single owner of `DesiredMove.goal`.
            // Taking `&mut` here once tempted this system into clearing the goal on arrival — a write
            // nothing could observe, since `squad_think` re-resolves the goal every tick before this
            // system runs. `&` makes a second writer a compile error rather than a comment.
            Option<&crate::squad_ai::cohesion::DesiredMove>,
        ),
        With<Unit>,
    >,
    // Crabs clinging to units, for the encumbrance slowdown (a piranha pile bogs a unit down).
    attached: Query<&CrabAttached>,
    sim: Res<SimTuning>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
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
                    radius: beh.squad_move.orca_radius,
                    avoids: moving,
                },
            )
        })
        .collect();

    for (entity, mut transform, speed, mut velocity, mut order, desired) in &mut units {
        let pos = transform.translation;
        let self_pos = pos.xz();

        // Encumbrance: crabs clinging to this unit drag its top speed down (never to a dead stop).
        let crabs = crab_load.get(&entity).copied().unwrap_or(0);
        let max_speed =
            speed.0 * (1.0 / (1.0 + crabs as f32 * sim.combat.crab_drag)).max(beh.squad_move.min_encumber);

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
            if off.length_squared() <= beh.squad_move.orca_query_radius * beh.squad_move.orca_query_radius {
                neighbors.push(*ag);
            }
            if !ag.avoids
                && off.length_squared() <= beh.squad_move.blob_radius * beh.squad_move.blob_radius
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
            radius: beh.squad_move.orca_radius,
            avoids: true,
        };
        let new_vel =
            orca::new_velocity(&me, pref, &neighbors, &walls, beh.squad_move.orca_time_horizon, dt, max_speed);
        velocity.0 = new_vel;

        // Integrate the ORCA velocity against walls (unit↔wall is the resolver's job, not ORCA's).
        let delta = Vec3::new(new_vel.x, 0.0, new_vel.y) * dt;
        transform.translation = dungeon.resolve_move(pos, delta, UNIT_HALF_EXTENTS);
        let new_goal_dist = (goal_xz - transform.translation.xz()).length();

        if let Some(order) = order.as_mut() {
            // --- Player-order arrival (unchanged): progress-based stall + packed-in blob. ---
            // The timer only resets when the unit gets genuinely closer to the goal, so a unit shoved
            // in circles at non-zero speed still eventually counts as stalled.
            if new_goal_dist < order.best_dist - beh.squad_move.progress_eps {
                order.best_dist = new_goal_dist;
                order.no_progress_time = 0.0;
            } else {
                order.no_progress_time += dt;
            }
            // Arrival: reached the goal, or packed in — stalled *and* either right at the goal or
            // wedged behind the settled blob. Because settled units exist only at the goal (no mid-
            // route give-up), `blocked_by_settled` can only become true once a unit reaches the back
            // of that blob, so the blob grows outward from the goal and never nucleates a stall mid-hall.
            let packed = order.no_progress_time >= beh.squad_move.pack_stuck_time
                && (goal_dist < beh.squad_move.pack_radius || blocked_by_settled);
            if goal_dist < beh.squad_move.arrive_radius || packed {
                commands.entity(entity).remove::<MoveOrder>();
                velocity.0 = Vec2::ZERO;
            }
        } else if new_goal_dist < beh.squad_move.arrive_radius {
            // --- AI-goal arrival: reached the cohesion/role goal → come to rest for this tick. ---
            //
            // We do NOT clear `desired.goal` here. `squad_think` re-resolves it from scratch every tick
            // (it runs earlier in `FixedUpdate`, see `SquadPlugin`), so it is the single owner; a write
            // here would be overwritten before anything could read it — including the `agents` snapshot
            // above, which is built at the top of this system, i.e. *after* this tick's `squad_think`.
            // What actually lets a unit settle is the Regroup/FollowAnchor deadband in `resolve_goal`,
            // which yields `None` near the anchor (see the `agents` comment above).
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
pub(crate) fn unit_facing(
    time: Res<Time>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
    mut units: Query<(&mut Transform, &Velocity, &AimTarget, &FacingOverride), With<Unit>>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    if dt <= 0.0 {
        return;
    }
    for (mut transform, velocity, aim, facing_override) in &mut units {
        // Precedence: an explicit `FacingOverride` (the Researcher aiming its warding beam) wins; else the
        // fire target (flattened to the unit's own height so it yaws, never pitches); else the travel
        // direction. `None` on all three ⇒ hold the current facing.
        let target = facing_override
            .0
            .or(aim.0)
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
            transform.rotation = transform.rotation.slerp(facing, (beh.squad_move.turn_speed * dt).min(1.0));
        }
    }
}
