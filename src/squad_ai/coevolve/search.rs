//! The co-evolutionary search loop itself, plus the common-opponent re-evaluation that keeps a
//! non-stationary fitness comparable across generations.
//! Split out of the former single-file `coevolve.rs`; a pure move (FVS-N-3).

use super::*;
use crate::squad_ai::surprise::containment_criterion;

// ── The search ───────────────────────────────────────────────────────────────────────────────────

/// The **held-in dungeon seeds** every search evaluates against. Defined once, on purpose: a re-selection
/// must not be able to leave a stale copy behind. It already has — `0xA11CE` and `0xBEEF` were retired when
/// the mold landed (see `mold::MoldConfig`'s `Default`), and survived for months in docs and test comments
/// that still called them "held-in", long enough to mislead a later reader into re-tuning the episode floor
/// against a world the search no longer runs. `0xB0BA` was retired on 2026-07-19 for the same class of
/// reason: baking the searched audio elite into `config.ron` (the `audio:` slice) made the swarm's acoustic
/// coordination more lethal, tipping that knife-edge world from a one-hit-point survival (489 dmg) into a
/// squad wipe (779 dmg); it was replaced by `0xD00D`. `0xD00D` was itself retired on 2026-07-20 when the
/// squad-member mesh was swapped (the greybox figurine → the VALKYRIE rig): the taller mesh fractures into
/// more gib chunks per death (`autogib` bakes 23 vs the greybox's handful), and each death's larger meat
/// "magnet" draws the swarm harder onto the living squad — tipping `0xD00D` from 5/5 with margin (205 dmg)
/// into a wipe (699 dmg). It is replaced by `0xFEED`, where the authored squad survives 5/5 with margin
/// (83 dmg at 7200) and the swarm survives — re-verified by
/// `the_authored_brains_produce_a_real_encounter_on_every_world`.
///
/// Chosen so the shipped squad produces a real encounter on each: it survives (with the swarm also alive),
/// so neither side is wiped. Under the heavier VALKYRIE mesh the whole set runs a touch hotter — `0x5C09191`
/// now settles to 3/5 by 5400 ticks (still a clear margin above a wipe), while `0x1CE5`/`0xFEED` hold 5/5.
pub const HELD_IN_SEEDS: [u64; 3] = [0x5C09191, 0x1CE5, 0xFEED];

