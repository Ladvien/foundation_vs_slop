//! Terminal screens — Victory, Game Over, and the shared Debrief that both funnel into.
//!
//! **Presentation only.** The win/lose decision lives in [`crate::session`], on `FixedUpdate`, inside
//! the deterministic core; this module only *mirrors* it onto [`AppState`] and draws the result. The
//! arrow is one-way — nothing here writes [`RunOutcome`] — which is what keeps the outcome pinnable by
//! the headless goldens (`tests/session.rs`) even though `AppState` itself never exists there.
//!
//! Windowed-only, like the rest of `crate::ui`: every system is on `Update`/`OnEnter`/`OnExit`, reads
//! sim state and never writes it, so nothing enters `snapshot_hash`.

use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::rows::{spawn_rows, Cell, Emphasis, Row};
use super::state::{despawn_scoped, AppState};
use super::theme::{glyph, FontAssets, UiTheme, Z_MENU};
use super::widgets::{button_visual, text, text_colored};
use crate::session::{DefeatCause, ForceVictory, RunClock, RunOutcome};
use crate::site::o5::{ExpeditionReport, O5Standing, Rating};

/// Root marker for the victory screen (despawned on exit).
#[derive(Component)]
pub struct VictoryRoot;

/// Root marker for the game-over screen (despawned on exit).
#[derive(Component)]
pub struct GameOverRoot;

/// Root marker for the debrief screen (despawned on exit).
#[derive(Component)]
pub struct DebriefRoot;

pub struct DebriefPlugin;

impl Plugin for DebriefPlugin {
    fn build(&self, app: &mut App) {
        // Claim the session resources this plugin's systems READ. `init_resource`/`add_message` are
        // idempotent, so `SessionPlugin` still owns and writes them — this only guarantees a bare `App`
        // that adds `UiPlugin` alone (the UI-liveness test) cannot panic on a missing resource. Same
        // idiom, and same reason, as `UiPlugin`'s `init_resource::<DebugCaptureActive>()`.
        app.init_resource::<RunOutcome>()
            .init_resource::<RunClock>()
            .add_message::<ForceVictory>()
            .add_systems(OnEnter(AppState::Victory), spawn_victory)
            .add_systems(OnExit(AppState::Victory), despawn_scoped::<VictoryRoot>)
            .add_systems(OnEnter(AppState::GameOver), spawn_game_over)
            .add_systems(OnExit(AppState::GameOver), despawn_scoped::<GameOverRoot>)
            .add_systems(OnEnter(AppState::Debrief), spawn_debrief)
            .add_systems(OnExit(AppState::Debrief), despawn_scoped::<DebriefRoot>)
            .add_systems(
                Update,
                mirror_run_outcome.run_if(in_state(AppState::InGame)),
            );

        // Dev-only: reach the Victory/Debrief screens without playing the timer out. Debug-only, so the
        // shipped binary contains neither the hotkey nor a path to victory other than the win condition.
        #[cfg(debug_assertions)]
        app.add_systems(
            Update,
            dev_force_victory.run_if(in_state(AppState::InGame)),
        );
    }
}

/// Mirror the sim's [`RunOutcome`] onto the screen flow. The single reader-to-screen edge.
///
/// `set_if_neq` rather than `set`: this runs every frame the outcome is decided, and the transition
/// only lands at the next `StateTransition`, so a plain `set` would re-fire `OnExit`/`OnEnter` for the
/// state we are already entering — respawning the screen on top of itself.
fn mirror_run_outcome(outcome: Res<RunOutcome>, mut next: ResMut<NextState<AppState>>) {
    let screen = match *outcome {
        RunOutcome::Undecided => return,
        RunOutcome::Victory => AppState::Victory,
        RunOutcome::Defeat(_) => AppState::GameOver,
    };
    // Fully qualified ON PURPOSE. `ResMut<T>` implements `DetectChangesMut`, whose `set_if_neq`
    // **shadows** the inherent `NextState::set_if_neq` under method resolution — and it compares whole
    // `NextState` values, which are not `PartialEq`, so `next.set_if_neq(state)` does not even compile
    // (it reports a missing `PartialEq for NextState`, which reads like the state API is wrong rather
    // than like the wrong method was picked). This is the state-API footgun `BACKLOG.md` flagged for
    // re-confirmation; the fully-qualified call is the one that reaches the state machine.
    NextState::set_if_neq(&mut next, screen);
}

