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

/// The five phases of building a run's world, run in order on `OnEnter(RunState::Active)`.
///
/// The order is a real dependency chain, not tidiness: the config is refreshed before anything reads
/// it, the grids are sized from the `Dungeon`, the populace is placed on floor cells the `Dungeon`
/// defines, and the post-pass reads the populace. Anything registered here must be **idempotent
/// across runs** — it executes once per expedition, not once per process.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunBuild {
    /// Refresh each consumer's per-run config resource from [`crate::config::GameConfig`], **after**
    /// anything that dials it for this expedition has written (`director::pick_next_challenge`).
    ///
    /// Plugins snapshot their own slice into a resource at *plugin-build* time, which is right for the
    /// first run and was silently wrong for every one after it: the director sampled a cell from the
    /// level archive, wrote `gc.dungeon`/`gc.mycelia`/`gc.placement.*`, and **no consumer ever read
    /// `GameConfig` again**, so every expedition was the authored world under a Branch-universe label
    /// (FVS-H-8). This stage is what makes a later run see a different slice. It is deliberately a
    /// *stage* rather than a one-off fix in `dungeon`: any future slice the director learns to dial
    /// has a defined place to be refreshed, and the seam is named where the ordering lives.
    Config,
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
    /// **The real win (FVS-B-3).** Contain `count` anomalies *and* get the surviving squad back to the
    /// extraction point. Both halves are required: a capture you cannot walk out with is not a secure.
    ///
    /// The extraction point is the cell the squad inserted at (`Dungeon::spawn`), which is why this
    /// needs no new worldgen — you leave the way you came in. When Site-67 lands (FVS-G-5) the ASYNC
    /// door is the thing standing on that cell, and *this rule does not change*: the door is the
    /// extraction zone with a body.
    ExtractContained {
        /// How many anomalies must be held when the squad reaches the exit.
        count: u32,
    },
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

