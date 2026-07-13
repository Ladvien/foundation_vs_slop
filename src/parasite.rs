//! SCP-150 — the parasitic isopod ("tongue-eating louse", *Cymothoa exigua* body plan).
//!
//! A slow-burn body-horror **anomaly** that is nothing like the crab swarm's fast attrition: a lone,
//! free-crawling juvenile (a **manca**, the real animal's host-seeking larval stage) stalks a host,
//! leaps onto it, burrows *inside*, gestates for an in-world "hour", then **bursts out** — killing the
//! host and birthing a small brood that begins the cycle again. Its horror is the reproduction loop:
//! it can only breed by spending a host, so it is a self-limiting parasite (Ruiz-L & Madrid-V 1992,
//! *Biology of Cymothoa exigua*, DOI 10.7773/cm.v18i1.885 — the manca stage, the single-brood-then-death
//! life history) rather than an infinite spawner. Later phases give it the **extended phenotype** —
//! hijacking the host's AI to isolate it from its group before the burst (Heil 2016, *Host Manipulation
//! by Parasites*, DOI 10.3389/fevo.2016.00080).
//!
//! Hosts are **both** squad units and crabs — a three-body web (parasite ↔ crab ↔ squad). They carry an
//! always-present [`Parasitizable`] marker + [`Infestation`] state (a flipped field, never an
//! inserted/removed component, so the hashed host archetype never splits — see `squad.rs`'s note).
//!
//! Built on the `crate::crab` template: an unscaled root (invisible laser-hit sphere + `Hostile` +
//! `Health`) with the scaled `scp-150.glb` scene as a child, surface-manifold locomotion over the shared
//! [`crate::surface_nav::SurfaceGraph`], and the crab's `AnimationGraph`/`AnimationTransitions` clip
//! recipe. This module is **Phase 1** (free manca: crawl / stalk / leap + animation); embed, gestation,
//! and burst arrive in later phases.

use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

use bevy::prelude::*;

use crate::ai::field::{FieldId, Stig};
use crate::config::GameConfig;
use crate::dungeon::Dungeon;
use crate::enemy::Hostile;
use crate::gore::{GoreEvent, GoreKind, GoreQueue};
use crate::health::{Health, NoHealthBar};
use crate::light::{light_push, LightField, Photophobic};
use crate::placement::PlacedIn;
use crate::sim::SimTuning;
use crate::surface_nav::{clamp_to_patch, project_tangent, surface_orientation, SurfaceGraph};
use crate::util::{hash01_u32, nearest_planar, unit_is_facing};
use crate::wfc::{E, N, S, W};

/// The SCP-150 asset (skinned glTF, 12 baked clips). Clip indices follow the asset manual's table order:
/// 0 Idle_Snug · 1 Idle_Alert · 2 Walk1 · 3 Walk2 · 4 Run · 5 Leap · 6 Attack1 · 7 Attack2 · 8 Forage1 ·
/// 9 Forage2 · 10 BurrowOut · 11 Climb.
const SCP150_GLB: &str = "scp150/scp-150.glb";

/// Huddles seed at least this far (tiles) from the squad spawn. (Spawn geometry — count/hp/speeds are
/// gameplay knobs and live in `sim::ParasiteTuning`; harborage/huddle spacing is in the huddle-const block.)
const MANCA_MIN_SPAWN_DIST: f32 = 8.0;

/// Uniform render scale for the child model. The asset body is ≈3.6 long in Blender units; at 0.0275 the
/// juvenile reads ≈0.1 m — a tiny scuttling louse, ¼ the earlier size (user request). Tuned by devshot.
const MANCA_RENDER_SCALE: f32 = 0.0275;
/// Root body-centre height above the surface, along the surface normal (also seats the collider). Scaled
/// with the model (¼) so the tiny body rests on the floor instead of floating above it.
const MANCA_BODY_CENTER: f32 = 0.025;
/// Local Y offset of the scaled model under the root so its body rests on the surface. Kept proportional to
/// RENDER_SCALE (¼) so the feet stay planted at the smaller size. Calibrated by eye.
const MANCA_MODEL_Y: f32 = 0.045;
/// Radius of the invisible collider sphere (the laser raycast target); world-size since the root is
/// unscaled. Kept deliberately GENEROUS relative to the ¼-size visual so the tiny mancae stay shootable —
/// the hitbox is invisible, and they huddle densely, so a bolt into a clump reliably connects with one.
pub(crate) const MANCA_COLLIDER_R: f32 = 0.12;
/// The asset faces **−X** in Blender (head/mouth at −X); the engine's forward is **−Z**. Rotate the child
/// model −90° about +Y so its head points along the entity's facing (`surface_orientation` aims −Z).
const MODEL_FACING: f32 = -FRAC_PI_2;

/// Frame-dt clamp so a hitch can't fling a manca off its surface (mirrors `crab::MAX_FRAME_DT`).
const MAX_FRAME_DT: f32 = 1.0 / 30.0;
// TRANSFER_RADIUS + NORMAL_EASE moved to `surface_nav` (shared by the crab/manca surface step;
// now consumed inside `surface_nav::steer_surface_core`).
/// Patch-normal Y below this reads as a wall (drives the Climb clip vs Walk).
const WALL_NORMAL_Y: f32 = 0.7;

/// Leap gait tone (mirrors the crab pounce, `crab::JUMP_*`). The *reach* (`leap_len`) and *cadence*
/// (`leap_cooldown`) are gameplay knobs in `sim::ParasiteTuning`; these shape constants stay in code. A
/// manca won't lunge at a host already in its face (`LEAP_MIN`); it hunkers `LEAP_HUNKER`s then arcs
/// `LEAP_AIR`s to `LEAP_ARC` peak height.
const LEAP_MIN: f32 = 1.0;
const LEAP_HUNKER: f32 = 0.3;
const LEAP_AIR: f32 = 0.35;
const LEAP_ARC: f32 = 0.8;
/// Blind-side gate: a manca only commits its leap from outside the host's facing cone. `cos(60°)` ⇒ a
/// ±60° front arc the host "sees"; the manca leaps from the rear 240° (README-style stalk-to-blind-side).
const BLIND_COS: f32 = 0.5;
/// Blind-side stalk band: while closing to within this of a host that is *facing* it, the manca arcs
/// tangentially toward the host's rear instead of charging head-on, until it slips into the blind arc.
const STALK_BAND: f32 = 3.5;
const STALK_STRENGTH: f32 = 0.8;

/// Embed reach geometry: a manca within `HOST_BODY_RADIUS + MANCA_EMBED_RANGE` (planar) of a host burrows
/// in. Sized so a landed leap (or a crawl right up to the host) connects. (The embed *damage* is a
/// gameplay knob in `sim::ParasiteTuning`; these radii are geometry consts.)
const HOST_BODY_RADIUS: f32 = 0.35;
const MANCA_EMBED_RANGE: f32 = 0.2;

/// Cross-fade between animation clips.
const CROSSFADE: Duration = Duration::from_millis(150);
/// Clip playback-rate multipliers (the authored clips are long; play them faster for a lively scuttle).
const WALK_ANIM_SPEED: f32 = 2.0;
const CLIMB_ANIM_SPEED: f32 = 2.0;
const ATTACK_ANIM_SPEED: f32 = 1.6;
/// The eruption climb-out (BurrowOut) plays slow for a dragged-out, dramatic emergence (one-shot).
const BURROW_ANIM_SPEED: f32 = 0.8;

// --- Huddle / dormancy behaviour (isopod collective ecology) ---------------------------------------
// SCP-150 mancae are gregarious like real terrestrial isopods (the *Cymothoa* body plan's land cousins):
// they aggregate into dense clusters at sheltered "harborages" — wall corners and the footprints of
// furniture — and REST there in a *calmed* state, doing nothing, until a disturbance flips them to an
// *excited* hunting state; that arousal then spreads contagiously through the cluster (poke a patch of
// daddy-long-legs and the whole clump erupts). Grounding: Devigne, Broly & Deneubourg 2011 (social
// inter-attraction drives aggregation, DOI 10.1371/journal.pone.0017389); Broly, Mullier, Deneubourg &
// Devigne 2015 (self-organized aggregation + collective shelter choice, DOI 10.1007/s10071-015-0925-6);
// Broly & Deneubourg 2015 (the calmed⇄excited two-state cohesion model with behavioural contagion,
// DOI 10.1371/journal.pcbi.1004290); Devigne, Deneubourg & Broly 2013 (harborage/sheltering benefits,
// DOI 10.1007/s00040-013-0313-7). The huddle steer blends Reynolds cohesion + separation (Olfati-Saber,
// IEEE TAC 2006, DOI 10.1109/TAC.2005.864190) with a thigmotactic harborage pull. Following the module's
// existing split, these behaviour-SHAPE consts stay here beside LEAP_*/STALK_* rather than moving into the
// harness-evolved `sim::ParasiteTuning` — adding fields there would shift the offline search's positional
// genome encoding (`world_genome`) and break saved elites mid-experiment.

/// Target mancae per huddle; the initial swarm splits into `ceil(initial_count / HUDDLE_SIZE)` clusters.
const HUDDLE_SIZE: usize = 40;
/// A huddle's mancae seed across floor cells within this radius (tiles) of the chosen harborage site.
const HUDDLE_RADIUS: f32 = 2.0;
/// Harborage sites are seeded at least this far apart (tiles) so distinct huddles occupy distinct corners.
const HARBORAGE_SEP: f32 = 6.0;
/// A floor cell needs at least this harborage score (orthogonal wall count, +3 if on/next to furniture) to
/// seed a huddle — i.e. a real corner (≥2 walls) OR a furniture hide. See `harborage_score`.
const HARBORAGE_MIN_SCORE: i32 = 2;
/// Deterministic in-patch spawn jitter (world units, ± half of this) so huddle-mates seeded onto the same
/// cell don't stack on one exact point — short-range separation needs a nonzero offset to push them apart.
const MANCA_SPAWN_JITTER: f32 = 0.2;
/// Dormant settle/creep speed (world units/s) — a huddle only ever shuffles in place; it does not travel.
const SETTLE_SPEED: f32 = 0.5;
/// A dormant manca more than this (world units) from its harborage anchor reads as *traveling* (Walk/Climb
/// anim); nearer than this it is *settled* (the snug idle) — small residual jitter doesn't flip the clip.
const SETTLE_ARRIVE: f32 = 0.6;
/// Cohesion pull toward the local dormant-neighbour centroid (flock-centering; social inter-attraction).
const COHESION_STRENGTH: f32 = 0.6;
/// Short-range separation so a huddle packs shoulder-to-shoulder without collapsing into a single point.
const HUDDLE_SEP_RADIUS: f32 = 0.35;
const HUDDLE_SEP_STRENGTH: f32 = 0.8;
/// Pull toward the manca's harborage anchor (pins the clump to its corner/furniture, resists dispersal).
const HARBORAGE_BIAS: f32 = 1.0;

