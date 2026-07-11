//! Full-sim replay + repeatability (feature `test-harness`). Only compiled with the harness feature.
//!
//! Two oracles at two altitudes (the vetted split — Ostrowski & Aroudj 2013; Bécares 2017; and the
//! "unstable oracle" caveat, Kato et al. 2026):
//!   * **Deterministic gameplay core** (Avian solver OFF) → **exact same-seed hash**. This is the
//!     repeatability guarantee for the game LOGIC: AI, movement, combat, economy.
//!   * **Full sim** (physics ON) → **liveness oracle** (no panic / NaN / out-of-range health / runaway
//!     spawn). Avian's float solver is not bit-reproducible (a documented invariant), so exact hashing
//!     is the wrong tool there; liveness degrades gracefully instead.
//!
//! Runs the real game plugins headless (no window). Each test holds `serial_guard()` for the whole App
//! lifetime — two headless Apps must not run concurrently (shared global task pool + GPU device).
#![cfg(feature = "test-harness")]

use foundation_vs_slop::sim_harness::{
    build_headless_app, field_hash, liveness_violations, serial_guard, snapshot_hash, step, SimConfig,
};

#[test]
fn headless_app_boots_and_steps_without_panicking() {
    let _serial = serial_guard();
    let cfg = SimConfig::default();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 10);
    assert_ne!(snapshot_hash(&mut app), 0, "a booted, stepped sim must have non-trivial state");
}

#[test]
fn deterministic_core_is_bit_identical() {
    // THE repeatability proof. The gameplay LOGIC (physics OFF) is bit-reproducible: two independent
    // same-seed runs, stepped the same fixed ticks, hash identically. This is the direct answer to
    // "is everything repeatable from the same seed?" — yes, for everything the solver doesn't touch.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();

    let mut a = build_headless_app(&cfg);
    step(&mut a, &cfg, 180); // ~3 s: dungeon gen, spawns, AI think, movement, combat, economy
    let ha = snapshot_hash(&mut a);
    drop(a);

    let mut b = build_headless_app(&cfg);
    step(&mut b, &cfg, 180);
    let hb = snapshot_hash(&mut b);

    assert_eq!(ha, hb, "physics-free core must be bit-identical across same-seed runs");
}

#[test]
fn migrated_defaults_reproduce_the_shipped_golden_hash() {
    // Phase-1 byte-identity gate for the const→config (`SimTuning`) migration. Promoting the combat /
    // economy / deposit / fear / boss numbers out of Rust `const`s and into the `sim:` config slice must
    // be a PURE refactor: the deterministic core, run from the shipped config (dungeon seed 0x5C09191) for
    // 1800 fixed ticks, must still hash to the value measured BEFORE the migration. A drifted default — in
    // `SimTuning::default()` or the `config.ron` `sim:` slice — reds this test instead of silently shifting
    // a gameplay value. This is the absolute-value lock the same-seed reproducibility tests above cannot
    // provide.
    //
    // Proven byte-identical across the migration: pre-migration HEAD and the post-migration tree BOTH hash to
    // exactly this at 1800 ticks (measured by stashing the migration and re-running). It supersedes the older
    // `0x716d0cfbb69b778e` quoted in TESTING.md, which predates this branch's gameplay changes
    // (faction-relative fear, psionic field-sight) and is stale.
    const GOLDEN: u64 = 0xec1add310772895c;
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1800);
    assert_eq!(
        snapshot_hash(&mut app),
        GOLDEN,
        "deterministic-core hash drifted from the pre-migration golden — the const→config promotion \
         changed a gameplay value (or the shipped `sim:` slice differs from SimTuning::default())"
    );
}

#[test]
fn field_passes_are_bit_identical() {
    // The direct oracle for the "iterate only floor cells" optimization of the evaporate/diffuse/hotspot
    // passes (commit 973319d). `snapshot_hash` folds only actor Transform+Health, so it catches a diffusion
    // regression only *transitively* — if the perturbed gradient happens to move a crab to a different cell —
    // and never exercises `saturation_stats` at all. `field_hash` folds the field grids themselves (every
    // Stig channel cell + every RallyField vector, full grid, plus saturation_stats), so a reordered
    // neighbour sum, a broken floor mask, or a rock cell that stops being 0 reds this test outright. Same
    // deterministic-core config and tick count as the actor golden above, so the two are directly comparable.
    const GOLDEN_FIELD: u64 = 0x9e33_16af_f944_c5f8;
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1800);
    assert_eq!(
        field_hash(&mut app),
        GOLDEN_FIELD,
        "stigmergy field grids drifted from the golden — the evaporate/diffuse/hotspot floor-cell \
         iteration is no longer bit-identical to the full-grid scan"
    );
}