impl SessionConfig {
    /// Reject a malformed authored win rule at load — **one path, no fallback**. A win condition that
    /// can never be met, or that is met on tick zero, is a content bug and must fail at the door rather
    /// than produce a run nobody can win (or one that wins itself).
    ///
    /// Called from `config::load_game_config`, the single validation seam — not from a plugin `build`.
    pub fn validate(&self) -> Result<(), String> {
        match self.win {
            WinCondition::SurviveTicks(0) => {
                Err("win: SurviveTicks(0) resolves to Victory on the first tick".into())
            }
            WinCondition::ExtractContained { count: 0 } => {
                Err("win: ExtractContained(count: 0) is won by walking to the exit".into())
            }
            _ => Ok(()),
        }
    }
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
        app.init_resource::<AutoStartFirstRun>()
            .init_state::<RunState>()
            .insert_resource(seed)
            .add_sub_state::<RunPhase>()
            .insert_resource(win)
            .init_resource::<RunOutcome>()
            .init_resource::<RunClock>()
            .init_resource::<SquadSeen>()
            .add_message::<ForceVictory>()
            .configure_sets(
                OnEnter(RunState::Active),
                (RunBuild::Config, RunBuild::World, RunBuild::Grids, RunBuild::Populate, RunBuild::PostPopulate)
                    .chain(),
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
                (tick_run_clock, resolve_run, advance_run_phase)
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

/// Should boot drop straight into an expedition?
///
/// **`true` for the harness, `false` for the windowed game — and the split is load-bearing.**
///
/// Every headless test and the whole offline search assume a world exists after `step(app, 1)`:
/// `tests/replay.rs`, `tests/session.rs`, `tests/containment.rs` and every golden are written against
/// that. So the harness must keep booting straight into `Active`, byte-identically.
///
/// The windowed game must not, because it now opens in **Site-67** (`AppState::Site`), and building an
/// expedition world nobody asked for would both waste the work and advance nothing meaningful — the
/// player enters a run by walking into the ASYNC door.
///
/// A resource rather than `#[cfg(feature = "test-harness")]`: a cfg would make the shipped binary and
/// the tested binary structurally different plugin graphs, which is exactly what `sim_harness`'s
/// "identical plugin graph" discipline exists to prevent. Push 8 measured resources hash-neutral, so
/// defaulting it `true` and overriding it in `lib::run` leaves every golden untouched.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoStartFirstRun(pub bool);

impl Default for AutoStartFirstRun {
    fn default() -> Self {
        // Harness-shaped by default, so a bare `App` behaves the way every existing test expects.
        Self(true)
    }
}

/// Leave [`RunState::Idle`] once `Startup` has produced the asset handles the world build depends on.
/// `PostStartup` runs after every `Startup` system, and the frame's `StateTransition` then fires
/// `OnEnter(Active)` before `RunFixedMainLoop` — so the world exists before the first fixed tick.
fn begin_first_run(auto: Res<AutoStartFirstRun>, mut next: ResMut<NextState<RunState>>) {
    if !auto.0 {
        // The windowed game opens at Site-67 and enters a run through the ASYNC door instead.
        return;
    }
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

/// Everything [`decide`] reads, gathered so the rule stays a pure function of named facts rather than a
/// growing positional argument list. Built once per tick by [`resolve_run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunFacts {
    /// Fixed ticks elapsed this run.
    pub ticks: u64,
    /// Any `Unit` with `current > 0.0`.
    pub squad_alive: bool,
    /// Has a living squad ever been observed this run? Guards FVS-A-5's build gap.
    pub squad_seen: bool,
    /// Anomalies currently carrying [`crate::containment::Contained`] — **live entities, not
    /// `Specimen` records.** `Specimen` deliberately outlives the run (it is the roguelite boundary),
    /// so counting it would hand expedition 2 a free win on expedition 1's captures.
    pub contained: u32,
    /// Every living unit is inside an extraction zone.
    pub squad_extracted: bool,
    /// The debug-only F10 request.
    pub forced_victory: bool,
}

/// Pure win/lose decision, split out of [`resolve_run`] so the rule is unit-testable without an `App`
/// (the same shape as `ui::state::should_freeze`). Returns `None` while the run is still going.
///
/// Precedence: **a real defeat beats a forced victory.** The dev trigger exists to reach a screen, not
/// to rewrite what happened.
fn decide(win: WinCondition, f: RunFacts) -> Option<RunOutcome> {
    if f.squad_seen && !f.squad_alive {
        return Some(RunOutcome::Defeat(DefeatCause::SquadWipe));
    }
    match win {
        WinCondition::SurviveTicks(n) if f.ticks >= n => return Some(RunOutcome::Victory),
        WinCondition::SurviveTicks(_) => {}
        // Both halves, every tick. Deliberately NOT ratcheted: an anomaly destroyed after capture drops
        // `contained` and un-arms the win, which is the coherent reading ("there is nothing left to
        // extract") and costs no un-ratchet code.
        WinCondition::ExtractContained { count }
            if f.contained >= count && f.squad_extracted =>
        {
            return Some(RunOutcome::Victory);
        }
        WinCondition::ExtractContained { .. } => {}
    }
    f.forced_victory.then_some(RunOutcome::Victory)
}

/// Which phase the run is in, derived from live state rather than ratcheted forward.
///
/// Pure, for the same reason [`decide`] is. Deriving rather than latching means a lost capture walks the
/// phase back on its own — there is no "un-advance" path to get wrong.
///
/// `required` is `None` for win conditions that have no containment target ([`WinCondition::SurviveTicks`]),
/// which holds the phase at `Locating`/`Containing` rather than parking it in `Extracting` forever.
fn phase_for(in_progress: bool, contained: u32, required: Option<u32>) -> RunPhase {
    match required {
        Some(n) if contained >= n => RunPhase::Extracting,
        _ if in_progress || contained > 0 => RunPhase::Containing,
        _ => RunPhase::Locating,
    }
}

/// The single writer of [`RunOutcome`]. Reads the world and applies [`decide`]. It writes **only** that
/// resource — the world stays `Active` so the player reads the debrief over the final frame.
fn resolve_run(
    win: Res<WinCondition>,
    clock: Res<RunClock>,
    mut outcome: ResMut<RunOutcome>,
    mut seen: ResMut<SquadSeen>,
    mut forced: MessageReader<ForceVictory>,
    units: Query<(&crate::health::Health, &Transform), With<crate::squad::Unit>>,
    contained: Query<(), With<crate::containment::Contained>>,
    zones: Query<(&crate::containment::ExtractionZone, &Transform)>,
) {
    // Order-independent throughout: `any`/`all`/`count` over a predicate are commutative, so no
    // canonical sort is needed here (contrast every site that *picks* from a query — see
    // `tests/determinism_lint.rs`).
    // Health, not entity existence — see the module docs on why an ordering edge was the wrong tool.
    let squad_alive = units.iter().any(|(hp, _)| hp.current > 0.0);
    if squad_alive {
        seen.0 = true;
    }

    // EVERY living unit must be in a zone. "All" rather than "any" is the deliberate exfil reading:
    // leaving a member behind should cost the run, not be free. Vacuously true with no living units,
    // which is harmless — the wipe branch above already decided that case.
    let squad_extracted = units.iter().filter(|(hp, _)| hp.current > 0.0).all(|(_, tf)| {
        zones.iter().any(|(zone, ztf)| zone.contains(ztf.translation, tf.translation))
    });

    let facts = RunFacts {
        ticks: clock.ticks,
        squad_alive,
        squad_seen: seen.0,
        // Live `Contained` anomalies, which are run-scoped and therefore reset themselves each run.
        contained: contained.iter().count() as u32,
        squad_extracted,
        forced_victory: forced.read().next().is_some(),
    };

    let Some(decided) = decide(*win, facts) else {
        return;
    };
    *outcome = decided;
}

/// The single writer of [`NextState<RunPhase>`]. A sibling of [`resolve_run`], not a part of it: two
/// facts, two single writers.
fn advance_run_phase(
    win: Res<WinCondition>,
    phase: Res<State<RunPhase>>,
    mut next: ResMut<NextState<RunPhase>>,
    attempts: Query<&crate::containment::Containment>,
    contained: Query<(), With<crate::containment::Contained>>,
) {
    let required = match *win {
        WinCondition::ExtractContained { count } => Some(count),
        WinCondition::SurviveTicks(_) => None,
    };
    let in_progress =
        attempts.iter().any(|c| c.phase() == crate::containment::Phase::BeingContained);
    let want = phase_for(in_progress, contained.iter().count() as u32, required);
    // `set_if_neq` so an unchanged phase writes nothing and cannot fire a same-state `OnEnter`.
    if *phase.get() != want {
        next.set(want);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: WinCondition = WinCondition::SurviveTicks(100);
    const EXTRACT: WinCondition = WinCondition::ExtractContained { count: 1 };

    /// A live, unremarkable run: squad up, nothing contained, nobody at the exit.
    fn facts(ticks: u64, squad_alive: bool, squad_seen: bool, forced_victory: bool) -> RunFacts {
        RunFacts {
            ticks,
            squad_alive,
            squad_seen,
            contained: 0,
            squad_extracted: false,
            forced_victory,
        }
    }

    #[test]
    fn a_run_with_a_living_squad_and_time_left_is_undecided() {
        assert_eq!(decide(WIN, facts(0, true, true, false)), None);
        assert_eq!(decide(WIN, facts(99, true, true, false)), None);
    }

    #[test]
    fn a_dead_squad_is_only_a_wipe_once_a_squad_has_existed() {
        // Before spawn: nothing alive is "not populated yet", not a defeat. This is the predicate that
        // keeps FVS-A-5's run-scoped construction from resolving the run on its first tick.
        assert_eq!(decide(WIN, facts(0, false, false, false)), None);
        assert_eq!(
            decide(WIN, facts(0, false, true, false)),
            Some(RunOutcome::Defeat(DefeatCause::SquadWipe))
        );
    }

    #[test]
    fn the_timer_wins_at_and_after_the_threshold() {
        assert_eq!(decide(WIN, facts(100, true, true, false)), Some(RunOutcome::Victory));
        assert_eq!(decide(WIN, facts(101, true, true, false)), Some(RunOutcome::Victory));
    }

    #[test]
    fn a_dev_forced_victory_cannot_launder_a_real_defeat() {
        // Forced victory on a live run: allowed (that is the tool's whole purpose).
        assert_eq!(decide(WIN, facts(10, true, true, true)), Some(RunOutcome::Victory));
        // Forced victory on a wiped squad: the real rule still wins.
        assert_eq!(
            decide(WIN, facts(10, false, true, true)),
            Some(RunOutcome::Defeat(DefeatCause::SquadWipe))
        );
    }

    #[test]
    fn extraction_requires_both_a_capture_and_a_return() {
        let held = RunFacts { contained: 1, ..facts(500, true, true, false) };
        let at_exit = RunFacts { squad_extracted: true, ..facts(500, true, true, false) };
        // A capture with nobody at the exit is not a win — this is the assertion that makes the rule
        // "extract", not merely "contain".
        assert_eq!(decide(EXTRACT, held), None);
        // And standing at the exit empty-handed is not a win either.
        assert_eq!(decide(EXTRACT, at_exit), None);
        // Both halves.
        assert_eq!(
            decide(EXTRACT, RunFacts { squad_extracted: true, ..held }),
            Some(RunOutcome::Victory)
        );
    }

    #[test]
    fn extraction_ignores_the_clock_entirely() {
        // No deadline in this variant: a patient player is not punished by the win rule. (Losing to a
        // timer would be a DEFEAT rule, and there isn't one.)
        let won = RunFacts { contained: 1, squad_extracted: true, ..facts(0, true, true, false) };
        assert_eq!(decide(EXTRACT, won), Some(RunOutcome::Victory));
        assert_eq!(decide(EXTRACT, RunFacts { ticks: 10_000_000, ..won }), Some(RunOutcome::Victory));
    }

    #[test]
    fn losing_the_specimen_un_arms_the_win() {
        // `contained` counts LIVE anomalies, so destroying one after capture drops the count and the
        // squad standing at the exit no longer wins. Coherent — there is nothing left to extract — and
        // it falls out of deriving rather than ratcheting.
        let won = RunFacts { contained: 1, squad_extracted: true, ..facts(500, true, true, false) };
        assert_eq!(decide(EXTRACT, won), Some(RunOutcome::Victory));
        assert_eq!(decide(EXTRACT, RunFacts { contained: 0, ..won }), None);
    }

    #[test]
    fn a_wipe_still_beats_a_completed_extraction() {
        // Precedence is unchanged by the new variant: dying on the extraction pad is still a defeat.
        let wiped = RunFacts {
            contained: 1,
            squad_extracted: true,
            ..facts(500, false, true, false)
        };
        assert_eq!(decide(EXTRACT, wiped), Some(RunOutcome::Defeat(DefeatCause::SquadWipe)));
    }

    #[test]
    fn the_phase_is_derived_not_ratcheted() {
        let req = Some(1);
        // Nothing happening yet.
        assert_eq!(phase_for(false, 0, req), RunPhase::Locating);
        // An attempt under way, or any banked capture, reads as Containing.
        assert_eq!(phase_for(true, 0, req), RunPhase::Containing);
        assert_eq!(phase_for(false, 1, Some(2)), RunPhase::Containing);
        // Quota met.
        assert_eq!(phase_for(false, 1, req), RunPhase::Extracting);
        // ...and it walks BACK when the capture is lost. Deriving is what makes this free.
        assert_eq!(phase_for(false, 0, req), RunPhase::Locating);
    }

    #[test]
    fn a_win_condition_with_no_containment_target_never_reaches_extracting() {
        // `SurviveTicks` has no quota, so parking the phase in `Extracting` forever would be a lie.
        assert_eq!(phase_for(false, 0, None), RunPhase::Locating);
        assert_eq!(phase_for(false, 9, None), RunPhase::Containing);
        assert_eq!(phase_for(true, 0, None), RunPhase::Containing);
    }

    #[test]
    fn a_win_condition_that_wins_itself_is_rejected_at_the_door() {
        assert!(SessionConfig { win: WinCondition::SurviveTicks(0) }.validate().is_err());
        assert!(SessionConfig { win: WinCondition::ExtractContained { count: 0 } }
            .validate()
            .is_err());
        assert!(SessionConfig { win: EXTRACT }.validate().is_ok());
        assert!(SessionConfig { win: WIN }.validate().is_ok());
    }

    #[test]
    fn a_decided_outcome_reports_itself_decided() {
        assert!(!RunOutcome::Undecided.is_decided());
        assert!(RunOutcome::Victory.is_decided());
        assert!(RunOutcome::Defeat(DefeatCause::SquadWipe).is_decided());
    }
}
