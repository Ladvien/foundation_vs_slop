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

use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::prelude::*;

use crate::anim;
use crate::audio::Sfx;
use crate::crab::CrabAttached;
use crate::dungeon::Dungeon;
use crate::flowfield::FlowField;
use crate::gore::{GibSource, GoreEvent, GoreKind, GoreQueue};
use crate::health::{Biological, Health};
use crate::orca::{self, Agent};
use crate::sim::SimTuning;
use crate::ai::brain::{ActiveBehavior, ThinkTimer};
use crate::ai::drives::Drives;
use crate::squad_ai::actions::UtterCooldown;
use crate::squad_ai::cohesion::DesiredMove;
use crate::squad_ai::dialogue::MemoryStream;
use crate::squad_ai::persona::load_personas;
use crate::squad_ai::role::RoleId;

/// The squad itself — a bodiless organisational entity that owns its operatives through the
/// [`MemberOf`] relationship.
///
/// Deliberately carries **no `Transform` and no `Health`**: it is a roster node, not an actor, so it is
/// invisible to `sim_harness::snapshot_hash` (which folds exactly `(Transform, Health)`) and to
/// `liveness_violations`' actor count. The squad's *spatial* model stays where it already lives — the
/// virtual `squad_ai::cohesion::SquadAnchor` centroid — because that is a smoothed position, not a
/// member list, and the two answer different questions.
#[derive(Component)]
pub struct Squad;

/// "This operative serves in that squad." Carried by **every** `Unit`, inserted at spawn and never
/// toggled, so the hashed squad stays in one archetype (the same rule the module docs state for
/// `SquadMember` versus the windowed-only `Leader` marker).
#[derive(Component)]
#[relationship(relationship_target = SquadRoster)]
pub struct MemberOf(pub Entity);

/// Every living operative in this squad. Maintained by Bevy: a member that despawns is removed from
/// this collection by the relationship's own hooks, which is the despawn hygiene D-2 asks for — no
/// bookkeeping system, and no stale `Entity` to dereference after a death.
///
/// **This component is REMOVED when the last member dies**, not left behind empty — that is Bevy's
/// representation of an empty relationship target. So read it as `Option<&SquadRoster>`; a bare
/// `Query<&SquadRoster>` silently matches nothing on a wiped squad, which reads as "no squad" rather
/// than "a squad with no survivors". `tests/squad.rs` pins the behaviour.
///
/// **Iteration order is spawn order, not a total order.** It is fine to *count* or *test membership*
/// here, but anything that picks, budgets, or draws from a shared RNG while walking this collection
/// must first impose a stable key — `SquadMember` is the one every other site uses (`laser::fire_laser`,
/// `sim_harness::issue_squad_order`). See `tests/determinism_lint.rs`.
#[derive(Component)]
#[relationship_target(relationship = MemberOf)]
pub struct SquadRoster(Vec<Entity>);

impl SquadRoster {
    /// How many operatives are still alive on this roster.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the squad has been wiped.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The members, in **spawn order**. Read the type docs before letting this order decide anything.
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }
}

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

/// Scale VALKYRIE to a ~6 ft squad member: her rig exports at 1.61 m native, so 1.61 × 1.13 ≈ 1.82 m —
/// about three-quarters of the 2.4 m (~8 ft) ceiling, a believable human proportion. Deliberately the
/// same ~1.82 m target the old greybox used, so the floating health bar (`health::BAR_Y`) and every other
/// eyeballed offset stay calibrated. Uniform, so the slung rifle and the autogib fragments stay
/// proportional. Collision (`UNIT_HALF_EXTENTS`) stays narrower than the visual on purpose — see below.
const FIGURINE_SCALE: f32 = 1.13;
/// Square collision half-extent. Sized well under the narrowest walkable channel so units don't
/// wedge/catch in 1-tile doorways: a doorway walled on both sides has `TILE - 2·WALL_THICKNESS = 0.72`
/// of clear width, and a 0.44-wide unit leaves ~0.14 m of slack per side to slide through cleanly. Well
/// under the figurine's visual radius on purpose — reaching the goal reliably beats pixel-exact
/// contact, and the visual is far wider anyway.
const UNIT_HALF_EXTENTS: Vec2 = Vec2::splat(0.22);
const MAX_FRAME_DT: f32 = 1.0 / 30.0;

// Unit locomotion / ORCA / pack-cohesion knobs — UNIT_SPEED, MIN_ENCUMBER, TURN_SPEED, ORCA_RADIUS,
// ORCA_TIME_HORIZON, ORCA_QUERY_RADIUS, ARRIVE_RADIUS, PACK_RADIUS, BLOB_RADIUS, PROGRESS_EPS,
// PACK_STUCK_TIME — now live in the `behavior:` config slice (`BehaviorTuning::squad_move`), read as
// `Res<BehaviorTuning>`. See src/behavior_tuning.rs. (The laser scales fire spread by unit speed via the
// same slice's `squad_move.unit_speed`.)

/// VALKYRIE — SCP MTF assault operative: a single-skin rig (62 joints, root `Root`), 20 looping 24 fps
/// clips, with an integrated rifle, thigh holster and ammo pouch skinned into THIS scene (not separate
/// held models). Replaces the old un-rigged Kenney greybox.
///
/// Ten clips are wired: idle, idle_alert and the six-way gait blend space (walk / run / walk_back /
/// run_back / both strafes) on the body, plus aim and fire masked to the upper body — see the
/// locomotion section below. `reload`, `crouch_walk`, `jump_fwd`, `jump_back`, `aim_walk`,
/// `walk_start`/`walk_stop` and their backward twins, and `death` are authored but deliberately NOT
/// wired: no mechanic drives them, and a wired clip nothing can trigger is a stub.
/// See `/mnt/codex_fs/game_assets/SCP_Characters/gltf/valkyrie_bevy_integration.md`.
const FIGURINE_GLB: &str = "characters/valkyrie.glb";

