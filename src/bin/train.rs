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
    world_archive_doc, SearchConfig, SearchResult, SquadGenome, SwarmGenome, Templates,
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
    /// Ticks per episode (1800 = 30 s @ 60 Hz).
    #[arg(long, default_value_t = 1800, value_parser = parse_pos_u32)]
    ticks: u32,
    /// Dungeon seeds (comma-separated; decimal or 0x-hex). Default: 0x5C09191.
    #[arg(long, value_delimiter = ',', value_parser = parse_seed)]
    seeds: Vec<u64>,
    /// Throughput lever: FixedUpdate sub-steps per render pass (does not change physics).
    #[arg(long, default_value_t = 1.0)]
    speed: f32,
}

#[derive(Args)]
struct ProbeArgs {
    /// Ticks per episode (1800 = 30 s @ 60 Hz).
    #[arg(long, default_value_t = 1800, value_parser = parse_pos_u32)]
    ticks: u32,
    /// Dungeon seeds (comma-separated; decimal or 0x-hex). Default: 0x5C09191.
    #[arg(long, value_delimiter = ',', value_parser = parse_seed)]
    seeds: Vec<u64>,
}

/// Shared flags for every search subcommand. Defaults are the PRACTICAL search settings the old `tune.sh`
/// applied (30 gens × 16 children, 1800 ticks) — NOT `SearchConfig::default()`'s tiny 8/4/7200 — so a bare
/// `train evolve3` / `./tune.sh evolve3` runs a useful search out of the box.
#[derive(Args, Clone)]
struct SearchArgs {
    /// Generations (POET: outer iterations).
    #[arg(long, default_value_t = 30, value_parser = parse_pos_u32)]
    generations: u32,
    /// Children proposed per generation (per side, for co-evolution).
    #[arg(long, default_value_t = 16, value_parser = parse_pos_u32)]
    batch: u32,
    /// Ticks per rollout episode.
    #[arg(long, default_value_t = 1800, value_parser = parse_pos_u32)]
    ticks: u32,
    /// MAP-Elites archive resolution (bins per descriptor axis).
    #[arg(long, default_value_t = 8, value_parser = parse_pos_usize)]
    res: usize,
    /// Search RNG seed (distinct search trajectories; `--islands` derives its own per island).
    #[arg(long, default_value_t = 0xC0FFEE, value_parser = parse_seed)]
    seed: u64,
    /// Held-in dungeon seeds the objective is evaluated on (comma-separated; decimal or 0x-hex). Needs >= 2.
    /// Default: 0x5C09191,0x1CE5,0xB0BA.
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
    /// Force plain per-generation lines instead of the live progress bar (for logs / CI).
    #[arg(long)]
    no_progress: bool,
    /// Internal: run as an island child — emit machine-readable PROGRESS/RESULT on stdout for the parent.
    #[arg(long, hide = true)]
    island_child: bool,
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
}