/// Dev hotkey (F10) — request a victory. Writes a *message* the sim's single writer consumes; it never
/// touches [`RunOutcome`] itself, and a real defeat still beats it (`session::decide`).
#[cfg(debug_assertions)]
fn dev_force_victory(keys: Res<ButtonInput<KeyCode>>, mut force: MessageWriter<ForceVictory>) {
    if keys.just_pressed(KeyCode::F10) {
        force.write(ForceVictory);
    }
}

/// The shared full-screen CRT panel every terminal screen sits in. `TabGroup` scopes the keyboard
/// navigation `UiPlugin` registers globally, so these screens need no per-screen nav wiring.
fn terminal_root<T: Component>(marker: T, theme: &UiTheme) -> impl Bundle {
    (
        marker,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: Val::Px(theme.space_lg),
            ..default()
        },
        BackgroundColor(theme.bg),
        GlobalZIndex(Z_MENU),
        TabGroup::new(0),
    )
}

fn spawn_victory(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    commands
        .spawn(terminal_root(VictoryRoot, &theme))
        .with_children(|p| {
            p.spawn(text_colored(&theme, &fonts, "SECURED", theme.font_title, theme.accent));
            p.spawn(text_colored(
                &theme,
                &fonts,
                "// SITE HELD — MTF EXTRACTING",
                theme.font_body,
                theme.text_muted,
            ));
            p.spawn(button_visual(&theme))
                .with_children(|b| {
                    b.spawn(text(&theme, &fonts, "DEBRIEF", theme.font_body));
                })
                .observe(|_: On<Activate>, mut next: ResMut<NextState<AppState>>| {
                    next.set(AppState::Debrief);
                });
        });
}

fn spawn_game_over(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    commands
        .spawn(terminal_root(GameOverRoot, &theme))
        .with_children(|p| {
            p.spawn(text_colored(&theme, &fonts, "MTF LOST", theme.font_title, theme.danger));
            p.spawn(text_colored(
                &theme,
                &fonts,
                "// NO SURVIVORS — SITE UNCONTAINED",
                theme.font_body,
                theme.text_muted,
            ));
            p.spawn(button_visual(&theme))
                .with_children(|b| {
                    b.spawn(text(&theme, &fonts, "DEBRIEF", theme.font_body));
                })
                .observe(|_: On<Activate>, mut next: ResMut<NextState<AppState>>| {
                    next.set(AppState::Debrief);
                });
        });
}

