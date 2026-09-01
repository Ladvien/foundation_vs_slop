//! Terminal-state goldens (feature `test-harness`): a run must be able to *end*, both ways, and end
//! reproducibly.
//!
//! **Why this oracle is exact when most game oracles cannot be.** Kato et al. (2026, *Software Testing
//! Beyond Closed Worlds: Open-World Games as an Extreme Case*, arXiv:2604.04047) name "Unstable
//! Oracles" as a defining property of this genre: expected outcomes emerge from interactions among
//! players, environments and autonomous agents, so correctness criteria drift and cannot be assumed
//! fixed. That is exactly why the rest of this suite leans on liveness and SSIM. The session rule is
//! the deliberate counter-example — a *closed*, total predicate over the world ("a squad existed and
//! now none live" / "the clock passed N fixed ticks") whose truth does not depend on balance, content,
//! or taste. It stays a stable oracle while the behavioural ones drift, which is precisely why the
//! win/lose decision was put in the deterministic core (`src/session/`) instead of the UI.
//!
//! Every test holds `serial_guard()` for its App's whole lifetime (harness invariant 4).
#![cfg(feature = "test-harness")]

use foundation_vs_slop::session::{DefeatCause, RunOutcome, RunSeed, RunState, WinCondition};
use foundation_vs_slop::sim_harness::{
    build_headless_app, contained_count, containable_targets, extraction_zone_cell, field_hash,
    floor_cells, gib_hash, gib_rows, issue_squad_order, kill_squad, run_outcome, run_ticks,
    site_root, site_specimens, squad_is_extracted,
    serial_guard, snapshot_hash, step, step_until_autogib_ready, SimConfig,
};

/// Read the coarse run state. Kept local to the test: `RunState` is a Bevy `States`, and reaching it
/// needs the `State<S>` wrapper the harness has no reason to re-export.
fn run_state(app: &mut bevy::prelude::App) -> RunState {
    *app.world().resource::<bevy::prelude::State<RunState>>().get()
}

/// Bring a fresh app to a **fixed** tick with the fracture bake already complete, so a kill there is
/// comparable across builds.
///
/// Both halves matter. Waiting for `carnage` alone is not enough — `step_until_autogib_ready` steps a
/// *variable* number of ticks (the bake lands when the GLB streams in, which is wall-clock dependent), so
/// gating on it and killing immediately moves the kill to a different tick in every run and compares two
/// different sims. Waiting and then advancing to a fixed absolute tick pins both: the bake is done AND
/// the kill happens at the same point of the same trajectory every time.
fn app_at_stable_kill_point(cfg: &SimConfig) -> bevy::prelude::App {
    /// Comfortably past the bake even under heavy CPU load, and cheap at physics-off speeds.
    ///
    /// **Raised 600 → 1200 on 2026-07-27, because 600 was measured not to be comfortable enough.** The
    /// documented FVS-N-8 reproducer (a full `cargo test` immediately followed by this target) failed
    /// 1 run in 5 on `the fracture bake never completed` — the settle giving up, *not* a gib divergence.
    ///
    /// Why more ticks help at all, since `step()` advances a fixed dt regardless of wall clock: the
    /// harness pins Bevy's IO task pool to **one** thread (that is what makes system order
    /// deterministic), so under CPU saturation that single thread is starved and the GLB streams in
    /// *fewer bytes per tick*. More ticks is more wall-clock for it, which is the only lever the test
    /// has.
    ///
    /// The settle budget and the kill point are deliberately the **same** constant. They are coupled by
    /// `now <= KILL_TICK` below — the kill must land after the bake — so splitting them into two knobs
    /// would let someone raise the settle and silently produce a kill point the bake can outlast, which
    /// is the exact failure this whole helper exists to prevent.
    const KILL_TICK: u64 = 1200;
    let mut app = build_headless_app(cfg);
    assert!(
        step_until_autogib_ready(&mut app, cfg, KILL_TICK as u32).is_some(),
        "the fracture bake never completed — the figurine asset never streamed in"
    );
    let now = run_ticks(&mut app);
    assert!(now <= KILL_TICK, "the bake outlasted the kill point ({now} > {KILL_TICK})");
    step(&mut app, cfg, (KILL_TICK - now) as u32);
    assert_eq!(run_ticks(&mut app), KILL_TICK, "the kill point must be the same tick in every run");
    app
}

#[test]
fn a_fresh_app_runs_one_fewer_fixed_tick_than_harness_steps() {
    // **The harness's first `update()` runs no fixed tick**, so `step(n)` on a fresh `App` advances the
    // fixed schedule exactly `n - 1` times. Every timing assertion in this file is written against that.
    //
    // It is structural, not a race: `TimeUpdateStrategy::ManualDuration` routes through
    // `Time::<Real>::update_with_duration` → `update_with_instant`, and on the FIRST call `last_update`
    // is `None`, so it seeds `first_update`/`last_update` and returns **without advancing**. Every later
    // update advances by exactly `fixed_dt`.
    //
    // Pinned here rather than "fixed" in `step()`: every committed golden is defined in terms of
    // `step(n)` (`deterministic_core_is_bit_identical` steps 180 for 179 ticks), so making `step` deliver
    // a literal `n` would move every one of them for no gameplay reason. The contract is the ruler; this
    // test stops the ruler moving silently under a Bevy upgrade.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 10);
    assert_eq!(run_ticks(&mut app), 9, "step(n) on a fresh App must advance the fixed schedule n-1");
}

