//! Title screen. Minimal for now — a CRT title card that starts the game on Enter/click. The full
//! New Run / Continue / Settings / Quit menu (with seed entry) lands in the menus phase; this
//! establishes the real `Title → InGame` transition the rest of the UI builds on.

use bevy::prelude::*;

use super::state::AppState;
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::{text_colored, text};

/// Root marker for the title screen (despawned on exit).
#[derive(Component)]
pub struct TitleRoot;

pub struct TitlePlugin;

impl Plugin for TitlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Title), spawn_title)
            .add_systems(
                OnExit(AppState::Title),
                super::state::despawn_scoped::<TitleRoot>,
            )
            .add_systems(Update, start_on_input.run_if(in_state(AppState::Title)));
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
            p.spawn(text(
                &theme,
                &fonts,
                "[ PRESS ENTER OR CLICK TO BEGIN ]",
                theme.font_body,
            ));
        });
}

fn start_on_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if keys.just_pressed(KeyCode::Enter)
        || keys.just_pressed(KeyCode::NumpadEnter)
        || mouse.just_pressed(MouseButton::Left)
    {
        next.set(AppState::InGame);
    }
}