#[test]
fn authored_world_config_override_is_a_noop() {
    // Phase-2 seam identity: installing the *shipped* world (decoded from the authored world genome) through
    // `SimConfig::config` must be byte-identical to installing nothing. This pins the whole
    // encode → decode → WorldConfig → GameConfig(ai_tuning, sim) → running-sim path as lossless — it must
    // reproduce the Phase-1 golden exactly. If the override seam or encode/decode drifted a single knob,
    // this reds.
    use foundation_vs_slop::squad_ai::world_genome::{authored, decode};
    let _serial = serial_guard();
    let authored_world = decode(&authored()).expect("the authored world genome decodes");
    let cfg = SimConfig::deterministic_core().with_world_config(authored_world);
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1800);
    assert_eq!(
        snapshot_hash(&mut app),
        0xec1add310772895c,
        "installing the authored world config changed the sim — the override seam or encode/decode is lossy"
    );
}

#[test]
fn a_mutated_world_config_changes_the_sim() {
    // The dual of the no-op test: a *mutated* world genome, installed the same way, must change
    // `snapshot_hash`. Proves the config actually reaches the running sim (crab fields/fear, combat,
    // economy) rather than being silently dropped — the world-population analogue of
    // `search_calibration::a_candidate_genome_actually_changes_the_simulation`.
    use foundation_vs_slop::rng::seeded;
    use foundation_vs_slop::squad_ai::world_genome::{authored, decode, mutate};
    let _serial = serial_guard();

    let base = SimConfig::deterministic_core()
        .with_world_config(decode(&authored()).expect("decode authored"));
    let mut a = build_headless_app(&base);
    step(&mut a, &base, 600);
    let ha = snapshot_hash(&mut a);
    drop(a);

    // A large sigma so many knobs (field rates, fear gains, combat, economy) move unmistakably.
    let mutant = mutate(&authored(), 1.0, &mut seeded(0xB0A7)).expect("mutate");
    let mcfg = SimConfig::deterministic_core().with_world_config(decode(&mutant).expect("decode mutant"));
    let mut b = build_headless_app(&mcfg);
    step(&mut b, &mcfg, 600);
    let hb = snapshot_hash(&mut b);

    assert_ne!(
        ha, hb,
        "a mutated world config produced an identical sim — the config override is not reaching gameplay"
    );
}

#[test]
fn deterministic_core_is_bit_identical_across_many_builds() {
    // Stronger guard than the two-build check above. Entity enumeration order is NOT stable across
    // same-seed `App` instances in one process (GLB scene-child instantiation + entity-id reuse permute
    // it), so any gameplay decision that keys on iteration order — a "keep the first on a tie" pick, a
    // non-associative float sum over an entity list, a value fed by an async-loaded asset — diverges
    // only intermittently. The two-build test catches such a bug just ~1% of the time, so it slipped
    // through for months; building MANY apps and hashing each makes a per-instance-order dependence fail
    // reliably. Keep N high enough that a ~1%-per-build regression is caught essentially every run.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();

    let mut reference: Option<u64> = None;
    for build in 0..24 {
        let mut app = build_headless_app(&cfg);
        step(&mut app, &cfg, 180);
        let h = snapshot_hash(&mut app);
        match reference {
            None => reference = Some(h),
            Some(r) => assert_eq!(
                h, r,
                "physics-free core diverged on build {build}: gameplay must not depend on entity \
                 enumeration order (see util::nearest_planar / crab::assign_meat_targets)"
            ),
        }
    }
}

#[test]
fn core_state_evolves_over_time() {
    // Guards against a dead sim silently "passing" repeatability: state after 180 ticks must differ from
    // the freshly-spawned state (things actually moved / fought / were born). Physics-free so it's stable.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1);
    let early = snapshot_hash(&mut app);
    step(&mut app, &cfg, 179);
    let late = snapshot_hash(&mut app);
    assert_ne!(early, late, "the simulation should evolve — state must change over 180 ticks");
}

#[test]
fn speed_setting_is_deterministic_and_has_effect() {
    // The speed knob (`Time<Virtual>` relative speed) drives fast-forward without compromising
    // determinism: two runs at the same non-unit speed reach the same state, and a higher speed advances
    // the sim further per update.
    //
    // NOTE we deliberately do NOT assert exact equality ACROSS different speeds. The pinned sim advances
    // by a fixed sub-step, but cosmetic per-frame `Update` systems that legitimately touch the wall clock
    // — hitstop scaling `Time<Virtual>`, etc. — run once per update regardless of how many fixed
    // sub-steps that update contains, so the sub-step COUNT can differ by one across speeds. Same-seed /
    // same-speed reproducibility is the guarantee (see `deterministic_core_is_bit_identical`).
    let _serial = serial_guard();
    let fast = SimConfig { speed: 2.0, ..SimConfig::deterministic_core() };

    let mut a = build_headless_app(&fast);
    step(&mut a, &fast, 90);
    let ha = snapshot_hash(&mut a);
    drop(a);

    let mut b = build_headless_app(&fast);
    step(&mut b, &fast, 90);
    let hb = snapshot_hash(&mut b);
    assert_eq!(ha, hb, "same seed at the same speed must be reproducible");

    // 2× speed for 90 updates advances further than 1× for 90 updates.
    let base = SimConfig::deterministic_core();
    let mut c = build_headless_app(&base);
    step(&mut c, &base, 90);
    let hc = snapshot_hash(&mut c);
    assert_ne!(ha, hc, "a higher speed must advance the sim further per update");
}