#[test]
fn a_squad_wipe_resolves_the_run_to_defeat() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = app_at_stable_kill_point(&cfg);

    // Confirm the run is genuinely undecided before we touch it — otherwise a bug that resolves at tick
    // 0 would make the assertion below vacuous.
    assert_eq!(run_outcome(&mut app), RunOutcome::Undecided, "a live squad must leave the run open");
    assert_eq!(run_state(&mut app), RunState::Active);

    let struck = kill_squad(&mut app);
    assert_eq!(struck, 5, "the shipped squad is five members");

    // ONE tick is enough by construction, and deliberately so: `resolve_run` reads `Health`, not
    // entity existence, so it does not have to wait for `despawn_dead_units`' commands to flush (and
    // the session therefore needs no ordering edge into the existing schedule — see `src/session`).
    // If this ever needs two ticks, that property has been lost.
    step(&mut app, &cfg, 1);

    assert_eq!(
        run_outcome(&mut app),
        RunOutcome::Defeat(DefeatCause::SquadWipe),
        "a wiped squad must resolve the run to defeat"
    );

    // The world stays `Active` through a resolved run: the player reads the debrief over the final
    // frame, and the run-scoped entities must still be there to render. Tearing down is what LEAVING the
    // run does (`RETURN TO SITE`), not what resolving it does — an earlier draft got this wrong and
    // despawned the whole world at the moment of victory.
    assert_eq!(run_state(&mut app), RunState::Active, "a resolved run keeps its world");
}

#[test]
fn the_survive_timer_resolves_the_run_to_victory() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    // Override the shipped 5-minute placeholder with a 2-second one. `resolve_run` reads the resource
    // every tick, so this is the same single seam the shipped config installs through — not a bypass.
    app.world_mut().insert_resource(WinCondition::SurviveTicks(120));

    // `step(120)` ⇒ 119 fixed ticks on a fresh App (see the tick-accounting test above): one short.
    step(&mut app, &cfg, 120);
    assert_eq!(run_ticks(&mut app), 119, "precondition: one tick short of the threshold");
    assert_eq!(run_outcome(&mut app), RunOutcome::Undecided, "the timer must not fire early");

    step(&mut app, &cfg, 1);
    assert_eq!(run_outcome(&mut app), RunOutcome::Victory, "reaching the tick threshold must win");
    assert_eq!(run_ticks(&mut app), 120, "the clock must stop on the resolving tick");
    step(&mut app, &cfg, 5);
    assert_eq!(run_ticks(&mut app), 120, "and stays stopped");
    assert_eq!(run_state(&mut app), RunState::Active, "a resolved run keeps its world");
}

#[test]
fn a_resolved_run_stays_resolved_and_its_clock_freezes() {
    // The latch. `resolve_run` and `tick_run_clock` are both gated on
    // `resource_equals(RunOutcome::Undecided)`, so a resolved run can neither re-resolve nor age.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    app.world_mut().insert_resource(WinCondition::SurviveTicks(60));

    // 61 steps ⇒ 60 fixed ticks on a fresh App — exactly the threshold.
    step(&mut app, &cfg, 61);
    assert_eq!(run_outcome(&mut app), RunOutcome::Victory);
    let frozen = run_ticks(&mut app);
    assert_eq!(frozen, 60);

    // Now wipe the squad *after* the win. A second writer, or a missing latch, would flip the verdict
    // to defeat — the run is over and history is not editable.
    kill_squad(&mut app);
    step(&mut app, &cfg, 120);

    assert_eq!(run_outcome(&mut app), RunOutcome::Victory, "a resolved outcome must never be rewritten");
    assert_eq!(run_ticks(&mut app), frozen, "the run clock must stop at the resolving tick");
}

#[test]
fn both_terminal_paths_are_bit_reproducible() {
    // Same-seed reproducibility of the *resolving* trajectories, not just the outcome enum: the
    // session systems run inside the pinned core, so adding them must not have cost the core its
    // bit-identity (the property `deterministic_core_is_bit_identical` pins for the base sim).
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();

    let victory_hash = |_: ()| {
        let mut app = build_headless_app(&cfg);
        app.world_mut().insert_resource(WinCondition::SurviveTicks(120));
        step(&mut app, &cfg, 180);
        let outcome = run_outcome(&mut app);
        let h = snapshot_hash(&mut app);
        (outcome, h)
    };
    let (oa, ha) = victory_hash(());
    let (ob, hb) = victory_hash(());
    assert_eq!(oa, RunOutcome::Victory);
    assert_eq!(oa, ob, "same seed must reach the same outcome");
    assert_eq!(ha, hb, "a resolved run must still be bit-identical across same-seed builds");

    let defeat_hash = |_: ()| {
        // Fixed kill point with the bake settled — otherwise the two builds spawn different gib
        // populations and the crabs that forage them diverge downstream.
        let mut app = app_at_stable_kill_point(&cfg);
        kill_squad(&mut app);
        step(&mut app, &cfg, 30);
        let outcome = run_outcome(&mut app);
        let h = snapshot_hash(&mut app);
        (outcome, h)
    };
    let (da, dha) = defeat_hash(());
    let (db, dhb) = defeat_hash(());
    assert_eq!(da, RunOutcome::Defeat(DefeatCause::SquadWipe));
    assert_eq!(da, db, "same seed must reach the same outcome");
    assert_eq!(dha, dhb, "a wiped run must still be bit-identical across same-seed builds");
}