/// Everything one run needs. `episode_ticks` at 60 Hz: 7200 ≈ 120 s of simulated time.
///
/// 120 s is a **floor**, not a preference, and it is measured (`train probe`) rather than chosen. The
/// evaluation alternates player-ordered advance with AI-controlled engagement (see `evaluate`), so only part
/// of the episode moves the squad toward the nests.
///
/// **Measured 2026-07-20** — re-measured after the squad-member mesh swap to the VALKYRIE rig (which retired
/// the knife-edge seed `0xD00D`; see [`HELD_IN_SEEDS`]). Authored brains, `train probe --ticks N`, reporting
/// `unit_damage_taken` per held-in seed (survivor count shown only where it is not 5/5):
///
/// | seed | 1800 | 3600 | 5400 | 7200 |
/// |---|---|---|---|---|
/// | `0x5C09191` | 63 | 63 | 258 (3/5) | 258 (3/5) |
/// | `0x1CE5` | 23 | 182 | 214 | 291 |
/// | `0xFEED` | 77 | 83 | 83 | 83 |
///
/// Every cell passes `minimal_criterion`: survivors ≥ 1 with the swarm alive and real damage taken. Unlike
/// the greybox table, the heavier mesh makes every world draw first blood by 1800 ticks, so no cell sits at
/// the sub-half-hit-point `0.0` that the old floor was pinned to.
///
/// **The 7200 floor is retained** — it is `SearchConfig::default()`, co-calibrated with the archive
/// resolution rather than by `minimal_criterion` alone. Under the greybox, `0x1CE5` was the *binding* world
/// (no measurable stakes until 7200), and that knife-edge is what set the floor; the heavier VALKYRIE mesh
/// removed it (every seed now takes damage early, and `0x5C09191` even attrits to 3/5 by 5400). Re-deriving a
/// possibly-lower floor is a separate measurement (archive thinning + replayability spread) deliberately left
/// to a dedicated `train probe` sweep, not lowered opportunistically here.
///
/// A shorter episode does not make the search cheaper; it makes it thin.
///
/// **Re-measure with `train probe` after anything that moves the deterministic trajectory.** These numbers
/// are a snapshot, not a law. The previous snapshot went stale silently and was later read as authoritative.
pub struct SearchConfig {
    pub seed: u64,
    pub generations: u32,
    /// Children proposed per side per generation.
    pub batch: u32,
    pub episode_ticks: u32,
    /// Held-in dungeon seeds. Each candidate's two rollouts draw two *different* seeds from this set, so
    /// learnability measures behaviour that generalises across worlds rather than a memorised map.
    pub dungeon_seeds: Vec<u64>,
    pub resolution: usize,
    /// How many worker **processes** evaluate rollouts in parallel. `1` (the default) runs every rollout
    /// inline in this process — the reference path. `N > 1` spawns `N` `train worker` subprocesses and
    /// fans a whole generation's `batch × OPPONENTS` independent triples (per population) across them — the
    /// batch MAP-Elites emitter (`batch_population`). Parallelism must be across *processes*, never threads:
    /// `sim_harness` holds a process-wide lock and pins the compute pool to one thread for determinism (see
    /// `evaluate` module doc). Because `score_triple_compact` draws no search RNG and a rollout is a pure
    /// function of its `(brains, world, seed, ticks)`, the fan-out reduces in the exact input order and the
    /// archives are **byte-identical** to `jobs = 1` (proved by `tests/search_parallel.rs`). The useful
    /// ceiling is now `batch × OPPONENTS` (per population per generation) — a whole batch is scored at once —
    /// so `jobs` scales to the box; raise `batch` for more width.
    pub jobs: usize,
    /// Convergence early-stop: stop when the combined QD-score of the three archives has not improved for
    /// this many consecutive generations (Mouret & Clune 2015 archive-property termination). `0` disables it
    /// (run every generation). Bit-reproducible — see [`crate::squad_ai::qd::PlateauStop`].
    pub patience: u32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            seed: 0xC0FFEE,
            generations: 8,
            batch: 4,
            episode_ticks: 7200,
            dungeon_seeds: HELD_IN_SEEDS.to_vec(),
            resolution: 8,
            jobs: 1,
            patience: 0,
        }
    }
}

/// Draw two DIFFERENT held-in dungeon seeds (the second differs whenever the set allows), so learnability
/// measures behaviour that generalises across worlds rather than a memorised map. Split out of the old
/// `evaluate_pair` so a challenger's *exact* worlds can be replayed against an incumbent in the Phase-5
/// common-opponent re-evaluation.
fn draw_two_seeds(seeds: &[u64], rng: &mut ChaCha8Rng) -> (u64, u64) {
    let i = rng.below(seeds.len());
    let j = if seeds.len() > 1 {
        let mut j = rng.below(seeds.len() - 1);
        if j >= i {
            j += 1;
        }
        j
    } else {
        i
    };
    (seeds[i], seeds[j])
}

/// One triple to evaluate: the three genomes and the seed pair. The unit of parallel work — a worker
/// process needs nothing else (it rebuilds `Templates` and holds the frozen prior), and a rollout is a
/// pure function of `(brains, world, seed, ticks)`, so this is a self-contained, order-independent job.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TripleJob {
    pub squad: SquadGenome,
    pub swarm: SwarmGenome,
    pub world: WorldGenome,
    pub seed_a: u64,
    pub seed_b: u64,
    pub ticks: u32,
}

/// The wire-friendly result of a [`TripleJob`]: the scalar fitness plus all three descriptors (cheap —
/// four `f32` pairs). The heavy `EpisodeTrace` is reduced worker-side and never crosses the process
/// boundary. `None` (at the `Option` layer above) means the triple failed the minimal criterion.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub(crate) struct TripleScore {
    pub score: f32,
    pub squad: BehaviorDescriptor,
    pub swarm: BehaviorDescriptor,
    pub world: BehaviorDescriptor,
}