/// Laser bolts spawn from this fixed offset in the unit's **rotated, unscaled** local frame:
/// `laser::fire_laser` computes `unit.translation + unit.rotation * MUZZLE_OFFSET`. Deliberately
/// **decoupled from the cosmetic `FIGURINE_SCALE`** — the muzzle's world position feeds the hashed sim
/// (combat targeting: front-arc gate + nearest-enemy rank, and bolt spawn), so it must NOT move when the
/// art is rescaled. Tying it to the model scale is what let the VALKYRIE swap silently shift targeting and
/// wipe the squad on a borderline world (`search_calibration::the_authored_brains_produce_a_real_encounter_on_every_world`).
/// Kept a `const` (never read from the async-loaded rifle bone) for the same determinism reason.
///
/// The value reproduces the SHIPPED muzzle world offset **exactly**: the old greybox fired from
/// `transform_point((0.18, 0.3, -0.55))` at `FIGURINE_SCALE` 2.6 — and `transform_point` applies scale
/// (component-wise) then rotation then translation, so that is `rotation * (2.6 * (0.18, 0.3, -0.55))`.
/// `f32` multiply is commutative, so folding the 2.6 into the constant is bit-identical — the deterministic
/// core stays frozen (golden hash unchanged) across the mesh swap.
pub const MUZZLE_OFFSET: Vec3 = Vec3::new(0.18 * 2.6, 0.3 * 2.6, -0.55 * 2.6);

/// The Researcher carries this handheld flashlight instead of the blaster (CC0, authored via BlenderMCP;
/// see `/mnt/codex_fs/game_assets/low_poly_flashlight`). The lens faces the model's local +Y after the
/// Y-up glTF export, and the model stands ~2.5 units tall on its tail cap, so we scale it down and pitch
/// it so the beam points forward out of the figurine's hand. These transform constants are cosmetic only
/// — the gameplay light cone points along the unit's facing, not the model (see `light`).
const FLASHLIGHT_GLB: &str = "low_poly_flashlight/low_poly_flashlight.glb";
// Chest-height, forward — VALKYRIE's hands rest at her slung rifle across the torso, so the flashlight
// reads as held there. Pre-scale local (child of the unit, ×`FIGURINE_SCALE`); scale bumped to keep the
// same world size now that the unit scale dropped from 2.6 to 1.13. Cosmetic — tune by devshot.
const FLASHLIGHT_OFFSET: Vec3 = Vec3::new(0.15, 1.2, -0.35);
// The flashlight model exports ~2.5 units tall, so this lands it at a handheld ~0.2 m once multiplied
// through `FIGURINE_SCALE` (child of the unit). Cosmetic — tune by devshot.
const FLASHLIGHT_SCALE: f32 = 0.08;
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

// ---------------------------------------------------------------------------------------------
// VALKYRIE skeletal locomotion — a continuous blend space over one shared gait phase.
// ---------------------------------------------------------------------------------------------
//
// Ten clips stay resident on every figurine's `AnimationPlayer` and are never restarted; each frame
// this module writes ten weights and one phase. Eight of them are the locomotion blend space
// (`anim::blend`), driven by the unit's smoothed speed and its travel direction *in its own frame*.
// The last two — aim and fire — are masked to the upper body and layered over the top, so a unit
// keeps walking while it shoots (Shroff, "Realizing NPCs", Game AI Pro 2 ch. 36 §36.4.1/§36.4.3).
//
// The blend/phase machinery itself lives in `crate::anim`; read its module docs for why there is no
// transition at all. What lives here is the VALKYRIE-specific half: which glb clip fills which slot,
// each gait clip's measured metadata, and the mapping from `Velocity`/`AimTarget` to weights.

/// glb animation indices in the 20-clip retargeted rig. **Load-bearing** — the Mixamo rifle retarget
/// already reordered them once, so `tests/valkyrie_asset.rs` pins index → name against the asset.
const CLIP_IDLE: usize = 0;
const CLIP_IDLE_ALERT: usize = 1;
const CLIP_AIM: usize = 3;
const CLIP_FIRE: usize = 4;
const CLIP_WALK: usize = 5;
const CLIP_WALK_BACK: usize = 8;
const CLIP_RUN: usize = 11;
const CLIP_RUN_BACK: usize = 12;
/// `valkyrie_strafe_l` (13) and `valkyrie_strafe_r` (14) are wired **by measured direction, not by
/// name**: the rig faces glTF `+Z`, so the character's own right is local `−X` (`hand_r`, `foot_r` and
/// `thigh_r` all sit at negative X in the bind pose), and clip 13's planted foot drives the body toward
/// `−X` — i.e. clip 13 sidesteps to the character's RIGHT and clip 14 to its LEFT. The names are
/// inverted in the source asset; wiring them by name would send units skating the wrong way on both
/// sides. See the artist notes in `docs/artist_guide.md`.
const CLIP_STRAFE_LEFTWARD: usize = 14;
const CLIP_STRAFE_RIGHTWARD: usize = 13;

