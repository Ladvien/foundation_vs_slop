//! **The watch feed** — an anomalous screen that generates *while it is being watched*, and is
//! contained by depriving it of an audience (FVS-C-7).
//!
//! # Why this creature exists
//!
//! `ATTENTION` (`FieldId::ATTENTION`, the ambient decaying scalar the squad's line of sight deposits)
//! already drives SCP-1048: the Builder Bear **builds only what nobody is watching**, and containing
//! it means refusing to look away. This is the same channel with the condition flipped — a thing that
//! works *for an audience* and goes inert without one.
//!
//! That inversion is deliberately **not** new engineering, which is the whole argument for doing it
//! before FVS-C-6's per-entity watch primitive:
//!
//! * The perception fact already exists (`ai::utility::Fact::SeenBySquad`).
//! * The behaviour reads the same ambient field the bear reads, through the same
//!   `Stigmergy::sample` call.
//! * The containment rule is one authored line with `sign: AtMost` where 1048 has `AtLeast`. The rule
//!   model already supports it; `scp999` already ships a mixed-sign rule.
//!
//! **It adds no `Mode`, and that is a hard constraint rather than an accident.** `MODE_COUNT` sets
//! `NeuralPolicy::WEIGHT_COUNT`, so a new mode invalidates every baked policy archive *by width* —
//! `from_weights` rejects it loudly, as designed. SCP-1048 added two (`Build`, `Emote`), so a creature
//! adding one is the normal case, not a stretch, and doing it here would have silently wasted a
//! multi-hour bake. The screen is *building*, so `Mode::Build` is also the honest model.
//!
//! # The fiction, and why it is the antagonist's mechanic
//!
//! `lib.rs` names the antagonist: **SCP-9191, a rogue monster-generating AI** — literal AI slop. A
//! screen that mass-produces the same creature for as long as anyone looks at it is that thesis made
//! mechanical rather than narrated, and it is the first place the endgame theme touches gameplay.
//! What it emits is the swarm the player already fights, which is the point: the generator has no
//! imagination.
//!
//! # Determinism
//!
//! Gameplay, so `FixedUpdate` and pinned. Two rules are load-bearing:
//!
//! * **Placement is a scan, not a draw** — the same deterministic cell walk `scp999::spawn_scp999`
//!   uses. No RNG, so no seed to get wrong.
//! * **`sort_total!` before spawning.** The generation pass advances a shared counter
//!   ([`BroadcastSeq`]) that seeds each newborn crab, so ECS query order would otherwise decide which
//!   screen consumes which seed — the exact hazard `CLAUDE.md` names. Screens are immortal and never
//!   share a cell, so their position bits are a stable total key.

use bevy::prelude::*;

use crate::ai::field::FieldId;
use crate::ai::field::Stig;
use crate::dungeon::Dungeon;
use crate::sim::SimTuning;

/// An anomalous screen. Static: it never moves, so its position is a stable identity.
#[derive(Component, Debug)]
pub struct BroadcastScreen {
    /// Progress toward the next emission, in `[0, 1]`. Rises while watched, falls while ignored.
    pub charge: f32,
    /// How many creatures this screen has generated. Honest instrumentation rather than a derived
    /// guess: the first attempt to measure activation inferred it from "charge is low now and was
    /// high once", which counts idle windows, not firings, and reported 50 emissions where there
    /// were two. A counter that increments where the spawn happens cannot be misread.
    pub emissions: u32,
}

/// Monotonic seed source for crabs this anomaly generates.
///
/// Separate from `CrabSpawnSeq` deliberately: sharing it would make the swarm's nest-breeding seeds
/// depend on how much television the squad watched, coupling two economies that are otherwise
/// independent — and making a nest's output non-reproducible from its own state.
#[derive(Resource, Default, Debug)]
pub struct BroadcastSeq(pub u64);

