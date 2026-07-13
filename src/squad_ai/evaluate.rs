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
    let mut cfg = SimConfig::deterministic_core_seeded(dungeon_seed).with_brains(brains);
    // `None` runs the shipped world (the baseline prior sweep passes `None`); `Some(w)` installs an evolved
    // world for the third co-evolving population. One config reaches the sim either way.
    if let Some(w) = config {
        cfg = cfg.with_world_config(w);
    }
    // Likewise the acoustic-stimulus slice: `None` runs the shipped `audio:` config (the prior sweep and
    // every non-audio population pass `None`); `Some(a)` installs an evolved `AudioTuning` for the audio
    // population's rollout. Same single `GameConfig` seam.
    if let Some(a) = audio {
        cfg = cfg.with_audio_config(a);
    }
    // The behaviour slice: `None` runs the shipped `behavior:` config (every non-behaviour population passes
    // `None`); `Some(b)` installs an evolved `BehaviorTuning` for the behaviour population's rollout. Same
    // single `GameConfig` seam.
    if let Some(b) = behavior {
        cfg = cfg.with_behavior_config(b);
    }
    run_episode(&cfg, ticks)
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
    run_episode(&cfg, ticks)
}

/// Run one headless episode of `ticks` fixed steps under `cfg`, driven by the synthetic player, and report
/// the trace + outcome. The candidate (brains / world / audio / behaviour / policy) is already baked into
/// `cfg`; this is the shared body of [`rollout`] and [`rollout_with_policy`].
fn run_episode(cfg: &SimConfig, ticks: u32) -> Rollout {
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
            nests.sort_by_key(|c| (c.as_vec2().distance_squared(from) * 100.0) as i64);
            nests
        }
    };

    // Enable recording *after* `Startup` and the warm-up tick, so the recorder never counts the spawn
    // frame's initial `ActiveBehavior` writes as decisions.
    start_recording(&mut app);

    // Starting squad health total — the denominator for the survival-belief series (see `run`). Captured
    // after the warm-up tick, before recording, so it is the full-strength squad the player started with.
    let start_max_health = squad_health(&mut app).1;
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
        );
        hub_idx += 1;
    }

    let mut rollout = finish_recording(&mut app, violations, peak_field, field_flatness, belief);
    rollout.outcome.ordered_ticks = ordered_ticks.min(ticks);
    rollout
}

/// Step `ticks`, consulting the liveness oracle every [`LIVENESS_EVERY`] and sampling the survival belief at
/// the same cadence. Returns `ticks`.
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
) -> u32 {
    let mut done = 0;
    while done < ticks {
        let chunk = (ticks - done).min(LIVENESS_EVERY);
        step(app, cfg, chunk);
        done += chunk;
        *violations += liveness_violations(app).len() as u32;
        // Field-sanity: track the worst degeneracy (highest peak, flattest smear) across the episode.
        let (peak, flatness) = field_saturation(app);
        *peak_field = peak_field.max(peak);
        *field_flatness = field_flatness.max(flatness);
        // Survival-belief sample for the interest proxies: current squad health / the starting total.
        // `start_max_health <= 0` (no squad at all) yields belief 0 — nothing left to survive.
        let b = if start_max_health > 0.0 {
            (squad_health(app).0 / start_max_health).clamp(0.0, 1.0)
        } else {
            0.0
        };
        belief.push(b);
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