/// Evaluate one `(squad, swarm, world)` triple, reduced to the compact [`TripleScore`]: two rollouts on two
/// given worlds (`seed_a`, `seed_b`), with the evolved world config installed on both. `None` when either
/// rollout fails the behavioural minimal criterion — no real encounter, so nothing to score. Fitness is the
/// unchanged `W·S·L` against the frozen prior; each population reads its own descriptor off the first trace.
/// This is the exact computation a worker performs, and the inline path performs too, so both routes are
/// provably identical.
pub(crate) fn score_triple_compact(
    t: &Templates,
    squad: &SquadGenome,
    swarm: &SwarmGenome,
    world: &WorldGenome,
    prior: &ModePrior,
    seed_a: u64,
    seed_b: u64,
    episode_ticks: u32,
) -> Result<Option<TripleScore>, String> {
    let wc = world_genome::decode(world)?;
    let a = rollout(brains_of(t, squad, swarm)?, Some(wc), None, None, seed_a, episode_ticks);
    // FVS-I-1: the containment constraint is applied HERE, at the world archive, and nowhere else.
    // `minimal_criterion` is the shared gate every other evaluator uses; a level or an audio config is
    // not infeasible because nobody captured anything. See `surprise::containment_criterion`.
    if minimal_criterion(&a.outcome).is_err() || containment_criterion(&a.outcome).is_err() {
        return Ok(None);
    }
    let b = rollout(brains_of(t, squad, swarm)?, Some(wc), None, None, seed_b, episode_ticks);
    if minimal_criterion(&b.outcome).is_err() || containment_criterion(&b.outcome).is_err() {
        return Ok(None);
    }
    Ok(Some(TripleScore {
        score: fitness(&a.trace, &b.trace, prior).score(),
        squad: squad_descriptor(&a.trace, &a.outcome),
        swarm: swarm_descriptor(&a.trace),
        world: world_descriptor(&a.outcome),
    }))
}

/// Where a batch of [`TripleJob`]s is evaluated. `Inline` runs them in this process (the reference path,
/// and what the tests and single-core runs use); `Pool` fans them across worker processes. Both call the
/// identical [`score_triple_compact`], and both return results in input order, so the choice never changes
/// what the search computes — only how fast.
pub(crate) enum Evaluator<'a> {
    Inline { t: &'a Templates, prior: &'a ModePrior },
    Pool(crate::squad_ai::parallel::WorkerPool),
}

impl Evaluator<'_> {
    /// Evaluate `jobs`, preserving order. Any error (a decode/feasibility bug, or a worker dying) is fatal
    /// and propagates — never a silent skip, which would corrupt the archive with a candidate scored on
    /// fewer opponents than it was credited for.
    fn eval(&self, jobs: &[TripleJob]) -> Result<Vec<Option<TripleScore>>, String> {
        match self {
            Evaluator::Inline { t, prior } => jobs
                .iter()
                .map(|j| {
                    score_triple_compact(t, &j.squad, &j.swarm, &j.world, prior, j.seed_a, j.seed_b, j.ticks)
                })
                .collect(),
            Evaluator::Pool(pool) => pool.eval(jobs),
        }
    }
}

/// Both archives after a run.
pub struct SearchResult {
    pub squad: Population<SquadGenome>,
    pub swarm: Population<SwarmGenome>,
    pub world: Population<WorldGenome>,
    pub evaluations: u32,
    pub rejected_infeasible: u32,
    pub rejected_by_criterion: u32,
}

