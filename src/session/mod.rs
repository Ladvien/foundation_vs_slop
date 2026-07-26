//! **Session lifecycle — the deterministic answer to "did this run resolve, and how?"**
//!
//! One expedition = one run. This module owns the *decision* (win / lose / still going); `crate::ui`
//! owns the *screens* that show it. The split is not stylistic — it is what makes the outcome testable:
//!
//! `AppState` (`Boot → Title → Warmup → InGame`) is registered only by `UiPlugin` in `lib::run`, and
//! `tests/replay.rs::ui_never_leaks_into_deterministic_core` **asserts it is absent** in the headless
//! harness. So a win/lose decision expressed as an `AppState` variant would be invisible to every
//! deterministic test — the thing most worth pinning would be the one thing unpinnable. Instead:
//!
//! * **Decision** — this module. [`SessionPlugin`] is registered in **both** `lib::run` and
//!   `sim_harness`, exactly like every other gameplay plugin, so [`RunOutcome`] is readable headless and
//!   the terminal states are covered by the exact-hash gate.
//! * **Presentation** — `crate::ui::debrief` mirrors [`RunOutcome`] onto `AppState`, windowed-only,
//!   on `Update`. It never writes back.
//!
//! # The single-writer / latch rule
//!
//! [`resolve_run`] is the **only** writer of [`RunOutcome`], and both systems here are gated on
//! `resource_equals(RunOutcome::Undecided)` — that run condition **is** the latch. A resolved run can
//! never re-resolve, and the clock stops with it. There is deliberately no second guard (an
//! `if already_decided { return }` *and* a run condition would be two mechanisms for one invariant —
//! the shape that makes a regression untraceable; see `TESTING.md` invariant 5 for the same argument
//! applied to the thread pool).
//!
//! The latch has to be the resource for a second reason too: `NextState` applies in the
//! `StateTransition` schedule, which runs *before* `RunFixedMainLoop` in a frame, so a frame catching up
//! several fixed sub-steps would re-run a state-gated `resolve_run` before the transition landed. The
//! resource flips immediately; a state does not.
//!
//! # Determinism
//!
//! Everything here is `FixedUpdate` and enters the pinned core. [`RunClock`] counts **fixed ticks**, not
//! wall time, so the win timer is identical at any harness `speed` and on any frame rate.
//!
//! The wipe test reads **`Health`, not entity existence**, and that choice is load-bearing for the
//! goldens. Testing "are there any `Unit` entities left?" would have to be ordered after
//! `squad::despawn_dead_units` to be meaningful — and an explicit ordering edge makes Bevy insert an
//! automatic `ApplyDeferred` right after that system, flushing its despawn commands *earlier in the
//! tick than they flush today*. Every system between there and the old flush point would then observe a
//! different world, which is a gameplay change smuggled in by a scheduling constraint. Reading health
//! needs no edge: a unit at zero HP is dead whether or not its despawn command has landed, so the
//! predicate is identical before and after the flush and the existing schedule is untouched.
//!
//! Neither system needs a canonical sort — `all()` over a boolean predicate is commutative, so nothing
//! here appears in `tests/determinism_lint.rs`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Lifecycle of one expedition. Coarse on purpose: it exists to give schedules a seam
/// (`OnEnter`/`OnExit`, and the source for [`RunPhase`]), while the *reason* a run ended lives in
/// [`RunOutcome`]. Keeping the cause out of the state enum is what stops `Resolved` from fanning into
/// one variant per defeat cause, each needing its own state-scoped teardown.
#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunState {
    /// No world exists yet. The **default**, and not decoration: world construction runs on
    /// `OnEnter(Active)` and needs the asset handles `Startup` produces (`ValkyrieAnim`, `CrabAnim`,
    /// `MancaAnim`, the laser/gore/audio assets). Bevy runs `StateTransition` *before* `PreStartup`, so a
    /// default of `Active` would fire the build before a single asset existed. [`begin_first_run`] leaves
    /// `Idle` from `PostStartup` instead, and the frame's own `StateTransition` — which sits after
    /// `PreUpdate`, ahead of `RunFixedMainLoop` — builds the world before the first fixed tick.
    #[default]
    Idle,
    /// A world exists and is being played.
    ///
    /// **`Active` means "a world exists", not "the run is unresolved".** A resolved run stays `Active`
    /// while the player reads the debrief over the final frame — the *outcome* lives in [`RunOutcome`],
    /// which is the whole point of splitting decision from state. An earlier draft added a `Resolved`
    /// variant and paid for it immediately: resolving fired `OnExit(Active)`, which despawned the entire
    /// world at the moment of victory and reset the outcome that had just been written. Leaving the run
    /// (`RETURN TO SITE`, `QUIT TO TITLE`) is what sets [`RunState::Idle`] and tears the world down.
    Active,
}

