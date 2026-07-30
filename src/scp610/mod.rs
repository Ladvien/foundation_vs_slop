//! **SCP-610, "The Flesh that Hates"** — an area-denial infection bloom.
//!
//! The first user of [`crate::containment::Quarantinable`], which has existed, been tested, and
//! spawned on nothing since it was written. `containment::area`'s own module doc says why it was
//! built: *"For SCP-610 the canon procedure is isolating an **area**, not point-capturing a body —
//! there is no single body to capture."* This is that body-less thing.
//!
//! # It does not move, and that is the design
//!
//! Every other anomaly in this game is an actor: the crabs swarm, the bears build, the blob seeks, the
//! watcher watches. SCP-610 is **terrain that is alive**. It has no brain, no [`crate::ai::Drives`],
//! no locomotion and no attack — it grows where it is and denies the room, and the only answer is to
//! bound the region and hold it. Canon supports this directly (infected eventually root themselves in
//! place), and `assets/scp610/README.md` §5 lists a rooted Stage-3 form as the asset's natural
//! extension rather than a compromise.
//!
//! That is also why it needs no `BrainId`, no `Mode`/`Fact` additions and no faction of its own — all
//! of which are **append-only** enums whose discriminants index saved beliefs, mode distributions and
//! archived RL policies. A creature that perceives nothing needs none of them. It carries
//! [`crate::ai::faction::Faction::Anomaly`], which the faction docs describe as a *perception*
//! partition with a deliberately empty drive list ("afraid of nothing") — exactly right for mindless
//! flesh, and it adds no drive that would leak into the watcher's behaviour.
//!
//! # The `mutation` morph is the disease, not a wobble
//!
//! The asset ships two morph targets: `Basis` (still-human) and `mutation` (full Stage-2 infected —
//! limbs extended, head-lobe bulged). The hand-off doc is emphatic that this is *"not cosmetic wobble
//! like a blob's squash/stretch — it is the disease progressing"*. So the weight is driven from the
//! bloom's own age: a fresh bloom still reads as a person, and it becomes the thing while the player
//! decides what to do about it.
//!
//! That driver lives in the **visuals** plugin, windowed-only. The morph changes no hashed state, and
//! putting it in the sim would make a cosmetic weight part of `snapshot_hash`.
//!
//! # Tunables are constants here, deliberately
//!
//! No `sim.scp610:` config slice. Adding a top-level or `sim:` slice obliges `squad_ai::world_genome`
//! to learn every knob (or `authored_round_trips_exactly` fails) and `tests/genome_coverage.rs` to
//! rule on it. Nothing below is a *difficulty* dial the search should be tuning — they are the shape of
//! one creature — so they stay as named constants until something wants to evolve them. The one thing
//! that *is* authored is the containment rule, which lives in `config.ron`'s `containment:` slice like
//! every other anomaly's.

use std::sync::Arc;

use bevy::prelude::*;

use crate::containment::Quarantinable;

/// The asset. Recompressed 28.7 MB → 5.2 MB (see `assets/scp610/README.md`); contract unchanged.
const SCP610_GLB: &str = "scp610/scp-610.glb";

/// Authored at real human scale — 1.80 × 0.86 × 1.90 m fully grown — so unlike the blob and the crab
/// this spawns unscaled. `assets/scp610/README.md` §2.
const RENDER_SCALE: f32 = 1.0;

/// Blooms per level. Small on purpose: this is a room-denial hazard, and three of them across a level
/// is already three rooms the squad has to solve rather than walk through.
const BLOOM_COUNT: usize = 3;
/// Minimum distance from the squad spawn, in tiles — nobody opens a run standing in one. Measured to a
/// room's centre, since placement is per-room (see [`spawn_scp610_blooms`]).
const SPAWN_MIN_DIST: i32 = 24;

/// Seconds for a fresh bloom to go from "still looks like a person" to fully turned.
///
/// Slow on purpose. The morph is the tell that the thing in the corner *used to be someone*, and it
/// only lands if the player is present while it happens rather than arriving after the fact.
const MUTATION_SECS: f32 = 45.0;