/// The debrief's body, as rows.
///
/// Pure, so the summary's *content* is testable without an `App` — and the content is the point of
/// this screen. Before FVS-L-7 it said only the verdict and a tick count, which is the shape of a
/// screen that has nothing to report.
///
/// **Written as changes, not levels** where a change exists. Andersen, Miller, Kiverstein & Deterding
/// 2022 (DOI 10.3389/fpsyg.2022.924953) argue players are "sensitive not just to absolute error, but
/// also to changes in the rate of error reduction" — the standing budget is a level the player cannot
/// act on, whereas *what this expedition earned* is the quantity that says whether the run was worth
/// running.
///
/// **It ends by naming the next goal.** Phan et al. 2016 found *"I always know my next goal when I
/// finish an event"* the weakest item in the GUESS usability subscale (M=5.46 of 7); the debrief is
/// literally the end of an event, so it is exactly where that failure would land.
fn debrief_rows(
    outcome: RunOutcome,
    ticks: u64,
    report: Option<ExpeditionReport>,
    earned: Option<u32>,
    budget: u32,
    rating: Option<Rating>,
) -> Vec<Row> {
    // The sim clock counts fixed ticks at the pinned 60 Hz (`lib::run`), so this is exact, not an
    // estimate off a wall clock that a pause or a slow frame would have skewed.
    let seconds = ticks as f64 / 60.0;
    let (verdict, glyph_for) = match outcome {
        RunOutcome::Undecided => ("RUN IN PROGRESS", glyph::LOCKED),
        RunOutcome::Victory => ("SECURED", glyph::DONE),
        RunOutcome::Defeat(DefeatCause::SquadWipe) => ("LOST — SQUAD WIPED", glyph::UNMET),
    };

    let mut rows = vec![
        Row::kv("OUTCOME", verdict)
            .with_glyph(glyph_for)
            .with_emphasis(Emphasis::Alert),
        Row::kv("EXPEDITION TIME", format!("{seconds:.1} s")),
    ];

    match report {
        Some(r) => {
            rows.push(Row::header("THE EXPEDITION"));
            // Losses, not survivors: the number the player feels is what it cost. A run that brought
            // everyone home reads `0`, which is the good news stated in the same units as the bad.
            let lost = r.squad_size.saturating_sub(r.survivors);
            rows.push(
                Row::kv("OPERATIVES LOST", format!("{lost} of {}", r.squad_size))
                    .with_glyph(if lost == 0 { glyph::MET } else { glyph::UNMET })
                    .with_emphasis(if lost == 0 { Emphasis::Muted } else { Emphasis::Alert }),
            );
            rows.push(Row::kv("ANOMALIES CONTAINED", r.captures.to_string()).with_glyph(
                if r.captures > 0 { glyph::MET } else { glyph::LOCKED },
            ));
            // A breach is a nest left standing — the thing that will still be there next time.
            rows.push(
                Row::kv("BREACHES LEFT OPEN", r.breaches.to_string())
                    .with_glyph(if r.breaches == 0 { glyph::MET } else { glyph::UNMET })
                    .with_emphasis(if r.breaches == 0 { Emphasis::Muted } else { Emphasis::Normal }),
            );
        }
        None => {
            // Explicitly, rather than an empty panel that reads as a bug.
            rows.push(Row::note("NO EXPEDITION ON FILE"));
        }
    }

    rows.push(Row::header("THE COUNCIL"));
    if let Some(rating) = rating {
        rows.push(Row::kv("RATING", format!("{rating:?}").to_uppercase()));
    }
    if let Some(earned) = earned {
        // The delta first, the level second — the earned figure is the one the run moved.
        rows.push(Row::kv("ALLOWANCE GRANTED", "").push(Cell::Delta(earned as f32)));
    }
    rows.push(Row::kv("O5 BUDGET", budget.to_string()).with_emphasis(Emphasis::Muted));

    // What now. Never absent, whatever happened.
    rows.push(Row::header("NEXT"));
    rows.push(Row::note(match outcome {
        RunOutcome::Victory => "RETURN TO SITE-67 — STUDY WHAT YOU BROUGHT BACK",
        _ => "RETURN TO SITE-67 — REQUISITION, THEN TRY AGAIN",
    }));
    rows
}

fn spawn_debrief(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    outcome: Res<RunOutcome>,
    clock: Res<RunClock>,
    standing: Option<Res<O5Standing>>,
) {
    // Optional: `O5Plugin` is registered in `lib::run` alongside `UiPlugin` but not by `UiPlugin`
    // itself, so the UI-liveness test's bare `App` has no standing. A missing `Res` PANICS in Bevy
    // 0.19, and the debrief degrades to the outcome and the clock rather than taking the app down.
    let (report, earned, budget, rating) = match standing.as_deref() {
        Some(s) => (s.last_report, s.last_allowance(), s.budget, s.last_rating),
        None => (None, None, 0, None),
    };
    let rows = debrief_rows(*outcome, clock.ticks, report, earned, budget, rating);

    commands
        .spawn(terminal_root(DebriefRoot, &theme))
        .with_children(|p| {
            p.spawn(text_colored(&theme, &fonts, "DEBRIEF", theme.font_title, theme.accent));
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    min_width: Val::Px(420.0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|body| spawn_rows(body, &theme, &fonts, &rows));
            p.spawn(button_visual(&theme))
                .with_children(|b| {
                    b.spawn(text(&theme, &fonts, "RETURN TO SITE", theme.font_body));
                })
                .observe(
                    |_: On<Activate>,
                     mut next: ResMut<NextState<AppState>>,
                     mut run: ResMut<NextState<crate::session::RunState>>| {
                        // The debrief is read OVER the finished world (the run stays `Active` through
                        // resolution), so leaving it is what tears that world down — see `session::RunState`.
                        run.set(crate::session::RunState::Idle);
                        // …and you land in SITE-67, not on a title card. This is the loop closing: the
                        // specimen you just extracted is already in a cell when you walk in, which is
                        // the whole reason the hub is a place rather than a menu (FVS-G-4/D-4).
                        //
                        // Note it is reachable from here and NOT from boot, deliberately: `Dungeon` is
                        // never removed, so once one expedition has run it exists (stale) for the rest
                        // of the process and nothing panics. Opening at the Site on a COLD start is the
                        // remaining case, and it is FVS-G-6.
                        next.set(AppState::Site);
                    },
                );
        });
}