/// A dormant manca rouses when the squad's gunfire field (`THREAT_GUN`) at its cell exceeds this. Low, so
/// even distant/indirect gunfire (not just a direct hit, which deposits ~`threat_per_shot`≈0.5) wakes the
/// clump — the huddles sit in far corners, so a tight threshold never fired in practice. (A direct hit ALSO
/// trips the damage trigger in `manca_rouse`, independent of this field.)
const ROUSE_THREAT: f32 = 0.04;
/// A dormant manca rouses when a fresh (un-infested) host steps within this planar distance. Generous (~a
/// room), so an approaching squad member or crab visibly startles the clump from a distance rather than
/// having to walk right into it.
const ROUSE_PROXIMITY: f32 = 5.0;
/// Arousal contagion radius: a Roused manca wakes Dormant siblings within this planar distance, so once one
/// wakes the excitation sweeps the whole cluster in a few ticks — poke it and the patch erupts as a unit
/// (Broly & Deneubourg 2015).
const ROUSE_CONTAGION_R: f32 = 3.0;
/// A Roused manca with no disturbance for this long re-settles to Dormant and creeps back to its harborage.
const ROUSE_CALM_SECONDS: f32 = 9.0;

// --- Roused swarm (readable collective motion) -----------------------------------------------------
// A roused huddle does NOT scatter into lone stalkers — it moves as ONE legible swarm, because player
// legibility is the whole point. The design has three at-a-glance silhouettes keyed to a per-manca
// commitment scalar `commit`∈[0,1]: a low-commit agitated MILL (slow, loosely aligned ring near the
// harborage) that ramps *continuously* to a high-commit CHARGE (fast, strongly polarized arrow streaming
// at the nearest host). The unified heading is emergent alignment consensus — strong alignment plus a
// WEAK per-manca seek toward the nearest fresh host makes the cluster agree on a direction and advance as
// a mass, with no shared bearing field needed (so the crab's `RallyField` is never touched). Grounding:
// Reynolds 1987 alignment — the third boids rule, previously absent here (DOI 10.1145/37402.37406); the
// alignment + weak-target-attraction → collective-heading mechanism of Couzin et al. 2002
// (DOI 10.1006/jtbi.2002.3065); the speed→polarization relationship (slow shoal → fast polarized school,
// and flash expansion under threat) of Gautrais et al. 2012 (DOI 10.1371/journal.pcbi.1002678). Consts
// stay here (NOT `sim::ParasiteTuning`) for the same reason as the huddle block — the offline search's
// positional `world_genome` encoding must not shift. The evolved crawl/climb speeds ARE reused as the
// base, so the harness still tunes them; commitment only *modulates* that base.

/// Alignment (heading matching) weight at full commitment — the readable polarization of a charge. Scaled
/// by `commit` from `ALIGN_FLOOR` up to this, so a mill is loosely aligned and a charge is a tight arrow.
const ALIGN_STRENGTH: f32 = 1.2;
/// A faint alignment always present (the dormant huddle and a commit≈0 mill) so a resting/agitated patch
/// reads as subtly coherent rather than random noise — the floor of the `commit` lerp to `ALIGN_STRENGTH`.
const ALIGN_FLOOR: f32 = 0.12;
/// Weak pull toward the nearest fresh host — grounds the alignment consensus on a real target WITHOUT
/// letting any single manca break from the swarm to beeline. Kept below `ALIGN_STRENGTH` by design.
const SEEK_STRENGTH: f32 = 0.5;
/// Roused travel-speed multiplier on the evolved crawl/climb base at commit=0 (the mill — barely faster
/// than the dormant creep) …
const MILL_SPEED_FACTOR: f32 = 0.5;
/// … and at commit=1 (the committed charge — faster than a lone stalker). `lerp` by `commit`.
const CHARGE_SPEED_FACTOR: f32 = 1.35;
/// Commitment ramps up by this per second while a roused manca has a fresh host to hunt …
const COMMIT_RAMP: f32 = 0.5;
/// … and decays by this per second when no fresh host remains — the swarm relaxes from charge back to mill.
const COMMIT_DECAY: f32 = 0.7;
/// Per-second rate at which a manca's commitment is nudged toward its roused-neighbour average, so
/// commitment spreads through the cluster (the swarm commits together — a readable wave) rather than
/// manca-by-manca. Applied as `(avg - commit) * (COMMIT_SPREAD * dt)`.
const COMMIT_SPREAD: f32 = 2.0;

/// Flash-expansion: the tick a manca rouses it briefly bursts OUTWARD from the cluster, then re-forms into
/// the mill — the readable "it woke up" pop, riding the one-ring/tick rouse contagion so the burst sweeps
/// the clump as an expanding ripple (Reynolds' "maneuver wave"; Gautrais et al. 2012 flash expansion).
const FLASH_SECS: f32 = 0.3;
/// Outward radial push (world units) at the peak of the flash pop, decaying to zero over `FLASH_SECS`.
const FLASH_IMPULSE: f32 = 2.5;
/// Separation multiplier added at the peak of the flash pop (the clump briefly over-spaces, then re-packs).
const FLASH_SEP_BOOST: f32 = 2.0;

// --- Host-burst eruption (the Alien-chestburster sequence) -----------------------------------------
// When gestation completes the host does NOT die instantly. It CONVULSES, then a manca tears out of its
// chest — dealing ⅓ of the host's MAX HP (not an instakill: the host survives, wounded, and keeps fighting),
// ripping open a persistent hole and spraying blood + flesh chunks — then the brood slowly drags itself out
// of the wound and drops to the floor. A survivor is re-infestable, but a fresh brood carries an embed
// cooldown so it can't immediately re-parasitise the host it just erupted from. Feel/timing are code consts
// (NOT `ParasiteTuning` — its positional `world_genome` encoding must not shift). All VFX (the wound, blood,
// chunks, screen-shake) are cosmetic (gore/juice on `Update`), invisible to the deterministic-core hash;
// only the ⅓-HP decrement, the ~1.5 s burst delay, and the chest-spawned brood move it.

/// The eruption costs `hp.max / this` — 3 ⇒ ⅓ of max HP; the host lives if it had more than a third left.
const BURST_DAMAGE_DIVISOR: f32 = 3.0;
/// Convulse wind-up: seconds the host shudders + blood wells before the manca tears out.
const BURST_CONVULSE_SECS: f32 = 1.5;
/// Bleed-out: seconds the wound streams after the eruption before the host is released from infestation.
const BURST_BLEED_SECS: f32 = 2.0;
/// Cadence (seconds) of the blood drips pushed at the wound during convulse + bleed.
const BLEED_INTERVAL: f32 = 0.18;
/// Seconds a manca spends slowly dragging itself OUT of the host chest (the BurrowOut climb) before it drops
/// to the floor.
const EMERGE_SECS: f32 = 1.2;
/// How far (world units, along the host's forward) a manca crawls out of the chest across the climb before
/// it drops to the floor.
const EMERGE_DIST: f32 = 0.3;
/// A freshly-erupted manca cannot embed for this long — stops the brood from instantly re-infesting the host
/// it just burst from (the requested re-infestation cooldown). Initial level mancae get 0 (embed at once).
const EMBED_COOLDOWN: f32 = 6.0;
/// Chest anchor (host-LOCAL, pre-root-scale) where the wound opens and the manca erupts — front of the torso
/// (−Z is forward). The unit figurine (root scale 2.6) and the small crab need different offsets.
const CHEST_LOCAL_UNIT: Vec3 = Vec3::new(0.0, 0.30, -0.25);
const CHEST_LOCAL_CRAB: Vec3 = Vec3::new(0.0, 0.20, -0.05);
/// Radius of the wound disc mesh; parented to the host root, so it inherits the body scale (a big unit gets
/// a proportionally bigger hole than a crab).
const WOUND_R: f32 = 0.09;
/// Screen-shake added per tick during the convulse wind-up (a rising dread rumble) and the big kick at the
/// instant of eruption. Cosmetic (`juice::Trauma`) — never gates pinned state.
const CONVULSE_TRAUMA_PER_TICK: f32 = 0.006;
const ERUPT_TRAUMA: f32 = 0.55;

// --- Host-side components (always present on every unit AND every crab) ----------------------------

/// Marks an entity a manca can infest — every squad unit and every crab (a union target set spanning two
/// creature types). A plain marker so `manca_hunt` can scan `With<Parasitizable>` without caring which
/// kind of host it is. Added at each host's spawn site, never at runtime, so it never splits a hashed
/// archetype.
#[derive(Component)]
pub struct Parasitizable;

/// A host's infestation state — **always present** (a flipped field, not an inserted/removed marker), so
/// adding/clearing an infestation never changes the host's archetype (the determinism invariant every
/// host type relies on; see `squad.rs`). Inert until a later phase wires embed/gestation/burst; a fresh
/// host is `Infestation::default()` (`active == false`).
#[derive(Component)]
pub struct Infestation {
    /// Is a parasite currently embedded and gestating in this host?
    pub active: bool,
    /// Seconds since embedding (gestation clock; advanced by a later phase).
    pub timer: f32,
    /// Stable per-infestation seed (salts the brood RNG at burst) — set on embed, never position-derived.
    pub seed: u32,
    /// The chestburster eruption sub-state. A FIELD (not an inserted component), like `active`/`timer`, so it
    /// never splits the host's hashed archetype — the determinism invariant this struct is built around (and
    /// where a future `cure_progress` would also live).
    burst: BurstPhase,
    /// Countdown within the current burst phase (Convulse → Bleed), advanced on the pinned FixedUpdate path.
    burst_timer: f32,
}

impl Default for Infestation {
    fn default() -> Self {
        Self { active: false, timer: 0.0, seed: 0, burst: BurstPhase::Idle, burst_timer: 0.0 }
    }
}

impl Infestation {
    /// Is the chestburster mid-eruption — the visible Convulse wind-up shudder or the Bleed aftermath? A
    /// public read for cosmetic/windowed consumers (the VHS anomaly-glitch tell), mirroring
    /// `enemy::SmileyState::is_angry`. The `burst` field itself stays private (it is determinism-owned,
    /// advanced only on the pinned path).
    pub fn is_erupting(&self) -> bool {
        matches!(self.burst, BurstPhase::Convulse | BurstPhase::Bleed)
    }
}

/// The host-eruption sub-state (a field on [`Infestation`]). `Idle` = gestating or clean; `Convulse` = the
/// wind-up shudder while pressure builds; `Bleed` = the wound streams after eruption, then the host is
/// released from infestation. The `Erupt` moment (⅓-HP damage + wound + brood + blood gush) is the one-shot
/// Convulse→Bleed transition, not a lingering phase.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BurstPhase {
    Idle,
    Convulse,
    Bleed,
}

/// Insert the always-present host parasitism components. Called from every host spawn site (squad units,
/// crabs) so the two markers are uniform across each host archetype.
pub fn host_infestation_bundle() -> (Parasitizable, Infestation) {
    (Parasitizable, Infestation::default())
}

// --- Manca (free parasite) components --------------------------------------------------------------

/// Marker on a manca root entity (also the raycast collider).
#[derive(Component)]
pub struct Manca;

/// A manca's position on the surface manifold and its facing.
#[derive(Component)]
pub struct MancaMotion {
    /// Current [`SurfaceGraph`] patch id.
    patch: u32,
    /// World-space point ON the current surface (pre-seat).
    pos: Vec3,
    /// Current surface normal (eased toward the patch normal across transfers).
    normal: Vec3,
    /// Last travel heading (for smooth facing).
    heading: Vec3,
    /// Harborage anchor (world): the corner/furniture centre this manca huddles at while dormant, and
    /// creeps back to when it re-settles after a hunt. Set once at spawn; the clump is pinned to it.
    home: Vec3,
}

