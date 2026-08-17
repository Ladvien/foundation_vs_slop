//! **The UI census ratchet: panel ink comes from the palette, and text stays on the scale.**
//!
//! `chrome.rs`'s founding argument is that a fact stated more than once drifts, and the 2026-08-17
//! audit measured how far this crate had drifted from it: one hover grey as an unnamed literal in
//! two files, a hand-halved copy of `ACCENT`, three byte-transcribed chrome colours in the plot
//! palette, and two font-size dialects for one role in one pane. FVS-R-19/20/21 swept all of that
//! onto names; this is what keeps it swept — the same move `keys.rs` makes for bindings,
//! `stages::distinct` for staging points, and `census_is_the_one_counter.rs` for counts: a rule
//! that was a review comment becomes a rule that can fail a build.
//!
//! # The two rules
//!
//! - **A `Color::srgb`/`srgba` literal outside `chrome.rs` needs a `CHROME-OK:` marker** (same
//!   line or the line above), stating why it is not a palette word. The legitimate class is world
//!   ink — gizmo palettes, tool tints, a stage floor — and the marker is the decision on the
//!   record, the determinism lint's `SORT-OK` precedent. Variable-argument constructors
//!   (`srgb_u8(r, g, b)`) are not literals and pass.
//! - **A literal `from_font_size` stays on the type scale** — 9 / 10 / 11 / 13 / 15 / 18. The
//!   scale is pinned as data rather than each use marked, because a size on the scale is not a
//!   decision, while a new 12 or 14 is one — and it fails here until it is either put on the scale
//!   deliberately or taken back off the screen.
//!
//! Test modules are exempt: a fixture painting a throwaway colour asserts nothing about the
//! shipped panels.

use std::path::{Path, PathBuf};

/// The sizes the editor's text is allowed to be, per the 2026-08-17 type-role decision:
/// 9 `section`/fine print, 10 labels and list rows, 11 body and values, 13 the tab strip's word,
/// 15 the panel title, 18 the name-box value.
const TYPE_SCALE: &[f32] = &[9.0, 10.0, 11.0, 13.0, 15.0, 18.0];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every non-test line of every `src/*.rs` except `chrome.rs`, as `(file, 1-based line, text)`.
///
/// Test modules are skipped by the `#[cfg(test)]`-at-column-zero rule `compose_is_read_only.rs`
/// documents — including its warning that the scan must skip PAST each module rather than stop at
/// the first, or it reads nothing and passes vacuously.
fn panel_source() -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    let dir = src_dir();
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .filter(|p| p.file_name().is_some_and(|n| n != "chrome.rs"))
        .collect();
    names.sort();
    for path in names {
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let lines: Vec<&str> = text.lines().collect();
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
            out.push((file.clone(), i + 1, lines[i].to_owned()));
            i += 1;
        }
    }
    assert!(
        out.len() > 5_000,
        "the scan saw only {} lines of panel source — it has stopped reading the crate, which \
         would make both assertions below vacuous",
        out.len()
    );
    out
}

/// A literal colour constructor on this line — `Color::srgb(` or `Color::srgba(` whose first
/// argument starts with a digit or a dot, so `srgb_u8(r, g, b)` over variables does not count.
fn literal_colour(line: &str) -> bool {
    for pat in ["Color::srgb(", "Color::srgba("] {
        let mut from = 0;
        while let Some(at) = line[from..].find(pat) {
            let after = &line[from + at + pat.len()..];
            if after
                .trim_start()
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || c == '.')
            {
                return true;
            }
            from += at + pat.len();
        }
    }
    false
}

/// **Panel ink comes from the palette.** A raw colour literal outside chrome is either a palette
/// word that has not been named yet, or a world-ink decision — and the marker is how the decision
/// is told apart from the leak.
#[test]
fn panel_ink_comes_from_the_palette() {
    let source = panel_source();
    let mut leaks = Vec::new();
    for (ix, (file, line_no, line)) in source.iter().enumerate() {
        if !literal_colour(line) {
            continue;
        }
        let marked = line.contains("CHROME-OK:")
            || (ix > 0 && source[ix - 1].0 == *file && source[ix - 1].2.contains("CHROME-OK:"));
        if !marked {
            leaks.push(format!("{file}:{line_no}: {}", line.trim()));
        }
    }
    assert!(
        leaks.is_empty(),
        "raw colour literals outside chrome.rs, with no `// CHROME-OK: <why>` on or above them — \
         name them in the palette, or state the decision:\n{}",
        leaks.join("\n")
    );
}

/// **Text stays on the scale.** A size on the scale is a role; a size off it is a new decision,
/// and it fails here until it is made deliberately (added to [`TYPE_SCALE`] with its role) rather
/// than slipped in.
#[test]
fn text_sizes_stay_on_the_scale() {
    let mut off_scale = Vec::new();
    for (file, line_no, line) in panel_source() {
        let mut from = 0;
        while let Some(at) = line[from..].find("from_font_size(") {
            let after = &line[from + at + "from_font_size(".len()..];
            let literal: String = after
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(px) = literal.parse::<f32>() {
                if !TYPE_SCALE.contains(&px) {
                    off_scale.push(format!("{file}:{line_no}: {px} — {}", line.trim()));
                }
            }
            from += at + "from_font_size(".len();
        }
    }
    assert!(
        off_scale.is_empty(),
        "font sizes off the 9/10/11/13/15/18 scale — a new size is a type-role decision, not a \
         tweak:\n{}",
        off_scale.join("\n")
    );
}
