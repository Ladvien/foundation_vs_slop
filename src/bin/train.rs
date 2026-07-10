//! Offline behaviour-search driver (feature `test-harness`).
//!
//! The offline half of the experience system: it runs the real simulation headlessly, many times, to
//! search the space of squad/swarm brains. Nothing here ships in the game binary — the runtime only
//! ever *reads* the archive this produces.
//!
//! Right now it implements one subcommand, `bench`, which answers the question every later phase
//! depends on: **how many simulated ticks per second can one process actually deliver?** MAP-Elites
//! (Mouret & Clune, "Illuminating search spaces by mapping elites", arXiv:1504.04909) needs thousands
//! of evaluations, and `sim_harness` admits exactly one `App` per process (it holds a process-wide lock
//! and pins the global compute pool + rayon to one thread for determinism). So throughput is
//! `processes × ticks_per_second`, and the evaluation budget has to be derived from a measurement
//! rather than a guess.
//!
//! Subcommands:
//! ```text
//! train bench  [--ticks N] [--seeds A,B,C] [--speed S]   # throughput + determinism probe
//! train probe  [--ticks N] [--seeds A,B,C]               # one authored episode: outcome + criterion
//! train prior  [--ticks N] [--seeds A,B,C]               # sweep the shipped brain -> baseline_prior.ron
//! train evolve [--ticks N] [--seeds A,B,C] [--generations G] [--batch B] [--seed S] [--res R]
//! ```
//! `prior` must run before `evolve`: surprise is measured against the shipped brain's realised mode
//! distribution — what the player expects — and that reference is frozen for the whole search.

use std::time::Instant;

use foundation_vs_slop::sim_harness::{
    build_headless_app, liveness_violations, serial_guard, snapshot_hash, step, SimConfig,
};
use foundation_vs_slop::squad_ai::coevolve::{
    search, squad_archive_doc, swarm_archive_doc, sweep_prior, SearchConfig, Templates,
};

/// Where the frozen baseline expectation lives. Committed, and validated on load.
const PRIOR_PATH: &str = "assets/config/baseline_prior.ron";
/// Where the illuminated archives land, for a human to read before anything ships.
const SQUAD_ARCHIVE_PATH: &str = "assets/config/elites_squad.ron";
const SWARM_ARCHIVE_PATH: &str = "assets/config/elites_swarm.ron";

/// Rollouts consumed by one genome evaluation in the planned search, used to project the budget:
/// 2 rollouts (the learnability pair — a mode-transition model fitted on rollout A must predict
/// rollout B; Schmidhuber, "Driven by Compression Progress", arXiv:0812.4360) × 3 sampled opponents
/// (sampling across the opponent archive rather than only its incumbent avoids the coevolutionary
/// "mediocre stable states" of Ficici & Pollack; cf. Wang et al., POET, arXiv:1901.01753) × 3 held-in
/// dungeon seeds.
const ROLLOUTS_PER_GENOME: u32 = 2 * 3 * 3;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprintln!("usage: train bench [--ticks N] [--seeds A,B,C] [--speed S]");
        std::process::exit(2);
    };

    let result = match cmd.as_str() {
        "bench" => parse_bench(&args[1..]).map(|(ticks, seeds, speed)| bench(ticks, &seeds, speed)),
        "prior" => parse_bench(&args[1..]).and_then(|(ticks, seeds, _)| prior(ticks, &seeds)),
        "evolve" => parse_evolve(&args[1..]).and_then(evolve),
        "probe" => parse_bench(&args[1..]).and_then(|(ticks, seeds, _)| probe(ticks, &seeds)),
        other => Err(format!("unknown subcommand {other:?} (expected bench | probe | prior | evolve)")),
    };
    if let Err(e) = result {
        eprintln!("train {cmd}: {e}");
        std::process::exit(1);
    }
}

