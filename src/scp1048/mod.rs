//! **SCP-1048 "Builder Bear"** — the benign original and the three hostile copies it assembles.
//!
//! One module owns all four bears behind one marker plus a variant enum, as the asset hand-off docs
//! (`assets/scp1048*/README.md`) recommend: they share an 8-bone rig, a clip vocabulary, a movement
//! model and a strike cadence, and differ only in *attack expression* — which is a code branch on
//! [`Scp1048Variant`], not a separate creature.
//!
//! The loop that makes the family interesting is **replication**. The original is not a threat and
//! cannot be shot; it wanders, dances when it is being watched, and builds a hostile copy when it is
//! not. So the fantasy the player actually plays against is *the bear you cannot shoot is building
//! the ones you must*, and the counter is simply to keep eyes on it.
//!
//! ## What each variant carries, and why the archetypes differ
//!
//! Every component is inserted **at spawn and never toggled** — a flipped marker would split the
//! hashed archetype and make ECS iteration order run-dependent. Where a variant needs different
//! state, that is a value field ([`Scp1048State`], [`Scp1048Build`]), following the `MancaMood` /
//! `Infestation` idiom in `crate::parasite`.
//!
//! The archetype does branch on the variant, in two places, both fixed at birth:
//!
//! - **`Hostile` on the copies only.** `laser::fire_laser` queries `With<Hostile>`, so a
//!   `LaserTarget` without `Hostile` is invisible to the raycast — there is no "targetable but not
//!   auto-engaged" middle ground. Giving the original `Hostile` would have the squad delete it on
//!   sight, and the replication mechanic would never run once. This is the same deliberate choice
//!   `crate::scp999` documents for the comfort blob.
//! - **`Biological` on A and B only.** SCP-1048-A is built from human ears and SCP-1048-B contains a
//!   human infant's arm, so Almond Water heals and poisons them; the plush original and C's rusted
//!   scrap are constructs and it ignores them. `crate::health::Biological` obliges every carrier to
//!   also carry `CyanideSmell`, whose stable per-spawn seed is supplied by [`Scp1048Seed`].
//!
//! That second split means A/B and C copies sit in **different archetypes**, so iteration order
//! between them is not stable across `App` instances. Nothing here may decide anything from query
//! order without a total sort — see the determinism notes on the replication spawner and the field
//! deposits.
//!
//! Deliberately absent on all four, for the same reason `scp999` records its omissions:
//! - no `Prey` — the crab swarm must not treat a teddy bear as food;
//! - no `Parasitizable` / `Infestation` — a manca cannot burrow into a plush toy.

use bevy::prelude::*;

use crate::dungeon::Dungeon;
use crate::sim::SimTuning;

pub mod anim;
pub mod behavior;
pub mod brain;
pub mod effects;
pub mod replicate;

pub use effects::EarGrowth;

/// Which of the four bears this is.
///
/// The three copies are grouped as "hostile" for everything except the one `match` in the behaviour
/// executor that picks an attack. Keep [`Scp1048Variant::ALL`] in discriminant order — [`index`] is
/// used to key the per-variant animation tables.
///
/// [`index`]: Scp1048Variant::index
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Scp1048Variant {
    /// SCP-1048 — the benign original. Not hostile, not shootable, and the only one that builds.
    Original,
    /// SCP-1048-A — built entirely of human ears, no face. Attacks with a shriek that grows ear
    /// tissue on everyone nearby.
    EarCopy,
    /// SCP-1048-B — a human infant's arm through a burst seam. Attacks with a looping tantrum.
    InfantArm,
    /// SCP-1048-C — rusted metal scrap with a crude gun fused over its right paw.
    Scrap,
}

impl Scp1048Variant {
    /// Every variant, in discriminant order.
    pub const ALL: [Scp1048Variant; 4] = [
        Scp1048Variant::Original,
        Scp1048Variant::EarCopy,
        Scp1048Variant::InfantArm,
        Scp1048Variant::Scrap,
    ];