/// Run the co-evolutionary search. `report` is called once per generation so a driver can log or
/// checkpoint; nothing here writes to disk.
pub fn search(
    t: &Templates,
    prior: &ModePrior,
    cfg: &SearchConfig,
    mut report: impl FnMut(u32, &SearchResult),
) -> Result<SearchResult, String> {
    prior.validate()?;
    if cfg.dungeon_seeds.is_empty() {
        return Err("search needs at least one dungeon seed".into());
    }

    let mut rng = seeded(cfg.seed);
    let authored_squad = SquadGenome::authored(t);
    let authored_swarm = SwarmGenome::authored(t);
    let authored_world = world_genome::authored();

    let mut result = SearchResult {
        squad: Population::new(cfg.resolution),
        swarm: Population::new(cfg.resolution),
        world: Population::new(cfg.resolution),
        evaluations: 0,
        rejected_infeasible: 0,
        rejected_by_criterion: 0,
    };

    // Where rollouts run. `jobs = 1` (default) evaluates inline in this process — the reference path the
    // tests pin. `jobs > 1` spawns a worker pool; it changes only *where* each triple is scored, never the
    // result (see `parallel` module doc). The pool lives for the whole search and is torn down on return.
    let evaluator = if cfg.jobs <= 1 {
        Evaluator::Inline { t, prior }
    } else {
        Evaluator::Pool(crate::squad_ai::parallel::WorkerPool::spawn(cfg.jobs, prior, t)?)
    };

    // Each generation mutates one child per population and scores it against `OPPONENTS` pairs drawn from
    // the OTHER two archives (three-way co-evolution in the spirit of POET, arXiv:1901.01753, and multi-
    // agent autocurricula, Baker et al. arXiv:1909.07528): a squad child fights (swarm, world) pairs, a
    // swarm child fights (squad, world), and a world child is judged by the (squad, swarm) it induces.
    // Convergence early-stop on the three archives' combined QD-score (no-op when `cfg.patience == 0`).
    let mut plateau = crate::squad_ai::qd::PlateauStop::new(cfg.patience);
    for generation in 0..cfg.generations {
        // Each population's whole generation is proposed and scored as ONE batch (see `batch_population`):
        // `cfg.batch` children against a frozen archive snapshot, every child's `OPPONENTS` triples flattened
        // into a single `eval` call so `--jobs` scales to `batch × OPPONENTS` workers, inserted in a pinned
        // order. Sub-phases stay ordered squad→swarm→world within a generation (a swarm child still fights
        // this generation's freshly-inserted squad elites), which keeps the three-way autocurriculum tight.

        // ── squad children vs (swarm, world) opponents ──
        batch_population(
            cfg,
            &mut rng,
            &evaluator,
            &mut result.squad,
            &result.swarm,
            &result.world,
            &authored_squad,
            &authored_swarm,
            &authored_world,
            &mut result.evaluations,
            &mut result.rejected_infeasible,
            &mut result.rejected_by_criterion,
            |parent, rng, rejected| propose_squad(t, parent, rng, rejected),
            |_child| Ok(()),
            |child, swarm, world| {
                feasible(t, child, swarm)?;
                world_genome::is_feasible(world)
            },
            |c, swarm, world, sa, sb| TripleJob {
                squad: c.clone(),
                swarm: swarm.clone(),
                world: world.clone(),
                seed_a: sa,
                seed_b: sb,
                ticks: cfg.episode_ticks,
            },
            |s| s.squad,
        )?;

        // ── swarm children vs (squad, world) opponents ──
        batch_population(
            cfg,
            &mut rng,
            &evaluator,
            &mut result.swarm,
            &result.squad,
            &result.world,
            &authored_swarm,
            &authored_squad,
            &authored_world,
            &mut result.evaluations,
            &mut result.rejected_infeasible,
            &mut result.rejected_by_criterion,
            |parent, rng, rejected| propose_swarm(t, parent, rng, rejected),
            |_child| Ok(()),
            |child, squad, world| {
                feasible(t, squad, child)?;
                world_genome::is_feasible(world)
            },
            |c, squad, world, sa, sb| TripleJob {
                squad: squad.clone(),
                swarm: c.clone(),
                world: world.clone(),
                seed_a: sa,
                seed_b: sb,
                ticks: cfg.episode_ticks,
            },
            |s| s.swarm,
        )?;

        // ── world children vs (squad, swarm) opponents ──
        batch_population(
            cfg,
            &mut rng,
            &evaluator,
            &mut result.world,
            &result.squad,
            &result.swarm,
            &authored_world,
            &authored_squad,
            &authored_swarm,
            &mut result.evaluations,
            &mut result.rejected_infeasible,
            &mut result.rejected_by_criterion,
            |parent, rng, _rejected| propose_world(parent, rng),
            |child| world_genome::is_feasible(child),
            |_child, squad, swarm| feasible(t, squad, swarm),
            |c, squad, swarm, sa, sb| TripleJob {
                squad: squad.clone(),
                swarm: swarm.clone(),
                world: c.clone(),
                seed_a: sa,
                seed_b: sb,
                ticks: cfg.episode_ticks,
            },
            |s| s.world,
        )?;

        report(generation, &result);
        let qd_total = result.squad.archive.qd_score()
            + result.swarm.archive.qd_score()
            + result.world.archive.qd_score();
        if plateau.should_stop(qd_total) {
            break;
        }
    }
    Ok(result)
}

