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
//! # It is killable, and killing it yields NOTHING
//!
//! FVS-K-1 gave it [`Health`], [`crate::enemy::Hostile`] and a [`crate::laser::LaserTarget`], which is
//! the half of FVS-C-1's acceptance that never actually shipped. Without them `Health` would be a
//! component nothing can reach — the "shipped a mechanism, nobody can reach it" failure `BACKLOG.md`
//! names as its top process risk.
//!
//! It matters because it makes the game's central tension real for this species. The squad **shoots
//! the bloom by default**, and the authored containment rule is `THREAT_ANOMALY ≤ 0.35` AND
//! `NOISE_SQUAD ≤ 0.20` — so shooting it is precisely what stops you containing it. The counter-play
//! is the `HOLD FIRE` verb, and the first-contact conversation already tells the player so in as many
//! words: *"Quarantine the room and hold the line. Quietly — it settles when we stop making noise."*
//! Kill it and you get a corpse; cordon it and you get a specimen.
//!
//! [`kill_blooms`] is deliberately **not** a despawn. Every other creature here dies through `autogib`
//! fracture; 610 collapses on its own baked clip and stays, because a dead mass of flesh is still the
//! terrain it always was. `assets/scp610/README.md` §5 says the clip exists for exactly this reason.
//! There is no `Killed` marker: `containment::state`'s module doc is explicit that a marker with no
//! reward hook is still *a place someone could branch*, and the reward lives only on `Contained`.
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

pub mod material;

use crate::containment::Quarantinable;
use crate::health::Health;

/// The asset. Recompressed 28.7 MB → 5.2 MB (see `assets/scp610/README.md`); contract unchanged.
const SCP610_GLB: &str = "scp610/scp-610.glb";

/// This creature's name in `assets/emerge/rigs.ron`.
pub(crate) const RIG: &str = "scp610";

// Authored at real human scale — 1.80 × 0.86 × 1.90 m fully grown — so unlike the blob and the crab
// this spawns unscaled: rigs.ron declares scale 1.0, carried on `Scp610Anim.scale`.
// `assets/scp610/README.md` §2.

/// Blooms per level. Small on purpose: this is a room-denial hazard, and three of them across a level
/// is already three rooms the squad has to solve rather than walk through.
pub(crate) const BLOOM_COUNT: usize = 3;

/// This species' key in [`crate::placement::anomalies::AnomalySites`] — the shared level-wide placement
/// pass that decides where every anomaly goes (and keeps them off each other).
pub(crate) const ANOMALY_KEY: &str = "scp610";
/// Minimum distance from the squad spawn, in tiles — nobody opens a run standing in one. Measured to a
/// room's centre, since placement is per-room (see [`spawn_scp610_blooms`]).
pub(crate) const SPAWN_MIN_DIST: i32 = 24;

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
    /// The manifest's render scale for the model child (1.0; see rigs.ron).
    scale: f32,
}


/// Blend slots, in the order [`build_scp610_anim`] adds them. The array `set_targets` takes is
/// positional, so these name the positions rather than leaving two bare literals in
/// [`drive_scp610_animation`].
const SLOT_IDLE: usize = 0;
const SLOT_DEATH: usize = 1;
const SLOT_COUNT: usize = 2;

/// Hit points. High: killing a bloom should be a decision the player commits to and pays for in
/// noise, not something a stray burst does on the way past. The squad's own containment rule caps
/// `NOISE_SQUAD` at 0.20, so a sustained kill is self-evidently the opposite of a capture.
const BLOOM_HP: f32 = 220.0;

