//! Title screen — CRT title card with New Run / Settings / Quit. Seed entry and Continue (which
//! needs the save system) come with their gated phases; this is the real main menu the rest of the
//! flow hangs off.

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
        app.add_systems(OnEnter(AppState::Title), spawn_title)
            .add_systems(
                OnExit(AppState::Title),
                super::state::despawn_scoped::<TitleRoot>,
            )
            .add_systems(
                Update,
                start_on_enter_key.run_if(in_state(TitleMenu::Root)),
            );
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

            // New Run
            p.spawn(button_visual(&theme))
                .with_children(|b| {
                    b.spawn(text(&theme, &fonts, "NEW RUN", theme.font_body));
                })
                .observe(
                    |_: On<Activate>, mut next: ResMut<NextState<AppState>>| {
                        next.set(AppState::InGame);
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

/// Convenience: `Enter` starts a new run from the root title (not while the settings panel is up).
fn start_on_enter_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        next.set(AppState::InGame);
    }
}
