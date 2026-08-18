//! **A widget system writes when something changed, or it does not write.**
//!
//! `chrome::Follow`'s doc comment records what this costs when it is only a habit: two followers
//! re-armed themselves off `Res::is_changed`, wrote every frame, and the scroll never ran — a bug
//! reported twice that passed its tests both times. The rule that came out of it is that no system
//! writes `ScrollPosition`, `Node` or a colour unconditionally per frame, because those are
//! change-detected and everything downstream reads that detection.
//!
//! Enforcement was review, and review is what let it through the first time. This is the same move
//! `chrome_census.rs` makes for colours and `every_list_follows_its_selection.rs` makes for
//! followers: a rule that was a comment becomes a rule that can fail a build.
//!
//! # What counts as satisfying it
//!
//! Either shape is fine, and both appear in the code this guards:
//!
//! - **`Changed<..>`** (or `Added<..>`) in the query filter — the system does not run for unchanged
//!   entities at all.
//! - **Compare before writing** — `if node.display != want { node.display = want; }`. Used where the
//!   link runs the wrong way for a filter: `chrome::hide_idle_scrollbars` reads the *target's*
//!   `ComputedNode` from the track, so a `Changed` filter there would watch the wrong entity.
//!
//! A system that genuinely must write every frame says so with `// WRITES-EVERY-FRAME-OK: <why>`,
//! the `SORT-OK` precedent from the determinism lint: the decision on the record rather than absent.

use std::path::{Path, PathBuf};

/// The modules that draw. Widening this list is fine; it is here so the scan states what it covers
/// rather than implying it covers everything.
const WATCHED: &[&str] = &["chrome.rs", "surface.rs", "notice.rs", "compass.rs"];

/// A write to one of these is a write the layout or render world reads.
const WATCHED_WRITES: &[&str] = &[
    ".display =",
    ".scale =",
    ".viewport =",
    ".left =",
    ".top =",
    ".width =",
    ".height =",
];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Split a file into `(fn name, body)` for every top-level `fn`, test modules excluded.
fn functions(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    // `#[cfg(test)]` at column zero starts the test module; everything after it is fixtures.
    let live = src.split("\n#[cfg(test)]").next().unwrap_or(src);
    let mut rest = live;
    while let Some(at) = rest.find("\nfn ") {
        let after = &rest[at + 4..];
        let name: String = after.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        let Some(open) = after.find('{') else { break };
        let mut depth = 0usize;
        let mut end = open;
        for (i, c) in after[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push((name, after[..=end].to_string()));
        rest = &after[end..];
    }
    out
}

/// How many times this body assigns something the layout reads.
///
/// Counted **per line**, because the first cut counted patterns and double-charged
/// `camera.viewport = Some(want)` — once for `.viewport =` and once for `= Some(want)` — which then
/// demanded two guards for one write. A lint that miscounts is a lint people learn to ignore.
fn count_writes(body: &str) -> usize {
    body.lines()
        .filter(|line| {
            let l = line.trim();
            if l.starts_with("//") {
                return false;
            }
            WATCHED_WRITES.iter().any(|w| l.contains(w))
                || l.contains(".0 = ")
                || l.contains("= Some(want)")
        })
        .count()
}

/// Comparisons that could be guarding a write. `!=` and `==` both are: this crate writes
/// `if x != want { x = want }` in some places and `if x == want { return }` in others, and a
/// `set_if`-style helper hides the comparison in a call — so a body that delegates counts too.
fn count_guards(body: &str) -> usize {
    body.matches("!=").count() + body.matches("==").count() + body.matches("set(&mut ").count()
}

#[test]
fn a_drawing_system_writes_only_when_something_changed() {
    let mut offenders = Vec::new();
    for file in WATCHED {
        let path = src_dir().join(file);
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        for (name, body) in functions(&src) {
            // Only systems that take a mutable handle on something the layout reads.
            let writes_layout = body.contains("&mut Node")
                || body.contains("&mut BackgroundColor")
                || body.contains("ScrollPosition")
                || body.contains("&mut Camera");
            if !writes_layout {
                continue;
            }
            if count_writes(&body) == 0 {
                continue;
            }
            // **Counted, not name-matched.** The first cut looked for `!= want` literally and
            // reported two systems that compare perfectly well against differently-named locals —
            // a lint whose false positives teach people to delete it. What actually has to hold is
            // that every write is behind a comparison, so the guards are counted against the
            // writes.
            let writes = count_writes(&body);
            let guards = count_guards(&body);
            let guarded = body.contains("Changed<")
                || body.contains("Added<")
                || body.contains("WRITES-EVERY-FRAME-OK:")
                || guards >= writes;
            if !guarded {
                offenders.push(format!(
                    "{file}::{name} — {writes} write(s) to layout, {guards} comparison(s)"
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these systems write something the layout reads, with no `Changed<..>` filter and no \
         compare before the write. `Node`, `BackgroundColor`, `ScrollPosition` and `Camera` are all \
         change-detected, and writing them every frame destroys that detection for everything \
         downstream — `chrome::Follow`'s doc records the two-day bug that came of it. Gate the \
         query, compare before writing, or state the exception with \
         `// WRITES-EVERY-FRAME-OK: <why>`:\n{}",
        offenders.join("\n")
    );
}

/// **The scan can see the systems it claims to check.**
///
/// The companion assertion, and the one that matters most: a parser that quietly matched nothing
/// would pass forever. `chrome.rs` alone carries several of these.
#[test]
fn the_scan_actually_finds_drawing_systems() {
    let src = std::fs::read_to_string(src_dir().join("chrome.rs")).expect("chrome.rs");
    let found: Vec<String> = functions(&src)
        .into_iter()
        .filter(|(_, b)| b.contains("&mut Node") || b.contains("&mut BackgroundColor"))
        .map(|(n, _)| n)
        .collect();
    assert!(
        found.len() >= 3,
        "the scan found only {} drawing systems in chrome.rs — if the parser has stopped seeing \
         them, the rule above is being enforced against nothing: {found:?}",
        found.len()
    );
}
