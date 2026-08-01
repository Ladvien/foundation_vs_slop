//! Pause overlay. `Esc` toggles [`MenuState::Pause`] while in-game; entering it freezes the sim
//! (via [`super::state::sync_sim_blocked`] → `SimBlocked`) and dims the world. Restart Run waits on
//! the run-state/save phase (there's no world teardown yet).

use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::state::{AppState, MenuState};
use super::theme::{FontAssets, UiTheme, Z_MENU, Z_MENU_DIM};
use super::widgets::{button_visual, text, text_colored};

/// Root marker for the pause overlay (despawned on exit).
#[derive(Component)]
pub struct PauseRoot;

pub struct PauseMenuPlugin;

impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        // Keyboard navigation and focus cleanup are handled globally in `UiPlugin` for every menu
        // screen; this plugin only owns the Esc toggle and the overlay's own lifecycle. Its buttons
        // live inside a `TabGroup` (see `spawn_pause`) so the shared nav can reach them.
        // Gate Escape off while the dev-only region-capture note box is open, so it finalizes the note
        // instead of opening the pause overlay. `NoteInputActive` is never present in release.
        app.add_systems(
            Update,
            toggle_pause
                .run_if(in_state(AppState::InGame))
                .run_if(not(resource_exists::<crate::NoteInputActive>)),
        )
            .add_systems(OnEnter(MenuState::Pause), spawn_pause)
            .add_systems(
                OnExit(MenuState::Pause),
                super::state::despawn_scoped::<PauseRoot>,
            );
    }
}

/// `Esc` opens the pause overlay from play, or closes it from the overlay. Settings/roster have
/// their own screens.
fn toggle_pause(
    actions: crate::input::Actions,
    menu: Res<State<MenuState>>,
    mut next: ResMut<NextState<MenuState>>,
) {
    if !actions.just_pressed(crate::input::Action::PauseMenu) {
        return;
    }
    match menu.get() {
        MenuState::Closed => next.set(MenuState::Pause),
        MenuState::Pause => next.set(MenuState::Closed),
        _ => {}
    }
}

fn spawn_pause(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    commands
        .spawn((
            PauseRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(theme.space_md),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            GlobalZIndex(Z_MENU_DIM),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    padding: UiRect::all(Val::Px(theme.space_lg)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(theme.space_md),
                    ..default()
                },
                BackgroundColor(theme.panel),
                super::widgets::border_all(theme.panel_border),
                GlobalZIndex(Z_MENU),
                // Scopes keyboard nav to this overlay's buttons (their `TabIndex` is inert without it).
                TabGroup::new(0),
            ))
            .with_children(|c| {
                c.spawn(text_colored(
                    &theme,
                    &fonts,
                    "— PAUSED —",
                    theme.font_title * 0.5,
                    theme.accent,
                ));

                // Resume
                c.spawn(button_visual(&theme))
                    .with_children(|b| {
                        b.spawn(text(&theme, &fonts, "RESUME", theme.font_body));
                    })
                    .observe(
                        |_: On<Activate>, mut next: ResMut<NextState<MenuState>>| {
                            next.set(MenuState::Closed);
                        },
                    );

                // Settings
                c.spawn(button_visual(&theme))
                    .with_children(|b| {
                        b.spawn(text(&theme, &fonts, "SETTINGS", theme.font_body));
                    })
                    .observe(
                        |_: On<Activate>, mut next: ResMut<NextState<MenuState>>| {
                            next.set(MenuState::Settings);
                        },
                    );

                // Controls
                c.spawn(button_visual(&theme))
                    .with_children(|b| {
                        b.spawn(text(&theme, &fonts, "CONTROLS", theme.font_body));
                    })
                    .observe(
                        |_: On<Activate>, mut next: ResMut<NextState<MenuState>>| {
                            next.set(MenuState::Controls);
                        },
                    );

                // Abandon the expedition — the deliberate END, as opposed to a visit.
                //
                // `input::Action::VisitSite` walks to the Site with the run still live; this is the
                // other verb, and the two must stay visibly different because they are one keystroke
                // apart and only one of them is recoverable. Ending the run here is what fires
                // `OnExit(Active)`: the world despawns via `run_scoped()`, `advance_to_next_world`
                // picks the next Branch universe, and `persist::save_campaign` banks the campaign.
                c.spawn(button_visual(&theme))
                    .with_children(|b| {
                        b.spawn(text(&theme, &fonts, "ABANDON EXPEDITION", theme.font_body));
                    })
                    .observe(
                        |_: On<Activate>,
                         mut next: ResMut<NextState<AppState>>,
                         mut run: ResMut<NextState<crate::session::RunState>>| {
                            run.set(crate::session::RunState::Idle);
                            // …and you land at SITE-67, the same place a resolved run leaves you.
                            // Abandoning is a way to *end* an expedition, not to leave the game.
                            next.set(AppState::Site);
                        },
                    );

                // Quit to title
                c.spawn(button_visual(&theme))
                    .with_children(|b| {
                        b.spawn(text(&theme, &fonts, "QUIT TO TITLE", theme.font_body));
                    })
                    .observe(
                        |_: On<Activate>,
                         mut next: ResMut<NextState<AppState>>,
                         mut run: ResMut<NextState<crate::session::RunState>>| {
                            // Abandoning the run ends it: `OnExit(Active)` despawns the world and advances
                            // the seed, so the next NEW RUN is a different map (FVS-A-5).
                            run.set(crate::session::RunState::Idle);
                            next.set(AppState::Title);
                        },
                    );
            });
        });
}