    /// Dense index into the per-variant animation tables.
    pub const fn index(self) -> usize {
        match self {
            Scp1048Variant::Original => 0,
            Scp1048Variant::EarCopy => 1,
            Scp1048Variant::InfantArm => 2,
            Scp1048Variant::Scrap => 3,
        }
    }

    /// The asset path for this variant. Each bear ships its own glb with variant-prefixed clip names
    /// (`scp1048_*`, `scp1048a_*`, …) precisely so all four can be resident at once without
    /// animation-name collisions. Clip indices are pinned by `tests/creature_clip_contract.rs`.
    pub const fn glb(self) -> &'static str {
        match self {
            Scp1048Variant::Original => "scp1048/scp-1048.glb",
            Scp1048Variant::EarCopy => "scp1048a/scp-1048-a.glb",
            Scp1048Variant::InfantArm => "scp1048b/scp-1048-b.glb",
            Scp1048Variant::Scrap => "scp1048c/scp-1048-c.glb",
        }
    }

    /// Is this one of the three copies? Hostile bears get `Hostile` + `LaserTarget`; the original
    /// gets neither (see the module docs).
    pub const fn is_hostile(self) -> bool {
        !matches!(self, Scp1048Variant::Original)
    }

    /// Is this bear made of human tissue? A is human ears, B carries an infant's arm — Almond Water
    /// heals and poisons those two. The plush original and C's rusted scrap are constructs.
    pub const fn is_biological(self) -> bool {
        matches!(self, Scp1048Variant::EarCopy | Scp1048Variant::InfantArm)
    }
}

/// Marker for every bear, carrying its variant as a **value field** so the whole family shares one
/// component (and one query) rather than four markers.
#[derive(Component, Clone, Copy, Debug)]
pub struct Scp1048 {
    pub variant: Scp1048Variant,
}

/// Marker for the benign original alone.
///
/// Exists so the cosmetic plugin can register `fog::hide_in_fog::<Scp1048Benign>` for it: the copies
/// are already covered by the family-wide `hide_in_fog::<Hostile>` pass, and one fog writer per
/// entity is cleaner than two idempotent ones racing to write the same visibility.
#[derive(Component)]
pub struct Scp1048Benign;

/// Immortal per-instance decorrelation seed, assigned at spawn and never changed.
///
/// This is the **tiebreak key** for every total order in the module. It must never be derived from
/// the bear's position: a copy is built in its parent's own cell, so a position-derived key could not
/// break a position tie — which is exactly the trap the determinism rules in `CLAUDE.md` name.
#[derive(Component, Clone, Copy)]
pub struct Scp1048Seed(pub u32);

/// Which clip the bear should be showing. A **field** on [`Scp1048State`], never a component, so
/// changing it cannot migrate the archetype.
///
/// This is gameplay state, not a cosmetic cache: the behaviour executor advances it on `FixedUpdate`
/// and the animation driver only *reads* it on `Update`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnimState {
    /// Breathing/sway idle — the default for every variant.
    RestIdle,
    /// The canon "dances while observed" wiggle (original and B ship this clip).
    Dance,
    /// A hop in place.
    Jump,
    /// Folding down onto its bottom. Chains into [`AnimState::Draw`] on the original.
    SitDown,
    /// Seated arm-scribble — the original drawing a child-like picture.
    Draw,
    /// The hostile idle: a hunched, flailing threat display.
    Rage,
    /// The attack proper. Which clip that is depends on the variant: A screams, B throws a tantrum.
    Attack,
    /// SCP-1048-C only: the arm cannon raised to a level aim, held on its last frame.
    Aim,
    /// SCP-1048-C only: one shot. Starts and ends in the aim pose, so shots replay with no cross-fade.
    Fire,
    /// SCP-1048-C only: clubbing down with the scrap gun at close range.
    Whip,
}

