//! Title screen — CRT title card with New Run / Settings / Quit. Seed entry and Continue (which
//! needs the save system) come with their gated phases; this is the real main menu the rest of the
//! flow hangs off.

use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::state::{AppState, TitleMenu};
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::{button_visual, text, text_colored};

/// Root marker for the title screen (despawned on exit).
#[derive(Component)]
pub struct TitleRoot;

pub struct TitlePlugin;

impl Plugin for TitlePlugin {
    fn build(&self, app: &mut App) {
        // Keyboard navigation (Up/Down/W-S to move, Enter/Space/NumpadEnter to activate) and focus
        // cleanup are handled globally in `UiPlugin` for every menu screen — this screen only needs
        // to spawn its buttons inside a `TabGroup` (see `spawn_title`).
        app.add_systems(OnEnter(AppState::Title), spawn_title)
            .add_systems(OnExit(AppState::Title), super::state::despawn_scoped::<TitleRoot>)
            // `FVS_AUTORUN=1` presses NEW RUN for you. It exists so an unattended **measurement** run
            // can reach live gameplay — `perf_probe` samples nothing useful on a title screen, and
            // this box has no `xdotool` to click with.
            //
            // Deliberately NOT a shortcut that skips any of the transition: it performs exactly the
            // two state sets the button's observer performs, so an auto-started run is the same run a
            // player would get (same `RunSeed` advance, same warmup). A cheaper "jump straight to
            // InGame" would measure a world that never existed.
            .add_systems(OnEnter(AppState::Title), autorun.after(spawn_title));
    }
}

fn spawn_title(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    commands
        .spawn((
            TitleRoot,
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
            // Scopes keyboard nav to this screen's buttons (their `TabIndex` is inert without it).
            TabGroup::new(0),
        ))
        .with_children(|p| {
            p.spawn(text_colored(
                &theme,
                &fonts,
                "FOUNDATION vs. SLOP",
                theme.font_title,
                theme.accent,
            ));
            p.spawn(text_colored(
                &theme,
                &fonts,
                "// SCP-9191 CONTAINMENT SITE — WATCH FEED",
                theme.font_body,
                theme.text_muted,
            ));

            // New Run. Via `Warmup`, which waits for the mold to finish colonising the dungeon before the
            // player ever sees it — usually a single frame, since the colony grows behind this very screen.
            p.spawn(button_visual(&theme))
                .with_children(|b| {
                    b.spawn(text(&theme, &fonts, "NEW RUN", theme.font_body));
                })
                .observe(
                    |_: On<Activate>,
                     mut next: ResMut<NextState<AppState>>,
                     cur_run: Res<State<crate::session::RunState>>,
                     mut run: ResMut<NextState<crate::session::RunState>>| {
                        // Start a fresh expedition. `RunState::Idle → Active` rebuilds the world from the
                        // advanced `RunSeed`, so this button now means what it says (FVS-A-5); before, the
                        // world was built once at `Startup` and NEW RUN resumed the used one.
                        // Guard against a SAME-STATE transition, and note this compares the CURRENT
                        // state — not the pending one.
                        //
                        // Boot already leaves `Idle` for `Active` (`session::begin_first_run`), so at
                        // the title the run state is ALREADY `Active`. A plain `set` to the state we
                        // are in fires `OnExit(Active)` + `OnEnter(Active)`, tearing down and rebuilding
                        // the entire world for nothing — the trap BACKLOG §2 names ("`DespawnOnExit`/
                        // `DespawnOnEnter` fire on same-state transitions"). It is how the windowed
                        // build crashed on 2026-07-26: the redundant rebuild ran `setup_mycelia` a
                        // second time and `gate_coarse_readback` then found two `CoarseReadback`
                        // entities.
                        //
                        // `NextState::set_if_neq` is the WRONG tool here even fully qualified (see
                        // `ui::debrief`'s note on the `DetectChangesMut` shadowing): it compares the
                        // *pending* value, and a fresh `NextState` is `Unchanged`, so it would happily
                        // queue the same-state transition anyway.
                        //
                        // After `QUIT TO TITLE` the state really is `Idle`, so a genuine new expedition
                        // still transitions and still advances the `RunSeed` (FVS-A-5).
                        if *cur_run.get() != crate::session::RunState::Active {
                            run.set(crate::session::RunState::Active);
                        }
                        next.set(AppState::Warmup);
                    },
                );

            // Settings
            p.spawn(button_visual(&theme))
                .with_children(|b| {
                    b.spawn(text(&theme, &fonts, "SETTINGS", theme.font_body));
                })
                .observe(
                    |_: On<Activate>, mut next: ResMut<NextState<TitleMenu>>| {
                        next.set(TitleMenu::Settings);
                    },
                );

            // Controls — reachable BEFORE starting a run, deliberately. A player who cannot work
            // the controls has not started the game yet (`ui::controls_screen`).
            p.spawn(button_visual(&theme))
                .with_children(|b| {
                    b.spawn(text(&theme, &fonts, "CONTROLS", theme.font_body));
                })
                .observe(
                    |_: On<Activate>, mut next: ResMut<NextState<TitleMenu>>| {
                        next.set(TitleMenu::Controls);
                    },
                );

            // Quit
            p.spawn(button_visual(&theme))
                .with_children(|b| {
                    b.spawn(text(&theme, &fonts, "QUIT", theme.font_body));
                })
                .observe(|_: On<Activate>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                });
        });
}

/// Take the NEW RUN transition automatically when `FVS_AUTORUN=1`. See the registration note.
fn autorun(
    mut next: ResMut<NextState<AppState>>,
    mut run: ResMut<NextState<crate::session::RunState>>,
    cur_run: Res<State<crate::session::RunState>>,
) {
    if std::env::var("FVS_AUTORUN").as_deref() != Ok("1") {
        return;
    }
    info!("title: FVS_AUTORUN=1 — starting a run without waiting for input");
    if *cur_run.get() != crate::session::RunState::Active {
        run.set(crate::session::RunState::Active);
    }
    next.set(AppState::Warmup);
}