/// **FVS-A-5's acceptance test: a second expedition is a genuinely different world.**
///
/// Before this, `Dungeon::generate` ran at *plugin build* and every creature spawned on `Startup`, so
/// the world was a process-lifetime fact: `QUIT TO TITLE` → `NEW RUN` resumed the same used map with the
/// same corpses on it. "NEW RUN" was a lie. This drives the real transition — leave `RunState::Active`,
/// re-enter it — and checks all three halves of the fix: the old world is gone, a new one is built, and
/// it is a *different* one.
#[test]
fn leaving_and_re_entering_a_run_builds_a_fresh_different_world() {
    use bevy::prelude::{NextState, State};
    use foundation_vs_slop::dungeon::Dungeon;
    use foundation_vs_slop::squad::Unit;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 30);

    let first_seed = app.world().resource::<RunSeed>().0;
    let first_spawn = app.world().resource::<Dungeon>().spawn;
    let unit_count = |app: &mut bevy::prelude::App| {
        let world = app.world_mut();
        let mut q = world.query_filtered::<bevy::prelude::Entity, bevy::prelude::With<Unit>>();
        q.iter(world).count()
    };
    // Counting ONLY units is what let FVS-N-13 hide: the dungeon's tiles (and their Avian static
    // colliders) were never `run_scoped()`, so every expedition left a whole map resident and the
    // next one generated *through* it — invisible walls a gib chunk bounces off. Units despawned
    // correctly the whole time, so this test passed while the leak grew.
    let tile_count = |app: &mut bevy::prelude::App| {
        let world = app.world_mut();
        let mut q = world
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<foundation_vs_slop::dungeon::Tile>>();
        q.iter(world).count()
    };
    assert_eq!(unit_count(&mut app), 5, "the first run is populated");
    let first_tiles = tile_count(&mut app);
    assert!(first_tiles > 0, "the first run built a dungeon");

    // Leave the run — what `RETURN TO SITE` / `QUIT TO TITLE` do.
    app.world_mut().resource_mut::<NextState<RunState>>().set(RunState::Idle);
    step(&mut app, &cfg, 2);
    assert_eq!(*app.world().resource::<State<RunState>>().get(), RunState::Idle);
    assert_eq!(
        unit_count(&mut app),
        0,
        "leaving a run must despawn its entities — that is `run_scoped()` doing its job"
    );
    assert_eq!(
        tile_count(&mut app),
        0,
        "leaving a run must despawn its DUNGEON too (FVS-N-13). A surviving tile set means the next \
         expedition generates a second map through the first one, colliders and all."
    );

    // Start a new one — what `NEW RUN` does.
    app.world_mut().resource_mut::<NextState<RunState>>().set(RunState::Active);
    step(&mut app, &cfg, 2);
    assert_eq!(unit_count(&mut app), 5, "a new run must re-populate");
    let second_tiles = tile_count(&mut app);
    assert!(second_tiles > 0, "a new run must build its own dungeon");
    // The decisive shape: two expeditions in, the world holds ONE dungeon's worth of tiles, not two.
    assert!(
        second_tiles < first_tiles * 2,
        "expedition 2 resident tiles ({second_tiles}) approach two dungeons ({first_tiles} each) — \
         the run-1 map is still resident"
    );
    assert_eq!(run_outcome(&mut app), RunOutcome::Undecided, "a new run starts undecided");
    assert!(run_ticks(&mut app) <= 2, "a new run restarts its clock, got {}", run_ticks(&mut app));

    // ...and it is a DIFFERENT world, which is the whole point of the item.
    let second_seed = app.world().resource::<RunSeed>().0;
    assert_ne!(second_seed, first_seed, "the run seed must advance between expeditions");
    assert_ne!(
        app.world().resource::<Dungeon>().spawn,
        first_spawn,
        "a new expedition must generate a different map, not resume the used one"
    );
}