/// Per-bear gameplay state. Always present on all four variants; fields simply stay inert for a
/// variant that cannot reach them (an original never aims a gun).
#[derive(Component, Clone, Copy, Debug)]
pub struct Scp1048State {
    /// The clip the animation driver should be blending toward.
    pub anim: AnimState,
    /// Counts down the *authored* length of a timed pose, so the state machine — not the animation
    /// layer — owns how long a one-shot is held. This is what makes the original's
    /// `sit_down` → `draw_picture` chain land on the right frame.
    pub phase_timer: f32,
    /// Seconds until this bear may strike again.
    pub strike_cd: f32,
    /// SCP-1048-C only: has the arm cannon been raised? Gates `aim_gun` (played once) from
    /// `fire_gun` (replayed per shot).
    pub aimed: bool,
    /// Set by [`behavior::scp1048_act`] on exactly the tick an attack connects, and consumed by
    /// [`effects`] the same tick.
    ///
    /// This is the seam that keeps *what pose to play* (the executor's job, and the one place the
    /// variants differ) apart from *what an attack does* (damage, dread, ear growths — each with its
    /// own determinism argument). It is a value field like everything else here, so it never migrates
    /// the archetype, and both systems run on `FixedUpdate` in a pinned order, so the handoff is
    /// deterministic.
    pub strike_landed: bool,
}

impl Scp1048State {
    /// A freshly spawned bear: idle, nothing timed, able to strike as soon as it decides to.
    pub fn new() -> Self {
        Scp1048State {
            anim: AnimState::RestIdle,
            phase_timer: 0.0,
            strike_cd: 0.0,
            aimed: false,
            strike_landed: false,
        }
    }
}

impl Default for Scp1048State {
    fn default() -> Self {
        Scp1048State::new()
    }
}

/// The original's construction economy — how much material it has scavenged, and whether it may
/// start another copy.
///
/// Present on **all four** variants, not just the original, so the family shares one archetype on
/// this axis: a copy's brain simply never selects `Mode::Build`, so its fields stay at their initial
/// values forever. That is the `Infestation` idiom — an always-present, inert-until-used value
/// field, rather than a component inserted when the bear first scavenges (which would migrate the
/// archetype mid-run and break determinism).
///
/// Building is gated on **not being observed**, which is the canon behaviour (no copy was ever seen
/// being assembled) and reuses the `seen_by_squad` perception the boss already computes. The
/// amplify-locally, no-central-plan shape is stigmergic construction (Khuong et al., "Stigmergic
/// construction and topochemical information shape ant nest architecture", PNAS 2016,
/// doi:10.1073/pnas.1509829113).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Scp1048Build {
    /// Material banked so far, capped at `sim.scp1048.build_cost`.
    pub materials: f32,
    /// Seconds remaining before another build may start.
    pub cooldown: f32,
    /// How many copies this bear has completed. Salts the variant draw so consecutive builds from one
    /// parent do not all come out the same.
    pub builds_done: u32,
}

impl Scp1048Build {
    /// Can a copy be assembled right now? Feeds `Fact::BuildReady`.
    pub fn ready(&self, build_cost: f32) -> bool {
        self.materials >= build_cost && self.cooldown <= 0.0
    }
}

// Render scale for the model child: rigs.ron declares 1.0 per variant — the asset's authored, canon
// size (~0.33 m tall). The asset docs sanction 1.3–1.6 if the bear reads too small at the RTS camera
// height; that decision stays one number in the manifest and never becomes an edit to the glb.

/// Half-extent used for wall-sliding movement (`Dungeon::resolve_move`).
pub const SCP1048_HALF: Vec2 = Vec2::splat(0.2);

/// Collider radius for laser targeting. Deliberately generous relative to the bear's 0.176 m width —
/// the same reasoning as `parasite::MANCA_COLLIDER_R`: a canon-sized 0.33 m plush is a very small
/// thing to expect a player to hit, so the shootable volume is larger than the silhouette.
pub const SCP1048_COLLIDER_R: f32 = 0.20;

