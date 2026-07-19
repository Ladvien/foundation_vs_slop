//! Offline QD/RL tuning driver (feature `test-harness`).
//!
//! The offline half of the experience system: it runs the real simulation headlessly, many times, to
//! search the space of squad/swarm brains, worlds, levels, audio, and neural policies. Nothing here ships
//! in the game binary — the runtime only ever *reads* the archives this produces.
//!
//! MAP-Elites (Mouret & Clune, "Illuminating search spaces by mapping elites", arXiv:1504.04909) needs
//! thousands of evaluations, and `sim_harness` admits exactly one `App` per process (it holds a
//! process-wide lock and pins the global compute pool + rayon to one thread for determinism). So throughput
//! is `processes × ticks_per_second`, which `--islands N` (N independent processes) and `--jobs N` (a
//! worker pool for the co-evolution rollouts) both exploit.
//!
//! This is a `clap` CLI — run `train --help` (and `train <subcommand> --help`) for the full flag list.
//! A search prints a live progress bar with ETA + per-generation stats on a terminal, and falls back to
//! plain per-generation lines when piped. `--apply` bakes the result into the shipped defaults, and (for
//! prior-backed searches) the baseline prior is auto-refreshed when `config.ron` is newer than it.

use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};

use foundation_vs_slop::elite_overlay::{apply_dim, Dim};
use foundation_vs_slop::sim_harness::{
    build_headless_app, field_hash, liveness_violations, serial_guard, snapshot_hash, step, SimConfig,
};
use foundation_vs_slop::squad_ai::coevolve::{
    brains_of, mutate_squad_feasible, search, squad_archive_doc, swarm_archive_doc, sweep_prior,
    world_archive_doc, SearchConfig, SearchResult, SquadGenome, SwarmGenome, Templates, HELD_IN_SEEDS,
};
use foundation_vs_slop::squad_ai::evaluate::{rollout, rollout_with_belief};
use foundation_vs_slop::squad_ai::interest::Interest;
use foundation_vs_slop::squad_ai::poet::{poet_search, PoetConfig};
use foundation_vs_slop::squad_ai::surprise::{self, ModePrior};
use foundation_vs_slop::squad_ai::world_genome::{self, WorldGenome};
use foundation_vs_slop::squad_ai::audio_search::{self, AudioSearchConfig};
use foundation_vs_slop::squad_ai::behavior_search::{self, BehaviorSearchConfig};
use foundation_vs_slop::squad_ai::rl_search::{self, RlSearchConfig};
use foundation_vs_slop::squad_ai::level_eval;
use foundation_vs_slop::squad_ai::level_search::{self, LevelSearchConfig};

/// Where the frozen baseline expectation lives. Committed, and validated on load.
const PRIOR_PATH: &str = "assets/config/baseline_prior.ron";
/// Where the illuminated archives land, for a human to read before anything ships.
const SQUAD_ARCHIVE_PATH: &str = "assets/config/elites_squad.ron";
const SWARM_ARCHIVE_PATH: &str = "assets/config/elites_swarm.ron";
const WORLD_ARCHIVE_PATH: &str = "assets/config/elites_world.ron";
/// The illuminated *level* archive: evolved dungeon architecture + furniture amount + mushroom amount.
const LEVELS_ARCHIVE_PATH: &str = "assets/config/elites_levels.ron";
/// The illuminated *audio* archive: evolved acoustic-stimulus config (propagation + loudness + perception).
const AUDIO_ARCHIVE_PATH: &str = "assets/config/elites_audio.ron";
/// The illuminated *behaviour* archive: evolved per-agent behaviour subset (locomotion/steering/senses/
/// combat cadence/boids), overlaid onto the shipped `behavior:` base.
const BEHAVIOR_ARCHIVE_PATH: &str = "assets/config/elites_behavior.ron";
/// The illuminated *policy* archive: evolved `NeuralPolicy` weight vectors (the neuroevolution learner).
const RL_ARCHIVE_PATH: &str = "assets/config/elites_policy.ron";
/// The POET output: the surviving (environment, agent) niches — evolved worlds paired with the squad that
/// solves them, plus each pairing's fitness + human-interest score.
const POET_ARCHIVE_PATH: &str = "assets/config/elites_poet.ron";
/// Where `--islands N` writes each island's archive + log.
const ISLANDS_DIR: &str = "islands_out";

/// **The bake ledger** — append-only, tracked, one record per `train apply` / `train all --apply` phase.
///
/// Git already records *what* a bake changed: `config.ron` and `tests/replay.rs` are both tracked, so
/// `git log -p assets/config/config.ron` is the history of every baked value and `git log -p tests/replay.rs`
/// is the history of every golden. Two things git cannot tell you, which this file exists to record:
///
/// 1. **Which elite caused it.** The archives are gitignored (reproducible outputs, not source) and the next
///    run overwrites them, so the elite behind a golden move is gone by morning. [`BAKE_HISTORY_DIR`] keeps
///    the exact archive; this ledger names it.
/// 2. **Attribution inside one run.** `train all --repin-goldens` bakes several phases before you ever see a
///    diff, so git collapses them into one. One record per phase says which phase moved the golden, and how
///    far.
///
/// That is what makes `--repin-goldens` reviewable rather than "trust me": re-pinning surrenders *change
/// detection*, and this is the trail you review instead.
const BAKE_LEDGER: &str = "BAKES.md";
/// Snapshots of every archive that was ever baked, keyed by stamp + dim — so an old elite stays reviewable
/// (and re-bakeable) after the next run overwrites `elites_*.ron`. Gitignored: machine artifacts, sometimes
/// large; the ledger carries the reviewable summary.
const BAKE_HISTORY_DIR: &str = "assets/config/bake_history";

/// Rollouts consumed by one genome evaluation in the planned search, used to project the budget:
/// 2 rollouts (the learnability pair — a mode-transition model fitted on rollout A must predict
/// rollout B; Schmidhuber, "Driven by Compression Progress", arXiv:0812.4360) × 3 sampled opponents
/// (sampling across the opponent archive rather than only its incumbent avoids the coevolutionary
/// "mediocre stable states" of Ficici & Pollack; cf. Wang et al., POET, arXiv:1901.01753) × 3 held-in
/// dungeon seeds.
const ROLLOUTS_PER_GENOME: u32 = 2 * 3 * 3;

const CONFIG_PATH: &str = "assets/config/config.ron";
const REPLAY_PATH: &str = "tests/replay.rs";

// ── CLI definition (clap derive) ────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "train",
    about = "Offline QD/RL tuning driver for Foundation vs. Slop (test-harness build only).",
    long_about = "Runs the real simulation headlessly to search squad/swarm brains, worlds, levels, audio, \
                  and neural policies. Searches print a live progress bar + ETA; `--apply` bakes the winner \
                  into the shipped config; `--islands N` fans N independent search processes across cores."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Throughput + determinism probe: time one headless episode per seed.
    Bench(BenchArgs),
    /// One authored episode per seed: raw outcome + criterion (calibration diagnostic).
    Probe(ProbeArgs),
    /// Sweep the shipped brain -> baseline_prior.ron (run before the behavioural searches).
    Prior(ProbeArgs),
    /// Co-evolve squad × swarm × world; commit the squad + swarm archives.
    Evolve(SearchArgs),
    /// Co-evolve squad × swarm × world; commit all three (incl. the evolved worlds + mould).
    Evolve3(SearchArgs),
    /// Evolve dungeon architecture + furniture + mushrooms (static objective, GPU-free, no prior).
    Levels(SearchArgs),
    /// Evolve the acoustic-stimulus config (behavioural witnessed-surprise objective).
    Audio(SearchArgs),
    /// Evolve a curated behaviour subset (behavioural witnessed-surprise objective).
    Behavior(SearchArgs),
    /// Neuroevolve a NeuralPolicy's weights (behavioural witnessed-surprise objective).
    Rl(SearchArgs),
    /// POET open-ended world × squad co-evolution.
    Poet(SearchArgs),
    /// Train the WHOLE system end-to-end: prior → levels → audio → evolve3 → rl. The single command for
    /// "retrain everything" — defaults to MAX parallelism (auto-detect cores) and auto-applies + verifies at
    /// the end (pass `--no-apply` to skip baking).
    All(AllArgs),
    /// Permanently bake an evolved elite into the shipped defaults (config.ron + Default impls + goldens).
    Apply(ApplyArgs),
    /// Determinism stability guard: recompute the deterministic-core goldens and require agreement.
    Verify {
        #[arg(long, default_value_t = GOLDEN_STABILITY_REPS, value_parser = parse_pos_u32)]
        reps: u32,
    },
    /// Internal rollout-evaluation worker for `--jobs N` (length-prefixed RON IPC — not a human CLI).
    #[command(hide = true)]
    Worker,
}

#[derive(Args)]
struct BenchArgs {
    /// Ticks per episode (7200 = 120 s @ 60 Hz — the measured episode floor; see `SearchConfig`). Budget
    /// projections are only honest at the length the search will actually run.
    #[arg(long, default_value_t = 7200, value_parser = parse_pos_u32)]
    ticks: u32,
    /// Dungeon seeds (comma-separated; decimal or 0x-hex). Default: the held-in set.
    #[arg(long, value_delimiter = ',', value_parser = parse_seed)]
    seeds: Vec<u64>,
    /// Throughput lever: FixedUpdate sub-steps per render pass (does not change physics).
    #[arg(long, default_value_t = 1.0)]
    speed: f32,
}

#[derive(Args)]
struct ProbeArgs {
    /// Ticks per episode (7200 = 120 s @ 60 Hz — the measured episode floor; see `SearchConfig`).
    #[arg(long, default_value_t = 7200, value_parser = parse_pos_u32)]
    ticks: u32,
    /// Dungeon seeds (comma-separated; decimal or 0x-hex). Default: the **whole held-in set**, because a
    /// calibration read from a subset is how a retired seed once got mistaken for a held-in one.
    #[arg(long, value_delimiter = ',', value_parser = parse_seed)]
    seeds: Vec<u64>,
}

/// Shared flags for every search subcommand. The search-EFFORT defaults are practical settings (30 gens ×
/// 16 children) rather than `SearchConfig::default()`'s 8 × 4, so a bare `cargo train evolve3` runs a useful
/// search out of the box.
///
/// `ticks` is NOT an effort knob and is not traded off against them: it is the measured episode floor, and
/// it matches `SearchConfig::default()`. Below it the criterion is balanced on a knife's edge and the
/// archives thin out (see `SearchConfig`'s doc for the measurement).
#[derive(Args, Clone)]
struct SearchArgs {
    /// Generations (POET: outer iterations).
    #[arg(long, default_value_t = 30, value_parser = parse_pos_u32)]
    generations: u32,
    /// Convergence early-stop: quit a search once its QD-score has not improved for this many consecutive
    /// generations (Mouret & Clune 2015 archive-property termination). `0` disables it (run every
    /// generation). Cuts wall-clock on phases that converge early; the result stays bit-reproducible.
    #[arg(long, default_value_t = 8)]
    patience: u32,
    /// Children proposed per generation (per side, for co-evolution).
    #[arg(long, default_value_t = 16, value_parser = parse_pos_u32)]
    batch: u32,
    /// Ticks per rollout episode (7200 = 120 s @ 60 Hz — the measured floor; see `SearchConfig`).
    #[arg(long, default_value_t = 7200, value_parser = parse_pos_u32)]
    ticks: u32,
    /// MAP-Elites archive resolution (bins per descriptor axis).
    #[arg(long, default_value_t = 8, value_parser = parse_pos_usize)]
    res: usize,
    /// Search RNG seed (distinct search trajectories; `--islands` derives its own per island).
    #[arg(long, default_value_t = 0xC0FFEE, value_parser = parse_seed)]
    seed: u64,
    /// Held-in dungeon seeds the objective is evaluated on (comma-separated; decimal or 0x-hex). Needs >= 2.
    /// Default: the held-in set (`coevolve::HELD_IN_SEEDS`).
    #[arg(long, value_delimiter = ',', value_parser = parse_seed)]
    seeds: Vec<u64>,
    /// Rollout worker processes for co-evolution (`evolve`/`evolve3`); capped useful at OPPONENTS (3).
    #[arg(long, default_value_t = 1, value_parser = parse_pos_usize)]
    jobs: usize,
    /// Use the CMA-ME adaptive emitter (only honoured by `rl`).
    #[arg(long)]
    cma: bool,
    /// Override the output archive path (single-output searches only).
    #[arg(long)]
    out: Option<PathBuf>,
    /// After the search, permanently bake the winner into the shipped config + regenerate the prior.
    #[arg(long)]
    apply: bool,
    /// Fan N independent search processes across cores, then pick the best-fitness elite (single-output
    /// searches only; `evolve`/`evolve3` write fixed multi-archive paths and must run alone).
    #[arg(long, default_value_t = 1, value_parser = parse_pos_usize)]
    islands: usize,
    /// With `--apply`: accept that the bake MOVES the deterministic goldens, and re-pin `tests/replay.rs`.
    ///
    /// Without this, a bake whose elite changes the shipped sim aborts and reports the drift — see
    /// `apply_archive`. That default is right for a bake you did not ask for; it is wrong for a search you
    /// deliberately ran to change the game, which is why `--apply` without this is refused UP FRONT rather
    /// than hours later.
    ///
    /// What you give up by passing it is **change detection**, not determinism: `recompute_goldens_stable`
    /// still requires every repeated measurement to agree, so a nondeterministic core reds here regardless.
    /// What you must NOT conclude afterwards is that a green `cargo test --features test-harness` validates
    /// the training — it passes by construction once the goldens are re-pinned. Review `git diff` and the
    /// elite archives; the test suite only tells you the sim is reproducible.
    #[arg(long)]
    repin_goldens: bool,
    /// Force plain per-generation lines instead of the live progress bar (for logs / CI).
    #[arg(long)]
    no_progress: bool,
    /// Internal: run as an island child — emit machine-readable PROGRESS/RESULT on stdout for the parent.
    #[arg(long, hide = true)]
    island_child: bool,
}

