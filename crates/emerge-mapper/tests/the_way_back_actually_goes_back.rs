//! **The way back out of a door has to actually go back.**
//!
//! Source ratchets, not behaviour tests, and they exist because the defect they guard is invisible
//! to a green suite. On 2026-08-16 `Cmd+O`, the `‹ kits & maps` button and the `Esc` peel all killed
//! the application instead of returning to the menu — while two headless tests asserting the way
//! back stayed green, because both asserted on the *message* the editor wrote and the message was
//! written correctly every time. It was dropped afterwards.
//!
//! # These moved with the mechanism, and that is the point
//!
//! They used to pin a **process** design: a supervisor, one menu process per lap, `--choose`, and an
//! exit code of 64 the parent compared against. That shape existed because winit builds at most one
//! `EventLoop` per process, so a menu you could return to had to be a fresh process.
//!
//! Both screens are one application now (`src/screen.rs`), asked for at the keyboard on the same
//! day. The property these tests defend is unchanged — *leaving a door lands you on the menu* — so
//! they now pin the state machine that carries it. A ratchet that kept naming the old mechanism
//! would fail for being out of date rather than for the thing going wrong, which is the fastest way
//! to teach somebody to delete it.

use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// **`main` still hands its exit code back.**
///
/// Less load-bearing than it was — the way back is no longer an exit code — but a `main` that drops
/// `AppExit` still lies to `emerge-mapper && something`, and this is the cheapest possible guard.
#[test]
fn the_editor_hands_its_exit_code_back() {
    let src = read("src/main.rs");
    assert!(
        src.contains("fn main() -> AppExit"),
        "`main` must return the exit code rather than `()`."
    );
    assert!(
        src.contains("\n    app.run()\n}"),
        "`main` must END with `app.run()` and no semicolon. A discarded `AppExit` is the defect \
         this file exists for: the message is written, the message is dropped, and tests asserting \
         the message stay green while nothing happens."
    );
}

/// **Leaving a door sets the state back to the menu.**
///
/// The successor to the exit-code check. Three keys reach `leave_for_menu`'s answer — `Esc` on a
/// clean map, `D` on a dirty one, and `S` after a save — and each has to end on the menu. All three
/// name the same state, and nothing else in `editor.rs` does, so this counts them.
#[test]
fn every_way_out_of_a_door_lands_on_the_menu() {
    let src = read("src/editor.rs");
    let ways = src.matches("next.set(crate::screen::Screen::Menu)").count();
    assert!(
        ways >= 3,
        "every way out of a door must set `Screen::Menu`; found {ways}. The three are `Esc` on a \
         clean map, `D` on a dirty one, and `S` when the save succeeds — and a way out that forgets \
         it leaves the author in a door with no way back."
    );
    assert!(
        !src.contains("BACK_TO_MENU"),
        "the exit code is gone with the process boundary; naming it again would be a second way to \
         say `Screen::Menu`."
    );
}

/// **The menu is a state, not a second process.**
///
/// The whole point of the 2026-08-16 rebuild — *"can we not open a whole another editing window? I'd
/// like to keep the same bevy application running."* One `App`, one window, two screens.
///
/// Pinned by absence as well as presence: `main` re-spawning itself is exactly what came out, and it
/// is the kind of thing that creeps back as "just for this one case".
#[test]
fn the_menu_is_a_state_and_not_another_process() {
    let src = read("src/main.rs");
    assert!(
        src.contains("screen::ScreenPlugin"),
        "`main` must add the plugin that owns the two transitions."
    );
    assert!(
        src.contains("chooser::ChooserPlugin") && src.contains("harness::add_editor_plugins"),
        "both screens' plugins are added to the one app; which of them RUNS is the state's business."
    );
    for gone in ["Command::new", "current_exe", "--choose", "fn supervise"] {
        assert!(
            !src.contains(gone),
            "`{gone}` is the process design coming back. The menu is `Screen::Menu` now — a second \
             process would be a second event loop, which winit refuses, and a second plugin graph, \
             which only one of the two would ever be tested."
        );
    }
}

/// **A door is loaded before the screen that needs it is built.**
///
/// `OnExit(Menu)` rather than `OnEnter(Editor)`, and the ordering is the whole reason: a state
/// transition runs every `OnExit` before any `OnEnter`, so `Project` is in the World before the
/// editor's spawns run. Reversed, roughly a hundred systems taking `Res<Project>` would each panic
/// on their first frame — in Bevy 0.19 a missing `Res<T>` panics rather than skipping.
#[test]
fn the_project_is_loaded_before_the_editor_is_built() {
    let src = read("src/screen.rs");
    assert!(
        src.contains("OnExit(Screen::Menu), open_the_door"),
        "the door must open on the way OUT of the menu, so the project exists before any \
         `OnEnter(Editor)` system looks for it."
    );
    assert!(
        src.contains("OnExit(Screen::Editor), close_the_door"),
        "and a door must tear down on the way out, or its entities and its `Project` survive into \
         the next one."
    );
}