/// Clamp per-frame dt so a hitch cannot tunnel a bear through a wall (mirrors `enemy`/`crab`/`scp999`).
pub(crate) const MAX_FRAME_DT: f32 = 1.0 / 30.0;

/// Minimum spacing between two seeded bears, so a `count > 1` world scatters them instead of stacking
/// them in one room. Matches `enemy::SPAWN_SEP`; the distance from the *squad* is the evolvable
/// `scp1048.spawn_min_dist`.
const SPAWN_SEP: f32 = 3.0;

/// This species' key in [`crate::placement::anomalies::AnomalySites`] — the shared level-wide placement
/// pass that decides where every anomaly goes (and keeps them off each other).
///
/// Only the **benign original** is placed by that pass. SCP-1048-A/B/C are *built* at runtime inside
/// their parent's own cell (`replicate.rs`, `SPAWN_JITTER` 0.35 m), which is deliberate — the copies
/// are made where the bear is. Spacing the original therefore spaces its whole brood.
pub(crate) const ANOMALY_KEY: &str = "scp1048";

/// Monotonic spawn counter — a unique, ever-increasing seed handed to each bear at birth.
///
/// Mirrors `parasite::MancaSpawnSeq`, and for the same reason: a bear built at runtime shares its
/// parent's cell, so a position-derived seed would hand siblings identical values. Never keyed on
/// `Entity` (ids are recycled and are not reproducible across runs of one seed).
#[derive(Resource, Default)]
pub struct Scp1048SpawnSeq(pub u32);

/// The bear's two `FixedUpdate` phases.
///
/// An explicit set rather than `.after(scp1048_act)`: naming a *system* in an ordering constraint
/// creates an implicit `SystemTypeSet` for it, and combining that with the system's own `in_set`
/// membership is unsolvable — Bevy rejects the schedule at startup with "a system cannot run before or
/// after a set it belongs to". Two named phases say the same thing and stay solvable.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scp1048Set {
    /// Motion and pose — [`behavior::scp1048_act`].
    Act,
    /// The consequences of a blow: dread, exposure, damage ([`effects`]).
    Effects,
    /// Scavenging and the assembly of a new copy ([`replicate`]). Last, so a copy born this tick
    /// starts acting next tick rather than half-way through this one.
    Replicate,
}

/// The gameplay half: seeding, the behaviour executor, and (from here on) the effects and replication.
///
/// Registered in BOTH `lib::run` and `sim_harness::build_headless_app`, because bears carry `Health`
/// and move on `FixedUpdate` — they are part of the hashed simulation.
pub struct Scp1048Plugin;

