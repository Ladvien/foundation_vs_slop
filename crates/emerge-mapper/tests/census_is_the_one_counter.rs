//! **Nothing outside `emerge_core::census` counts the catalog.**
//!
//! A module is not a discipline. `emerge_core::census` exists because three module notes in this crate
//! each stated the size of the same library and all three disagreed — and because a composition layer
//! multiplies the problem: one library gave one count, but a library plus compositions plus a map that
//! stamps them gives *described*, *composed*, *stamped*, and every panel wanting to say "N of M" is a
//! place to compute one slightly differently.
//!
//! Adding the module prevents nothing on its own. This is what prevents it, on the precedent of the
//! ratchets this project already trusts: `emerge-core`'s `engine_free.rs`, the key census's collision
//! test, and the twelve-row display ceiling. Each turns a rule that was a comment into a rule that can
//! fail a build.
//!
//! # What is forbidden, precisely
//!
//! **Rendering** a count of a catalog collection — taking `.len()` of the descriptor list, the
//! composition list, or a map's placements, stamps or locations *on a line that formats a string*.
//!
//! Not the `.len()` itself. The first version of this test forbade that outright and flagged ten
//! sites in `editor.rs`, nine of which were index arithmetic: `if index >= placements.len()`,
//! `let first = placements.len()` before an undo entry. Those cannot drift — they are compared
//! against the very list they measure, in the same expression. What drifts is a number a human reads
//! and believes, so that is what this matches.
//!
//! # Why a source scan and not a type
//!
//! Because the honest alternative — making the fields private behind accessors — would put the
//! collections themselves out of reach, and the editor legitimately iterates all four of them. The
//! thing to forbid is *counting* them, not touching them, and that distinction lives in the text.

use std::path::Path;

/// How many lines after a render macro still count as its arguments.
///
/// Four: enough for `format!(` plus a wrapped argument list as rustfmt writes one, short enough that
/// an unrelated `.len()` four lines later is not blamed on it.
const WINDOW: usize = 4;

/// The ways a line turns a value into something a person reads.
const RENDERS: &[&str] = &["format!(", "write!(", "writeln!(", "info!(", "warn!(", "error!("];

/// A count that belongs to the census, and the accessor that answers it instead.
const FORBIDDEN: &[(&str, &str)] = &[
    ("library.descriptors.len()", "census::of_catalog(..).descriptors"),
    ("measured.descriptors.len()", "census::of_catalog(..).descriptors"),
    ("compositions.compositions.len()", "census::of_catalog(..).compositions"),
    ("map.placements.len()", "census::of_map(..).placements"),
    ("map.stamps.len()", "census::of_map(..).stamps"),
    ("map.locations.len()", "census::of_map(..).locations"),
];

fn sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "rs")
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push((name, text));
        }
    }
    assert!(!out.is_empty(), "found no sources to scan — the path is wrong, not the crate");
    out
}

#[test]
fn no_panel_counts_the_catalog_for_itself() {
    let mut found: Vec<String> = Vec::new();
    for (name, text) in sources() {
        let lines: Vec<&str> = text.lines().collect();
        for (line_no, line) in lines.iter().enumerate() {
            // A comment naming the pattern is how this test explains itself; it is not a count.
            if line.trim_start().starts_with("//") {
                continue;
            }
            // **A window, not a line.** Requiring the macro and the count on one physical line made
            // the ratchet blind to exactly what rustfmt does to a long call: `format!(` on one line
            // and the `.len()` argument on the next. That was not hypothetical — this crate had a
            // wrapped violation the single-line version reported clean.
            let window: String = lines
                .iter()
                .skip(line_no)
                .take(WINDOW)
                .filter(|l| !l.trim_start().starts_with("//"))
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            if !RENDERS.iter().any(|r| line.contains(r)) {
                continue;
            }
            for (pattern, instead) in FORBIDDEN {
                if window.contains(pattern) {
                    found.push(format!(
                        "{name}:{} renders `{pattern}` itself — ask `{instead}`",
                        line_no + 1
                    ));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "a catalog count rendered from outside the census:\n  {}\n\nOne table, everything derived from \
         it — that is what makes two panels disagreeing about the same number unrepresentable rather \
         than unlikely.",
        found.join("\n  ")
    );
}

/// The forbidden list is only meaningful if the thing it points at exists.
#[test]
fn the_census_answers_everything_this_test_forbids() {
    let library = emerge_core::library::Library::default();
    let map = emerge_core::map::Map::default();
    let catalog = emerge_core::census::of_catalog(&library, &[]);
    let counted = emerge_core::census::of_map(&map);
    // Naming each field is the point: if one is removed, this stops compiling and the row above it in
    // FORBIDDEN has to be reconsidered rather than quietly pointing at nothing.
    assert_eq!(
        (catalog.descriptors, catalog.compositions),
        (0, 0),
        "an empty catalog counts zero"
    );
    assert_eq!(
        (counted.placements, counted.stamps, counted.locations),
        (0, 0, 0),
        "an empty map counts zero"
    );
}
