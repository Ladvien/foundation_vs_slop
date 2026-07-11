//! Headless, deterministic simulation harness (feature `test-harness`).
//!
//! Drives the *real* game plugins off-screen so replay / liveness tests can run the exact production
//! simulation without a window, at a controllable speed, and — above all — **reproducibly from a seed**.
//! Fixed-timestep repeatability is the documented precondition for same-seed replay (Bécares &
//! Gómez-Martín 2017, "An approach to automated videogame beta testing", §9): the harness advances a
//! *fixed* `Time` delta per `step`, so the simulation never sees variable frame pacing.
//!
//! Render is brought up head-lessly (no window, Winit disabled) and **with no wgpu backend** rather than
//! stripped, so every game plugin — including the custom-material ones — runs unmodified against a
//! registered render world that never creates an adapter, device, or queue. Visual output is simply
//! absent. Simulation state is deterministic regardless: rendering reads sim state, never writes it, and
//! the snapshot excludes all visual/physics-gib components. See the `RenderPlugin` note in
//! [`build_headless_app_unfinished`] for the measurement that admitted this.

use bevy::prelude::*;
use std::sync::{Mutex, MutexGuard};

/// Process-wide lock serializing headless harness runs. Two `App`s running concurrently in one test
/// process share Bevy's global compute-task pool **and** the GPU device; that contention makes their
/// otherwise-deterministic runs interfere (the same-seed hashes diverge). Each is individually
/// reproducible — they must simply not overlap. Hold [`serial_guard`] for a harness `App`'s whole
/// lifetime and determinism holds regardless of the test runner's `--test-threads`.
static HARNESS_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the harness serialization lock (see [`HARNESS_LOCK`]). Poison-tolerant: a panicking test still
/// releases a usable lock to the next one.
pub fn serial_guard() -> MutexGuard<'static, ()> {
    HARNESS_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Knobs for a harness run.
pub struct SimConfig {
    /// Simulation step in seconds. The whole pinned sim advances by exactly this each `step` tick.
    pub fixed_dt: f32,
    /// Wall-speed multiplier for headless runs, realized by advancing **real** time manually: each
    /// `app.update()` advances `fixed_dt * speed`, so `speed` fixed sub-steps run per update (see the
    /// `TimeUpdateStrategy::ManualDuration` setup in [`build_headless_app`]). Deliberately **not** via
    /// `Time<Virtual>::set_relative_speed` — that clock is owned in the windowed game by the single
    /// writer `juice::tick_hitstop`, and driving real time here sidesteps its per-frame re-assertion.
    /// Does NOT affect determinism: each fixed sub-step still sees exactly `fixed_dt`, so the step
    /// sequence is identical at any speed.
    pub speed: f32,
    /// Include the Avian physics layer (gib chunks). Physics floats are **not** bit-reproducible (a
    /// documented invariant — see `CLAUDE.md` / the testing strategy), so a run with `physics = true` is
    /// only fit for *liveness/tolerance* oracles, never an exact same-seed hash. `physics = false` runs
    /// the gameplay LOGIC (AI, movement, combat, economy) with no solver, which IS bit-reproducible and
    /// is what the exact same-seed replay pins.
    pub physics: bool,
    /// Which brains the simulation runs. `Authored` is the shipped game; `Candidate` installs a genome
    /// under evaluation by the offline behaviour search. Inserted as a resource *before* `AiPlugin` and
    /// `SquadAiPlugin` build, so their `init_resource`/`Startup` systems pick it up.
    pub brains: crate::ai::brain::BrainSource,
    /// Override the dungeon generation seed shipped in `assets/config/config.ron`. `None` runs the
    /// shipped world (what the replay goldens pin); `Some(s)` generates a different one.
    ///
    /// This is a knob, not a fallback: exactly one seed reaches `Dungeon::generate` on either setting,
    /// and a bad seed still fails loudly there. It exists because a behaviour search that only ever sees
    /// one map learns that map — the offline squad/swarm search (`squad_ai::qd`) evaluates every genome
    /// across a held-in seed set and validates on a held-out one.
    pub dungeon_seed: Option<u64>,
    /// Override the evolvable world-dynamics config: the field-propagation tuning (`ai_tuning`) plus the
    /// simulation-dynamics tuning (`sim`). `None` runs the shipped slices (what the replay goldens pin);
    /// `Some(w)` installs an evolved world for one rollout. Like `dungeon_seed`, this is a knob applied at
    /// the single `GameConfig` seam before the consumer plugins build — exactly one config reaches them.
    pub config: Option<crate::config::WorldConfig>,
    /// Override the acoustic-stimulus + audio tuning (`audio:` slice). `None` runs the shipped slice (what
    /// the replay goldens pin); `Some(a)` installs an evolved `AudioTuning` for one rollout — the
    /// audio-population analogue of [`Self::config`], applied at the same `GameConfig` seam before
    /// `AiPlugin` reads `gc.audio` into its resources.
    pub audio: Option<crate::audio_tuning::AudioTuning>,
}

