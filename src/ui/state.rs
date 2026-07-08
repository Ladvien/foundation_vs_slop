//! UI lifecycle state machine.
//!
//! Two axes:
//! - [`AppState`] — top-level lifecycle: `Boot` (wait for UI assets) → `Title` → `InGame`.
//! - [`MenuState`] — in-game overlay stack (pause / settings / roster), a **substate** that only
//!   exists while [`AppState::InGame`]. [`MenuState::Closed`] means "playing, no overlay".
//! - [`TitleMenu`] — a tiny substate of [`AppState::Title`] so the *same* settings panel can be
//!   reached from the title screen and from the in-game pause menu.
//!
//! **Determinism note:** these states, and every system gated on them, live only in the windowed
//! build (`UiPlugin`, registered in `lib::run`). The headless replay harness never registers
//! `AppState`, so gameplay plugins must **not** be gated on `in_state(AppState::InGame)` — they
//! keep booting on `Startup`/`FixedUpdate` exactly as before. The world generates *under* the
//! boot/title screens and is held frozen there via [`crate::time_control::SimBlocked`].

use bevy::prelude::*;

use crate::time_control::SimBlocked;

#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
pub enum AppState {
    /// Waiting on UI assets (fonts) to be ready before the first text frame renders.
    #[default]
    Boot,
    /// Main menu / title card. The already-generated world sits frozen behind it.
    Title,
    /// Playing. Overlays are tracked by [`MenuState`].
    InGame,
}

/// In-game overlay stack. Only exists while [`AppState::InGame`] (a Bevy substate).
#[derive(SubStates, Default, Debug, Clone, PartialEq, Eq, Hash)]
#[source(AppState = AppState::InGame)]
pub enum MenuState {
    /// Playing, no overlay open.
    #[default]
    Closed,
    Pause,
    Settings,
    Roster,
}

impl MenuState {
    /// Whether this overlay blocks play (freezes the sim, dims the world). Every non-`Closed`
    /// overlay is blocking today; kept as a method so the policy has one home.
    pub fn is_blocking(&self) -> bool {
        !matches!(self, MenuState::Closed)
    }
}

/// Title-screen substate so the shared settings panel is reachable from the title too.
#[derive(SubStates, Default, Debug, Clone, PartialEq, Eq, Hash)]
#[source(AppState = AppState::Title)]
pub enum TitleMenu {
    #[default]
    Root,
    Settings,
}

/// Generic "despawn on screen exit": remove every entity tagged with the screen-root marker `T`
/// (children despawn with it). Register as `OnExit(state)` with the screen's root marker, e.g.
/// `add_systems(OnExit(AppState::Title), despawn_scoped::<TitleRoot>)`.
pub fn despawn_scoped<T: Component>(mut commands: Commands, roots: Query<Entity, With<T>>) {
    for e in &roots {
        commands.entity(e).despawn();
    }
}

/// Single writer of [`SimBlocked`]: freeze the sim whenever a blocking screen is up — during boot,
/// on the title, or while an in-game overlay is open. Runs only in the windowed build, so the
/// harness never touches `SimBlocked`.
pub fn sync_sim_blocked(
    app_state: Res<State<AppState>>,
    // `State<MenuState>` only exists while `InGame` (it's a substate); absent otherwise.
    menu: Option<Res<State<MenuState>>>,
    mut blocked: ResMut<SimBlocked>,
) {
    let want = match app_state.get() {
        AppState::Boot | AppState::Title => true,
        AppState::InGame => menu.map(|m| m.get().is_blocking()).unwrap_or(false),
    };
    if blocked.0 != want {
        blocked.0 = want;
    }
}