/// The four phases of building a run's world, run in order on `OnEnter(RunState::Active)`.
///
/// The order is a real dependency chain, not tidiness: the grids are sized from the `Dungeon`, the
/// populace is placed on floor cells the `Dungeon` defines, and the post-pass reads the populace.
/// Anything registered here must be **idempotent across runs** — it executes once per expedition, not
/// once per process.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunBuild {
    /// Generate the `Dungeon` for this run's seed.
    World,
    /// Size the per-cell fields (stigmergy, fog, light, mold, almond water) to that dungeon.
    Grids,
    /// Spawn tiles, furniture, squad, and creatures.
    Populate,
    /// Passes that read the spawned populace (manca seeding, faction validation).
    PostPopulate,
}

/// The dungeon seed for the **current** run.
///
/// Seeded from `GameConfig.dungeon.seed`, so the first expedition of a process is exactly the world the
/// replay goldens pin and `SimConfig::dungeon_seed` keeps working through its existing seam. Advanced by
/// splitmix64 on leaving a run, so the *next* expedition is a different world and the whole sequence is
/// still reproducible from the configured seed — which is what makes "each seed is one Branch universe"
/// mechanical rather than flavour.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSeed(pub u64);

impl RunSeed {
    /// Advance to the next run's seed (splitmix64 finalizer — a full-period, well-mixed successor, so
    /// consecutive expeditions share no structure even though the sequence is deterministic).
    fn advance(&mut self) {
        let mut z = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        self.0 = z ^ (z >> 31);
    }
}

/// The three beats of an expedition (vision tier 2: *locate, contain, extract*). Scaffolding for Push 2
/// — nothing drives it until the containment system (FVS-B-3) does.
///
/// Sourced on [`RunState::Active`], **not** `AppState::InGame`: the containment systems that will
/// advance it run on `FixedUpdate` in the harness, where `AppState` does not exist.
#[derive(SubStates, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[source(RunState = RunState::Active)]
pub enum RunPhase {
    /// Find the target.
    #[default]
    Locating,
    /// Drive it into its containable basin and hold.
    Containing,
    /// Carry the contained specimen out.
    Extracting,
}

/// Why a run ended badly. One variant today — the squad wipe is the only *well-defined* loss the sim
/// can currently express (`crate::squad` documents total wipe as a real, unprotected outcome). Breach
/// and site-overrun arrive with Push 2/Push 3, and each is a real rule, not a fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefeatCause {
    /// Every squad member died.
    SquadWipe,
}

/// How this run ended. Written exactly once, by [`resolve_run`].
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunOutcome {
    /// Still going.
    #[default]
    Undecided,
    /// The win condition was met.
    Victory,
    /// A loss condition fired.
    Defeat(DefeatCause),
}

impl RunOutcome {
    /// Whether the run has ended (either way). Used by the windowed mirror to decide when to leave
    /// gameplay; never used as a second latch (see the module docs).
    pub fn is_decided(self) -> bool {
        !matches!(self, RunOutcome::Undecided)
    }
}

/// Fixed-tick clock for the current run. **Not wall time**: it advances once per `FixedUpdate`, so a
/// timer expressed in it is identical at any harness `speed`, any frame rate, and across a replay.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunClock {
    /// Fixed ticks elapsed since the run began.
    pub ticks: u64,
}

/// What it takes to win. The placeholder timer is the *pre-capture* win: it exists so the whole
/// resolve → screens → teardown spine is provable before the containment verb exists (FVS-A-3).
///
/// Push 2 replaces it — `ExtractContained` — by adding a variant, not by wrapping this one in a
/// fallback. Exactly one variant is ever active for a run.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WinCondition {
    /// Hold the site for this many **fixed ticks** (60 = 1 s at the pinned 60 Hz).
    SurviveTicks(u64),
}

/// The `session:` config slice.
///
/// **Deliberately not part of [`crate::config::WorldConfig`]** — i.e. the offline search may not evolve
/// it. Every other tunable here would be fair game under CLAUDE.md's "every feature must evolve" rule,
/// and this is the reasoned exception: the win condition defines what *winning means*, so evolving it
/// would change the measuring stick between rollouts and make archive fitness incomparable across
/// niches. A search free to shorten the timer would "solve" every level by making them shorter. The
/// knob that decides the objective cannot itself be an objective.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    /// The win rule installed at startup as the [`WinCondition`] resource.
    pub win: WinCondition,
}