impl Default for SimConfig {
    fn default() -> Self {
        // 1/60 s fixed step — the game's `MAX_FRAME_DT` clamp is 1/30, so this is a well-behaved sub-step.
        Self {
            fixed_dt: 1.0 / 60.0,
            speed: 1.0,
            physics: true,
            brains: crate::ai::brain::BrainSource::Authored,
            dungeon_seed: None,
            config: None,
            audio: None,
        }
    }
}

impl SimConfig {
    /// A physics-free configuration: the deterministic gameplay core, suitable for exact same-seed hashing.
    pub fn deterministic_core() -> Self {
        Self { physics: false, ..Self::default() }
    }

    /// The deterministic core on a specific generated world — one evaluation environment for the
    /// offline behaviour search.
    pub fn deterministic_core_seeded(dungeon_seed: u64) -> Self {
        Self { dungeon_seed: Some(dungeon_seed), ..Self::deterministic_core() }
    }

    /// Install a candidate genome for one evaluation rollout.
    pub fn with_brains(mut self, brains: crate::ai::brain::BrainSource) -> Self {
        self.brains = brains;
        self
    }

    /// Install an evolved world config for one evaluation rollout — the world-population analogue of
    /// [`with_brains`]. Applied at the same `GameConfig` seam as `dungeon_seed` (see `build_headless_app`).
    pub fn with_world_config(mut self, config: crate::config::WorldConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Install an evolved acoustic/audio config for one evaluation rollout — the audio-population
    /// analogue of [`with_world_config`]. Applied at the same `GameConfig` seam (see `build_headless_app`).
    pub fn with_audio_config(mut self, audio: crate::audio_tuning::AudioTuning) -> Self {
        self.audio = Some(audio);
        self
    }
}

/// Build a headless `App` running the full game simulation with no window, **before** `finish()`.
/// Split out so UI liveness tests can add `ui::UiPlugin` before finish (plugins can't be added
/// after). The determinism path uses [`build_headless_app`], which never adds the UI.
pub fn build_headless_app_unfinished(cfg: &SimConfig) -> App {
    // Force the GLOBAL compute pool to a single thread BEFORE any plugin touches it. Bevy runs parallel
    // queries / systems (and Avian's solver) on this pool; with >1 worker the float reductions and
    // command ordering are non-deterministic, so two same-seed runs diverge. `get_or_init` is a global
    // `OnceLock` — doing it here, ahead of `DefaultPlugins`' `TaskPoolPlugin`, guarantees ours wins and
    // every harness `App` in the process shares a 1-thread pool. This is the real single-threaded
    // deterministic mode (setting `TaskPoolPlugin::num_threads` alone loses the init race).
    let pool = bevy::tasks::ComputeTaskPool::get_or_init(|| {
        bevy::tasks::TaskPoolBuilder::new().num_threads(1).thread_name("fvs-sim".into()).build()
    });
    // Fail loud if we lost the init race. `get_or_init` is a no-op when the global pool already exists, so
    // if anything initialized it with the default (multi-thread) worker count first, our 1-thread request
    // is silently dropped and same-seed replay would diverge with no code change. Assert the shared pool is
    // truly single-threaded rather than let a silent multi-thread pool corrupt determinism.
    assert_eq!(
        pool.thread_num(),
        1,
        "headless harness requires a 1-thread ComputeTaskPool for determinism, but the global pool was \
         already initialized with {} thread(s) — something touched ComputeTaskPool before build_headless_app",
        pool.thread_num()
    );
    // Avian's solver and the placement MCMC use the global rayon pool, whose work-stealing float
    // reductions are timing-dependent (hence the flakiness). Pin rayon to one thread as well. Must be set
    // before rayon's global pool first initializes — this runs before any Startup system, so it wins.
    // SAFETY: single-threaded at this point (no other thread reads the environment concurrently).
    unsafe {
        std::env::set_var("RAYON_NUM_THREADS", "1");
    }

    let mut app = App::new();

    // DefaultPlugins, but headless: no primary window and the Winit event loop disabled so the app is
    // stepped manually with `app.update()` instead of `app.run()` handing control to winit.
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            // Single compute thread → deterministic system execution order. Bevy's multithreaded executor
            // is free to run unordered (non-conflicting) systems in any order across threads; with a
            // one-thread pool that ordering is fixed, so command/resource mutations that lack an explicit
            // `.before/.after` can't race. This is the harness's determinism guarantee (the plan's
            // "single-threaded deterministic mode").
            .set(bevy::app::TaskPoolPlugin {
                task_pool_options: bevy::app::TaskPoolOptions::with_num_threads(1),
            })
            // No wgpu backend: the render plugin still registers every render type (so the custom
            // `Material` plugins build and `Assets<StandardMaterial>` exists), but no adapter, device, or
            // queue is created and no GPU work is submitted.
            //
            // This is not a second code path — it is the *same* plugin graph with the device omitted.
            // It is sound because `snapshot_hash` covers `(Transform, Health)`, every writer of which
            // runs on `FixedUpdate`, while rendering only ever reads simulation state. The `FixedUpdate`
            // / `Update` split (see TESTING.md) is what enforces that, and `ui_never_leaks_into_
            // deterministic_core` guards the one plugin that would otherwise breach it.
            //
            // Verified when this landed: with a real Metal backend, seed `0x5C09191` × 1800 ticks at
            // speed 1 hashes to `716d0cfbb69b778e`; with `backends: None` it hashes to the same value,
            // and the whole replay + liveness suite passes unchanged. (The two cannot be compared inside
            // one test — the harness admits a single `App` per process — so this is a recorded
            // measurement, not an automated assertion.)
            //
            // Measured on an M5: step time for that episode falls 9.31 s → 3.18 s. Solving
            // `T = updates·R + steps·S` across `speed` ∈ {1,20} put ~84% of a headless run in
            // per-`update()` render-extract rather than simulation. That is what makes the offline
            // behaviour search (`squad_ai::genome`) affordable, and it drops the harness's GPU
            // requirement entirely — the replay/liveness suite now runs on a pure-CPU runner.
            .set(bevy::render::RenderPlugin {
                render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                    bevy::render::settings::WgpuSettings { backends: None, ..default() },
                )),
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>(),
    );