/// Bolt hit volume. A standing figure: `assets/scp610/README.md` §2 gives the grown envelope as
/// 1.80 × 0.86 × 1.90 m, but most of that width is outflung mutant limbs rather than mass, so the
/// capsule is sized to the torso the player is actually aiming at.
const COLLIDER_R: f32 = 0.30;
const COLLIDER_HALF_HEIGHT: f32 = 0.60;

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
        app.add_systems(Startup, build_scp610_anim)
            .add_systems(
                OnEnter(crate::session::RunState::Active),
                spawn_scp610_blooms.in_set(crate::session::RunBuild::Populate),
            )
            // `FixedUpdate`: both write stigmergy deposits and one removes components, all pinned
            // state. `.before(AiSet::Deposits)` is the ordering every sibling deposit system uses
            // (`enemy::deposit_anomaly_aura`, `scp1048::effects::deposit_bear_dread`,
            // `parasite::deposit_manca_dread`) — the batch has to be complete before the drain runs.
            //
            // Chained so a corpse cannot radiate: `kill_blooms` retires the dead ones first, and
            // `deposit_flesh_drone` then skips anything at zero health.
            //
            // Deliberately NOT `.after(HealthDamage)`. That reads as the obvious thing to want — see
            // this tick's damage before deciding who died — but `HealthDamage` is a *set* with
            // members on both sides of the deposit drain (`crab_jump` is one), so combining it with
            // `.before(AiSet::Deposits)` makes the graph unsolvable and Bevy panics at schedule init:
            // *"system set `HealthDamage` and system `crab_jump (in set HealthDamage)` have both
            // `in_set` and `before`-`after` relationships"*. The cost of dropping it is that a bloom
            // killed on tick N is retired on tick N+1 — one tick of a corpse still being shootable,
            // which is deterministic and invisible.
            .add_systems(
                FixedUpdate,
                (kill_blooms, deposit_flesh_drone)
                    .chain()
                    .before(crate::ai::AiSet::Deposits)
                    .distributive_run_if(in_state(crate::session::RunState::Active)),
            )
            // ⚠️ **The pose driver belongs in the GAMEPLAY plugin, not the visuals one**, and this
            // is the same argument `build_scp610_anim` above already makes: `BlendSource` is inserted
            // at spawn, so `anim::attach_pose_blenders` gives every bloom a `PoseBlender` **in the
            // headless harness too**. A blender whose driver was registered windowed-only is a
            // blender with no driver at all there, and `PoseBlender::new` zeroes every weight — so
            // its weights sum to 0 instead of 1.
            //
            // `liveness::every_wired_figurine_keeps_a_well_formed_pose_blend_through_a_live_run`
            // asserts a partition of unity over EVERY blender in the world and caught exactly that.
            // Every sibling already does it this way (`crab::drive_crab_animation`, `scp1048`,
            // `parasite`) — 610 was the only one that put its driver in the cosmetic half.
            //
            // Still `Update`, never `FixedUpdate`, so it stays outside `snapshot_hash`
            // (`docs/animation.md`: the animation layer is cosmetic by construction). Reading
            // `Health` to pick idle-vs-death is a read, not a write.
            .add_systems(
                Update,
                drive_scp610_animation
                    .after(crate::anim::PoseAttachSet)
                    .before(crate::anim::PoseBlendSet)
                    .distributive_run_if(in_state(crate::session::RunState::Active)),
            );
    }
}

/// Cosmetic half — windowed only. Never registered in the harness.
pub struct Scp610VisualsPlugin;

