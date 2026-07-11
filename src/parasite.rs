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

use std::collections::HashSet;
use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

use bevy::prelude::*;

use crate::dungeon::Dungeon;
use crate::enemy::Hostile;
use crate::gore::{GoreEvent, GoreKind, GoreQueue};
use crate::health::{Health, NoHealthBar};
use crate::light::Photophobic;
use crate::sim::SimTuning;
use crate::surface_nav::{clamp_to_patch, project_tangent, surface_orientation, SurfaceGraph};
use crate::util::{hash01_u32, nearest_planar, unit_is_facing};

/// The SCP-150 asset (skinned glTF, 12 baked clips). Clip indices follow the asset manual's table order:
/// 0 Idle_Snug · 1 Idle_Alert · 2 Walk1 · 3 Walk2 · 4 Run · 5 Leap · 6 Attack1 · 7 Attack2 · 8 Forage1 ·
/// 9 Forage2 · 10 BurrowOut · 11 Climb.
const SCP150_GLB: &str = "scp150/scp-150.glb";

/// Mancae seed at least this far (tiles) from the squad spawn, and at least this far apart. (Spawn
/// geometry — count/hp/speeds are gameplay knobs and live in `sim::ParasiteTuning`.)
const MANCA_MIN_SPAWN_DIST: f32 = 8.0;
const MANCA_CLUSTER_SEP: f32 = 4.0;

/// Uniform render scale for the child model. The asset body is ≈3.6 long in Blender units; at 0.11 the
/// juvenile reads ≈0.4 m — smaller than an adult crab, sized to the ~6 ft squad. Tuned by devshot.
const MANCA_RENDER_SCALE: f32 = 0.11;
/// Root body-centre height above the surface, along the surface normal (also seats the collider).
const MANCA_BODY_CENTER: f32 = 0.1;
/// Local Y offset of the scaled model under the root so its body rests on the surface. Calibrated by eye.
const MANCA_MODEL_Y: f32 = 0.18;
/// Radius of the invisible collider sphere (the laser raycast target); world-size since the root is
/// unscaled. Sized to hug the visible manca.
pub(crate) const MANCA_COLLIDER_R: f32 = 0.22;
/// The asset faces **−X** in Blender (head/mouth at −X); the engine's forward is **−Z**. Rotate the child
/// model −90° about +Y so its head points along the entity's facing (`surface_orientation` aims −Z).
const MODEL_FACING: f32 = -FRAC_PI_2;

/// Frame-dt clamp so a hitch can't fling a manca off its surface (mirrors `crab::MAX_FRAME_DT`).
const MAX_FRAME_DT: f32 = 1.0 / 30.0;
/// Reach the flow gate this close to commit a patch transfer.
const TRANSFER_RADIUS: f32 = 0.22;
/// How fast the manca's surface normal eases toward a new patch's normal (per second).
const NORMAL_EASE: f32 = 12.0;
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
    // A later medic "cure" phase adds `cure_progress: f32` here; the struct is intentionally shaped for it.
}

impl Default for Infestation {
    fn default() -> Self {
        Self { active: false, timer: 0.0, seed: 0 }
    }
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
}

