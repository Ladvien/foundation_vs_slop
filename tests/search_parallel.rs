//! **Parallel-search determinism gate** (feature `test-harness`, needs a GPU).
//!
//! The multi-process evaluator (`squad_ai::parallel`, driven by `SearchConfig::jobs > 1`) exists purely to
//! spend more CPU for less wall-clock. Its whole correctness claim is that it changes *nothing* about what
//! the search computes: because a rollout is a pure function of `(brains, world, seed, ticks)` and draws no
//! search RNG, fanning the `OPPONENTS` triples of a candidate across worker processes — and reducing them
//! back in input order — must yield the **byte-identical** archives that the inline path (`jobs = 1`) does.
//!
//! This test pins that. It runs the same tiny search twice, once inline and once across a worker pool, and
//! asserts every occupied cell of all three archives matches exactly (descriptor, fitness, and genome
//! handle, bit-for-bit). If the parallelisation ever perturbs the RNG stream, the reduction order, or the
//! float math, this reds the build. It is slow (each rollout boots the real game and steps 120 s of
//! simulated time), so it lives in the harness lane, not the fast deterministic-core lane.
//!
//! # History — this header has been wrong three times; that is the useful part
//!
//! These tests were red for **months** and every explanation offered for it was wrong:
//!
//! 1. It blamed **G0** (rollout non-determinism) and asserted the failure was "not a bug in the batch
//!    emitter". G0 was real — `laser::fire_laser` drew a shared RNG stream in ECS query order — but it was
//!    never why these tests were red.
//! 2. They were actually failing at `assert!(filled > 0)`: the archive was **empty**, so no determinism
//!    comparison ever ran. Cause (G0b): `config.ron` held a machine-baked levels elite instead of the
//!    authored level, so the squad fought a different map and was wiped.
//! 3. Once the archive filled, `jobs=1` vs `jobs=N` genuinely disagreed — and the obvious reading, that the
//!    parallel path was at fault, was **also wrong**. Measured: `inline != inline` at `jobs=1`. The
//!    reduction is index-addressed, seeds are pre-drawn before any fan-out, and the wire is bit-exact. The
//!    parallelism was innocent the whole time; these tests were reporting a **rollout** bug (G0c: `GibKey`
//!    was derived from the death origin position, so it could not break the position tie it existed to
//!    break — see `docs/rl/2026-07-16-search-rollout-nondeterminism.md`).
//!
//! Two lessons worth keeping. **A red test is evidence that something is wrong, not evidence about what.**
//! And note why the arms are not symmetric: `jobs=N` self-loads (N contending worker processes) while
//! `jobs=1` is quiet — and this bug class is *hidden* by a quiet box (an idle machine returned 12/12
//! identical rollouts with G0 live). So the two arms differ in the one variable that gates the bug's
//! visibility, which is exactly how a rollout bug masquerades as a parallelism bug.
//!
//! If these red again, suspect the rollout before `parallel.rs`: run
//! `replay::search_rollouts_are_reproducible_under_load` first — it is cheaper, it covers the same two
//! worlds, and it fails for the real reason.
#![cfg(feature = "test-harness")]

use foundation_vs_slop::squad_ai::coevolve::{search, sweep_prior, Population, SearchConfig, Templates};

/// Two worlds — the minimum `evolve` allows (a candidate's two rollouts must run on different worlds). Kept
/// to two to hold the test's rollout budget down.
///
/// **NOT the search's held-in set** — that is `coevolve::HELD_IN_SEEDS`, and `0xA11CE` was retired from it
/// when the mold tipped it into squad wipes. It stays here for the same reason it stays in
/// `replay::search_rollouts_are_reproducible_under_load`: it *splits*, which is what a determinism gate
/// wants. Determinism is a property of the sim, not of the seeds the search happens to run.
const SEEDS: [u64; 2] = [0x5C09191, 0xA11CE];

