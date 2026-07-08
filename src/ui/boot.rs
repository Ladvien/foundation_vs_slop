//! Boot screen: hold on a CRT "booting" card until UI fonts are ready, then advance to the title.
//!
//! With the embedded default font this resolves almost immediately; the frame budget below is a
//! defensive cap so boot can never hang waiting on an already-embedded asset.

use bevy::prelude::*;

use super::state::AppState;
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::text_colored;

/// Root marker for the boot screen (despawned on exit).
#[derive(Component)]
pub struct BootRoot;

/// Max frames to wait for fonts before advancing regardless — keeps boot from stalling.
const MAX_BOOT_FRAMES: u32 = 30;

pub struct BootScreenPlugin;

impl Plugin for BootScreenPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Boot), spawn_boot)
            .add_systems(
                OnExit(AppState::Boot),
                super::state::despawn_scoped::<BootRoot>,
            )
            .add_systems(Update, advance_when_ready.run_if(in_state(AppState::Boot)));
    }
}

fn spawn_boot(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    commands
        .spawn((
            BootRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(theme.bg),
            GlobalZIndex(Z_MENU),
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn(text_colored(
                &theme,
                &fonts,
                "◢ SCP-9191 CONTAINMENT FEED — INITIALIZING…",
                theme.font_body,
                theme.accent,
            ));
        });
}

fn advance_when_ready(
    assets: Res<AssetServer>,
    fonts: Res<FontAssets>,
    mut frames: Local<u32>,
    mut next: ResMut<NextState<AppState>>,
) {
    *frames += 1;
    let ready = assets.is_loaded_with_dependencies(&fonts.body);
    if ready || *frames >= MAX_BOOT_FRAMES {
        next.set(AppState::Title);
    }
}