/// Flags for the whole-system `all` pipeline. One shared set, threaded sensibly into each phase (`--jobs`
/// drives the co-evolution worker fan-out; `--islands` drives the single-output searches; `rl` gets CMA-ME).
#[derive(Args)]
struct AllArgs {
    /// Generations per phase.
    #[arg(long, default_value_t = 30, value_parser = parse_pos_u32)]
    generations: u32,
    /// Convergence early-stop per phase: quit once the QD-score has not improved for this many consecutive
    /// generations (`0` disables). Applied to every phase; the big automatic wall-clock win for `train all`.
    #[arg(long, default_value_t = 8)]
    patience: u32,
    /// Children proposed per generation (per side, for co-evolution).
    #[arg(long, default_value_t = 16, value_parser = parse_pos_u32)]
    batch: u32,
    /// Ticks per rollout episode (7200 = 120 s @ 60 Hz — the measured floor; see `SearchConfig`).
    #[arg(long, default_value_t = 7200, value_parser = parse_pos_u32)]
    ticks: u32,
    /// MAP-Elites archive resolution (bins per descriptor axis).
    #[arg(long, default_value_t = 8, value_parser = parse_pos_usize)]
    res: usize,
    /// Search RNG seed (each phase derives its own trajectory from it).
    #[arg(long, default_value_t = 0xC0FFEE, value_parser = parse_seed)]
    seed: u64,
    /// Held-in dungeon seeds the objective is evaluated on (comma-separated; decimal or 0x-hex). Needs >= 2.
    #[arg(long, value_delimiter = ',', value_parser = parse_seed)]
    seeds: Vec<u64>,
    /// Rollout worker processes for the `evolve3` co-evolution phase (the batch emitter scales to
    /// `batch × OPPONENTS`). `0` (the default) = auto: use every logical core.
    #[arg(long, default_value_t = 0)]
    jobs: usize,
    /// Island fan-out for the single-output phases (`levels`/`audio`/`rl`), picking the best-fitness elite.
    /// `0` (the default) = auto: use every logical core.
    #[arg(long, default_value_t = 0)]
    islands: usize,
    /// Do NOT bake at the end. By default `all` bakes each config-backed phase's winner into the shipped
    /// config as it finishes and runs the determinism verify — pass this to leave `config.ron`/goldens alone.
    #[arg(long)]
    no_apply: bool,
    /// Required to bake: accept that the run MOVES the deterministic goldens, and re-pin them per phase.
    ///
    /// A search you ran on purpose is a change to the shipped sim, so its bake **will** move the goldens; the
    /// per-bake default is to abort on that drift and let a human decide (`apply_archive` step 4). For an
    /// unattended pipeline that would mean burning hours and then dying at the first phase, so `all` refuses
    /// AT STARTUP unless you either accept the movement here or pass `--no-apply`. See `SearchArgs`'s field
    /// for what accepting actually costs you (change detection — not determinism).
    #[arg(long)]
    repin_goldens: bool,
    /// Force plain per-generation lines instead of the live progress bar (for logs / CI).
    #[arg(long)]
    no_progress: bool,
}

#[derive(Args)]
struct ApplyArgs {
    /// Which config slice to bake: behavior | world | audio | levels.
    dim: String,
    /// The evolved elite archive to bake from.
    archive: PathBuf,
    /// Pick a specific archive cell `row,col` (default: the archive's best elite).
    #[arg(long)]
    cell: Option<String>,
    /// Accept a golden that MOVED and re-pin `tests/replay.rs` to the new value.
    ///
    /// Without this, a bake whose recomputed goldens differ from the committed ones ABORTS and reports the
    /// drift. That is deliberate: a moved golden means the bake changed the shipped sim, which is exactly the
    /// thing a human is supposed to look at before it lands. TESTING.md: "Changing one is a deliberate,
    /// human-reviewed act — never auto-approve a diff." Pass this only when you have read the diff and the
    /// movement is the intended effect of the elite you are baking.
    #[arg(long)]
    repin_goldens: bool,
}

/// The default log filter: silence the headless Bevy asset-not-found spam at the source (so `cargo train …`
/// needs no `RUST_LOG` export), keeping warnings from the sim itself.
const DEFAULT_LOG_FILTER: &str =
    "warn,bevy_asset=off,bevy_render=off,bevy_gltf=off,bevy_gizmos=off,wgpu=off,naga=off";

/// Install the process-global tracing subscriber ONCE, honouring `RUST_LOG` when the caller set one.
///
/// The harness disables Bevy's `LogPlugin` (see `sim_harness::build_headless_app_unfinished`): the subscriber
/// is process-global, but a search builds one `App` per rollout — thousands of them — and each would fight to
/// install it, so every rollout after the first logged
/// `ERROR bevy_log: Could not set global logger …`. The process owns the logger; the `App`s don't. Installing
/// here keeps the sim's warnings visible exactly once, with none of the per-rollout spam.
fn init_logging() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| DEFAULT_LOG_FILTER.to_string());
    // A failure here means something already installed a subscriber — nothing to do, and not worth aborting
    // a multi-hour search over.
    let _ = bevy::log::tracing_subscriber::fmt()
        .with_env_filter(bevy::log::tracing_subscriber::EnvFilter::new(filter))
        .try_init();
}

fn main() {
    init_logging();

    let cli = Cli::parse();
    let result = match cli.command {
        Command::Bench(a) => {
            bench(a.ticks, &seeds_or(&a.seeds, &HELD_IN_SEEDS), a.speed);
            Ok(())
        }
        Command::Probe(a) => probe(a.ticks, &seeds_or(&a.seeds, &HELD_IN_SEEDS)),
        Command::Prior(a) => prior(a.ticks, &seeds_or(&a.seeds, &HELD_IN_SEEDS)),
        Command::Evolve(a) => run_search(SearchKind::Evolve, a),
        Command::Evolve3(a) => run_search(SearchKind::Evolve3, a),
        Command::Levels(a) => run_search(SearchKind::Levels, a),
        Command::Audio(a) => run_search(SearchKind::Audio, a),
        Command::Behavior(a) => run_search(SearchKind::Behavior, a),
        Command::Rl(a) => run_search(SearchKind::Rl, a),
        Command::Poet(a) => run_search(SearchKind::Poet, a),
        Command::All(a) => run_all(a),
        Command::Apply(a) => Dim::parse(&a.dim)
            .and_then(|dim| apply_archive(dim, &a.archive, a.cell.as_deref(), a.repin_goldens)),
        Command::Verify { reps } => verify(reps),
        Command::Worker => foundation_vs_slop::squad_ai::parallel::worker_main(),
    };
    if let Err(e) = result {
        eprintln!("train: {e}");
        std::process::exit(1);
    }
}

/// The seeds the caller gave, or `fallback` if none.
fn seeds_or(seeds: &[u64], fallback: &[u64]) -> Vec<u64> {
    if seeds.is_empty() { fallback.to_vec() } else { seeds.to_vec() }
}

// ── Search kinds + orchestration ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchKind {
    Evolve,
    Evolve3,
    Levels,
    Audio,
    Behavior,
    Rl,
    Poet,
}

impl SearchKind {
    /// The clap subcommand name (also the island archive/log prefix).
    fn cli_name(self) -> &'static str {
        match self {
            SearchKind::Evolve => "evolve",
            SearchKind::Evolve3 => "evolve3",
            SearchKind::Levels => "levels",
            SearchKind::Audio => "audio",
            SearchKind::Behavior => "behavior",
            SearchKind::Rl => "rl",
            SearchKind::Poet => "poet",
        }
    }
    /// The canonical single-output archive path for this kind — the stable location a human (or a
    /// `FVS_*_ELITE` overlay) reads. `--islands` fans winners into `islands_out/` under ephemeral names that
    /// the next run clears, so the island parent copies its winner HERE; a single search writes here directly.
    /// `Evolve`/`Evolve3` write fixed multi-archive paths and have no single output.
    fn archive_path(self) -> Option<&'static str> {
        match self {
            SearchKind::Levels => Some(LEVELS_ARCHIVE_PATH),
            SearchKind::Audio => Some(AUDIO_ARCHIVE_PATH),
            SearchKind::Behavior => Some(BEHAVIOR_ARCHIVE_PATH),
            SearchKind::Rl => Some(RL_ARCHIVE_PATH),
            SearchKind::Poet => Some(POET_ARCHIVE_PATH),
            SearchKind::Evolve | SearchKind::Evolve3 => None,
        }
    }
    /// Progress-bar label.
    fn label(self) -> &'static str {
        match self {
            SearchKind::Rl => "policy",
            SearchKind::Behavior => "behaviour",
            other => other.cli_name(),
        }
    }
    /// Behavioural searches need the frozen prior; the static `levels` objective does not.
    fn needs_prior(self) -> bool {
        !matches!(self, SearchKind::Levels)
    }
    /// Only single-output searches can be fanned into islands; co-evolution writes fixed multi-archive paths.
    fn supports_islands(self) -> bool {
        !matches!(self, SearchKind::Evolve | SearchKind::Evolve3)
    }
    /// Which config slice a `--apply` bakes, or an error explaining why this kind can't be baked.
    fn apply_dim(self) -> Result<Dim, String> {
        match self {
            SearchKind::Evolve3 => Ok(Dim::World),
            SearchKind::Levels => Ok(Dim::Levels),
            SearchKind::Audio => Ok(Dim::Audio),
            SearchKind::Behavior => Ok(Dim::Behavior),
            SearchKind::Evolve => {
                Err("evolve does not commit a world archive to bake — use `evolve3 --apply`".into())
            }
            SearchKind::Rl => Err(
                "rl/policy has no config slice to bake (a NeuralPolicy is opaque weights) — run it with \
                 FVS_POLICY_ELITE=<archive> instead of --apply"
                    .into(),
            ),
            SearchKind::Poet => {
                Err("poet has no single config slice to bake — inspect the niches archive by hand".into())
            }
        }
    }
}

/// What a completed single search produced: the best fitness (for island ranking) + its archive path.
struct SearchOutcome {
    best: f32,
    out: PathBuf,
}

/// Entry point for every search subcommand: validate, fan into islands or run one search, then optionally
/// bake the winner.
/// Train the whole system end-to-end, in one command: prior → levels → audio → evolve3 → rl. The single entry
/// point for "retrain everything" after a mechanic change (e.g. the Almond Water belief/inversion redesign,
/// which widened the policy observation and grew the world genome). One shared flag set threads into each
/// phase: `--jobs` fans out the `evolve3` co-evolution workers (it runs alone, islands=1); the single-output
/// phases (`levels`/`audio`/`rl`) use `--islands`; `rl` gets the CMA-ME emitter. **Defaults to max: `--jobs`
/// and `--islands` auto-resolve to every logical core, and it auto-applies** — each config-backed phase bakes
/// its winner into the shipped config as it finishes and the pipeline verifies determinism at the end (pass
/// `--no-apply` to skip baking). The `rl` neural policy has no config slice, so it is never baked — its elite
/// lands in its archive, loaded via the `FVS_POLICY_ELITE` overlay.
fn run_all(a: AllArgs) -> Result<(), String> {
    let seeds = seeds_or(&a.seeds, &HELD_IN_SEEDS);
    // Auto (0) → every logical core. Phases run sequentially, so each saturates the box in turn: `evolve3`
    // fans `jobs` batch-emitter workers, the single-output phases fan `islands` independent searches.
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let jobs = if a.jobs == 0 { cores } else { a.jobs };
    let islands = if a.islands == 0 { cores } else { a.islands };
    let apply = !a.no_apply;

    // FAIL FAST. A phase's bake aborts if its elite moved the goldens, and a real elite from a real search
    // moves them — that is what a successful search MEANS. Discovering it after the first multi-hour phase,
    // with three phases left unrun, is the worst possible time. Decide it here, in the first second.
    if apply && !a.repin_goldens {
        return Err(format!(
            "`train all` bakes each phase, and a bake whose elite changes the shipped sim MOVES the \
             deterministic goldens — which a real search's elite will, because that is what a successful \
             search MEANS. Baking with the goldens pinned aborts mid-pipeline, hours in, with the remaining \
             phases unrun. Choose now rather than then:\n\
             \n\
             --repin-goldens : bake each phase and re-pin as you go, accepting that the goldens move.\n\
             Determinism is still proven — `recompute_goldens_stable` requires every repeated measurement to \
             agree, so a nondeterministic core still reds. What you give up is CHANGE DETECTION, and note \
             that a green `cargo test --features test-harness` afterwards then proves NOTHING about the \
             training: the goldens were just set to whatever the code emits, so the suite passes by \
             construction. Review `git diff {CONFIG_PATH}` and {BAKE_LEDGER} instead — that ledger exists \
             precisely because this flag surrenders the automatic check.\n\
             \n\
             --no-apply : search every phase and bake nothing. The archives are written for you to read and \
             bake by hand. NOTE: phases then stop composing — each search runs against the same base config, \
             so baking all of them afterwards ships a combination no search ever evaluated together.\n"
        ));
    }

    println!(
        "train all — cores={cores}, jobs={jobs}, islands={islands}, batch={}, generations={}, apply={apply}, \
         repin_goldens={}",
        a.batch, a.generations, a.repin_goldens
    );

    // A per-phase SearchArgs from the shared flags.
    let phase = |jobs: usize, islands: usize, cma: bool, apply: bool| SearchArgs {
        generations: a.generations,
        patience: a.patience,
        batch: a.batch,
        ticks: a.ticks,
        res: a.res,
        seed: a.seed,
        seeds: seeds.clone(),
        jobs,
        cma,
        out: None,
        apply,
        islands,
        repin_goldens: a.repin_goldens,
        no_progress: a.no_progress,
        island_child: false,
    };
    let banner = |i: u32, name: &str| println!("\n═══ train all — phase {i}/6: {name} ═══");

    banner(1, "prior (baseline the shipped brain)");
    prior(a.ticks, &seeds)?;
    // `levels` is SEARCH-ONLY here, like `rl` — it is never baked into `config.ron` by the pipeline. Its
    // elites are STRUCTURAL (`level_genome::decode` adds/drops room types), so a bake would change the
    // `dungeon:` block's shape, which `splice_block` refuses because rewriting it would strip the authored
    // rationale (a `--dim levels` bake stripped ~279 comment lines on 2026-07-16 — the incident that built
    // the refusal). Levels ships via `FVS_LEVELS_ELITE`, its native mechanism, exactly as the policy ships
    // via `FVS_POLICY_ELITE`. This also keeps the held-in worlds a STABLE reference (authored architecture +
    // fixed seeds) for the later phases, rather than shifting the maps under the co-evolution mid-run.
    // (`train apply levels <archive>` by hand still works for a value-only elite that keeps every room type.)
    banner(2, "levels (dungeon architecture + furniture + mushrooms)");
    run_search(SearchKind::Levels, phase(1, islands, false, false))?; // search-only: structural → overlay, not baked
    banner(3, "audio (acoustic stimulus)");
    run_search(SearchKind::Audio, phase(1, islands, false, apply))?;
    // `behavior` (89 knobs of `BehaviorTuning`) was absent from this pipeline entirely, so "retrain
    // everything" covered 3 of the 4 bakeable dims (`elite_overlay::Dim`) and the fourth could only be
    // reached by running `train behavior` by hand. It sits BEFORE `evolve3` deliberately: it tunes the base
    // the squad brains run on, and the co-evolution should radiate from the tuned base rather than have it
    // move underneath the archives afterwards. Baking it re-baselines the prior (see `bake_winner`), which is
    // what makes ordering safe here at all.
    banner(4, "behavior (squad brain tuning — speeds, bands, thresholds)");
    run_search(SearchKind::Behavior, phase(1, islands, false, apply))?;
    banner(5, "evolve3 (squad × swarm × world, incl. the evolvable Almond Water dynamics)");
    run_search(SearchKind::Evolve3, phase(jobs, 1, false, apply))?;
    banner(6, "rl (neuroevolve the squad policy — now perceives water / belief / anosmia)");
    run_search(SearchKind::Rl, phase(1, islands, true, false))?; // policy has no config slice → never baked
    println!(
        "  → trained policy is at {}. Use it in-game with FVS_POLICY_ELITE={}",
        RL_ARCHIVE_PATH, RL_ARCHIVE_PATH
    );

    if apply {
        println!("\n═══ train all — verify determinism after auto-apply ═══");
        verify(GOLDEN_STABILITY_REPS)?;
    }
    // The two overlay-shipped dims: config.ron holds the baked value dims (audio/behavior/world); these two
    // ride alongside as archives, applied at launch. Print both together so nothing trained is left on the
    // floor for want of an env var.
    println!("\n✓ train all: full pipeline complete");
    if apply {
        println!("\nShipped in config.ron: audio + behavior + world (baked).");
    } else {
        // --no-apply bakes NOTHING (see run_all's mode note above): every phase searched against the same
        // base and wrote its archive for hand-baking. Printing "baked" here — and pointing at a
        // config.ron/BAKES.md diff that shows nothing changed — would tell an operator reading only the log
        // tail that the trained values ship. They do not, and the archives would be silently discarded.
        println!("\nNothing baked (--no-apply): audio + behavior + world were searched but NOT written to");
        println!("config.ron. Each phase wrote its archive for you to review and bake by hand with");
        println!("`train apply <dim> <archive>`.");
    }
    println!("Ship alongside, via env (structural / opaque — not config-bakeable):");
    println!("  FVS_LEVELS_ELITE={LEVELS_ARCHIVE_PATH}   # evolved dungeon architecture + furniture + mushrooms");
    println!("  FVS_POLICY_ELITE={RL_ARCHIVE_PATH}   # the neuroevolved squad policy");
    if apply {
        println!("Review first: read the archives, and `git diff {CONFIG_PATH}` + {BAKE_LEDGER} for the baked dims.");
    } else {
        println!("Review first: read each archive before deciding whether to bake it.");
    }
    Ok(())
}