/// `(duration s, phase offset, cycle distance world u)` for each gait clip, measured off
/// `assets/characters/valkyrie.glb` and scaled by [`FIGURINE_SCALE`]:
///
/// * **duration** — the clip's own length.
/// * **phase offset** — where this clip sits relative to `walk`, from cross-correlating both feet's
///   height curves. It is the *negative* of the lag that best aligns them, because the shared φ is
///   expressed in `walk`'s frame and each clip is seeked to `frac(φ + offset)`. Walk and run come out
///   0.016 apart — the pair that matters most was already aligned by the retarget.
/// * **cycle distance** — how far the character actually travels per cycle, taken from the *planted*
///   foot's velocity relative to the static `Root` (these are in-place clips: every one has a
///   bit-exactly zero root translation). This is what sets the cadence, so it is measured rather than
///   guessed; the previous hand-entered `WALK_AUTHOR_SPEED = 1.5` was ~1.5× the real 0.98 u/s and made
///   every walking unit drag its feet.
///
/// Speed correction from these numbers is §36.2.5, generalised from one clip to a blend by
/// `anim::gait_cycles_per_sec`.
const GAIT_WALK: (f32, f32, f32) = (1.417, 0.000, 1.388);
const GAIT_RUN: (f32, f32, f32) = (0.750, -0.016, 2.135);
const GAIT_WALK_BACK: (f32, f32, f32) = (1.458, -0.141, 1.538);
const GAIT_RUN_BACK: (f32, f32, f32) = (0.583, -0.062, 1.185);
/// Clip 14 (`valkyrie_strafe_r`), which travels to the character's left.
const GAIT_STRAFE_LEFTWARD: (f32, f32, f32) = (0.583, 0.047, 1.259);
/// Clip 13 (`valkyrie_strafe_l`), which travels to the character's right.
const GAIT_STRAFE_RIGHTWARD: (f32, f32, f32) = (0.708, -0.031, 1.937);

/// Mask group holding every bone below the waist. The aim/fire clips mask it out, so on those bones
/// they contribute nothing and the locomotion mixture poses the legs alone.
const MASK_LOWER_BODY: u32 = 0;

/// The bones in [`MASK_LOWER_BODY`]. Everything from `spine_01` up stays with the action layer, which
/// is where a rifle pose belongs. Matched by name against the live skeleton (never by a precomputed
/// path), so a re-export that renames a bone shows up as a missing name rather than a silently wrong
/// mask — `tests/valkyrie_asset.rs` asserts every one of these exists in the glb.
const LOWER_BODY_BONES: [&str; 14] = [
    "Root",
    "pelvis",
    "thigh_l",
    "thigh_r",
    "calf_l",
    "calf_r",
    "foot_l",
    "foot_r",
    "ball_l",
    "ball_r",
    "skirt_l",
    "skirt_r",
    "thigh_holster",
    "ammo_pouch",
];

/// Slot index of the looping aim pose, and of the one-shot fire recoil — both on the masked upper-body
/// layer, immediately after the eight locomotion slots of [`anim::blend`].
const SLOT_AIM: usize = anim::blend::LOCO_SLOTS;
const SLOT_FIRE: usize = anim::blend::LOCO_SLOTS + 1;
const N_SLOTS: usize = anim::blend::LOCO_SLOTS + 2;

/// Share of the upper body the action layer takes when a unit is aiming or firing.
///
/// Deliberately short of 1.0. The root blend node normalises per bone, so the locomotion clips carry
/// `1 − ACTION_ALPHA` and the action clips carry `ACTION_ALPHA`; on lower-body bones the action clips
/// are masked out and never pushed, leaving the locomotion mixture as the sole contributor at whatever
/// common factor — its internal ratios, and hence the pose, are untouched. At exactly 1.0 every
/// locomotion weight would be bit-zero, `animate_targets` would skip them all, and the legs would fall
/// to the bind pose. The 10% residue also keeps the arms swinging a little with the stride, which
/// reads better than a hard override.
const ACTION_ALPHA: f32 = 0.9;

/// Time constant of the cosmetic speed/direction filter, seconds.
///
/// `unit_movement` slams `Velocity` to zero the tick a unit arrives (and holds it there while idle), so
/// the raw signal is a cliff, not a curve. §36.2.5 wants the controller to approach its target speed
/// "using smoothing or an acceleration/deceleration curve" — but `Velocity` is hashed sim state, so the
/// smoothing has to live out here on the cosmetic side instead.
///
/// Kept short on purpose: the smoothed speed also drives the gait phase, so a long tail would keep the
/// legs striding after the character has physically stopped. At 0.10 s the residual foot-slide on a
/// hard stop is a few centimetres — a much better trade than the snap it replaces.
const LOCO_SMOOTH_TAU: f32 = 0.10;

/// Below this planar speed the travel direction is meaningless, so the smoothed direction holds its
/// last value instead of chasing numerical noise — a decelerating unit keeps facing the way it was going.
const DIR_HOLD_SPEED: f32 = 0.05;

/// The one shared VALKYRIE animation graph and its slot table. Every figurine's `AnimationPlayer`
/// points at this graph; the table is handed out by refcount, never copied. `pub(crate)` so the
/// Research Room's spawn button can pass it to [`spawn_unit`].
#[derive(Resource)]
pub(crate) struct ValkyrieAnim {
    graph: Handle<AnimationGraph>,
    slots: Arc<[anim::Slot]>,
}