impl Plugin for Scp1048Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Scp1048SpawnSeq>()
            // Anim tables are assets (`Startup`); the bears populate the world (per-run, FVS-A-5).
            .add_systems(Startup, anim::build_scp1048_anim)
            .add_systems(OnEnter(crate::session::RunState::Active), spawn_scp1048.in_set(crate::session::RunBuild::Populate))
            // Act after Think, so the executor works from this tick's decision — the same ordering
            // every other creature's movement system uses. Effects then read the `strike_landed` flag
            // Act just set, and the dread must reach the grid before it is drained/evaporated.
            .configure_sets(
                FixedUpdate,
                (
                    Scp1048Set::Act.after(crate::ai::AiSet::Think),
                    Scp1048Set::Effects.after(Scp1048Set::Act),
                    Scp1048Set::Replicate.after(Scp1048Set::Effects),
                ),
            )
            .add_systems(
                FixedUpdate,
                (replicate::scp1048_scavenge, replicate::scp1048_replicate)
                    .chain()
                    .in_set(Scp1048Set::Replicate).distributive_run_if(in_state(crate::session::RunState::Active)),
            )
            .add_systems(FixedUpdate, behavior::scp1048_act.in_set(Scp1048Set::Act).distributive_run_if(in_state(crate::session::RunState::Active)))
            // Exposure reads the pose Act just set, so it belongs in Effects.
            .add_systems(FixedUpdate, effects::scp1048_scream_exposure.in_set(Scp1048Set::Effects).distributive_run_if(in_state(crate::session::RunState::Active)))
            // The dread deposit is deliberately NOT in `Effects`. The AI phase runs
            // Deposits → FieldUpdate → Drives → Think, so "after the executor" and "before this tick's
            // deposit drain" are contradictory — Bevy rejects that schedule outright. Emitting before
            // the drain therefore reads the *previous* tick's bear state, a one-tick lag on the dread.
            // That is exactly what `parasite::deposit_manca_dread` does and it is imperceptible: the
            // alternative (landing in the next tick's drain) would lag by one tick anyway, and this way
            // the dread is evaporated and diffused on the same pass it arrives.
            .add_systems(FixedUpdate, effects::deposit_bear_dread.before(crate::ai::AiSet::Deposits).distributive_run_if(in_state(crate::session::RunState::Active)))
            // The eighth and ninth links in the cross-plugin `HealthDamage` chain (see
            // `health::HealthDamage`). The explicit `.after(fire_laser)` is not decoration: several
            // writers hit one unit's `Health` on the same tick and float addition is not associative,
            // so leaving the order to plugin-registration accident is what the M1 re-pin recorded in
            // `tests/replay.rs` was about. Chained so asphyxiation always follows the blow.
            .add_systems(
                FixedUpdate,
                (effects::scp1048_strike_damage, effects::scp1048_asphyxiate)
                    .chain()
                    .after(crate::laser::fire_laser)
                    .after(Scp1048Set::Act)
                    .in_set(crate::health::HealthDamage).distributive_run_if(in_state(crate::session::RunState::Active)),
            )
            // The clip driver lives HERE, not in the windowed `Scp1048VisualsPlugin`, even though it is
            // cosmetic (`Update`, writes only blend weights). It has to: `spawn_scp1048` puts an
            // `anim::BlendSource` on the bear root, and `anim::attach_pose_blenders` — which IS in the
            // harness — then wires the streamed-in `AnimationPlayer` with **every slot at zero weight**.
            // If the only system that ever sets those targets were windowed-only, a bear headless would
            // hold a permanently undriven blender (weights summing to 0), which is what
            // `liveness::every_wired_figurine_keeps_a_well_formed_pose_blend_through_a_live_run` catches
            // — it fired the moment the bear's GLB finished streaming, so it presented as a flake.
            // Squad, crab and manca all register their clip drivers in their harness-visible creature
            // plugins for the same reason; this was the lone outlier.
            .add_systems(
                Update,
                anim::drive_scp1048_animation
                    .after(crate::anim::PoseAttachSet)
                    .before(crate::anim::PoseBlendSet).distributive_run_if(in_state(crate::session::RunState::Active)),
            );
    }
}

/// The cosmetic half: fog hiding. Windowed-only — registered in `lib::run` and **never** in
/// `sim_harness`. It writes `Visibility`, which is not `(Transform, Health)` on a hashed entity, so it
/// cannot perturb `snapshot_hash`.
pub struct Scp1048VisualsPlugin;

impl Plugin for Scp1048VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            // The hostile copies are already hidden by the shared `hide_in_fog::<Hostile>` pass,
            // so only the benign original names itself here — one fog writer per entity.
            crate::fog::hide_in_fog::<Scp1048Benign>.distributive_run_if(in_state(crate::session::RunState::Active)),
            );
    }
}