/// Deterministic placement: the floor cells **just past** `spawn_min_dist`, spread apart.
///
/// A screen is *found*, not handed over — `spawn_min_dist` keeps it off the squad's doorstep. But
/// "past the minimum" has to mean *just* past it, and the first draft got that wrong in a way that
/// made the whole anomaly invisible.
///
/// # The bug this shape exists to prevent (FVS-N-30)
///
/// The first version mirrored `scp999::spawn_scp999`'s raster scan and took the FIRST cells
/// satisfying the minimum — i.e. scanning from `y=0, x=0`, the corner of the map. Measured on the
/// held-in seeds: the squad's closest approach across 60 s of passive play was **101-137 m**, ambient
/// `ATTENTION` at a screen peaked at **0.000**, and the feed never charged at all. A minimum distance
/// consumed in scan order is a *maximum* distance in disguise.
///
/// SCP-999 survives the same idiom only because it **moves** — it oozes toward the most-anxious
/// member, so it finds the squad even when it spawns in a corner. A static anomaly cannot, and
/// copying a mobile creature's placement was the mistake. (999's `spawn_min_dist` is equally not
/// doing what its name says; noted rather than changed here, since for 999 it is cosmetic.)
///
/// So: rank every eligible cell by distance from spawn and take the NEAREST, which is what a minimum
/// actually means. Screens are then kept `SCREEN_SEPARATION` apart so they do not stack into one
/// room, since two screens in one place is one screen with a spare.
///
/// # The screen is WALL-MOUNTED, and that is the second bug this shape prevents
///
/// The first version dropped the TV on the floor cell centre with an identity rotation. Both halves of
/// that were wrong, and the player captured it (2026-08-01, *"This TV is poking through the wall and
/// doesn't fit the aesthetic"*):
///
/// * **It clipped the wall, always.** `retro_tv_large.glb` measures 0.881 m wide, and a walled cell
///   offers `0.5·TILE_SIZE − WALL_THICKNESS` = **0.36 m** of clear floor from its centre against a
///   half-width of **0.44 m**. Not luck — geometry. Every screen that landed in a walled cell poked
///   ~0.08 m through.
/// * **It faced world +Z regardless of the room.** An identity quat on a prop whose whole point is
///   being *looked at* meant the screen could stare into the wall behind it.
///
/// Meanwhile `placement::furnish` already solves exactly this for wall sconces, and the fix is to place
/// the screen the way it places them: off [`Dungeon::wall_faces_near`], seated on the wall's inner
/// plane, yawed by the wall's inward normal. A bracket-mounted CRT high on a corridor wall is also what
/// the anomaly wants to be — a thing you walk under and catch out of the corner of your eye — rather
/// than furniture someone left in a hallway.
///
/// Camera-facing walls (`-X`/`-Z` inward normals) are skipped for the same reason `furnish::wall_runs`
/// skips them: those walls are cut down to knee height for the isometric view, so anything mounted at
/// head height on one would hang in the cutaway gap.
fn spawn_screens(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    rules: Res<crate::containment::ContainmentRules>,
    mut targets: ResMut<crate::containment::TargetSeq>,
    assets: Res<AssetServer>,
) {
    let cfg = &sim.broadcast;
    if cfg.count == 0 {
        return;
    }
    let scene: Handle<WorldAsset> =
        assets.load(GltfAssetLabel::Scene(0).from_asset("retro_tvs/retro_tv_large.glb"));

    // Every eligible cell, keyed by distance from spawn. The key carries the cell coords so it is a
    // stable TOTAL order: many cells share a distance (they lie on a ring), and `sort_total!` would
    // otherwise panic on the tie — correctly, since raster order deciding which of two equidistant
    // cells gets the screen is exactly the ECS-order dependence the determinism rules forbid.
    let mut eligible: Vec<(u32, i32, i32)> = Vec::new();
    for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }
            let d = (cell - dungeon.spawn).as_vec2().length();
            if d < cfg.spawn_min_dist {
                continue;
            }
            eligible.push((d.to_bits(), x, y));
        }
    }
    crate::sort_total!(&mut eligible, |k: &(u32, i32, i32)| (k.0, k.1, k.2));

    let mut placed: Vec<Vec3> = Vec::new();
    for (_, x, y) in eligible {
        if placed.len() >= cfg.count {
            break;
        }
        let cell = IVec2::new(x, y);
        let center = dungeon.cell_center(cell);
        if placed.iter().any(|p| p.distance(center) < SCREEN_SEPARATION) {
            continue;
        }
        // A cell with no mountable wall is simply not a candidate — the screen needs something to hang
        // on. Skipping is right here (unlike a silent degraded placement): the scan continues outward
        // and the shortfall warning below still fires if the level genuinely cannot hold the count.
        let Some((face, normal)) = mountable_wall(&dungeon, center) else {
            continue;
        };
        spawn_screen_at(
            &mut commands,
            screen_transform(face, normal),
            &scene,
            &rules,
            &mut targets,
        );
        placed.push(center);
    }
    if placed.len() < cfg.count {
        // Loud, not silent: a level that cannot hold the authored count is a content fact worth
        // seeing, and a screen that quietly did not spawn is the failure mode this whole item is about.
        warn!(
            "watch feed: placed {} of {} screens — no further floor cell {SCREEN_SEPARATION} m clear \
             of the others, past spawn_min_dist, and carrying a non-camera-facing wall to mount on",
            placed.len(),
            cfg.count
        );
    }
}