/// The bloom's animation graph + slot table, built once at `Startup`.
///
/// One clip: `scp610_idle`, the asset's "agitated tremor" (canon — infected seek contact even before
/// pursuing). Without it the creature stands in its T-pose bind pose, which reads as a broken asset
/// rather than as a stationary one.
///
/// A resource rather than per-spawn construction so every bloom clones one handle, and so the
/// harness and the windowed build take the identical path.
#[derive(Resource)]
pub struct Scp610Anim {
    graph: Handle<AnimationGraph>,
    slots: Arc<[crate::anim::Slot]>,
}

/// glTF animation index for `scp610_idle`. Pinned by `tests/creature_clip_contract.rs`.
const CLIP_IDLE: usize = 0;

/// Marks an SCP-610 bloom.
#[derive(Component)]
pub struct Scp610;

/// Per-bloom deterministic seed. A monotonic spawn ordinal, never an `Entity` id (recycled) and never
/// a position (ties) — the same rule `scp999`/`crab` follow.
#[derive(Component, Clone, Copy)]
pub struct Scp610Seed(pub u32);

/// Drives the `mutation` morph weight. Cosmetic; lives on the entity so the visuals plugin can read a
/// per-bloom age without keeping a side table.
#[derive(Component)]
pub struct Scp610Mutation {
    /// 0.0 = still-human, 1.0 = fully turned.
    pub current: f32,
    pub rate_per_sec: f32,
}

impl Default for Scp610Mutation {
    fn default() -> Self {
        Self { current: 0.0, rate_per_sec: 1.0 / MUTATION_SECS }
    }
}

/// Gameplay half — registered in **both** `lib::run` and `sim_harness`.
pub struct Scp610Plugin;

impl Plugin for Scp610Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, build_scp610_anim).add_systems(
            OnEnter(crate::session::RunState::Active),
            spawn_scp610_blooms.in_set(crate::session::RunBuild::Populate),
        );
    }
}

/// Cosmetic half — windowed only. Never registered in the harness.
pub struct Scp610VisualsPlugin;

impl Plugin for Scp610VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (drive_mutation, drive_scp610_animation)
                .distributive_run_if(in_state(crate::session::RunState::Active))
                .after(crate::anim::PoseAttachSet),
        );
    }
}

/// `Startup`: build the one shared graph. Registered in the gameplay plugin, not the visuals one,
/// because `BlendSource` is inserted **at spawn** — a component added later would churn the hashed
/// archetype — so the resource has to exist wherever the spawner runs, harness included.
fn build_scp610_anim(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let (graph, nodes) = AnimationGraph::from_clips([
        assets.load(GltfAssetLabel::Animation(CLIP_IDLE).from_asset(SCP610_GLB))
    ]);
    // `free`, not `gait`: the bloom never travels, so there is no ground distance to sync a stride
    // against. Speed 1.0 — the clip is authored at 24 fps gameplay tempo (README 5).
    let slots: Arc<[crate::anim::Slot]> =
        nodes.iter().map(|&n| crate::anim::Slot::free(n, 1.0)).collect();
    commands.insert_resource(Scp610Anim { graph: graphs.add(graph), slots });
}