/// The wipe path's **actor and field** state is reproducible under load.
///
/// This is the real gate, and it is the pinned sim: `snapshot_hash` (every actor's transform + health)
/// and `field_hash` (the stigmergy grids) are what every gameplay decision reads. Deliberately runs
/// under CPU load — TESTING.md invariant 9, "a determinism probe on an IDLE box proves nothing" — and
/// deliberately exercises a death, the contended path no committed golden covers (invariant 11: the
/// 180-tick gate idles the squad at spawn, so nothing in it ever dies).
///
/// Gib chunk *positions* are checked separately and are currently known-divergent — see
/// [`zz_repro_gib_spawn_positions_diverge_under_load`] and FVS-N-8.
#[test]
fn the_wipe_paths_actors_and_fields_are_reproducible_under_load() {
    let (results, _) = wipe_trials();
    let distinct: Vec<(u64, u64)> = dedup(results.iter().map(|r| (r.actors, r.fields)));
    assert_eq!(
        distinct.len(),
        1,
        "the wipe path produced {} distinct actor/field results over {REPS} loaded reps: {distinct:?}",
        distinct.len()
    );
}

/// **FVS-N-8's regression gate — the bug is FIXED (2026-07-26) and this is what keeps it fixed.**
///
/// Was `#[ignore]`d for two sessions as a filed-not-hidden reproducer. It is the hardest form of the
/// bug: five squad members killed simultaneously (after the fracture bake has settled — see
/// [`app_at_stable_kill_point`]) with the box under CPU load, comparing actors, fields and gibs across
/// same-seed runs.
///
/// **The cause, for anyone who sees this go red again.** `carnage::seed_from` hashed the **`AssetId`**
/// of the character GLB to seed the fracture. An `AssetId` is a slot index in the asset arena, assigned
/// by async load order — so the same mesh got a different id run to run and `fracture` sliced the body
/// along completely different planes. Measured: 23 of 23 fragments differing in both centroid and
/// half-extents. Every symptom this test was written to describe followed from that one line.
///
/// **Two earlier fixes did not help, and it is worth knowing why they were still right:** a drain key
/// that was a prefix of its value, and a vertex soup assembled in async-load order. Both were genuine
/// latent defects of the same family; neither was this. A third — the scatter seed being a
/// `Local<u32>` accumulator carrying history across ticks — was also found and removed, and also was
/// not this. The lesson the previous sessions paid for: *this fingerprint (identical counts, identical
/// keys, identical ring order, positions differing in the last bits) points at the GEOMETRY SOURCE, not
/// at the ordering of the code that consumes it.*
///
/// If this reddens, run `tests/autogib_determinism.rs` first — it isolates the bake in 1.5 s with no
/// load, and it will tell you in one line whether the fracture or the gore path regressed.
#[test]
fn gib_spawn_positions_stay_identical_under_load() {
    let (results, details) = wipe_trials();
    let distinct = dedup(results.iter().map(|r| (r.actors, r.fields, r.gibs)));
    if distinct.len() > 1 {
        let (rows_a, ring_a) = &details[0];
        let (rows_b, ring_b) = details.iter().skip(1).find(|d| *d != &details[0]).unwrap_or(&details[0]);
        let cols = ["key", "x", "y", "z", "weight", "phase"];
        let mut which = String::from("rows identical");
        'outer: for (ra, rb) in rows_a.iter().zip(rows_b.iter()) {
            for (c, (a, b)) in ra.iter().zip(rb.iter()).enumerate() {
                if a != b {
                    which = format!("first differs at col '{}' (key {}): {a} vs {b}", cols[c], ra[0]);
                    break 'outer;
                }
            }
        }
        panic!(
            "gib state split into {} distinct results over {REPS} loaded reps\n  \
             counts {} vs {}\n  rows   {which}\n  ring   {}",
            distinct.len(),
            rows_a.len(),
            rows_b.len(),
            if ring_a == ring_b { "same" } else { "DIFFER" },
        );
    }
}

/// Reps per trial. G0 split ~30% of rollouts under load, so 10 reps miss a same-rate splitter <3%.
const REPS: usize = 10;

/// One reading of the world after a wipe.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
struct WipeResult {
    actors: u64,
    fields: u64,
    gibs: u64,
}

/// Distinct values, in first-seen order (no `Hash`/`Ord` needed on the tuple).
fn dedup<T: PartialEq>(it: impl Iterator<Item = T>) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for v in it {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// Run the wipe scenario `REPS` times **under CPU load**, returning each rep's hashes and gib detail.
///
/// The load threads are the point: an idle box produced 10/10 identical results here while a loaded one
/// split (TESTING.md invariant 9).
fn wipe_trials() -> (Vec<WipeResult>, Vec<(Vec<[u64; 6]>, Vec<u64>)>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Ticks after the kill. The split shows at the very first one, so this stays minimal.
    const AFTER: u32 = 1;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();

    let stop = Arc::new(AtomicBool::new(false));
    let load: Vec<_> = (0..8)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x: u64 = 0;
                while !stop.load(Ordering::Relaxed) {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                }
                x
            })
        })
        .collect();

    let mut results = Vec::with_capacity(REPS);
    let mut details = Vec::with_capacity(REPS);
    for _ in 0..REPS {
        let mut app = app_at_stable_kill_point(&cfg);
        kill_squad(&mut app);
        step(&mut app, &cfg, AFTER);
        results.push(WipeResult {
            actors: snapshot_hash(&mut app),
            fields: field_hash(&mut app),
            gibs: gib_hash(&mut app),
        });
        details.push(gib_rows(&mut app));
    }

    stop.store(true, Ordering::Relaxed);
    for t in load {
        let _ = t.join();
    }
    (results, details)
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// FVS-B-3 — `ExtractContained`: the real win.
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// Force an anomaly to `Contained` through the same one-way marker the game uses.
///
/// Deliberately the marker and not a config tweak: `Contained` is what carries the `on_add` hook that
/// grants a `Specimen`, so a test that inserts it exercises the real reward path. Driving SCP-999's
/// befriend rule for real is `tests/containment.rs`'s job (`scp999_is_captured_by_befriending_it`);
/// these tests are about the WIN rule, and re-deriving a capture in each of them would make them fail
/// for reasons that have nothing to do with extraction.
fn force_contained(app: &mut bevy::prelude::App, anomaly: bevy::prelude::Entity) {
    app.world_mut().entity_mut(anomaly).insert(foundation_vs_slop::containment::Contained);
}

