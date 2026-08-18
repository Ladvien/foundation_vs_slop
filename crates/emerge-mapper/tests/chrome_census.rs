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
//! - **A font size outside `chrome.rs` is a `chrome::text::` ROLE, never a number.** The old form
//!   of this rule accepted any literal on a pinned scale, which stopped a stray 12 arriving and did
//!   nothing about a hundred sites choosing among six numbers with no rule between them — the exact
//!   drift the 2026-08-17 audit measured. Naming the role puts "what does a heading measure" in one
//!   place, the way the palette already works.
//!
//! Test modules are exempt: a fixture painting a throwaway colour asserts nothing about the
//! shipped panels.

use std::path::{Path, PathBuf};

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

/// **Text is named, not numbered.**
///
/// This test used to accept any literal on the 9/10/11/13/15/18 scale, and that was the weaker half
/// of the rule: it stopped a stray 12 or 14 arriving, and did nothing at all about a hundred call
/// sites choosing among six numbers with no rule between them. The 2026-08-17 audit measured the
/// result — section headings at 9, 10 **and** 11 in one editor, two "COMPOSITIONS" headings in one
/// pane at different sizes, label/value pairs at 10/11, 10/10 and flat 11 depending on the tab, and
/// the anim bench rendering declared over measured **inverted**. The palette had `chrome::token`
/// discipline and the spacing had a scale; *size never got a name*.
///
/// So the rule is now the one the palette already has: **a size outside `chrome.rs` is a
/// `chrome::text::` role**, and a bare number fails here. Which value a role carries is `text`'s
/// decision to change in one place — two roles may share a value, and `HEADING` and `LABEL` both do.
#[test]
fn text_is_named_not_numbered() {
    let mut numbered = Vec::new();
    for (file, line_no, line) in panel_source() {
        let mut from = 0;
        while let Some(at) = line[from..].find("from_font_size(") {
            let after = &line[from + at + "from_font_size(".len()..];
            let literal: String = after
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(px) = literal.parse::<f32>() {
                numbered.push(format!("{file}:{line_no}: {px} — {}", line.trim()));
            }
            from += at + "from_font_size(".len();
        }
    }
    assert!(
        numbered.is_empty(),
        "font sizes written as numbers outside chrome.rs. Name the ROLE — \
         `crate::chrome::text::{{TITLE, TAB, BODY, HEADING, LABEL, HINT}}` — so that changing what a \
         role measures is one edit rather than a hunt:\n{}",
        numbered.join("\n")
    );
}

/// **The roles are the whole scale, and the scale is short.**
///
/// The companion to the rule above: naming a size is worth nothing if `text` grows a role per call
/// site. Six roles over five values is the shape the audit's findings ask for, and a seventh is a
/// decision somebody should have to make on purpose.
#[test]
fn the_type_scale_stays_short() {
    let src = std::fs::read_to_string(src_dir().join("chrome.rs"))
        .unwrap_or_else(|e| panic!("chrome.rs: {e}"));
    let module = src
        .split_once("pub mod text {")
        .map(|(_, rest)| rest.split_once("\n}").map(|(m, _)| m).unwrap_or(rest))
        .unwrap_or_else(|| panic!("`chrome::text` is where the type scale lives"));

    let roles: Vec<&str> = module
        .lines()
        .filter(|l| l.trim_start().starts_with("pub const "))
        .collect();
    assert!(
        roles.len() <= 6,
        "the type scale has grown to {} roles. A role per call site is the drift the audit \
         measured, one indirection later:\n{}",
        roles.len(),
        roles.join("\n")
    );

    let mut values: Vec<String> = roles
        .iter()
        .filter_map(|l| l.rsplit_once("= ").map(|(_, v)| v.trim_end_matches(';').to_owned()))
        .collect();
    values.sort();
    values.dedup();
    assert!(
        values.len() <= 5,
        "the type scale carries {} distinct sizes. The audit's finding was six with no rule; \
         more than five is not a fix:\n{:?}",
        values.len(),
        values
    );
}