/// Cosmetically smoothed locomotion parameters for one figurine. Lives on the `FigurineModel` child
/// beside its [`anim::BlendSource`] — never on the `Unit` — so the hashed squad archetype is never
/// split (issue #18; same reason the scene and the `Recolored` tag live here). Inserted in the spawn
/// batch itself, so the child's archetype never churns at an async tick either.
#[derive(Component, Default)]
struct LocoSmooth {
    /// Smoothed planar speed, world units/second.
    speed: f32,
    /// Smoothed travel direction as `(x, z)` **in the unit's own frame** — a vector, not an angle, so
    /// the filter can't blow up wrapping across ±π. Zero until the unit first moves.
    dir: Vec2,
}

/// Build the shared VALKYRIE animation graph once at startup (before any unit can animate).
///
/// Flat by necessity: a blend node contributes its own *static* weight, and per-instance control
/// exists only on the leaf clips (`weight = active_animation.weight * graph_node.weight`), so an
/// intermediate "action layer" node could not be faded per unit. Masking the two action clips
/// individually gets the same layering with none of that problem.
fn build_valkyrie_anim(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let mut graph = AnimationGraph::new();
    let root = graph.root;
    let clip = |i: usize| {
        assets.load::<AnimationClip>(GltfAssetLabel::Animation(i).from_asset(FIGURINE_GLB))
    };

    // Order matters: these must line up with `anim::blend`'s `SLOT_*` constants.
    let idle = graph.add_clip(clip(CLIP_IDLE), 1.0, root);
    let idle_alert = graph.add_clip(clip(CLIP_IDLE_ALERT), 1.0, root);
    let walk = graph.add_clip(clip(CLIP_WALK), 1.0, root);
    let run = graph.add_clip(clip(CLIP_RUN), 1.0, root);
    let walk_back = graph.add_clip(clip(CLIP_WALK_BACK), 1.0, root);
    let run_back = graph.add_clip(clip(CLIP_RUN_BACK), 1.0, root);
    let strafe_l = graph.add_clip(clip(CLIP_STRAFE_LEFTWARD), 1.0, root);
    let strafe_r = graph.add_clip(clip(CLIP_STRAFE_RIGHTWARD), 1.0, root);
    // The upper-body layer. The mask is populated from the live skeleton on the first attach.
    let mask = 1 << MASK_LOWER_BODY;
    let aim = graph.add_clip_with_mask(clip(CLIP_AIM), mask, 1.0, root);
    let fire = graph.add_clip_with_mask(clip(CLIP_FIRE), mask, 1.0, root);

    let slots: Arc<[anim::Slot]> = Arc::from([
        anim::Slot::free(idle, 1.0),
        anim::Slot::free(idle_alert, 1.0),
        anim::Slot::gait(walk, GAIT_WALK.0, GAIT_WALK.1, GAIT_WALK.2),
        anim::Slot::gait(run, GAIT_RUN.0, GAIT_RUN.1, GAIT_RUN.2),
        anim::Slot::gait(walk_back, GAIT_WALK_BACK.0, GAIT_WALK_BACK.1, GAIT_WALK_BACK.2),
        anim::Slot::gait(run_back, GAIT_RUN_BACK.0, GAIT_RUN_BACK.1, GAIT_RUN_BACK.2),
        anim::Slot::gait(
            strafe_l,
            GAIT_STRAFE_LEFTWARD.0,
            GAIT_STRAFE_LEFTWARD.1,
            GAIT_STRAFE_LEFTWARD.2,
        ),
        anim::Slot::gait(
            strafe_r,
            GAIT_STRAFE_RIGHTWARD.0,
            GAIT_STRAFE_RIGHTWARD.1,
            GAIT_STRAFE_RIGHTWARD.2,
        ),
        anim::Slot::free(aim, 1.0),
        anim::Slot::one_shot(fire, 1.0),
    ]);
    debug_assert_eq!(slots.len(), N_SLOTS);

    commands.insert_resource(ValkyrieAnim { graph: graphs.add(graph), slots });
}

/// Populate [`MASK_LOWER_BODY`] from the first VALKYRIE skeleton to finish streaming in. Every figurine
/// shares one skeleton, so the `AnimationTargetId`s are identical across instances and the group is
/// built exactly once.
///
/// It is a system of its own rather than a step inside the shared attach pass because the bones
/// carry their `AnimationTargetId`/`AnimatedBy` on their own schedule: keying the build to the single
/// frame an `AnimationPlayer` appears would make it depend on scene-spawn ordering. Here it simply
/// retries until a real skeleton answers, then stops for good.
fn build_lower_body_mask(
    anim: Res<ValkyrieAnim>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut done: Local<bool>,
    figurines: Query<&anim::PoseBlender, With<FigurineModel>>,
    bones: Query<(&Name, &AnimationTargetId, &AnimatedBy)>,
) {
    if *done {
        return;
    }
    // Any figurine will do, and it does not matter which one the (unstable) query order hands back:
    // `AnimationTargetId` is a hash of the bone's name path, so every VALKYRIE instance produces the
    // same ids and therefore the same mask group.
    let Some(figurine) = figurines.iter().next() else {
        return; // no figurine has been wired yet
    };
    // `AssetMut`, so it must be a `mut` binding to deref-mut through; the write also flags the graph
    // as changed, which is what makes `thread_animation_graphs` re-thread it with the new mask.
    let Some(mut graph) = graphs.get_mut(&anim.graph) else {
        return;
    };
    let mut found = 0;
    for (name, id, animated_by) in &bones {
        if animated_by.0 == figurine.player && LOWER_BODY_BONES.contains(&name.as_str()) {
            graph.add_target_to_mask_group(*id, MASK_LOWER_BODY);
            found += 1;
        }
    }
    if found == 0 {
        return; // the skeleton's bones have not been given their targets yet — try again next frame
    }
    *done = true;
    if found == LOWER_BODY_BONES.len() {
        // A one-shot line, so a windowed run positively confirms the mask was built — otherwise
        // "nothing logged" is ambiguous between "matched all 14" and "still waiting for bones".
        info!("valkyrie: upper-body mask built — {found} lower-body bones excluded from aim/fire");
    } else {
        // A real skeleton answered but does not carry the bones the mask contract names, so the
        // upper-body layer would silently pose the legs. Say so loudly — `tests/valkyrie_asset.rs` is
        // the gate that stops this reaching a build in the first place.
        error!(
            "valkyrie: lower-body mask matched {found}/{} bones — aim/fire will bleed into the legs. \
             Expected {LOWER_BODY_BONES:?}",
            LOWER_BODY_BONES.len()
        );
    }
}