/// The reachable floor cell farthest from `from` — somewhere the squad is unambiguously NOT extracting.
fn farthest_floor_from(app: &mut bevy::prelude::App, from: bevy::prelude::IVec2) -> bevy::prelude::IVec2 {
    let mut cells = floor_cells(app);
    // SORT-OK: cells are unique grid coordinates, so `(distance, cell)` is a total order and the pick
    // cannot depend on the order `floor_cells` happened to yield.
    cells.sort_unstable_by_key(|c| {
        let d = *c - from;
        (std::cmp::Reverse(d.x * d.x + d.y * d.y), c.x, c.y)
    });
    cells.first().copied().unwrap_or(from)
}

/// March the squad off the extraction pad and confirm it actually left.
///
/// **Load-bearing, not ceremony.** `spawn_squad` clusters the five operatives around `Dungeon::spawn`,
/// and the extraction zone is placed on that same cell — so a fresh run begins with the squad already
/// extracted. A test that captures something without moving first therefore wins instantly and proves
/// nothing about the walk-out. (This cost three confidently-wrong failures before it was noticed, which
/// is the same lesson FVS-M-4 records: a reproduction that has not been sanity-checked against its own
/// geometry is not evidence.)
fn walk_squad_off_the_pad(app: &mut bevy::prelude::App, cfg: &SimConfig) -> bool {
    let Some(exit) = extraction_zone_cell(app) else { return false };
    let far = farthest_floor_from(app, exit);
    if !issue_squad_order(app, far) {
        return false;
    }
    for _ in 0..40 {
        step(app, cfg, 30);
        if !squad_is_extracted(app) {
            return true;
        }
    }
    false
}

#[test]
fn a_capture_alone_does_not_win_the_run() {
    // THE test that proves the win rule is "extract", not merely "contain". If this ever passes with
    // the squad parked far from the exit, `ExtractContained` has quietly degenerated into a capture
    // counter and the walk-out — where the tension lives — has stopped existing.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    app.world_mut().insert_resource(WinCondition::ExtractContained { count: 1 });
    step(&mut app, &cfg, 60);

    let Some(target) = containable_targets(&mut app).into_iter().next() else {
        // No capturable anomaly on this seed is a content fact, not a failure of this rule.
        return;
    };
    // OFF THE PAD FIRST. The squad spawns on the extraction cell, so capturing before moving wins
    // immediately and would make this assertion vacuous.
    if !walk_squad_off_the_pad(&mut app, &cfg) {
        return;
    }
    force_contained(&mut app, target.1);
    step(&mut app, &cfg, 5);
    assert_eq!(contained_count(&mut app), 1, "precondition: an anomaly is held");
    assert!(!squad_is_extracted(&mut app), "precondition: the squad is off the pad");

    assert_eq!(
        run_outcome(&mut app),
        RunOutcome::Undecided,
        "a capture with the squad nowhere near the exit must NOT win the run"
    );
}

#[test]
fn extracting_a_contained_anomaly_wins_the_run() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    app.world_mut().insert_resource(WinCondition::ExtractContained { count: 1 });
    step(&mut app, &cfg, 60);

    let Some(target) = containable_targets(&mut app).into_iter().next() else {
        return;
    };
    if !walk_squad_off_the_pad(&mut app, &cfg) {
        return;
    }
    force_contained(&mut app, target.1);
    step(&mut app, &cfg, 5);
    assert_eq!(
        run_outcome(&mut app),
        RunOutcome::Undecided,
        "holding it out in the level is not yet winning"
    );

    let exit = extraction_zone_cell(&mut app).expect("every run places an extraction zone");
    assert!(issue_squad_order(&mut app, exit), "the exit must be reachable");
    step(&mut app, &cfg, 900);

    assert_eq!(
        run_outcome(&mut app),
        RunOutcome::Victory,
        "a held anomaly plus a squad at the exit is the win"
    );
}

