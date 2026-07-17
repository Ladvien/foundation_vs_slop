//! **Rollout evaluation** (feature `test-harness`) — run one candidate against one opponent on one
//! world, and report what happened.
//!
//! This is the only place the offline search touches the simulation. It boots the *real* game plugins
//! headlessly (`sim_harness`), installs the candidate through [`BrainSource`], enables the recorder, steps
//! a fixed number of ticks, and hands back an [`EpisodeTrace`] + [`EpisodeOutcome`].
//!
//! Two rollouts per candidate, on **different dungeon seeds**, are what [`crate::squad_ai::surprise`]
//! needs: both feed the surprise term, and the pair feeds learnability (fit a mode-transition model on
//! the first, require it to predict the second). Nothing here interprets them — scoring lives in
//! `surprise`, admission lives in `surprise::minimal_criterion`.
//!
//! **One `App` per process.** `sim_harness` holds a process-wide lock and pins the global compute pool
//! and rayon to a single thread for determinism, so parallelism in the search is *processes*, never
//! threads. [`serial_guard`] is held for each `App`'s lifetime regardless, so a single-process driver is
//! correct too, just serial.

use bevy::math::IVec2;
use bevy::prelude::App;

use crate::ai::brain::BrainSource;
use crate::audio_tuning::AudioTuning;
use crate::config::WorldConfig;
use crate::sim_harness::{
    build_headless_app, clear_squad_orders, field_saturation, floor_cells, issue_squad_order,
    liveness_violations, nest_cells, ordered_unit_count, serial_guard, snapshot_hash, squad_centroid_cell,
    squad_health, step, PolicyFactory, SimConfig,
};

use super::surprise::{EpisodeOutcome, EpisodeTrace};
use super::trace::Recording;

/// How often, in fixed ticks, the liveness oracle is consulted during a rollout. A violation anywhere in
/// the episode disqualifies the candidate via the minimal criterion — an elite that NaNs the world is not
/// an elite, however surprising.
const LIVENESS_EVERY: u32 = 300;

/// Ticks the synthetic player spends advancing the squad under a standing order (~5 s at 60 Hz).
///
/// Measured, not guessed. At 2 s the squad never arrived anywhere: orders were re-issued mid-transit, it
/// oscillated, and coverage plateaued at ~4% even over a two-minute episode. Crossing a 192-tile dungeon
/// takes far longer than the flow field's re-plan interval.
const ADVANCE_TICKS: u32 = 300;

/// Ticks the squad is then left **unordered**, with the AI in full control (~5 s at 60 Hz).
///
/// This phase is not optional garnish — it is the only part of the episode that evaluates the brain. A
/// standing `MoveOrder` overrides locomotion *and* excludes the unit from `unit_actions` and `medic_heal`
/// (both are `Without<MoveOrder>`), so a permanently-ordered squad exercises nothing but the mode label
/// and the auto-firing rifles. Alternating also matches how the game is actually played: the player
/// advances the squad, then watches the fight.
const ENGAGE_TICKS: u32 = 300;

/// Advance windows spent DWELLING toward each crab hub before the engage window. The squad is fast and the
/// swarm is slow, but crabs forage away from their nests, so a single ADVANCE window (~30 world-units) never
/// closes the gap — measured, the squad stalled ~30 units off the nearest crab and never fired. Three
/// windows (~90 units of unbroken travel, no engage drift between them) reliably lands it on the hub, where
/// breeding respawns crabs into firing range. Keep it a FIXED count (not an arrival check) so the tour
/// schedule stays independent of the brain under test.
const DWELL_ADVANCES: u32 = 3;

/// Floor goals interleaved between nests, so the tour both explores and fights.
const FLOOR_GOALS: usize = 4;

