//! **Source lint: the squad simulation must not know what screen the player is looking at.**
//!
//! GPU-free, no `App` — this runs in the `cargo test` hard gate, so it blocks on every push.
//!
//! # Why this exists
//!
//! `input::Action::VisitSite` lets the player walk to Site-67 with an expedition still `Active`
//! (`docs/2026-08-01-two-live-layers.md`). The squad keeps executing its standing order while nobody
//! is watching, and falls through to `squad_ai::policy::ActivePolicy` once that order is consumed.
//! That behaviour is not a feature anyone wrote — it is a *consequence* of the squad stack gating on
//! `session::RunState` and nothing else (`squad.rs`'s and `squad_ai/mod.rs`'s
//! `distributive_run_if(in_state(RunState::Active))`). The design doc's §4 leans on it entirely.
//!
//! Two one-line changes would silently destroy it, and both are the sort of thing a reasonable person
//! adds while tidying:
//!
//! * **A gate.** `.run_if(in_state(AppState::InGame))` on any squad system freezes the unattended
//!   squad. The player visits the Site, the expedition stops dead, and the risk premise evaporates —
//!   with no error and nothing in the logs.
//! * **A cleanup.** An `OnExit(AppState::InGame)` system that removes `MoveOrder`/`PushOrder` drops
//!   the player's standing order every time they leave for the Site. "Continue the last order" —
//!   the director's call in §4 — quietly becomes "hold".
//!
//! # Why the existing tests cannot catch either
//!
//! This is the trap, and it is worth stating plainly: `replay::ui_never_leaks_into_deterministic_core`
//! asserts `State<AppState>` is **absent** from the headless app. So in the harness a stray
//! `OnExit(AppState::InGame)` system never fires and a stray `in_state(AppState::InGame)` condition is
//! never satisfied by a state that exists — the whole replay suite stays green while the windowed game
//! is broken. The determinism firewall proves the core does not *depend* on `AppState`; it cannot
//! prove that nothing in the squad stack *mentions* it. Only the source can say that.
//!
//! Same reasoning as `determinism_lint`, whose module doc puts it best: comments were the only
//! enforcement, and comments do not fail.
//!
//! # The contract
//!
//! No file under `src/squad.rs` or `src/squad_ai/` may contain the token `AppState` — in code or in a
//! comment. There is no escape hatch, deliberately: unlike a sort's determinism contract, there is no
//! legitimate reason for the simulation to know which screen is up. `session`'s module docs already
//! state the rule ("the containment systems that will read it must run headless"); this makes it fail.
//!
//! Presentation code that legitimately reads `AppState` — `site::visuals`, `ui::*` — is not in scope.

use std::path::{Path, PathBuf};

/// The forbidden token. Matching on the bare type name catches every form that matters:
/// `in_state(AppState::InGame)`, `OnExit(AppState::InGame)`, `Res<State<AppState>>`, and a
/// `crate::ui::state::AppState` import alike.
const FORBIDDEN: &str = "AppState";

/// Every file the simulation half of the squad owns.
fn squad_sources() -> Result<Vec<PathBuf>, String> {
    let mut out = vec![PathBuf::from("src/squad.rs")];
    collect_rs(Path::new("src/squad_ai"), &mut out)?;
    // A rename or a move must not silently empty this lint — an empty file set would pass forever.
    if out.len() < 2 {
        return Err(format!(
            "expected src/squad.rs plus the src/squad_ai/ tree, found {} file(s) — has the squad \
             stack moved? Point this lint at its new home rather than deleting it.",
            out.len()
        ));
    }
    Ok(out)
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    // Collected then sorted: `read_dir` yields in filesystem order, so an unsorted walk would report
    // a different first offender between machines and make the failure message irreproducible.
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read an entry of {}: {e}", dir.display()))?;
        paths.push(entry.path());
    }
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_rs(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[test]
fn the_squad_simulation_never_mentions_appstate() {
    let files = match squad_sources() {
        Ok(f) => f,
        Err(e) => panic!("{e}"),
    };

    let mut offences: Vec<String> = Vec::new();
    for path in &files {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => panic!("cannot read {}: {e}", path.display()),
        };
        for (i, line) in src.lines().enumerate() {
            if line.contains(FORBIDDEN) {
                offences.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "the squad simulation must not know which screen the player is on — {} site(s):\n{}\n\n\
         A squad system keyed on `AppState` either freezes the unattended expedition (a run \
         condition) or drops the player's standing order (an `OnExit` cleanup) the moment they visit \
         Site-67. Gate on `session::RunState` instead: it is the state that means \"an expedition is \
         alive\", and it is what every other squad system already uses. See \
         docs/2026-08-01-two-live-layers.md §4.",
        offences.len(),
        offences.join("\n")
    );
}

/// The lint is only worth anything if it is pointed at files that exist and are non-trivial — a typo in
/// a path would otherwise make it pass by scanning nothing.
#[test]
fn the_lint_actually_reads_the_squad_stack() {
    let files = match squad_sources() {
        Ok(f) => f,
        Err(e) => panic!("{e}"),
    };
    for path in &files {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => panic!("cannot read {}: {e}", path.display()),
        };
        assert!(!src.is_empty(), "{} is empty — the lint would scan nothing", path.display());
    }
    // The token this lint bans must genuinely be findable in the tree, or a rename of `AppState` would
    // leave the lint passing vacuously forever while the real coupling went unchecked.
    let ui_state = match std::fs::read_to_string("src/ui/state.rs") {
        Ok(s) => s,
        Err(e) => panic!("cannot read src/ui/state.rs: {e}"),
    };
    assert!(
        ui_state.contains(FORBIDDEN),
        "`{FORBIDDEN}` no longer appears in src/ui/state.rs — if the UI state enum was renamed, this \
         lint is now checking for a token that cannot occur and must be updated."
    );
}