/// Seed `sim.scp1048.count` benign originals out in the level, at least `spawn_min_dist` tiles from
/// the squad spawn and `SPAWN_SEP` apart — the same greedy far-from-spawn raster every creature uses,
/// so every species seeds by one rule.
///
/// **Only originals are seeded.** The hostile copies are *built* during play; a level that started
/// with them would give away the mechanic the module exists for.
///
/// Deterministic by construction: a fixed raster scan over dungeon cells, no RNG and no query, so for
/// a given layout the bear starts in the same place every run and `spawn_min_dist` (evolvable) is what
/// moves it. Per-bear decorrelation comes from the monotonic [`Scp1048SpawnSeq`].
fn spawn_scp1048(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    assets: Res<AssetServer>,
    bear_anim: Res<anim::Scp1048Anim>,
    mut seq: ResMut<Scp1048SpawnSeq>,
    rules: Res<crate::containment::ContainmentRules>,
    mut targets: ResMut<crate::containment::TargetSeq>,
    sites: Res<crate::placement::anomalies::AnomalySites>,
) {
    if sim.scp1048.count == 0 {
        return;
    }
    // Sites come from the shared level-wide pass (`placement::anomalies`), which applied
    // `spawn_min_dist` and kept the bear clear of every OTHER anomaly. Spacing the original also spaces
    // its brood: 1048-A/B/C are built inside the parent's own cell (`replicate.rs`), so where the bear
    // stands is where the copies appear — which is why the player saw 1048 and 1048-A cornered together
    // with 610 and the boss. It warns on its own shortfall.
    let chosen: Vec<IVec2> = sites.get(ANOMALY_KEY).to_vec();
    if chosen.is_empty() {
        return;
    }

    for cell in &chosen {
        let s = seq.0;
        seq.0 += 1;
        spawn_scp1048_at(
            &mut commands,
            &assets,
            &bear_anim,
            &sim,
            s,
            dungeon.cell_center(*cell),
            Scp1048Variant::Original,
            rules.0.scp1048.clone(),
            &mut targets,
        );
    }
    info!("scp1048: seeded {} Builder Bear(s) out in the level", chosen.len());
}

