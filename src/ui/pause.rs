//! Pause overlay. `Esc` toggles [`MenuState::Pause`] while in-game; entering it freezes the sim
//! (via [`super::state::sync_sim_blocked`] → `SimBlocked`) and dims the world. Restart Run waits on
//! the run-state/save phase (there's no world teardown yet).

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
        app.add_systems(Update, toggle_pause.run_if(in_state(AppState::InGame)))
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
    keys: Res<ButtonInput<KeyCode>>,
    menu: Res<State<MenuState>>,
    mut next: ResMut<NextState<MenuState>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
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

                // Quit to title
                c.spawn(button_visual(&theme))
                    .with_children(|b| {
                        b.spawn(text(&theme, &fonts, "QUIT TO TITLE", theme.font_body));
                    })
                    .observe(
                        |_: On<Activate>, mut next: ResMut<NextState<AppState>>| {
                            next.set(AppState::Title);
                        },
                    );
            });
        });
}
