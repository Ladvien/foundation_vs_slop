//! UI lifecycle state machine.
//!
//! Two axes:
//! - [`AppState`] — top-level lifecycle: `Boot` (wait for UI assets) → `Title` → `Warmup` → `InGame`.
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
    /// Between "NEW RUN" and play: wait for the mold to finish colonising the dungeon
    /// ([`crate::mycelia::MoldWarm`]) so the player never watches it arrive. Usually passes straight
    /// through — the mold runs on `Time<Real>` and has been growing behind the boot and title screens.
    Warmup,
    /// Playing. Overlays are tracked by [`MenuState`].
    InGame,
    /// The run was won. Entered by `ui::debrief::mirror_run_outcome` when the sim's
    /// [`crate::session::RunOutcome`] resolves — never set directly by gameplay, which cannot see
    /// `AppState` at all (see the module note above and `crate::session`).
    Victory,
    /// The run was lost. Same one-way mirror as [`AppState::Victory`].
    GameOver,
    /// Post-run summary, reachable from both terminal screens; returns to [`AppState::Site`].
    Debrief,
    /// **Standing in Site-67** between expeditions (FVS-G-4). The persistent hub: operatives walk it,
    /// specimens are visibly held in the containment wing, and the ASYNC door is the only way out.
    ///
    /// The run state while here is `RunState::Idle`, so no expedition world exists — the Site is simply
    /// what is on screen when there is no run. Entering an expedition is walking an avatar into the
    /// door, which calls `NextState::set(RunState::Active)`; FVS-A-5 already implements that end to end,
    /// so the hub needs no state machinery of its own.
    Site,
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
    Controls,
    Roster,
    /// A modal dialogue exchange is in progress (see `crate::dialogue`). Blocking like the other
    /// overlays — it freezes the sim via [`sync_sim_blocked`] — but spawns no dim overlay of its own,
    /// so the in-world bubbles read over the live (frozen) scene.
    Conversation,
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
    /// The key list (`ui::controls_screen`). Reachable from the title as well as from the pause
    /// menu because it is an **Access** surface — a player who cannot work the controls has not
    /// started the game yet, so making them start one to read the keys is backwards.
    Controls,
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
    // The dev-only region-capture note box freezes the frame while the player types. Routed through
    // this single `SimBlocked` writer (not a second writer of `Time<Virtual>`/`GameSpeed`) so the one
    // pause path holds. Never present in release — the resource is only inserted by the debug tool.
    note_input: Option<Res<crate::NoteInputActive>>,
    // Per spec, **arming** the region capture (Ctrl/Cmd+P) auto-pauses so the player boxes a *still*
    // frame — not just while the note box is open afterwards. `DebugCaptureActive` is always compiled and
    // stays `false` in release (the dev tool that sets it is stripped), so this adds no release behaviour,
    // and it routes the arm-freeze through this one `SimBlocked` writer exactly like the note box — never a
    // second writer of `UserPaused`/`Time<Virtual>`.
    capture: Res<crate::DebugCaptureActive>,
    mut blocked: ResMut<SimBlocked>,
) {
    let menu_blocking = menu.map(|m| m.get().is_blocking()).unwrap_or(false);
    let want = should_freeze(capture.0, note_input.is_some(), app_state.get(), menu_blocking);
    if blocked.0 != want {
        blocked.0 = want;
    }
}

/// Pure freeze decision for [`sync_sim_blocked`], split out so the single-writer freeze rule — including
/// the spec's "arming Cmd+P auto-pauses" — is unit-testable without an `App`. The sim freezes when the
/// region capture is **armed** (`capture_active`), while the note box is open (`note_open`), during
/// boot/title/warmup, or while a blocking in-game menu is up.
fn should_freeze(capture_active: bool, note_open: bool, app_state: &AppState, menu_blocking: bool) -> bool {
    capture_active
        || note_open
        || match app_state {
            AppState::Boot | AppState::Title | AppState::Warmup => true,
            // The Site must NOT freeze. Freezing sets `SimBlocked`, which gates `camera::drive_camera`
            // and the Site's own avatar mover — the hub would render as a still photograph you cannot
            // walk around in. There is no expedition running to freeze anyway: `RunState` is `Idle`
            // here, so nothing pinned is ticking that we would want stopped.
            AppState::Site => false,
            // The run is over: freeze the world behind the terminal screens so the last frame the
            // player saw is the one they read the verdict over. The sim would otherwise keep ticking
            // under the Debrief (crabs still walking around a dead squad).
            AppState::Victory | AppState::GameOver | AppState::Debrief => true,
            AppState::InGame => menu_blocking,
        }
}

#[cfg(test)]
mod freeze_tests {
    use super::*;

    /// Pins the spec requirement: arming the region capture (Ctrl/Cmd+P) auto-pauses the sim — and the
    /// release-safety invariant that normal play does not freeze (so a stripped dev tool changes nothing).
    #[test]
    fn arming_region_capture_freezes_the_sim() {
        // The spec: an armed capture freezes the frame so the box is drawn on a still image.
        assert!(should_freeze(true, false, &AppState::InGame, false), "armed capture must freeze");
        // Existing behaviour: the note box freezes while typing.
        assert!(should_freeze(false, true, &AppState::InGame, false), "open note box must freeze");
        // Release-safe / normal play: nothing armed, no note, in game, no menu → the sim runs.
        assert!(!should_freeze(false, false, &AppState::InGame, false), "normal play must NOT freeze");
        // Boot/title/warmup always freeze; a blocking in-game menu freezes.
        assert!(should_freeze(false, false, &AppState::Title, false));
        // The Site is the one non-playing state that must stay LIVE: freezing sets `SimBlocked`, which
        // gates the camera and the avatar mover, and a hub you cannot walk around in is a photograph.
        assert!(
            !should_freeze(false, false, &AppState::Site, false),
            "Site-67 must not freeze — the player walks around in it"
        );
        assert!(should_freeze(false, false, &AppState::InGame, true));
    }
}
