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

use super::state::{despawn_scoped, AppState};
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::{button_visual, text, text_colored};
use crate::session::{DefeatCause, ForceVictory, RunClock, RunOutcome};

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

fn spawn_debrief(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    outcome: Res<RunOutcome>,
    clock: Res<RunClock>,
) {
    // The sim clock counts fixed ticks at the pinned 60 Hz (`lib::run`), so this is exact, not an
    // estimate off a wall clock that a pause or a slow frame would have skewed.
    let seconds = clock.ticks as f64 / 60.0;
    let verdict = match *outcome {
        RunOutcome::Undecided => "RUN IN PROGRESS",
        RunOutcome::Victory => "OUTCOME: SECURED",
        RunOutcome::Defeat(DefeatCause::SquadWipe) => "OUTCOME: LOST — SQUAD WIPED",
    };
    commands
        .spawn(terminal_root(DebriefRoot, &theme))
        .with_children(|p| {
            p.spawn(text_colored(&theme, &fonts, "DEBRIEF", theme.font_title, theme.accent));
            p.spawn(text_colored(&theme, &fonts, verdict, theme.font_body, theme.text));
            p.spawn(text_colored(
                &theme,
                &fonts,
                format!("EXPEDITION TIME: {seconds:.1} s ({} ticks)", clock.ticks),
                theme.font_body,
                theme.text_muted,
            ));
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
