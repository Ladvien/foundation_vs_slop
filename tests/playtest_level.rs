//! Playtest-scored level evaluation (feature `test-harness`). Its own integration-test binary — like
//! `replay.rs` / `liveness.rs` — because `evaluate_playtest` builds a headless `App`, and
//! `sim_harness::build_headless_app` must be the FIRST thing in the process to touch the global compute
//! pool so it can pin it to a single thread for determinism. In the `--lib` unit-test binary other tests
//! initialise the pool first and trip that assert; a dedicated binary keeps this the first (and only) App.
#![cfg(feature = "test-harness")]

use foundation_vs_slop::squad_ai::level_eval::{evaluate_playtest, load_base};
use foundation_vs_slop::squad_ai::level_genome::authored;

#[test]
fn shipped_level_playtests_and_is_deterministic() {
    // Do NOT hold `serial_guard()` here — `evaluate_playtest`'s rollouts acquire the (non-reentrant) guard
    // internally per App; holding it here would deadlock. This is the only test in this binary, so the
    // rollout's `build_headless_app` is the first thing to pin the thread pool.
    let (base, manifest) = load_base().expect("shipped config loads");
    let g = authored(&base);
    let seeds = [0x5C09191u64];

    let a = evaluate_playtest(&g, &base, &manifest, &seeds, 1800).expect("shipped level plays");
    assert!((0.0..=1.0).contains(&a.fitness), "engagement fitness must be in [0,1], got {}", a.fitness);
    assert!((0.0..=1.0).contains(&a.axes.0) && (0.0..=1.0).contains(&a.axes.1));

    let b = evaluate_playtest(&g, &base, &manifest, &seeds, 1800).expect("shipped level plays again");
    assert_eq!(a.fitness, b.fitness, "playtest scoring must be deterministic");
    assert_eq!(a.axes, b.axes, "descriptor axes must be deterministic");
}