/// Sample `OPPONENTS` opponents from an archive, falling back to `OPPONENTS` copies of the authored genome
/// while the archive is still empty — so an opponent set is always exactly `OPPONENTS` long and two sets
/// (one per other population) can be paired index-by-index into `(a, b)` opponents for one triple.
fn sample_or_authored<G: Clone>(pop: &Population<G>, authored: &G, rng: &mut ChaCha8Rng) -> Vec<G> {
    let sampled = pop.sample_opponents(OPPONENTS, rng);
    if sampled.is_empty() {
        vec![authored.clone(); OPPONENTS]
    } else {
        // `sample_opponents` draws `OPPONENTS` with replacement, so this is already that length.
        sampled.into_iter().cloned().collect()
    }
}

/// Propose and score ONE population's whole generation as a batch, then insert — the **batch variant of
/// MAP-Elites** (Mouret & Clune 2015, *Illuminating search spaces by mapping elites*, arXiv:1504.04909,
/// §"batch": "a batch of `b` individuals is generated and evaluated in parallel before the map is updated";
/// parallel-scaling rationale: Colas, Madhavan, Huizinga & Clune 2020, *Scaling MAP-Elites to Deep
/// Neuroevolution*, doi:10.1145/3377930.3390217). The three co-evolving populations share this exact
/// Predraw/Eval/Insert structure and differ only in where the child sits in a triple, so it is parameterized:
///
/// - `propose` mutates a sampled parent into a feasible child (the brains rejection-sample; the world child
///   is feasible by construction);
/// - `pre_check` gates the child itself (the world child screens its own knobs; the brains pass `|_| Ok(())`);
/// - `check` screens one opponent pair (the two feasibility calls, which draw no RNG);
/// - `make_job` places a child-role genome into the correct triple slot beside its two opponents — used both
///   for the forward jobs (with `child`) and, inside `try_insert_with_reeval`, to re-score a surviving
///   incumbent on the challenger's exact recorded conditions (with `incumbent`);
/// - `select` picks this population's descriptor axis off a [`TripleScore`].
///
/// **Determinism + parallelism-invariance.** All RNG (parent pick, the variable-length `propose` redraws, the
/// two opponent samples, and each triple's `draw_two_seeds`) is consumed serially in child-then-opponent
/// order during PREDRAW, before any rollout — and a rollout draws none. So the whole generation's `batch ×
/// OPPONENTS` triples can be flattened into ONE `evaluator.eval` call: it reduces in input order, and inserts
/// are applied in the pinned predraw order (so the `>=` elitism tie-break in `try_insert_with_reeval` and the
/// contested-cell re-evals are reproducible). `jobs=1` (inline) and `jobs=N` (pool) therefore produce
/// bit-identical archives — the `--jobs` ceiling rises from `OPPONENTS` (3) to `batch × OPPONENTS`. Children
/// are proposed against the archive as it stands at the start of this sub-phase (inserts deferred) — the
/// standard online→batch trade.
#[allow(clippy::too_many_arguments)]
fn batch_population<C: Clone, O1: Clone, O2: Clone>(
    cfg: &SearchConfig,
    rng: &mut ChaCha8Rng,
    evaluator: &Evaluator,
    population: &mut Population<C>,
    opp_pop1: &Population<O1>,
    opp_pop2: &Population<O2>,
    authored_child: &C,
    authored_opp1: &O1,
    authored_opp2: &O2,
    evaluations: &mut u32,
    rejected_infeasible: &mut u32,
    rejected_by_criterion: &mut u32,
    propose: impl Fn(&C, &mut ChaCha8Rng, &mut u32) -> Result<C, String>,
    pre_check: impl Fn(&C) -> Result<(), String>,
    check: impl Fn(&C, &O1, &O2) -> Result<(), String>,
    make_job: impl Fn(&C, &O1, &O2, u64, u64) -> TripleJob,
    select: impl Fn(&TripleScore) -> BehaviorDescriptor,
) -> Result<(), String> {
    // One child's forward conditions, carried from predraw to the deferred insert.
    struct Pending<C, O1, O2> {
        child: C,
        recorded: Vec<(O1, O2, u64, u64)>,
    }

    // Phase 1 — PREDRAW: propose every child against the frozen (start-of-sub-phase) archives and build all
    // their triples. This is the only place RNG is consumed, in a fixed serial order.
    let mut pending: Vec<Pending<C, O1, O2>> = Vec::with_capacity(cfg.batch as usize);
    let mut all_jobs: Vec<TripleJob> = Vec::with_capacity(cfg.batch as usize * OPPONENTS);
    for _ in 0..cfg.batch {
        let parent = population.sample_parent(rng).cloned().unwrap_or_else(|| authored_child.clone());
        let child = propose(&parent, rng, rejected_infeasible)?;
        let opps1 = sample_or_authored(opp_pop1, authored_opp1, rng);
        let opps2 = sample_or_authored(opp_pop2, authored_opp2, rng);
        // A one-time gate on the child itself (the world child screens its own knobs). A failure is a bug.
        pre_check(&child)?;
        let mut recorded: Vec<(O1, O2, u64, u64)> = Vec::with_capacity(opps1.len());
        for (o1, o2) in opps1.iter().zip(&opps2) {
            // Feasible by construction; a failure here is a bug, not a candidate to skip.
            check(&child, o1, o2)?;
            *evaluations += 1;
            let (sa, sb) = draw_two_seeds(&cfg.dungeon_seeds, rng);
            all_jobs.push(make_job(&child, o1, o2, sa, sb));
            recorded.push((o1.clone(), o2.clone(), sa, sb));
        }
        pending.push(Pending { child, recorded });
    }

    // Phase 2 — EVAL the whole generation's triples in one flattened call (up to `batch × OPPONENTS`,
    // order preserved). This is where `--jobs` now scales past `OPPONENTS`.
    let outcomes = evaluator.eval(&all_jobs)?;

    // Phase 3 — INSERT in the pinned predraw order. Splitting `outcomes` by each child's job count keeps the
    // reduce identical to the per-child path; the fixed order makes the elitism tie-break reproducible.
    let mut cursor = 0usize;
    for p in pending {
        let n = p.recorded.len();
        let slice = &outcomes[cursor..cursor + n];
        cursor += n;

        let mut scores = Vec::new();
        let mut descriptors = Vec::new();
        let mut kept: Vec<(O1, O2, u64, u64)> = Vec::new();
        for (outcome, rec) in slice.iter().zip(p.recorded) {
            match outcome {
                Some(s) => {
                    scores.push(s.score);
                    descriptors.push(select(s));
                    kept.push(rec);
                }
                None => *rejected_by_criterion += 1,
            }
        }
        if scores.is_empty() {
            continue;
        }
        let descriptor = mean_descriptor(&descriptors);
        let challenger_fitness = mean(&scores);
        population.try_insert_with_reeval(descriptor, challenger_fitness, p.child.clone(), |incumbent| {
            reeval_on_recorded(evaluator, &kept, |rec| make_job(incumbent, &rec.0, &rec.1, rec.2, rec.3))
        })?;
    }
    Ok(())
}

