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
use crate::sim_harness::{
    build_headless_app, clear_squad_orders, field_saturation, floor_cells, issue_squad_order,
    liveness_violations, nest_cells, ordered_unit_count, serial_guard, step, SimConfig,
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

/// Floor goals interleaved between nests, so the tour both explores and fights.
const FLOOR_GOALS: usize = 4;

/// One episode's evidence.
pub struct Rollout {
    pub trace: EpisodeTrace,
    pub outcome: EpisodeOutcome,
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
pub fn rollout(brains: BrainSource, dungeon_seed: u64, ticks: u32) -> Rollout {
    // Held for the App's whole lifetime (harness invariant 4).
    let _serial = serial_guard();

    let cfg = SimConfig::deterministic_core_seeded(dungeon_seed).with_brains(brains);
    let mut app = build_headless_app(&cfg);

    // One tick so the dungeon and squad exist before the tour is planned.
    step(&mut app, &cfg, 1);
    let goals = tour_goals(&mut app);

    // Enable recording *after* `Startup` and the warm-up tick, so the recorder never counts the spawn
    // frame's initial `ActiveBehavior` writes as decisions.
    start_recording(&mut app);

    let mut violations = 0u32;
    // Worst field-degeneracy seen across the episode (the saturation / whole-map-smear guard).
    let mut peak_field = 0.0f32;
    let mut field_flatness = 0.0f32;
    let mut elapsed = 0u32;
    let mut next_goal = 0usize;
    // Ticks in which at least one unit was under a player order — reported so an evaluation that has
    // silently reverted to "the player drives everything" is visible rather than assumed away.
    let mut ordered_ticks = 0u32;

    while elapsed < ticks {
        // ── advance: the player orders the squad toward the next goal ──
        if !goals.is_empty() {
            // An unreachable goal is skipped, not retried: the tour is a fixed schedule, and stalling on
            // one goal would make episode length depend on the dungeon.
            issue_squad_order(&mut app, goals[next_goal % goals.len()]);
            next_goal += 1;
        }
        elapsed += run(
            &mut app,
            &cfg,
            ADVANCE_TICKS.min(ticks - elapsed),
            &mut violations,
            &mut peak_field,
            &mut field_flatness,
        );
        if ordered_unit_count(&mut app) > 0 {
            ordered_ticks += ADVANCE_TICKS;
        }
        if elapsed >= ticks {
            break;
        }

        // ── engage: hand the squad back to its brain ──
        clear_squad_orders(&mut app);
        elapsed += run(
            &mut app,
            &cfg,
            ENGAGE_TICKS.min(ticks - elapsed),
            &mut violations,
            &mut peak_field,
            &mut field_flatness,
        );
    }

    let mut rollout = finish_recording(&mut app, violations, peak_field, field_flatness);
    rollout.outcome.ordered_ticks = ordered_ticks.min(ticks);
    rollout
}

/// Step `ticks`, consulting the liveness oracle every [`LIVENESS_EVERY`]. Returns `ticks`.
fn run(
    app: &mut App,
    cfg: &SimConfig,
    ticks: u32,
    violations: &mut u32,
    peak_field: &mut f32,
    field_flatness: &mut f32,
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
    }
    ticks
}

fn start_recording(app: &mut App) {
    // `Startup` has already run (`build_headless_app` calls `finish`/`cleanup`), but no `FixedUpdate` tick
    // has: the recorder's first observation is therefore the first real decision.
    app.world_mut().resource_mut::<Recording>().start();
}

fn finish_recording(app: &mut App, liveness_violations: u32, peak_field: f32, field_flatness: f32) -> Rollout {
    let mut rec = app.world_mut().resource_mut::<Recording>();
    rec.enabled = false;
    let mut outcome = rec.outcome;
    outcome.liveness_violations = liveness_violations;
    outcome.peak_field = peak_field;
    outcome.field_flatness = field_flatness;
    Rollout { trace: std::mem::take(&mut rec.trace), outcome }
}