/// A manca's ballistic-leap state — hunker then arc onto a host, mirroring `crab::CrabJump`. `Ready` =
/// grounded (normal locomotion runs); `Hunker`/`Air` own the manca's transform, so `manca_hunt` skips it.
#[derive(Component)]
pub struct MancaLeap {
    phase: LeapPhase,
    /// Time left in the current `Hunker`/`Air`/`Emerging` phase.
    timer: f32,
    /// Cooldown before the next leap (counts down while `Ready`).
    cooldown: f32,
    from: Vec3,
    to: Vec3,
    /// The host this manca is erupting FROM while `phase == Emerging` — it clings to that host's chest as it
    /// climbs out, then detaches to the floor. `None` for every normal manca (a field, so the archetype is
    /// stable).
    host: Option<Entity>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LeapPhase {
    Ready,
    Hunker,
    Air,
    /// Slowly dragging itself OUT of a host's chest (the one-shot BurrowOut clip). Owned like a leap —
    /// hunt/huddle skip it — it follows the host's chest until the climb finishes, then drops via `Air`.
    Emerging,
}

/// The manca's animation state, chosen from movement/leap each frame (drives clip selection).
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum MancaAnimState {
    /// Snug idle — a dormant manca settled into its huddle (the still, packed "patch of daddy long legs").
    Snug,
    /// Alert idle — a roused manca standing with no move/target.
    Idle,
    Walk,
    Climb,
    Attack,
    /// Erupting — the one-shot BurrowOut clip while a manca drags itself out of a host chest.
    BurrowOut,
}

/// A manca's behavioural mood — the calmed⇄excited two-state model of isopod cluster cohesion (Broly &
/// Deneubourg 2015, *Behavioural Contagion Explains Group Cohesion in a Social Crustacean*,
/// DOI 10.1371/journal.pcbi.1004290). A manca spawns [`MoodState::Dormant`]: huddled at a harborage,
/// passive — it does NOT hunt, leap, or embed. A disturbance (a host underfoot, the squad's gunfire field,
/// or an already-roused sibling nearby — the contagion) flips it to [`MoodState::Roused`] and it runs the
/// hunt→leap→embed drive. With no disturbance for `ROUSE_CALM_SECONDS` it re-settles to Dormant. Inserted
/// once at spawn and never removed, so the hashed manca archetype never splits (the module's determinism
/// invariant; cf. [`Infestation`]).
#[derive(Component)]
pub struct MancaMood {
    state: MoodState,
    /// Counts down while Roused and undisturbed; at ≤ 0 the manca re-settles to Dormant.
    calm_timer: f32,
    /// Seconds until this manca may embed into a host. A fresh brood starts at `EMBED_COOLDOWN` so it can't
    /// instantly re-infest the host it just erupted from; initial level mancae start at 0. Counted down in
    /// `manca_rouse`, gated in `manca_embed`.
    embed_cd: f32,
    /// Commitment ∈ [0,1] driving the readable mill→charge ramp of the roused swarm: 0 = a slow, loosely
    /// aligned agitated mill; 1 = a fast, strongly polarized charge. Ramps up (`COMMIT_RAMP`) while roused
    /// with a fresh host, decays (`COMMIT_DECAY`) without one, and is nudged toward the roused-neighbour
    /// average (`COMMIT_SPREAD`) so the swarm commits together. Updated in `manca_hunt`; 0 while dormant.
    commit: f32,
    /// Flash-expansion countdown (seconds): stamped to `FLASH_SECS` the tick this manca rouses, then decays
    /// to 0. While positive the manca bursts outward from the cluster — the "it woke up" pop that rides the
    /// rouse contagion as an expanding ripple. Stamped in `manca_rouse`, decayed + applied in `manca_hunt`.
    flash: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MoodState {
    Dormant,
    Roused,
}

/// Immortal per-manca spawn seed — the source of every per-instance random draw (never the spawn
/// position: a brood shares its host's cell, so a position hash would clone siblings). Mirrors
/// `crab::CrabSeed`.
#[derive(Component, Clone, Copy)]
pub struct MancaSeed(pub u32);

/// Link from a manca to its (asynchronously-spawned) `AnimationPlayer`, plus which state's clip is
/// currently playing (so `drive_manca_animation` only re-triggers on a real change).
#[derive(Component)]
struct MancaAnimPlayer {
    player: Entity,
    playing: Option<MancaAnimState>,
}

// --- Resources -------------------------------------------------------------------------------------

/// Monotonic spawn counter — a unique, ever-increasing seed handed to each manca at birth (mirrors
/// `crab::CrabSpawnSeq`). Per-manca randomization derives from THIS, never from the spawn position.
#[derive(Resource, Default)]
pub struct MancaSpawnSeq(pub u64);

/// Shared handles kept so `parasite_burst` can birth a brood at runtime without reloading (mirrors
/// `crab::CrabAssets`).
#[derive(Resource)]
pub struct MancaAssets {
    collider: Handle<Mesh>,
    scene: Handle<WorldAsset>,
}

/// A persistent chestburster hole on a host — a dark bloody disc parented to the host body (so it rides +
/// rotates with the body and despawns with it, like the gun). Cosmetic (no `Health`), so it is invisible to
/// the deterministic-core hash.
#[derive(Component)]
struct Wound;

/// The shared wound-disc mesh + material, built once so `parasite_burst` can tear a hole into a host at
/// runtime without reloading (mirrors [`MancaAssets`]).
#[derive(Resource)]
struct WoundAssets {
    disc: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

/// The one shared animation graph + node handles for the manca clips.
#[derive(Resource)]
struct MancaAnim {
    graph: Handle<AnimationGraph>,
    snug: AnimationNodeIndex,
    idle: AnimationNodeIndex,
    walk: AnimationNodeIndex,
    climb: AnimationNodeIndex,
    attack: AnimationNodeIndex,
    burrow: AnimationNodeIndex,
}

// --- Plugin ----------------------------------------------------------------------------------------

pub struct ParasitePlugin;

impl Plugin for ParasitePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MancaSpawnSeq>()
            .add_systems(Startup, (build_manca_anim, build_wound_assets, build_lump_assets))
            // Spawn in `PostStartup` so the crab's `SurfaceGraph` (built in its `Startup`) already exists —
            // mancae ride the same surface manifold and must never build a second graph.
            .add_systems(PostStartup, spawn_mancae)
            // Pinned manca simulation on `FixedUpdate`: rouse (flip mood) → huddle (dormant aggregation) →
            // hunt/leap (roused locomotion) → embed (flips a host's Infestation + despawns the manca) →
            // gestation clock → burst. All change pinned state, so ordering is explicit and the whole chain
            // is covered by the exact-hash gate. `manca_huddle` reads the LightField, so it is ordered after
            // `light::LightFieldWritten` (mirrors `crab_locomotion`).
            .add_systems(
                FixedUpdate,
                (
                    manca_rouse,
                    manca_huddle
                        .after(manca_rouse)
                        .after(crate::light::LightFieldWritten),
                    manca_hunt.after(manca_huddle),
                    manca_leap.after(manca_hunt),
                    manca_embed.after(manca_leap),
                    gestation_tick.after(manca_embed),
                    // The eruption driver: convulse → erupt (⅓-HP damage + wound + chest-spawned brood + gush)
                    // → bleed. Still ordered before the crab despawn owner (harmless now the burst no longer
                    // zeroes HP) and after the gestation clock that arms it.
                    parasite_burst.after(gestation_tick).before(crate::crab::CrabDespawn),
                    // Sole HP≤0 owner for mancae (a shot manca) — mirrors `crab_despawn_dead`.
                    manca_despawn_dead,
                    // A ROUSED brood radiates THREAT_ANOMALY into the shared fear field, so the squad's
                    // existing anomaly-fear machinery (FOUNDATION_FEAR_CHANNELS → FEAR → Flee) answers a
                    // charge with no bespoke code — and psi-vision renders it. Pushed before the drain so the
                    // dread lands this tick (mirrors `enemy::deposit_anomaly_aura`).
                    deposit_manca_dread.before(crate::ai::AiSet::Deposits),
                ),
            )
            // Cosmetic: skeletal animation attach/drive stays on `Update` (mirrors the crab).
            .add_systems(Update, (attach_manca_animation, drive_manca_animation));
    }
}

/// A **roused** SCP-150 brood radiates dread into the shared `THREAT_ANOMALY` field — the same channel the
/// living watcher emits and the only channel units read (through walls, via the Psionic) as anomalous
/// presence. Before this, the parasite was a parallel AI stack that deposited into NO stigmergy channel, so
/// a roused arrow of mancae charging a unit provoked no fear, no `Flee`, and stayed dark in psi-vision — the
/// marquee set-piece was invisible to the rest of the sim. Now a charging brood is answered by the squad's
/// existing anomaly-fear machinery with no code specific to the parasite.
///
/// Only ROUSED mancae emit: a dormant huddle is silent (it must not terrify the squad while it sleeps). This
/// is safe against the "manca death must not magnet the crab swarm" rule (`manca_despawn_dead`): crabs read
/// only `THREAT_GUN` for fear, never `THREAT_ANOMALY`, so the swarm is untouched — only units feel it.
///
/// Determinism: positions are value-sorted before emitting, because overlapping deposit discs accumulate
/// with non-associative float `+=` (the same reason `enemy::deposit_anomaly_aura` and
/// `crab::deposit_crab_fields` sort). Ordered `.before(AiSet::Deposits)` so the dread reaches the grid the
/// same tick it is drained/evaporated.
fn deposit_manca_dread(
    time: Res<Time>,
    mancae: Query<(&Transform, &MancaMood)>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    sim: Res<SimTuning>,
) {
    let amount = sim.deposit.manca_dread_rate * time.delta_secs();
    let mut positions: Vec<Vec3> = mancae
        .iter()
        .filter(|(_, mood)| mood.state == MoodState::Roused)
        .map(|(tf, _)| tf.translation)
        .collect();
    positions.sort_unstable_by_key(|p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    for pos in positions {
        deposits.0.push(crate::ai::field::Deposit {
            pos,
            field: crate::ai::field::FieldId::THREAT_ANOMALY,
            amount,
        });
    }
}

/// Build the shared animation graph over the manca clips we drive in Phase 1.
fn build_manca_anim(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let (graph, nodes) = AnimationGraph::from_clips([
        assets.load(GltfAssetLabel::Animation(0).from_asset(SCP150_GLB)), // Idle_Snug (dormant huddle)
        assets.load(GltfAssetLabel::Animation(1).from_asset(SCP150_GLB)), // Idle_Alert
        assets.load(GltfAssetLabel::Animation(2).from_asset(SCP150_GLB)), // Walk1
        assets.load(GltfAssetLabel::Animation(11).from_asset(SCP150_GLB)), // Climb
        assets.load(GltfAssetLabel::Animation(7).from_asset(SCP150_GLB)), // Attack2 (pounce-bite)
        assets.load(GltfAssetLabel::Animation(10).from_asset(SCP150_GLB)), // BurrowOut (one-shot eruption)
    ]);
    let handle = graphs.add(graph);
    commands.insert_resource(MancaAnim {
        graph: handle,
        snug: nodes[0],
        idle: nodes[1],
        walk: nodes[2],
        climb: nodes[3],
        attack: nodes[4],
        burrow: nodes[5],
    });
}

/// Build the shared wound-disc mesh + dark bloody material once (the chestburster hole stamped onto hosts).
fn build_wound_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let disc = meshes.add(Circle::new(WOUND_R));
    // A wet, near-black hole with a faint red glow so it reads as raw torn flesh even in a dark corner.
    let mat = mats.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.01, 0.01),
        emissive: LinearRgba::rgb(0.12, 0.0, 0.0),
        perceptual_roughness: 0.55,
        reflectance: 0.04,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(WoundAssets { disc, mat });
}