/// A manca's ballistic-leap state — hunker then arc onto a host, mirroring `crab::CrabJump`. `Ready` =
/// grounded (normal locomotion runs); `Hunker`/`Air` own the manca's transform, so `manca_hunt` skips it.
#[derive(Component)]
pub struct MancaLeap {
    phase: LeapPhase,
    /// Time left in the current `Hunker`/`Air` phase.
    timer: f32,
    /// Cooldown before the next leap (counts down while `Ready`).
    cooldown: f32,
    from: Vec3,
    to: Vec3,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LeapPhase {
    Ready,
    Hunker,
    Air,
}

/// The manca's animation state, chosen from movement/leap each frame (drives clip selection).
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum MancaAnimState {
    Idle,
    Walk,
    Climb,
    Attack,
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

/// The one shared animation graph + node handles for the manca clips.
#[derive(Resource)]
struct MancaAnim {
    graph: Handle<AnimationGraph>,
    idle: AnimationNodeIndex,
    walk: AnimationNodeIndex,
    climb: AnimationNodeIndex,
    attack: AnimationNodeIndex,
}

// --- Plugin ----------------------------------------------------------------------------------------

pub struct ParasitePlugin;

impl Plugin for ParasitePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MancaSpawnSeq>()
            .add_systems(Startup, build_manca_anim)
            // Spawn in `PostStartup` so the crab's `SurfaceGraph` (built in its `Startup`) already exists —
            // mancae ride the same surface manifold and must never build a second graph.
            .add_systems(PostStartup, spawn_mancae)
            // Pinned manca simulation on `FixedUpdate`: locomotion + leap (change Transform), then embed
            // (flips a host's Infestation + despawns the manca) and the gestation clock. All change pinned
            // state, so ordering is explicit and the whole chain is covered by the exact-hash gate.
            .add_systems(
                FixedUpdate,
                (
                    manca_hunt,
                    manca_leap.after(manca_hunt),
                    manca_embed.after(manca_leap),
                    gestation_tick.after(manca_embed),
                    // Burst before the crab's despawn owner so a crab host gibs the same tick it bursts;
                    // it only zeroes host HP + spawns the brood — the host's own despawn owner does the gore.
                    parasite_burst.after(gestation_tick).before(crate::crab::CrabDespawn),
                    // Sole HP≤0 owner for mancae (a shot manca) — mirrors `crab_despawn_dead`.
                    manca_despawn_dead,
                ),
            )
            // Cosmetic: skeletal animation attach/drive stays on `Update` (mirrors the crab).
            .add_systems(Update, (attach_manca_animation, drive_manca_animation));
    }
}

/// Build the shared animation graph over the manca clips we drive in Phase 1.
fn build_manca_anim(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let (graph, nodes) = AnimationGraph::from_clips([
        assets.load(GltfAssetLabel::Animation(1).from_asset(SCP150_GLB)), // Idle_Alert
        assets.load(GltfAssetLabel::Animation(2).from_asset(SCP150_GLB)), // Walk1
        assets.load(GltfAssetLabel::Animation(11).from_asset(SCP150_GLB)), // Climb
        assets.load(GltfAssetLabel::Animation(7).from_asset(SCP150_GLB)), // Attack2 (pounce-bite)
    ]);
    let handle = graphs.add(graph);
    commands.insert_resource(MancaAnim {
        graph: handle,
        idle: nodes[0],
        walk: nodes[1],
        climb: nodes[2],
        attack: nodes[3],
    });
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

    // Greedily pick far, spread-apart floor cells (deterministic, mirrors the crab's nest-seed scan).
    let mut seeds: Vec<IVec2> = Vec::new();
    'scan: for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }
            if (cell - dungeon.spawn).as_vec2().length() < MANCA_MIN_SPAWN_DIST {
                continue;
            }
            if seeds.iter().any(|c| (*c - cell).as_vec2().length() < MANCA_CLUSTER_SEP) {
                continue;
            }
            seeds.push(cell);
            if seeds.len() >= sim.parasite.initial_count {
                break 'scan;
            }
        }
    }
    if seeds.is_empty() {
        warn!("parasite: no floor cell far enough from spawn to seed a manca");
        return;
    }

    let mut spawned = 0usize;
    for cell in seeds {
        let Some(patch) = graph.floor_patch_cell(cell) else { continue };
        let s = seq.0 as u32;
        seq.0 += 1;
        spawn_manca_on_patch(&mut commands, &graph, patch, &collider, &scene, s, &sim.parasite);
        spawned += 1;
    }
    info!("parasite: seeded {spawned} SCP-150 mancae");
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
) {
    let p = graph.patch(patch);
    let pos = p.center;
    let normal = p.normal;
    let heading = p.tan_u;
    let seat = pos + normal * MANCA_BODY_CENTER;

    let mut ec = commands.spawn((
        Manca,
        Hostile,
        Health::new(tuning.manca_hp),
        NoHealthBar, // lone stalkers, but no floating bar — they read as ambient dread, not a HP puzzle
        // A slop entity: same faction as the watcher boss, so the squad's anomaly-fear machinery applies.
        crate::ai::faction::Faction::Anomaly,
        MancaMotion { patch, pos, normal, heading },
        MancaLeap { phase: LeapPhase::Ready, timer: 0.0, cooldown: tuning.leap_cooldown, from: Vec3::ZERO, to: Vec3::ZERO },
        MancaAnimState::Idle,
        MancaSeed(rand_seed),
        // Sphere collider mesh paired with its CPU laser hit-volume (same radius, zero-height capsule) so
        // bolts test against the manca headlessly + deterministically.
        (
            Mesh3d(collider.clone()),
            crate::laser::LaserTarget { radius: MANCA_COLLIDER_R, half_height: 0.0 },
        ),
        // Render-only: smooth the manca's 60 Hz movement + surface rotation across the display refresh.
        (
            Transform::from_translation(seat).with_rotation(surface_orientation(heading, normal)),
            avian3d::prelude::TransformInterpolation,
        ),
        Visibility::Inherited,
    ));
    // Photophobic: mancae steer toward shadow like crabs — they pool in the dark, reinforcing the
    // "drifts to a dark corner" fantasy (the light nudge is wired in a later phase; the trait is fixed
    // here so it never churns the hashed archetype).
    ec.insert(Photophobic);
    // The scaled glTF body, `−X`→`−Z` corrected, seated so its body rests on the surface.
    ec.with_child((
        WorldAssetRoot(scene.clone()),
        Transform::from_translation(Vec3::Y * MANCA_MODEL_Y)
            .with_rotation(Quat::from_rotation_y(MODEL_FACING))
            .with_scale(Vec3::splat(MANCA_RENDER_SCALE)),
    ));
}