fn main() {
    // Silence the headless Bevy asset-not-found spam at the source (absorbs the old `tune.sh` RUST_LOG
    // export) unless the caller set their own filter.
    if std::env::var_os("RUST_LOG").is_none() {
        // SAFETY: single-threaded here — this runs at the very top of `main`, before any thread (the island
        // readers, the compute pool) is spawned, so there is no concurrent env access to race with.
        unsafe {
            std::env::set_var(
                "RUST_LOG",
                "warn,bevy_asset=off,bevy_render=off,bevy_gltf=off,bevy_gizmos=off,wgpu=off,naga=off",
            );
        }
    }

    let cli = Cli::parse();
    let result = match cli.command {
        Command::Bench(a) => {
            bench(a.ticks, &seeds_or(&a.seeds, &[0x5C09191]), a.speed);
            Ok(())
        }
        Command::Probe(a) => probe(a.ticks, &seeds_or(&a.seeds, &[0x5C09191])),
        Command::Prior(a) => prior(a.ticks, &seeds_or(&a.seeds, &[0x5C09191])),
        Command::Evolve(a) => run_search(SearchKind::Evolve, a),
        Command::Evolve3(a) => run_search(SearchKind::Evolve3, a),
        Command::Levels(a) => run_search(SearchKind::Levels, a),
        Command::Audio(a) => run_search(SearchKind::Audio, a),
        Command::Behavior(a) => run_search(SearchKind::Behavior, a),
        Command::Rl(a) => run_search(SearchKind::Rl, a),
        Command::Poet(a) => run_search(SearchKind::Poet, a),
        Command::Apply(a) => {
            Dim::parse(&a.dim).and_then(|dim| apply_archive(dim, &a.archive, a.cell.as_deref()))
        }
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
fn run_search(kind: SearchKind, mut a: SearchArgs) -> Result<(), String> {
    if a.seeds.is_empty() {
        a.seeds = vec![0x5C09191, 0x1CE5, 0xB0BA];
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
        let dim = kind.apply_dim()?;
        println!();
        println!(">> --apply: baking the {} elite {} into the shipped config…", kind.cli_name(), outcome.out.display());
        apply_archive(dim, &outcome.out, None)?;
        println!(">> regenerating the baseline prior for the new tuning…");
        prior(a.ticks, &a.seeds)?;
    }
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

    if a.apply {
        let dim = kind.apply_dim()?;
        println!(">> --apply: baking {win_out} into the shipped config…");
        apply_archive(dim, Path::new(&win_out), None)?;
        println!(">> regenerating the baseline prior for the new tuning…");
        prior(a.ticks, &a.seeds)?;
    } else if let Ok(dim) = kind.apply_dim() {
        println!("   ship it:  train apply {} {win_out}    (or re-run with --apply)", dim_cli_name(dim));
    }
    Ok(())
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
fn apply_archive(dim: Dim, archive: &Path, cell: Option<&str>) -> Result<(), String> {
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
    let mut gc = foundation_vs_slop::config::load_game_config()?;
    let desc = apply_dim(&mut gc, dim, &spec)?;
    println!("apply: {desc}");

    // 2 + 3. Splice the config.ron slice(s) and regenerate the guarded `Default` impl(s).
    let mut cfg_text = std::fs::read_to_string(CONFIG_PATH).map_err(|e| format!("{CONFIG_PATH}: {e}"))?;
    let mut touched: Vec<String> = vec![CONFIG_PATH.to_string()];
    match dim {
        Dim::Behavior => {
            cfg_text = splice_block(&cfg_text, "behavior", &ron_slice(&gc.behavior)?)?;
            regen_default("src/behavior_tuning.rs", "BehaviorTuning", &format!("{:#?}", gc.behavior))?;
            touched.push("src/behavior_tuning.rs".into());
        }
        Dim::World => {
            cfg_text = splice_block(&cfg_text, "sim", &ron_slice(&gc.sim)?)?;
            cfg_text = splice_block(&cfg_text, "ai_tuning", &ron_slice(&gc.ai_tuning)?)?;
            regen_default("src/sim.rs", "SimTuning", &format!("{:#?}", gc.sim))?;
            regen_default("src/ai/tuning.rs", "AiTuning", &format!("{:#?}", gc.ai_tuning))?;
            touched.push("src/sim.rs".into());
            touched.push("src/ai/tuning.rs".into());
        }
        Dim::Audio => {
            cfg_text = splice_block(&cfg_text, "audio", &ron_slice(&gc.audio)?)?;
            regen_default("src/audio_tuning.rs", "AudioTuning", &format!("{:#?}", gc.audio))?;
            touched.push("src/audio_tuning.rs".into());
        }
        Dim::Levels => {
            // These config types have no `Default` impl, so only config.ron is rewritten.
            cfg_text = splice_block(&cfg_text, "dungeon", &ron_slice(&gc.dungeon)?)?;
            cfg_text = splice_block(&cfg_text, "mycelia", &ron_slice(&gc.mycelia)?)?;
            cfg_text = splice_block(&cfg_text, "metropolis", &ron_slice(&gc.placement.metropolis)?)?;
            cfg_text = splice_block(&cfg_text, "density", &ron_slice(&gc.placement.density)?)?;
        }
    }
    std::fs::write(CONFIG_PATH, &cfg_text).map_err(|e| format!("{CONFIG_PATH}: write: {e}"))?;

    // 4. Recompute the goldens from the freshly-baked config and re-pin them. (Env overlays are guaranteed
    //    unset above, so `deterministic_core` reproduces exactly what the replay test will assert.) Go through
    //    the stability guard — a golden is only stamped if repeated builds agree, so a non-deterministic core
    //    reds here instead of silently pinning a one-off value.
    let (snap, field) = recompute_goldens_stable(GOLDEN_STABILITY_REPS)?;
    repin_replay(snap, field)?;
    touched.push(REPLAY_PATH.to_string());

    // 5 + 6. Report + next steps.
    println!();
    println!("baked. re-pinned goldens: snapshot 0x{snap:016x}, field 0x{field:016x}");
    println!("files changed:");
    for f in &touched {
        println!("  {f}");
    }
    println!();
    println!("NEXT: regenerate the baseline prior for the new shipped tuning:  train prior");
    println!("      review:  git diff        verify:  cargo test --features test-harness");
    println!("      revert:  git checkout -- {}", touched.join(" "));
    Ok(())
}

/// Serialize a config slice to RON in config.ron's anonymous-tuple style (`struct_names: false`).
fn ron_slice<T: serde::Serialize>(v: &T) -> Result<String, String> {
    ron::ser::to_string_pretty(v, ron::ser::PrettyConfig::default()).map_err(|e| format!("serialize slice: {e}"))
}

/// Replace the `<name>: ( … )` block in `config.ron` text with `<indent><name>: <value_ron>,`. The block is
/// found by a line whose trim equals `<name>: (` and paren-balanced to its close (works for top-level 4-space
/// slices and 8-space placement sub-slices alike; every target name is unique in the file).
fn splice_block(text: &str, name: &str, value_ron: &str) -> Result<String, String> {
    let lines: Vec<&str> = text.lines().collect();
    let header = format!("{name}: (");
    let start = lines
        .iter()
        .position(|l| l.trim() == header)
        .ok_or_else(|| format!("config.ron: no `{name}:` block header"))?;
    let indent: String = lines[start].chars().take_while(|c| *c == ' ').collect();
    let mut depth = 0i32;
    let mut end = None;
    for (idx, line) in lines.iter().enumerate().skip(start) {
        for c in line.chars() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
        }
        if depth == 0 {
            end = Some(idx);
            break;
        }
    }
    let end = end.ok_or_else(|| format!("config.ron: unbalanced `{name}:` block"))?;
    let mut out: Vec<String> = lines[..start].iter().map(|s| s.to_string()).collect();
    out.push(format!("{indent}{name}: {value_ron},"));
    out.extend(lines[end + 1..].iter().map(|s| s.to_string()));
    Ok(out.join("\n") + "\n")
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

/// Re-pin the replay goldens in `tests/replay.rs`: replace the old `GOLDEN` (snapshot) hex — which appears
/// twice (the const and the world-config no-op assertion) — and the `GOLDEN_FIELD` hex, everywhere they
/// occur, with the freshly recomputed values.
fn repin_replay(snap: u64, field: u64) -> Result<(), String> {
    let text = std::fs::read_to_string(REPLAY_PATH).map_err(|e| format!("{REPLAY_PATH}: {e}"))?;
    let old_snap = extract_hex(&text, "const GOLDEN: u64 = ")?;
    let old_field = extract_hex(&text, "const GOLDEN_FIELD: u64 = ")?;
    let text = text.replace(&old_snap, &format!("0x{snap:016x}"));
    let text = text.replace(&old_field, &format!("0x{field:016x}"));
    std::fs::write(REPLAY_PATH, text).map_err(|e| format!("{REPLAY_PATH}: write: {e}"))
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