/// One episode's evidence.
pub struct Rollout {
    pub trace: EpisodeTrace,
    pub outcome: EpisodeOutcome,
    /// `snapshot_hash` (folded `Transform` + `Health` of every actor) at the final tick. The exact-value
    /// oracle for "did this config change where the agents ended up", independent of the trace's mode
    /// counts — a config that only shifts *steering* (not which discrete mode is chosen) moves this but not
    /// the mode histogram. Used by `a_mutated_audio_config_changes_the_sim` to prove the acoustic coupling
    /// reaches gameplay.
    pub snapshot: u64,
    /// Per-checkpoint squad **survival belief** `b_t ∈ [0,1]` (aggregate squad health / the episode's
    /// starting health), sampled every liveness checkpoint. The input to `squad_ai::interest`, which reduces
    /// this trajectory to the human-interest proxies (suspense / outcome-surprise / effectance).
    pub belief: Vec<f32>,
}

/// **The synthetic player.**
///
/// Without one, the offline search evaluates a game nobody is playing: the squad idles at spawn, covers a
/// handful of cells, and *every* episode fails the behavioural minimal criterion. (It did. That is how
/// this was found.) Firing is automatic (`laser::fire_laser` — "no key to hold"), but *movement* is the
/// player's, and movement is what produces an encounter.
///
/// The tour is a deterministic stride across the reachable floor interleaved with the crab nests — the
/// objective a real player walks toward. Each goal gets one [`ADVANCE_TICKS`] phase under a standing
/// order, followed by one [`ENGAGE_TICKS`] phase with **all orders revoked**, so the squad AI actually
/// runs.
///
/// It is **part of the environment, not part of the candidate**. It must be byte-identical for the
/// baseline prior sweep and for every candidate, or the surprise term would be measuring differences in
/// how the *player* behaved rather than in how the *squad* did. Hence: no RNG, no dependence on the
/// brains under test, and a stride derived only from the dungeon.
fn tour_goals(app: &mut App) -> Vec<IVec2> {
    let nests = nest_cells(app);
    let floors = floor_cells(app);
    if floors.is_empty() {
        return nests;
    }
    // A spread of floor goals (deterministic stride, no RNG), with the nests interleaved. The nests are
    // the objective: a player walks toward them, and that is what puts the squad inside the swarm. A tour
    // of arbitrary floor cells kills a few crabs at range and nobody is ever bitten — the behavioural
    // minimal criterion then (correctly) rejects every episode as "nothing was at stake".
    let stride = (floors.len() / FLOOR_GOALS.max(1)).max(1);
    let explore: Vec<IVec2> = floors.iter().step_by(stride).copied().collect();

    let mut goals = Vec::with_capacity(explore.len() + nests.len());
    let mut nests = nests.into_iter();
    for cell in explore {
        goals.push(cell);
        if let Some(nest) = nests.next() {
            goals.push(nest);
        }
    }
    goals.extend(nests);
    goals
}

/// Run one headless episode of `ticks` fixed steps with `brains` installed on dungeon seed `dungeon_seed`,
/// driven by the synthetic player.
///
/// Physics is **off** (`deterministic_core`): only gib chunks are `RigidBody::Dynamic` and they carry no
/// `Health`, so they cannot influence gameplay — but the Avian solver is not bit-reproducible, and a
/// search whose fitness wobbles between identical evaluations is searching noise.
pub fn rollout(
    brains: BrainSource,
    config: Option<WorldConfig>,
    audio: Option<AudioTuning>,
    behavior: Option<crate::behavior_tuning::BehaviorTuning>,
    dungeon_seed: u64,
    ticks: u32,
) -> Rollout {
    run_episode(&deterministic_cfg(brains, config, audio, behavior, dungeon_seed), ticks, false)
}

/// Like [`rollout`], but also records the per-checkpoint **survival-belief series** (`Rollout::belief`) that
/// `squad_ai::interest` reduces to the human-interest proxies. Only the consumers that read `belief` — `train
/// probe` and `train poet` — call this and pay for the extra per-checkpoint `squad_health` query; every other
/// search calls [`rollout`], which skips the sampling entirely.
pub fn rollout_with_belief(
    brains: BrainSource,
    config: Option<WorldConfig>,
    audio: Option<AudioTuning>,
    behavior: Option<crate::behavior_tuning::BehaviorTuning>,
    dungeon_seed: u64,
    ticks: u32,
) -> Rollout {
    run_episode(&deterministic_cfg(brains, config, audio, behavior, dungeon_seed), ticks, true)
}