fn run_search(kind: SearchKind, mut a: SearchArgs) -> Result<(), String> {
    if a.seeds.is_empty() {
        a.seeds = HELD_IN_SEEDS.to_vec();
    }
    if a.seeds.len() < 2 {
        return Err(
            "a search needs >= 2 dungeon seeds: the two rollouts of a candidate must run on DIFFERENT \
             worlds, or learnability measures a memorised map rather than a behaviour"
                .into(),
        );
    }
    // Fail fast: if `--apply` was asked for a kind that can't be baked, say so before a multi-hour search.
    if a.apply {
        kind.apply_dim()?;
    }

    if a.islands > 1 {
        if !kind.supports_islands() {
            return Err(format!(
                "`{}` does not support --islands (it writes fixed multi-archive paths) — run it alone \
                 (use --jobs N for its worker pool instead)",
                kind.cli_name()
            ));
        }
        return run_islands(kind, a);
    }

    // Single search. Island children never refresh the prior (the parent did it once; N children racing to
    // rewrite baseline_prior.ron would corrupt it).
    if kind.needs_prior() && !a.island_child {
        ensure_prior_fresh(a.ticks, &a.seeds)?;
    }

    let progress = Progress::new(a.generations, a.no_progress, a.island_child, kind.label());
    let outcome = run_single(kind, &a, &progress)?;

    if a.island_child {
        // The parent parses this line to rank islands.
        println!("RESULT best={} out={}", outcome.best, outcome.out.display());
    }

    if a.apply {
        bake_winner(kind, &a, &outcome.out)?;
    }
    Ok(())
}

/// Bake a finished search's winner, then re-baseline the prior against the tuning that just landed.
///
/// **The one bake path for every search.** `run_search`'s single-process arm and `run_islands`' fan-out arm
/// both call it, so `--apply` cannot come to mean two different things depending on `--islands` — these were
/// two copies of the same block, each hardcoding `repin_goldens: false`, and a fix applied to one would have
/// silently missed the other.
///
/// The prior re-sweep is not optional: it models the game *as shipped*, and the bake just changed what ships.
/// (`ensure_prior_fresh` also catches this on the next search, by mtime.)
fn bake_winner(kind: SearchKind, a: &SearchArgs, out: &Path) -> Result<(), String> {
    let dim = kind.apply_dim()?;
    println!();
    println!(">> --apply: baking the {} elite {} into the shipped config…", kind.cli_name(), out.display());
    apply_archive(dim, out, None, a.repin_goldens)?;
    println!(">> regenerating the baseline prior for the new tuning…");
    prior(a.ticks, &a.seeds)?;
    Ok(())
}

fn run_single(kind: SearchKind, a: &SearchArgs, p: &Progress) -> Result<SearchOutcome, String> {
    match kind {
        SearchKind::Evolve => evolve_run(a, p),
        SearchKind::Evolve3 => evolve3_run(a, p),
        SearchKind::Levels => levels_run(a, p),
        SearchKind::Audio => audio_run(a, p),
        SearchKind::Behavior => behavior_run(a, p),
        SearchKind::Rl => rl_run(a, p),
        SearchKind::Poet => poet_run(a, p),
    }
}

/// Regenerate the baseline prior when it is missing or older than `config.ron` (the shipped game it models
/// has since changed, so a stale prior would measure "surprise" against a game that no longer exists).
fn ensure_prior_fresh(ticks: u32, seeds: &[u64]) -> Result<(), String> {
    let stale = match std::fs::metadata(PRIOR_PATH) {
        Err(_) => true, // missing
        Ok(pm) => match (pm.modified(), std::fs::metadata(CONFIG_PATH).and_then(|m| m.modified())) {
            (Ok(prior_t), Ok(cfg_t)) => cfg_t > prior_t,
            _ => false, // can't compare mtimes — leave the existing prior alone rather than churn
        },
    };
    if stale {
        eprintln!(">> baseline_prior.ron is missing or older than config.ron — regenerating…");
        prior(ticks, seeds)?;
    }
    Ok(())
}

// ── Live progress ────────────────────────────────────────────────────────────────────────────────────

/// Per-generation progress reporting. `Bar` on a terminal, `Plain` per-generation lines when piped or
/// `--no-progress`, and `Child` machine-readable lines an island parent parses. Updated ONLY from the
/// search's side-effect-only `report` closure, so it never perturbs search determinism.
enum Progress {
    Bar(indicatif::ProgressBar),
    Plain,
    Child { generations: u32 },
}

impl Progress {
    fn new(generations: u32, no_progress: bool, island_child: bool, label: &str) -> Progress {
        if island_child {
            return Progress::Child { generations };
        }
        if no_progress || !std::io::stderr().is_terminal() {
            return Progress::Plain;
        }
        let bar = indicatif::ProgressBar::new(generations as u64);
        let style = indicatif::ProgressStyle::with_template(
            "{prefix:>9} {bar:28.cyan/blue} gen {pos}/{len} [{elapsed_precise}<{eta_precise}] {msg}",
        )
        .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar());
        bar.set_style(style);
        bar.set_prefix(label.to_string());
        bar.enable_steady_tick(Duration::from_millis(120));
        Progress::Bar(bar)
    }

    fn tick(&self, generation: u32, best: f32, msg: &str) {
        match self {
            Progress::Bar(b) => {
                b.set_position(generation as u64);
                b.set_message(msg.to_string());
            }
            Progress::Plain => eprintln!("  gen {generation:>3}: {msg}"),
            Progress::Child { generations } => {
                println!("PROGRESS gen={generation}/{generations} best={best}")
            }
        }
    }

    fn finish(&self, msg: &str) {
        match self {
            Progress::Bar(b) => b.finish_with_message(msg.to_string()),
            Progress::Plain => eprintln!("  {msg}"),
            Progress::Child { .. } => {}
        }
    }
}

// ── Island model: N independent search processes, best elite wins ──────────────────────────────────────

