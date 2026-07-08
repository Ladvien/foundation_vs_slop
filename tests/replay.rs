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
    build_headless_app, liveness_violations, serial_guard, snapshot_hash, step, SimConfig,
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