    // Physics (gib chunks only) — same scoping as `lib::run`. Gated: the Avian solver is the one part of
    // the sim that is not bit-reproducible, so exact same-seed replay runs with it OFF.
    if cfg.physics {
        app.add_plugins(avian3d::prelude::PhysicsPlugins::default());
        app.insert_resource(avian3d::prelude::Gravity(Vec3::NEG_Y * 18.0));
    }

    // ConfigPlugin first: it loads + validates the unified `assets/config/config.ron` and inserts
    // the `GameConfig` resource that every consumer plugin below reads at build time. Added on its own
    // so a `dungeon_seed` override can be applied to `GameConfig` *before* `DungeonPlugin::build` reads
    // it — that plugin generates the world eagerly at build time, so this is the only seam. Splitting
    // the tuple does not change plugin build order.
    app.add_plugins(crate::config::ConfigPlugin);
    if let Some(seed) = cfg.dungeon_seed {
        app.world_mut().resource_mut::<crate::config::GameConfig>().dungeon.seed = seed;
    }
    // Override the evolvable world-dynamics slices (field propagation + sim tuning) the same way, before
    // the consumer plugins read them at build (`AiPlugin` reads `ai_tuning` + `sim` into resources). `None`
    // runs the shipped config the goldens pin; `Some(w)` installs an evolved world for one rollout. Same
    // "mutate GameConfig at the seam" mechanism as `dungeon_seed` — one config reaches the consumers.
    if let Some(a) = cfg.audio {
        // Same seam as the world/dungeon overrides: install the evolved `audio:` slice before `AiPlugin`
        // reads `gc.audio` into `AudioTuning`/the channel defs/the din-fear gains. `None` → shipped slice.
        app.world_mut().resource_mut::<crate::config::GameConfig>().audio = a;
    }
    if let Some(w) = cfg.config {
        let mut gc = app.world_mut().resource_mut::<crate::config::GameConfig>();
        gc.ai_tuning = w.ai;
        gc.sim = w.sim;
    }
    // Insert BEFORE `AiPlugin`/`SquadAiPlugin`: their `init_resource::<BrainSource>()` is a no-op when the
    // resource already exists, so this is what selects authored-vs-candidate brains for the whole run.
    app.insert_resource(cfg.brains.clone());