/// Fan `a.islands` independent search processes across cores (each its own process → its own pinned
/// single-thread pool, so determinism holds), drive one progress bar per island, then pick the
/// highest-best-fitness elite across their archives.
fn run_islands(kind: SearchKind, a: SearchArgs) -> Result<(), String> {
    use std::process::{Command as PCommand, Stdio};

    let n = a.islands;
    // The parent refreshes the prior ONCE, before spawning; the children skip it.
    if kind.needs_prior() {
        ensure_prior_fresh(a.ticks, &a.seeds)?;
    }
    std::fs::create_dir_all(ISLANDS_DIR).map_err(|e| format!("{ISLANDS_DIR}: create: {e}"))?;

    // Clear this dim's stale island outputs, or an all-dead run could "pick a winner" from leftovers.
    let name = kind.cli_name();
    if let Ok(rd) = std::fs::read_dir(ISLANDS_DIR) {
        for entry in rd.flatten() {
            let f = entry.file_name().to_string_lossy().into_owned();
            let is_arch = f.starts_with(&format!("elites_{name}_")) && f.ends_with(".ron");
            let is_log = f.starts_with(&format!("{name}_")) && f.ends_with(".log");
            if is_arch || is_log {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let seeds_csv = a.seeds.iter().map(|s| format!("0x{s:X}")).collect::<Vec<_>>().join(",");
    let mp = indicatif::MultiProgress::new();
    let show_bars = !a.no_progress && std::io::stderr().is_terminal();
    eprintln!(">> launching {n} '{name}' islands → {ISLANDS_DIR}/ (each a separate process)");

    struct Island {
        idx: usize,
        child: std::process::Child,
        out_reader: std::thread::JoinHandle<Option<f32>>,
        err_reader: std::thread::JoinHandle<String>,
        out_path: String,
    }
    let mut islands: Vec<Island> = Vec::with_capacity(n);

    for i in 1..=n {
        // Distinct per-island search seed (Knuth multiplicative hash), kept in u32 range.
        let seed = (i as u64).wrapping_mul(2654435761) & 0x7FFF_FFFF;
        let out_path = format!("{ISLANDS_DIR}/elites_{name}_{i}.ron");
        let log_path = format!("{ISLANDS_DIR}/{name}_{i}.log");

        let mut cmd = PCommand::new(&exe);
        cmd.arg(name)
            .args(["--generations", &a.generations.to_string()])
            .args(["--batch", &a.batch.to_string()])
            .args(["--ticks", &a.ticks.to_string()])
            .args(["--res", &a.res.to_string()])
            .args(["--seeds", &seeds_csv])
            .args(["--jobs", &a.jobs.to_string()])
            .args(["--seed", &format!("0x{seed:X}")])
            .args(["--out", &out_path])
            .args(["--islands", "1", "--island-child"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if a.cma {
            cmd.arg("--cma");
        }
        // Die with the driver: without this, a Ctrl+C/kill on the parent orphaned these island processes
        // (they kept searching at ~75% CPU each and pegged the box). See `parallel::set_pdeathsig`.
        foundation_vs_slop::squad_ai::parallel::set_pdeathsig(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("spawn island {i}: {e}"))?;
        let stdout = child.stdout.take().ok_or("island child stdout unavailable")?;
        let stderr = child.stderr.take().ok_or("island child stderr unavailable")?;

        // One bar per island, updated from the child's PROGRESS lines.
        let bar = mp.add(indicatif::ProgressBar::new(a.generations as u64));
        if show_bars {
            let style = indicatif::ProgressStyle::with_template(
                "{prefix:>10} {bar:24.cyan/blue} gen {pos}/{len} [{elapsed_precise}] {msg}",
            )
            .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar());
            bar.set_style(style);
            bar.set_prefix(format!("island {i}"));
            bar.enable_steady_tick(Duration::from_millis(120));
        } else {
            bar.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }

        // stdout reader: drive the bar, capture the RESULT best, tee everything to the island log.
        let out_reader = std::thread::spawn(move || {
            let mut best: Option<f32> = None;
            let reader = std::io::BufReader::new(stdout);
            let mut log = std::fs::File::create(&log_path).ok();
            for line in reader.lines().map_while(Result::ok) {
                if let Some(rest) = line.strip_prefix("PROGRESS ") {
                    let mut g = 0u64;
                    let mut b = 0.0f32;
                    for tok in rest.split_whitespace() {
                        if let Some(v) = tok.strip_prefix("gen=") {
                            if let Some((cur, _)) = v.split_once('/') {
                                g = cur.parse().unwrap_or(0);
                            }
                        } else if let Some(v) = tok.strip_prefix("best=") {
                            b = v.parse().unwrap_or(0.0);
                        }
                    }
                    bar.set_position(g);
                    bar.set_message(format!("best {b:.3}"));
                } else if let Some(rest) = line.strip_prefix("RESULT ") {
                    for tok in rest.split_whitespace() {
                        if let Some(v) = tok.strip_prefix("best=") {
                            best = v.parse().ok();
                        }
                    }
                }
                if let Some(f) = log.as_mut() {
                    use std::io::Write;
                    let _ = writeln!(f, "{line}");
                }
            }
            bar.finish();
            best
        });
        // stderr reader: fully drain (avoid pipe-fill deadlock) and keep it for error reporting.
        let err_reader = std::thread::spawn(move || {
            use std::io::Read;
            let mut s = String::new();
            let _ = std::io::BufReader::new(stderr).read_to_string(&mut s);
            s
        });

        islands.push(Island { idx: i, child, out_reader, err_reader, out_path });
    }

    // Wait for all, collect (best, ok, stderr).
    struct Done {
        best: Option<f32>,
        ok: bool,
        out_path: String,
        stderr: String,
    }
    let mut done: Vec<Done> = Vec::with_capacity(n);
    for mut isl in islands {
        let status = isl.child.wait().map_err(|e| format!("island {} wait: {e}", isl.idx))?;
        let best = isl.out_reader.join().unwrap_or(None);
        let stderr = isl.err_reader.join().unwrap_or_default();
        done.push(Done { best, ok: status.success(), out_path: isl.out_path, stderr });
    }

    let alive: Vec<&Done> = done.iter().filter(|d| d.ok && d.best.is_some()).collect();
    if alive.is_empty() {
        let first_err = done
            .iter()
            .find(|d| !d.stderr.trim().is_empty())
            .map(|d| d.stderr.trim())
            .unwrap_or("(no stderr captured — see the island logs)");
        return Err(format!(
            "all {n} islands failed — nothing to pick from. first error:\n{}\n(full logs: {ISLANDS_DIR}/{name}_*.log)",
            first_err.lines().take(6).collect::<Vec<_>>().join("\n")
        ));
    }

    let Some(winner) = alive
        .iter()
        .max_by(|x, y| x.best.unwrap_or(f32::MIN).total_cmp(&y.best.unwrap_or(f32::MIN)))
    else {
        return Err("no island winner".into());
    };
    let win_out = winner.out_path.clone();
    let win_best = winner.best.unwrap_or(0.0);
    let failed = n - alive.len();
    println!();
    println!(
        ">> WINNER: {win_out}  (best fitness {win_best:.4}; {} of {n} islands produced elites{})",
        alive.len(),
        if failed > 0 { format!(", {failed} failed") } else { String::new() }
    );

    // Land the winner at the stable canonical path. `islands_out/` is gitignored and cleared at the start of
    // the next island run, so a winner left there is gone by the next phase — and the `FVS_*_ELITE` overlay
    // hints (and any hand bake) would point at a stale file. Copy first, then bake/hint from the canonical
    // location, so "the winner is at elites_<dim>.ron" is always true.
    let canonical = match kind.archive_path() {
        Some(p) => {
            std::fs::copy(&win_out, p).map_err(|e| format!("copy winner {win_out} -> {p}: {e}"))?;
            println!("   winner copied to {p}");
            p.to_string()
        }
        None => win_out.clone(),
    };

    if a.apply {
        bake_winner(kind, &a, Path::new(&canonical))?;
    } else {
        match kind {
            // Levels elites are often structural (room drops), which `train apply` refuses — so lead with the
            // overlay, its robust path, and note the bake works only for a value-only elite.
            SearchKind::Levels => println!(
                "   ship it:  FVS_LEVELS_ELITE={canonical} (overlay — handles any elite), \
                 or `train apply levels {canonical}` for a value-only elite that keeps every room type"
            ),
            SearchKind::Rl => println!("   ship it:  FVS_POLICY_ELITE={canonical}"),
            _ if kind.apply_dim().is_ok() => println!(
                "   ship it:  train apply {} {canonical}   (or re-run with --apply)",
                kind.cli_name()
            ),
            _ => {}
        }
    }
    Ok(())
}

/// `YYYY-MM-DDTHH:MM:SSZ` from the system clock. Dependency-free — there is no date crate in this tree, and
/// one stamp is not worth one. Civil-from-days is Howard Hinnant's algorithm ("chrono-Compatible Low-Level
/// Date Algorithms"). A clock before 1970 is an `Err`, not a fudged zero: the ledger's whole value is that
/// its record is trustworthy.
fn utc_stamp() -> Result<String, String> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system clock is before the unix epoch — cannot stamp the bake ledger: {e}"))?
        .as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    Ok(format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z"))
}

/// Snapshot the baked archive and append one record to [`BAKE_LEDGER`]. Returns the snapshot's path.
///
/// Called only on a bake that actually landed — a run that aborted on golden drift changed nothing worth
/// recording, and a ledger that logs non-events is one nobody reads.
fn record_bake(
    dim: Dim,
    archive: &Path,
    desc: &str,
    before: (u64, u64),
    after: (u64, u64),
    repinned: bool,
    touched: &[String],
) -> Result<String, String> {
    use std::fmt::Write as _;

    let stamp = utc_stamp()?;
    std::fs::create_dir_all(BAKE_HISTORY_DIR)
        .map_err(|e| format!("{BAKE_HISTORY_DIR}: create: {e}"))?;
    let kept = format!("{BAKE_HISTORY_DIR}/{}-{}.ron", stamp.replace(':', "-"), dim_cli_name(dim));
    std::fs::copy(archive, &kept)
        .map_err(|e| format!("{kept}: snapshot the baked archive: {e}"))?;

    let mut rec = String::new();
    if !Path::new(BAKE_LEDGER).exists() {
        rec.push_str(
            "# Bake history\n\n\
             Append-only. One record per `train apply` / `train all --apply` phase, written by\n\
             `train`'s `record_bake`. **Do not hand-edit** — and do not delete a record to make a diff look\n\
             tidy; a bake you would rather not have recorded is exactly the one worth reading later.\n\n\
             Git already holds the *values* a bake changed (`git log -p assets/config/config.ron`) and the\n\
             goldens (`git log -p tests/replay.rs`). This file adds the two things git cannot: WHICH elite\n\
             caused a change (the archives are gitignored and the next run overwrites them — the snapshot\n\
             under `assets/config/bake_history/` is the only surviving copy), and per-phase attribution\n\
             inside a single `train all` run, which git otherwise collapses into one diff.\n\n\
             A moved golden is only reviewable because of this trail. Read it before you trust a run.\n",
        );
    }
    let _ = write!(
        rec,
        "\n## {stamp} — {dim_name}\n\n\
         - elite:    {desc}\n\
         - archive:  {archive_disp}\n\
         - snapshot: {kept}\n\
         - goldens:  {verdict}\n",
        dim_name = dim_cli_name(dim),
        archive_disp = archive.display(),
        verdict = if repinned {
            format!(
                "MOVED and re-pinned\n  \
                 - snapshot: 0x{:016x} -> 0x{:016x}\n  \
                 - field:    0x{:016x} -> 0x{:016x}",
                before.0, after.0, before.1, after.1
            )
        } else {
            format!("unchanged (snapshot 0x{:016x}, field 0x{:016x})", after.0, after.1)
        },
    );
    let _ = write!(rec, "- files:    {}\n", touched.join(", "));

    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(BAKE_LEDGER)
        .map_err(|e| format!("{BAKE_LEDGER}: open for append: {e}"))?;
    std::io::Write::write_all(&mut f, rec.as_bytes())
        .map_err(|e| format!("{BAKE_LEDGER}: append: {e}"))?;
    Ok(kept)
}

/// The `train apply` dim name for a `Dim`.
fn dim_cli_name(dim: Dim) -> &'static str {
    match dim {
        Dim::Behavior => "behavior",
        Dim::World => "world",
        Dim::Audio => "audio",
        Dim::Levels => "levels",
    }
}

// ── Diagnostics: probe + prior ─────────────────────────────────────────────────────────────────────────

/// Run the authored brains once per seed and print the raw episode outcome + the fitness factors.
///
/// This exists so the behavioural minimal criterion's thresholds are **calibrated from measurement**
/// rather than guessed. The first guess (10% map coverage) was unreachable: `reachable_cells` counts fine
/// floor tiles (metres), of which a dungeon has thousands, while a squad on a 30 s tour walks a few
/// hundred at most.
fn probe(ticks: u32, seeds: &[u64]) -> Result<(), String> {
    use foundation_vs_slop::squad_ai::coevolve::{brains_of, squad_descriptor, swarm_descriptor, world_descriptor, SquadGenome, SwarmGenome};
    use foundation_vs_slop::squad_ai::evaluate::rollout_with_belief;
    use foundation_vs_slop::squad_ai::surprise::{minimal_criterion, witnessed_fraction};

    let t = Templates::authored();
    let squad = SquadGenome::authored(&t);
    let swarm = SwarmGenome::authored(&t);

    // Accumulate one run signature per seed (interest × tone axes) + whether every seed was admitted, so the
    // cross-seed REPLAYABILITY (expressive-range spread) can be reported after the per-seed detail. This is
    // the replayability objective's diagnostic surface, calibrated the same way the other proxies are.
    use foundation_vs_slop::squad_ai::replayability::{replayability_gated, spread, RunSignature};
    let mut signatures: Vec<RunSignature> = Vec::new();
    let mut all_admitted = true;

    for &seed in seeds {
        let r = rollout_with_belief(brains_of(&t, &squad, &swarm)?, None, None, None, seed, ticks);
        let o = &r.outcome;
        let coverage = if o.reachable_cells > 0 {
            o.cells_covered as f32 / o.reachable_cells as f32
        } else {
            0.0
        };
        println!("world 0x{seed:X}  ({ticks} ticks)");
        println!("  decisions      : {} ({} witnessed = {:.1}%)",
            r.trace.decisions.len(),
            r.trace.decisions.iter().filter(|d| d.witnessed).count(),
            100.0 * witnessed_fraction(&r.trace));
        println!("  squad          : {} of {} survived, {:.0} damage taken", o.survivors, o.squad_size, o.unit_damage_taken);
        println!("  agency         : {} duty decisions | {} / {ticks} ticks under player order ({:.0}% AI-driven)",
            o.squad_duty_decisions, o.ordered_ticks,
            100.0 * (1.0 - o.ordered_ticks as f32 / ticks as f32));
        println!("  swarm          : {} killed, {} alive (peak {})", o.crabs_killed, o.crabs_alive, o.crab_peak);
        println!("  parasite       : {} removed, {} alive (peak {})", o.manca_deaths, o.manca_alive, o.manca_peak);
        println!("  boss           : {} removed, {} alive", o.boss_deaths, o.boss_alive);
        println!("  vitality       : {} total deaths, {} total lives, {} peak pop  <- calibrate DEATHS/LIVES_FULLSCALE from these",
            o.total_deaths(), o.total_lives(), o.peak_population());
        println!("  coverage       : {} / {} cells = {:.2}%", o.cells_covered, o.reachable_cells, 100.0 * coverage);
        println!("  liveness       : {} violation(s)", o.liveness_violations);
        println!("  field          : peak {:.2}, flatness {:.1}% (field-sanity gate calibration)", o.peak_field, 100.0 * o.field_flatness);
        // Human-interest proxies (suspense / outcome-surprise / effectance) reduced from the per-checkpoint
        // survival-belief series — printed so their scale can be CALIBRATED from the shipped game (as the
        // MIN_COVERAGE / FEAR bands were), not guessed. See `squad_ai::interest`.
        let interest = foundation_vs_slop::squad_ai::interest::Interest::from_belief(&r.belief);
        println!(
            "  interest       : suspense {:.3}, outcome-surprise {:.3}, effectance {:.3}  (score {:.3}, {} checkpoints)",
            interest.suspense, interest.outcome_surprise, interest.effectance, interest.score(), r.belief.len()
        );
        // Tone / experience-shape proxies (dread / loneliness-liminality / pacing-arc) reduced from the same
        // belief series — printed for the same reason: their weighting into the objective is CALIBRATED by
        // the Phase-5 audition gate against rated runs, not guessed. See `squad_ai::experience`.
        let experience = foundation_vs_slop::squad_ai::experience::Experience::from_belief(&r.belief);
        println!(
            "  tone           : dread {:.3}, loneliness {:.3}, pacing {:.3}  (score {:.3})",
            experience.dread, experience.loneliness, experience.pacing, experience.score()
        );
        println!("  squad descr    : aggression {:.3}, exploration {:.3}", squad_descriptor(&r.trace, o).aggression, squad_descriptor(&r.trace, o).exploration);
        println!("  swarm descr    : aggression {:.3}, persistence {:.3}", swarm_descriptor(&r.trace).aggression, swarm_descriptor(&r.trace).exploration);
        println!("  world descr    : deaths {:.3}, lives {:.3} (normalised deaths×lives axes)", world_descriptor(o).aggression, world_descriptor(o).exploration);
        // What did the brain actually choose? The agency clause must be defined from this, not guessed.
        {
            use foundation_vs_slop::squad_ai::surprise::ActorKind;
            use std::collections::BTreeMap;
            let mut unit_modes: BTreeMap<String, u32> = BTreeMap::new();
            let mut creature_modes: BTreeMap<String, u32> = BTreeMap::new();
            for d in &r.trace.decisions {
                let bucket = if matches!(d.context.actor, ActorKind::Role(_)) { &mut unit_modes } else { &mut creature_modes };
                *bucket.entry(format!("{:?}", d.mode)).or_default() += 1;
            }
            let fmt = |m: &BTreeMap<String, u32>| {
                let mut v: Vec<_> = m.iter().collect();
                // SORT-OK: offline reporting over a Vec, not an ECS query.
                v.sort_by(|a, b| b.1.cmp(a.1));
                v.iter().take(8).map(|(k, c)| format!("{k} {c}")).collect::<Vec<_>>().join(", ")
            };
            println!("  unit modes     : {}", fmt(&unit_modes));
            println!("  creature modes : {}", fmt(&creature_modes));
            // Fairness / exploitability of the AUTHORED brain on this world — a reference point. The Phase-4
            // neuroevolution playtester (`rl_eval::evaluate_playtester`) searches for the strongest play, and
            // its exploitable max is what the fairness objective actually consumes; this is the shipped-brain
            // baseline, printed for calibration. See `squad_ai::fairness`.
            use foundation_vs_slop::squad_ai::fairness;
            let unit_counts: Vec<u32> = unit_modes.values().copied().collect();
            let comp = fairness::survival_competence(o.survivors, o.squad_size);
            let conc = fairness::mode_concentration(&unit_counts);
            println!(
                "  fairness (auth): competence {:.3}, concentration {:.3}, exploitability {:.3}, fairness {:.3}",
                comp, conc, fairness::exploitability(comp, conc), fairness::fairness(comp, conc)
            );
        }
        signatures.push(RunSignature::from_belief(&r.belief));
        let admitted = minimal_criterion(o).is_ok();
        all_admitted &= admitted;
        match minimal_criterion(o) {
            Ok(()) => println!("  criterion      : PASS"),
            Err(why) => println!("  criterion      : FAIL — {why}"),
        }
        println!();
    }

    // Cross-seed replayability: how much of the interest × tone space this candidate's runs cover. Gated —
    // a broken seed zeroes it (see `squad_ai::replayability`).
    if signatures.len() >= 2 {
        println!(
            "replayability across {} seeds : spread {:.3}, gated {:.3} ({})",
            signatures.len(),
            spread(&signatures),
            replayability_gated(&signatures, all_admitted),
            if all_admitted { "all seeds admitted" } else { "some seed inadmissible → gated to 0" }
        );
    }
    Ok(())
}

/// Sweep the shipped brain and commit the baseline prior. Every later behavioural `evolve` reads it.
fn prior(ticks: u32, seeds: &[u64]) -> Result<(), String> {
    let templates = Templates::authored();
    println!("sweeping the authored brain over {} world(s), {ticks} ticks each...", seeds.len());
    let prior = sweep_prior(&templates, seeds, ticks)?;
    prior.validate()?;
    let n = prior.total_observations();
    if n == 0 {
        // A prior with no evidence is uniform everywhere, which would make *every* candidate maximally
        // surprising. Refuse it at the door rather than train against noise.
        return Err("the sweep observed zero decisions — nothing to build a prior from".into());
    }
    write_ron(PRIOR_PATH, &prior)?;
    println!("wrote {PRIOR_PATH}: {n} observed decisions");
    Ok(())
}

// ── Co-evolution (evolve / evolve3) ──────────────────────────────────────────────────────────────────

/// Build a co-evolution `SearchConfig` from the shared search flags.
fn coevo_config(a: &SearchArgs) -> SearchConfig {
    SearchConfig {
        seed: a.seed,
        generations: a.generations,
        patience: a.patience,
        batch: a.batch,
        episode_ticks: a.ticks,
        dungeon_seeds: a.seeds.clone(),
        resolution: a.res,
        jobs: a.jobs,
    }
}

/// Run the three-way co-evolution (squad × swarm × world), driving `progress`, and return the templates +
/// filled archives. Both `evolve` and `evolve3` delegate here; they differ only in which archives they
/// commit — the world population co-evolves either way.
fn run_coevolution(cfg: SearchConfig, progress: &Progress) -> Result<(Templates, SearchResult), String> {
    let templates = Templates::authored();
    let src = std::fs::read_to_string(PRIOR_PATH)
        .map_err(|e| format!("{PRIOR_PATH}: {e} — run `train prior` first"))?;
    let prior = ron::from_str(&src).map_err(|e| format!("{PRIOR_PATH}: malformed: {e}"))?;

    println!(
        "co-evolving {} generations x {} children/side, {} ticks/episode, worlds {:?}, seed 0x{:X}, {} worker(s)",
        cfg.generations, cfg.batch, cfg.episode_ticks, cfg.dungeon_seeds, cfg.seed, cfg.jobs.max(1)
    );

    let result = search(&templates, &prior, &cfg, |generation, r| {
        let msg = format!(
            "squad {:>3} (qd {:.3}) | swarm {:>3} (qd {:.3}) | world {:>3} (qd {:.3}) | {} evals, {} inf, {} fail",
            r.squad.archive.coverage(),
            r.squad.archive.qd_score(),
            r.swarm.archive.coverage(),
            r.swarm.archive.qd_score(),
            r.world.archive.coverage(),
            r.world.archive.qd_score(),
            r.evaluations,
            r.rejected_infeasible,
            r.rejected_by_criterion
        );
        progress.tick(generation, 0.0, &msg);
        // Checkpoint every generation. `evolve3` otherwise commits the archives only at the very end, so
        // any interruption of the multi-hour run discards all of it. Writing each generation keeps the
        // latest completed generation always on disk.
        if let Err(e) = checkpoint_archives(&templates, r) {
            eprintln!("  (checkpoint write failed: {e})");
        }
    })?;
    progress.finish("co-evolution complete");
    Ok((templates, result))
}

/// Commit all three co-evolution archives to `assets/config/`. Called every generation from
/// `run_coevolution` (so an interrupted run keeps its latest archives) and again at the end.
fn checkpoint_archives(templates: &Templates, r: &SearchResult) -> Result<(), String> {
    write_ron(SQUAD_ARCHIVE_PATH, &squad_archive_doc(templates, &r.squad)?)?;
    write_ron(SWARM_ARCHIVE_PATH, &swarm_archive_doc(templates, &r.swarm)?)?;
    write_ron(WORLD_ARCHIVE_PATH, &world_archive_doc(&r.world)?)?;
    Ok(())
}

/// The reward-hacking guard is only a guard if someone reads the diff.
fn print_read_warning() {
    println!();
    println!("READ THE ELITES BEFORE SHIPPING THEM. They are RON in the same shape you author by hand;");
    println!("that readability is the reward-hacking guard, and it only works if someone looks.");
}

/// Two-population view: co-evolve and commit the squad + swarm archives (world is illuminated but not saved).
fn evolve_run(a: &SearchArgs, progress: &Progress) -> Result<SearchOutcome, String> {
    let (templates, result) = run_coevolution(coevo_config(a), progress)?;
    write_ron(SQUAD_ARCHIVE_PATH, &squad_archive_doc(&templates, &result.squad)?)?;
    write_ron(SWARM_ARCHIVE_PATH, &swarm_archive_doc(&templates, &result.swarm)?)?;
    println!();
    println!("wrote {SQUAD_ARCHIVE_PATH} ({} elites)", result.squad.archive.coverage());
    println!("wrote {SWARM_ARCHIVE_PATH} ({} elites)", result.swarm.archive.coverage());
    print_read_warning();
    Ok(SearchOutcome { best: 0.0, out: PathBuf::from(SQUAD_ARCHIVE_PATH) })
}

/// Three-population run: co-evolve and commit all three archives, including the evolved worlds.
fn evolve3_run(a: &SearchArgs, progress: &Progress) -> Result<SearchOutcome, String> {
    let (templates, result) = run_coevolution(coevo_config(a), progress)?;
    write_ron(SQUAD_ARCHIVE_PATH, &squad_archive_doc(&templates, &result.squad)?)?;
    write_ron(SWARM_ARCHIVE_PATH, &swarm_archive_doc(&templates, &result.swarm)?)?;
    write_ron(WORLD_ARCHIVE_PATH, &world_archive_doc(&result.world)?)?;
    println!();
    println!("wrote {SQUAD_ARCHIVE_PATH} ({} elites)", result.squad.archive.coverage());
    println!("wrote {SWARM_ARCHIVE_PATH} ({} elites)", result.swarm.archive.coverage());
    println!("wrote {WORLD_ARCHIVE_PATH} ({} elites)", result.world.archive.coverage());
    print_read_warning();
    // `--apply` for evolve3 bakes the evolved WORLD.
    Ok(SearchOutcome { best: 0.0, out: PathBuf::from(WORLD_ARCHIVE_PATH) })
}

// ── Single-output searches (levels / audio / behavior / rl / poet) ──────────────────────────────────────

/// Load the frozen mode-prior. The behavioural searches all need it (`train prior` writes it).
fn load_prior() -> Result<ModePrior, String> {
    let src = std::fs::read_to_string(PRIOR_PATH)
        .map_err(|e| format!("{PRIOR_PATH}: {e} — run `train prior` first"))?;
    ron::from_str(&src).map_err(|e| format!("{PRIOR_PATH}: malformed: {e}"))
}

/// Resolve the archive output path for a single-output search.
fn out_path(a: &SearchArgs, default: &str) -> PathBuf {
    a.out.clone().unwrap_or_else(|| PathBuf::from(default))
}

/// `PathBuf` → `&str` (the RON writer takes `&str`); non-UTF-8 paths are a loud error, not a silent lossy.
fn path_str(p: &Path) -> Result<&str, String> {
    p.to_str().ok_or_else(|| format!("non-UTF-8 output path: {}", p.display()))
}

/// Standalone level search: evolve dungeon architecture + furniture amount + mushroom amount under the
/// static level-quality objective. GPU-free and fast (each genome is generate-and-measure, not a rollout),
/// so it needs no `prior`.
fn levels_run(a: &SearchArgs, progress: &Progress) -> Result<SearchOutcome, String> {
    let (base, manifest) = level_eval::load_base()?;
    let cfg = LevelSearchConfig {
        seed: a.seed,
        generations: a.generations,
        patience: a.patience,
        batch: a.batch,
        sigma: 0.3,
        resolution: a.res,
        dungeon_seeds: a.seeds.clone(),
    };
    println!(
        "evolving levels: {} generations x {} children, res {}, held-in worlds {:?}, seed 0x{:X}, sigma {}",
        cfg.generations, cfg.batch, cfg.resolution, cfg.dungeon_seeds, cfg.seed, cfg.sigma
    );

    let result = level_search::search(&base, &manifest, &cfg, |generation, r| {
        let best = r.pop.archive.best().map_or(0.0, |e| e.fitness);
        let msg = format!(
            "levels {:>3} (qd {:.3}, best {:.3}) | {} evals, {} inf, {} fail",
            r.pop.archive.coverage(), r.pop.archive.qd_score(), best,
            r.evaluations, r.rejected_infeasible, r.rejected_by_criterion
        );
        progress.tick(generation, best, &msg);
    })?;
    progress.finish("done");

    let path = out_path(a, LEVELS_ARCHIVE_PATH);
    write_ron(path_str(&path)?, &level_search::level_archive_doc(&result.pop, &base)?)?;
    println!();
    println!("wrote {} ({} elites)", path.display(), result.pop.archive.coverage());
    print_read_warning();
    Ok(SearchOutcome { best: result.pop.archive.best().map_or(0.0, |e| e.fitness), out: path })
}

/// Standalone audio search: evolve the acoustic-stimulus config under the witnessed-learnable-surprise
/// objective. Its fitness is a full-sim rollout (sound feeds agent perception), so it needs the frozen
/// `prior`.
fn audio_run(a: &SearchArgs, progress: &Progress) -> Result<SearchOutcome, String> {
    let prior = load_prior()?;
    let cfg = AudioSearchConfig {
        seed: a.seed,
        generations: a.generations,
        patience: a.patience,
        batch: a.batch,
        sigma: 0.3,
        resolution: a.res,
        dungeon_seeds: a.seeds.clone(),
        episode_ticks: a.ticks,
    };
    println!(
        "evolving audio: {} generations x {} children, res {}, held-in worlds {:?}, seed 0x{:X}, {} ticks/episode",
        cfg.generations, cfg.batch, cfg.resolution, cfg.dungeon_seeds, cfg.seed, cfg.episode_ticks
    );

    let result = audio_search::search(&prior, &cfg, |generation, r| {
        let best = r.pop.archive.best().map_or(0.0, |e| e.fitness);
        let msg = format!(
            "audio {:>3} (qd {:.3}, best {:.3}) | {} evals, {} inf, {} fail",
            r.pop.archive.coverage(), r.pop.archive.qd_score(), best,
            r.evaluations, r.rejected_infeasible, r.rejected_by_criterion
        );
        progress.tick(generation, best, &msg);
    })?;
    progress.finish("done");

    let path = out_path(a, AUDIO_ARCHIVE_PATH);
    write_ron(path_str(&path)?, &audio_search::audio_archive_doc(&result.pop)?)?;
    println!();
    println!("wrote {} ({} elites)", path.display(), result.pop.archive.coverage());
    print_read_warning();
    Ok(SearchOutcome { best: result.pop.archive.best().map_or(0.0, |e| e.fitness), out: path })
}

/// Standalone behaviour search: evolve a curated subset of the `behavior:` config under the
/// witnessed-learnable-surprise objective. Full-sim rollout fitness → needs the frozen `prior`. Elites
/// overlay onto the shipped base, so an archive cell is a readable diff to transcribe.
fn behavior_run(a: &SearchArgs, progress: &Progress) -> Result<SearchOutcome, String> {
    let prior = load_prior()?;
    let cfg = BehaviorSearchConfig {
        seed: a.seed,
        generations: a.generations,
        patience: a.patience,
        batch: a.batch,
        sigma: 0.3,
        resolution: a.res,
        dungeon_seeds: a.seeds.clone(),
        episode_ticks: a.ticks,
    };
    println!(
        "evolving behaviour: {} generations x {} children, res {}, held-in worlds {:?}, seed 0x{:X}, {} ticks/episode",
        cfg.generations, cfg.batch, cfg.resolution, cfg.dungeon_seeds, cfg.seed, cfg.episode_ticks
    );

    let result = behavior_search::search(&prior, &cfg, |generation, r| {
        let best = r.pop.archive.best().map_or(0.0, |e| e.fitness);
        let msg = format!(
            "behaviour {:>3} (qd {:.3}, best {:.3}) | {} evals, {} inf, {} fail",
            r.pop.archive.coverage(), r.pop.archive.qd_score(), best,
            r.evaluations, r.rejected_infeasible, r.rejected_by_criterion
        );
        progress.tick(generation, best, &msg);
    })?;
    progress.finish("done");

    let path = out_path(a, BEHAVIOR_ARCHIVE_PATH);
    write_ron(path_str(&path)?, &behavior_search::behavior_archive_doc(&result.pop)?)?;
    println!();
    println!("wrote {} ({} elites)", path.display(), result.pop.archive.coverage());
    print_read_warning();
    Ok(SearchOutcome { best: result.pop.archive.best().map_or(0.0, |e| e.fitness), out: path })
}

/// Standalone policy (neuroevolution) search: evolve a `NeuralPolicy`'s MLP weights under the
/// witnessed-learnable-surprise objective. Full-sim rollout fitness → needs the frozen `prior`. An elite is
/// an OPAQUE weight vector; the guard is the minimal criterion + watching it play, not a readable diff.
fn rl_run(a: &SearchArgs, progress: &Progress) -> Result<SearchOutcome, String> {
    let prior = load_prior()?;
    let cfg = RlSearchConfig {
        seed: a.seed,
        generations: a.generations,
        patience: a.patience,
        batch: a.batch,
        sigma: 0.3,
        resolution: a.res,
        dungeon_seeds: a.seeds.clone(),
        episode_ticks: a.ticks,
        use_cma: a.cma,
    };
    println!(
        "evolving policy ({}): {} generations x {} children, res {}, held-in worlds {:?}, seed 0x{:X}, {} ticks/episode",
        if a.cma { "CMA-ME emitter" } else { "neuroevolution, isotropic" },
        cfg.generations, cfg.batch, cfg.resolution, cfg.dungeon_seeds, cfg.seed, cfg.episode_ticks
    );

    let result = rl_search::search(&prior, &cfg, |generation, r| {
        let best = r.pop.archive.best().map_or(0.0, |e| e.fitness);
        let msg = format!(
            "policy {:>3} (qd {:.3}, best {:.3}) | {} evals, {} inf, {} fail",
            r.pop.archive.coverage(), r.pop.archive.qd_score(), best,
            r.evaluations, r.rejected_infeasible, r.rejected_by_criterion
        );
        progress.tick(generation, best, &msg);
    })?;
    progress.finish("done");

    let path = out_path(a, RL_ARCHIVE_PATH);
    write_ron(path_str(&path)?, &rl_search::rl_archive_doc(&result.pop)?)?;
    println!();
    println!("wrote {} ({} elites)", path.display(), result.pop.archive.coverage());
    print_read_warning();
    Ok(SearchOutcome { best: result.pop.archive.best().map_or(0.0, |e| e.fitness), out: path })
}

/// One POET niche, serialized: an evolved world paired with the squad that solves it, and the pairing's
/// fitness + human-interest score. Both genomes are the same readable RON the config searches emit.
#[derive(serde::Serialize)]
struct PoetNicheDoc {
    best_fitness: f32,
    best_interest: f32,
    env: WorldGenome,
    agent: SquadGenome,
}

/// POET (Wang et al. 2019): the open-ended outer loop. It co-generates *worlds* and the *squads* that solve
/// them, admitting a new world only when it sits in the "neither too easy nor too hard" band for the current
/// best squad, transferring squads between worlds, and steering budget by learning progress. Each pairing is
/// scored by a real rollout, so it needs the frozen `prior`.
fn poet_run(a: &SearchArgs, progress: &Progress) -> Result<SearchOutcome, String> {
    let prior: ModePrior = load_prior()?;

    let t = Templates::authored();
    // POET evolves worlds × squads against the shipped (authored) swarm — one moving opponent at a time.
    let swarm = SwarmGenome::authored(&t);
    let seeds = a.seeds.clone();
    let ticks = a.ticks;
    let poet_cfg = PoetConfig { seed: a.seed, iterations: a.generations, ..PoetConfig::default() };

    println!(
        "POET: {} iterations, up to {} niches, held-in worlds {:?}, seed 0x{:X}, {} ticks/episode",
        poet_cfg.iterations, poet_cfg.max_niches, seeds, poet_cfg.seed, ticks
    );

    // Score a (world, squad) pairing: two rollouts (the learnability pair) with the evolved world installed,
    // gated by the behavioural minimal criterion, returning the witnessed-learnable-surprise fitness and the
    // human-interest score. `None` is an MCC reject (POET reads it as "too hard / degenerate").
    let evaluate = |world_g: &WorldGenome, squad_g: &SquadGenome| -> Option<(f32, f32)> {
        let world_config = world_genome::decode(world_g).ok()?;
        let brains = brains_of(&t, squad_g, &swarm).ok()?;
        let a = rollout_with_belief(brains.clone(), Some(world_config.clone()), None, None, seeds[0], ticks);
        surprise::minimal_criterion(&a.outcome).ok()?;
        let b = rollout(brains, Some(world_config), None, None, seeds[1], ticks);
        surprise::minimal_criterion(&b.outcome).ok()?;
        let fitness = surprise::fitness(&a.trace, &b.trace, &prior).score();
        let interest = Interest::from_belief(&a.belief).score();
        Some((fitness, interest))
    };

    let result = poet_search(
        &poet_cfg,
        world_genome::authored(),
        SquadGenome::authored(&t),
        |g: &WorldGenome, rng| world_genome::mutate(g, 0.3, rng),
        |g: &SquadGenome, rng| mutate_squad_feasible(&t, g, rng),
        evaluate,
        |it, r| {
            let peak_interest = r.niches.iter().map(|n| n.best_interest).fold(0.0f32, f32::max);
            let best_fit = r.niches.iter().map(|n| n.best_fitness).fold(0.0f32, f32::max);
            let msg = format!(
                "{} niches | {} created, {} rejected, {} transfers | {} evals | best fit {:.3}, peak interest {:.3}",
                r.niches.len(), r.created, r.rejected, r.transfers, r.evaluations, best_fit, peak_interest
            );
            progress.tick(it, best_fit, &msg);
        },
    )?;
    progress.finish("done");

    let doc: Vec<PoetNicheDoc> = result
        .niches
        .iter()
        .map(|n| PoetNicheDoc {
            best_fitness: n.best_fitness,
            best_interest: n.best_interest,
            env: n.env.clone(),
            agent: n.agent.clone(),
        })
        .collect();
    let path = out_path(a, POET_ARCHIVE_PATH);
    write_ron(path_str(&path)?, &doc)?;
    println!();
    println!(
        "wrote {} ({} niches, {} created, {} transfers over the run)",
        path.display(), result.niches.len(), result.created, result.transfers
    );
    print_read_warning();
    let best_fit = result.niches.iter().map(|n| n.best_fitness).fold(0.0f32, f32::max);
    Ok(SearchOutcome { best: best_fit, out: path })
}

// ── `train apply`: permanently bake an evolved elite into the shipped defaults ──────────────────────

/// Permanently ship an evolved elite: rewrite the `config.ron` slice(s), the matching Rust `Default`
/// impl(s), and the deterministic-replay goldens together, so `cargo test` stays green. Full-auto, one
/// call. `policy` is intentionally unsupported (a `NeuralPolicy` has no config slice — use
/// `FVS_POLICY_ELITE` to run one).
fn apply_archive(
    dim: Dim,
    archive: &Path,
    cell: Option<&str>,
    repin_goldens: bool,
) -> Result<(), String> {
    // Env overlays would double-apply and perturb the golden recompute — refuse to bake while any is set.
    for v in [
        foundation_vs_slop::elite_overlay::BEHAVIOR_ENV,
        foundation_vs_slop::elite_overlay::WORLD_ENV,
        foundation_vs_slop::elite_overlay::AUDIO_ENV,
        foundation_vs_slop::elite_overlay::LEVELS_ENV,
        foundation_vs_slop::elite_overlay::POLICY_ENV,
    ] {
        if std::env::var(v).map(|s| !s.trim().is_empty()).unwrap_or(false) {
            return Err(format!("unset {v} before `apply` (an env overlay conflicts with a permanent bake)"));
        }
    }

    let archive = path_str(archive)?;
    let spec = match cell {
        Some(c) => format!("{archive}#{c}"),
        None => archive.to_string(),
    };

    // 1. Load the clean shipped config, then overlay the elite — validated on the same one path the runtime
    //    overlay uses (a cross-slice-invalid level elite fails here, before anything is written).
    //
    //    `base` is the SAME config without the elite. `splice_block` diffs `base` against `gc` to find what
    //    the elite actually moved: both are the same Rust type through the same serializer, so a path that
    //    differs is a real change and not an artefact of the authored file omitting a defaulted field.
    let base = foundation_vs_slop::config::load_game_config()?;
    let mut gc = foundation_vs_slop::config::load_game_config()?;
    let desc = apply_dim(&mut gc, dim, &spec)?;
    println!("apply: {desc}");

    // 2 + 3. Splice the config.ron slice(s) and regenerate the guarded `Default` impl(s).
    let mut cfg_text = std::fs::read_to_string(CONFIG_PATH).map_err(|e| format!("{CONFIG_PATH}: {e}"))?;
    let mut touched: Vec<String> = vec![CONFIG_PATH.to_string()];
    match dim {
        Dim::Behavior => {
            cfg_text = splice_block(
                &cfg_text,
                "behavior",
                &ron_slice(&base.behavior)?,
                &ron_slice(&gc.behavior)?,
            )?;
            regen_default("src/behavior_tuning.rs", "BehaviorTuning", &format!("{:#?}", gc.behavior))?;
            touched.push("src/behavior_tuning.rs".into());
        }
        Dim::World => {
            // All four slices `world_genome` encodes, matching `apply_dim(Dim::World)`. If the permanent
            // bake spliced fewer slices than the runtime overlay applies, one elite would mean two different
            // games — and its archived fitness would match neither.
            use foundation_vs_slop::almond_water::AlmondWaterDynamics;
            use foundation_vs_slop::light::LightingDynamics;
            cfg_text = splice_block(&cfg_text, "sim", &ron_slice(&base.sim)?, &ron_slice(&gc.sim)?)?;
            cfg_text = splice_block(
                &cfg_text,
                "ai_tuning",
                &ron_slice(&base.ai_tuning)?,
                &ron_slice(&gc.ai_tuning)?,
            )?;
            cfg_text = splice_block(&cfg_text, "mold", &ron_slice(&base.mold)?, &ron_slice(&gc.mold)?)?;
            // The `almond_water:` block also holds structural + visual knobs the search never touches, so
            // splice only the evolvable subset — its field names are a flat subset of the authored block,
            // and both sides are the same type, so `splice_block`'s shape check passes by construction.
            cfg_text = splice_block(
                &cfg_text,
                "almond_water",
                &ron_slice(&AlmondWaterDynamics::from_config(&base.almond_water))?,
                &ron_slice(&AlmondWaterDynamics::from_config(&gc.almond_water))?,
            )?;
            // Same subset trick for `lighting:` — only the two gameplay dials evolve; the visual knobs are
            // authored and must survive the bake untouched.
            cfg_text = splice_block(
                &cfg_text,
                "lighting",
                &ron_slice(&LightingDynamics::from_config(&base.lighting))?,
                &ron_slice(&LightingDynamics::from_config(&gc.lighting))?,
            )?;
            regen_default("src/sim.rs", "SimTuning", &format!("{:#?}", gc.sim))?;
            regen_default("src/ai/tuning.rs", "AiTuning", &format!("{:#?}", gc.ai_tuning))?;
            regen_default("src/mold.rs", "MoldConfig", &format!("{:#?}", gc.mold))?;
            regen_default(
                "src/almond_water/mod.rs",
                "AlmondWaterDynamics",
                &format!("{:#?}", AlmondWaterDynamics::from_config(&gc.almond_water)),
            )?;
            regen_default(
                "src/light.rs",
                "LightingDynamics",
                &format!("{:#?}", LightingDynamics::from_config(&gc.lighting)),
            )?;
            touched.push("src/sim.rs".into());
            touched.push("src/ai/tuning.rs".into());
            touched.push("src/mold.rs".into());
            touched.push("src/almond_water/mod.rs".into());
            touched.push("src/light.rs".into());
        }
        Dim::Audio => {
            cfg_text =
                splice_block(&cfg_text, "audio", &ron_slice(&base.audio)?, &ron_slice(&gc.audio)?)?;
            regen_default("src/audio_tuning.rs", "AudioTuning", &format!("{:#?}", gc.audio))?;
            touched.push("src/audio_tuning.rs".into());
        }
        Dim::Levels => {
            // These config types have no `Default` impl, so only config.ron is rewritten.
            cfg_text =
                splice_block(&cfg_text, "dungeon", &ron_slice(&base.dungeon)?, &ron_slice(&gc.dungeon)?)?;
            cfg_text =
                splice_block(&cfg_text, "mycelia", &ron_slice(&base.mycelia)?, &ron_slice(&gc.mycelia)?)?;
            cfg_text = splice_block(
                &cfg_text,
                "metropolis",
                &ron_slice(&base.placement.metropolis)?,
                &ron_slice(&gc.placement.metropolis)?,
            )?;
            cfg_text = splice_block(
                &cfg_text,
                "density",
                &ron_slice(&base.placement.density)?,
                &ron_slice(&gc.placement.density)?,
            )?;
        }
    }
    std::fs::write(CONFIG_PATH, &cfg_text).map_err(|e| format!("{CONFIG_PATH}: write: {e}"))?;

    // 4. Recompute the goldens from the freshly-baked config and compare them against the committed ones.
    //    (Env overlays are guaranteed unset above, so `deterministic_core` reproduces exactly what the replay
    //    test will assert.) Go through the stability guard — a golden is only trusted if repeated builds
    //    agree, so a non-deterministic core reds here instead of pinning a one-off value.
    //
    //    A MOVED golden aborts unless `--repin-goldens` says otherwise. This used to re-pin unconditionally,
    //    and on 2026-07-16 that turned a real regression invisible: `cargo train all` spliced a machine-baked
    //    levels elite over the authored level AND re-pinned the goldens to match it, so the five tests that
    //    correctly detected the swap went green against the new value. A bake that both changes the sim and
    //    moves the ruler cannot be reviewed. TESTING.md: a golden is "a deliberate, human-reviewed act —
    //    never auto-approve a diff."
    let (snap, field) = recompute_goldens_stable(GOLDEN_STABILITY_REPS)?;
    let committed = read_committed_goldens()?;
    let drifted = (snap, field) != committed;
    if drifted && !repin_goldens {
        // `config.ron` is already written at this point, so say so — the operator needs to know the tree is
        // dirty and exactly how to undo it.
        return Err(format!(
            "GOLDEN DRIFT — refusing to re-pin.\n\
             \n  snapshot: 0x{:016x} (committed)  ->  0x{snap:016x} (this bake)\n  \
               field:    0x{:016x} (committed)  ->  0x{field:016x} (this bake)\n\
             \nThe baked elite changes the shipped sim. That may be exactly what you want — but a golden is a\n\
             deliberate, human-reviewed act (TESTING.md), and re-pinning it here would move the ruler that\n\
             measures the change, in the same step that makes it.\n\
             \n{CONFIG_PATH} HAS been written; {REPLAY_PATH} has NOT.\n\
             \n  review:  git diff {CONFIG_PATH}\n  \
               accept:  re-run with --repin-goldens\n  \
               revert:  git checkout -- {}",
            committed.0,
            committed.1,
            touched.join(" "),
        ));
    }
    if drifted {
        repin_replay(snap, field)?;
        touched.push(REPLAY_PATH.to_string());
    }

    // 5. Record it. Before the report, not after: the trail must survive an operator who never reads stdout
    //    (every unattended `train all` phase). Re-pinning surrenders change detection, so this ledger is the
    //    thing a reviewer reads instead — it is not optional bookkeeping, it is the review surface.
    let kept = record_bake(dim, Path::new(&archive), &desc, committed, (snap, field), drifted, &touched)?;
    touched.push(BAKE_LEDGER.to_string());

    // 6 + 7. Report + next steps.
    println!();
    if drifted {
        println!("baked. goldens MOVED and were re-pinned (--repin-goldens): snapshot 0x{snap:016x}, field 0x{field:016x}");
        println!("  was: snapshot 0x{:016x}, field 0x{:016x}", committed.0, committed.1);
    } else {
        println!("baked. goldens unchanged: snapshot 0x{snap:016x}, field 0x{field:016x}");
    }
    println!("recorded in {BAKE_LEDGER}; archive kept at {kept}");
    println!("files changed:");
    for f in &touched {
        println!("  {f}");
    }
    println!();
    println!("NEXT: regenerate the baseline prior for the new shipped tuning:  train prior");
    if drifted {
        // A re-pinned golden makes the suite pass by construction — say so where it will actually be read,
        // so nobody mistakes a green run for evidence the training was good.
        println!("      REVIEW THIS: the goldens moved, so `cargo test --features test-harness` now passes by");
        println!("      construction and proves only that the sim is reproducible — NOT that the bake was good.");
        println!("      The reviewable artefacts are:  git diff {CONFIG_PATH}   and   {BAKE_LEDGER}");
    } else {
        println!("      review:  git diff        verify:  cargo test --features test-harness");
    }
    // `git checkout -- …` restores tracked files only; BAKE_LEDGER is an append-only ledger, untracked on
    // its first write. Passing an untracked path makes git reject the WHOLE pathspec and revert nothing, so
    // list it apart from the checkout rather than let it abort the recipe an operator copies verbatim.
    let reverts: Vec<&str> = touched.iter().map(String::as_str).filter(|f| *f != BAKE_LEDGER).collect();
    println!("      revert:  git checkout -- {}", reverts.join(" "));
    println!("               then drop this bake's entry from {BAKE_LEDGER} (delete the file if it was the first bake).");
    Ok(())
}

/// Serialize a config slice to RON in config.ron's anonymous-tuple style (`struct_names: false`).
fn ron_slice<T: serde::Serialize>(v: &T) -> Result<String, String> {
    ron::ser::to_string_pretty(v, ron::ser::PrettyConfig::default()).map_err(|e| format!("serialize slice: {e}"))
}

/// A scalar leaf found in RON source text: where it lives in the tree, and the exact byte span of its value
/// token so it can be substituted without disturbing anything around it.
struct Leaf {
    /// Dotted field path from the block root, with `[i]` for sequence elements (`room_types[2].weight`).
    path: String,
    /// Byte range of the scalar token within the scanned text.
    span: std::ops::Range<usize>,
    /// The scalar token's source text (`0x5C09191`, `2`, `1.0`, `"bathroom"`, `true`, `None`).
    text: String,
}

/// Scan RON source into its scalar leaves, tracking the path to each and the byte span of its value token.
///
/// This is deliberately a *source* scanner, not a deserializer: it must know where each scalar sits in the
/// original bytes so `splice_block` can substitute one number and leave every comment, alignment, and
/// literal spelling around it untouched. It skips `//`, `/* */`, and string contents — which is also why the
/// old paren-counting block scan was wrong (it counted `(` inside comments and strings).
fn scan_ron_leaves(text: &str) -> Result<Vec<Leaf>, String> {
    let b = text.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();

    // Advance past whitespace and comments.
    fn skip(b: &[u8], mut i: usize) -> usize {
        loop {
            while i < b.len() && (b[i] as char).is_whitespace() {
                i += 1;
            }
            if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(b.len());
                continue;
            }
            return i;
        }
    }

    // An identifier (field name, enum variant, `Some`, `None`, `true`).
    fn ident(b: &[u8], mut i: usize) -> (usize, String) {
        let s = i;
        while i < b.len() && ((b[i] as char).is_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        (i, String::from_utf8_lossy(&b[s..i]).into_owned())
    }

    // Recursive-descent over one value. `path` is the path to THIS value.
    #[allow(clippy::too_many_arguments)]
    fn value(
        b: &[u8],
        text: &str,
        mut i: usize,
        path: &str,
        out: &mut Vec<Leaf>,
        depth: usize,
    ) -> Result<usize, String> {
        if depth > 32 {
            return Err("config.ron: value nested deeper than 32 — refusing".to_string());
        }
        i = skip(b, i);
        if i >= b.len() {
            return Err(format!("config.ron: unexpected end of input at `{path}`"));
        }

        // `Some(...)` / `None` — an Option wrapper is transparent to the path.
        if (b[i] as char).is_alphabetic() {
            let (after, id) = ident(b, i);
            let j = skip(b, after);
            if id == "Some" && j < b.len() && b[j] == b'(' {
                i = value(b, text, j + 1, path, out, depth + 1)?;
                i = skip(b, i);
                if i >= b.len() || b[i] != b')' {
                    return Err(format!("config.ron: unclosed `Some(` at `{path}`"));
                }
                return Ok(i + 1);
            }
            // A named struct/enum like `Grid` or `Foo(...)`: if a `(` follows, descend; else it's a scalar.
            if j < b.len() && b[j] == b'(' {
                return strukt(b, text, j + 1, path, out, depth + 1);
            }
            out.push(Leaf { path: path.to_string(), span: i..after, text: id });
            return Ok(after);
        }

        match b[i] {
            b'(' => strukt(b, text, i + 1, path, out, depth + 1),
            b'[' => {
                let mut n = 0usize;
                i += 1;
                loop {
                    i = skip(b, i);
                    if i >= b.len() {
                        return Err(format!("config.ron: unclosed `[` at `{path}`"));
                    }
                    if b[i] == b']' {
                        return Ok(i + 1);
                    }
                    i = value(b, text, i, &format!("{path}[{n}]"), out, depth + 1)?;
                    n += 1;
                    i = skip(b, i);
                    if i < b.len() && b[i] == b',' {
                        i += 1;
                    }
                }
            }
            b'"' => {
                let s = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(b.len());
                out.push(Leaf {
                    path: path.to_string(),
                    span: s..i,
                    text: text[s..i].to_string(),
                });
                Ok(i)
            }
            _ => {
                // A bare scalar: run to the next delimiter. Comments never start mid-token in RON.
                let s = i;
                while i < b.len() && !matches!(b[i], b',' | b')' | b']' | b'\n') {
                    i += 1;
                }
                let raw = text[s..i].trim_end();
                out.push(Leaf {
                    path: path.to_string(),
                    span: s..s + raw.len(),
                    text: raw.to_string(),
                });
                Ok(s + raw.len())
            }
        }
    }

    // A struct body, positioned just after its `(`. Fields are `key: value`; bare values are tuple elements.
    fn strukt(
        b: &[u8],
        text: &str,
        mut i: usize,
        path: &str,
        out: &mut Vec<Leaf>,
        depth: usize,
    ) -> Result<usize, String> {
        let mut tuple_idx = 0usize;
        loop {
            i = skip(b, i);
            if i >= b.len() {
                return Err(format!("config.ron: unclosed `(` at `{path}`"));
            }
            if b[i] == b')' {
                return Ok(i + 1);
            }
            // Field or tuple element? A `key:` has an identifier then a colon.
            let mut child = format!("{path}[{tuple_idx}]");
            if (b[i] as char).is_alphabetic() || b[i] == b'_' {
                let (after, id) = ident(b, i);
                let j = skip(b, after);
                if j < b.len() && b[j] == b':' {
                    child = if path.is_empty() { id } else { format!("{path}.{id}") };
                    i = j + 1;
                } else {
                    tuple_idx += 1;
                }
            } else {
                tuple_idx += 1;
            }
            i = value(b, text, i, &child, out, depth + 1)?;
            i = skip(b, i);
            if i < b.len() && b[i] == b',' {
                i += 1;
            }
        }
    }

    i = skip(b, i);
    if i >= b.len() || b[i] != b'(' {
        return Err("config.ron: block value does not start with `(`".to_string());
    }
    let end = strukt(b, text, i + 1, "", &mut out, 0)?;
    let tail = skip(b, end);
    if tail < b.len() {
        return Err(format!("config.ron: trailing input after block value: {:?}", &text[tail..]));
    }
    Ok(out)
}

/// Do two RON scalar tokens denote the same value? Compares by PARSED value, not by spelling — so the
/// authored `seed: 0x5C09191` and the serializer's `96506257` are equal, and the authored line is left
/// alone (hex spelling, alignment, and its `// nods to SCP-9191` comment all preserved).
fn scalar_eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (ron::from_str::<ron::Value>(a), ron::from_str::<ron::Value>(b)) {
        (Ok(x), Ok(y)) => x == y,
        // Unparseable on either side: fall back to exact text. Never treat "can't tell" as "equal".
        _ => false,
    }
}

/// Locate the `<name>: ( … )` block in `config.ron`, returning the byte span of its VALUE (the `( … )`).
/// The scan is comment- and string-aware, unlike the old raw `(`/`)` char count.
fn find_block_value(text: &str, name: &str) -> Result<std::ops::Range<usize>, String> {
    let header = format!("{name}: (");
    let mut search = 0usize;
    let at = loop {
        let rel = text[search..]
            .find(&header)
            .ok_or_else(|| format!("config.ron: no `{name}:` block header"))?;
        let abs = search + rel;
        // Must be a real field, not a substring of a longer name (`density` inside `foo_density`) and not
        // inside a comment.
        let line_start = text[..abs].rfind('\n').map(|n| n + 1).unwrap_or(0);
        let prefix = &text[line_start..abs];
        let in_comment = prefix.contains("//");
        let boundary = prefix.chars().last().is_none_or(|c| c.is_whitespace());
        if !in_comment && boundary {
            break abs;
        }
        search = abs + header.len();
    };
    let open = at + name.len() + 2; // past `name:`, onto the ` (`
    let open = text[open..].find('(').map(|o| open + o).ok_or("config.ron: malformed header")?;
    // Reuse the scanner to find the matching close: scan the tail and take the end it reports.
    let tail = &text[open..];
    let mut depth = 0usize;
    let b = tail.as_bytes();
    let (mut i, mut in_str, mut in_line_comment, mut in_block_comment) = (0usize, false, false, false);
    while i < b.len() {
        let c = b[i];
        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
        } else if in_block_comment {
            if c == b'*' && i + 1 < b.len() && b[i + 1] == b'/' {
                in_block_comment = false;
                i += 1;
            }
        } else if in_str {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            in_line_comment = true;
            i += 1;
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            in_block_comment = true;
            i += 1;
        } else if c == b'"' {
            in_str = true;
        } else if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                return Ok(open..open + i + 1);
            }
        }
        i += 1;
    }
    Err(format!("config.ron: unbalanced `{name}:` block"))
}

