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
        "evolve3" => parse_evolve(&args[1..]).and_then(evolve3),
        // Standalone level search: evolve dungeon/furniture/mushroom config under the static
        // level-quality objective. GPU-free, no `prior` needed (the objective isn't behavioural).
        "levels" => parse_evolve(&args[1..]).and_then(levels),
        // Standalone audio search: evolve the acoustic-stimulus config under the SAME behavioural
        // witnessed-surprise objective as the world population (audio feeds agent perception). Needs the
        // `prior` (`train prior` first) — regenerate it after any Mode change (MODE_COUNT is now 25).
        "audio" => parse_evolve(&args[1..]).and_then(audio),
        // Standalone behaviour search: evolve a curated subset of the `behavior:` config (locomotion,
        // steering, senses, combat cadence, boids) under the SAME behavioural witnessed-surprise objective.
        // Needs the `prior` (`train prior` first) — regenerate it after any Mode change.
        "behavior" => parse_evolve(&args[1..]).and_then(behavior),
        // Standalone policy (neuroevolution) search: evolve a `NeuralPolicy`'s weights under the SAME
        // behavioural witnessed-surprise objective. Needs the `prior` (`train prior` first).
        "rl" => parse_evolve(&args[1..]).and_then(rl),
        // POET open-ended outer loop: co-evolve worlds + the squads that solve them, with a learning-progress
        // curriculum and cross-niche transfer. Needs the `prior` (`train prior` first).
        "poet" => parse_evolve(&args[1..]).and_then(poet),
        "probe" => parse_bench(&args[1..]).and_then(|(ticks, seeds, _)| probe(ticks, &seeds)),
        // Internal: a rollout-evaluation worker for `--jobs N`. Spawned by the search's `WorkerPool`, never
        // run by hand — it speaks a length-prefixed RON protocol on stdin/stdout, not a human CLI.
        "worker" => foundation_vs_slop::squad_ai::parallel::worker_main(),
        other => Err(format!(
            "unknown subcommand {other:?} (expected bench | probe | prior | evolve | evolve3 | levels | audio | behavior | rl | poet)"
        )),
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
    use foundation_vs_slop::squad_ai::coevolve::{brains_of, squad_descriptor, swarm_descriptor, world_descriptor, SquadGenome, SwarmGenome};
    use foundation_vs_slop::squad_ai::evaluate::rollout_with_belief;
    use foundation_vs_slop::squad_ai::surprise::{minimal_criterion, witnessed_fraction};

    let t = Templates::authored();
    let squad = SquadGenome::authored(&t);
    let swarm = SwarmGenome::authored(&t);

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
    /// Use the CMA-ME adaptive emitter (valueless `--cma` flag; only honoured by the `rl` search today).
    cma: bool,
}

