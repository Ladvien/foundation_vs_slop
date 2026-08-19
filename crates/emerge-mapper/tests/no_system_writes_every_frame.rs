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
const WATCHED: &[&str] = &[
    "chrome.rs",
    "surface.rs",
    "notice.rs",
    "compass.rs",
    // Added 2026-08-18, after `tiles::style_tabs` shipped an unconditional per-frame `BorderColor`
    // write with every other write in the same function guarded.
    //
    // **And it is worth saying that widening the scan is not what would have caught it.** The counts
    // are per BODY, so a function with four guarded writes and one unguarded one balances: this
    // rule finds a system that guards NOTHING, not a system that misses one write. Localising the
    // count to each write is the next version of this file, and until it exists that limit is
    // stated here rather than assumed away.
    "tiles.rs",
];

/// A write to one of these is a write the layout or render world reads.
const WATCHED_WRITES: &[&str] = &[
    ".display =",
    ".scale =",
    ".viewport =",
    ".left =",
    ".top =",
    ".width =",
    ".height =",
    // `*border = BorderColor::all(..)` is a whole-component write and looks nothing like the field
    // assignments above it, which is exactly why it was missed.
    "= BorderColor::all(",
];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The offset of the next top-level `fn` and how far past the line start its name begins.
/// Handles `fn`, `pub fn`, `pub(crate) fn` and `pub(super) fn` — anything else is not top level here.
fn next_fn(rest: &str) -> Option<(usize, usize)> {
    ["\nfn ", "\npub fn ", "\npub(crate) fn ", "\npub(super) fn "]
        .iter()
        .filter_map(|p| rest.find(p).map(|at| (at, p.len())))
        .min_by_key(|(at, _)| *at)
}

/// **Skip past each test module, never truncate at the first.** The naive version split on the
/// first `#[cfg(test)]` and kept the head — and `chrome.rs`'s first test module is at line 1917 of
/// 2395, so a fifth of the file this rule exists for was never read. `the_sweep_is_finished.rs`
/// records the same hole being found the hard way in `tiles.rs`, and `compose_is_read_only.rs` says
/// why it matters: *"a ratchet that cannot fail is worse than no ratchet, because it reads as a
/// guarantee."* Borrowed from those two rather than re-derived, a third time.
fn code_outside_tests(src: &str) -> String {
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
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}

/// Split a file into `(fn name, body)` for every top-level `fn`, test modules excluded.
///
/// **`pub fn` counts.** Anchoring on `"\nfn "` skipped every exported function, which in `chrome.rs`
/// is 22 of 29 — including `hide_idle_scrollbars`, the system this module's own doc holds up as the
/// example of the rule. Same anchor bug as `the_sweep_is_finished.rs::signatures`.
fn functions(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let live = code_outside_tests(src);
    let mut rest = live.as_str();
    while let Some((at, skip)) = next_fn(rest) {
        let after = &rest[at + skip..];
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
///
/// **`set(&mut ..)` is NOT counted, and that was a measured mistake.** Passing `&mut` of a field of a
/// `Mut<T>` into a helper runs `Mut::deref_mut`, which calls `set_changed()` *before* the helper's
/// own comparison happens — so the shape guards the write and not the dirty flag, which is the only
/// half anything downstream reads. Crediting it meant this lint certified the one pattern that
/// provably defeats the rule it enforces. Verified with a probe: a helper of that shape reports
/// `is_changed()` after a no-op; an in-place compare does not.
fn count_guards(body: &str) -> usize {
    body.matches("!=").count() + body.matches("==").count()
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
        found.len() >= 6,
        "the scan found only {} drawing systems in chrome.rs — if the parser has stopped seeing \
         them, the rule above is being enforced against nothing: {found:?}",
        found.len()
    );
}