/// The one shared builder — used by the seeded spawner and (later) the Research Room F6 palette, so a
/// dev-dropped bloom is byte-identical to a naturally seeded one. Same contract as
/// `scp999::spawn_scp999_at`.
pub fn spawn_scp610_at(
    commands: &mut Commands,
    assets: &AssetServer,
    seed: u32,
    pos: Vec3,
    rule: crate::containment::ContainmentRule,
    seq: &mut crate::containment::TargetSeq,
    anim: &Scp610Anim,
) -> Entity {
    commands
        .spawn((
            crate::session::run_scoped(),
            Scp610,
            // The uniform aim key. Minted in the shared builder for the reason `scp999` documents.
            seq.next(),
            // **The point of this module.** `Quarantinable` is a species property inserted at spawn and
            // never toggled — toggling it would churn the hashed archetype. `containment::area`'s
            // `tick_quarantine` has existed and run on nothing until now.
            Quarantinable,
            crate::containment::Containment::new(rule, crate::knowledge::Subject::Flesh),
            crate::ai::faction::Faction::Anomaly,
            Scp610Seed(seed),
            Scp610Mutation::default(),
            // At spawn, never toggled: `anim::attach_pose_blenders` installs the `PoseBlender` on the
            // streamed-in model's `AnimationPlayer` by walking up to the nearest `BlendSource`.
            crate::anim::BlendSource { graph: anim.graph.clone(), slots: anim.slots.clone() },
            // Authored at real scale with its base at y=0, so no render scale and no Y offset.
            Transform::from_translation(pos),
            Visibility::Inherited,
        ))
        .with_child((
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(SCP610_GLB))),
            Transform::from_scale(Vec3::splat(RENDER_SCALE)),
        ))
        .id()
}

/// Seed the level's blooms, one per room, spread across the level.
///
/// **Placed by ROOM, not by a raster scan over cells.** The first version scanned cells from (0,0) and
/// took the first N that passed the filters, which is deterministic and was also wrong: the first N
/// eligible cells all live in the map's low corner, so every bloom clustered there and most of a
/// 192x192 level never saw one. `SPAWN_MIN_DIST` does not fix that — it only excludes cells *near the
/// squad*, it does not distribute.
///
/// Striding the region list instead gives one bloom per evenly-spaced room across the whole level,
/// still with no RNG: `Dungeon::regions` is pinned generation output, so the same seed yields the same
/// rooms in the same order, and picking indices `0, n/k, 2n/k, ...` is a pure function of that.
///
/// A room is also the right unit for the fiction. Area denial means nothing in a corridor the squad can
/// back out of; it means something when the room *is* the objective.
fn spawn_scp610_blooms(
    mut commands: Commands,
    assets: Res<AssetServer>,
    dungeon: Res<crate::dungeon::Dungeon>,
    rules: Res<crate::containment::ContainmentRules>,
    mut seq: ResMut<crate::containment::TargetSeq>,
    anim: Res<Scp610Anim>,
) {
    let spawn = dungeon.spawn;

    // Rooms far enough from the squad's start that nobody opens a run inside one.
    let eligible: Vec<&crate::placement::ir::Region> = dungeon
        .regions
        .iter()
        .filter(|r| {
            let c = r.rect.center_cell();
            (IVec2::new(c[0], c[1]) - spawn).abs().max_element() >= SPAWN_MIN_DIST
        })
        .collect();
    if eligible.is_empty() {
        return;
    }

    // Even stride over the eligible rooms. Integer arithmetic, so it is exact and order-independent.
    let want = BLOOM_COUNT.min(eligible.len());
    for i in 0..want {
        let room = eligible[i * eligible.len() / want];
        let c = room.rect.center_cell();
        let cell = IVec2::new(c[0], c[1]);
        // A room's centre cell is floor by construction, but a notched room can hollow it out; fall
        // back to nothing rather than spawning a bloom inside rock.
        if !dungeon.is_floor(cell) {
            continue;
        }
        spawn_scp610_at(
            &mut commands,
            &assets,
            i as u32,
            dungeon.cell_center(cell),
            rules.0.scp610.clone(),
            &mut seq,
            &anim,
        );
    }
}

