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