    // The full game simulation, identical to production (see `lib::run`). Cosmetic plugins are included
    // too — they run harmlessly headless and keep the plugin graph identical, which matters because some
    // sim systems are ordered relative to them.
    app.add_plugins((
        (crate::dungeon::DungeonPlugin, crate::placement::PlacementPlugin),
        crate::world::WorldPlugin,
        crate::camera::CameraPlugin,
        // Squad movement AND squad AI — registered together, exactly as production `lib::run` does, so
        // the squad AI's pinned `FixedUpdate` systems (`update_anchor`, `squad_think`, `unit_actions`,
        // `medic_heal`) are exercised by the exact-hash determinism gate. This was previously deferred
        // because driving the cast harder surfaced a pre-existing fragility — cosmetic async GLTF scene
        // loads attaching `Children`/`SceneInstance` to *sim* actors — that churned archetypes and
        // shifted ECS iteration order between same-seed runs. That is now fixed at the root: the unit
        // figurine scene lives on a cosmetic child, not the `Unit` (see `crate::squad`, issue #18), so
        // the sim archetype is fixed at spawn and iteration order is stable.
        (crate::squad::SquadPlugin, crate::squad_ai::SquadAiPlugin),
        crate::selection::SelectionPlugin,
        crate::fog::FogPlugin,
        crate::health::HealthPlugin,
        (
            crate::ai::AiPlugin,
            crate::enemy::EnemyPlugin,
            crate::crab::CrabPlugin,
            crate::nest::NestPlugin,
        ),
        crate::laser::LaserPlugin,
        crate::impact_fx::ImpactFxPlugin,
        (
            crate::time_control::TimeControlPlugin,
            crate::juice::JuicePlugin,
            crate::gore::GorePlugin,
            crate::autogib::AutogibPlugin,
        ),
        crate::audio::GameAudioPlugin,
        (crate::vhs::VhsPlugin, crate::blood_lens::BloodLensPlugin),
    ));

