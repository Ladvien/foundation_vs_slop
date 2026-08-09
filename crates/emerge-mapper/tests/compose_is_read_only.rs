//! **The Compose tab opens `compositions.ron` nowhere.**
//!
//! Compositions are authored on the Map — arrange, box-select, name — and the Compose tab reads what
//! that produced: the strip, the members, the derived interface, what has gone stale, and which group
//! is armed. It used to author them too, with its own new/add/seat/flush/turn/drop/paint verbs and its
//! own undo stack, which meant one file had two writers reachable by two different sets of keys.
//!
//! # Why a test and not a note
//!
//! "Compose is read-only" is only worth saying if it can fail. The verb that made this a test rather
//! than a comment is `R` — *record what these members present now*. It is derivation rather than
//! authoring, which makes it feel like a view operation, and it **persists**, which makes it a writer.
//! Keeping it would have left the property as "read-only except `R`", and that is a description of the
//! code rather than a claim about it. So `R` moved with the rest, and this is what holds the line.
//!
//! On the precedent of the ratchets this project already trusts: `emerge-core`'s `engine_free.rs`, the
//! key census's collision test, the twelve-row display ceiling, and `census_is_the_one_counter.rs`
//! beside this file. Each turns a rule that was a comment into a rule that can fail a build.
//!
//! # What is forbidden, precisely
//!
//! Naming the compositions file, serialising a `Compositions`, or writing bytes — anywhere in
//! `compose.rs` outside its `#[cfg(test)]` modules. A test may build a `Compositions` in memory and
//! round-trip it through `to_ron`; that touches no disk and is how the paint-order encoding is pinned.
//!
//! Reading is not forbidden and must not be: the tab's whole job is to show what the file says. It
//! reads it through `Project`, which the loader filled.

use std::path::Path;

/// The ways a line reaches the compositions file.
///
/// `to_ron` is on the list even though it only produces a `String`: it is the step that turns the
/// in-memory set into the bytes on disk, and a tab that serialises the set has already decided it is
/// going to write it. Catching it here names the intent rather than the syscall.
const WRITES: &[&str] = &[
    "Compositions::FILE",
    "save_atomic",
    "to_ron()",
    "fs::write",
    "File::create",
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

#[test]
fn the_compose_tab_never_writes_the_compositions_file() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/compose.rs");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    let mut offences = Vec::new();
    for (n, line) in code_outside_tests(&src) {
        let code = line.split("//").next().unwrap_or(line);
        for w in WRITES {
            if code.contains(w) {
                offences.push(format!("  src/compose.rs:{n}  {}", line.trim()));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "the Compose tab writes `compositions.ron`, and it is supposed to be the tab that only \
         reads it. Authoring lives on the Map — arrange, box-select, name. If a genuinely new \
         reason to write from here exists, it is a design change and this test is where to argue \
         it, not a line to delete:\n{}",
        offences.join("\n")
    );
}

/// **And the Map is the one that does** — asserted through the same scan, which is what keeps the
/// two halves honest.
///
/// Two things at once. Asserting only that Compose is silent would also pass if nothing anywhere
/// could author a composition, which is a working editor with the feature removed. And running the
/// *identical* scan over a file that is known to write proves the scan can see a writer at all —
/// without this, the test above is a guarantee about a function that returns nothing.
#[test]
fn the_map_is_the_one_that_writes_it_and_the_scan_can_see_it() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/editor.rs");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let writes: Vec<usize> = code_outside_tests(&src)
        .into_iter()
        .filter(|(_, l)| {
            let code = l.split("//").next().unwrap_or(l);
            WRITES.iter().any(|w| code.contains(w))
        })
        .map(|(n, _)| n)
        .collect();
    assert!(
        !writes.is_empty(),
        "nothing on the Map writes `compositions.ron`, so either capture stopped reaching the file \
         — and no tab can author a composition — or `WRITES` no longer names how one is written, \
         which would make the test above vacuous"
    );
}