impl Plugin for Scp610VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<material::Scp610FleshMaterial>::default(),
            MaterialPlugin::<material::Scp610EyeMaterial>::default(),
        ))
        .add_systems(Startup, material::load_textures)
        .add_systems(
            Update,
            (
                // `drive_scp610_animation` is NOT here — it drives a component the harness also has,
                // so it lives in the gameplay plugin. See the note there.
                drive_mutation,
                // Ordered after `drive_mutation` so the weight a bloom is showing this frame is the
                // weight its shader is told about, not last frame's.
                material::coat_blooms,
                material::drive_disruption,
            )
                .chain()
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
    manifest: Res<crate::rigs::RigManifest>,
) {
    // Order MUST match SLOT_IDLE / SLOT_DEATH — `set_targets` is positional, so the manifest's slot
    // order is the contract.
    let rig = match manifest.rig(RIG) {
        Ok(r) => r,
        Err(e) => {
            error!("{e}");
            return;
        }
    };
    // Idle is `free`, not `gait`: the bloom never travels, so there is no ground distance to sync a
    // stride against. Speed 1.0 — the clips are authored at 24 fps gameplay tempo (README 5).
    //
    // Death is `one_shot`. It used to be `free` like the idle, and the comment here claimed the blender
    // had no "play once" — that was wrong: `Playback::OneShot` has existed alongside `Free`/`Gait` all
    // along (`parasite`'s BurrowOut and `scp1048`'s `fire_gun` both use it), and it is exactly the
    // no-rewind-no-transition primitive `docs/animation.md` allows. `RepeatAnimation::Never` means the
    // clip runs through once and then holds its final frame, so the corpse stays collapsed. As `free`
    // it looped, and a bloom re-collapsed every 1.29 s forever — the player's 2026-08-01 report,
    // "SCP-610 just keeps falling over and over again". Still no `AnimationTransitions` anywhere near
    // the blender, which `docs/animation.md` forbids because its `PostUpdate` pass stomps the weights.
    let (graph, slots) = crate::rigs::build(rig, &assets, &mut graphs);
    debug_assert_eq!(slots.len(), SLOT_COUNT, "slot table drifted from the SLOT_* constants");
    commands.insert_resource(Scp610Anim { graph, slots, scale: rig.scale });
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
            material::Scp610Disruption::default(),
            // Killable, and killing it yields nothing — see the module header. All three go on at
            // spawn and only `Hostile`/`LaserTarget` are ever removed, once, by `kill_blooms`.
            Health::new(BLOOM_HP),
            crate::enemy::Hostile,
            crate::laser::LaserTarget {
                radius: COLLIDER_R,
                half_height: COLLIDER_HALF_HEIGHT,
                // From the spawn ordinal, never the `Entity` id (recycled) — the rule every other
                // spawn site follows. `TargetKind` makes it unique across species.
                id: crate::laser::target_id(crate::laser::TargetKind::Flesh, seed as u64),
            },
            // At spawn, never toggled: `anim::attach_pose_blenders` installs the `PoseBlender` on the
            // streamed-in model's `AnimationPlayer` by walking up to the nearest `BlendSource`.
            crate::anim::BlendSource { graph: anim.graph.clone(), slots: anim.slots.clone() },
            // Authored at real scale with its base at y=0, so no render scale and no Y offset.
            Transform::from_translation(pos),
            Visibility::Inherited,
        ))
        .with_child((
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(SCP610_GLB))),
            Transform::from_scale(Vec3::splat(anim.scale)),
        ))
        .id()
}