#[test]
fn ui_never_leaks_into_deterministic_core() {
    // Determinism firewall. The windowed `UiPlugin` (states, HUD, menus) is registered only in
    // `lib::run`, never in the harness — so its `AppState` must be absent here. The pause resources
    // `UserPaused`/`SimBlocked` DO exist (owned by `TimeControlPlugin`), but the UI is their only
    // writer, so in the headless core they must stay at their inert `false` defaults. A stray
    // `SimBlocked=true` would freeze replay; this asserts that can't happen.
    use bevy::prelude::State;
    use foundation_vs_slop::time_control::{SimBlocked, UserPaused};
    use foundation_vs_slop::ui::state::AppState;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 5);

    assert!(
        app.world().get_resource::<State<AppState>>().is_none(),
        "UI AppState must not exist in the headless deterministic core"
    );
    assert!(
        !app.world().resource::<SimBlocked>().0,
        "SimBlocked must stay false in the core (no UI writer present)"
    );
    assert!(
        !app.world().resource::<UserPaused>().0,
        "UserPaused must stay false in the core (no key input present)"
    );
}

#[test]
fn ui_screens_spawn_and_pause_blocks_the_sim() {
    // OPERABILITY liveness (Game-UI Guidance §1.5): boot the *real* windowed UI headless and prove
    // the screens actually spawn and the state flow works — the substitute for a pixel screenshot,
    // which this headless env can't produce (no monitor → black drawable). Not a determinism test:
    // it builds its own UI-inclusive app; the core reference app (`build_headless_app`) is untouched.
    use bevy::prelude::*;
    use foundation_vs_slop::sim_harness::build_headless_app_unfinished;
    use foundation_vs_slop::time_control::SimBlocked;
    use foundation_vs_slop::ui::hud::{HudRoot, SpeedText};
    use foundation_vs_slop::ui::pause::PauseRoot;
    use foundation_vs_slop::ui::state::{AppState, MenuState};
    use foundation_vs_slop::ui::UiPlugin;

    let _serial = serial_guard();
    // Redirect settings IO to a temp dir so the test never writes the real user config.
    // SAFETY: `serial_guard` is held, so this is the only thread touching the environment.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", std::env::temp_dir().join("fvs_ui_liveness"));
    }

    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app_unfinished(&cfg);
    app.add_plugins(UiPlugin);
    app.finish();
    app.cleanup();

    // Boot gates to the title (font-ready or its frame cap) within a few dozen frames.
    for _ in 0..40 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<State<AppState>>().get(),
        &AppState::Title,
        "boot should reach the title screen"
    );
    assert!(
        app.world().resource::<SimBlocked>().0,
        "the title screen must block the sim underneath it"
    );

    // Enter the game → HUD spawns, sim unblocks.
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    app.update();
    assert!(
        !app.world().resource::<SimBlocked>().0,
        "in-game with no menu open must unblock the sim"
    );
    {
        let mut q = app.world_mut().query_filtered::<Entity, With<HudRoot>>();
        assert_eq!(q.iter(app.world()).count(), 1, "HUD root should spawn on entering the game");
    }
    {
        let mut q = app.world_mut().query_filtered::<Entity, With<SpeedText>>();
        assert!(q.iter(app.world()).next().is_some(), "HUD speed readout should exist");
    }

    // Open the pause menu → overlay spawns, sim blocks again.
    app.world_mut()
        .resource_mut::<NextState<MenuState>>()
        .set(MenuState::Pause);
    app.update();
    app.update();
    assert!(
        app.world().resource::<SimBlocked>().0,
        "the pause menu must block the sim"
    );
    {
        let mut q = app.world_mut().query_filtered::<Entity, With<PauseRoot>>();
        assert!(q.iter(app.world()).next().is_some(), "pause overlay should spawn");
    }
}

#[test]
fn full_sim_stays_live() {
    // Full physics-inclusive sim (the real production plugin set). Not exact-hashable (Avian isn't
    // bit-reproducible), so we assert LIVENESS every 30 ticks over ~5 s: no panic, no NaN transforms, no
    // out-of-range health, no runaway spawn. This is the soft-lock / crash net (Stage 4 in miniature).
    let _serial = serial_guard();
    let cfg = SimConfig::default();
    let mut app = build_headless_app(&cfg);
    for checkpoint in 1..=10 {
        step(&mut app, &cfg, 30);
        let v = liveness_violations(&mut app);
        assert!(v.is_empty(), "liveness violated at tick {}: {v:?}", checkpoint * 30);
    }
}
