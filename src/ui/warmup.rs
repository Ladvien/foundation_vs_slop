//! Warmup screen: hold a CRT card between "NEW RUN" and play while the mold finishes colonising.
//!
//! The mycelium is meant to have been in these corridors for years. At the shipped `sim_hz` a colony takes
//! minutes to spread, so a player dropped straight into a fresh dungeon would watch it arrive — the one
//! thing the whole perceptual speed limit exists to prevent. `mycelia` therefore runs its
//! `warmup_ticks` as fast as the GPU accepts them and raises [`MoldWarm`] when the colony is established.
//!
//! This screen is only the *backstop*. The mold advances on `Time<Real>`, so it is already colonising
//! underneath the boot and title cards while the player reads the menu; by the time most players click
//! through, [`MoldWarm`] is set and this state passes through in a single frame. It earns its keep for the
//! player who clicks NEW RUN immediately.
//!
//! No frame cap, deliberately, unlike [`super::boot`]. Boot waits on an *embedded* font that is essentially
//! always ready, so a cap there is a cheap guard against a stall. Here the wait is on real GPU work whose
//! duration depends on the machine; advancing early on a timer would hand the player exactly the bare carpet
//! this screen exists to hide. A mold that never warms means the compute chain never dispatched, and that
//! path already panics loudly in `mycelia::pipeline`.

use bevy::prelude::*;

use crate::mycelia::MoldWarm;

use super::state::AppState;
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::text_colored;

/// Root marker for the warmup screen (despawned on exit).
#[derive(Component)]
pub struct WarmupRoot;

pub struct WarmupScreenPlugin;

impl Plugin for WarmupScreenPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Warmup), spawn_warmup)
            .add_systems(
                OnExit(AppState::Warmup),
                super::state::despawn_scoped::<WarmupRoot>,
            )
            .add_systems(Update, advance_when_warm.run_if(in_state(AppState::Warmup)));
    }
}

fn spawn_warmup(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    commands
        .spawn((
            WarmupRoot,
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
                "◢ SCP-9191 — CULTURING SUBSTRATE…",
                theme.font_body,
                theme.accent,
            ));
        });
}

/// Advance to play the moment the colony is established. `MoldWarm` is usually already `true` on the first
/// frame of this state, and then this screen is never really seen.
fn advance_when_warm(warm: Res<MoldWarm>, mut next: ResMut<NextState<AppState>>) {
    if warm.0 {
        next.set(AppState::InGame);
    }
}
