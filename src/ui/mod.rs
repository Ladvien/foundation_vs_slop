//! Game system UI — windowed-only, one plugin per surface.
//!
//! [`UiPlugin`] owns the whole UI stack: the [`state`] machine (`Boot → Title → InGame` +
//! in-game overlay substates), the CRT [`theme`], reusable [`widgets`], and one plugin per screen
//! ([`boot`], [`title`], [`pause`], [`hud`]).
//!
//! **Registered only in `lib::run` — never in the headless harness.** Consequences that keep the
//! deterministic core intact:
//! - Gameplay plugins are *not* gated on `in_state(AppState::InGame)`; the world generates under
//!   the boot/title screens and is frozen there via [`crate::time_control::SimBlocked`], which only
//!   this plugin writes ([`state::sync_sim_blocked`]).
//! - Every system here runs on `Update`/`OnEnter`/`OnExit`, never `FixedUpdate`, and only reads
//!   sim state — so nothing enters `snapshot_hash`.
//! - `UiWidgetsPlugins` is already added by `DefaultPlugins`, so it is intentionally **not** added
//!   here (double-adding a unique plugin panics).

use bevy::prelude::*;

pub mod boot;
pub mod hud;
pub mod pause;
pub mod settings_menu;
pub mod state;
pub mod theme;
pub mod title;
pub mod widgets;

use state::{AppState, MenuState, TitleMenu};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .add_sub_state::<MenuState>()
            .add_sub_state::<TitleMenu>()
            .add_plugins((
                crate::settings::SettingsPlugin,
                theme::UiThemePlugin,
                boot::BootScreenPlugin,
                title::TitlePlugin,
                pause::PauseMenuPlugin,
                settings_menu::SettingsMenuPlugin,
                hud::HudPlugin,
            ))
            // Sole writer of `SimBlocked`: freeze the sim under any blocking screen.
            .add_systems(Update, state::sync_sim_blocked)
            // Hover feedback for all themed menu buttons.
            .add_systems(Update, widgets::style_menu_buttons);
    }
}
