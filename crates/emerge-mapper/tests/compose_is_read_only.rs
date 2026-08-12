//! **One module opens `compositions.ron`, and every author-facing verb comes through it.**
//!
//! # What this used to say, and why it changed
//!
//! FVS-R-15 made the Compose tab read-only and moved authoring to the Map, buying one invariant:
//! **one writer**. This file held that line by naming tabs — `compose.rs` must not write,
//! `editor.rs` must. Naming tabs was always a proxy for the real rule, and it worked for as long as
//! exactly one tab authored.
//!
//! It stopped working when tiles became assemblable. A tile is a `Composition`, so the Tiles tab now
//! authors one too — and a per-tab rule facing that has two options, both bad: forbid the feature, or
//! get widened one tab at a time until it forbids nothing. The second is how a ratchet becomes
//! decoration.
//!
//! So the rule moved to what it was always aiming at. **`project.rs` is the only module that names the
//! file**, through `Project::commit_composition`, and every tab reaches it there. That is the same
//! shape `tiles::commit_measured` has for `library.ron`, and it is checkable directly rather than by
//! enumerating which tabs are allowed to be trusted this week.
//!
//! # What is forbidden, precisely
//!
//! Naming the compositions file, serialising a `Compositions`, or writing bytes — anywhere outside
//! `project.rs`, outside `#[cfg(test)]` modules. A test may build a `Compositions` in memory and
//! round-trip it through `to_ron`; that touches no disk and is how the paint-order encoding is pinned.
//!
//! Reading is not forbidden and must not be: several tabs' whole job is to show what the file says.
//! They read it through `Project`, which the loader filled.

use std::path::Path;

/// **Naming the compositions file.** The one token that means *this* file and no other.
///
/// `save_atomic` and `to_ron()` are deliberately not here: they are how every RON file in the project
/// is written, so a rule built on them cannot tell a tab saving `library.ron` — which `tiles.rs`
/// legitimately does — from a tab saving this one. The first draft of this rewrite did exactly that
/// and failed on `tiles.rs:2251`, which is correct code. The file's *name* is the unambiguous thing.
const NAMES_THE_FILE: &str = "Compositions::FILE";

/// The ways a line writes bytes at all, for the one module that must write none.
const ANY_WRITE: &[&str] = &["save_atomic", "to_ron()", "fs::write", "File::create"];

/// **The one module allowed to do it.** Everything else asks it.
const DOOR: &str = "src/project.rs";

/// Every module that could plausibly reach for the file, and must not name it.
///
/// Named rather than globbed, so adding a tab is a deliberate line here rather than a silent
/// exemption — and so a rename that empties this list fails loudly at the `read` below.
const ASKS: &[&str] = &[
    "src/compose.rs",
    "src/editor.rs",
    "src/build.rs",
    "src/tiles.rs",
];

/// Every line of `src` that is not inside a `#[cfg(test)]` module, as `(1-based line, text)`.
///
/// **It skips the test modules' bodies rather than stopping at the first one**, and the difference is
/// not academic: the first draft of this file broke out of the loop at `#[cfg(test)]`, which in
/// `editor.rs` sits at line 4202 while the write it was supposed to find is at 5103 — so the scan
/// reported a clean file and the test passed by looking at nothing. It was caught by running the same
/// logic against a file known to write; a ratchet that cannot fail is worse than no ratchet, because
/// it reads as a guarantee.
///
/// Test modules here are declared at column zero and closed by a `}` at column zero, which is what
/// makes this a two-line rule rather than a parser.
fn code_outside_tests(src: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("#[cfg(test)]") {
            i += 1;
            while i < lines.len() && lines[i] != "}" {
                i += 1;
            }
            i += 1;
            continue;
        }
        out.push((i + 1, lines[i]));
        i += 1;
    }
    out
}

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn only_the_project_module_opens_the_compositions_file() {
    let mut offences = Vec::new();
    for rel in ASKS {
        let src = read(rel);
        for (n, line) in code_outside_tests(&src) {
            let code = line.split("//").next().unwrap_or(line);
            if code.contains(NAMES_THE_FILE) {
                offences.push(format!("  {rel}:{n}  {}", line.trim()));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "a tab writes `compositions.ron` itself instead of asking \
         `Project::commit_composition`. One module opens that file — {DOOR} — because insert, sort, \
         validate-the-whole-set and write-atomically have to happen together, and a second copy of \
         that sequence is a second chance to skip the validation. If a genuinely new reason to write \
         from a tab exists, it is a design change and this test is where to argue it, not a line to \
         delete:\n{}",
        offences.join("\n")
    );
}

/// **And the door itself writes** — without this the test above is a guarantee about a scan that
/// finds nothing.
///
/// The same argument the previous version made for scanning `editor.rs` after `compose.rs`: asserting
/// only that the tabs are silent would also pass if the feature had been deleted, or if `WRITES` had
/// gone stale against a renamed API.
#[test]
fn the_door_is_where_the_write_actually_is() {
    let src = read(DOOR);
    let found: Vec<usize> = code_outside_tests(&src)
        .into_iter()
        .filter(|(_, l)| {
            let code = l.split("//").next().unwrap_or(l);
            code.contains(NAMES_THE_FILE) || ANY_WRITE.iter().any(|w| code.contains(w))
        })
        .map(|(n, _)| n)
        .collect();
    assert!(
        !found.is_empty(),
        "{DOOR} does not write `compositions.ron`, so the scan above is looking for something that \
         never appears and would pass over any code at all"
    );
}

/// **Every author-facing verb reaches the door**, so the check above cannot be satisfied by a tab
/// that simply stopped saving.
///
/// Two tabs author compositions and they are named here: the Map captures a box selection, the Tiles
/// tab assembles one member at a time. If a third appears it belongs in this list, which is the point
/// — the cost of a new writer should be a line in a test rather than nothing at all.
#[test]
fn both_authoring_tabs_commit_through_the_door() {
    for rel in ["src/editor.rs", "src/build.rs"] {
        let src = read(rel);
        assert!(
            code_outside_tests(&src)
                .iter()
                .any(|(_, l)| l.contains("commit_composition")),
            "{rel} authors compositions but never calls `Project::commit_composition`. Either it \
             stopped saving, or it found another way to the file — and the second is what this file \
             exists to catch."
        );
    }
}

/// **And the Compose tab still writes nothing at all** — the narrower property FVS-R-15 actually
/// bought, kept because it is still true and still worth holding.
///
/// It reads the set, shows what each group presents, and arms one for the Map. It authors nothing, so
/// unlike every other tab it may not write *any* file — which is checkable with the generic tokens
/// that would be too blunt anywhere else.
#[test]
fn the_compose_tab_writes_no_file_at_all() {
    let src = read("src/compose.rs");
    let offences: Vec<String> = code_outside_tests(&src)
        .into_iter()
        .filter_map(|(n, l)| {
            let code = l.split("//").next().unwrap_or(l);
            ANY_WRITE
                .iter()
                .any(|w| code.contains(w))
                .then(|| format!("  src/compose.rs:{n}  {}", l.trim()))
        })
        .collect();
    assert!(
        offences.is_empty(),
        "the Compose tab writes a file. It reads the set and arms a group; authoring happens on the \
         Map and on the Tiles tab. `R` — record what these members present now — is the verb that \
         made this a test rather than a comment: it feels like a view operation and it persists:\n{}",
        offences.join("\n")
    );
}