#[test]
fn losing_the_specimen_before_the_exit_un_arms_the_win() {
    // The derived-not-ratcheted property, end to end: destroying the captured anomaly drops the live
    // `Contained` count and the squad standing on the pad no longer wins.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    app.world_mut().insert_resource(WinCondition::ExtractContained { count: 1 });
    step(&mut app, &cfg, 60);

    let Some(target) = containable_targets(&mut app).into_iter().next() else {
        return;
    };
    if !walk_squad_off_the_pad(&mut app, &cfg) {
        return;
    }
    force_contained(&mut app, target.1);
    step(&mut app, &cfg, 5);
    // Destroy it before the walk-out.
    app.world_mut().entity_mut(target.1).despawn();
    step(&mut app, &cfg, 5);
    assert_eq!(contained_count(&mut app), 0, "precondition: nothing left to extract");

    let exit = extraction_zone_cell(&mut app).expect("every run places an extraction zone");
    assert!(issue_squad_order(&mut app, exit));
    step(&mut app, &cfg, 600);

    assert_eq!(
        run_outcome(&mut app),
        RunOutcome::Undecided,
        "there is nothing to extract, so reaching the exit must not win"
    );
}

#[test]
fn an_extraction_run_is_bit_reproducible() {
    // Same seed, same scripted actions, same hash — the property every other assertion here rests on.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();

    let run_once = || {
        let mut app = build_headless_app(&cfg);
        app.world_mut().insert_resource(WinCondition::ExtractContained { count: 1 });
        step(&mut app, &cfg, 60);
        if let Some(t) = containable_targets(&mut app).into_iter().next() {
            force_contained(&mut app, t.1);
        }
        if let Some(exit) = extraction_zone_cell(&mut app) {
            issue_squad_order(&mut app, exit);
        }
        step(&mut app, &cfg, 300);
        (run_outcome(&mut app), snapshot_hash(&mut app), field_hash(&mut app))
    };

    assert_eq!(run_once(), run_once(), "an extraction run must be bit-reproducible");
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// FVS-G-1 / D-4 — the persistent Site, and the roguelite boundary.
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// The run's extraction-zone entity — a run-scoped control, to contrast against the persistent Site.
fn extraction_zone_entity(app: &mut bevy::prelude::App) -> Option<bevy::prelude::Entity> {
    let world = app.world_mut();
    let mut q = world
        .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<foundation_vs_slop::containment::ExtractionZone>>();
    q.iter(world).next()
}

/// Leave the current run and start a fresh one, the way `RETURN TO SITE` -> `NEW RUN` does.
fn re_enter_a_run(app: &mut bevy::prelude::App, cfg: &SimConfig) {
    use bevy::prelude::NextState;
    app.world_mut().resource_mut::<NextState<RunState>>().set(RunState::Idle);
    step(app, cfg, 2);
    app.world_mut().resource_mut::<NextState<RunState>>().set(RunState::Active);
    step(app, cfg, 60);
}

#[test]
fn the_site_survives_run_teardown_and_is_never_rebuilt() {
    // FVS-G-1's literal acceptance. The Site persists by NOT carrying `session::run_scoped()` — there is
    // no exempt-list, which is why this is worth pinning: a future spawner that copy-pastes
    // `run_scoped()` onto a Site entity would silently reintroduce the bug A-4 was written for.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let before = site_root(&mut app).expect("the Site is built at Startup");
    let zone_before = extraction_zone_entity(&mut app);
    re_enter_a_run(&mut app, &cfg);
    let after = site_root(&mut app).expect("the Site must outlive a run");
    let zone_after = extraction_zone_entity(&mut app);

    assert_eq!(before, after, "the Site must be the SAME entity across a run boundary, not rebuilt");
    // …while a RUN-SCOPED entity really was torn down and rebuilt, or the assertion above is vacuous.
    // Contrasting the two directly is the point: the Site persists by *not* carrying `run_scoped()`,
    // so the test has to show that the tag is what makes the difference.
    assert_ne!(
        zone_before, zone_after,
        "the extraction zone is run-scoped, so re-entering must rebuild it as a NEW entity"
    );
}

#[test]
fn specimens_accumulate_across_expeditions_and_stay_held_at_the_site() {
    // FVS-D-4 plus the roguelite boundary in one test: capture in run 1, leave, capture in run 2, and
    // assert BOTH are still on the Site's roster. This is the property `Specimen` gives up run-scoping
    // for, and nothing else asserts it.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some(first) = containable_targets(&mut app).into_iter().next() else {
        return;
    };
    force_contained(&mut app, first.1);
    step(&mut app, &cfg, 5);
    assert_eq!(site_specimens(&mut app).len(), 1, "run 1 banks one specimen");

    re_enter_a_run(&mut app, &cfg);
    // The run-scoped world is gone and rebuilt, so this is a genuinely different anomaly entity.
    let Some(second) = containable_targets(&mut app).into_iter().next() else {
        return;
    };
    force_contained(&mut app, second.1);
    step(&mut app, &cfg, 5);

    let held = site_specimens(&mut app);
    assert_eq!(held.len(), 2, "specimens must ACCUMULATE across expeditions, not reset with the world");

    // Every one of them is linked to the Site, not merely alive.
    let site = site_root(&mut app).expect("Site exists");
    for s in held {
        let link = app.world().get::<foundation_vs_slop::site::HeldAt>(s);
        assert_eq!(link.map(|h| h.0), Some(site), "every specimen must be HeldAt the Site");
    }
}