/// Minimum spacing between screens, in metres. Two screens in one room is one screen with a spare.
const SCREEN_SEPARATION: f32 = 20.0;

/// Height of the screen's **base** up the wall. `retro_tv_large.glb` is 0.656 m tall and origined at its
/// base centre, so the set top lands at 2.06 m under a [`crate::dungeon::WALL_HEIGHT`] of 2.4 — clear of
/// a ~1.85 m operative's head, and low enough to still read on screen in the isometric view.
const SCREEN_MOUNT_HEIGHT: f32 = 1.40;

/// Half the set's depth (measured: local Z spans ±0.281), plus the same 0.02 m skin `furnish` insets
/// sconces by. Seats the chassis back flush on the wall plane without z-fighting it.
const SCREEN_WALL_INSET: f32 = 0.281 + 0.02;

/// Pick the wall face this cell's screen mounts on, or `None` if it has none worth mounting on.
///
/// Deterministic by construction: [`Dungeon::wall_faces_near`] emits faces in a fixed E/W/N/S order
/// decided by cell geometry alone — no RNG, no ECS query — so taking the first survivor is a total
/// order already. Camera-facing walls are filtered for the cutaway reason in [`spawn_screens`]' docs.
fn mountable_wall(dungeon: &Dungeon, center: Vec3) -> Option<(Vec3, Vec3)> {
    dungeon
        .wall_faces_near(center)
        .into_iter()
        .find(|&(_, normal)| {
            !(crate::dungeon::SHORT_CAMERA_WALLS && crate::dungeon::is_camera_facing(normal))
        })
}

/// Seat a screen on a wall face: lifted to [`SCREEN_MOUNT_HEIGHT`], pushed [`SCREEN_WALL_INSET`] into
/// the room along the wall's inward `normal`, and yawed so the glass faces the room.
///
/// The yaw is `atan2(normal.x, normal.z)`, which maps the model's local +Z onto the inward normal —
/// and local +Z **is** the screen direction for this kit (`light::attach_screen_lights` documents it:
/// the spotlight there adds a PI flip precisely because Bevy's spot axis is −Z while the glass faces
/// +Z). Same expression `furnish` yaws its wall anchors by, so a kit swap that changes the convention
/// breaks both in the same direction rather than one silently.
fn screen_transform(face: Vec3, normal: Vec3) -> Transform {
    Transform::from_translation(
        face.with_y(SCREEN_MOUNT_HEIGHT) + normal * SCREEN_WALL_INSET,
    )
    .with_rotation(Quat::from_rotation_y(normal.x.atan2(normal.z)))
}

/// Spawn one screen with an already-seated `transform`.
///
/// `pub` for the Research Room dev palette, which does **not** call it yet (checked 2026-08-01:
/// `src/research_room/` names nothing in this module). Kept public and documented as the one way to
/// place a screen, so the palette wires to this rather than growing a second spawn path.
///
/// Takes a whole `Transform` rather than a bare position because a screen's **yaw is load-bearing** —
/// it is an anomaly you contain by looking away from, so which way the glass points is gameplay, not
/// dressing. Callers build one with [`screen_transform`] from a wall face; the dev palette may pass its
/// own. The old `pos: Vec3` signature could only ever produce the identity rotation that had every
/// screen facing world +Z.
pub fn spawn_screen_at(
    commands: &mut Commands,
    transform: Transform,
    scene: &Handle<WorldAsset>,
    rules: &crate::containment::ContainmentRules,
    seq: &mut crate::containment::TargetSeq,
) -> Entity {
    commands
        .spawn((
            BroadcastScreen { charge: 0.0, emissions: 0 },
            // Contained by looking AWAY — the `AtMost` inverse of SCP-1048's rule. Carried on the
            // entity like every other anomaly's, so `containment::tick` needs no special case.
            crate::containment::Containment::new(
                rules.0.broadcast.clone(),
                crate::knowledge::Subject::WatchFeed,
            ),
            // Without a `TargetId` the containment VERBS cannot address it: `nearest_target` iterates
            // `(TargetId, _, Vec3)` and sorts on the id for a stable total order. An anomaly carrying a
            // `Containment` but no id would show a rule in the records and be unselectable in play —
            // the "shipped, tested, unreachable" shape this repo keeps catching.
            seq.next(),
            transform,
            crate::session::run_scoped(),
            WorldAssetRoot(scene.clone()),
        ))
        .id()
}