/// Build the deterministic-core `SimConfig` for a rollout, installing whichever candidate slices are present.
/// `None` runs the shipped slice (what the baseline prior sweep and every non-owning population pass); `Some`
/// installs an evolved one — one config reaches the sim either way, at the single `GameConfig` seam.
fn deterministic_cfg(
    brains: BrainSource,
    config: Option<WorldConfig>,
    audio: Option<AudioTuning>,
    behavior: Option<crate::behavior_tuning::BehaviorTuning>,
    dungeon_seed: u64,
) -> SimConfig {
    let mut cfg = SimConfig::deterministic_core_seeded(dungeon_seed).with_brains(brains);
    if let Some(w) = config {
        cfg = cfg.with_world_config(w);
    }
    if let Some(a) = audio {
        cfg = cfg.with_audio_config(a);
    }
    if let Some(b) = behavior {
        cfg = cfg.with_behavior_config(b);
    }
    cfg
}

/// Like [`rollout`], but installs a **learned squad controller** (`ActivePolicy`) for the episode — the
/// evaluation path for the neuroevolution population. The creatures keep their authored brains
/// (`BrainSource::Authored`): we evolve the *squad's* decision policy against the shipped swarm. `policy`
/// mints a fresh controller per rollout (a `Box<dyn SquadPolicy>` is not `Clone`). Everything else — the
/// synthetic player, the recorder, the scoring inputs — is byte-identical to [`rollout`].
pub fn rollout_with_policy(
    policy: PolicyFactory,
    config: Option<WorldConfig>,
    audio: Option<AudioTuning>,
    behavior: Option<crate::behavior_tuning::BehaviorTuning>,
    dungeon_seed: u64,
    ticks: u32,
) -> Rollout {
    let mut cfg = SimConfig::deterministic_core_seeded(dungeon_seed)
        .with_brains(BrainSource::Authored)
        .with_policy(policy);
    if let Some(w) = config {
        cfg = cfg.with_world_config(w);
    }
    if let Some(a) = audio {
        cfg = cfg.with_audio_config(a);
    }
    if let Some(b) = behavior {
        cfg = cfg.with_behavior_config(b);
    }
    // The neuroevolution population scores fitness + the squad descriptor, never the belief series → skip it.
    run_episode(&cfg, ticks, false)
}

/// Run one rollout on an **evolved level** (dungeon architecture + furniture + mould-habitat), with the
/// shipped brains, sampling the belief series so the level can be scored by the experience proxies. The
/// level-population analogue of [`rollout`] — the PCGRL "score a level by how it plays" path (Khalifa et al.
/// 2020). `dungeon_seed` overrides the level's own seed so one evolved level is evaluated across the held-in
/// seed set.
pub fn rollout_level(
    level: crate::squad_ai::level_genome::LevelPhenotype,
    dungeon_seed: u64,
    ticks: u32,
) -> Rollout {
    let cfg = SimConfig::deterministic_core_seeded(dungeon_seed)
        .with_brains(BrainSource::Authored)
        .with_level(level);
    run_episode(&cfg, ticks, true)
}

