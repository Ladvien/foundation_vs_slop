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

/// **Every `src/*.rs`, as code** — `#[cfg(test)]` modules dropped, comment lines dropped — as
/// `(file name, code)`.
///
/// Both strips are defects this file has already had. A `#[cfg(test)]` module is a place where the
/// state may legitimately be set to prove something about setting it, and a unit test added to
/// `editor.rs` later would redden a ratchet about the *editor* for a reason that has nothing to do
/// with leaving a door. And `editor.rs`'s own scheduling note narrates the three
/// `next.set(Screen::Menu)` calls that used to exist — so a scan that read prose counted four ways
/// out and could not tell a regression from a paragraph.
///
/// The `#[cfg(test)]` walk must skip **past** the module rather than stop at the first one, which is
/// the warning `chrome_census.rs` and `every_key_has_a_home.rs` both carry.
fn code_outside_tests() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{dir:?}: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let lines: Vec<&str> = text.lines().collect();
        let mut kept: Vec<&str> = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            if lines[i].starts_with("#[cfg(test)]") {
                i += 1;
                if i < lines.len() && lines[i].starts_with("mod ") {
                    i += 1;
                    while i < lines.len() && !lines[i].starts_with('}') {
                        i += 1;
                    }
                }
                continue;
            }
            if lines[i].trim_start().starts_with("//") {
                i += 1;
                continue;
            }
            kept.push(lines[i]);
            i += 1;
        }
        out.push((name, kept.join("\n")));
    }
    assert!(
        out.iter().any(|(f, _)| f == "editor.rs") && out.iter().any(|(f, _)| f == "screen.rs"),
        "the scan must see both files this ratchet is about; it saw {:?}",
        out.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>()
    );
    out
}

/// **The body of a top-level `fn`**, from its declaration line to the `}` in column one.
///
/// Crude and exact: `rustfmt` puts a top-level item's closing brace at column zero and nothing
/// inside a body there, so the first bare `}` after the declaration is the end. Used to ask a
/// *gesture* whether it walks the door, which is the question — asking the file whether the name
/// appears anywhere in it is what let four doc comments stand in for three call sites.
fn body_of(code: &str, decl: &str) -> Option<String> {
    let lines: Vec<&str> = code.lines().collect();
    let at = lines.iter().position(|l| l.starts_with(decl))?;
    let end = lines[at..]
        .iter()
        .position(|l| *l == "}")
        .map_or(lines.len(), |i| at + i + 1);
    Some(lines[at..end].join("\n"))
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

/// **Every way out of a door goes through one door, and that door lands on the menu.**
///
/// Three gestures leave: `Cmd+O`, the chrome bar's back button, and `Y` to the unsaved-work
/// question. This used to count `next.set(Screen::Menu)` in `editor.rs` and demand at least three of
/// them, one per gesture — which was true when each gesture set the state itself.
///
/// **They were then consolidated onto `save_and_leave`**, deliberately: a refused save has to keep
/// you on the map with the reason rather than drop you on the menu with unwritten work, and three
/// copies of that decision is three places for it to differ. So there is exactly *one* way out that
/// sets the state now, and the old count made the right shape look like a regression.
///
/// # It claimed a crate-wide property and read one file
///
/// Both halves scanned `src/editor.rs` alone, while `src/screen.rs` has carried a second
/// `next.set(Screen::Menu)` ever since the two screens became one application —
/// `open_the_door`'s bail-out for a state set with nothing chosen. So *"the menu is reached from
/// exactly one place"* was never true of the crate, and a fourth gesture added in any other file
/// would have satisfied the assertion while going around the refused-save branch. The scan is the
/// crate now, and that bail-out is named as the one exception rather than being out of shot.
///
/// And the `save_and_leave` half was a floor over *textual* occurrences, doc comments included:
/// `editor.rs`'s own prose mentions the function four times, so the floor of four was already
/// cleared with every call site deleted. It is tied to the gestures instead — each of the three
/// systems that leaves, asked whether its **own body** walks the door.
#[test]
fn every_way_out_of_a_door_lands_on_the_menu() {
    let code = code_outside_tests();

    let mut exits: Vec<(String, String)> = Vec::new();
    for (file, text) in &code {
        for line in text.lines() {
            if line.contains(".set(") && line.contains("Screen::Menu)") {
                exits.push((file.clone(), line.trim().to_owned()));
            }
        }
    }
    let in_file = |name: &str| exits.iter().filter(|(f, _)| f == name).count();
    assert_eq!(
        in_file("editor.rs"),
        1,
        "the menu is reached from exactly one place in `editor.rs` — `save_and_leave`. Found {} \
         of {exits:?}; more than one means a gesture has gone around it, and around the \
         refused-save branch with it.",
        in_file("editor.rs")
    );
    assert_eq!(
        in_file("screen.rs"),
        1,
        "`screen::open_the_door` must keep its bail-out — entering a door with nothing chosen has \
         to fall back to the menu, because the alternative is a hundred systems each discovering \
         separately that there is no `Project`. Found {} of them.",
        in_file("screen.rs")
    );
    assert_eq!(
        exits.len(),
        2,
        "exactly two places in this crate set `Screen::Menu`: the one door out \
         (`editor::save_and_leave`) and `screen::open_the_door`'s entry bail-out. The scan found \
         {exits:?} — a third is a way out that can forget the save."
    );

    let editor = code
        .iter()
        .find(|(f, _)| f == "editor.rs")
        .map(|(_, t)| t.as_str())
        .unwrap_or_else(|| panic!("`src/editor.rs` must be in the scan"));
    assert!(
        editor.contains("pub fn save_and_leave("),
        "`save_and_leave` is the one door out, and the three gestures below are checked against \
         that name — if it was renamed, rename it here too rather than deleting the check."
    );
    // **Each gesture, asked of its own body.** The whole point of the consolidation is that these
    // three do not set the state themselves; what makes that safe rather than merely tidy is that
    // all three still reach the function that does.
    for (gesture, decl) in [
        ("`Cmd+O`", "fn back_to_the_menu("),
        ("`Y` to the leaving prompt", "fn answer_the_leaving_prompt("),
        ("the chrome bar's back button", "fn back_button_clicked("),
    ] {
        let body = body_of(editor, decl).unwrap_or_else(|| {
            panic!("`{decl}` is gone from `editor.rs`, so {gesture} has no system to leave by")
        });
        assert!(
            body.contains("save_and_leave("),
            "{gesture} leaves a door without walking `save_and_leave`, so it can drop an author on \
             the menu with unwritten work — the defect the consolidation removed. `{decl}` must \
             call it. Its body reads:\n{body}"
        );
    }
    for (file, text) in &code {
        assert!(
            !text.contains("BACK_TO_MENU"),
            "`{file}` names `BACK_TO_MENU`: the exit code went with the process boundary, and \
             naming it again would be a second way to say `Screen::Menu`."
        );
    }
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