/// Run the authored brains once per seed and print the raw episode outcome + the fitness factors.
///
/// This exists so the behavioural minimal criterion's thresholds are **calibrated from measurement**
/// rather than guessed. The first guess (10% map coverage) was unreachable: `reachable_cells` counts fine
/// floor tiles (metres), of which a dungeon has thousands, while a squad on a 30 s tour walks a few
/// hundred at most.
fn probe(ticks: u32, seeds: &[u64]) -> Result<(), String> {
    use foundation_vs_slop::squad_ai::coevolve::{brains_of, squad_descriptor, swarm_descriptor, SquadGenome, SwarmGenome};
    use foundation_vs_slop::squad_ai::evaluate::rollout;
    use foundation_vs_slop::squad_ai::surprise::{minimal_criterion, witnessed_fraction};

    let t = Templates::authored();
    let squad = SquadGenome::authored(&t);
    let swarm = SwarmGenome::authored(&t);

    for &seed in seeds {
        let r = rollout(brains_of(&t, &squad, &swarm)?, seed, ticks);
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
        println!("  swarm          : {} killed, {} alive", o.crabs_killed, o.crabs_alive);
        println!("  coverage       : {} / {} cells = {:.2}%", o.cells_covered, o.reachable_cells, 100.0 * coverage);
        println!("  liveness       : {} violation(s)", o.liveness_violations);
        println!("  field          : peak {:.2}, flatness {:.1}% (field-sanity gate calibration)", o.peak_field, 100.0 * o.field_flatness);
        println!("  squad descr    : aggression {:.3}, exploration {:.3}", squad_descriptor(&r.trace, o).aggression, squad_descriptor(&r.trace, o).exploration);
        println!("  swarm descr    : aggression {:.3}, persistence {:.3}", swarm_descriptor(&r.trace).aggression, swarm_descriptor(&r.trace).exploration);
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
        }
        match minimal_criterion(o) {
            Ok(()) => println!("  criterion      : PASS"),
            Err(why) => println!("  criterion      : FAIL — {why}"),
        }
        println!();
    }
    Ok(())
}

/// Sweep the shipped brain and commit the baseline prior. Every later `evolve` reads it.
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

struct EvolveArgs {
    cfg: SearchConfig,
}

fn parse_evolve(args: &[String]) -> Result<EvolveArgs, String> {
    let mut cfg = SearchConfig::default();
    let mut i = 0;
    while i < args.len() {
        let value = || args.get(i + 1).ok_or_else(|| format!("{} needs a value", args[i]));
        match args[i].as_str() {
            "--ticks" => cfg.episode_ticks = parse_u32(value()?)?,
            "--generations" => cfg.generations = parse_u32(value()?)?,
            "--batch" => cfg.batch = parse_u32(value()?)?,
            "--res" => cfg.resolution = parse_u32(value()?)? as usize,
            "--seed" => cfg.seed = parse_u64(value()?)?,
            "--seeds" => cfg.dungeon_seeds = parse_seeds(value()?)?,
            other => return Err(format!("unknown flag {other:?}")),
        }
        i += 2;
    }
    if cfg.dungeon_seeds.len() < 2 {
        return Err(
            "evolve needs >= 2 dungeon seeds: the two rollouts of a candidate must run on DIFFERENT \
             worlds, or learnability measures a memorised map rather than a behaviour"
                .into(),
        );
    }
    if cfg.resolution == 0 {
        return Err("--res must be > 0".into());
    }
    Ok(EvolveArgs { cfg })
}

