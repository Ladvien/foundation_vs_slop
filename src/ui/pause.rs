//! Pause overlay. `Esc` toggles [`MenuState::Pause`] while in-game; entering it freezes the sim
//! (via [`super::state::sync_sim_blocked`] → `SimBlocked`) and dims the world. Full pause menu
//! buttons (Resume / Settings / Save & Quit) land in the menus phase; this wires the real
//! blocking-overlay behaviour.

use bevy::prelude::*;

use super::state::{AppState, MenuState};
use super::theme::{FontAssets, UiTheme, Z_MENU, Z_MENU_DIM};
use super::widgets::{text, text_colored};

/// Root marker for the pause overlay (despawned on exit).
#[derive(Component)]
pub struct PauseRoot;

pub struct PauseMenuPlugin;

impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            toggle_pause.run_if(in_state(AppState::InGame)),
        )
        .add_systems(OnEnter(MenuState::Pause), spawn_pause)
        .add_systems(
            OnExit(MenuState::Pause),
            super::state::despawn_scoped::<PauseRoot>,
        );
    }
}

/// `Esc` opens the pause overlay from play, or closes it from the overlay. Other overlays
/// (settings/roster) are left to the menus phase.
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
        // Leave settings/roster handling to their own screens.
        _ => {}
    }
}

fn spawn_pause(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    // Full-screen dim behind the text.
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
                    row_gap: Val::Px(theme.space_sm),
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
                c.spawn(text(
                    &theme,
                    &fonts,
                    "Esc to resume",
                    theme.font_body,
                ));
            });
        });
}