/// The measured episode floor — `coevolve::SearchConfig`'s doc carries the damage-per-seed table. Below it
/// the minimal criterion sits on a knife's edge, the admitted fraction collapses, and `filled > 0` below
/// gets flaky.
///
/// **Mind the causality.** An earlier version of this comment said that below the floor "the archives come
/// back empty and the test proves nothing" — and the empty archives this test really suffered were **G0b**
/// (a machine-baked levels elite in `config.ron`, so the squad fought a different map and was wiped; see the
/// History note above), *not* the tick count. Re-measured 2026-07-17: every held-in seed passes the criterion
/// even at 1800, so a short episode **thins** archives rather than emptying them. The floor is real; this
/// comment was citing it for a failure it did not cause, and that misattribution later cost a reader a full
/// investigation.
const EPISODE_TICKS: u32 = 7200;

/// A bit-exact fingerprint of one archive: every occupied cell in the archive's fixed sorted order, with
/// each `f32` compared by its bits (identical computations must produce identical bits — no epsilon).
fn fingerprint<G>(pop: &Population<G>) -> Vec<((usize, usize), u32, u32, u32, u64)> {
    pop.archive
        .iter()
        .map(|(cell, e)| {
            (
                *cell,
                e.descriptor.aggression.to_bits(),
                e.descriptor.exploration.to_bits(),
                e.fitness.to_bits(),
                e.genome,
            )
        })
        .collect()
}

#[test]
fn parallel_search_reproduces_the_inline_archives_bit_for_bit() {
    // No `serial_guard()` here: every `rollout` (inline, and inside each worker process) takes the
    // non-reentrant `HARNESS_LOCK` itself, so holding it here too would deadlock. `--test-threads=1`
    // already serialises tests within this process; the worker pool's rollouts run in *other* processes,
    // each with its own lock.

    // Workers are this crate's `train` binary re-invoked as `train worker`. Under `cargo test`,
    // `current_exe()` is the test harness, so point the pool at the real `train` binary. Safe here: the
    // harness lane is single-threaded, so no other thread races this env write.
    unsafe { std::env::set_var("TRAIN_WORKER_EXE", env!("CARGO_BIN_EXE_train")) };

    let t = Templates::authored();
    // Build the frozen prior in-memory (the same sweep `train prior` writes to disk), so the test needs no
    // gitignored `baseline_prior.ron`. Both searches score against this identical reference.
    let prior = sweep_prior(&t, &SEEDS, EPISODE_TICKS).expect("sweep the authored prior");

    // A small but non-trivial search: two generations (so a gen-1 child can contest a gen-0 cell and drive
    // the common-opponent re-evaluation — the other parallelised path), batch 1, both held-in worlds.
    let base = SearchConfig {
        seed: 0xC0FFEE,
        generations: 2,
        batch: 1,
        episode_ticks: EPISODE_TICKS,
        dungeon_seeds: SEEDS.to_vec(),
        resolution: 8,
        jobs: 1,
        patience: 0, // no early-stop: exercise the full fixed-length run
    };

    let inline = search(&t, &prior, &base, |_, _| {}).expect("inline search");
    let parallel = search(
        &t,
        &prior,
        &SearchConfig { jobs: 3, ..clone_cfg(&base) },
        |_, _| {},
    )
    .expect("parallel search");

    // The run must actually fill niches, or "identical" would be the trivial empty-vs-empty match.
    let filled = inline.squad.archive.coverage()
        + inline.swarm.archive.coverage()
        + inline.world.archive.coverage();
    assert!(filled > 0, "search illuminated no niches — cannot prove determinism on empty archives");

    assert_eq!(fingerprint(&inline.squad), fingerprint(&parallel.squad), "squad archive diverged");
    assert_eq!(fingerprint(&inline.swarm), fingerprint(&parallel.swarm), "swarm archive diverged");
    assert_eq!(fingerprint(&inline.world), fingerprint(&parallel.world), "world archive diverged");

    // The bookkeeping counters must match too: same evaluations, same infeasible/criterion rejections.
    assert_eq!(inline.evaluations, parallel.evaluations, "evaluation count diverged");
    assert_eq!(inline.rejected_infeasible, parallel.rejected_infeasible, "infeasible count diverged");
    assert_eq!(
        inline.rejected_by_criterion, parallel.rejected_by_criterion,
        "criterion-rejection count diverged"
    );
}