    // Take control of the clock. Fixed timestep = `fixed_dt`; each `app.update()` advances REAL time by
    // `speed * fixed_dt`, so `speed` fixed sub-steps run per update (speed 1 ⇒ exactly one). Realising
    // speed through the manual real-time advance — rather than `Time<Virtual>::set_relative_speed` — is
    // deliberate: the `juice` hitstop system re-sets the virtual relative-speed every frame and would
    // clobber a harness setting, whereas it leaves REAL time alone. Both durations come from one f64 so
    // there is no sub-nanosecond accumulator drift.
    let fixed = std::time::Duration::from_secs_f64(cfg.fixed_dt as f64);
    let advance = std::time::Duration::from_secs_f64((cfg.fixed_dt as f64) * (cfg.speed.max(0.0) as f64));
    app.insert_resource(Time::<bevy::time::Fixed>::from_duration(fixed));
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(advance));

    app
}

/// Build + finish the headless deterministic-sim app (no UI) — the replay/determinism reference.
/// `ui::UiPlugin` must never be added here (guarded by `ui_never_leaks_into_deterministic_core`).
pub fn build_headless_app(cfg: &SimConfig) -> App {
    let mut app = build_headless_app_unfinished(cfg);
    // Run plugin `finish`/`cleanup` now (creates the headless render device etc.) so the render-world
    // resources every PBR system validates against exist before the first `step`. `update()` skips
    // re-running these once done.
    app.finish();
    app.cleanup();
    app
}

/// Advance the simulation by `ticks` fixed steps. Each `app.update()` advances the clock by exactly
/// `cfg.fixed_dt` (via `TimeUpdateStrategy`), so the run is independent of wall time.
pub fn step(app: &mut App, _cfg: &SimConfig, ticks: u32) {
    for _ in 0..ticks {
        app.update();
    }
}