/// Rewrite the `<name>: ( … )` block in `config.ron` to hold `value_ron`, **substituting only the scalars
/// that actually changed** and leaving every other byte — comments, alignment, literal spelling — exactly
/// as authored.
///
/// This replaced a `to_string_pretty` splice that overwrote the whole block with one generated line. That
/// destroyed every comment inside it: on 2026-07-16 a `--dim levels` bake stripped ~279 lines of
/// hand-written rationale from `config.ron` (which carries ~563 comment lines — the reasoning IS the file).
///
/// **It refuses rather than guessing.** If the bake changes the block's *shape* — a dropped/added field, a
/// different sequence length, an `Option` appearing or vanishing — there is no honest edit: the prose around
/// the old shape describes a design the elite no longer has, so preserving it would leave the file
/// confidently lying. `--dim levels` can do exactly this (`level_genome::decode` drops unselected
/// `room_types`), and that bake now errors with the path instead of quietly rewriting the block.
///
/// `before_ron` and `after_ron` are the SAME slice serialized before and after the elite was applied. The
/// diff is taken between those two — never between the authored *text* and the baked value — because the
/// authored text legitimately omits `#[serde(default)]` fields (`room_types[…].expands` is written only on
/// the types that set it). Diffing text against a serializer that always spells every field out would read
/// those omissions as a shape change. Two serializations of one type cannot disagree that way, so what is
/// left is real: a value moved, a sequence grew or shrank, an `Option` flipped.
fn splice_block(text: &str, name: &str, before_ron: &str, after_ron: &str) -> Result<String, String> {
    let span = find_block_value(text, name)?;
    let authored = scan_ron_leaves(&text[span.clone()])
        .map_err(|e| format!("{name}: reading the authored block: {e}"))?;
    let before = scan_ron_leaves(before_ron).map_err(|e| format!("{name}: reading the pre-bake value: {e}"))?;
    let after = scan_ron_leaves(after_ron).map_err(|e| format!("{name}: reading the baked value: {e}"))?;

    // Shape check: before vs after. Same Rust type through the same serializer, so any path difference is a
    // genuine structural change (sequence length, `Option` variant), not a defaulted-field omission.
    let (bp, ap): (Vec<&str>, Vec<&str>) = (
        before.iter().map(|l| l.path.as_str()).collect(),
        after.iter().map(|l| l.path.as_str()).collect(),
    );
    if bp != ap {
        let dropped: Vec<&&str> = bp.iter().filter(|p| !ap.contains(p)).take(4).collect();
        let added: Vec<&&str> = ap.iter().filter(|p| !bp.contains(p)).take(4).collect();
        return Err(format!(
            "cannot splice `{name}`: the elite changes the block's SHAPE, not just its values \
             ({} leaves before, {} after).\n  \
               dropped: {dropped:?}\n  \
               added:   {added:?}\n\
             \nRewriting it would mean deleting or inventing lines, and the hand-written rationale around \
             them describes the AUTHORED design — preserving those comments would leave them describing a \
             structure that no longer exists, which is worse than deleting them, because a stale comment \
             reads as authoritative.\n\
             \nThis is expected for `--dim levels` when an elite drops a room type (`level_genome::decode` \
             skips unselected `room_types`). Ship that elite through the runtime overlay \
             (`FVS_LEVELS_ELITE`, see src/elite_overlay.rs), or hand-edit the block and its prose together.",
            bp.len(),
            ap.len(),
        ));
    }

    // Where the authored text actually spells each field out.
    let placed: std::collections::HashMap<&str, &Leaf> =
        authored.iter().map(|l| (l.path.as_str(), l)).collect();

    // The changed leaves, and where each one lives in the authored text.
    let mut edits: Vec<(std::ops::Range<usize>, &str)> = Vec::new();
    let mut unplaceable: Vec<String> = Vec::new();
    for (b, a) in before.iter().zip(&after) {
        if scalar_eq(&b.text, &a.text) {
            continue;
        }
        match placed.get(a.path.as_str()) {
            Some(l) => edits.push((l.span.clone(), a.text.as_str())),
            // The value moved, but the authored file never writes this field — it rides on a serde default.
            // Adding the line means choosing where to put it and what to say about it. Refuse.
            None => unplaceable.push(format!("{} ({} -> {})", a.path, b.text, a.text)),
        }
    }
    if !unplaceable.is_empty() {
        return Err(format!(
            "cannot splice `{name}`: the elite changes {} field(s) the authored block does not spell out \
             (they sit at their `#[serde(default)]` value):\n  {}\n\
             \nWriting them would mean inserting lines into hand-authored prose and deciding, on your \
             behalf, where they go and what they mean. Hand-edit the block, or ship the elite through the \
             runtime overlay (see src/elite_overlay.rs).",
            unplaceable.len(),
            unplaceable.join("\n  "),
        ));
    }

    // Apply right-to-left so earlier spans stay valid.
    // SORT-OK: byte spans in one file, unique by construction — offline tooling, not an ECS query.
    edits.sort_by_key(|(s, _)| std::cmp::Reverse(s.start));
    let mut out = text.to_string();
    for (s, new_text) in &edits {
        out.replace_range(span.start + s.start..span.start + s.end, new_text);
    }
    println!(
        "  {name}: {} value(s) changed, {} unchanged (comments preserved)",
        edits.len(),
        before.len() - edits.len()
    );
    Ok(out)
}