/// Dev-only request to end the run in victory, so the Victory/Debrief screens are reachable without
/// playing a full timer out. Sent only by a `debug_assertions`-gated hotkey in `crate::ui::debrief`;
/// nothing in a release build ever writes it.
///
/// It is a *request*, not a write: [`resolve_run`] stays the single writer of [`RunOutcome`], and a
/// real defeat still beats it (see [`decide`]) — a debug tool must not be able to launder a loss into
/// a win.
#[derive(Message, Debug, Clone, Copy)]
pub struct ForceVictory;

/// Latch for "a squad has existed in this run". The wipe test presupposes a squad: *zero living units*
/// means "wiped" only after there was something to wipe. Without this, the tick before spawn
/// (unavoidable once FVS-A-5 makes construction run-scoped) reads as an instant defeat.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct SquadSeen(bool);

pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        let gc = app.world().resource::<crate::config::GameConfig>();
        let win = gc.session.win;
        // The FIRST run uses the configured seed verbatim, so the shipped world — and every golden and
        // `SimConfig::dungeon_seed` override that rides the same `GameConfig` seam — is unchanged.
        let seed = RunSeed(gc.dungeon.seed);
        app.init_state::<RunState>()
            .insert_resource(seed)
            .add_sub_state::<RunPhase>()
            .insert_resource(win)
            .init_resource::<RunOutcome>()
            .init_resource::<RunClock>()
            .init_resource::<SquadSeen>()
            .add_message::<ForceVictory>()
            .configure_sets(
                OnEnter(RunState::Active),
                (RunBuild::World, RunBuild::Grids, RunBuild::Populate, RunBuild::PostPopulate).chain(),
            )
            .add_systems(PostStartup, begin_first_run)
            // Reset BEFORE the world is built, so a fresh run starts at tick 0 with an open outcome.
            .add_systems(OnEnter(RunState::Active), reset_run.before(RunBuild::World))
            .add_systems(OnExit(RunState::Active), advance_to_next_world)
            // The run condition IS the latch (module docs): once the outcome leaves `Undecided`,
            // neither system runs again, so the clock freezes at the resolving tick and the outcome
            // can never be overwritten.
            .add_systems(
                FixedUpdate,
                (tick_run_clock, resolve_run)
                    .chain()
                    .after(crate::health::HealthDamage)
                    // Two conditions, two distinct facts — not one invariant guarded twice. `Idle` means
                    // "no world yet" (nothing to judge); `Undecided` is the outcome latch.
                    .run_if(in_state(RunState::Active))
                    .run_if(resource_equals(RunOutcome::Undecided)),
            );
    }
}

/// Tag for an entity that belongs to **this expedition** and must not survive it.
///
/// Every world-population spawn site carries this: dungeon tiles, furniture, the squad, crabs, nests,
/// mancae, bears, SCP-999, the watcher. Declaring the lifetime *at the spawn* — rather than in one
/// teardown system holding a list of everything to clean up — is what stops a new spawner from silently
/// leaking into the next run: the rule travels with the entity that needs it.
///
/// Deliberately **not** on: the camera, UI, asset handles, or anything the Site will own (FVS-G-1) —
/// those outlive a run by design.
pub fn run_scoped() -> DespawnOnExit<RunState> {
    DespawnOnExit(RunState::Active)
}

/// Leave [`RunState::Idle`] once `Startup` has produced the asset handles the world build depends on.
/// `PostStartup` runs after every `Startup` system, and the frame's `StateTransition` then fires
/// `OnEnter(Active)` before `RunFixedMainLoop` — so the world exists before the first fixed tick.
fn begin_first_run(mut next: ResMut<NextState<RunState>>) {
    next.set(RunState::Active);
}

/// Zero the per-run state as an expedition begins.
fn reset_run(
    mut clock: ResMut<RunClock>,
    mut outcome: ResMut<RunOutcome>,
    mut seen: ResMut<SquadSeen>,
) {
    *clock = RunClock::default();
    *outcome = RunOutcome::Undecided;
    // Critical on a re-run: the previous squad is despawned on this same transition, so a stale `true`
    // would read the gap before the new squad spawns as an instant wipe.
    *seen = SquadSeen::default();
}