/// A deterministic hash of the gameplay simulation state, **excluding physics gib chunks**. Every
/// gameplay actor (unit, enemy, crab, nest) carries a `Health` component; gib chunks do not — so a query
/// over `(&Transform, &Health)` captures exactly the pinned actors and naturally drops the
/// non-reproducible physics debris (whose float transforms must never be hashed). Rows are keyed and
/// sorted by the stable spawn-order entity index so the hash is order-independent, and floats are hashed
/// by exact bit pattern. This is the replay oracle: same seed ⇒ same hash.
pub fn snapshot_hash(app: &mut App) -> u64 {
    let world = app.world_mut();
    let mut query = world.query::<(&Transform, &crate::health::Health)>();
    // Capture PHYSICAL state only — position + health — deliberately NOT the entity id. Entity-id
    // allocation order can differ between two same-seed runs (spawn order isn't part of the observable
    // game state), so hashing ids would report a false divergence. Rows are sorted by their own value so
    // the hash is invariant to iteration/allocation order and reflects only what the world *looks like*.
    let mut rows: Vec<[u32; 5]> = query
        .iter(world)
        .map(|(t, h)| {
            [
                t.translation.x.to_bits(),
                t.translation.y.to_bits(),
                t.translation.z.to_bits(),
                h.current.to_bits(),
                h.max.to_bits(),
            ]
        })
        .collect();
    rows.sort_unstable();

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let feed = |bytes: &[u8], h: &mut u64| {
        for &b in bytes {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    // Row count first, so a spawn/despawn divergence is caught even if the survivors happen to collide.
    feed(&(rows.len() as u64).to_le_bytes(), &mut hash);
    for row in rows {
        for w in row {
            feed(&w.to_le_bytes(), &mut hash);
        }
    }
    hash
}

/// A deterministic hash of the stigmergy field grids — the determinism oracle `snapshot_hash` cannot
/// provide. `snapshot_hash` folds only actor Transform+Health, never a field cell, so a reordered diffusion
/// neighbour sum or a broken floor mask that doesn't happen to relocate an agent is invisible to it. This
/// folds every `Stig` channel cell and every `RallyField` vector (full grid, so rock-cells-stay-0 is pinned
/// too) plus the derived `saturation_stats`, into one FNV-1a hash. Same seed ⇒ same hash. Test-only.
#[cfg(feature = "test-harness")]
pub fn field_hash(app: &mut App) -> u64 {
    let world = app.world();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    if let Some(stig) = world.get_resource::<crate::ai::field::Stig>() {
        stig.fold_fingerprint(&mut hash);
    }
    if let Some(rally) = world.get_resource::<crate::ai::field::RallyField>() {
        rally.fold_fingerprint(&mut hash);
    }
    hash
}

/// Issue a squad move order toward `goal` (a dungeon cell): build one shared flow field and insert a
/// `MoveOrder` on every unit. This is the headless-safe way to drive the squad — it bypasses
/// `selection::command_input`, which needs a cursor + window the harness doesn't have (Bécares: replay
/// high-level *intentions*, not raw input). Returns `false` if `goal` is unreachable (no field built).
pub fn issue_squad_order(app: &mut App, goal: IVec2) -> bool {
    let world = app.world_mut();
    let field = {
        let dungeon = world.resource::<crate::dungeon::Dungeon>();
        crate::flowfield::FlowField::build(dungeon, goal)
    };
    let Some(field) = field else {
        return false;
    };
    let field = std::sync::Arc::new(field);
    let mut q = world.query_filtered::<Entity, With<crate::squad::Unit>>();
    let units: Vec<Entity> = q.iter(world).collect();
    for e in units {
        world.entity_mut(e).insert(crate::squad::MoveOrder::new(field.clone()));
    }
    true
}

/// Revoke every standing player order, handing locomotion (and the role actions gated on
/// `Without<MoveOrder>`) back to the squad AI.
///
/// A standing `MoveOrder` is not merely a movement hint — it is authoritative. `squad::unit_movement`
/// steers by the order's flow field and ignores `DesiredMove`; `perception::squad_think` sets
/// `DesiredMove.goal = None`; and **both `actions::unit_actions` and `actions::medic_heal` are
/// `Without<MoveOrder>`**, so an ordered unit neither examines, wards, barks, nor heals. An offline
/// evaluation that keeps the squad permanently ordered is therefore not evaluating the squad brain at
/// all. Returns the number of units released.
pub fn clear_squad_orders(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut q = world.query_filtered::<Entity, (With<crate::squad::Unit>, With<crate::squad::MoveOrder>)>();
    let ordered: Vec<Entity> = q.iter(world).collect();
    for e in &ordered {
        world.entity_mut(*e).remove::<crate::squad::MoveOrder>();
    }
    ordered.len()
}

/// How many units currently carry a player `MoveOrder` — the fraction of an episode in which the squad
/// AI is *not* in control.
pub fn ordered_unit_count(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(), (With<crate::squad::Unit>, With<crate::squad::MoveOrder>)>();
    q.iter(world).count()
}

/// Field-degeneracy stats `(peak, flatness)` for the offline search's field-sanity gate (see
/// `ai::field::Stig::saturation_stats`). `(0.0, 0.0)` before the fields/dungeon exist. Read-only.
pub fn field_saturation(app: &mut App) -> (f32, f32) {
    let world = app.world();
    match (
        world.get_resource::<crate::ai::field::Stig>(),
        world.get_resource::<crate::dungeon::Dungeon>(),
    ) {
        (Some(stig), Some(_dungeon)) => stig.saturation_stats(),
        _ => (0.0, 0.0),
    }
}

/// The dungeon cells currently occupied by squad units (for coverage tracking).
pub fn unit_cells(app: &mut App) -> Vec<IVec2> {
    let world = app.world_mut();
    let positions: Vec<Vec3> = {
        let mut q = world.query_filtered::<&Transform, With<crate::squad::Unit>>();
        q.iter(world).map(|t| t.translation).collect()
    };
    let dungeon = world.resource::<crate::dungeon::Dungeon>();
    positions.iter().map(|p| dungeon.world_to_cell(*p)).collect()
}

/// The dungeon cells the crab nests occupy. The offline evaluation's synthetic player walks its tour
/// through these, because a player who never seeks the objective never has an encounter — and a search
/// whose episodes contain no encounter learns nothing (see `squad_ai::evaluate`).
pub fn nest_cells(app: &mut App) -> Vec<IVec2> {
    let world = app.world_mut();
    let positions: Vec<Vec3> = {
        let mut q = world.query_filtered::<&Transform, With<crate::nest::Nest>>();
        q.iter(world).map(|t| t.translation).collect()
    };
    let dungeon = world.resource::<crate::dungeon::Dungeon>();
    positions.iter().map(|p| dungeon.world_to_cell(*p)).collect()
}

/// The squad's centroid cell. The offline tour uses this ONCE at plan time to order the crab hubs
/// nearest-first, so the fast squad reaches the slow swarm early in the episode. Read at tour-planning
/// time (right after spawn), so it reflects the deterministic spawn layout, not brain-driven movement —
/// the tour schedule stays independent of the brain under test.
pub fn squad_centroid_cell(app: &mut App) -> IVec2 {
    let world = app.world_mut();
    let positions: Vec<Vec3> = {
        let mut q = world.query_filtered::<&Transform, With<crate::squad::Unit>>();
        q.iter(world).map(|t| t.translation).collect()
    };
    let dungeon = world.resource::<crate::dungeon::Dungeon>();
    if positions.is_empty() {
        return dungeon.spawn;
    }
    let mean = positions.iter().copied().sum::<Vec3>() / positions.len() as f32;
    dungeon.world_to_cell(mean)
}

/// Every floor cell of the generated dungeon (goal-selection source + coverage denominator).
pub fn floor_cells(app: &mut App) -> Vec<IVec2> {
    let world = app.world_mut();
    let dungeon = world.resource::<crate::dungeon::Dungeon>();
    dungeon.floor_cells().collect()
}

/// Liveness oracle for the FULL (physics-inclusive) sim, whose Avian layer is not bit-reproducible so
/// can't be exact-hashed (Lu et al. 2022; the "unstable oracle" caveat). Returns the list of invariant
/// violations — **empty means healthy**. Checks the soft-lock / crash net: every actor has a finite
/// transform and finite, in-range health, and the world isn't empty or exploded. Cheap enough to call
/// every few ticks across a long random-agent run.
pub fn liveness_violations(app: &mut App) -> Vec<String> {
    /// A crab swarm reproduces, but a runaway (leak/explosion) should still trip. Generous ceiling.
    const MAX_ACTORS: usize = 2000;
    let world = app.world_mut();
    let mut query = world.query::<(&Transform, &crate::health::Health)>();
    let mut out = Vec::new();
    let mut count = 0usize;
    for (t, h) in query.iter(world) {
        count += 1;
        let p = t.translation;
        if !p.is_finite() {
            out.push(format!("non-finite transform {p:?}"));
        }
        if !h.current.is_finite() || !h.max.is_finite() {
            out.push(format!("non-finite health cur={} max={}", h.current, h.max));
        } else if h.current > h.max + 1.0e-3 {
            out.push(format!("health {} exceeds max {}", h.current, h.max));
        }
        // Stop flooding the report if something has gone very wrong.
        if out.len() > 16 {
            break;
        }
    }
    if count == 0 {
        out.push("no actors present (world emptied or never populated)".into());
    }
    if count > MAX_ACTORS {
        out.push(format!("actor count {count} exceeds ceiling {MAX_ACTORS} (runaway spawn?)"));
    }
    out
}