/// Replace the body of `fn default() -> Self { … }` inside `impl Default for <ty>` in a Rust source file with
/// `literal` (a `{:#?}` struct-literal expression). Brace-balanced from the fn's opening `{`.
fn regen_default(path: &str, ty: &str, literal: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let impl_at = text
        .find(&format!("impl Default for {ty} {{"))
        .ok_or_else(|| format!("{path}: no `impl Default for {ty}`"))?;
    let fn_marker = "fn default() -> Self {";
    let fn_rel = text[impl_at..].find(fn_marker).ok_or_else(|| format!("{path}: no default fn for {ty}"))?;
    let body_open = impl_at + fn_rel + fn_marker.len(); // just past the `{`
    let bytes = text.as_bytes();
    let mut depth = 1i32;
    let mut j = body_open;
    while j < bytes.len() && depth > 0 {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        j += 1;
    }
    if depth != 0 {
        return Err(format!("{path}: unbalanced default fn for {ty}"));
    }
    let body_close = j - 1; // the matching `}`
    let indented = literal.replace('\n', "\n        ");
    let mut out = String::with_capacity(text.len() + literal.len());
    out.push_str(&text[..body_open]);
    out.push_str(&format!("\n        {indented}\n    "));
    out.push_str(&text[body_close..]);
    std::fs::write(path, out).map_err(|e| format!("{path}: write: {e}"))
}