// ── Common-opponent re-evaluation (the Phase-5 non-stationarity fix) ────────────────────────────────
//
// `reeval_on_recorded` re-scores an incumbent on a challenger's EXACT recorded opponents and seeds, so the two
// are compared under identical conditions before `try_insert_with_reeval`'s elitism test. `None` means the
// incumbent produced no real encounter on any of them (inadmissible here) — the challenger, which did, wins.
// No fresh RNG is drawn (recorded seeds are replayed), so the run stays reproducible.
//
// SERIAL_GUARD: each `rollout` inside `score_triple_compact` acquires the non-reentrant `HARNESS_LOCK` itself
// and releases it before the next, so these sequential re-eval rollouts are safe. `search()` must therefore
// NEVER hold `serial_guard` around the generation loop — doing so (e.g. to "reuse one lock") would deadlock
// the very first re-eval on the lock the loop already holds.
//
// The `to_job` closure (supplied by `score_and_insert`'s `make_job`) drops the incumbent into whichever triple
// slot this population owns, beside the two opponents pulled from each recorded tuple.
fn reeval_on_recorded<R>(
    evaluator: &Evaluator,
    recorded: &[R],
    to_job: impl Fn(&R) -> TripleJob,
) -> Result<Option<f32>, String> {
    let jobs: Vec<TripleJob> = recorded.iter().map(to_job).collect();
    let scores: Vec<f32> = evaluator.eval(&jobs)?.into_iter().flatten().map(|s| s.score).collect();
    Ok(if scores.is_empty() { None } else { Some(mean(&scores)) })
}