/// Spawn one bear. **The single builder** — the Startup seeder, the runtime replicator and the
/// Research Room dev tool all funnel through it, so a built copy is byte-identical to a seeded one.
///
/// Every component lands here and none is ever toggled afterwards. The two archetype branches (see the
/// module docs) are both decided from `variant`, which is fixed at birth.
pub fn spawn_scp1048_at(
    commands: &mut Commands,
    assets: &AssetServer,
    bear_anim: &anim::Scp1048Anim,
    sim: &SimTuning,
    seed: u32,
    pos: Vec3,
    variant: Scp1048Variant,
    rule: crate::containment::ContainmentRule,
    targets: &mut crate::containment::TargetSeq,
) -> Entity {
    let table = bear_anim.get(variant);
    let mut ec = commands.spawn((
        crate::session::run_scoped(),
        Scp1048 { variant },
        Scp1048Seed(seed),
        Scp1048State::new(),
        Scp1048Build::default(),
        // Carries `Health` even though the original is unshootable, so the deterministic-core snapshot
        // folds every bear uniformly.
        crate::health::Health::new(sim.scp1048.hp),
        crate::health::NoHealthBar,
        // `Drives` + `Faction` are a mandatory pair: `validate_factions` panics on a `Drives` carrier
        // without one, because an untagged agent would silently never feel fear.
        crate::ai::drives::Drives::new(),
        crate::ai::faction::Faction::Bear,
        // The `think` contract: a brain id, a decision cache, and a seed-staggered timer so bears do
        // not all re-decide on the same frame.
        if variant.is_hostile() {
            crate::ai::brain::BrainId::BearCopy
        } else {
            crate::ai::brain::BrainId::Bear
        },
        crate::ai::brain::ActiveBehavior::new(seed),
        crate::ai::brain::ThinkTimer::staggered(seed),
        // Root is UNSCALED; the model child carries the render scale and the spawn yaw (issue #18).
        Transform::from_translation(pos),
        Visibility::Inherited,
        // Render-only: smooth the bear's 60 Hz movement across the display refresh.
        avian3d::prelude::TransformInterpolation,
    ));
    // FVS-C-3: the out-watch capture, plus the uniform aim key the player's throw resolves by.
    //
    // A SECOND `insert` rather than more tuple elements: the spawn above is already at Bevy's
    // 15-element cap. This is the same idiom `squad::spawn_unit` uses for
    // `parasite::host_infestation_bundle()`, and it reads better anyway — "and it is also containable"
    // is a separate statement about the bear.
    //
    // Both are attached in the SHARED builder, so a Research-Room F6 bear is byte-identical to a
    // seeded one, and both are value fields present from spawn, so the hashed archetype never churns.
    // `BuilderBear`, not `BearCopies`: this is the benign original. The hostile copies it builds are a
    // different subject entirely — an operative can rationally believe one is harmless and the other
    // lethal, and that distinction is the whole reason `knowledge::Subject` separates them.
    ec.insert((
        crate::containment::Containment::new(rule, crate::knowledge::Subject::BuilderBear),
        targets.next(),
    ));
    // Cosmetic animation wiring: `anim::attach_pose_blenders` finds this on the root when the scene's
    // `AnimationPlayer` streams in. Inserted at spawn, so it never churns the hashed archetype.
    ec.insert(crate::anim::BlendSource {
        graph: table.graph.clone(),
        slots: table.slots.clone(),
    });
    if variant.is_hostile() {
        ec.insert((
            crate::enemy::Hostile,
            crate::laser::LaserTarget {
                radius: SCP1048_COLLIDER_R,
                half_height: 0.0,
                id: crate::laser::target_id(crate::laser::TargetKind::Bear, seed as u64),
            },
        ));
    } else {
        ec.insert(Scp1048Benign);
    }
    // A is built from human ears and B carries an infant's arm, so the belief-water heals and poisons
    // those two; the plush original and C's rusted scrap are constructs and it ignores them. Every
    // `Biological` must also carry `CyanideSmell` (see `crate::health`).
    if variant.is_biological() {
        ec.insert((
            crate::health::Biological,
            crate::health::CyanideSmell::from_seed_in(
                crate::health::smell_seed::BEAR,
                seed as u64,
            ),
        ));
    }
    // The model child: authored facing is +Z and the game's forward is -Z, hence the 180° yaw.
    ec.with_child((
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(variant.glb()))),
        Transform::from_scale(Vec3::splat(table.scale))
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
    ));
    ec.id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_indices_are_dense_and_in_discriminant_order() {
        for (i, v) in Scp1048Variant::ALL.iter().enumerate() {
            assert_eq!(v.index(), i, "{v:?} is not at its discriminant position");
        }
    }

    #[test]
    fn exactly_one_variant_is_benign_and_it_is_the_one_that_builds() {
        let hostile: Vec<_> = Scp1048Variant::ALL.iter().filter(|v| v.is_hostile()).collect();
        assert_eq!(hostile.len(), 3, "the family is one original plus three copies");
        assert!(!Scp1048Variant::Original.is_hostile());
    }

    #[test]
    fn only_the_tissue_built_copies_are_biological() {
        // A is human ears, B carries an infant's arm; the plush original and C's scrap are not flesh.
        assert!(Scp1048Variant::EarCopy.is_biological());
        assert!(Scp1048Variant::InfantArm.is_biological());
        assert!(!Scp1048Variant::Original.is_biological());
        assert!(!Scp1048Variant::Scrap.is_biological());
    }

    #[test]
    fn every_variant_names_a_distinct_glb() {
        for (i, a) in Scp1048Variant::ALL.iter().enumerate() {
            for b in &Scp1048Variant::ALL[i + 1..] {
                assert_ne!(a.glb(), b.glb(), "{a:?} and {b:?} share an asset path");
            }
        }
    }

    #[test]
    fn build_readiness_needs_both_material_and_a_clear_cooldown() {
        const COST: f32 = 12.0;
        assert!(Scp1048Build { materials: COST, cooldown: 0.0, builds_done: 0 }.ready(COST));
        assert!(!Scp1048Build { materials: COST - 0.1, cooldown: 0.0, builds_done: 0 }.ready(COST));
        assert!(!Scp1048Build { materials: COST, cooldown: 1.0, builds_done: 0 }.ready(COST));
    }
}
