//! **Guards on the harness lane's known-red skip list — the debt, made self-enforcing.**
//!
//! `.github/workflows/ci.yml`'s `harness` job is a hard gate with four `--skip`ped tests. All four are
//! pre-existing failures, each with a `BACKLOG.md` entry carrying its measurements. A skip like that has
//! one failure mode above all others: **it outlives its reason.** The bug gets fixed, the test would now
//! pass, and the skip silently stays — so the gate keeps a hole in it that nobody remembers opening.
//!
//! These tests have **inverted semantics on purpose**: each asserts that a skip's *reason still holds*.
//! When one goes red, nothing is broken — it means somebody fixed the underlying bug, and the message
//! says which skip and which guard to delete. Read a failure here as a to-do, not a regression.
//!
//! # Why they guard the reason and not the test
//!
//! Re-running the four skipped tests would be the direct approach and is the wrong trade here. Two of
//! them roll out 7200-tick episodes, and `tests/playtest_level.rs` documents that it must be the *only*
//! `App` in its binary (`build_headless_app` has to be the first thing in the process to pin the compute
//! pool to one thread). So a guard that builds an `App` cannot live beside it, and one that re-runs the
//! rollouts would add minutes to the gate to re-learn what `BACKLOG.md` already records.
//!
//! Both guards below are therefore **pure functions over public data** — no `App`, no sim, no measurable
//! runtime. That buys cheapness at a real cost in coverage, stated plainly in each test.
#![cfg(feature = "test-harness")]

use foundation_vs_slop::squad_ai::surprise::{minimal_criterion, EpisodeOutcome};

/// **Guards two skips: `playtest_level` and `search_calibration`'s candidate-genome test.**
///
/// The criterion half of this debt is **paid** (2026-08-07). `minimal_criterion` used to reject the
/// shipped brains with *"no crab died — the world was static"*, because the synthetic player holds fire
/// while a capture is under way and the gate only recognised kills. It now accepts a completed
/// containment as a resolved encounter, and
/// `search_calibration::the_authored_brains_produce_a_real_encounter_on_every_world` went green and had
/// its skip deleted.
///
/// **What is left is the other half, and it is a different defect: the brain barely runs.** Measured on
/// world `0x5C09191`:
///
/// ```text
/// ordered_ticks: 4500 of 7200 (62.5%)   weapons_tight_ticks: 900 (12.5%)
/// ```
///
/// A standing `MoveOrder` excludes a unit from `unit_actions`/`medic_heal` (both `Without<MoveOrder>`),
/// so only the ENGAGE window exercises the brain at all. That is why
/// `a_candidate_genome_actually_changes_the_simulation` still fails: two different genomes produce a
/// byte-identical hash because neither gets enough control to diverge. And it is why
/// `playtest_level`'s 1800-tick smoke rollout still fails — one hub cycle costs
/// `DWELL_ADVANCES × ADVANCE_TICKS + ENGAGE_TICKS` = 1200 ticks, so 1800 barely reaches one engage
/// window and the capture that would resolve the episode usually has not completed.
///
/// Both therefore rest on the same number: the fraction of an episode the brain controls. This guard
/// pins it, so rebalancing the tour — the candidate fix — fires it.
///
/// **What this does NOT cover.** A fix that lengthens `playtest_level`'s horizon instead of rebalancing
/// the tour would make that test pass without moving this guard. It catches the shared cause, not every
/// possible route around it.
#[test]
fn the_brain_barely_runs_so_two_skips_are_still_needed() {
    use foundation_vs_slop::squad_ai::evaluate::{ADVANCE_TICKS, DWELL_ADVANCES, ENGAGE_TICKS};

    // One hub cycle: the dwell windows under a standing order, then the one window the brain owns.
    let cycle = DWELL_ADVANCES * ADVANCE_TICKS + ENGAGE_TICKS;
    let brain_fraction = ENGAGE_TICKS as f32 / cycle as f32;
    assert!(
        brain_fraction <= 0.30,
        "the synthetic player now leaves the brain in control for {:.0}% of each hub cycle (was 25%). \
         That is the rebalancing the two remaining skips were waiting on: re-run \
         `playtest_level::shipped_level_playtests_and_is_deterministic` and \
         `search_calibration::a_candidate_genome_actually_changes_the_simulation`, delete the `--skip` \
         lines that now pass from `.github/workflows/ci.yml`'s `harness` job, and delete this guard. \
         Nothing is broken; this is the to-do firing.",
        brain_fraction * 100.0
    );

    // And the 1800-tick smoke rollout still cannot fit two hub cycles, which is the other half of
    // `playtest_level`'s failure. Stated as data so a horizon change is visible here too.
    assert!(
        cycle * 2 > 1800,
        "a hub cycle is now {cycle} ticks, so `playtest_level`'s 1800-tick rollout reaches two engage \
         windows — re-run it; if it passes, delete its skip."
    );
}

/// **Guards the fourth skip: `containment::watching_the_feed_makes_it_generate_and_ignoring_it_stops`.**
///
/// That test is red because `sim.broadcast.watch_threshold` sits **inside the ambient noise floor**.
/// Measured 2026-08-05: a screen with a unit in line of sight reads 0.77 (at the 8-cell vision edge) to
/// 1.39 (point blank), while a screen nobody can see rests at 0.0025–0.0067 of pure diffusion. The
/// shipped threshold is under that resting floor, so every screen counts as watched forever and "look
/// away to contain it" cannot hold.
///
/// Raising it is not a straightforward fix — `broadcast.spawn_min_dist` is 16.0 tiles against a
/// `fog::VISION_RADIUS` of 8, so a screen is seeded beyond the range its own mechanic can reach and any
/// threshold above the noise makes the anomaly a prop instead. `BACKLOG.md` carries both measurements
/// and the placement decision that has to come first.
///
/// This guard fires the moment the threshold leaves the noise floor, which is the necessary half of any
/// fix.
#[test]
fn the_watch_threshold_is_still_inside_the_noise_floor_so_its_skip_is_still_needed() {
    // The highest ambient reading measured at a screen nobody was looking at. A threshold above this can
    // no longer be tripped by diffusion alone.
    const MEASURED_AMBIENT_CEILING: f32 = 0.0067;

    let tuning = foundation_vs_slop::sim::SimTuning::default();
    let threshold = tuning.broadcast.watch_threshold;
    assert!(
        threshold <= MEASURED_AMBIENT_CEILING,
        "`broadcast.watch_threshold` is now {threshold}, above the {MEASURED_AMBIENT_CEILING} ambient \
         diffusion ceiling measured at an unwatched screen — so the ATTENTION gate can discriminate \
         again. Re-run `containment::watching_the_feed_makes_it_generate_and_ignoring_it_stops`; if it \
         passes, delete its `--skip` from `.github/workflows/ci.yml` and delete this guard. Check the \
         paired containment ceiling moved with it (`assets/config/config.ron`'s broadcast `requires` \
         line) and that `spawn_min_dist` vs `fog::VISION_RADIUS` was resolved — see `BACKLOG.md`. \
         Nothing is broken; this is the to-do firing."
    );
}
