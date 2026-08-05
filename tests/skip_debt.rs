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

/// **Guards three skips: `playtest_level` and both `search_calibration` tests.**
///
/// All three fail because `minimal_criterion` rejects the shipped brains with *"no crab died — the world
/// was static"*, measured on world `0x5C09191`:
///
/// ```text
/// ordered_ticks: 4500 of 7200   weapons_tight_ticks: 900
/// captures_attempted: 3, completed: 1     crabs_killed: 0   unit_damage_taken: 0.0
/// ```
///
/// The squad is under a standing player order for 62.5% of the episode and holding fire for another
/// 12.5% — and holding fire is a *containment verb*, correctly used: three captures were attempted and
/// one completed. So the containment mechanic and this criterion's "something must have died" are in
/// direct conflict, and the harness satisfies containment.
///
/// `search_calibration`'s own message names the fix this guards: *"a threshold in
/// `surprise::minimal_criterion` needs recalibrating"*. The obvious recalibration is to let a
/// **completed capture** count as a real encounter alongside a kill — the outcome already carries
/// `captures_completed`. This test constructs exactly that episode: everything the criterion wants,
/// zero kills, one capture. While the criterion still rejects it, the three skips are still needed.
///
/// **What this does NOT cover.** Two of the three candidate fixes leave the criterion untouched —
/// shortening `ADVANCE_TICKS` so less of the episode is spent ordered, or stopping the synthetic player
/// holding weapons tight. Either would make the three tests pass without moving this guard. It catches
/// the fix the failing test itself recommends, not every possible fix.
#[test]
fn minimal_criterion_still_demands_a_kill_so_three_skips_are_still_needed() {
    // An episode that satisfies every other clause: a live squad, a live swarm, real agency, damage
    // taken, reachable floor — and a completed containment capture instead of a kill.
    let contained_not_killed = EpisodeOutcome {
        squad_size: 5,
        survivors: 5,
        squad_duty_decisions: 440,
        crabs_alive: 43,
        crabs_killed: 0,
        unit_damage_taken: 4.0,
        reachable_cells: 3577,
        // The real measured coverage from the failure dump. Not decoration: `minimal_criterion` also
        // requires >2% of the map covered, and leaving this at `0` had the synthetic episode refused
        // for THAT clause instead — which the control assertion above caught on the first run.
        cells_covered: 332,
        captures_completed: 1,
        captures_attempted: 3,
        liveness_violations: 0,
        ..EpisodeOutcome::default()
    };
    // **This guard proves it can fail before it asserts anything.** The same episode with a single kill
    // must be ACCEPTED — otherwise `contained_not_killed` is being refused by some other clause, the
    // assertion below would hold for a reason unrelated to the skips, and this guard would be exactly
    // the kind that cannot fail. (That defect shipped in this repo once already:
    // `every_action_resolves_to_a_binding` asserted only that the returned row had non-empty fields,
    // which the fallback row satisfies.)
    let same_but_one_kill = EpisodeOutcome { crabs_killed: 1, ..contained_not_killed };
    assert!(
        minimal_criterion(&same_but_one_kill).is_ok(),
        "the control case is refused: this synthetic episode fails `minimal_criterion` even WITH a kill \
         ({:?}), so the assertion below proves nothing about the three skips. Fix the synthetic outcome \
         until only the kill clause separates the two.",
        minimal_criterion(&same_but_one_kill).err()
    );

    let verdict = minimal_criterion(&contained_not_killed);
    assert!(
        verdict.is_err(),
        "`minimal_criterion` now ACCEPTS a zero-kill episode that completed a containment capture — \
         which is the recalibration `search_calibration`'s own failure message asks for. If that was \
         deliberate, the three sim-based skips in `.github/workflows/ci.yml`'s `harness` job are \
         probably stale: re-run `playtest_level` and both `search_calibration` tests, delete the skips \
         that now pass, and delete this guard. Nothing is broken — this is the to-do firing."
    );
    // And that it is refused for the documented reason, not some unrelated clause — otherwise this
    // guard would keep passing for a reason that has nothing to do with the three skips.
    let why = verdict.err().unwrap_or_default();
    assert!(
        why.contains("no crab died"),
        "the criterion still refuses a capture-only episode, but no longer because nothing died — it \
         says {why:?}. Re-read `BACKLOG.md`'s entry for the four skips: this guard's reasoning is \
         built on the \"no crab died\" clause and needs revisiting."
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