/// Advance each bloom's `mutation` weight and push it into the streamed-in model.
///
/// Cosmetic and windowed-only. The scene arrives asynchronously, so the `MorphWeights` holder is found
/// by walking descendants each frame rather than cached at spawn — caching it would mean writing a
/// component at a wall-clock-dependent tick, which is the churn `squad.rs`'s model-child split exists
/// to avoid.
fn drive_mutation(
    time: Res<Time>,
    mut blooms: Query<(Entity, &mut Scp610Mutation)>,
    children: Query<&Children>,
    mut weights: Query<&mut MorphWeights>,
) {
    let dt = time.delta_secs();
    for (root, mut m) in &mut blooms {
        m.current = (m.current + m.rate_per_sec * dt).clamp(0.0, 1.0);
        // EVERY descendant holding weights, not the first: the mesh has TWO primitives (body + eye),
        // and Bevy gives each its own `MorphWeights`. Driving only the first turned half the creature.
        let holders: Vec<Entity> = std::iter::once(root)
            .chain(children.iter_descendants(root))
            .filter(|e| weights.get(*e).is_ok())
            .collect();
        for holder in holders {
            let Ok(mut mw) = weights.get_mut(holder) else { continue };
            // Index **0**, not 1. `assets/scp610/README.md` §4 says "index 1 is `mutation`, index 0 is
            // Basis" — that is BLENDER shape-key numbering, where Basis is itself a key. glTF does not
            // work that way: Basis is the base mesh and `targets` holds only the deltas, so the file
            // carries exactly ONE target per primitive (verified: `targetNames: ['mutation']`,
            // `targets` length 1). Writing index 1 was always a `None` and the morph never applied —
            // three blooms authored at 0.0 / 0.5 / 1.0 rendered identically.
            if let Some(w) = mw.weights_mut().first_mut() {
                *w = m.current;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blooms must be spread over the level, not clustered.
    ///
    /// The first implementation raster-scanned cells from (0,0) and took the first N that passed its
    /// filters. That is deterministic — and it put every bloom in the map's low corner, because the
    /// first eligible cells in raster order are all adjacent. `SPAWN_MIN_DIST` did not catch it: it
    /// excludes cells near the *squad*, and says nothing about how far blooms are from each other.
    ///
    /// This pins the property that actually matters (spread), not the mechanism, so a future change to
    /// the stride is free as long as blooms still land in different parts of the level.
    #[test]
    fn blooms_are_spread_across_the_level_not_clustered() {
        // A synthetic region list standing in for a generated level: 12 rooms marching diagonally.
        let rooms: Vec<[i32; 2]> = (0..12).map(|i| [10 + i * 14, 10 + i * 14]).collect();
        let spawn = IVec2::new(0, 0);

        let eligible: Vec<[i32; 2]> = rooms
            .iter()
            .copied()
            .filter(|c| (IVec2::new(c[0], c[1]) - spawn).abs().max_element() >= SPAWN_MIN_DIST)
            .collect();
        let want = BLOOM_COUNT.min(eligible.len());
        let picked: Vec<IVec2> = (0..want)
            .map(|i| {
                let c = eligible[i * eligible.len() / want];
                IVec2::new(c[0], c[1])
            })
            .collect();

        assert_eq!(picked.len(), BLOOM_COUNT, "every bloom should find a room");
        // The failing case: a raster scan would have returned three adjacent rooms. Require the picks
        // to span most of the eligible range instead.
        let first = picked.first().expect("at least one bloom");
        let last = picked.last().expect("at least one bloom");
        let span = (*last - *first).abs().max_element();
        let full = (IVec2::from_array(*eligible.last().expect("eligible rooms"))
            - IVec2::from_array(*eligible.first().expect("eligible rooms")))
        .abs()
        .max_element();
        assert!(
            span * 2 >= full,
            "blooms span {span} of an available {full} — that is clustering, which is the bug this \
             replaced (raster order put all three in one corner)"
        );
    }
}

/// Hold the single idle slot at full weight.
///
/// Trivial today because there is one clip, but it is the seam the rest of the roster grows through:
/// when the bloom gains states, this is where `set_targets` picks between them.
fn drive_scp610_animation(mut blooms: Query<&mut crate::anim::PoseBlender, With<Scp610>>) {
    for mut blender in &mut blooms {
        // A slot-count mismatch is a wiring bug, not a runtime condition — report, do not mask.
        if let Err(e) = blender.set_targets(&[1.0]) {
            warn_once!("scp610: pose blend rejected: {e}");
        }
    }
}