fn evolve(args: EvolveArgs) -> Result<(), String> {
    let cfg = args.cfg;
    let templates = Templates::authored();
    let src = std::fs::read_to_string(PRIOR_PATH)
        .map_err(|e| format!("{PRIOR_PATH}: {e} — run `train prior` first"))?;
    let prior = ron::from_str(&src).map_err(|e| format!("{PRIOR_PATH}: malformed: {e}"))?;

    println!(
        "co-evolving {} generations x {} children/side, {} ticks/episode, worlds {:?}, seed 0x{:X}",
        cfg.generations, cfg.batch, cfg.episode_ticks, cfg.dungeon_seeds, cfg.seed
    );

    let result = search(&templates, &prior, &cfg, |generation, r| {
        println!(
            "  gen {generation:>3}: squad {:>3} niches (qd {:.3}) | swarm {:>3} niches (qd {:.3}) | \
             {} evals, {} infeasible, {} failed the criterion",
            r.squad.archive.coverage(),
            r.squad.archive.qd_score(),
            r.swarm.archive.coverage(),
            r.swarm.archive.qd_score(),
            r.evaluations,
            r.rejected_infeasible,
            r.rejected_by_criterion
        );
    })?;

    write_ron(SQUAD_ARCHIVE_PATH, &squad_archive_doc(&templates, &result.squad)?)?;
    write_ron(SWARM_ARCHIVE_PATH, &swarm_archive_doc(&templates, &result.swarm)?)?;
    println!();
    println!("wrote {SQUAD_ARCHIVE_PATH} ({} elites)", result.squad.archive.coverage());
    println!("wrote {SWARM_ARCHIVE_PATH} ({} elites)", result.swarm.archive.coverage());
    println!();
    println!("READ THE ELITES BEFORE SHIPPING THEM. They are RON in the same shape you author by hand;");
    println!("that readability is the reward-hacking guard, and it only works if someone looks.");
    Ok(())
}

/// Pretty RON so an elite is a reviewable diff, not one long line.
fn write_ron<T: serde::Serialize>(path: &str, value: &T) -> Result<(), String> {
    let text = ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("{path}: serialize: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("{path}: write: {e}"))
}

fn parse_u32(v: &str) -> Result<u32, String> {
    let n = v.parse::<u32>().map_err(|e| format!("{v:?}: {e}"))?;
    if n == 0 {
        return Err(format!("{v:?} must be > 0"));
    }
    Ok(n)
}

fn parse_u64(v: &str) -> Result<u64, String> {
    match v.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16),
        None => v.parse::<u64>(),
    }
    .map_err(|e| format!("{v:?}: {e}"))
}

fn parse_seeds(v: &str) -> Result<Vec<u64>, String> {
    let seeds = v.split(',').map(|s| parse_u64(s.trim())).collect::<Result<Vec<u64>, String>>()?;
    if seeds.is_empty() {
        return Err("at least one seed".into());
    }
    Ok(seeds)
}

/// Parse `--ticks N`, `--seeds A,B,C`, and `--speed S`. Every failure is an `Err` the caller reports
/// and exits on — no silent defaulting of a malformed value (the project's one-path rule); an *absent*
/// flag takes the documented default, which is a different thing from a malformed one.
fn parse_bench(args: &[String]) -> Result<(u32, Vec<u64>, f32), String> {
    // 1800 ticks = 30 s of simulated time at the pinned 60 Hz `FixedUpdate`.
    let mut ticks: u32 = 1800;
    let mut seeds: Vec<u64> = vec![0x5C09191];
    let mut speed: f32 = 1.0;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--speed" => {
                let v = args.get(i + 1).ok_or("--speed needs a value")?;
                speed = v.parse::<f32>().map_err(|e| format!("--speed {v:?}: {e}"))?;
                if !(speed.is_finite() && speed > 0.0) {
                    return Err("--speed must be finite and > 0".into());
                }
                i += 2;
            }
            "--ticks" => {
                let v = args.get(i + 1).ok_or("--ticks needs a value")?;
                ticks = v.parse::<u32>().map_err(|e| format!("--ticks {v:?}: {e}"))?;
                if ticks == 0 {
                    return Err("--ticks must be > 0".into());
                }
                i += 2;
            }
            "--seeds" => {
                let v = args.get(i + 1).ok_or("--seeds needs a comma-separated list")?;
                seeds = v
                    .split(',')
                    .map(|s| {
                        let s = s.trim();
                        let hex = s.strip_prefix("0x");
                        match hex {
                            Some(h) => u64::from_str_radix(h, 16),
                            None => s.parse::<u64>(),
                        }
                        .map_err(|e| format!("--seeds {s:?}: {e}"))
                    })
                    .collect::<Result<Vec<u64>, String>>()?;
                if seeds.is_empty() {
                    return Err("--seeds must name at least one seed".into());
                }
                i += 2;
            }
            other => return Err(format!("unknown flag {other:?}")),
        }
    }
    Ok((ticks, seeds, speed))
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