/// The batch-emitter scaling proof: with the batch variant of MAP-Elites (`batch_population`), a whole
/// generation's `batch × OPPONENTS` triples are flattened into ONE eval call, so `--jobs` scales past the old
/// `OPPONENTS = 3` ceiling. This pins that the lift stays bit-exact — `batch 2` (so two children are flattened
/// per generation, and can contest the same cell inside one batch, driving the deferred re-eval) with
/// `jobs 4` (> OPPONENTS) must reproduce the inline (jobs 1) archives exactly. The parallel-vs-sequential
/// (jobs=1 ≡ jobs=N) equivalence test for the new emitter. Kept to one generation to hold the 7200-tick
/// rollout budget down; the `batch = 1`, two-generation case above pins the cross-generation re-eval path.
#[test]
fn batch_emitter_scales_past_opponents_deterministically() {
    unsafe { std::env::set_var("TRAIN_WORKER_EXE", env!("CARGO_BIN_EXE_train")) };
    let t = Templates::authored();
    let prior = sweep_prior(&t, &SEEDS, EPISODE_TICKS).expect("sweep the authored prior");

    let base = SearchConfig {
        seed: 0x0BA7C4,
        generations: 1,
        batch: 2, // > 1: multiple children flattened into one eval per generation (the batch path)
        episode_ticks: EPISODE_TICKS,
        dungeon_seeds: SEEDS.to_vec(),
        resolution: 8,
        jobs: 1,
        patience: 0, // no early-stop: exercise the full fixed-length run
    };

    let inline = search(&t, &prior, &base, |_, _| {}).expect("inline search");
    // jobs 4 > OPPONENTS (3): only reachable because the emitter now batches the whole generation.
    let parallel = search(&t, &prior, &SearchConfig { jobs: 4, ..clone_cfg(&base) }, |_, _| {})
        .expect("parallel search (jobs 4)");

    let filled = inline.squad.archive.coverage()
        + inline.swarm.archive.coverage()
        + inline.world.archive.coverage();
    assert!(filled > 0, "search illuminated no niches — cannot prove determinism on empty archives");

    assert_eq!(fingerprint(&inline.squad), fingerprint(&parallel.squad), "squad archive diverged at jobs=8");
    assert_eq!(fingerprint(&inline.swarm), fingerprint(&parallel.swarm), "swarm archive diverged at jobs=8");
    assert_eq!(fingerprint(&inline.world), fingerprint(&parallel.world), "world archive diverged at jobs=8");
    assert_eq!(inline.evaluations, parallel.evaluations, "evaluation count diverged");
    assert_eq!(inline.rejected_infeasible, parallel.rejected_infeasible, "infeasible count diverged");
    assert_eq!(
        inline.rejected_by_criterion, parallel.rejected_by_criterion,
        "criterion-rejection count diverged"
    );
}

/// `SearchConfig` isn't `Clone` (it lives next to the search, not as a value type), so spell out the copy
/// the struct-update syntax needs.
fn clone_cfg(cfg: &SearchConfig) -> SearchConfig {
    SearchConfig {
        seed: cfg.seed,
        generations: cfg.generations,
        batch: cfg.batch,
        episode_ticks: cfg.episode_ticks,
        dungeon_seeds: cfg.dungeon_seeds.clone(),
        resolution: cfg.resolution,
        jobs: cfg.jobs,
        patience: cfg.patience,
    }
}
