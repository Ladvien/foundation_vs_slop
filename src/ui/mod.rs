//! Game system UI — windowed-only, one plugin per surface.
//!
//! [`UiPlugin`] owns the whole UI stack: the [`state`] machine (`Boot → Title → Warmup → InGame`,
//! plus the terminal `Victory`/`GameOver`/`Debrief` screens + in-game overlay substates), the CRT
//! [`theme`], reusable [`widgets`], and one plugin per screen ([`boot`], [`title`], [`warmup`],
//! [`pause`], [`hud`], [`debrief`]).
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
pub mod briefing;
pub mod containment_hud;
pub mod research_hud;
pub mod site_hud;
pub mod verb_bar;
pub mod debrief;
pub mod hud;
pub mod layout;
pub mod pause;
pub mod rows;
pub mod settings_menu;
pub mod state;
pub mod theme;
pub mod title;
pub mod warmup;
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
                // Owns the 3×3 region grid every panel is parented into. Must be added before the
                // panel plugins so `OnEnter` ordering can name `layout::spawn_frame`.
                layout::HudLayoutPlugin,
                boot::BootScreenPlugin,
                title::TitlePlugin,
                warmup::WarmupScreenPlugin,
                pause::PauseMenuPlugin,
                settings_menu::SettingsMenuPlugin,
                hud::HudPlugin,
                // Terminal screens. Presentation only — the win/lose decision is `crate::session`,
                // inside the deterministic core; this plugin mirrors it one-way onto `AppState`.
                debrief::DebriefPlugin,
                // Reads *why* a containment is progressing or breaking (FVS-L-1).
                containment_hud::ContainmentHudPlugin,
                verb_bar::VerbBarPlugin,
                research_hud::ResearchHudPlugin,
            site_hud::SiteHudPlugin,
            ))
            // `state::sync_sim_blocked` reads `DebugCaptureActive` non-optionally (it is documented as
            // "always compiled … stays false in release"), so the plugin that registers the reader is what
            // guarantees the resource exists. Init here rather than in `lib::run` alone: `UiPlugin` is also
            // added to a bare `App` by the UI-liveness test, which otherwise panics on a missing resource.
            // `init_resource` is idempotent, so a plugin can safely claim every resource its systems read.
            .init_resource::<crate::DebugCaptureActive>()
            // Sole writer of `SimBlocked`: freeze the sim under any blocking screen.
            .add_systems(Update, state::sync_sim_blocked)
            // Shared menu behavior for every screen — registered once, globally. Each no-ops when no
            // menu is open, so a new screen needs no per-screen nav/focus wiring (and none can be
            // forgotten): hover/keyboard/NumpadEnter selection, the hover+focus tint, and dropping
            // stale focus when the last menu closes.
            .add_systems(
                Update,
                (
                    widgets::style_menu_buttons,
                    widgets::menu_keyboard_nav,
                    widgets::focus_hovered_menu_button,
                    widgets::menu_activate_numpad_enter,
                    widgets::clear_menu_focus_when_empty,
                ),
            );
    }
}