/// This frame's ten slot weights for one figurine.
///
/// [`anim::blend::locomotion_weights`] gives a partition of unity over the eight locomotion slots;
/// those are scaled by `1 − α` and the two masked action slots take `α`. The total therefore stays
/// exactly 1, which is what lets `anim::apply_pose_blenders` ease every weight at one rate without the
/// mixture drifting — and, because the action clips are masked out of the lower body, what makes the
/// legs keep walking at full strength while the upper body aims or recoils (§36.4.1/§36.4.3).
fn valkyrie_weights(speed: f32, theta: f32, aiming: bool, firing: bool) -> [f32; N_SLOTS] {
    let alpha = if firing || aiming { ACTION_ALPHA } else { 0.0 };
    let mut weights = [0.0f32; N_SLOTS];
    for (slot, w) in anim::blend::locomotion_weights(speed, theta, aiming).iter().enumerate() {
        weights[slot] = w * (1.0 - alpha);
    }
    // Within the layer, the recoil owns it while it plays and hands back to the aim pose after.
    weights[SLOT_FIRE] = if firing { alpha } else { 0.0 };
    weights[SLOT_AIM] = if firing { 0.0 } else { alpha };
    weights
}

/// Turn each unit's live movement into this frame's blend weights. Speed and travel direction are
/// filtered first (see [`LOCO_SMOOTH_TAU`]); [`valkyrie_weights`] does the rest.
fn drive_valkyrie_animation(
    time: Res<Time>,
    // Bolts spawned this frame; their `shooter` tells us which units just fired so the figurine can
    // play the one-shot fire clip. A purely cosmetic read of the sim's output — never writes hashed
    // state, and `fire_laser` already gates firing by role, so the flashlight Researcher never appears.
    new_bolts: Query<&crate::laser::Laser, Added<crate::laser::Laser>>,
    mut figurines: Query<
        (&ChildOf, &mut anim::PoseBlender, &mut LocoSmooth),
        With<FigurineModel>,
    >,
    units: Query<(&Transform, &Velocity, &AimTarget), With<Unit>>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    if dt <= 0.0 {
        return;
    }
    let ease = 1.0 - (-dt / LOCO_SMOOTH_TAU).exp();

    // Units that fired a bolt this frame. Tiny (≤ squad size), so the linear `contains` below is fine —
    // this is a membership set with no ordering, so there is no sort to make canonical (a raw sort here
    // would trip the determinism lint for nothing).
    let fired: Vec<Entity> = new_bolts.iter().map(|b| b.shooter).collect();

    for (child_of, mut blender, mut smooth) in &mut figurines {
        let unit = child_of.parent();
        let Ok((transform, velocity, aim)) = units.get(unit) else {
            continue; // parent isn't a unit (or despawned)
        };

        // --- smooth the raw sim signal ----------------------------------------------------------
        let raw_speed = velocity.0.length();
        smooth.speed += (raw_speed - smooth.speed) * ease;
        if raw_speed > DIR_HOLD_SPEED {
            // Rotate the world-space travel direction into the unit's own frame. `unit_facing` only
            // ever yaws, so this is a planar rotation and the y component stays ~0.
            let world = Vec3::new(velocity.0.x, 0.0, velocity.0.y) / raw_speed;
            let local = transform.rotation.inverse() * world;
            let want = Vec2::new(local.x, local.z);
            smooth.dir = (smooth.dir + (want - smooth.dir) * ease).normalize_or(want);
        }

        // --- the upper-body action layer --------------------------------------------------------
        // `active_shot` is bookkept by the apply pass, which runs *after* this system, so a bolt fired
        // this frame is not visible there yet — counting it here keeps the recoil from lagging a frame
        // behind the muzzle flash.
        let just_fired = fired.contains(&unit);
        if just_fired {
            blender.trigger(SLOT_FIRE);
        }
        let firing = just_fired || blender.active_shot() == Some(SLOT_FIRE);
        let aiming = aim.0.is_some();

        // --- the locomotion blend space + the masked action layer --------------------------------
        let theta = anim::blend::travel_angle(smooth.dir);
        let weights = valkyrie_weights(smooth.speed, theta, aiming, firing);

        if let Err(e) = blender.set_targets(&weights) {
            error!("valkyrie: {e}");
        }
        blender.set_ground_speed(smooth.speed);
    }
}

pub struct SquadPlugin;

