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
/// Minimum distance from the squad spawn, in tiles — nobody opens a run standing in one.
const SPAWN_MIN_DIST: i32 = 24;
/// Minimum separation between blooms, in tiles, so they read as separate sites.
const SPAWN_SEP: i32 = 18;

/// Seconds for a fresh bloom to go from "still looks like a person" to fully turned.
///
/// Slow on purpose. The morph is the tell that the thing in the corner *used to be someone*, and it
/// only lands if the player is present while it happens rather than arriving after the fact.
const MUTATION_SECS: f32 = 45.0;

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
        app.add_systems(
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
            drive_mutation.distributive_run_if(in_state(crate::session::RunState::Active)),
        );
    }
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

/// Seed the level's blooms by a deterministic raster scan — no RNG, the same idiom
/// `scp999::spawn_scp999`, `enemy::spawn_enemies` and `crab::setup::spawn_crabs` use.
fn spawn_scp610_blooms(
    mut commands: Commands,
    assets: Res<AssetServer>,
    dungeon: Res<crate::dungeon::Dungeon>,
    rules: Res<crate::containment::ContainmentRules>,
    mut seq: ResMut<crate::containment::TargetSeq>,
) {
    let spawn = dungeon.spawn;
    let mut placed: Vec<IVec2> = Vec::with_capacity(BLOOM_COUNT);

    for z in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            if placed.len() >= BLOOM_COUNT {
                break;
            }
            let cell = IVec2::new(x, z);
            if !dungeon.is_floor(cell) {
                continue;
            }
            // A bloom belongs in a room, not a corridor: area denial means nothing in a passage the
            // squad can simply back out of.
            if dungeon.is_corridor(cell) {
                continue;
            }
            if (cell - spawn).abs().max_element() < SPAWN_MIN_DIST {
                continue;
            }
            if placed.iter().any(|p| (*p - cell).abs().max_element() < SPAWN_SEP) {
                continue;
            }
            let seed = placed.len() as u32;
            spawn_scp610_at(
                &mut commands,
                &assets,
                seed,
                dungeon.cell_center(cell),
                rules.0.scp610.clone(),
                &mut seq,
            );
            placed.push(cell);
        }
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
        let Some(holder) = std::iter::once(root)
            .chain(children.iter_descendants(root))
            .find(|e| weights.get(*e).is_ok())
        else {
            continue; // scene still streaming in
        };
        if let Ok(mut mw) = weights.get_mut(holder) {
            // Target index 1 is `mutation`; index 0 is `Basis` and is never driven directly
            // (`assets/scp610/README.md` §4).
            if let Some(w) = mw.weights_mut().get_mut(1) {
                *w = m.current;
            }
        }
    }
}