#[test]
fn a_specimen_record_never_carries_a_transform_or_health() {
    // THE hash-property assertion, and the one that catches a future "just put the cell body on the
    // Specimen" shortcut. `snapshot_hash` folds `(Transform, Health)` pairs, so a specimen that gained
    // both would start contributing rows to the pinned hash — from an entity that deliberately outlives
    // the run, which would make the golden depend on how many expeditions had been played.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some(t) = containable_targets(&mut app).into_iter().next() else { return };
    force_contained(&mut app, t.1);
    step(&mut app, &cfg, 5);

    for s in site_specimens(&mut app) {
        let e = app.world().entity(s);
        assert!(e.get::<bevy::prelude::Transform>().is_none(), "a Specimen must stay bodiless");
        assert!(e.get::<foundation_vs_slop::health::Health>().is_none(), "…and carry no Health");
    }
}

#[test]
fn an_empty_site_reads_as_no_specimens_rather_than_no_site() {
    // The relationship gotcha, pinned. Bevy REMOVES `SiteSpecimens` when it empties, so a bare
    // `Query<&SiteSpecimens>` matches nothing on a fresh save — which reads as "there is no Site".
    // Every consumer must treat absence as "holds nothing".
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    assert!(site_root(&mut app).is_some(), "the Site exists…");
    assert!(site_specimens(&mut app).is_empty(), "…and simply holds nothing yet");
}

#[test]
fn every_banked_specimen_arrives_with_an_open_research_posterior() {
    // FVS-E-1. The posterior is created WITH the specimen, so there is never a window in which a
    // capture is banked but unresearchable — and no second code path that could create one differently.
    use foundation_vs_slop::research::{HiddenParam, ResearchPosterior};

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some(t) = containable_targets(&mut app).into_iter().next() else { return };
    force_contained(&mut app, t.1);
    step(&mut app, &cfg, 5);

    let held = site_specimens(&mut app);
    assert_eq!(held.len(), 1, "precondition: one capture banked");
    let p = app
        .world()
        .get::<ResearchPosterior>(held[0])
        .copied()
        .expect("a banked specimen must carry a research posterior");
    assert!(!p.is_complete(), "a fresh capture must have everything left to learn");
    for q in HiddenParam::ALL {
        assert_eq!(p.belief(q), 0.5, "and must start at maximum entropy on every parameter");
    }
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// FVS-G-2 — save/load, and FVS-G-3's boundary made concrete.
// ─────────────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_campaign_survives_a_full_world_round_trip() {
    // G-2's acceptance against the REAL world rather than a struct: capture, snapshot the campaign,
    // wipe every specimen as a process restart would, restore, and check the meta-progress came back.
    use foundation_vs_slop::persist::{apply_save, capture_save};
    use foundation_vs_slop::research::{Capability, ResearchPosterior, TechTree, Unlocks};

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some(t) = containable_targets(&mut app).into_iter().next() else { return };
    force_contained(&mut app, t.1);
    step(&mut app, &cfg, 5);
    let banked = site_specimens(&mut app);
    assert_eq!(banked.len(), 1, "precondition: one capture banked");

    // Give it a payout and some research, so the round trip has something to lose.
    app.world_mut().entity_mut(banked[0]).insert(Unlocks(vec![Capability::MoraleField]));
    if let Some(mut p) = app.world_mut().get_mut::<ResearchPosterior>(banked[0]) {
        p.observe(foundation_vs_slop::research::HiddenParam::Lethality, true, 0.85);
    }
    app.world_mut().resource_mut::<TechTree>().grant(Capability::FieldCure);
    let seed_before = app.world().resource::<foundation_vs_slop::session::RunSeed>().0;

    let save = capture_save(app.world_mut());
    assert_eq!(save.specimens.len(), 1);
    assert!(!save.specimens[0].unlocks.is_empty(), "the payout must be saved");

    // Simulate a restart: every specimen gone, the tree cleared, the seed clobbered.
    for e in site_specimens(&mut app) {
        app.world_mut().despawn(e);
    }
    *app.world_mut().resource_mut::<TechTree>() = TechTree::default();
    app.world_mut().resource_mut::<foundation_vs_slop::session::RunSeed>().0 = 1;
    assert!(site_specimens(&mut app).is_empty(), "precondition: the campaign is gone");

    apply_save(app.world_mut(), &save).expect("a save this build wrote must load");

    let restored = site_specimens(&mut app);
    assert_eq!(restored.len(), 1, "the specimen must come back");
    let p = app.world().get::<ResearchPosterior>(restored[0]).copied().expect("posterior restored");
    assert!(
        p.belief(foundation_vs_slop::research::HiddenParam::Lethality) > 0.5,
        "research progress must survive the round trip, not reset to 0.5"
    );
    assert!(
        app.world().resource::<TechTree>().has(Capability::FieldCure),
        "unlocks must survive"
    );
    assert_eq!(
        app.world().resource::<foundation_vs_slop::session::RunSeed>().0,
        seed_before,
        "the Branch seed must survive, or you resume someone else's campaign"
    );
    // And it is HeldAt the Site again, not floating.
    assert!(
        app.world().get::<foundation_vs_slop::site::HeldAt>(restored[0]).is_some(),
        "a restored specimen must be linked to the Site"
    );
}