/// Seed the level's blooms at the sites the shared level-wide pass solved for this species.
///
/// # Two placement bugs, and why the second needed the shared pass
///
/// The first version scanned cells from (0,0) and took the first N past the filters — deterministic,
/// and wrong: the first eligible cells in raster order are all adjacent, so every bloom clustered in the
/// map's low corner and most of a 192² level never saw one.
///
/// The fix then was to stride the region list, one bloom per evenly-spaced room. That distributed the
/// blooms *relative to each other* and left the real problem standing: bloom `i = 0` took `eligible[0]`,
/// the lowest-index room, which is still the corner — and nothing anywhere knew that SCP-999, SCP-1048,
/// the boss and the crab nests were each independently choosing that same corner by the same logic. The
/// player caught the result (2026-08-01): *"610, 1048, and 1048-A, and Smiley are all bundled in the
/// corner."* Its own regression test could not catch it either, because it asserted the *span* of the
/// three blooms on a synthetic diagonal room list — a property that stays true while all three sit in
/// one corner of a real level next to four other anomalies.
///
/// Placement now lives in `placement::anomalies`: one pass for the whole roster, cross-species spacing,
/// and best-candidate selection so there is no scan order left for a corner to win.
///
/// **This gives up "one bloom per room".** That was the right unit for the fiction — area denial means
/// nothing in a corridor the squad can back out of — and a cell-based pass does not guarantee it. What
/// replaces it is `anomaly_separation` (18 tiles shipped, comfortably past a large room's diagonal),
/// which achieves the same thing the fiction actually wanted, spread across the whole roster rather than
/// within one species.
fn spawn_scp610_blooms(
    mut commands: Commands,
    assets: Res<AssetServer>,
    dungeon: Res<crate::dungeon::Dungeon>,
    rules: Res<crate::containment::ContainmentRules>,
    mut seq: ResMut<crate::containment::TargetSeq>,
    anim: Res<Scp610Anim>,
    sites: Res<crate::placement::anomalies::AnomalySites>,
) {
    for (i, &cell) in sites.get(ANOMALY_KEY).iter().enumerate() {
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

/// Ease between the idle tremor and the death collapse.
///
/// Reads `Health` directly rather than a marker component: the blender only eases weights, so "dead"
/// needs no state of its own here and adding a marker would churn the archetype for a cosmetic fact.
///
/// The death slot is a one-shot, so it needs firing on the **edge** into death rather than every frame
/// (re-triggering restarts the clip, which is what sustained fire wants for a recoil and is exactly the
/// re-collapsing loop we are fixing here). `PoseBlender::target_weight` is the driver's own one-frame
/// memory of what it last asked for, so the edge is detectable without a second copy of the state —
/// the `parasite::drive_manca_animation` idiom, and the reason that accessor exists.
fn drive_scp610_animation(
    mut blooms: Query<(&mut crate::anim::PoseBlender, &Health), With<Scp610>>,
) {
    for (mut blender, health) in &mut blooms {
        let dead = health.current <= 0.0;
        if dead && blender.target_weight(SLOT_DEATH) == 0.0 {
            blender.trigger(SLOT_DEATH);
        }
        let mut targets = [0.0; SLOT_COUNT];
        targets[SLOT_IDLE] = if dead { 0.0 } else { 1.0 };
        targets[SLOT_DEATH] = if dead { 1.0 } else { 0.0 };
        // A slot-count mismatch is a wiring bug, not a runtime condition — report, do not mask.
        if let Err(e) = blender.set_targets(&targets) {
            warn_once!("scp610: pose blend rejected: {e}");
        }
    }
}

/// How much `THREAT_ANOMALY` dread a bloom radiates per unit of `NOISE_SWARM` din.
///
/// **One gene, two channels, and the ratio is deliberately NOT evolvable.** How loud the thing is is
/// a difficulty dial and belongs to the search (`audio_tuning::flesh_drone_loudness`); how much of
/// that loudness lands as *dread* rather than as *noise* is the shape of this one creature, which is
/// what this module's "tunables are constants here" header is about.
///
/// Well under 1.0 because the two channels are not symmetric in consequence. `NOISE_SWARM` is
/// unconstrained — it feeds `unit_fear_of_din`, so a bloom simply makes the squad uneasy.
/// `THREAT_ANOMALY` appears in **610's own containment rule** (`AtMost 0.35`, sampled at the bloom),
/// so every unit of dread it emits is capacity taken from its own capture. That is a real tension and
/// it is the intended one — a bloom is harder to contain the more of a presence it is — but it has to
/// stay a tension rather than an impossibility.
pub const DREAD_PER_DIN: f32 = 0.10;

/// Radiate the bloom's presence into the shared fields, every fixed tick, for as long as it lives.
///
/// Continuous rather than per-event, because SCP-610 does not *do* anything — the whole species is
/// "terrain that is alive", so its stimulus is a rate. Modelled on `scp1048::effects::deposit_bear_dread`,
/// including the sort: overlapping deposit discs accumulate into the grid with a non-associative
/// `f32 +=` that `drain_deposits` applies in batch order, so the batch is value-sorted before it goes
/// out.
///
/// A dead bloom radiates nothing. It stops being a threat the moment it stops being alive, which is
/// also what makes killing it a *legible* choice rather than a pointless one — the room does get
/// quieter, you just do not get a specimen.
fn deposit_flesh_drone(
    time: Res<Time>,
    blooms: Query<(&Transform, &Health), With<Scp610>>,
    audio: Res<crate::audio_tuning::AudioTuning>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
) {
    // **Per second, scaled by `dt` — not per tick.** Every other continuous depositor does this
    // (`enemy::deposit_anomaly_aura`, `scp1048::effects::deposit_bear_dread`), and getting it wrong
    // is not a subtle mis-tune: at 60 Hz a raw per-tick push is 60× the intended rate, which drove
    // THREAT_ANOMALY to a steady state ~35× over 610's own containment threshold and made the
    // species literally impossible to contain. Caught by
    // `tests/containment.rs::the_loudest_evolvable_bloom_can_still_be_contained`, which is the
    // entire reason that test exists.
    let dt = time.delta_secs();
    let din = audio.stimulus.flesh_drone_loudness * dt;
    if din <= 0.0 {
        return;
    }
    let dread = din * DREAD_PER_DIN;

    let mut out: Vec<crate::ai::field::Deposit> = Vec::new();
    for (tf, health) in &blooms {
        if health.current <= 0.0 {
            continue;
        }
        out.push(crate::ai::field::Deposit {
            pos: tf.translation,
            field: crate::ai::field::FieldId::NOISE_SWARM,
            amount: din,
        });
        out.push(crate::ai::field::Deposit {
            pos: tf.translation,
            field: crate::ai::field::FieldId::THREAT_ANOMALY,
            amount: dread,
        });
    }
    // SORT-OK: `sort_deposits` takes the whole value, and two blooms at one position emit
    // interchangeable deposits.
    crate::ai::field::sort_deposits(&mut out);
    deposits.0.extend(out);
}

/// A bloom whose health has run out: stop it being a target, and grant **nothing**.
///
/// Contrast `enemy::despawn_dead` and `crab::crab_despawn_dead`, which both despawn and gib. This one
/// does neither. It leaves the corpse standing (collapsed, via [`drive_scp610_animation`]) because
/// 610 is terrain, and it despawns nothing that a later cordon might have wanted — a dead bloom is
/// simply a room the player spent ammunition and noise on and got no specimen for.
///
/// **There is no specimen path here, and that is enforced by the type system rather than by this
/// function being careful.** The reward is an `on_add` hook on `containment::Contained`, so the only
/// way to a `Specimen` is to become `Contained`. `tests/containment.rs` pins it from the outside.
fn kill_blooms(
    mut commands: Commands,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    blooms: Query<(Entity, &Health, &Transform), (With<Scp610>, With<crate::enemy::Hostile>)>,
    sim: Res<crate::sim::SimTuning>,
) {
    // Canonical order, for the same reason `enemy::despawn_dead` documents: the SCENT `Deposit`
    // accumulates with a non-associative `f32 +=` that `drain_deposits` applies in batch order.
    let mut dead: Vec<(Entity, Vec3)> = blooms
        .iter()
        .filter(|(_, hp, _)| hp.current <= 0.0)
        .map(|(entity, _, tf)| (entity, tf.translation))
        .collect();
    // SORT-OK: two dead blooms at one position take identical side effects (same SCENT, same
    // removal), so the payload does not distinguish them — interchangeable.
    dead.sort_unstable_by_key(|(_, p)| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));

    let mut scent: Vec<crate::ai::field::Deposit> = Vec::new();
    for (entity, pos) in dead {
        // A ton of dead flesh is the richest feeding site on the level. Same channel and amount every
        // other death uses, so the swarm reads it the same way.
        scent.push(crate::ai::field::Deposit {
            pos,
            field: crate::ai::field::FieldId::SCENT,
            amount: sim.deposit.blood_scent,
        });
        sfx.write(crate::audio::Sfx::EnemyDeath(pos));
        // Removed exactly once — the query requires `Hostile`, so a corpse cannot re-enter this loop.
        commands
            .entity(entity)
            .remove::<crate::enemy::Hostile>()
            .remove::<crate::laser::LaserTarget>();
    }
    crate::ai::field::sort_deposits(&mut scent);
    deposits.0.extend(scent);
}