impl Plugin for SquadPlugin {
    fn build(&self, app: &mut App) {
        // `unit_movement` and death are PINNED sim → `FixedUpdate` (fixed dt, frame-rate independent).
        // `command_input` stays on `Update` (it reads mouse/cursor input, which is a per-frame concern);
        // the `MoveOrder` it inserts is simply picked up by the next fixed tick — a sub-frame latency the
        // player can't perceive. `recolor_units` is cosmetic and stays on `Update`.
        // Chained: `spawn_unit` pins the `ValkyrieAnim` graph + slots on each figurine child as an
        // `anim::BlendSource`, so the resource must exist before the first unit spawns.
        // `build_valkyrie_anim` loads assets (process-lifetime) and stays on `Startup`; `spawn_squad`
        // populates the world and is therefore per-run (FVS-A-5).
        app.add_systems(Startup, build_valkyrie_anim)
            .add_systems(OnEnter(crate::session::RunState::Active), spawn_squad.in_set(crate::session::RunBuild::Populate))
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
            // Cosmetic skeletal animation stays on `Update` (never touches hashed state). Attaching is
            // the shared `anim::attach_pose_blenders` pass (the figurine child carries a `BlendSource`
            // from spawn); `drive` runs after it so a figurine wired this frame gets its first weights
            // immediately (Bevy inserts the command flush on the ordered edge), and before
            // `PoseBlendSet` so the shared apply pass sees this frame's targets.
            .add_systems(
                Update,
                (
                    recolor_units,
                    build_lower_body_mask.after(anim::PoseAttachSet),
                    drive_valkyrie_animation
                        .after(anim::PoseAttachSet)
                        .before(anim::PoseBlendSet),
                ),
            );
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
    valk: Res<ValkyrieAnim>,
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
    if cells.len() < 5 {
        warn!(
            "squad spawn: only {} floor cells in the spawn spiral, spawning {} member(s) instead of 5",
            cells.len(),
            cells.len()
        );
    }

    // The role + persona roster, index-matched to spawn order (member i plays role i). Loaded from
    // `assets/config/personas.ron` when present (validated), else the code-literal defaults — a
    // malformed/invalid override is a loud startup panic, never a silent default (mirrors roles.ron).
    let personas = load_personas().unwrap_or_else(|e| panic!("personas.ron: {e}"));

    // The roster node, spawned before its members so every unit can name it at spawn (a relationship
    // source may not point at an entity that does not exist yet).
    let squad = commands.spawn((Squad, crate::session::run_scoped())).id();

    for (i, &cell) in cells.iter().enumerate() {
        // The normal squad's `SquadMember` and role are the same 0..5 index, so pass `i` for both — the
        // spawn stays byte-identical to the pre-extraction loop.
        spawn_unit(
            &mut commands,
            &assets,
            &valk,
            &sim,
            &beh,
            personas[i].clone(),
            dungeon.cell_center(cell),
            i,
            i,
            squad,
        );
    }
}

/// Spawn one squad `Unit` at world `pos` playing role `member_index` (0..5), returning its entity.
/// Extracted verbatim from [`spawn_squad`]'s loop body so the dev-only Research Room (`FVS_RESEARCH_ROOM`)
/// can drop a single live unit through the exact same path. The spawn is byte-identical to the original
/// (same components, order, and per-index `seed = member_index + 1`; the caller passes `pos =
/// dungeon.cell_center(cell)`), so the deterministic squad archetype/hash is unchanged — the golden
/// replay gate verifies it. See the original notes there for why the figurine rides a child, why every
/// component is on *every* unit, and why the flashlight branches on the role value.
pub(crate) fn spawn_unit(
    commands: &mut Commands,
    assets: &AssetServer,
    valk: &ValkyrieAnim,
    sim: &SimTuning,
    beh: &crate::behavior_tuning::BehaviorTuning,
    persona: crate::squad_ai::persona::Persona,
    pos: Vec3,
    role_index: usize,
    squad_member: usize,
    squad: Entity,
) -> Entity {
    let i = role_index;
    let outfit = OUTFITS[i];
    // `SquadMember` must stay a UNIQUE, stable per-unit key — `laser::fire`'s `sort_total!` and
    // `sim_harness::issue_squad_order` sort by it, and a tie panics the total-sort guard. The decision
    // `seed` likewise decorrelates per-unit draws and seeds `CyanideSmell` (another total-sort tiebreak).
    // Both derive from `squad_member`, NOT the (possibly repeated) role, so a dev tool spawning several
    // units of the same role (the Research Room) still gets distinct keys. The normal squad passes
    // `squad_member == role_index == i`, so its spawn stays byte-identical (golden replay verifies it).
    let seed = (squad_member as u32).wrapping_add(1);
    let figurine: Handle<WorldAsset> =
        assets.load(GltfAssetLabel::Scene(0).from_asset(FIGURINE_GLB));
    let mut unit = commands.spawn((
        // Grouped to stay under Bevy's 15-element tuple cap.
        (crate::session::run_scoped(), Unit),
        // Grouped: the bundle is at Bevy's 15-element tuple ceiling, and these two are the same fact —
        // who this operative is, and whose roster they are on.
        (SquadMember(squad_member), MemberOf(squad)),
        Prey, // crabs may swarm/bite units (nearest-prey targeting)
        MoveSpeed(beh.squad_move.unit_speed),
        Velocity(Vec2::ZERO),
        AimTarget(None),
        FacingOverride(None),
        Health::new(sim.combat.unit_hp),
        Biological, // living flesh Almond Water can heal (Foundation is a flesh faction)
        Outfit(outfit),
        (
            RoleId::ALL[i],
            persona,
            Drives::new(),
            crate::ai::faction::Faction::Foundation,
            ActiveBehavior::new(seed),
            ThinkTimer::staggered(seed),
            DesiredMove::default(),
            crate::squad_ai::perception::PerceptionLatch::default(),
            UtterCooldown::default(),
            MemoryStream::default(),
            crate::squad_ai::dialogue::SpokenLines::default(),
            crate::health::CyanideSmell::from_seed_in(crate::health::smell_seed::UNIT, seed as u64),
            // SCP-1048-A's ear-growth affliction. Always present and inert at 0 — never inserted when
            // a unit is first screamed at, which would migrate the archetype mid-run (the
            // `parasite::Infestation` idiom).
            crate::scp1048::EarGrowth::default(),
        ),
        FigurineSource(figurine.clone()),
        Visibility::default(),
        Transform::from_translation(pos).with_scale(Vec3::splat(FIGURINE_SCALE)),
        avian3d::prelude::TransformInterpolation,
    ));
    unit.insert(crate::parasite::host_infestation_bundle());
    // FVS-O-1b: what this operative knows. A SECOND `insert` because the bundle above is already at
    // Bevy's 15-element tuple cap — the same idiom the infestation bundle uses. A **value field present
    // from spawn**, never a marker toggled on acquisition, so learning something cannot split the
    // hashed archetype (`scp1048`'s rule).
    unit.insert(crate::knowledge::Knowledge::default());
    unit.with_child((
        FigurineModel,
        WorldAssetRoot(figurine),
        Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
        // The child owns the cosmetic animation state: `anim::attach_pose_blenders` wires the
        // streamed-in `AnimationPlayer` to the nearest `BlendSource` ancestor — this entity — so the
        // `PoseBlender` lands here beside `LocoSmooth`, never on the hashed `Unit` (issue #18).
        anim::BlendSource { graph: valk.graph.clone(), slots: valk.slots.clone() },
        LocoSmooth::default(),
    ));
    if RoleId::ALL[i] == RoleId::Researcher {
        unit.with_child((
            GunModel,
            FlashlightModel,
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(FLASHLIGHT_GLB))),
            Transform::from_translation(FLASHLIGHT_OFFSET)
                .with_scale(Vec3::splat(FLASHLIGHT_SCALE))
                .with_rotation(Quat::from_rotation_x(FLASHLIGHT_PITCH)),
        ));
    }
    unit.id()
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
    units: Query<(Entity, &Health, &Transform, &Outfit, &FigurineSource, &SquadMember), With<Unit>>,
) {
    // Death-din (`NOISE_SQUAD`) deposits are collected here and sorted before queueing: the query order
    // over dead units is not stable across App instances (async GLB load + entity-id reuse), so an
    // unsorted batch would smear the din channel order-dependently (see `field::sort_deposits`). Every
    // sibling deposit site already sorts (e.g. `crab_despawn_dead` by `Seed`); this one did not.
    let mut noise: Vec<crate::ai::field::Deposit> = Vec::new();
    // CANONICAL ORDER — load-bearing, and for a second reason beyond the din batch: the `GoreEvent` pushes
    // below fix `GibRing` insertion order (so the `max_gibs` cap evicts a different `Carryable`, which
    // `crab::assign_meat_targets` reads) and advance `drain_gore`'s shared per-event seed counter. Sorting
    // only `noise` left both unguarded. `SquadMember` is the stable spawn index — the same key
    // `sim_harness::issue_squad_order` and `laser::fire_laser` order by, for the same reason.
    let mut dead: Vec<(usize, Entity, &Transform, &Outfit, &FigurineSource)> = units
        .iter()
        .filter(|(_, hp, _, _, _, _)| hp.current <= 0.0)
        .map(|(entity, _, transform, outfit, figurine, member)| (member.0, entity, transform, outfit, figurine))
        .collect();
    crate::sort_total!(&mut dead, |&(member, ..)| member);

    for (_, entity, transform, outfit, figurine) in dead {
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
    names: Query<&Name>,
    has_material: Query<(), With<MeshMaterial3d<StandardMaterial>>>,
) {
    for (figurine, child_of) in &figurines {
        let Ok(outfit) = outfits.get(child_of.parent()) else {
            continue; // parent isn't a unit (or despawned) — nothing to color
        };
        // DFS the figurine subtree carrying "are we inside the chest-rig (`valkyrie_chestrig`) node's
        // subtree?". Only meshes under that accent node get recolored — VALKYRIE's authored body / gear /
        // rifle / hair materials are left untouched, so members stay identifiable without discarding the
        // MTF palette. The chest rig ships as a flat olive PBR factor (no texture), so a flat outfit-tinted
        // `StandardMaterial` matches its authored style exactly.
        let mut stack: Vec<(Entity, bool)> = match children.get(figurine) {
            Ok(c) => c.iter().map(|e| (e, false)).collect(),
            Err(_) => continue, // scene not instantiated yet — retry next frame
        };
        // Mint the outfit material lazily — only once we've actually found the accent mesh to recolor.
        // Creating it up-front would orphan a fresh `StandardMaterial` every frame the scene is still
        // streaming (the guard above `continue`s) or before the accent mesh appears, churning one throwaway
        // asset per unit per frame across the async-load window. `material.is_some()` also doubles as the
        // "did we recolor the accent?" flag that gates the `Recolored` tag — so a unit is never marked done
        // before its chest rig has actually streamed in.
        let mut material: Option<Handle<StandardMaterial>> = None;
        while let Some((e, in_accent)) = stack.pop() {
            let here = in_accent
                || names.get(e).map(|n| n.as_str().contains("chestrig")).unwrap_or(false);
            if here && has_material.get(e).is_ok() {
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
                stack.extend(ch.iter().map(|c| (c, here)));
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
            // The stable spawn index, carried solely to make the neighbour sort below a TOTAL order.
            &SquadMember,
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
    // `member` rides alongside each agent purely as the neighbour sort's tiebreak — `orca::Agent` stays a
    // pure-math type with no identity field (it has its own unit tests; keep the ECS out of it).
    let agents: Vec<(Entity, usize, Agent)> = units
        .iter()
        .map(|(e, t, _, v, order, desired, member)| {
            let moving = order.is_some() || desired.is_some_and(|d| d.goal.is_some());
            (
                e,
                member.0,
                Agent {
                    pos: t.translation.xz(),
                    vel: v.0,
                    radius: beh.squad_move.orca_radius,
                    avoids: moving,
                },
            )
        })
        .collect();

    for (entity, mut transform, speed, mut velocity, mut order, desired, _) in &mut units {
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
        let mut neighbors: Vec<(usize, Agent)> = Vec::new();
        let mut blocked_by_settled = false;
        for (other, member, ag) in &agents {
            if *other == entity {
                continue;
            }
            let off = ag.pos - self_pos;
            if off.length_squared() <= beh.squad_move.orca_query_radius * beh.squad_move.orca_query_radius {
                neighbors.push((*member, *ag));
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
        // `linear_program2/3`), whose float output depends on constraint ORDER: each line is optimized
        // only against the lines BEFORE it, and the index of the first infeasible line becomes
        // `linear_program3`'s `begin_line`. Reorder the constraints and the velocity — hence `Transform`,
        // hence `snapshot_hash` — can move. ECS query iteration order is not guaranteed stable across runs
        // (archetype membership shifts as components are added/removed), so neighbours are sorted by the
        // value-sort determinism idiom of `snapshot_hash`/`update_anchor`.
        //
        // `SquadMember` is the tiebreak, and it is what makes this a TOTAL order. Position bits ALONE are
        // not: two units at bit-identical xz tie, and `sort_unstable` gives no guarantee for ties — it
        // falls back to the very input order this sort exists to erase. Coincident positions are reachable
        // here (units spawn on cell centres; `resolve_move` clamps to identical floats). Same shape as the
        // crab-side sorts, which append `Seed`/`GibKey` after their bit-triples for this reason.
        // `blocked_by_settled` above is an order-independent OR, so it needs no sort.
        crate::sort_total!(&mut neighbors, |(member, a)| (a.pos.x.to_bits(), a.pos.y.to_bits(), *member));
        let neighbors: Vec<Agent> = neighbors.into_iter().map(|(_, a)| a).collect();

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

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit walking with its rifle up must have BOTH: real weight on a gait clip (its legs keep
    /// striding) and real weight on the masked aim clip (its upper body holds the rifle). The state
    /// machine this replaced could not express that at all — firing replaced locomotion outright and
    /// froze the legs mid-stride for the whole 1.167 s clip.
    #[test]
    fn aiming_while_walking_layers_instead_of_replacing() {
        let forward = 0.0;
        let w = valkyrie_weights(1.0, forward, true, false);
        assert!(
            w[anim::blend::SLOT_WALK] > 0.05,
            "the legs must keep walking under the aim layer: {w:?}"
        );
        assert!(w[SLOT_AIM] > 0.05, "the upper body must take the aim pose: {w:?}");
        assert_eq!(w[SLOT_FIRE], 0.0, "not firing, so the recoil slot stays silent");
    }

    #[test]
    fn firing_takes_the_layer_from_the_aim_pose_and_still_leaves_the_legs_running() {
        let w = valkyrie_weights(3.0, 0.0, true, true);
        assert!(w[SLOT_FIRE] > 0.05, "the recoil must own the layer while it plays: {w:?}");
        assert_eq!(w[SLOT_AIM], 0.0, "the aim pose hands the layer over while firing");
        assert!(w[anim::blend::SLOT_RUN] > 0.05, "a unit shooting on the move keeps running: {w:?}");
    }

    /// The layer split only works because the whole vector is a partition of unity: the root blend
    /// node normalises per bone, so the locomotion clips must carry exactly `1 − α` for the action
    /// clips' `α` to read as that share of the upper body — and for the lower body, where the action
    /// clips are masked out, to keep its mixture's internal ratios untouched.
    #[test]
    fn the_full_weight_vector_is_a_partition_of_unity() {
        for si in 0..=40 {
            let speed = 8.0 * si as f32 / 40.0;
            for ti in 0..=40 {
                let theta = -std::f32::consts::TAU + 2.0 * std::f32::consts::TAU * (ti as f32 / 40.0);
                for aiming in [false, true] {
                    for firing in [false, true] {
                        let w = valkyrie_weights(speed, theta, aiming, firing);
                        let sum: f32 = w.iter().sum();
                        assert!(
                            (sum - 1.0).abs() < 1.0e-5,
                            "speed {speed} theta {theta} aiming {aiming} firing {firing} → {sum}: {w:?}"
                        );
                    }
                }
            }
        }
    }

    /// `ACTION_ALPHA` must stay short of 1.0. At exactly 1.0 every locomotion weight is bit-zero,
    /// `animate_targets` skips them all, and the lower body — which the action clips are masked out of
    /// — falls to the bind pose.
    #[test]
    fn the_action_layer_never_starves_the_legs_completely() {
        assert!(ACTION_ALPHA < 1.0, "ACTION_ALPHA must leave the locomotion clips some weight");
        let w = valkyrie_weights(3.0, 0.0, true, true);
        let loco: f32 = w[..anim::blend::LOCO_SLOTS].iter().sum();
        assert!(loco > 0.0, "the legs would fall to the bind pose: {w:?}");
    }
}
