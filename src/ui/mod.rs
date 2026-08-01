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
pub mod hint;
pub mod controls_screen;
pub mod event_line;
pub mod research_hud;
pub mod site_hud;
pub mod verb_bar;
pub mod debrief;
pub mod hud;
pub mod layout;
pub mod minimap;
pub mod offscreen;
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
                // Nested rather than given their own slots: the top-level tuple is at Bevy's
                // 15-element cap (`docs/ui.md` §5). Grouped because they are all Access-side
                // surfaces — settings, keys, and where things are.
                (
                    // **Do not remove from this tuple.** `title.rs` and `pause.rs` both set
                    // `TitleMenu::Settings` / `MenuState::Settings`, and with no plugin registered
                    // those transitions despawn the pause overlay, spawn nothing, and leave the sim
                    // frozen with neither Escape nor a button to get out — a soft-lock. It was
                    // dropped once, by an edit that rewrote this tuple to respect the 15-element cap.
                    settings_menu::SettingsMenuPlugin,
                    // The key list, reachable from both the title and the pause menu.
                    controls_screen::ControlsScreenPlugin,
                    // Edge markers for the extraction point and the selected operatives. The camera
                    // follows nothing by design; these keep "follows nothing" from meaning "cannot
                    // find them again".
                    offscreen::OffscreenIndicatorPlugin,
                    // Topology of what you have seen — visible only while a sensor drone is live.
                    minimap::MinimapPlugin,
                    // One transient line for the thing that just happened. The game had no log,
                    // toast or notification of any kind before this.
                    event_line::EventLinePlugin,
                    // Teaching lines for the two verbs that move the player between the expedition
                    // and Site-67 — the Tab toggle and the ASYNC door. Grouped here rather than
                    // given a top-level slot both because the outer tuple is at the 15-element cap
                    // and because it belongs: this is an Access-side surface in exactly the sense
                    // `controls_screen` is, and it retires once learned.
                    hint::HintPlugin,
                ),
                hud::HudPlugin,
                // Terminal screens. Presentation only — the win/lose decision is `crate::session`,
                // inside the deterministic core; this plugin mirrors it one-way onto `AppState`.
                debrief::DebriefPlugin,
                // Reads *why* a containment is progressing or breaking (FVS-L-1).
                containment_hud::ContainmentHudPlugin,
                // ...and *where* it is happening (FVS-K-1). The panel names the rule; this draws the
                // cordon, the held anomaly, and the breach the panel can only report by disappearing.
                crate::containment::cordon::CordonFeedbackPlugin,
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
            // **`RunFixedMainLoop`, not `Update`** — unlike its sibling. `selection`'s order-issuing
            // input runs in `BeforeFixedMainLoop`, which sits *ahead* of `Update` in the main schedule
            // order (First → PreUpdate → StateTransition → RunFixedMainLoop → Update). A writer on
            // `Update` would therefore be read one frame stale, leaving a single frame after the player
            // presses VISIT SITE in which a click still commands the squad. `StateTransition` runs
            // before this, so the `AppState` read here is the fresh one.
            .add_systems(
                RunFixedMainLoop,
                state::sync_order_block.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
            )
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