fn parse_evolve(args: &[String]) -> Result<EvolveArgs, String> {
    let mut cfg = SearchConfig::default();
    let mut cma = false;
    let mut i = 0;
    while i < args.len() {
        // Valueless boolean flag — handled before the value-based flags so it doesn't consume the next arg.
        if args[i] == "--cma" {
            cma = true;
            i += 1;
            continue;
        }
        let value = || args.get(i + 1).ok_or_else(|| format!("{} needs a value", args[i]));
        match args[i].as_str() {
            "--ticks" => cfg.episode_ticks = parse_u32(value()?)?,
            "--generations" => cfg.generations = parse_u32(value()?)?,
            "--batch" => cfg.batch = parse_u32(value()?)?,
            "--res" => cfg.resolution = parse_u32(value()?)? as usize,
            "--seed" => cfg.seed = parse_u64(value()?)?,
            "--seeds" => cfg.dungeon_seeds = parse_seeds(value()?)?,
            // Worker processes for parallel rollout evaluation. `1` (default) runs inline. The archives are
            // byte-identical regardless — `--jobs` only trades CPU for wall-clock, capped at OPPONENTS (3).
            "--jobs" => cfg.jobs = parse_u32(value()?)? as usize,
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
    Ok(EvolveArgs { cfg, cma })
}

/// Run the three-way co-evolution (squad × swarm × world) and return the templates + filled archives.
/// Both `evolve` and `evolve3` delegate here; they differ only in which archives they commit — the world
/// population co-evolves either way (it is what makes the squad/swarm do things a player has not seen).
fn run_coevolution(cfg: SearchConfig) -> Result<(Templates, SearchResult), String> {
    let templates = Templates::authored();
    let src = std::fs::read_to_string(PRIOR_PATH)
        .map_err(|e| format!("{PRIOR_PATH}: {e} — run `train prior` first"))?;
    let prior = ron::from_str(&src).map_err(|e| format!("{PRIOR_PATH}: malformed: {e}"))?;

    println!(
        "co-evolving {} generations x {} children/side, {} ticks/episode, worlds {:?}, seed 0x{:X}, {} worker(s)",
        cfg.generations, cfg.batch, cfg.episode_ticks, cfg.dungeon_seeds, cfg.seed, cfg.jobs.max(1)
    );

    let result = search(&templates, &prior, &cfg, |generation, r| {
        println!(
            "  gen {generation:>3}: squad {:>3} (qd {:.3}) | swarm {:>3} (qd {:.3}) | world {:>3} (qd {:.3}) \
             | {} evals, {} infeasible, {} failed the criterion",
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
        // Checkpoint every generation. `evolve3` otherwise commits the archives only at the very end, so
        // any interruption of the multi-hour run (e.g. macOS jetsam under memory pressure) discards all of
        // it. Writing each generation keeps the latest completed generation always on disk.
        if let Err(e) = checkpoint_archives(&templates, r) {
            eprintln!("  (checkpoint write failed: {e})");
        }
    })?;
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
fn evolve(args: EvolveArgs) -> Result<(), String> {
    let (templates, result) = run_coevolution(args.cfg)?;
    write_ron(SQUAD_ARCHIVE_PATH, &squad_archive_doc(&templates, &result.squad)?)?;
    write_ron(SWARM_ARCHIVE_PATH, &swarm_archive_doc(&templates, &result.swarm)?)?;
    println!();
    println!("wrote {SQUAD_ARCHIVE_PATH} ({} elites)", result.squad.archive.coverage());
    println!("wrote {SWARM_ARCHIVE_PATH} ({} elites)", result.swarm.archive.coverage());
    print_read_warning();
    Ok(())
}

/// Three-population run: co-evolve and commit all three archives, including the evolved worlds.
fn evolve3(args: EvolveArgs) -> Result<(), String> {
    let (templates, result) = run_coevolution(args.cfg)?;
    write_ron(SQUAD_ARCHIVE_PATH, &squad_archive_doc(&templates, &result.squad)?)?;
    write_ron(SWARM_ARCHIVE_PATH, &swarm_archive_doc(&templates, &result.swarm)?)?;
    write_ron(WORLD_ARCHIVE_PATH, &world_archive_doc(&result.world)?)?;
    println!();
    println!("wrote {SQUAD_ARCHIVE_PATH} ({} elites)", result.squad.archive.coverage());
    println!("wrote {SWARM_ARCHIVE_PATH} ({} elites)", result.swarm.archive.coverage());
    println!("wrote {WORLD_ARCHIVE_PATH} ({} elites)", result.world.archive.coverage());
    print_read_warning();
    Ok(())
}

/// Standalone level search: evolve dungeon architecture + furniture amount + mushroom amount under the
/// static level-quality objective, and commit the illuminated archive. GPU-free and fast (each genome is
/// generate-and-measure, not a rollout), so it needs no `prior` and no `--jobs`. Reuses the `--generations
/// / --batch / --res / --seed / --seeds` flags; `--seeds` are the held-in dungeon seeds each level is
/// scored across (and must clear the criterion on all of them).
fn levels(args: EvolveArgs) -> Result<(), String> {
    let (base, manifest) = level_eval::load_base()?;
    let c = args.cfg;
    let cfg = LevelSearchConfig {
        seed: c.seed,
        generations: c.generations,
        batch: c.batch,
        sigma: 0.3,
        resolution: c.resolution,
        dungeon_seeds: c.dungeon_seeds,
    };
    println!(
        "evolving levels: {} generations x {} children, res {}, held-in worlds {:?}, seed 0x{:X}, sigma {}",
        cfg.generations, cfg.batch, cfg.resolution, cfg.dungeon_seeds, cfg.seed, cfg.sigma
    );

    let result = level_search::search(&base, &manifest, &cfg, |generation, r| {
        let best = r.pop.archive.best().map_or(0.0, |e| e.fitness);
        println!(
            "  gen {generation:>3}: levels {:>3} (qd {:.3}, best {:.3}) | {} evals, {} infeasible, {} failed the criterion",
            r.pop.archive.coverage(),
            r.pop.archive.qd_score(),
            best,
            r.evaluations,
            r.rejected_infeasible,
            r.rejected_by_criterion
        );
    })?;

    write_ron(LEVELS_ARCHIVE_PATH, &level_search::level_archive_doc(&result.pop, &base)?)?;
    println!();
    println!("wrote {LEVELS_ARCHIVE_PATH} ({} elites)", result.pop.archive.coverage());
    print_read_warning();
    Ok(())
}

/// Standalone audio search: evolve the acoustic-stimulus config (channel propagation + per-event loudness
/// + per-faction perception gains) under the witnessed-learnable-surprise objective, and commit the
/// illuminated archive. Unlike `levels`, its fitness is a full-sim rollout (sound feeds agent perception),
/// so it needs the frozen `prior` — run `train prior` first, and REGENERATE it after any `Mode` change
/// (this branch added `Mode::Investigate`, so `MODE_COUNT` is 25). Reuses the `--generations / --batch /
/// --res / --seed / --seeds / --ticks` flags; `--seeds` are the held-in worlds (the learnability pair uses
/// the first two, which must differ).
fn audio(args: EvolveArgs) -> Result<(), String> {
    let src = std::fs::read_to_string(PRIOR_PATH)
        .map_err(|e| format!("{PRIOR_PATH}: {e} — run `train prior` first"))?;
    let prior = ron::from_str(&src).map_err(|e| format!("{PRIOR_PATH}: malformed: {e}"))?;

    let c = args.cfg;
    let cfg = AudioSearchConfig {
        seed: c.seed,
        generations: c.generations,
        batch: c.batch,
        sigma: 0.3,
        resolution: c.resolution,
        dungeon_seeds: c.dungeon_seeds,
        episode_ticks: c.episode_ticks,
    };
    println!(
        "evolving audio: {} generations x {} children, res {}, held-in worlds {:?}, seed 0x{:X}, {} ticks/episode",
        cfg.generations, cfg.batch, cfg.resolution, cfg.dungeon_seeds, cfg.seed, cfg.episode_ticks
    );

    let result = audio_search::search(&prior, &cfg, |generation, r| {
        let best = r.pop.archive.best().map_or(0.0, |e| e.fitness);
        println!(
            "  gen {generation:>3}: audio {:>3} (qd {:.3}, best {:.3}) | {} evals, {} infeasible, {} failed the criterion",
            r.pop.archive.coverage(),
            r.pop.archive.qd_score(),
            best,
            r.evaluations,
            r.rejected_infeasible,
            r.rejected_by_criterion
        );
    })?;

    write_ron(AUDIO_ARCHIVE_PATH, &audio_search::audio_archive_doc(&result.pop)?)?;
    println!();
    println!("wrote {AUDIO_ARCHIVE_PATH} ({} elites)", result.pop.archive.coverage());
    print_read_warning();
    Ok(())
}

/// Standalone behaviour search: evolve a curated subset of the `behavior:` config (locomotion, steering,
/// senses, combat cadence, boids) under the witnessed-learnable-surprise objective, and commit the
/// illuminated archive. Like `audio`, its fitness is a full-sim rollout, so it needs the frozen `prior` —
/// run `train prior` first, and REGENERATE it after any `Mode` change. Reuses the `--generations / --batch
/// / --res / --seed / --seeds / --ticks` flags; `--seeds` are the held-in worlds (the learnability pair
/// uses the first two, which must differ). Elites overlay onto the shipped base, so an archive cell is a
/// readable diff of behaviour dials to transcribe into `config.ron`.
fn behavior(args: EvolveArgs) -> Result<(), String> {
    let src = std::fs::read_to_string(PRIOR_PATH)
        .map_err(|e| format!("{PRIOR_PATH}: {e} — run `train prior` first"))?;
    let prior = ron::from_str(&src).map_err(|e| format!("{PRIOR_PATH}: malformed: {e}"))?;

    let c = args.cfg;
    let cfg = BehaviorSearchConfig {
        seed: c.seed,
        generations: c.generations,
        batch: c.batch,
        sigma: 0.3,
        resolution: c.resolution,
        dungeon_seeds: c.dungeon_seeds,
        episode_ticks: c.episode_ticks,
    };
    println!(
        "evolving behaviour: {} generations x {} children, res {}, held-in worlds {:?}, seed 0x{:X}, {} ticks/episode",
        cfg.generations, cfg.batch, cfg.resolution, cfg.dungeon_seeds, cfg.seed, cfg.episode_ticks
    );

    let result = behavior_search::search(&prior, &cfg, |generation, r| {
        let best = r.pop.archive.best().map_or(0.0, |e| e.fitness);
        println!(
            "  gen {generation:>3}: behaviour {:>3} (qd {:.3}, best {:.3}) | {} evals, {} infeasible, {} failed the criterion",
            r.pop.archive.coverage(),
            r.pop.archive.qd_score(),
            best,
            r.evaluations,
            r.rejected_infeasible,
            r.rejected_by_criterion
        );
    })?;

    write_ron(BEHAVIOR_ARCHIVE_PATH, &behavior_search::behavior_archive_doc(&result.pop)?)?;
    println!();
    println!("wrote {BEHAVIOR_ARCHIVE_PATH} ({} elites)", result.pop.archive.coverage());
    print_read_warning();
    Ok(())
}

/// Standalone policy (neuroevolution) search: evolve a learned `NeuralPolicy`'s MLP weights under the
/// witnessed-learnable-surprise objective, and commit the archive. Like `behavior`/`audio`, its fitness is a
/// full-sim rollout, so it needs the frozen `prior` — run `train prior` first, and REGENERATE it after any
/// `Mode` change. Reuses the `--generations / --batch / --res / --seed / --seeds / --ticks` flags; `--seeds`
/// are the held-in worlds (the learnability pair uses the first two, which must differ). Unlike the config
/// searches, an elite is an OPAQUE weight vector — the guard is the minimal criterion + watching it play,
/// not a readable diff.
fn rl(args: EvolveArgs) -> Result<(), String> {
    let src = std::fs::read_to_string(PRIOR_PATH)
        .map_err(|e| format!("{PRIOR_PATH}: {e} — run `train prior` first"))?;
    let prior = ron::from_str(&src).map_err(|e| format!("{PRIOR_PATH}: malformed: {e}"))?;

    let use_cma = args.cma;
    let c = args.cfg;
    let cfg = RlSearchConfig {
        seed: c.seed,
        generations: c.generations,
        batch: c.batch,
        sigma: 0.3,
        resolution: c.resolution,
        dungeon_seeds: c.dungeon_seeds,
        episode_ticks: c.episode_ticks,
        use_cma,
    };
    println!(
        "evolving policy ({}): {} generations x {} children, res {}, held-in worlds {:?}, seed 0x{:X}, {} ticks/episode",
        if use_cma { "CMA-ME emitter" } else { "neuroevolution, isotropic" },
        cfg.generations, cfg.batch, cfg.resolution, cfg.dungeon_seeds, cfg.seed, cfg.episode_ticks
    );

    let result = rl_search::search(&prior, &cfg, |generation, r| {
        let best = r.pop.archive.best().map_or(0.0, |e| e.fitness);
        println!(
            "  gen {generation:>3}: policy {:>3} (qd {:.3}, best {:.3}) | {} evals, {} infeasible, {} failed the criterion",
            r.pop.archive.coverage(),
            r.pop.archive.qd_score(),
            best,
            r.evaluations,
            r.rejected_infeasible,
            r.rejected_by_criterion
        );
    })?;

    write_ron(RL_ARCHIVE_PATH, &rl_search::rl_archive_doc(&result.pop)?)?;
    println!();
    println!("wrote {RL_ARCHIVE_PATH} ({} elites)", result.pop.archive.coverage());
    print_read_warning();
    Ok(())
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

/// POET (Wang et al. 2019): the open-ended outer loop. It co-generates *worlds* (the world-dynamics genome)
/// and the *squads* that solve them, admitting a new world only when it sits in the "neither too easy nor
/// too hard" band for the current best squad (the minimal criterion), transferring squads between worlds,
/// and steering optimisation budget by learning progress. Each pairing is scored by a real rollout, so it
/// needs the frozen `prior` (`train prior` first). Reuses `--generations` (→ POET iterations) / `--seed` /
/// `--seeds` (the held-in learnability pair) / `--ticks`.
fn poet(args: EvolveArgs) -> Result<(), String> {
    let src = std::fs::read_to_string(PRIOR_PATH)
        .map_err(|e| format!("{PRIOR_PATH}: {e} — run `train prior` first"))?;
    let prior: ModePrior = ron::from_str(&src).map_err(|e| format!("{PRIOR_PATH}: malformed: {e}"))?;

    let c = args.cfg;
    if c.dungeon_seeds.len() < 2 {
        return Err("poet needs >= 2 dungeon seeds: the learnability pair must run on DIFFERENT worlds".into());
    }
    let t = Templates::authored();
    // POET evolves worlds × squads against the shipped (authored) swarm — one moving opponent at a time.
    let swarm = SwarmGenome::authored(&t);
    let seeds = c.dungeon_seeds.clone();
    let ticks = c.episode_ticks;
    let poet_cfg = PoetConfig { seed: c.seed, iterations: c.generations, ..PoetConfig::default() };

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
            println!(
                "  it {it:>3}: {} niches | {} created, {} rejected, {} transfers | {} evals | best fit {:.3}, peak interest {:.3}",
                r.niches.len(), r.created, r.rejected, r.transfers, r.evaluations, best_fit, peak_interest
            );
        },
    )?;

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
    write_ron(POET_ARCHIVE_PATH, &doc)?;
    println!();
    println!(
        "wrote {POET_ARCHIVE_PATH} ({} niches, {} created, {} transfers over the run)",
        result.niches.len(),
        result.created,
        result.transfers
    );
    print_read_warning();
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