/// Radius of the subdermal gestation lump (a small swelling on the host body).
const LUMP_R: f32 = 0.09;
/// Local offset of the lump on the host (upper body / chest area — a generic spot that reads on both the
/// unit figurine and a crab).
const LUMP_LOCAL: Vec3 = Vec3::new(0.0, 0.45, 0.0);

/// Shared lump mesh + sickly material so the gestation tell can be attached without reloading (mirrors
/// [`WoundAssets`]).
#[derive(Resource)]
pub(crate) struct LumpAssets {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

/// A persistent "something is growing in here" tell on an infested host DURING gestation — a twitching
/// subdermal lump parented to the host body. Cosmetic (no `Health`), and its writer is windowed-only, so it
/// is invisible to the deterministic core. Carries its host `Entity` so the tell can be removed when the
/// host stops gestating (survived the burst / cured) without touching the host's archetype (the
/// `Infestation` determinism invariant forbids a host-side marker).
#[derive(Component)]
pub(crate) struct InfestationLump {
    host: Entity,
}

fn build_lump_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(LUMP_R));
    // A sickly, faintly glowing swelling — greenish infection pressing up under the skin.
    let mat = mats.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.42, 0.12),
        emissive: LinearRgba::rgb(0.10, 0.14, 0.02),
        perceptual_roughness: 0.7,
        ..default()
    });
    commands.insert_resource(LumpAssets { mesh, mat });
}

/// Windowed-only: show/hide/twitch a subdermal lump on each host WHILE it gestates, so the chestburster is
/// a dreaded, readable payoff — the player could have seen it coming and intervened — instead of a random
/// gotcha. Never registered in the headless core (see [`lib::run`]), so no cosmetic child ever spawns there.
/// Spawns a lump on a newly-gestating host, twitches the existing ones, and despawns a lump once its host
/// stops gestating (erupted-and-survived, or cured). A host that DIED takes its child lump with it (recursive
/// despawn), so this only despawns lumps whose host is still alive but no longer carrying.
pub(crate) fn drive_infestation_tell(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<LumpAssets>>,
    hosts: Query<(Entity, &Infestation)>,
    mut lumps: Query<(Entity, &InfestationLump, &mut Transform)>,
) {
    let Some(assets) = assets else {
        return;
    };
    // A subtle asymmetric twitch (not a clean sine — a faint shudder).
    let t = time.elapsed_secs();
    let pulse = 1.0 + 0.18 * (t * 5.5).sin() + 0.06 * (t * 13.0).sin();

    let mut lumped: HashSet<Entity> = HashSet::new();
    for (lump_e, lump, mut tf) in &mut lumps {
        match hosts.get(lump.host) {
            Ok((_, inf)) if inf.active && !inf.is_erupting() => {
                lumped.insert(lump.host);
                tf.scale = Vec3::splat(pulse);
            }
            // Host still alive but no longer gestating (survived the burst / cured) → remove the tell.
            Ok(_) => {
                commands.entity(lump_e).despawn();
            }
            // Host gone — its child lump despawned with it; nothing to do.
            Err(_) => {}
        }
    }
    for (host_e, inf) in &hosts {
        if inf.active && !inf.is_erupting() && !lumped.contains(&host_e) {
            commands.entity(host_e).with_child((
                InfestationLump { host: host_e },
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(assets.mat.clone()),
                Transform::from_translation(LUMP_LOCAL),
                Visibility::Inherited,
            ));
        }
    }
}

/// Seed the initial free mancae into far, spread-apart floor cells (deterministic scan order, like
/// `crab::spawn_crabs`). Reuses the crab's shared `SurfaceGraph`.
fn spawn_mancae(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    graph: Option<Res<SurfaceGraph>>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut seq: ResMut<MancaSpawnSeq>,
    sim: Res<SimTuning>,
    // Placed furniture (spawned in Startup) — its cells are prime harborages: mancae hide under/beside it.
    furniture: Query<&Transform, With<PlacedIn>>,
) {
    let Some(graph) = graph else {
        warn!("parasite: SurfaceGraph missing at PostStartup — no mancae spawned");
        return;
    };

    let collider = meshes.add(Sphere::new(MANCA_COLLIDER_R));
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(SCP150_GLB));
    // Keep the shared handles so `parasite_burst` can birth broods at runtime (always inserted, even if
    // no initial mancae seed, so a burst can still spawn).
    commands.insert_resource(MancaAssets { collider: collider.clone(), scene: scene.clone() });

    let count = sim.parasite.initial_count;
    if count == 0 {
        info!("parasite: initial_count 0 — no mancae seeded");
        return;
    }

    // Furniture footprint cells as a membership SET, so the (unstable) query iteration order can't affect
    // the result — the harborage scan below stays deterministic.
    let furniture_cells: HashSet<IVec2> =
        furniture.iter().map(|t| dungeon.world_to_cell(t.translation)).collect();
    let card = [IVec2::new(1, 0), IVec2::new(-1, 0), IVec2::new(0, 1), IVec2::new(0, -1)];

    // A floor cell's harborage quality: how many orthogonal sides are walls (≥2 ⇒ a real 90° corner) plus a
    // strong bonus if furniture sits on it or an orthogonal neighbour. Thigmotaxis + harborage/shelter
    // selection (Devigne, Deneubourg & Broly 2013, DOI 10.1007/s00040-013-0313-7).
    let harborage_score = |cell: IVec2| -> i32 {
        let walls = [N, E, S, W].iter().filter(|&&d| dungeon.walled(cell, d)).count() as i32;
        let near_furniture = furniture_cells.contains(&cell)
            || card.iter().any(|o| furniture_cells.contains(&(cell + *o)));
        walls + if near_furniture { 3 } else { 0 }
    };

    // Score every eligible floor cell (deterministic row-major scan), keeping only real harborages far
    // enough from the squad spawn.
    let mut candidates: Vec<(i32, IVec2)> = Vec::new();
    for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }
            if (cell - dungeon.spawn).as_vec2().length() < MANCA_MIN_SPAWN_DIST {
                continue;
            }
            let score = harborage_score(cell);
            if score >= HARBORAGE_MIN_SCORE {
                candidates.push((score, cell));
            }
        }
    }
    if candidates.is_empty() {
        warn!("parasite: no harborage (corner/furniture) cell far enough from spawn to seed a huddle");
        return;
    }
    // Best harborages first; `sort_by` is STABLE, so equal-score cells keep their row-major order — the
    // whole ranking is a pure function of the (fixed) dungeon + furniture layout.
    candidates.sort_by(|a, b| b.0.cmp(&a.0));

    // Greedily choose huddle sites spread ≥ HARBORAGE_SEP apart so distinct huddles occupy distinct corners.
    let n_huddles = count.div_ceil(HUDDLE_SIZE).max(1);
    let mut sites: Vec<IVec2> = Vec::new();
    for (_score, cell) in &candidates {
        if sites.iter().any(|s| (*s - *cell).as_vec2().length() < HARBORAGE_SEP) {
            continue;
        }
        sites.push(*cell);
        if sites.len() >= n_huddles {
            break;
        }
    }

    // Distribute the swarm across the chosen sites (as evenly as possible), seeding each huddle as a dense
    // blob over the floor cells within HUDDLE_RADIUS of its site, all anchored (home) to the site centre.
    let n_sites = sites.len();
    let mut spawned = 0usize;
    for (i, site) in sites.iter().enumerate() {
        // Even split, remainder handed to the first `count % n_sites` sites (deterministic).
        let per_site = count / n_sites + usize::from(i < count % n_sites);
        if per_site == 0 {
            continue;
        }
        // Nearby floor cells to spread this huddle over (deterministic scan; the site itself always in).
        let mut cells: Vec<IVec2> = Vec::new();
        let r = HUDDLE_RADIUS.ceil() as i32;
        for dy in -r..=r {
            for dx in -r..=r {
                let c = *site + IVec2::new(dx, dy);
                if dungeon.is_floor(c)
                    && (c - *site).as_vec2().length() <= HUDDLE_RADIUS
                    && (c - dungeon.spawn).as_vec2().length() >= MANCA_MIN_SPAWN_DIST
                {
                    cells.push(c);
                }
            }
        }
        if cells.is_empty() {
            cells.push(*site);
        }
        let home = graph
            .floor_patch_cell(*site)
            .map(|p| graph.patch(p).center)
            .unwrap_or_else(|| dungeon.cell_center(*site));
        for k in 0..per_site {
            let cell = cells[k % cells.len()];
            let Some(patch) = graph.floor_patch_cell(cell) else { continue };
            let s = seq.0 as u32;
            seq.0 += 1;
            spawn_manca_on_patch(&mut commands, &graph, patch, &collider, &scene, s, &sim.parasite, home, 0.0, None);
            spawned += 1;
        }
    }
    info!("parasite: seeded {spawned} SCP-150 mancae across {n_sites} huddle(s)");
}

/// Spawn one manca seated on `patch`: an unscaled root (invisible collider + `Hostile` + `Health`) with
/// the scaled, `−X`-corrected glTF model as a child. Public so a later burst phase can birth a brood.
pub fn spawn_manca_on_patch(
    commands: &mut Commands,
    graph: &SurfaceGraph,
    patch: u32,
    collider: &Handle<Mesh>,
    scene: &Handle<WorldAsset>,
    rand_seed: u32,
    tuning: &crate::sim::ParasiteTuning,
    home: Vec3,
    embed_cd: f32,
    // `Some((host, chest_world_pos))` if this manca is erupting from a host chest — it spawns AT the chest in
    // the [`LeapPhase::Emerging`] climb-out; `None` for a normal surface-seated spawn.
    emerging: Option<(Entity, Vec3)>,
) {
    let p = graph.patch(patch);
    // Small deterministic in-patch jitter from the spawn seed so huddle-mates seeded onto the same cell
    // don't stack on one exact point (short-range separation only acts on a nonzero offset). Seed-derived,
    // so it is reproducible across same-seed runs.
    let jx = hash01_u32(rand_seed.wrapping_mul(0x9E37_79B1)) - 0.5;
    let jz = hash01_u32(rand_seed.wrapping_mul(0x85EB_CA77).wrapping_add(1)) - 0.5;
    let tan_v = p.normal.cross(p.tan_u);
    let pos = clamp_to_patch(p.center + (p.tan_u * jx + tan_v * jz) * MANCA_SPAWN_JITTER, p);
    let normal = p.normal;
    let heading = p.tan_u;
    let seat = pos + normal * MANCA_BODY_CENTER;
    // A brood erupting from a host spawns AT the chest in the slow BurrowOut climb-out (owned by
    // `manca_leap`'s Emerging arm, which follows the host, then drops it to the floor); a normal manca spawns
    // seated on the surface, Ready.
    let (init_pos, phase, phase_timer, host, anim) = match emerging {
        Some((h, chest)) => (chest, LeapPhase::Emerging, EMERGE_SECS, Some(h), MancaAnimState::BurrowOut),
        None => (seat, LeapPhase::Ready, 0.0, None, MancaAnimState::Snug),
    };

    let mut ec = commands.spawn((
        Manca,
        Hostile,
        Health::new(tuning.manca_hp),
        NoHealthBar, // lone stalkers, but no floating bar — they read as ambient dread, not a HP puzzle
        // A slop entity: same faction as the watcher boss, so the squad's anomaly-fear machinery applies.
        crate::ai::faction::Faction::Anomaly,
        MancaMotion { patch, pos, normal, heading, home },
        MancaLeap { phase, timer: phase_timer, cooldown: tuning.leap_cooldown, from: Vec3::ZERO, to: Vec3::ZERO, host },
        anim,
        MancaSeed(rand_seed),
        // Sphere collider mesh paired with its CPU laser hit-volume (same radius, zero-height capsule) so
        // bolts test against the manca headlessly + deterministically.
        (
            Mesh3d(collider.clone()),
            crate::laser::LaserTarget { radius: MANCA_COLLIDER_R, half_height: 0.0 },
        ),
        // Render-only: smooth the manca's 60 Hz movement + surface rotation across the display refresh.
        (
            Transform::from_translation(init_pos).with_rotation(surface_orientation(heading, normal)),
            avian3d::prelude::TransformInterpolation,
        ),
        Visibility::Inherited,
    ));
    // Photophobic + Dormant mood: a fresh manca spawns huddled and passive, steering toward shadow (the
    // light nudge is wired in `manca_huddle`, which reads this marker). Both inserted here at spawn so they
    // never churn the hashed archetype at runtime.
    ec.insert((
        Photophobic,
        MancaMood { state: MoodState::Dormant, calm_timer: 0.0, embed_cd, commit: 0.0, flash: 0.0 },
    ));
    // The scaled glTF body, `−X`→`−Z` corrected, seated so its body rests on the surface.
    ec.with_child((
        WorldAssetRoot(scene.clone()),
        Transform::from_translation(Vec3::Y * MANCA_MODEL_Y)
            .with_rotation(Quat::from_rotation_y(MODEL_FACING))
            .with_scale(Vec3::splat(MANCA_RENDER_SCALE)),
    ));
}