/// Mutate every role's genome. The band origin is the *template*, derived inside `genome::mutate`, so it
/// cannot drift with the parent.
pub(crate) fn mutate_squad(
    t: &Templates,
    parent: &SquadGenome,
    rng: &mut ChaCha8Rng,
) -> Result<SquadGenome, String> {
    if parent.0.len() != t.roles.len() {
        return Err(format!("squad genome has {} roles, expected {}", parent.0.len(), t.roles.len()));
    }
    let mut out = Vec::with_capacity(parent.0.len());
    for (template, p) in t.roles.iter().zip(&parent.0) {
        out.push(mutate(template, p, SIGMA, RANK_SWAP_P, rng)?);
    }
    Ok(SquadGenome(out))
}

pub(crate) fn mutate_swarm(
    t: &Templates,
    parent: &SwarmGenome,
    rng: &mut ChaCha8Rng,
) -> Result<SwarmGenome, String> {
    Ok(SwarmGenome {
        crab: mutate(&t.crab, &parent.crab, SIGMA, RANK_SWAP_P, rng)?,
        scout: mutate(&t.scout, &parent.scout, SIGMA, RANK_SWAP_P, rng)?,
        smiley: mutate(&t.smiley, &parent.smiley, SIGMA, RANK_SWAP_P, rng)?,
        bear: mutate(&t.bear, &parent.bear, SIGMA, RANK_SWAP_P, rng)?,
        bear_copy: mutate(&t.bear_copy, &parent.bear_copy, SIGMA, RANK_SWAP_P, rng)?,
    })
}

/// Propose a world child. `world_genome::mutate` clamps every knob into its hard `BOUNDS`, so a child is
/// feasible **by construction** — no rejection-sampling loop, unlike the brains (whose feasibility is a
/// value-space guard the mutation can violate). `is_feasible` is still asserted: a failure would be a
/// `BOUNDS` bug, and one path means surfacing it loudly rather than searching an infeasible world.
fn propose_world(parent: &WorldGenome, rng: &mut ChaCha8Rng) -> Result<WorldGenome, String> {
    let child = world_genome::mutate(parent, WORLD_SIGMA, rng)?;
    world_genome::is_feasible(&child)?;
    Ok(child)
}

/// Mean of a non-empty slice. Sorted before summing so the result does not depend on evaluation order —
/// float addition is not associative, and the whole run must be reproducible from its seed.
///
/// Bit order is a CANONICAL order, not a numeric one: negative floats would sort after positives (and
/// reversed among themselves). That is still a total order — determinism only needs the summation order
/// to be a fixed function of the multiset — so this stays correct if a fitness ever goes negative; only
/// the (unspecified anyway) summation sequence would look odd.
pub(crate) fn mean(xs: &[f32]) -> f32 {
    let mut sorted: Vec<u32> = xs.iter().map(|x| x.to_bits()).collect();
    // SORT-OK: bare f32 bits about to be summed (`mean`) — ties are identical terms.
    sorted.sort_unstable();
    let sum: f32 = sorted.iter().map(|b| f32::from_bits(*b)).sum();
    sum / xs.len() as f32
}

fn mean_descriptor(ds: &[BehaviorDescriptor]) -> BehaviorDescriptor {
    let x = mean(&ds.iter().map(|d| d.aggression).collect::<Vec<_>>());
    let y = mean(&ds.iter().map(|d| d.exploration).collect::<Vec<_>>());
    BehaviorDescriptor::new(x, y)
}