#[test]
fn loading_twice_does_not_double_the_campaign() {
    // The merge-versus-replace decision, pinned. Merging would silently double a campaign every time
    // it loaded, and there is no correct answer to "which of these two identical records is real".
    use foundation_vs_slop::persist::{apply_save, capture_save};

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some(t) = containable_targets(&mut app).into_iter().next() else { return };
    force_contained(&mut app, t.1);
    step(&mut app, &cfg, 5);

    let save = capture_save(app.world_mut());
    apply_save(app.world_mut(), &save).expect("load");
    apply_save(app.world_mut(), &save).expect("load again");
    assert_eq!(site_specimens(&mut app).len(), 1, "loading is a replacement, not a merge");
}

#[test]
fn a_lost_run_still_preserves_meta_progress() {
    // FVS-G-3's boundary, asserted rather than assumed: the squad wipes, the world tears down, and the
    // specimen banked earlier is still there. This is the whole reason `Specimen` is not run-scoped.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some(t) = containable_targets(&mut app).into_iter().next() else { return };
    // OFF THE PAD FIRST — the squad spawns inside the extraction zone, so capturing without moving
    // wins the run instantly and there is no loss to preserve progress across. Same trap as
    // `a_capture_alone_does_not_win_the_run`; it catches me every time this file grows a test.
    if !walk_squad_off_the_pad(&mut app, &cfg) {
        return;
    }
    force_contained(&mut app, t.1);
    step(&mut app, &cfg, 5);
    assert_eq!(site_specimens(&mut app).len(), 1);

    kill_squad(&mut app);
    step(&mut app, &cfg, 10);
    assert_eq!(
        run_outcome(&mut app),
        RunOutcome::Defeat(DefeatCause::SquadWipe),
        "precondition: the run was genuinely lost"
    );
    re_enter_a_run(&mut app, &cfg);

    assert_eq!(
        site_specimens(&mut app).len(),
        1,
        "a LOST run must still preserve what it banked — that is the roguelite boundary"
    );
}

/// **FVS-G-6** — `RunState::Idle` is a state the game can SIT IN.
///
/// # What this pins, and why it could not be pinned before
///
/// `Idle` used to be a one-frame blip: `session::begin_first_run` flips it on `PostStartup`, so nothing
/// ever observed a world-less frame. Site-67 needs the opposite — the player stands in `Idle` for
/// minutes. Measured 2026-07-26, flipping `AutoStartFirstRun(false)` panicked on the first frame:
/// `Parameter Res<Dungeon> failed validation: Resource does not exist`. In Bevy 0.19 a missing `Res<T>`
/// **panics**; it does not skip the system.
///
/// The fix is that every expedition-simulation system on `FixedUpdate` now carries
/// `distributive_run_if(in_state(RunState::Active))` — the same condition `session`'s own systems have
/// always used, so it is one mechanism rather than a second way of saying "there is a run".
///
/// **Why `in_state` and not `resource_exists::<Dungeon>`**, which the item left open: `Dungeon` is
/// **never removed** (`grep remove_resource::<Dungeon>` → 0 hits). After `RETURN TO SITE` it survives,
/// describing a despawned world — so `resource_exists` reads *true* for a stale world and would not
/// gate anything, while `in_state` would. That asymmetry decides it.
///
/// **Why `distributive_run_if` and not `run_if`**, which is the part worth remembering: `.run_if()` on
/// a *tuple* wraps it in an anonymous system set, and that extra graph node permutes the schedule's
/// linearisation — measured, it moved the deterministic golden by itself. `.distributive_run_if()`
/// attaches the condition to each system individually, adds no node, and leaves the golden **bit
/// identical**. Verified by giving the tuple form a trivially-true condition: the hash moved anyway,
/// proving the drift was structural rather than anything being skipped.
#[test]
fn the_game_can_sit_in_a_world_less_frame_without_a_dungeon() {
    use bevy::prelude::State;
    let _guard = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = foundation_vs_slop::sim_harness::build_headless_app_unfinished(&cfg);
    // Before `PostStartup`, so `begin_first_run` reads it and leaves the state in `Idle`.
    app.insert_resource(foundation_vs_slop::session::AutoStartFirstRun(false));
    app.finish();
    app.cleanup();

    // Several frames, not one: the panic this guards against is a *system parameter* validation, which
    // fires the first time each system actually runs. A single frame would miss anything gated behind a
    // fixed-timestep accumulator that has not yet reached its first tick.
    for _ in 0..120 {
        app.update();
    }

    assert_eq!(
        *app.world().resource::<State<RunState>>().get(),
        RunState::Idle,
        "with AutoStartFirstRun(false) the game must STAY world-less, not quietly start a run"
    );
    assert!(
        app.world().get_resource::<foundation_vs_slop::dungeon::Dungeon>().is_none(),
        "no expedition was started, so there must be no Dungeon — if one exists, something built a \
         world behind the Director's back and the Site would be showing a live expedition"
    );
}