/// Pick the next Branch universe as the player leaves a run.
///
/// The entities are not despawned here: each spawn site tags its own with
/// `DespawnOnExit(RunState::Active)` ([`run_scoped`]), which Bevy applies on this same transition. One
/// rule, declared where the entity is created, rather than a teardown system that has to be kept in step
/// with every spawner.
fn advance_to_next_world(mut seed: ResMut<RunSeed>) {
    seed.advance();
}

/// Advance the run clock by one fixed tick. `saturating_add` rather than `+`: at 60 Hz a `u64` needs
/// ~9.7 billion years to wrap, but a saturating counter cannot panic in release *or* debug, and this
/// runs in the pinned core where a panic is a lost rollout.
fn tick_run_clock(mut clock: ResMut<RunClock>) {
    clock.ticks = clock.ticks.saturating_add(1);
}

/// Pure win/lose decision, split out of [`resolve_run`] so the rule is unit-testable without an `App`
/// (the same shape as `ui::state::should_freeze`). Returns `None` while the run is still going.
///
/// Precedence: **a real defeat beats a forced victory.** The dev trigger exists to reach a screen, not
/// to rewrite what happened.
fn decide(
    win: WinCondition,
    ticks: u64,
    squad_alive: bool,
    squad_seen: bool,
    forced_victory: bool,
) -> Option<RunOutcome> {
    if squad_seen && !squad_alive {
        return Some(RunOutcome::Defeat(DefeatCause::SquadWipe));
    }
    match win {
        WinCondition::SurviveTicks(n) if ticks >= n => return Some(RunOutcome::Victory),
        WinCondition::SurviveTicks(_) => {}
    }
    forced_victory.then_some(RunOutcome::Victory)
}

/// The single writer of [`RunOutcome`]. Reads the world and applies [`decide`]. It writes **only** that
/// resource — the world stays `Active` so the player reads the debrief over the final frame.
fn resolve_run(
    win: Res<WinCondition>,
    clock: Res<RunClock>,
    mut outcome: ResMut<RunOutcome>,
    mut seen: ResMut<SquadSeen>,
    mut forced: MessageReader<ForceVictory>,
    units: Query<&crate::health::Health, With<crate::squad::Unit>>,
) {
    // Order-independent: `any`/`all` over a predicate is commutative, so no canonical sort is needed
    // here (contrast every site that *picks* from a query — see `tests/determinism_lint.rs`).
    // Health, not entity existence — see the module docs on why an ordering edge was the wrong tool.
    let squad_alive = units.iter().any(|hp| hp.current > 0.0);
    if squad_alive {
        seen.0 = true;
    }
    let forced_victory = forced.read().next().is_some();

    let Some(decided) = decide(*win, clock.ticks, squad_alive, seen.0, forced_victory) else {
        return;
    };
    *outcome = decided;
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: WinCondition = WinCondition::SurviveTicks(100);

    #[test]
    fn a_run_with_a_living_squad_and_time_left_is_undecided() {
        assert_eq!(decide(WIN, 0, true, true, false), None);
        assert_eq!(decide(WIN, 99, true, true, false), None);
    }

    #[test]
    fn a_dead_squad_is_only_a_wipe_once_a_squad_has_existed() {
        // Before spawn: nothing alive is "not populated yet", not a defeat. This is the predicate that
        // keeps FVS-A-5's run-scoped construction from resolving the run on its first tick.
        assert_eq!(decide(WIN, 0, false, false, false), None);
        assert_eq!(
            decide(WIN, 0, false, true, false),
            Some(RunOutcome::Defeat(DefeatCause::SquadWipe))
        );
    }

    #[test]
    fn the_timer_wins_at_and_after_the_threshold() {
        assert_eq!(decide(WIN, 100, true, true, false), Some(RunOutcome::Victory));
        assert_eq!(decide(WIN, 101, true, true, false), Some(RunOutcome::Victory));
    }

    #[test]
    fn a_dev_forced_victory_cannot_launder_a_real_defeat() {
        // Forced victory on a live run: allowed (that is the tool's whole purpose).
        assert_eq!(decide(WIN, 10, true, true, true), Some(RunOutcome::Victory));
        // Forced victory on a wiped squad: the real rule still wins.
        assert_eq!(
            decide(WIN, 10, false, true, true),
            Some(RunOutcome::Defeat(DefeatCause::SquadWipe))
        );
    }

    #[test]
    fn a_decided_outcome_reports_itself_decided() {
        assert!(!RunOutcome::Undecided.is_decided());
        assert!(RunOutcome::Victory.is_decided());
        assert!(RunOutcome::Defeat(DefeatCause::SquadWipe).is_decided());
    }
}
