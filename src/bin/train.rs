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
        // Permanently bake an evolved elite into the shipped defaults: rewrites config.ron + the matching
        // Rust `Default` impl + the replay goldens together, so `cargo test` stays green.
        "apply" => apply(&args[1..]),
        "probe" => parse_bench(&args[1..]).and_then(|(ticks, seeds, _)| probe(ticks, &seeds)),
        // Determinism stability guard: recompute the deterministic-core goldens `--reps` times (fresh App
        // each) and require agreement. Run across several fresh processes (under load) to catch the rare
        // cross-process flake before re-baking. See `recompute_goldens_stable`.
        "verify" => parse_verify(&args[1..]).and_then(verify),
        // Internal: a rollout-evaluation worker for `--jobs N`. Spawned by the search's `WorkerPool`, never
        // run by hand — it speaks a length-prefixed RON protocol on stdin/stdout, not a human CLI.
        "worker" => foundation_vs_slop::squad_ai::parallel::worker_main(),
        other => Err(format!(
            "unknown subcommand {other:?} (expected bench | probe | verify | prior | evolve | evolve3 | levels | audio | behavior | rl | poet | apply)"
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
    /// Override the output archive path (`--out <path>`). Lets many same-type searches (distinct `--seed`)
    /// run concurrently without clobbering each other's `elites_*.ron`. Honoured by the single-output
    /// searches (`levels`/`audio`/`behavior`/`rl`/`poet`); `evolve`/`evolve3` write three fixed files.
    out: Option<String>,
}

fn parse_evolve(args: &[String]) -> Result<EvolveArgs, String> {
    let mut cfg = SearchConfig::default();
    let mut cma = false;
    let mut out: Option<String> = None;
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
            "--out" => out = Some(value()?.clone()),
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
    Ok(EvolveArgs { cfg, cma, out })
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

    let path = args.out.as_deref().unwrap_or(LEVELS_ARCHIVE_PATH);
    write_ron(path, &level_search::level_archive_doc(&result.pop, &base)?)?;
    println!();
    println!("wrote {path} ({} elites)", result.pop.archive.coverage());
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

    let path = args.out.as_deref().unwrap_or(AUDIO_ARCHIVE_PATH);
    write_ron(path, &audio_search::audio_archive_doc(&result.pop)?)?;
    println!();
    println!("wrote {path} ({} elites)", result.pop.archive.coverage());
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

    let path = args.out.as_deref().unwrap_or(BEHAVIOR_ARCHIVE_PATH);
    write_ron(path, &behavior_search::behavior_archive_doc(&result.pop)?)?;
    println!();
    println!("wrote {path} ({} elites)", result.pop.archive.coverage());
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

    let path = args.out.as_deref().unwrap_or(RL_ARCHIVE_PATH);
    write_ron(path, &rl_search::rl_archive_doc(&result.pop)?)?;
    println!();
    println!("wrote {path} ({} elites)", result.pop.archive.coverage());
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
    let path = args.out.as_deref().unwrap_or(POET_ARCHIVE_PATH);
    write_ron(path, &doc)?;
    println!();
    println!(
        "wrote {path} ({} niches, {} created, {} transfers over the run)",
        result.niches.len(),
        result.created,
        result.transfers
    );
    print_read_warning();
    Ok(())
}

// ── `train apply`: permanently bake an evolved elite into the shipped defaults ──────────────────────

const CONFIG_PATH: &str = "assets/config/config.ron";
const REPLAY_PATH: &str = "tests/replay.rs";

/// Permanently ship an evolved elite: rewrite the `config.ron` slice(s), the matching Rust `Default`
/// impl(s), and the deterministic-replay goldens together, so `cargo test` stays green. Full-auto, one
/// command. `policy` is intentionally unsupported (a `NeuralPolicy` has no config slice — use
/// `FVS_POLICY_ELITE` to run one).
fn apply(args: &[String]) -> Result<(), String> {
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

    let dim_s = args
        .first()
        .ok_or("usage: train apply <behavior|world|audio|levels> <archive.ron> [--cell r,c]")?;
    let dim = Dim::parse(dim_s)?;
    let archive = args.get(1).ok_or("apply needs an archive path")?.clone();
    let mut cell: Option<String> = None;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--cell" => {
                cell = Some(args.get(i + 1).ok_or("--cell needs a `row,col` value")?.clone());
                i += 2;
            }
            other => return Err(format!("unknown flag {other:?}")),
        }
    }
    let spec = match &cell {
        Some(c) => format!("{archive}#{c}"),
        None => archive.clone(),
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

/// Parse `train verify` args: only `--reps N` (default [`GOLDEN_STABILITY_REPS`]).
fn parse_verify(args: &[String]) -> Result<u32, String> {
    let mut reps = GOLDEN_STABILITY_REPS;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--reps" {
            let v = it.next().ok_or("--reps needs a value")?;
            reps = v.parse().map_err(|_| format!("bad --reps {v:?}"))?;
        } else {
            return Err(format!("unknown verify arg {a:?}"));
        }
    }
    Ok(reps)
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