/// Recompute the deterministic-core goldens from the (freshly-baked) config.ron — the exact run the replay
/// test asserts: `deterministic_core` (seed from config.ron) + 1800 ticks. One measurement.
fn recompute_goldens() -> (u64, u64) {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1800);
    (snapshot_hash(&mut app), field_hash(&mut app))
}

/// **Determinism stability guard for golden re-baking.** Recompute the deterministic-core goldens `reps`
/// times (a fresh `App` each) and require every measurement to agree before returning the value. A re-bake
/// must never stamp a hash that isn't at least *intra-process* stable — a golden pinned from a single
/// measurement of a non-deterministic core would bake in whichever value that one run happened to produce.
///
/// This catches any nondeterminism observable across repeated builds in ONE process. The known-rare,
/// load-correlated CROSS-process flake (a wrong hash only under heavy CPU contention) is not fully
/// observable intra-process, so the re-bake procedure additionally runs `train verify --reps K` in several
/// *fresh* processes on an idle box and requires them to agree — this function is the first line, that is
/// the second. `Err` lists the disagreement rather than silently picking one.
fn recompute_goldens_stable(reps: u32) -> Result<(u64, u64), String> {
    let reps = reps.max(1);
    let first = recompute_goldens();
    for r in 1..reps {
        let again = recompute_goldens();
        if again != first {
            return Err(format!(
                "deterministic-core goldens are NOT stable across repeated builds — measurement 0 = \
                 (snapshot 0x{:016x}, field 0x{:016x}) but measurement {r} = (snapshot 0x{:016x}, \
                 field 0x{:016x}). Refusing to re-pin a non-deterministic golden. The core must be \
                 bit-reproducible before a golden can be committed.",
                first.0, first.1, again.0, again.1
            ));
        }
    }
    Ok(first)
}

/// How many times `train apply` remeasures the deterministic-core goldens (a fresh `App` each) and requires
/// agreement before pinning them. Cheap relative to the search that produced the elite; catches intra-process
/// nondeterminism at the door.
const GOLDEN_STABILITY_REPS: u32 = 3;

/// `train verify [--reps K]` — the scriptable determinism guard. Recompute the deterministic-core goldens
/// with the stability check and print them. Run it in several FRESH processes (a shell loop, ideally under
/// CPU load) and diff the output to catch the rare cross-process flake before trusting a re-baked golden.
fn verify(reps: u32) -> Result<(), String> {
    let (snap, field) = recompute_goldens_stable(reps)?;
    println!("deterministic-core stable over {reps} build(s): snapshot 0x{snap:016x}, field 0x{field:016x}");
    Ok(())
}

const SNAP_MARKER: &str = "const GOLDEN: u64 = ";
const FIELD_MARKER: &str = "const GOLDEN_FIELD: u64 = ";

/// The goldens currently committed in `tests/replay.rs`, as `(snapshot, field)`.
fn read_committed_goldens() -> Result<(u64, u64), String> {
    let text = std::fs::read_to_string(REPLAY_PATH).map_err(|e| format!("{REPLAY_PATH}: {e}"))?;
    Ok((parse_hex(&extract_hex(&text, SNAP_MARKER)?)?, parse_hex(&extract_hex(&text, FIELD_MARKER)?)?))
}

/// Parse a `0x…` u64 literal, tolerating RON/Rust `_` digit separators (`0xe1ec_dc58_3c8d_bfca`).
fn parse_hex(lit: &str) -> Result<u64, String> {
    let cleaned: String = lit.trim().trim_start_matches("0x").chars().filter(|c| *c != '_').collect();
    u64::from_str_radix(&cleaned, 16).map_err(|e| format!("{REPLAY_PATH}: bad golden literal `{lit}`: {e}"))
}

/// Re-pin the replay goldens in `tests/replay.rs`.
///
/// **Scoped to the two `const` declarations, deliberately.** This used to `str::replace` the old hex across
/// the WHOLE file, which had two failure modes: (1) `replay.rs`'s header is an archaeology log that quotes
/// prior hashes in prose on purpose, and any that matched the current value were silently rewritten — the
/// tool ate its own audit trail; (2) it existed only to keep a duplicated literal in
/// `authored_world_config_override_is_a_noop` in step, and that duplicate is now a reference to `GOLDEN`,
/// so there is exactly one declaration site per golden. Rewriting anything beyond these two statements is
/// out of scope by construction.
fn repin_replay(snap: u64, field: u64) -> Result<(), String> {
    let text = std::fs::read_to_string(REPLAY_PATH).map_err(|e| format!("{REPLAY_PATH}: {e}"))?;
    let text = repin_one(&text, SNAP_MARKER, snap)?;
    let text = repin_one(&text, FIELD_MARKER, field)?;
    std::fs::write(REPLAY_PATH, text).map_err(|e| format!("{REPLAY_PATH}: write: {e}"))
}