/// Move every grounded manca one step along the surface toward the nearest host, blind-side-stalking a
/// host that is facing it, and enter the leap wind-up once in range. Mid-leap mancae are owned by
/// `manca_leap` and skipped here.
fn manca_hunt(
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    hosts: Query<(&Transform, &Infestation), (With<Parasitizable>, Without<Manca>)>,
    mut mancae: Query<
        (&mut MancaMotion, &mut MancaAnimState, &mut MancaLeap, &mut Transform),
        With<Manca>,
    >,
) {
    let Some(graph) = graph else { return };
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    // Per-host: foot position + planar forward (local −Z) — for the blind-side stalk gate. Already-infested
    // hosts are skipped, so a manca seeks a FRESH host once one is claimed — the infestation spreads through
    // the group rather than piling onto a doomed host.
    let host_data: Vec<(Vec3, Vec3)> = hosts
        .iter()
        .filter(|(_, inf)| !inf.active)
        .map(|(t, _)| {
            let fwd = (t.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
            (t.translation, fwd)
        })
        .collect();

    for (mut motion, mut anim, leap, mut transform) in &mut mancae {
        // Mid-leap mancae are owned by `manca_leap` (it drives their arc + transform) — skip them here.
        if leap.phase != LeapPhase::Ready {
            continue;
        }

        // Nearest host (shared deterministic ranking; forward vector carried as the payload).
        let nearest = nearest_planar(motion.pos, host_data.iter().map(|&(hp, fwd)| (fwd, hp)));

        let moving = if let Some((hfwd, hpos, _d)) = nearest {
            let to_host = (hpos - motion.pos).with_y(0.0);
            let planar_d = to_host.length();
            // Blind-side stalk: if the host is close and looking at this manca, arc tangentially toward its
            // rear rather than charging head-on, until the manca clears the facing cone.
            let desired = if planar_d > LEAP_MIN
                && planar_d < STALK_BAND
                && unit_is_facing(hpos, hfwd, motion.pos, BLIND_COS)
            {
                let bearing = to_host.normalize_or_zero();
                let tang = Vec3::new(-bearing.z, 0.0, bearing.x); // perpendicular, ground plane
                let sign = if tang.dot(hfwd) >= 0.0 { 1.0 } else { -1.0 }; // toward the host's rear
                bearing + tang * sign * STALK_STRENGTH
            } else {
                hpos - motion.pos
            };
            // Climb speed on a wall patch, crawl speed on the floor.
            let speed = if graph.patch(motion.patch).normal.y < WALL_NORMAL_Y {
                sim.parasite.climb_speed
            } else {
                sim.parasite.crawl_speed
            };
            steer_surface(&mut motion, &graph, &dungeon, desired, speed, dt)
        } else {
            false
        };

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
/// surface normal + heading. Returns whether it actually moved. A lean cousin of `crab::steer_surface`
/// (no swarm separation — mancae are lone stalkers) over the shared surface primitives.
fn steer_surface(
    motion: &mut MancaMotion,
    graph: &SurfaceGraph,
    dungeon: &Dungeon,
    desired: Vec3,
    speed: f32,
    dt: f32,
) -> bool {
    let _ = dungeon; // reserved for future light/threat sampling; kept for signature parity with the crab
    let p = graph.patch(motion.patch);
    let desired_t = project_tangent(desired, p.normal).normalize_or_zero();

    // Pick the neighbour whose gate direction best matches the desired travel direction.
    let mut best: Option<(u32, Vec3)> = None;
    let mut best_dot = 0.0f32;
    if desired_t.length_squared() > 1.0e-6 {
        for (to, gate) in graph.neighbors(motion.patch) {
            let g_dir = project_tangent(gate - motion.pos, p.normal).normalize_or_zero();
            let d = g_dir.dot(desired_t);
            if d > best_dot {
                best_dot = d;
                best = Some((to, gate));
            }
        }
    }

    // Steer toward the chosen gate, else drift in the desired direction (clamped to the patch).
    let steer_to = best.map(|(_, g)| g).unwrap_or(motion.pos + desired_t);
    let tangent = project_tangent(steer_to - motion.pos, p.normal).normalize_or_zero();
    let move_vec = tangent * speed;
    motion.pos += move_vec * dt;
    motion.pos = clamp_to_patch(motion.pos, p);

    // Commit a patch transfer only on physically reaching the chosen gate — never across a wall.
    if let Some((to, gate)) = best {
        if motion.pos.distance(gate) < TRANSFER_RADIUS {
            motion.patch = to;
            motion.pos = clamp_to_patch(gate, graph.patch(to));
        }
    }

    // Ease onto the (possibly new) surface and turn toward travel.
    let t = (NORMAL_EASE * dt).min(1.0);
    let np = graph.patch(motion.patch);
    motion.normal = motion.normal.lerp(np.normal, t).normalize_or(np.normal);
    let moved = move_vec.length_squared() > 1.0e-6;
    if moved {
        let h = project_tangent(move_vec, motion.normal).normalize_or(motion.heading);
        motion.heading = motion.heading.lerp(h, t).normalize_or(motion.heading);
    }
    moved
}

/// Ballistic leap onto a host — hunker, then arc over, mirroring `crab::crab_jump`. While hunkering /
/// airborne this owns the manca's transform (`manca_hunt` skips it). Phase 1 lands and re-arms the
/// cooldown; a later phase hooks the *embed* onto the landing.
fn manca_leap(
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    hosts: Query<(&Transform, &Infestation), (With<Parasitizable>, Without<Manca>)>,
    mut mancae: Query<
        (&mut MancaMotion, &mut MancaAnimState, &mut MancaLeap, &mut Transform),
        With<Manca>,
    >,
) {
    let Some(graph) = graph else { return };
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    // Host positions + planar forwards (for the blind-side gate at launch); skip already-infested hosts.
    let host_data: Vec<(Vec3, Vec3)> = hosts
        .iter()
        .filter(|(_, inf)| !inf.active)
        .map(|(t, _)| {
            let fwd = (t.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
            (t.translation, fwd)
        })
        .collect();
    let nearest = |from: Vec3| nearest_planar(from, host_data.iter().map(|&(hp, fwd)| (fwd, hp)));

    for (mut motion, mut anim, mut leap, mut tf) in &mut mancae {
        match leap.phase {
            LeapPhase::Ready => {
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
    mancae: Query<(Entity, &MancaMotion, &MancaLeap, &MancaSeed, &Health), With<Manca>>,
    mut hosts: Query<
        (Entity, &Transform, &mut Health, &mut Infestation),
        (With<Parasitizable>, Without<Manca>),
    >,
) {
    // Grounded, living mancae only (a mid-leap manca embeds on the tick it lands and returns to `Ready`).
    let mut ready: Vec<(u32, Entity, Vec3)> = mancae
        .iter()
        .filter(|(_, _, leap, _, hp)| leap.phase == LeapPhase::Ready && hp.current > 0.0)
        .map(|(e, motion, _, seed, _)| (seed.0, e, motion.pos))
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
fn gestation_tick(time: Res<Time>, mut hosts: Query<&mut Infestation, With<Parasitizable>>) {
    let dt = time.delta_secs();
    for mut inf in &mut hosts {
        if inf.active {
            inf.timer += dt;
        }
    }
}

/// Deterministic brood size for a host, from its infestation seed — clamped to `[min, max]`.
fn brood_size(seed: u32, min: u32, max: u32) -> u32 {
    let span = (max - min + 1) as f32;
    let n = min + (hash01_u32(seed.wrapping_mul(0x9E37_79B1).wrapping_add(21)) * span) as u32;
    n.min(max)
}

/// Burst: an infestation whose gestation clock has run out births a small brood of fresh mancae at the
/// host's cell and kills the host. **One path (no duplicated gore):** this only spawns the brood and zeroes
/// the host's HP — the host's OWN despawn owner then gibs it in its idiom (`despawn_dead_units` →
/// `UnitCrunch`; `crab_despawn_dead` → ichor splat). Brood spawning is capped by [`MANCA_COUNT_MAX`], the
/// load-bearing gate that keeps the loop from exploding.
///
/// **Determinism.** Hosts burst in a stable geometric order (position bits, like `crab_contact_damage`),
/// brood seeds come from the monotonic [`MancaSpawnSeq`] (never position-derived), and brood size is a pure
/// function of the host's infestation seed — so which hosts burst, how many mancae, and the entity-id/gore
/// order are all reproducible across same-seed runs.
#[allow(clippy::type_complexity)]
fn parasite_burst(
    mut commands: Commands,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    manca_assets: Option<Res<MancaAssets>>,
    mut seq: ResMut<MancaSpawnSeq>,
    live_mancae: Query<(), With<Manca>>,
    mut hosts: Query<(Entity, &Transform, &mut Health, &mut Infestation), With<Parasitizable>>,
) {
    let (Some(graph), Some(manca_assets)) = (graph, manca_assets) else { return };
    let p = &sim.parasite;

    // Collect hosts ready to burst, keyed by a stable geometric sort key (not ECS iteration order).
    let mut ready: Vec<((u32, u32, u32), Entity, Vec3, u32)> = Vec::new();
    for (e, tf, _hp, inf) in &hosts {
        if inf.active && inf.timer >= p.gestation_seconds {
            let key = (
                tf.translation.x.to_bits(),
                tf.translation.y.to_bits(),
                tf.translation.z.to_bits(),
            );
            ready.push((key, e, tf.translation, brood_size(inf.seed, p.brood_min, p.brood_max)));
        }
    }
    if ready.is_empty() {
        return;
    }
    ready.sort_unstable_by_key(|(key, _, _, _)| *key);

    let mut live = live_mancae.iter().count();
    for (_key, host_e, hpos, brood) in ready {
        // Spawn the brood at the host's floor cell, capped by the population limit.
        if let Some(patch) = graph.floor_patch_cell(dungeon.world_to_cell(hpos)) {
            for _ in 0..brood {
                if live >= p.manca_count_max {
                    break;
                }
                let s = seq.0 as u32;
                seq.0 += 1;
                spawn_manca_on_patch(&mut commands, &graph, patch, &manca_assets.collider, &manca_assets.scene, s, p);
                live += 1;
            }
        }
        // Kill the host — zero HP + clear the infestation, and let the host's own despawn owner gib it.
        if let Ok((_, _, mut hp, mut inf)) = hosts.get_mut(host_e) {
            inf.active = false;
            hp.current = 0.0;
        }
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
            MancaAnimState::Idle => (anim.idle, 1.0),
            MancaAnimState::Walk => (anim.walk, WALK_ANIM_SPEED),
            MancaAnimState::Climb => (anim.climb, CLIMB_ANIM_SPEED),
            MancaAnimState::Attack => (anim.attack, ATTACK_ANIM_SPEED),
        };
        let active = transitions.play(&mut player, node, CROSSFADE);
        active.repeat().set_speed(speed);
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