#[cfg(test)]
mod debrief_tests {
    use super::*;

    fn flat(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| {
                let cells: Vec<String> = r
                    .cells
                    .iter()
                    .map(|c| match c {
                        Cell::Label(s) | Cell::Value(s) => s.clone(),
                        Cell::Delta(d) => super::super::rows::format_delta(*d),
                        Cell::Bar { frac } => format!("[bar {frac:.2}]"),
                    })
                    .collect();
                format!("{} {}", r.glyph, cells.join("  "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn report(squad: u32, survivors: u32, captures: u32, breaches: u32) -> ExpeditionReport {
        ExpeditionReport {
            squad_size: squad,
            survivors,
            captures,
            extracted: captures > 0,
            breaches,
        }
    }

    #[test]
    fn the_debrief_reports_what_actually_happened() {
        // The screen used to say only a verdict and a tick count — the shape of a summary with
        // nothing to summarise. Every term the Council rated the run on must be visible.
        let rows = debrief_rows(
            RunOutcome::Victory,
            5058,
            Some(report(5, 4, 2, 1)),
            Some(140),
            340,
            Some(Rating::Satisfactory),
        );
        let out = flat(&rows);
        for expected in [
            "OPERATIVES LOST",
            "ANOMALIES CONTAINED",
            "BREACHES LEFT OPEN",
            "O5 BUDGET",
            "RATING",
        ] {
            assert!(out.contains(expected), "the debrief must report {expected}:\n{out}");
        }
        assert!(out.contains("1 of 5"), "losses stated against the squad size:\n{out}");
    }

    #[test]
    fn the_allowance_is_shown_as_a_signed_change() {
        // The budget LEVEL is not actionable; what this expedition EARNED is. Andersen et al. 2022 —
        // the delta is the quantity the player responds to, so it must be unmistakably a delta.
        let rows = debrief_rows(
            RunOutcome::Victory,
            100,
            Some(report(5, 5, 1, 0)),
            Some(140),
            340,
            Some(Rating::Exemplary),
        );
        assert!(
            rows.iter().any(|r| r.cells.iter().any(|c| matches!(c, Cell::Delta(_)))),
            "the allowance must be a delta, not another level"
        );
        assert!(flat(&rows).contains("+140"), "and it must carry its sign: {}", flat(&rows));
    }

    #[test]
    fn the_debrief_always_names_the_next_goal() {
        // GUESS's weakest item industry-wide is "I always know my next goal when I finish an event".
        // The debrief IS the end of an event, so a silent one is that exact failure.
        for outcome in [
            RunOutcome::Victory,
            RunOutcome::Defeat(DefeatCause::SquadWipe),
            RunOutcome::Undecided,
        ] {
            let rows = debrief_rows(outcome, 10, None, None, 0, None);
            let out = flat(&rows);
            assert!(out.contains("NEXT"), "{outcome:?} debrief has no next-goal section:\n{out}");
            assert!(
                out.contains("SITE-67"),
                "{outcome:?} debrief must say where to go next:\n{out}"
            );
        }
    }

    #[test]
    fn a_clean_run_and_a_costly_one_do_not_read_the_same() {
        // Losses and breaches carry emphasis, so the cost of a run is seen rather than read. If a
        // flawless run and a mauled one produced identical emphasis, the summary would be a table
        // rather than a verdict.
        let clean = debrief_rows(RunOutcome::Victory, 10, Some(report(5, 5, 2, 0)), Some(140), 100, None);
        let costly = debrief_rows(RunOutcome::Victory, 10, Some(report(5, 1, 2, 3)), Some(90), 100, None);
        let loud = |rows: &[Row]| rows.iter().filter(|r| r.emphasis == Emphasis::Alert).count();
        assert!(
            loud(&costly) > loud(&clean),
            "a costly run must read louder: clean={} costly={}",
            loud(&clean),
            loud(&costly)
        );
    }

    #[test]
    fn a_missing_report_says_so_rather_than_going_blank() {
        // The UI-liveness test builds an App with no `O5Standing`. An empty body would read as a
        // broken screen; naming the absence reads as a state.
        let out = flat(&debrief_rows(RunOutcome::Victory, 10, None, None, 0, None));
        assert!(out.contains("NO EXPEDITION ON FILE"), "{out}");
    }
}
