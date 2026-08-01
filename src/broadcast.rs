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
}

/// Monotonic seed source for crabs this anomaly generates.
///
/// Separate from `CrabSpawnSeq` deliberately: sharing it would make the swarm's nest-breeding seeds
/// depend on how much television the squad watched, coupling two economies that are otherwise
/// independent — and making a nest's output non-reproducible from its own state.
#[derive(Resource, Default, Debug)]
pub struct BroadcastSeq(pub u64);

/// Deterministic placement, mirroring `scp999::spawn_scp999`'s cell scan.
///
/// A screen is *found*, not handed over: `spawn_min_dist` keeps it away from the squad's entry so the
/// player meets it after committing to the level.
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

    let mut placed = 0usize;
    'scan: for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }
            if (cell - dungeon.spawn).as_vec2().length() < cfg.spawn_min_dist {
                continue;
            }
            spawn_screen_at(&mut commands, dungeon.cell_center(cell), &scene, &rules, &mut targets);
            placed += 1;
            if placed >= cfg.count {
                break 'scan;
            }
        }
    }
}

/// Spawn one screen at `pos`. `pub` so the Research Room dev palette can place one by hand.
pub fn spawn_screen_at(
    commands: &mut Commands,
    pos: Vec3,
    scene: &Handle<WorldAsset>,
    rules: &crate::containment::ContainmentRules,
    seq: &mut crate::containment::TargetSeq,
) -> Entity {
    commands
        .spawn((
            BroadcastScreen { charge: 0.0 },
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
            Transform::from_translation(pos),
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