/// World-space chest point of a host — where the wound opens and a manca erupts. Units are the tall figurine
/// (root scale 2.6), crabs are small, so they use different host-local anchors; `transform_point` applies the
/// host's scale + rotation + translation.
fn host_chest(tf: &Transform, is_unit: bool) -> Vec3 {
    tf.transform_point(if is_unit { CHEST_LOCAL_UNIT } else { CHEST_LOCAL_CRAB })
}

/// Move the roused mancae as ONE readable swarm toward the nearest fresh host. Unlike a lone stalker, each
/// roused manca blends a WEAK seek toward the nearest fresh host with cohesion, separation, and a
/// commitment-scaled ALIGNMENT over its roused neighbours — so the cluster reaches a consensus heading and
/// advances as a mass (Reynolds 1987 alignment, the third boids rule; the alignment + weak-target-attraction
/// → collective-heading mechanism of Couzin et al. 2002). A per-manca commitment scalar ramps *continuously*
/// from an agitated MILL (slow, loosely aligned) to a committed CHARGE (fast, strongly polarized) — the
/// speed→polarization relationship of Gautrais et al. 2012 — and is nudged toward the neighbour average so
/// the swarm commits together. On rousing, a brief flash-expansion pop bursts each manca outward before the
/// mill re-forms. Mid-leap mancae are owned by `manca_leap` and skipped; leap wind-up still triggers from
/// `manca_leap` once a manca is delivered into range.
///
/// **Determinism.** Roused-neighbour positions/headings/commitments are read from a spatial-hash SNAPSHOT
/// taken (and position-bit sorted) before any manca moves, so every float sum runs in canonical order
/// regardless of ECS iteration — the trap `deterministic_core_is_bit_identical_across_many_builds` guards.
fn manca_hunt(
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    hosts: Query<(&Transform, &Infestation), (With<Parasitizable>, Without<Manca>)>,
    mut mancae: Query<
        (&mut MancaMotion, &mut MancaAnimState, &MancaLeap, &mut MancaMood, &mut Transform),
        With<Manca>,
    >,
    // Reused per-cell spatial hash of ROUSED boids (cleared in place), mirroring `manca_huddle`'s dormant one.
    mut hash: Local<HashMap<IVec2, Vec<SwarmNeighbor>>>,
) {
    let Some(graph) = graph else { return };
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    // Per-host: foot position + planar forward (local −Z) — for the blind-side stalk gate. Already-infested
    // hosts are skipped, so the swarm seeks a FRESH host — the infestation spreads through the group rather
    // than piling onto a doomed host.
    let host_data: Vec<(Vec3, Vec3)> = hosts
        .iter()
        .filter(|(_, inf)| !inf.active)
        .map(|(t, _)| {
            let fwd = (t.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
            (t.translation, fwd)
        })
        .collect();

    // Snapshot roused, grounded mancae into the hash (cleared in place), then sort each bucket by position
    // bits so the cohesion/alignment/commitment SUMS below are canonical (see `accumulate_boids`).
    for v in hash.values_mut() {
        v.clear();
    }
    for (motion, _, leap, mood, _) in &mancae {
        if mood.state == MoodState::Roused && leap.phase == LeapPhase::Ready {
            hash.entry(dungeon.world_to_cell(motion.pos)).or_default().push(SwarmNeighbor {
                pos: motion.pos,
                heading: motion.heading,
                commit: mood.commit,
            });
        }
    }
    for v in hash.values_mut() {
        v.sort_unstable_by_key(|nb| (nb.pos.x.to_bits(), nb.pos.y.to_bits(), nb.pos.z.to_bits()));
    }

    for (mut motion, mut anim, leap, mut mood, mut transform) in &mut mancae {
        // Only roused, grounded mancae swarm; dormant → huddle, mid-leap → leap.
        if mood.state != MoodState::Roused || leap.phase != LeapPhase::Ready {
            continue;
        }

        // Nearest fresh host (shared deterministic ranking; forward vector carried as the payload).
        let nearest = nearest_planar(motion.pos, host_data.iter().map(|&(hp, fwd)| (fwd, hp)));
        let cell = dungeon.world_to_cell(motion.pos);
        let acc = accumulate_boids(&hash, cell, motion.pos, HUDDLE_SEP_RADIUS);

        // --- Commitment ramp (continuous mill→charge) ------------------------------------------------
        // Ramp up while a fresh host is in play, decay when none remain; then nudge toward the roused-
        // neighbour average so commitment spreads through the cluster (a readable wave), not manca-by-manca.
        let ramp = if nearest.is_some() { COMMIT_RAMP } else { -COMMIT_DECAY };
        let mut commit = mood.commit + ramp * dt;
        if acc.n > 0.0 {
            let avg = acc.commit_sum / acc.n;
            commit += (avg - commit) * (COMMIT_SPREAD * dt).min(1.0);
        }
        commit = commit.clamp(0.0, 1.0);
        mood.commit = commit;

        // Flash-expansion pop: while it lasts, over-space and burst outward from the local centroid.
        let flash_k = if mood.flash > 0.0 { (mood.flash / FLASH_SECS).clamp(0.0, 1.0) } else { 0.0 };
        mood.flash = (mood.flash - dt).max(0.0);

        // --- Swarm steering: weak seek + cohesion + commit-scaled alignment; separation on the push ----
        let seek = if let Some((hfwd, hpos, _d)) = nearest {
            let to_host = (hpos - motion.pos).with_y(0.0);
            let planar_d = to_host.length();
            // Blind-side stalk: if the host is close and looking at this manca, arc tangentially toward its
            // rear rather than charging head-on, until the manca clears the facing cone.
            let dir = if planar_d > LEAP_MIN
                && planar_d < STALK_BAND
                && unit_is_facing(hpos, hfwd, motion.pos, BLIND_COS)
            {
                let bearing = to_host.normalize_or_zero();
                let tang = Vec3::new(-bearing.z, 0.0, bearing.x); // perpendicular, ground plane
                let sign = if tang.dot(hfwd) >= 0.0 { 1.0 } else { -1.0 }; // toward the host's rear
                (bearing + tang * sign * STALK_STRENGTH).normalize_or_zero()
            } else {
                to_host.normalize_or_zero()
            };
            dir * SEEK_STRENGTH
        } else {
            Vec3::ZERO
        };
        let cohesion = if acc.n > 0.0 {
            (acc.centroid / acc.n - motion.pos).with_y(0.0) * COHESION_STRENGTH
        } else {
            Vec3::ZERO
        };
        // Alignment scaled by commitment: a mill is loosely aligned (`ALIGN_FLOOR`), a charge is a tight
        // polarized arrow (`ALIGN_STRENGTH`). This is the readable blob→arrow transition.
        let align_gain = ALIGN_FLOOR + (ALIGN_STRENGTH - ALIGN_FLOOR) * commit;
        let align = acc.heading_sum.with_y(0.0).normalize_or_zero() * align_gain;
        let desired = seek + cohesion + align;

        // Separation (un-clamped push), boosted during the flash pop, plus a brief radial burst outward.
        let mut push = acc.sep * HUDDLE_SEP_STRENGTH * (1.0 + FLASH_SEP_BOOST * flash_k);
        if flash_k > 0.0 && acc.n > 0.0 {
            let outward = (motion.pos - acc.centroid / acc.n).with_y(0.0).normalize_or_zero();
            push += outward * FLASH_IMPULSE * flash_k;
        }

        // Climb speed on a wall patch, crawl speed on the floor (the evolved base) — modulated by commitment
        // from a mill creep up to a charge.
        let base = if graph.patch(motion.patch).normal.y < WALL_NORMAL_Y {
            sim.parasite.climb_speed
        } else {
            sim.parasite.crawl_speed
        };
        let speed = base * (MILL_SPEED_FACTOR + (CHARGE_SPEED_FACTOR - MILL_SPEED_FACTOR) * commit);
        let moving = steer_surface(&mut motion, &graph, &dungeon, desired, speed, push, dt);

        // Choose the animation state from motion + which surface we're on.
        let on_wall = graph.patch(motion.patch).normal.y < WALL_NORMAL_Y;
        *anim = if !moving {
            MancaAnimState::Idle
        } else if on_wall {
            MancaAnimState::Climb
        } else {
            MancaAnimState::Walk
        };

        // Seat & orient flat to the current surface.
        transform.translation = motion.pos + motion.normal * MANCA_BODY_CENTER;
        transform.rotation = surface_orientation(motion.heading, motion.normal);
    }
}

/// Steer a manca one step toward `desired` along its current surface, picking the graph neighbour whose
/// gate best matches the travel direction, committing a patch transfer on reaching a gate, and easing the
/// surface normal + heading. `swarm` is an extra world-space push (huddle cohesion/separation) folded in on
/// top of the base travel, exactly as `crab::steer_surface` folds Reynolds separation; the roused hunt
/// passes `Vec3::ZERO` (a lone stalker). Returns whether it actually moved.
fn steer_surface(
    motion: &mut MancaMotion,
    graph: &SurfaceGraph,
    dungeon: &Dungeon,
    desired: Vec3,
    speed: f32,
    swarm: Vec3,
    dt: f32,
) -> bool {
    let _ = dungeon; // reserved for future light/threat sampling; kept for signature parity with the crab

    // The huddle/swarm push (cohesion/separation; zero for the lone-stalker hunt), projected onto the
    // current surface. The shared core adds it as-is (no re-scale), so `project_tangent(swarm, n)` stays
    // bit-identical to the old copy. `steer_to_override = None` ⇒ the core runs the neighbour-gate scan.
    let n = graph.patch(motion.patch).normal;
    let push = project_tangent(swarm, n);
    crate::surface_nav::steer_surface_core(
        &mut motion.pos,
        &mut motion.patch,
        &mut motion.normal,
        &mut motion.heading,
        graph,
        desired,
        push,
        speed,
        dt,
        None,
    )
}

/// A neighbour snapshot for the boid scans: surface position, travel heading, and commitment. Both the
/// dormant huddle and the roused swarm hash these per floor cell so their Reynolds sums (cohesion,
/// separation, alignment, and — for the swarm — the commitment spread) run over a canonical,
/// position-bit-sorted order rather than ECS iteration order (the determinism trap; see `manca_huddle`).
#[derive(Clone, Copy)]
struct SwarmNeighbor {
    pos: Vec3,
    heading: Vec3,
    commit: f32,
}

/// The Reynolds neighbour terms accumulated over one 3×3 cell block (see `accumulate_boids`).
struct BoidAccum {
    /// Sum of neighbour positions (÷ `n` = local centroid, for cohesion).
    centroid: Vec3,
    /// Short-range separation push (away from neighbours within `sep_radius`).
    sep: Vec3,
    /// Sum of neighbour headings (normalize = the local consensus heading, for alignment).
    heading_sum: Vec3,
    /// Sum of neighbour commitments (÷ `n` = local average, for the commitment spread).
    commit_sum: f32,
    /// Neighbour count (excludes the self-hit).
    n: f32,
}

/// Accumulate the Reynolds neighbour terms over the 3×3 cell block around `cell`, from the per-cell `hash`.
/// Skips the self-hit (`d ≤ 1e-4`). **Determinism:** the caller MUST have sorted every bucket by position
/// bits first, so these non-associative float sums are bit-identical across App instances — the trap that
/// only `deterministic_core_is_bit_identical_across_many_builds` catches.
fn accumulate_boids(
    hash: &HashMap<IVec2, Vec<SwarmNeighbor>>,
    cell: IVec2,
    me: Vec3,
    sep_radius: f32,
) -> BoidAccum {
    let mut acc = BoidAccum {
        centroid: Vec3::ZERO,
        sep: Vec3::ZERO,
        heading_sum: Vec3::ZERO,
        commit_sum: 0.0,
        n: 0.0,
    };
    for gy in -1..=1 {
        for gx in -1..=1 {
            if let Some(others) = hash.get(&(cell + IVec2::new(gx, gy))) {
                for nb in others {
                    let away = me - nb.pos;
                    let d = away.length();
                    if d <= 1.0e-4 {
                        continue; // self
                    }
                    acc.centroid += nb.pos;
                    acc.heading_sum += nb.heading;
                    acc.commit_sum += nb.commit;
                    acc.n += 1.0;
                    if d < sep_radius {
                        acc.sep += away / d * (sep_radius - d);
                    }
                }
            }
        }
    }
    acc
}

/// Rouse/settle: flip a manca between [`MoodState::Dormant`] (huddled, passive) and [`MoodState::Roused`]
/// (hunting). A dormant manca wakes when the squad's gunfire field is hot at its cell (a bolt landed near
/// the huddle — laser impacts deposit `THREAT_GUN` at the strike point, so a hit on ANY member floods its
/// neighbours), when a fresh host steps within `ROUSE_PROXIMITY`, or when an already-roused sibling is
/// within `ROUSE_CONTAGION_R` — the excitation spreads through the cluster (Broly & Deneubourg 2015,
/// DOI 10.1371/journal.pcbi.1004290). A roused manca with no disturbance for `ROUSE_CALM_SECONDS`
/// re-settles to Dormant (then creeps back to its harborage in `manca_huddle`).
///
/// **Determinism.** Fresh-host positions and the roused-manca set are SNAPSHOTTED before any mood flips, so
/// the contagion advances one ring per tick from last tick's roused set (a cellular-automaton sweep),
/// independent of ECS iteration order. `dt` is the fixed sub-step on the pinned `FixedUpdate` path.
fn manca_rouse(
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    stig: Res<Stig>,
    hosts: Query<(&Transform, &Infestation), (With<Parasitizable>, Without<Manca>)>,
    mut mancae: Query<(&MancaMotion, Ref<Health>, &mut MancaMood), With<Manca>>,
) {
    let dt = time.delta_secs();

    // Snapshot fresh (un-infested) host foot positions — a footfall on the huddle wakes it.
    let host_pos: Vec<Vec3> =
        hosts.iter().filter(|(_, inf)| !inf.active).map(|(t, _)| t.translation).collect();
    // Snapshot currently-roused manca positions (read before any flip this tick) for the contagion.
    let roused: Vec<Vec3> =
        mancae.iter().filter(|(_, _, m)| m.state == MoodState::Roused).map(|(mo, _, _)| mo.pos).collect();

    let prox_sq = ROUSE_PROXIMITY * ROUSE_PROXIMITY;
    let contagion_sq = ROUSE_CONTAGION_R * ROUSE_CONTAGION_R;

    for (motion, health, mut mood) in &mut mancae {
        // Count down the post-eruption embed cooldown (a fresh brood can't immediately re-infest a host).
        if mood.embed_cd > 0.0 {
            mood.embed_cd -= dt;
        }
        // Being shot is the most direct disturbance: a manca's Health only ever changes when a bolt hits it,
        // so `Changed` (minus the spawn-add tick) is a bulletproof "I was just shot" trigger — independent of
        // the gunfire field's magnitude or drain timing. This is the core fix for "won't react even when shot".
        let shot = health.is_changed() && !health.is_added();
        let gunfire = stig.sample(FieldId::THREAT_GUN, &dungeon, motion.pos) > ROUSE_THREAT;
        let host_near =
            host_pos.iter().any(|h| (h.xz() - motion.pos.xz()).length_squared() < prox_sq);
        // A roused manca appears in `roused` at its own position (distance 0); the `> 1e-8` guard skips that
        // self-hit so a lone roused manca can still calm down, while distinct siblings — kept ≥ the huddle
        // separation apart — are never that close.
        let contagion = roused.iter().any(|r| {
            let d2 = (r.xz() - motion.pos.xz()).length_squared();
            d2 > 1.0e-8 && d2 < contagion_sq
        });
        let disturbed = shot || gunfire || host_near || contagion;

        match mood.state {
            MoodState::Dormant => {
                if disturbed {
                    mood.state = MoodState::Roused;
                    mood.calm_timer = ROUSE_CALM_SECONDS;
                    // Stamp the flash-expansion pop. Because arousal contagion flips one ring per tick, the
                    // burst sweeps the clump as an expanding ripple — the readable "the patch just woke up".
                    mood.flash = FLASH_SECS;
                }
            }
            MoodState::Roused => {
                if disturbed {
                    mood.calm_timer = ROUSE_CALM_SECONDS;
                } else {
                    mood.calm_timer -= dt;
                    if mood.calm_timer <= 0.0 {
                        mood.state = MoodState::Dormant;
                    }
                }
            }
        }
    }
}

/// Dormant mancae huddle: each creeps toward its harborage anchor, is drawn to its dormant neighbours
/// (cohesion) while packing shoulder-to-shoulder (short-range separation), and steers down the light
/// gradient into shadow — a dense, quivering cluster in a dark corner or under furniture (the "patch of
/// daddy long legs"). Roused or mid-leap mancae are owned by `manca_hunt`/`manca_leap` and skipped.
/// Aggregation follows the self-organized isopod model (Devigne, Broly & Deneubourg 2011,
/// DOI 10.1371/journal.pone.0017389; Broly et al. 2015, DOI 10.1007/s10071-015-0925-6) via Reynolds
/// cohesion + separation (Olfati-Saber 2006), and finally wires the once-inert `Photophobic` light nudge.
///
/// **Determinism.** Neighbour positions are read from a spatial-hash SNAPSHOT taken before any manca moves
/// (mirrors `crab_locomotion`), so the huddle step is independent of ECS iteration order. On `FixedUpdate`,
/// ordered `.after(light::LightFieldWritten)` so the gradient is the one baked this tick.
fn manca_huddle(
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    light_field: Res<LightField>,
    config: Res<GameConfig>,
    mut mancae: Query<
        (&mut MancaMotion, &mut MancaAnimState, &MancaLeap, &MancaMood, &mut Transform),
        With<Manca>,
    >,
    // Reused per-cell spatial hash of dormant boids (cleared in place, like `crab_locomotion`).
    mut hash: Local<HashMap<IVec2, Vec<SwarmNeighbor>>>,
) {
    let Some(graph) = graph else { return };
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    // Snapshot dormant-manca positions into the hash (cleared in place). CRITICAL determinism step: each
    // bucket is then sorted by position bits so the cohesion/separation float SUMS below run in a canonical
    // order, NOT ECS entity-iteration order (which is not stable across App instances — see
    // `deterministic_core_is_bit_identical_across_many_builds`). Non-associative float addition over the
    // neighbours would otherwise make the pinned hash diverge across builds.
    for v in hash.values_mut() {
        v.clear();
    }
    for (motion, _, leap, mood, _) in &mancae {
        if mood.state == MoodState::Dormant && leap.phase == LeapPhase::Ready {
            hash.entry(dungeon.world_to_cell(motion.pos)).or_default().push(SwarmNeighbor {
                pos: motion.pos,
                heading: motion.heading,
                commit: 0.0,
            });
        }
    }
    for v in hash.values_mut() {
        v.sort_unstable_by_key(|nb| (nb.pos.x.to_bits(), nb.pos.y.to_bits(), nb.pos.z.to_bits()));
    }

    let signed_gain = -config.lighting.photophobic_gain;

    for (mut motion, mut anim, leap, mood, mut transform) in &mut mancae {
        // Only grounded, dormant mancae huddle (roused → hunt; mid-leap → leap).
        if mood.state != MoodState::Dormant || leap.phase != LeapPhase::Ready {
            continue;
        }

        // Cohesion toward the local dormant centroid, short-range separation, and a FAINT alignment — the
        // same 3×3 per-cell scan the crab uses, now also summing neighbour headings (`accumulate_boids`).
        // The alignment is deliberately faint here (`ALIGN_FLOOR`): a resting patch reads as subtly coherent,
        // not a directed march — that polarization is the roused swarm's job (`manca_hunt`).
        let cell = dungeon.world_to_cell(motion.pos);
        let acc = accumulate_boids(&hash, cell, motion.pos, HUDDLE_SEP_RADIUS);
        let cohesion = if acc.n > 0.0 {
            (acc.centroid / acc.n - motion.pos).with_y(0.0) * COHESION_STRENGTH
        } else {
            Vec3::ZERO
        };
        let align = acc.heading_sum.with_y(0.0).normalize_or_zero() * ALIGN_FLOOR;

        // Pull back toward the harborage anchor (pins the clump to its corner/furniture), capped to 1 unit.
        let to_home = (motion.home - motion.pos).with_y(0.0);
        let home_pull = to_home.normalize_or_zero() * HARBORAGE_BIAS * to_home.length().min(1.0);

        // Descend the illuminance gradient into shadow (the once-inert `Photophobic` marker, now live).
        let light = light_push(&light_field, &dungeon, motion.pos, signed_gain);

        let desired = home_pull + cohesion + align + light;
        let _ = steer_surface(
            &mut motion,
            &graph,
            &dungeon,
            desired,
            SETTLE_SPEED,
            acc.sep * HUDDLE_SEP_STRENGTH,
            dt,
        );

        // Anim: settled → the snug idle; only a manca still traveling back to its harborage walks/climbs.
        let on_wall = graph.patch(motion.patch).normal.y < WALL_NORMAL_Y;
        *anim = if to_home.length() <= SETTLE_ARRIVE {
            MancaAnimState::Snug
        } else if on_wall {
            MancaAnimState::Climb
        } else {
            MancaAnimState::Walk
        };

        transform.translation = motion.pos + motion.normal * MANCA_BODY_CENTER;
        transform.rotation = surface_orientation(motion.heading, motion.normal);
    }
}

/// Ballistic leap onto a host — hunker, then arc over, mirroring `crab::crab_jump`. While hunkering /
/// airborne this owns the manca's transform (`manca_hunt` skips it). Phase 1 lands and re-arms the
/// cooldown; a later phase hooks the *embed* onto the landing.
fn manca_leap(
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    // `Option<&Unit>` distinguishes the tall figurine host from a crab so an Emerging manca climbs out of the
    // right chest anchor (see `host_chest`).
    hosts: Query<
        (&Transform, &Infestation, Option<&crate::squad::Unit>),
        (With<Parasitizable>, Without<Manca>),
    >,
    mut mancae: Query<
        (&mut MancaMotion, &mut MancaAnimState, &mut MancaLeap, &MancaMood, &mut Transform),
        With<Manca>,
    >,
) {
    let Some(graph) = graph else { return };
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    // Host positions + planar forwards (for the blind-side gate at launch); skip already-infested hosts.
    let host_data: Vec<(Vec3, Vec3)> = hosts
        .iter()
        .filter(|(_, inf, _)| !inf.active)
        .map(|(t, _, _)| {
            let fwd = (t.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
            (t.translation, fwd)
        })
        .collect();
    let nearest = |from: Vec3| nearest_planar(from, host_data.iter().map(|&(hp, fwd)| (fwd, hp)));

    for (mut motion, mut anim, mut leap, mood, mut tf) in &mut mancae {
        match leap.phase {
            LeapPhase::Ready => {
                // A dormant manca never commits a leap — it is passive until roused. A leap already in
                // flight (Hunker/Air below) always finishes, so a manca roused mid-approach still lands.
                if mood.state != MoodState::Roused {
                    continue;
                }
                leap.cooldown = (leap.cooldown - dt).max(0.0);
                if leap.cooldown > 0.0 {
                    continue;
                }
                if let Some((hfwd, hpos, d)) = nearest(motion.pos) {
                    // Blind-side gate: only commit the leap from outside the host's facing cone.
                    let in_blind_spot = !unit_is_facing(hpos, hfwd, motion.pos, BLIND_COS);
                    if d > LEAP_MIN && d < sim.parasite.leap_len && in_blind_spot {
                        leap.phase = LeapPhase::Hunker;
                        leap.timer = LEAP_HUNKER;
                        leap.from = motion.pos;
                        leap.to = hpos;
                    }
                }
            }
            LeapPhase::Hunker => {
                leap.timer -= dt;
                *anim = MancaAnimState::Attack;
                // Crouch: dip toward the surface during the wind-up.
                tf.translation = motion.pos + motion.normal * (MANCA_BODY_CENTER * 0.4);
                if leap.timer <= 0.0 {
                    if let Some((_, hpos, _)) = nearest(motion.pos) {
                        leap.to = hpos; // launch toward the host's CURRENT position
                    }
                    leap.from = motion.pos;
                    leap.phase = LeapPhase::Air;
                    leap.timer = LEAP_AIR;
                }
            }
            LeapPhase::Air => {
                leap.timer -= dt;
                let s = (1.0 - (leap.timer / LEAP_AIR)).clamp(0.0, 1.0);
                let ground = leap.from.lerp(leap.to, s);
                let height = LEAP_ARC * (std::f32::consts::PI * s).sin();
                motion.pos = ground;
                // Re-home onto the surface beneath the arc so it lands on a real patch.
                if let Some(fp) = graph.floor_patch_cell(dungeon.world_to_cell(ground)) {
                    motion.patch = fp;
                    motion.normal = graph.patch(fp).normal;
                }
                let dir = (leap.to - leap.from).with_y(0.0);
                if dir.length_squared() > 1.0e-6 {
                    motion.heading = dir.normalize_or(motion.heading);
                }
                tf.translation = ground + motion.normal * MANCA_BODY_CENTER + Vec3::Y * height;
                tf.rotation = surface_orientation(motion.heading, motion.normal);
                *anim = MancaAnimState::Attack;
                if leap.timer <= 0.0 {
                    // Land: clamp onto the patch and re-arm. (The embed is `manca_embed`, next in the chain.)
                    motion.pos = clamp_to_patch(motion.pos, graph.patch(motion.patch));
                    leap.phase = LeapPhase::Ready;
                    leap.cooldown = sim.parasite.leap_cooldown;
                }
            }
            LeapPhase::Emerging => {
                // Drag slowly OUT of the host's chest along its forward (−Z), following the host if it moves,
                // over EMERGE_SECS — then drop to the floor via a short Air arc. If the host has already died
                // (query miss), bail straight to the drop so the brood is never stranded.
                leap.timer -= dt;
                *anim = MancaAnimState::BurrowOut;
                let host_tf = leap.host.and_then(|h| hosts.get(h).ok());
                if let Some((htf, _inf, unit)) = host_tf {
                    let chest = host_chest(htf, unit.is_some());
                    let out = (htf.rotation * Vec3::NEG_Z).normalize_or(Vec3::NEG_Z);
                    let s = (1.0 - (leap.timer / EMERGE_SECS)).clamp(0.0, 1.0);
                    // Crawl outward + a small upward heave (peaks mid-climb) as it claws free of the wound.
                    let heave = Vec3::Y * (0.12 * (std::f32::consts::PI * s).sin());
                    tf.translation = chest + out * (EMERGE_DIST * s) + heave;
                    let flat = out.with_y(0.0).normalize_or(motion.heading);
                    motion.heading = flat;
                    tf.rotation = surface_orientation(flat, Vec3::Y);
                }
                if leap.timer <= 0.0 || host_tf.is_none() {
                    // Detach: fall to the floor patch (short Air arc), then normal seated life resumes.
                    leap.from = tf.translation;
                    leap.to = motion.pos;
                    leap.phase = LeapPhase::Air;
                    leap.timer = LEAP_AIR;
                    leap.host = None;
                }
            }
        }
    }
}

/// Burrow-in: a grounded manca touching a fresh host embeds — it flips the host's [`Infestation`] on,
/// bites it for `EMBED_DAMAGE`, and despawns (the free parasite is *inside* the host now). This is the
/// transition from "free crawler" to "gestating parasite"; the gestation clock then runs in
/// [`gestation_tick`] until the burst (Phase 3).
///
/// **Determinism.** Mancae are processed in `MancaSeed` order (stable, not ECS-iteration order), and a
/// host claimed this tick is added to `taken` so a second manca can't also embed into it — so which manca
/// claims which host, and the despawn order (which reuses freed entity ids), are reproducible across
/// same-seed runs. Host selection uses the shared `nearest_planar` geometric tie-break.
fn manca_embed(
    mut commands: Commands,
    sim: Res<SimTuning>,
    mancae: Query<(Entity, &MancaMotion, &MancaLeap, &MancaMood, &MancaSeed, &Health), With<Manca>>,
    mut hosts: Query<
        (Entity, &Transform, &mut Health, &mut Infestation),
        (With<Parasitizable>, Without<Manca>),
    >,
) {
    // Grounded, living, ROUSED mancae only (a dormant manca is passive; a mid-leap manca embeds on the tick
    // it lands and returns to `Ready`).
    let mut ready: Vec<(u32, Entity, Vec3)> = mancae
        .iter()
        .filter(|(_, _, leap, mood, _, hp)| {
            mood.state == MoodState::Roused
                && mood.embed_cd <= 0.0 // a freshly-erupted brood waits out its cooldown before re-infesting
                && leap.phase == LeapPhase::Ready
                && hp.current > 0.0
        })
        .map(|(e, motion, _, _, seed, _)| (seed.0, e, motion.pos))
        .collect();
    if ready.is_empty() {
        return;
    }
    ready.sort_unstable_by_key(|(seed, _, _)| *seed);

    // Snapshot fresh (un-infested) host positions once; `taken` guards against two mancae claiming one host.
    let fresh: Vec<(Entity, Vec3)> = hosts
        .iter()
        .filter(|(_, _, _, inf)| !inf.active)
        .map(|(e, t, _, _)| (e, t.translation))
        .collect();
    let reach_sq = (HOST_BODY_RADIUS + MANCA_EMBED_RANGE).powi(2);
    let mut taken: HashSet<Entity> = HashSet::new();

    for (seed, manca_e, mpos) in ready {
        let best = nearest_planar(
            mpos,
            fresh.iter().filter(|(e, _)| !taken.contains(e)).map(|&(e, p)| (e, p)),
        );
        let Some((host_e, hpos, _d)) = best else { continue };
        if (hpos.xz() - mpos.xz()).length_squared() > reach_sq {
            continue; // nearest fresh host is out of burrow-in reach
        }
        if let Ok((_, _, mut hp, mut inf)) = hosts.get_mut(host_e) {
            hp.current -= sim.parasite.embed_damage;
            inf.active = true;
            inf.timer = 0.0;
            inf.seed = seed; // the embedding manca's seed salts the brood RNG at burst
        }
        taken.insert(host_e);
        commands.entity(manca_e).despawn();
    }
}

/// Advance every active infestation's gestation clock. On `FixedUpdate`, so the timer is pinned state that
/// folds into the replay hash; the burst fires in Phase 3 once `timer >= GESTATION_SECONDS`.
fn gestation_tick(
    time: Res<Time>,
    sim: Res<SimTuning>,
    mut hosts: Query<&mut Infestation, With<Parasitizable>>,
) {
    let dt = time.delta_secs();
    for mut inf in &mut hosts {
        // Advance the gestation clock only while actively gestating and not already erupting.
        if !inf.active || inf.burst != BurstPhase::Idle {
            continue;
        }
        inf.timer += dt;
        if inf.timer >= sim.parasite.gestation_seconds {
            // Gestation complete — begin the slow eruption with the convulsing wind-up (driven by
            // `parasite_burst`), rather than bursting instantly.
            inf.burst = BurstPhase::Convulse;
            inf.burst_timer = BURST_CONVULSE_SECS;
        }
    }
}

/// Deterministic brood size for a host, from its infestation seed — clamped to `[min, max]`.
fn brood_size(seed: u32, min: u32, max: u32) -> u32 {
    let span = (max - min + 1) as f32;
    let n = min + (hash01_u32(seed.wrapping_mul(0x9E37_79B1).wrapping_add(21)) * span) as u32;
    n.min(max)
}

/// The eruption driver — advances every host whose burst is not `Idle` through Convulse → (Erupt) → Bleed.
/// At the Erupt moment (Convulse→Bleed) it deals ⅓ of the host's MAX HP (deliberately NOT an instakill — the
/// host survives, wounded, unless it was already below a third), tears a persistent [`Wound`] into its chest,
/// births the brood erupting from that chest (the slow [`LeapPhase::Emerging`] climb-out), and sprays a blood
/// gush + a camera kick. During Convulse + Bleed it drips blood at the wound. When Bleed ends the host is
/// released from infestation (`active = false`) — re-infestable, but the fresh brood's `embed_cd` stops it
/// re-parasitising the host it just erupted from.
///
/// **Determinism.** Hosts erupt in a stable geometric order (position bits), brood seeds come from the
/// monotonic [`MancaSpawnSeq`], and the ⅓-HP decrement lands on the pinned FixedUpdate tick — so which hosts
/// erupt, how many mancae, and the entity-id order are reproducible. The wound/blood/shake are cosmetic
/// (gore/juice on `Update`), invisible to `snapshot_hash`.
#[allow(clippy::type_complexity)]
fn parasite_burst(
    mut commands: Commands,
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    manca_assets: Option<Res<MancaAssets>>,
    wound_assets: Option<Res<WoundAssets>>,
    mut seq: ResMut<MancaSpawnSeq>,
    mut gore: ResMut<GoreQueue>,
    mut trauma: ResMut<crate::juice::Trauma>,
    live_mancae: Query<(), With<Manca>>,
    mut hosts: Query<
        (Entity, &Transform, &mut Health, &mut Infestation, Option<&crate::squad::Unit>),
        With<Parasitizable>,
    >,
) {
    let (Some(graph), Some(manca_assets), Some(wound_assets)) = (graph, manca_assets, wound_assets) else {
        return;
    };
    let p = &sim.parasite;
    let dt = time.delta_secs();

    // Erupting hosts in a stable geometric order (position bits) so the brood seq draw + population cap are
    // reproducible even when several hosts erupt the same tick.
    let mut order: Vec<((u32, u32, u32), Entity)> = hosts
        .iter()
        .filter(|(_, _, _, inf, _)| inf.burst != BurstPhase::Idle)
        .map(|(e, tf, _, _, _)| {
            let t = tf.translation;
            ((t.x.to_bits(), t.y.to_bits(), t.z.to_bits()), e)
        })
        .collect();
    if order.is_empty() {
        return;
    }
    order.sort_unstable_by_key(|(k, _)| *k);

    let mut live = live_mancae.iter().count();
    for (_key, host_e) in order {
        let Ok((_, htf, mut hp, mut inf, unit)) = hosts.get_mut(host_e) else { continue };
        let is_unit = unit.is_some();
        let chest = host_chest(htf, is_unit);
        let host_pos = htf.translation;

        match inf.burst {
            BurstPhase::Convulse => {
                let before = inf.burst_timer;
                inf.burst_timer -= dt;
                if drips(before, inf.burst_timer) {
                    push_bleed(&mut gore, chest); // blood wells under the skin as pressure builds
                }
                trauma.add(CONVULSE_TRAUMA_PER_TICK); // a rising, dreadful rumble
                if inf.burst_timer <= 0.0 {
                    // --- ERUPT: the manca tears out ---
                    hp.current -= hp.max / BURST_DAMAGE_DIVISOR; // ⅓ damage — deliberately NOT an instakill

                    // Tear a persistent hole into the chest: a dark disc parented to the host ROOT (a sibling
                    // of the figurine, so `recolor_units` leaves it alone), facing outward (−Z) and sat just
                    // proud of the body to avoid z-fighting. It rides + rotates with the host, despawns with it.
                    let wound_local = if is_unit { CHEST_LOCAL_UNIT } else { CHEST_LOCAL_CRAB };
                    commands.entity(host_e).with_child((
                        Wound,
                        Mesh3d(wound_assets.disc.clone()),
                        MeshMaterial3d(wound_assets.mat.clone()),
                        Transform::from_translation(wound_local + Vec3::NEG_Z * 0.03)
                            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
                        Visibility::Inherited,
                    ));

                    // Birth the brood, erupting from the chest in the slow climb-out, capped by the pop limit.
                    let brood = brood_size(inf.seed, p.brood_min, p.brood_max);
                    if let Some(patch) = graph.floor_patch_cell(dungeon.world_to_cell(host_pos)) {
                        let brood_home = graph.patch(patch).center;
                        for _ in 0..brood {
                            if live >= p.manca_count_max {
                                break;
                            }
                            let s = seq.0 as u32;
                            seq.0 += 1;
                            spawn_manca_on_patch(
                                &mut commands, &graph, patch, &manca_assets.collider,
                                &manca_assets.scene, s, p, brood_home, EMBED_COOLDOWN, Some((host_e, chest)),
                            );
                            live += 1;
                        }
                    }

                    push_gush(&mut gore, chest); // the wet burst of blood
                    // Flesh chunks burst out (cosmetic, non-economy — see `GoreKind::Viscera`).
                    gore.0.push(GoreEvent {
                        pos: chest,
                        kind: GoreKind::Viscera,
                        tint: Color::WHITE,
                        gib: None,
                        intensity: 0.0,
                    });
                    trauma.add(ERUPT_TRAUMA); // the camera kick
                    inf.burst = BurstPhase::Bleed;
                    inf.burst_timer = BURST_BLEED_SECS;
                }
            }
            BurstPhase::Bleed => {
                let before = inf.burst_timer;
                inf.burst_timer -= dt;
                if drips(before, inf.burst_timer) {
                    push_bleed(&mut gore, chest); // the wound keeps streaming
                }
                if inf.burst_timer <= 0.0 {
                    // Released — the wound stays, the host walks on, and it is re-infestable.
                    inf.active = false;
                    inf.burst = BurstPhase::Idle;
                }
            }
            BurstPhase::Idle => {}
        }
    }
}

/// True once per `BLEED_INTERVAL` as a burst timer counts down — a deterministic drip cadence so the wound
/// bleed doesn't spray a fresh billboard every single tick.
fn drips(before: f32, after: f32) -> bool {
    (before / BLEED_INTERVAL).floor() != (after / BLEED_INTERVAL).floor()
}

/// A single small blood spray at the wound (the sustained welling / dripping). `FleshHit` is never fog-gated,
/// spawns no chunks or shake, and its blood colour is built in (`tint` is ignored for non-gib kinds).
fn push_bleed(gore: &mut GoreQueue, pos: Vec3) {
    gore.0.push(GoreEvent { pos, kind: GoreKind::FleshHit, tint: Color::WHITE, gib: None, intensity: 0.0 });
}

/// The eruption gush — a fan of blood sprays around the wound at the instant the manca tears out.
fn push_gush(gore: &mut GoreQueue, chest: Vec3) {
    for off in [
        Vec3::ZERO,
        Vec3::Y * 0.12,
        Vec3::new(0.06, 0.05, -0.06),
        Vec3::new(-0.06, 0.03, -0.04),
        Vec3::new(0.0, -0.04, -0.08),
    ] {
        gore.0.push(GoreEvent {
            pos: chest + off,
            kind: GoreKind::FleshHit,
            tint: Color::WHITE,
            gib: None,
            intensity: 0.0,
        });
    }
}

/// The ONE system that removes a manca at ≤ 0 HP (a shot stalker) — the sole despawn+gore owner (mirrors
/// `crab_despawn_dead`). Emits deaths in stable `MancaSeed` order so the gore free-list reuse is
/// reproducible; a pale ichor splat, no field deposits (a manca death must not magnet the crab swarm).
fn manca_despawn_dead(
    mut commands: Commands,
    mut gore: ResMut<GoreQueue>,
    mancae: Query<(Entity, &Health, &Transform, &MancaSeed), With<Manca>>,
) {
    let mut dead: Vec<(u32, Entity, Vec3)> = mancae
        .iter()
        .filter(|(_, hp, _, _)| hp.current <= 0.0)
        .map(|(e, _, tf, seed)| (seed.0, e, tf.translation))
        .collect();
    if dead.is_empty() {
        return;
    }
    dead.sort_unstable_by_key(|(seed, _, _)| *seed);
    for (_, entity, pos) in dead {
        gore.0.push(GoreEvent {
            pos,
            kind: GoreKind::EnemySplat,
            tint: Color::srgb(0.85, 0.80, 0.70), // pale chitin ichor
            gib: None,
            intensity: 0.25,
        });
        commands.entity(entity).despawn();
    }
}

/// Wire a manca's asynchronously-spawned `AnimationPlayer` to the shared graph (mirrors
/// `crab::attach_crab_animation`). Skips players that don't belong to a manca.
fn attach_manca_animation(
    mut commands: Commands,
    anim: Option<Res<MancaAnim>>,
    added: Query<Entity, Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    mancae: Query<(), With<Manca>>,
) {
    let Some(anim) = anim else { return };
    for player in &added {
        // Walk up the hierarchy to find the owning manca, if any.
        let mut cur = player;
        let owner = loop {
            if mancae.get(cur).is_ok() {
                break Some(cur);
            }
            match parents.get(cur) {
                Ok(child_of) => cur = child_of.parent(),
                Err(_) => break None,
            }
        };
        let Some(owner) = owner else { continue };

        commands
            .entity(player)
            .insert((AnimationGraphHandle(anim.graph.clone()), AnimationTransitions::new()));
        commands.entity(owner).insert(MancaAnimPlayer { player, playing: None });
    }
}

/// Cross-fade each manca's clip to match its state; only acts on a real change (mirrors
/// `crab::drive_crab_animation`).
fn drive_manca_animation(
    anim: Option<Res<MancaAnim>>,
    mut mancae: Query<(&MancaAnimState, &mut MancaAnimPlayer)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Some(anim) = anim else { return };
    for (state, mut link) in &mut mancae {
        if link.playing == Some(*state) {
            continue;
        }
        let Ok((mut player, mut transitions)) = players.get_mut(link.player) else {
            continue; // transitions component not applied yet — retry next frame
        };
        let (node, speed) = match state {
            MancaAnimState::Snug => (anim.snug, 1.0),
            MancaAnimState::Idle => (anim.idle, 1.0),
            MancaAnimState::Walk => (anim.walk, WALK_ANIM_SPEED),
            MancaAnimState::Climb => (anim.climb, CLIMB_ANIM_SPEED),
            MancaAnimState::Attack => (anim.attack, ATTACK_ANIM_SPEED),
            MancaAnimState::BurrowOut => (anim.burrow, BURROW_ANIM_SPEED),
        };
        let active = transitions.play(&mut player, node, CROSSFADE);
        // Every clip loops except the one-shot eruption climb-out, which must play through once.
        if *state != MancaAnimState::BurrowOut {
            active.repeat();
        }
        active.set_speed(speed);
        link.playing = Some(*state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brood_size_stays_within_bounds_and_is_deterministic() {
        // Every seed yields a clutch inside the configured band, and the same seed always yields the same
        // size (the burst→brood loop must be reproducible for the exact-hash gate).
        for seed in 0u32..1000 {
            let n = brood_size(seed, 2, 3);
            assert!((2..=3).contains(&n), "brood {n} out of [2,3] for seed {seed}");
            assert_eq!(n, brood_size(seed, 2, 3), "brood_size must be deterministic");
        }
    }

    #[test]
    fn brood_size_single_clutch_is_exact() {
        // A degenerate [1,1] band always births exactly one manca (strictly self-replacing).
        for seed in 0u32..64 {
            assert_eq!(brood_size(seed, 1, 1), 1);
        }
    }

    #[test]
    fn brood_size_spans_the_whole_band() {
        // Both extremes of a wider band are reachable across seeds — the size actually varies, it isn't
        // pinned to one end by a bad rounding/clamp.
        let (mut lo, mut hi) = (false, false);
        for seed in 0u32..2000 {
            match brood_size(seed, 2, 4) {
                2 => lo = true,
                4 => hi = true,
                _ => {}
            }
        }
        assert!(lo && hi, "both brood extremes should be reachable across seeds");
    }
}