/// Replace the literal in the single `<marker><hex>;` declaration, leaving every other byte untouched.
fn repin_one(text: &str, marker: &str, value: u64) -> Result<String, String> {
    let at = text.find(marker).ok_or_else(|| format!("{REPLAY_PATH}: no `{marker}`"))?;
    if text[at + marker.len()..].contains(marker) {
        // Two declarations of one golden is the duplication this function was built to stop needing.
        return Err(format!("{REPLAY_PATH}: `{marker}` declared more than once — a golden has one home"));
    }
    let val_start = at + marker.len();
    let end = text[val_start..]
        .find(';')
        .ok_or_else(|| format!("{REPLAY_PATH}: unterminated `{marker}`"))?;
    Ok(format!("{}0x{value:016x}{}", &text[..val_start], &text[val_start + end..]))
}

/// Extract the hex literal after a `const X: u64 = ` marker, up to the `;`.
fn extract_hex(text: &str, marker: &str) -> Result<String, String> {
    let at = text.find(marker).ok_or_else(|| format!("{REPLAY_PATH}: no `{marker}`"))?;
    let rest = &text[at + marker.len()..];
    let end = rest.find(';').ok_or_else(|| format!("{REPLAY_PATH}: unterminated `{marker}`"))?;
    Ok(rest[..end].trim().to_string())
}

/// Pretty RON so an elite is a reviewable diff, not one long line.
fn write_ron<T: serde::Serialize>(path: &str, value: &T) -> Result<(), String> {
    let text = ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("{path}: serialize: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("{path}: write: {e}"))
}

// ── clap value parsers ─────────────────────────────────────────────────────────────────────────────────

/// A seed literal: decimal, or `0x`-prefixed hex. Absent flags take documented defaults; a *malformed*
/// value is a loud error (the project's one-path rule).
fn parse_seed(v: &str) -> Result<u64, String> {
    let v = v.trim();
    match v.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16),
        None => v.parse::<u64>(),
    }
    .map_err(|e| format!("{v:?}: {e}"))
}

fn parse_pos_u32(v: &str) -> Result<u32, String> {
    let n = v.parse::<u32>().map_err(|e| format!("{v:?}: {e}"))?;
    if n == 0 {
        return Err(format!("{v:?} must be > 0"));
    }
    Ok(n)
}

fn parse_pos_usize(v: &str) -> Result<usize, String> {
    let n = v.parse::<usize>().map_err(|e| format!("{v:?}: {e}"))?;
    if n == 0 {
        return Err(format!("{v:?} must be > 0"));
    }
    Ok(n)
}

/// Time one headless episode per seed on the deterministic core, reporting build vs. step cost
/// separately. The split matters: `build_headless_app` pays dungeon WFC + furniture placement + the
/// headless GPU device *once per process*, and a multi-process search pays it once per evaluation, so
/// a large build cost changes the optimal episode length.
fn bench(ticks: u32, seeds: &[u64], speed: f32) {
    // Held for every App's lifetime — the harness admits one at a time (its documented invariant 4).
    let _serial = serial_guard();

    // `speed` is a throughput lever, not a physics change: `build_headless_app` advances REAL time by
    // `fixed_dt * speed` per `app.update()`, so `speed` FixedUpdate sub-steps run per render pass. The
    // fixed sub-step itself is always exactly `fixed_dt`. Since the per-tick cost measured here is flat
    // in episode length (it is per-*update* overhead, not per-entity), raising `speed` amortises it.
    //
    // TESTING.md is explicit that cross-speed exact equality is NOT asserted, so `bench` prints the
    // snapshot hash and the caller compares across speeds rather than assuming.
    let updates = (ticks as f32 / speed).ceil() as u32;
    let fixed_steps = updates as f32 * speed;

    println!("ticks/episode : {ticks}  ({:.1} s simulated @ 60 Hz)", ticks as f32 / 60.0);
    println!("speed         : {speed}  ({updates} updates -> {fixed_steps} fixed sub-steps)");
    println!("seeds         : {seeds:?}");
    println!();

    let mut total_build = 0.0f64;
    let mut total_step = 0.0f64;

    for &seed in seeds {
        let cfg = SimConfig { speed, ..SimConfig::deterministic_core_seeded(seed) };

        let t0 = Instant::now();
        let mut app = build_headless_app(&cfg);
        let build_s = t0.elapsed().as_secs_f64();

        let t1 = Instant::now();
        step(&mut app, &cfg, updates);
        let step_s = t1.elapsed().as_secs_f64();

        let hash = snapshot_hash(&mut app);
        let violations = liveness_violations(&mut app);

        total_build += build_s;
        total_step += step_s;

        println!(
            "seed 0x{seed:X}: build {build_s:6.2}s | step {step_s:6.2}s | \
             {:8.0} tick/s | hash {hash:016x} | {}",
            f64::from(ticks) / step_s,
            if violations.is_empty() {
                "live".to_string()
            } else {
                format!("{} VIOLATION(S): {}", violations.len(), violations.join("; "))
            }
        );
    }

    let n = seeds.len() as f64;
    let mean_episode = (total_build + total_step) / n;
    let tick_rate = (ticks as f64 * n) / total_step;

    println!();
    println!("mean build    : {:.2} s/episode", total_build / n);
    println!("mean step     : {:.2} s/episode", total_step / n);
    println!("mean episode  : {mean_episode:.2} s   (build + step)");
    println!("throughput    : {tick_rate:.0} tick/s/process");
    println!();

    // Budget projection. Wall-clock for a search is
    //   genomes × ROLLOUTS_PER_GENOME × mean_episode / processes.
    let cores = std::thread::available_parallelism().map(|c| c.get()).unwrap_or(1);
    // The harness pins each App to one compute thread, so a worker is ~1 core; leave 2 for the OS.
    let processes = cores.saturating_sub(2).max(1);
    let per_genome = mean_episode * f64::from(ROLLOUTS_PER_GENOME);

    println!("rollouts/genome : {ROLLOUTS_PER_GENOME}  (2 learnability × 3 opponents × 3 seeds)");
    println!("cost/genome     : {per_genome:.1} s  ({:.1} s wall on {processes} procs)", per_genome / processes as f64);
    for genomes in [1_000u32, 5_000, 20_000] {
        let wall_h = (f64::from(genomes) * per_genome) / processes as f64 / 3600.0;
        println!("  {genomes:>6} genomes → {wall_h:6.2} h wall");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A miniature of `config.ron`'s real shape: prose above a field, an inline trailing comment, a hex
    /// literal, a nested one-line struct, an `Option`, and a sequence of tuple structs.
    const BLOCK: &str = r#"config: (
    other: (
        keep: 1,
    ),
    dungeon: (
        coarse_w: 6,
        corridor_width: 2,          // minimum corridor width (block-centre lane + 1 more)
        seed: 0x5C09191,            // nods to SCP-9191, the slop generator

        // Liminality dial: 1.0 = sparse Backrooms boxes adrift in the void.
        liminality: 1.0,
        notch: Some((
            chance: 0.8,
        )),
        wfc_weights: (
            rock: 3.0, dead_end: 1.2,
        ),
        room_types: [
            ( tag: "bathroom", weight: 0.8 ),
            ( tag: "hall",     weight: 1.0 ),
        ],
    ),
)
"#;

    /// The `before` arm: `BLOCK`'s dungeon values as the serializer spells them — every field present,
    /// including the ones the authored text leaves at their serde default.
    const BEFORE: &str = r#"(coarse_w: 6, corridor_width: 2, seed: 96506257, liminality: 1.0,
        notch: Some((chance: 0.8)), wfc_weights: (rock: 3.0, dead_end: 1.2),
        room_types: [(tag: "bathroom", weight: 0.8), (tag: "hall", weight: 1.0)])"#;

    #[test]
    fn splice_preserves_comments_and_touches_only_changed_scalars() {
        // corridor_width 2 -> 3 and rock 3.0 -> 4.5; everything else identical.
        let baked = r#"(coarse_w: 6, corridor_width: 3, seed: 96506257, liminality: 1.0,
            notch: Some((chance: 0.8)), wfc_weights: (rock: 4.5, dead_end: 1.2),
            room_types: [(tag: "bathroom", weight: 0.8), (tag: "hall", weight: 1.0)])"#;
        let out = splice_block(BLOCK, "dungeon", BEFORE, baked).expect("splice");

        // The rationale survives, verbatim.
        assert!(out.contains("// minimum corridor width (block-centre lane + 1 more)"));
        assert!(out.contains("// nods to SCP-9191, the slop generator"));
        assert!(out.contains("// Liminality dial: 1.0 = sparse Backrooms boxes adrift in the void."));
        // The changed scalars moved.
        assert!(out.contains("corridor_width: 3,"), "corridor_width not updated:\n{out}");
        assert!(out.contains("rock: 4.5,"), "nested one-line struct not updated:\n{out}");
        // `seed` is semantically unchanged (0x5C09191 == 96506257), so its line is byte-identical — the hex
        // spelling and its alignment survive. This is the whole point of comparing values, not text.
        assert!(out.contains("seed: 0x5C09191,            // nods to SCP-9191"), "seed line was disturbed:\n{out}");
        // Untouched neighbours stay put.
        assert!(out.contains("coarse_w: 6,"));
        assert!(out.contains("keep: 1,"));
        assert!(out.contains("liminality: 1.0,"));
    }

    #[test]
    fn splice_refuses_a_dropped_sequence_entry() {
        // `--dim levels` dropping a room type: 2 room_types -> 1.
        let baked = r#"(coarse_w: 6, corridor_width: 2, seed: 96506257, liminality: 1.0,
            notch: Some((chance: 0.8)), wfc_weights: (rock: 3.0, dead_end: 1.2),
            room_types: [(tag: "hall", weight: 1.0)])"#;
        let err = splice_block(BLOCK, "dungeon", BEFORE, baked).expect_err("must refuse a shape change");
        assert!(err.contains("SHAPE"), "{err}");
        assert!(err.contains("FVS_LEVELS_ELITE"), "the error must name the one-path alternative: {err}");
    }

    #[test]
    fn splice_refuses_a_vanished_option() {
        // `notch: Some(( … ))` -> `None` removes the `notch.chance` leaf: a shape change, not a value change.
        let baked = r#"(coarse_w: 6, corridor_width: 2, seed: 96506257, liminality: 1.0,
            notch: None, wfc_weights: (rock: 3.0, dead_end: 1.2),
            room_types: [(tag: "bathroom", weight: 0.8), (tag: "hall", weight: 1.0)])"#;
        let err = splice_block(BLOCK, "dungeon", BEFORE, baked).expect_err("must refuse a vanished Option");
        assert!(err.contains("SHAPE"), "{err}");
    }

    /// The old paren scan counted raw `(`/`)` anywhere, including inside comments and strings — so a lone
    /// paren in prose mis-located the block's end and mangled the file.
    #[test]
    fn block_scan_ignores_parens_in_comments_and_strings() {
        let text = r#"root: (
    dungeon: (
        // a smiley :) and an unbalanced ( paren in prose
        tag: "a ) string with ( parens",
        n: 1,
    ),
    after: (
        untouched: 7,
    ),
)
"#;
        let before = r#"(tag: "a ) string with ( parens", n: 1)"#;
        let baked = r#"(tag: "a ) string with ( parens", n: 2)"#;
        let out = splice_block(text, "dungeon", before, baked).expect("splice past the decoy parens");
        assert!(out.contains("n: 2,"), "{out}");
        assert!(out.contains("// a smiley :) and an unbalanced ( paren in prose"));
        // The block ended where it should: the sibling below is intact.
        assert!(out.contains("untouched: 7,"), "block end mis-located:\n{out}");
    }

    #[test]
    fn scalar_eq_compares_values_not_spelling() {
        assert!(scalar_eq("0x5C09191", "96506257"), "hex and decimal are the same number");
        assert!(scalar_eq("1.0", "1.0"));
        assert!(!scalar_eq("1.0", "1.5"));
        assert!(!scalar_eq("2", "3"));
    }

    /// `repin_replay` must touch ONLY the const declaration — `replay.rs`'s header quotes prior hashes in
    /// prose deliberately, and the old unbounded `str::replace` rewrote those too.
    #[test]
    fn repin_one_leaves_prose_hashes_alone() {
        let src = "// Was `0xdeadbeefdeadbeef`. See the log.\nconst GOLDEN: u64 = 0xdeadbeefdeadbeef;\n";
        let out = repin_one(src, SNAP_MARKER, 0x1234_5678_9abc_def0).expect("repin");
        assert!(out.contains("// Was `0xdeadbeefdeadbeef`. See the log."), "prose was rewritten:\n{out}");
        assert!(out.contains("const GOLDEN: u64 = 0x123456789abcdef0;"), "{out}");
    }

    #[test]
    fn repin_one_rejects_a_duplicated_golden() {
        let src = "const GOLDEN: u64 = 0x1;\nconst GOLDEN: u64 = 0x2;\n";
        let err = repin_one(src, SNAP_MARKER, 9).expect_err("two homes for one golden must be an error");
        assert!(err.contains("more than once"), "{err}");
    }

    #[test]
    fn parse_hex_accepts_ron_digit_separators() {
        assert_eq!(parse_hex("0xe1ec_dc58_3c8d_bfca").expect("sep"), 0xe1ec_dc58_3c8d_bfca);
        assert_eq!(parse_hex("0x38d3c9107d4eed33").expect("plain"), 0x38d3c9107d4eed33);
        assert!(parse_hex("0xnope").is_err());
    }

    /// The real-file guard, and the strongest one: splice each shipped slice with a value decoded FROM the
    /// shipped config. Nothing changed, so `config.ron` must come back BYTE-IDENTICAL — every comment, every
    /// hex literal, every column of alignment. If the scanner mis-parses any real construct in the authored
    /// file, this reds. (The synthetic fixtures above pin the behaviour; this pins it against reality.)
    #[test]
    fn splicing_the_shipped_config_with_its_own_values_is_a_byte_identical_no_op() {
        let gc = foundation_vs_slop::config::load_game_config().expect("load the shipped config");
        let text = std::fs::read_to_string(CONFIG_PATH).expect("read config.ron");
        for (name, value) in [
            ("behavior", ron_slice(&gc.behavior).expect("ser behavior")),
            ("sim", ron_slice(&gc.sim).expect("ser sim")),
            ("ai_tuning", ron_slice(&gc.ai_tuning).expect("ser ai_tuning")),
            ("audio", ron_slice(&gc.audio).expect("ser audio")),
            ("dungeon", ron_slice(&gc.dungeon).expect("ser dungeon")),
            ("mycelia", ron_slice(&gc.mycelia).expect("ser mycelia")),
            ("metropolis", ron_slice(&gc.placement.metropolis).expect("ser metropolis")),
            ("density", ron_slice(&gc.placement.density).expect("ser density")),
            ("mold", ron_slice(&gc.mold).expect("ser mold")),
            // These two splice a SUBSET type (the evolvable dials only), so this also pins that a subset
            // whose field names are a flat subset of the authored block round-trips byte-identically.
            (
                "almond_water",
                ron_slice(&foundation_vs_slop::almond_water::AlmondWaterDynamics::from_config(
                    &gc.almond_water,
                ))
                .expect("ser almond dynamics"),
            ),
            (
                "lighting",
                ron_slice(&foundation_vs_slop::light::LightingDynamics::from_config(&gc.lighting))
                    .expect("ser lighting dynamics"),
            ),
        ] {
            let out = splice_block(&text, name, &value, &value)
                .unwrap_or_else(|e| panic!("splicing `{name}` with its own value must succeed: {e}"));
            assert_eq!(out, text, "splicing `{name}` with its own decoded value changed config.ron");
        }
    }
}