/// Charge while watched, decay while ignored, and emit at full charge.
///
/// The gate is the **ambient** `ATTENTION` field at the screen's own cell — the same read
/// `scp1048::replicate` makes, and deliberately not `enemy::ObservedBySquad`, the per-entity flag
/// FVS-M-1 added for 173/096. An ambient field means an audience is a place you maintain, which is
/// what makes "look away to contain it" a position the player holds rather than a button.
#[allow(clippy::too_many_arguments)]
fn screens_generate_while_watched(
    mut commands: Commands,
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    stig: Res<Stig>,
    sim: Res<SimTuning>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
    graph: Option<Res<crate::crab::SurfaceGraph>>,
    crab_assets: Option<Res<crate::crab::CrabAssets>>,
    crab_anim: Option<Res<crate::crab::CrabAnim>>,
    mut seq: ResMut<BroadcastSeq>,
    mut screens: Query<(Entity, &Transform, &mut BroadcastScreen), Without<crate::containment::Contained>>,
) {
    let (Some(graph), Some(crab_assets), Some(crab_anim)) = (graph, crab_assets, crab_anim) else {
        return; // crab assets not loaded yet — nothing to generate with
    };
    let cfg = &sim.broadcast;
    let dt = time.delta_secs();

    // SORT-OK is NOT enough here: `seq` is shared across screens, so query order would decide which
    // screen gets which seed. Screens are immortal and never co-located, so position bits are total.
    let mut order: Vec<(u32, u32, u32, Entity)> = screens
        .iter()
        .map(|(e, tf, _)| {
            (tf.translation.x.to_bits(), tf.translation.y.to_bits(), tf.translation.z.to_bits(), e)
        })
        .collect();
    crate::sort_total!(&mut order, |k: &(u32, u32, u32, Entity)| (k.0, k.1, k.2));

    for (.., e) in order {
        let Ok((_, tf, mut screen)) = screens.get_mut(e) else { continue };
        let watched = stig.sample(FieldId::ATTENTION, &dungeon, tf.translation) >= cfg.watch_threshold;
        if watched {
            screen.charge += cfg.charge_rate * dt;
        } else {
            // Decays rather than resetting: looking away briefly should *cost* the feed progress, not
            // hand the player a free reset for a flicker of the camera. Same reasoning as
            // `OnBreak::Keep` on the forgiving containment rules.
            screen.charge = (screen.charge - cfg.decay_rate * dt).max(0.0);
        }
        if screen.charge < 1.0 {
            continue;
        }
        let Some(patch) = graph.floor_patch_cell(dungeon.world_to_cell(tf.translation)) else {
            continue; // its own cell is not seatable — hold the charge rather than losing it
        };
        screen.charge = 0.0;
        screen.emissions += 1;
        let s = seq.0 as u32;
        seq.0 += 1;
        crate::crab::spawn_crab_on_patch(
            &mut commands,
            &graph,
            patch,
            &crab_assets.collider,
            &crab_assets.scene,
            &crab_anim,
            s,
            &sim,
            beh.crab,
        );
    }
}

pub struct BroadcastPlugin;

impl Plugin for BroadcastPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BroadcastSeq>()
            .add_systems(
                OnEnter(crate::session::RunState::Active),
                spawn_screens.in_set(crate::session::RunBuild::Populate),
            )
            // `.after(AiSet::Deposits)` so the ATTENTION read sees THIS tick's line-of-sight deposit,
            // not last tick's — the same ordering `scp1048::replicate` relies on. A screen must go
            // inert on the first tick the squad actually looks away, or "look away to contain it"
            // lags the player's input by a frame and reads as unresponsive.
            .add_systems(
                FixedUpdate,
                screens_generate_while_watched
                    .after(crate::ai::AiSet::Deposits)
                    .run_if(in_state(crate::session::RunState::Active)),
            );
    }
}
