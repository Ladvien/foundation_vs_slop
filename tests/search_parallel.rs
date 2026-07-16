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
//! # CURRENTLY RED — a REAL inline-vs-parallel archive divergence (G0c). Two earlier causes are gone.
//!
//! History, because this header has been wrong twice:
//!
//! 1. It once blamed **G0** (rollout non-determinism) and asserted these were "not a bug in the batch
//!    emitter". G0 was real — `laser::fire_laser` drew a shared RNG stream in ECS query order — and is now
//!    **fixed**, pinned by `replay::search_rollouts_are_reproducible_under_load`. But G0 was never why these
//!    tests were red.
//! 2. They were actually failing at `assert!(filled > 0)` — the archive was **empty**, so no determinism
//!    comparison ever ran. Cause: `config.ron` held a machine-baked levels elite instead of the authored
//!    level, so the squad fought a different map and was wiped. Restoring the authored level fixed that;
//!    both held-in seeds now pass the minimal criterion.
//!
//! **What is red now (G0c) is what these tests were built to catch.** The archive fills, the fingerprint
//! comparison runs, and `jobs = 1` and `jobs = N` genuinely disagree:
//!
//! ```text
//! left  (jobs=1): [((0,0), 1025887771, 1027607250, 1029279680, 0)]
//! right (jobs=3): [((0,0), 0,          1032948542, 1048302511, 1)]
//! ```
//!
//! # What G0c is NOT (all read end-to-end, session 3)
//!
//! **It is not the reduction, and it is not this file's fault.** `WorkerPool::eval` is index-addressed
//! (`slots[idx]`), so results come back in INPUT order; `batch_population` Phase 3 inserts in pinned predraw
//! order; every seed is drawn serially BEFORE any fan-out; the bincode wire is bit-exact and doubly pinned;
//! `coevolve::mean` bit-sorts before summing. Auditing the reduction finds nothing — do not re-tread it.
//!
//! # The two real asymmetries
//!
//! 1. **Work assignment is a race.** Workers are long-lived and steal jobs off a shared
//!    `AtomicUsize::fetch_add` (`parallel.rs:105,121`). Which process runs a triple — and at what ordinal in
//!    that process's `App` sequence — is decided by the OS scheduler. Inline runs every triple sequentially
//!    in one process that has also already built the `sweep_prior` `App`s. So **these tests quietly demand
//!    that a rollout be a pure function of its inputs REGARDLESS of how many `App`s preceded it in the
//!    process.** Nothing states that invariant; it may not hold.
//! 2. **`jobs=3` IS the load.** Three contending worker processes are exactly the condition that exposes
//!    order-dependence bugs here; `jobs=1` is exactly the quiet condition that HIDES them (a quiet box
//!    returned 12/12 identical rollouts with G0 live). The two arms of this comparison differ in the one
//!    variable that gates the bug's visibility — so `inline != parallel` is fully explained by residual
//!    rollout non-determinism with zero parallelism faults.
//!
//! Amplifier: `try_insert_with_reeval`'s `s >= challenger_fitness` (`coevolve.rs:395`) is a razor-thin float
//! tiebreak — 1 ULP flips cell ownership, which changes the next generation's parent.
//!
//! **Diagnostic — and the obvious one lies.** "Run the inline search twice" is INVALID on a quiet box:
//! `inline == inline` is exactly what a live bug produces there. The inline arm must generate background
//! load (see `replay::search_rollouts_are_reproducible_under_load`). Run inline ×2 under load AND parallel
//! ×2: `inline != inline` ⇒ plain rollout non-determinism; `parallel != parallel` ⇒ same class, amplified;
//! both stable but `inline != parallel` ⇒ process-history (`App`-ordinal) dependence.
//!
//! Already tried and NOT sufficient: canonicalising `update_lasers` (bolt order, the friendly-fire draw, the
//! `LastAttacker` last-writer-wins pick, the `THREAT_GUN` deposit) and the ORCA neighbour sort's tiebreak.
//! Both were real bugs of the G0 class and are fixed — they just aren't this one.
//! See `docs/rl/2026-07-16-search-rollout-nondeterminism.md` §G0c.
#![cfg(feature = "test-harness")]

use foundation_vs_slop::squad_ai::coevolve::{search, sweep_prior, Population, SearchConfig, Templates};

/// Two held-in worlds — the minimum `evolve` allows (a candidate's two rollouts must run on different
/// worlds). Kept to two to hold the test's rollout budget down.
const SEEDS: [u64; 2] = [0x5C09191, 0xA11CE];

/// The calibrated episode floor. Below it the authored-derived children take no damage on some worlds and
/// the criterion rejects them, so the archives come back empty and the test proves nothing.
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
    }
}