/// Run one headless episode of `ticks` fixed steps under `cfg`, driven by the synthetic player, and report
/// the trace + outcome. The candidate (brains / world / audio / behaviour / policy) is already baked into
/// `cfg`; this is the shared body of [`rollout`], [`rollout_with_belief`], and [`rollout_with_policy`].
///
/// `sample_belief` gates the per-checkpoint survival-belief series: `true` only for the consumers that read
/// it (`train probe` / `train poet`), so the hot search paths pay nothing for a signal they never use.
/// A per-tick observer threaded through [`run_episode`]'s stepping loop.
///
/// It exists so a determinism probe can record a per-tick `snapshot_hash` trace **through the real
/// schedule** — the synthetic player's hub tour, dwell windows, and engage windows — rather than
/// re-deriving that schedule beside it. A re-derived probe drifts, and a drifted probe lies: an
/// `elapsed = 1` offset once made a window boundary look like the first divergent tick when the truth was
/// ~2000 ticks earlier. One schedule, one path; `None` is the production case and costs a null check per
/// tick against a full `app.update()`.
pub enum TickProbe<'a> {
    /// Production: observe nothing.
    Off,
    /// Record `(tick, snapshot_hash, field_hash, gib_hash)` every `every` ticks.
    ///
    /// BOTH hashes, deliberately. `snapshot_hash` folds only `(Transform, Health)` — the FIELDS are not in
    /// it. A field can therefore diverge for hundreds of ticks while every actor still agrees, and only
    /// surface once a quantised read (`belief_at(world_to_cell(pos))`, a threshold, a mode gate) finally
    /// flips something. Bisecting on `snapshot_hash` alone finds the first ACTOR divergence and calls it the
    /// origin, which is how you end up auditing the wrong system.
    Trace { tick: u32, every: u32, out: &'a mut Vec<(u32, u64, u64, u64)> },
    /// Capture the full `snapshot_rows` at exactly tick `at` — for diffing two runs at the tick they split.
    #[cfg(feature = "test-harness")]
    Rows { tick: u32, at: u32, out: &'a mut Vec<[u32; 5]> },
    /// Capture `snapshot_rows` at EVERY tick.
    ///
    /// Necessary because the first divergent tick VARIES between runs (this is a race that can fire at
    /// several points once crabs pile up), so "bisect with one sample set, then diff rows with a fresh one"
    /// compares two unrelated pairs: the fresh runs may have split earlier, and their row diff then shows
    /// accumulated drift rather than the originating change. One run, one record — find the split and read
    /// its diff from the SAME pair.
    #[cfg(feature = "test-harness")]
    RowTrace { tick: u32, out: &'a mut Vec<Vec<[u32; 5]>> },
}

impl TickProbe<'_> {
    fn observe(&mut self, app: &mut App) {
        match self {
            TickProbe::Off => {}
            TickProbe::Trace { tick, every, out } => {
                *tick += 1;
                if *tick % *every == 0 {
                    out.push((
                        *tick,
                        crate::sim_harness::snapshot_hash(app),
                        crate::sim_harness::field_hash(app),
                        crate::sim_harness::gib_hash(app),
                    ));
                }
            }
            #[cfg(feature = "test-harness")]
            TickProbe::Rows { tick, at, out } => {
                *tick += 1;
                if *tick == *at {
                    **out = crate::sim_harness::snapshot_rows(app);
                }
            }
            #[cfg(feature = "test-harness")]
            TickProbe::RowTrace { tick, out } => {
                *tick += 1;
                out.push(crate::sim_harness::snapshot_rows(app));
            }
        }
    }
}

fn run_episode(cfg: &SimConfig, ticks: u32, sample_belief: bool) -> Rollout {
    run_episode_probed(cfg, ticks, sample_belief, &mut TickProbe::Off)
}

/// [`run_episode`] with a per-tick observer. See [`TickProbe`] for why this seam exists.
///
/// `test-harness` only: the probe is a debugging affordance for the determinism hunt, not a shipped feature.
#[cfg(feature = "test-harness")]
pub fn trace_episode(
    brains: BrainSource,
    dungeon_seed: u64,
    ticks: u32,
    every: u32,
    out: &mut Vec<(u32, u64, u64, u64)>,
) {
    let cfg = deterministic_cfg(brains, None, None, None, dungeon_seed);
    let mut probe = TickProbe::Trace { tick: 0, every, out };
    run_episode_probed(&cfg, ticks, false, &mut probe);
}

/// Capture `snapshot_rows` at EVERY tick of the real episode (index 0 == tick 1). See [`TickProbe`].
#[cfg(feature = "test-harness")]
pub fn row_trace(brains: BrainSource, dungeon_seed: u64, ticks: u32, out: &mut Vec<Vec<[u32; 5]>>) {
    let cfg = deterministic_cfg(brains, None, None, None, dungeon_seed);
    let mut probe = TickProbe::RowTrace { tick: 0, out };
    run_episode_probed(&cfg, ticks, false, &mut probe);
}

/// Capture `snapshot_rows` at exactly tick `at` of the real episode. See [`TickProbe`].
#[cfg(feature = "test-harness")]
pub fn rows_at_tick(
    brains: BrainSource,
    dungeon_seed: u64,
    at: u32,
    out: &mut Vec<[u32; 5]>,
) {
    let cfg = deterministic_cfg(brains, None, None, None, dungeon_seed);
    let mut probe = TickProbe::Rows { tick: 0, at, out };
    run_episode_probed(&cfg, at, false, &mut probe);
}

fn run_episode_probed(
    cfg: &SimConfig,
    ticks: u32,
    sample_belief: bool,
    probe: &mut TickProbe<'_>,
) -> Rollout {
    // Held for the App's whole lifetime (harness invariant 4).
    let _serial = serial_guard();
    let mut app = build_headless_app(cfg);

    // One tick so the dungeon and squad exist before the tour is planned.
    step(&mut app, cfg, 1);
    // The tour heads for the CRAB HUBS (nests), nearest-first from the spawn layout, and DWELLS on each so
    // the squad actually reaches firing range and a firefight happens (its NOISE_SQUAD din is then a
    // stimulus the swarm reacts to). Falls back to the floor spread only on a nest-less map. Ordering by
    // the spawn-time centroid keeps the schedule brain-independent.
    let hubs = {
        let mut nests = nest_cells(&mut app);
        if nests.is_empty() {
            tour_goals(&mut app)
        } else {
            let from = squad_centroid_cell(&mut app).as_vec2();
            // TOTAL order: the quantised distance alone ties often (two nests at near-equal range round to
            // the same i64), and `sort_by_key` is *stable*, so tied nests would keep their input order —
            // which `nest_cells` now canonicalises, but relying on that implicitly is how this broke before.
            // Break ties on the cell itself so the tour is a pure function of the map, explicitly.
            // SORT-OK: the `(c.y, c.x)` suffix makes this total over distinct cells, and `nest_cells` dedups.
            nests.sort_by_key(|c| ((c.as_vec2().distance_squared(from) * 100.0) as i64, c.y, c.x));
            nests
        }
    };

    // Enable recording *after* `Startup` and the warm-up tick, so the recorder never counts the spawn
    // frame's initial `ActiveBehavior` writes as decisions.
    start_recording(&mut app);

    // Starting squad health total — the denominator for the survival-belief series (see `run`). Captured
    // after the warm-up tick, before recording, so it is the full-strength squad the player started with.
    // `0.0` when belief sampling is off, which `run` reads as the signal to skip the per-checkpoint query.
    let start_max_health = if sample_belief { squad_health(&mut app).1 } else { 0.0 };
    // Per-checkpoint survival belief, filled by `run` — the input to the interest proxies.
    let mut belief: Vec<f32> = Vec::new();
    let mut violations = 0u32;
    // Worst field-degeneracy seen across the episode (the saturation / whole-map-smear guard).
    let mut peak_field = 0.0f32;
    let mut field_flatness = 0.0f32;
    let mut elapsed = 0u32;
    let mut hub_idx = 0usize;
    // Ticks in which at least one unit was under a player order — reported so an evaluation that has
    // silently reverted to "the player drives everything" is visible rather than assumed away.
    let mut ordered_ticks = 0u32;

    while elapsed < ticks {
        let hub = hubs.get(hub_idx % hubs.len().max(1)).copied();
        // ── dwell-advance: drive the squad onto the crab hub over several unbroken windows ──
        for _ in 0..DWELL_ADVANCES {
            if elapsed >= ticks {
                break;
            }
            // An unreachable hub is skipped, not retried (a bad flow-field returns false): the schedule is
            // fixed, and stalling on one hub would make episode length depend on the dungeon.
            if let Some(hub) = hub {
                issue_squad_order(&mut app, hub);
            }
            let stepped = ADVANCE_TICKS.min(ticks - elapsed);
            elapsed += run(
                &mut app,
                cfg,
                stepped,
                &mut violations,
                &mut peak_field,
                &mut field_flatness,
                &mut belief,
                start_max_health,
                probe,
            );
            if ordered_unit_count(&mut app) > 0 {
                ordered_ticks += stepped;
            }
        }
        if elapsed >= ticks {
            break;
        }

        // ── engage: hand the squad back to its brain to fight the crabs now within reach ──
        clear_squad_orders(&mut app);
        elapsed += run(
            &mut app,
            cfg,
            ENGAGE_TICKS.min(ticks - elapsed),
            &mut violations,
            &mut peak_field,
            &mut field_flatness,
            &mut belief,
            start_max_health,
            probe,
        );
        hub_idx += 1;
    }

    let mut rollout = finish_recording(&mut app, violations, peak_field, field_flatness, belief);
    rollout.outcome.ordered_ticks = ordered_ticks.min(ticks);
    rollout
}

/// Step `ticks`, consulting the liveness oracle every [`LIVENESS_EVERY`] and — when `start_max_health > 0` —
/// sampling the survival belief at the same cadence. Returns `ticks`.
#[allow(clippy::too_many_arguments)]
fn run(
    app: &mut App,
    cfg: &SimConfig,
    ticks: u32,
    violations: &mut u32,
    peak_field: &mut f32,
    field_flatness: &mut f32,
    belief: &mut Vec<f32>,
    start_max_health: f32,
    probe: &mut TickProbe<'_>,
) -> u32 {
    let mut done = 0;
    while done < ticks {
        let chunk = (ticks - done).min(LIVENESS_EVERY);
        // Stepped ONE tick at a time so `probe` can observe every tick. This is not a different schedule:
        // `sim_harness::step(app, cfg, n)` is exactly `for _ in 0..n { app.update() }`, so the sim cannot
        // tell the difference. It matters that this stays the real schedule — a probe that re-derives the
        // synthetic player's hub tour reports a window boundary, not the true first divergence.
        for _ in 0..chunk {
            step(app, cfg, 1);
            probe.observe(app);
        }
        done += chunk;
        *violations += liveness_violations(app).len() as u32;
        // Field-sanity: track the worst degeneracy (highest peak, flattest smear) across the episode.
        let (peak, flatness) = field_saturation(app);
        *peak_field = peak_field.max(peak);
        *field_flatness = field_flatness.max(flatness);
        // Survival-belief sample for the interest proxies: current squad health / the starting total. Skipped
        // entirely (no `squad_health` query, no push) when `start_max_health <= 0` — which the caller sets to
        // disable sampling on the hot search paths that never read `belief`, and which also holds when the
        // squad was wiped at spawn (no belief to record). Consumers of an empty series read interest 0.
        if start_max_health > 0.0 {
            let b = (squad_health(app).0 / start_max_health).clamp(0.0, 1.0);
            belief.push(b);
        }
    }
    ticks
}

fn start_recording(app: &mut App) {
    // `Startup` has already run (`build_headless_app` calls `finish`/`cleanup`), but no `FixedUpdate` tick
    // has: the recorder's first observation is therefore the first real decision.
    app.world_mut().resource_mut::<Recording>().start();
}

fn finish_recording(
    app: &mut App,
    liveness_violations: u32,
    peak_field: f32,
    field_flatness: f32,
    belief: Vec<f32>,
) -> Rollout {
    // Fold the final actor state BEFORE borrowing `Recording` (both need `&mut app`).
    let snapshot = snapshot_hash(app);
    let mut rec = app.world_mut().resource_mut::<Recording>();
    rec.enabled = false;
    let mut outcome = rec.outcome;
    outcome.liveness_violations = liveness_violations;
    outcome.peak_field = peak_field;
    outcome.field_flatness = field_flatness;
    Rollout { trace: std::mem::take(&mut rec.trace), outcome, snapshot, belief }
}
