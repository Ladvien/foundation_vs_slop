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

/// **Text is named, not numbered — and now it is a type, so this test only has to catch the door.**
///
/// The old form of this rule scanned for `from_font_size(<digit>`, and the 2026-09-03 audit found
/// the hole in it: `chrome::chip` and `chrome::text_field` took `px: f32` and called
/// `from_font_size` *inside* `chrome.rs`, which this scan skips. **Twenty call sites were passing
/// bare `9.0` / `10.0` / `11.0` through a function argument**, in a crate whose own test claimed a
/// size was "a role, never a number". A regex cannot close that.
///
/// So `chrome::text::Role` is a newtype, `chrome::font(Role)` is the only way this crate builds a
/// `TextFont`, and the rule here is the simple one: **`from_font_size` does not appear outside
/// `chrome.rs`.** A builder that wants a size takes a `Role` and the compiler refuses a number.
#[test]
fn the_type_scale_is_a_type() {
    let mut raw = Vec::new();
    for (file, line_no, line) in panel_source() {
        if line.contains("from_font_size") {
            raw.push(format!("{file}:{line_no}: {}", line.trim()));
        }
    }
    assert!(
        raw.is_empty(),
        "`TextFont::from_font_size` takes an `f32` and will therefore always accept a number. Use \
         `crate::chrome::font(crate::chrome::text::ROLE)`, which takes a `Role` and cannot:\n{}",
        raw.join("\n")
    );
}

/// **The roles are the whole scale, and the scale is short.**
///
/// The companion to the rule above: naming a size is worth nothing if `text` grows a role per call
/// site. A seventh role *"is a decision somebody should have to make on purpose"*, and on 2026-09-03
/// somebody did: `CONTROL`, the word on a chip or a button, arrived with the decision that a toggle
/// and a command are different shapes. An eighth is the next such decision.
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
        .filter(|l| l.contains(": Role = Role("))
        .collect();
    assert!(
        roles.len() <= 7,
        "the type scale has grown to {} roles. A role per call site is the drift the audit \
         measured, one indirection later:\n{}",
        roles.len(),
        roles.join("\n")
    );

    let mut values: Vec<String> = roles
        .iter()
        .filter_map(|l| l.rsplit_once("Role(").map(|(_, v)| v.trim_end_matches(");").to_owned()))
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

/// **Spacing comes from the scale — the axis that was never ratcheted.**
///
/// The palette got a census in 2026-08 and the type scale got one; spacing never did, and the
/// 2026-09-03 audit measured what that cost: **79 literal `Val::Px` sites over 19 distinct values**
/// outside `chrome.rs`, against a scale that declares three gaps. The gap above a heading was 3, 4,
/// 6 or 8 depending on which file you were in; `CHIP_PAD` was restated by hand as
/// `axes(Px(6.0), Px(3.0))` in a file that imports it; the six label columns were 76 / 62 / 56 / 48 /
/// 40 / 14, which is why no two panels in the editor lined their values up.
///
/// Same shape as the colour rule, and the same escape hatch: genuine one-off geometry — a progress
/// bar's height, a fixed thumbnail — states its decision with a `// CHROME-OK: <why>` on or above
/// the line. `Val::Px(NAME)` over a constant is not a literal and passes silently, which is the
/// point.
#[test]
fn spacing_comes_from_the_scale() {
    let source = panel_source();
    let mut loose = Vec::new();
    for (ix, (file, line_no, line)) in source.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let mut from = 0;
        while let Some(at) = line[from..].find("Val::Px(") {
            let after = &line[from + at + "Val::Px(".len()..];
            let literal: String = after
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            from += at + "Val::Px(".len();
            // **Zero is not spacing.** `Val::Px(0.0)` is how a flex item says *no minimum* —
            // `min_width: 0` is what lets a scroll container clip instead of growing to fit, and
            // `flex_basis: 0` is how a column asks to be sized by its share rather than by its
            // content. Naming those would be naming the absence of a gap, and a `GAP_NONE` constant
            // would be read as a spacing decision by the next person. Any other number is one.
            if !literal.parse::<f32>().is_ok_and(|px| px != 0.0) {
                continue;
            }
            let marked = line.contains("CHROME-OK:")
                || (ix > 0 && source[ix - 1].0 == *file && source[ix - 1].2.contains("CHROME-OK:"));
            if !marked {
                loose.push(format!("{file}:{line_no}: {literal} — {}", line.trim()));
            }
        }
    }
    assert!(
        loose.is_empty(),
        "spacing written as numbers outside chrome.rs. Name it — `GAP_TIGHT` / `GAP_ROW` / \
         `GAP_GROUP` / `PAD` / `MARGIN` / `CHIP_PAD` / `BUTTON_PAD` / `FIELD_PAD` / `COL_TIGHT` / \
         `COL_LABEL` / `COL_WIDE` — or state the decision with `// CHROME-OK: <why>`:\n{}",
        loose.join("\n")
    );
}

// ── the ladder, measured ─────────────────────────────────────────────────────────────────────────

/// A palette constant's sRGB triple, parsed out of `chrome.rs`.
fn palette() -> std::collections::HashMap<String, [f32; 3]> {
    let src = std::fs::read_to_string(src_dir().join("chrome.rs"))
        .unwrap_or_else(|e| panic!("chrome.rs: {e}"));
    let mut out = std::collections::HashMap::new();
    for line in src.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let Some((name, value)) = rest.split_once(": Color = Color::srgb") else {
            continue;
        };
        // `srgba` carries a fourth component; the ladder is opaque grounds only.
        let Some(args) = value.strip_prefix('(').and_then(|v| v.split_once(')')).map(|(a, _)| a)
        else {
            continue;
        };
        let parts: Vec<f32> = args
            .split(',')
            .filter_map(|p| p.trim().parse::<f32>().ok())
            .collect();
        if let [r, g, b] = parts[..] {
            out.insert(name.to_owned(), [r, g, b]);
        }
    }
    assert!(
        out.len() > 15,
        "the palette scan found only {} opaque colours — it has stopped reading chrome.rs, which \
         would make every assertion below vacuous",
        out.len()
    );
    out
}

/// CIE L\*, the perceptual lightness of an sRGB triple.
fn lstar(c: [f32; 3]) -> f32 {
    let lin = |v: f32| {
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let y = 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
    if y > 0.008_856 {
        116.0 * y.cbrt() - 16.0
    } else {
        903.3 * y
    }
}

/// WCAG relative-luminance contrast ratio.
fn ratio(a: [f32; 3], b: [f32; 3]) -> f32 {
    let lin = |v: f32| {
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let y = |c: [f32; 3]| 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
    let (p, q) = (y(a), y(b));
    (p.max(q) + 0.05) / (p.min(q) + 0.05)
}

/// **Every surface that touches another is visibly a different surface.**
///
/// This is the test the 2026-09-03 audit was written to make possible, and the reason it measures
/// **ΔL\*** rather than WCAG contrast: down at near-black the ratio's `+0.05` flare term dominates,
/// so the old ladder scored 1.03:1 between a panel and the window behind it and *also* 1.03:1
/// between two greys nobody could tell apart — the metric could not see the defect it was being
/// asked about. In L\* the same pair measured **1.60**, below the just-noticeable difference for a
/// large flat field, which is what *"the layout overall is muddy as hell"* means in numbers.
///
/// 2.5 is the floor, not the target; the shipped ladder's tightest adjacent pair is 2.95.
#[test]
fn the_ladder_is_a_ladder() {
    let p = palette();
    let get = |name: &str| {
        *p.get(name)
            .unwrap_or_else(|| panic!("`chrome::{name}` is part of the elevation ladder"))
    };
    // Only pairs that actually meet on screen. `OVERLAY_BG` never touches `ROW_HOVER`, and a rule
    // that pretended otherwise would be a rule nobody could satisfy.
    let adjacent = [
        ("VOID", "PANEL_BG"),
        ("PANEL_BG", "HEADER_BG"),
        ("PANEL_BG", "ROW_BG"),
        ("PANEL_BG", "SLOT_BG"),
        ("PANEL_BG", "FOCUS_BG"),
        ("PANEL_BG", "OVERLAY_BG"),
        ("ROW_BG", "ROW_HOVER"),
        ("ROW_HOVER", "ROW_SELECTED"),
        ("ROW_BG", "ROW_PRESSED"),
    ];
    let mut flat = Vec::new();
    for (a, b) in adjacent {
        let d = (lstar(get(a)) - lstar(get(b))).abs();
        if d < 2.5 {
            flat.push(format!("{a} | {b}: ΔL* {d:.2}"));
        }
    }
    assert!(
        flat.is_empty(),
        "these surfaces meet on screen and are not visibly different surfaces (ΔL* < 2.5 is at or \
         below the just-noticeable difference for a large flat field):\n{}",
        flat.join("\n")
    );

    // And the edge has to read against everything it is drawn on, or a bordered panel is a panel
    // with an invisible border — which is where this started.
    let edge = get("PANEL_EDGE");
    for ground in ["VOID", "PANEL_BG", "ROW_BG", "OVERLAY_BG"] {
        let d = (lstar(edge) - lstar(get(ground))).abs();
        assert!(
            d >= 8.0,
            "PANEL_EDGE over {ground} is ΔL* {d:.2}; a hairline needs more separation than a fill \
             step, because it is one pixel wide"
        );
    }
}

/// **Every ink clears 4.5:1 on the grounds it actually renders on.**
///
/// Named per ink rather than "every ink on every ground", because that is a rule this palette
/// cannot satisfy and should not pretend to: `MUTED` means *an excluded pack*, it appears on a panel
/// and on a group band, and requiring it to stay legible on a selected row would force it up to
/// `LABEL`'s value and delete the distinction it exists to carry. The pairing is the decision; this
/// test is the decision written down.
///
/// The audit found three inks failing here, all of them small type — `LABEL` at 3.65:1 is the label
/// column of every label/value row in the editor, at 10 px, which wants *more* contrast than body
/// text rather than less.
#[test]
fn the_ink_clears_its_grounds() {
    let p = palette();
    let get = |name: &str| {
        *p.get(name)
            .unwrap_or_else(|| panic!("`chrome::{name}` is part of the palette"))
    };
    let quiet = ["PANEL_BG", "HEADER_BG", "FOCUS_BG", "ROW_BG", "SLOT_BG", "OVERLAY_BG"];
    let everywhere = [
        "PANEL_BG",
        "HEADER_BG",
        "FOCUS_BG",
        "ROW_BG",
        "SLOT_BG",
        "OVERLAY_BG",
        "ROW_HOVER",
        "ROW_SELECTED",
    ];
    // An ink, and the grounds it is allowed to appear on.
    let contract: [(&str, &[&str]); 8] = [
        ("TEXT", &everywhere),
        ("KEY", &everywhere),
        ("ACCENT", &everywhere),
        ("DANGER", &everywhere),
        ("LABELED", &everywhere),
        ("DIM", &quiet),
        ("LABEL", &quiet),
        // An excluded pack's header, which sits on a panel or on a group band and nowhere else.
        ("MUTED", &["PANEL_BG", "HEADER_BG", "ROW_BG"]),
    ];
    let mut illegible = Vec::new();
    for (ink, grounds) in contract {
        for ground in grounds {
            let r = ratio(get(ink), get(ground));
            if r < 4.5 {
                illegible.push(format!("{ink} on {ground}: {r:.2}:1"));
            }
        }
    }
    assert!(
        illegible.is_empty(),
        "these ink/ground pairs are below 4.5:1. Either raise the ink, or narrow the contract above \
         to say the pair does not happen — but say which:\n{}",
        illegible.join("\n")
    );
}

// ── shape ────────────────────────────────────────────────────────────────────────────────────────

/// **A clickable is one of the three shapes.**
///
/// The 2026-09-03 audit counted **33 clickable node kinds in six shape dialects**, including one
/// (`KitRow`) that was drawn like a row and answered no pointer at all, and one (`ShelfChip`) that
/// sensed hover and never showed it. The remedy was one row, one chip, one button — and the thing
/// that keeps it is that spawning a bare `Button` outside `chrome.rs` fails here.
///
/// The exception is real and narrow: the tab strip is deliberately **not** a `Button`
/// (`a_tab_is_not_a_button`), and two other controls carry their own `Pointer<Click>` for reasons
/// their own comments give. Those spawn no `Button` at all, so they pass without needing a marker.
#[test]
fn a_clickable_is_one_of_the_three_shapes() {
    let mut hand_rolled = Vec::new();
    let source = panel_source();
    for (ix, (file, line_no, line)) in source.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let spawns_button = line.contains("ui_widgets::Button")
            || line.trim() == "UiButton,"
            || line.trim() == "UiButton::default(),";
        if !spawns_button {
            continue;
        }
        let marked = line.contains("CHROME-OK:")
            || (ix > 0 && source[ix - 1].0 == *file && source[ix - 1].2.contains("CHROME-OK:"));
        if !marked {
            hand_rolled.push(format!("{file}:{line_no}: {}", line.trim()));
        }
    }
    assert!(
        hand_rolled.is_empty(),
        "a `Button` spawned outside chrome.rs is a fourth shape. Use `chrome::list_row`, \
         `chrome::chip` or `chrome::button`, which carry `RowRest` and therefore get rest / hover / \
         pressed / selected / disabled from one repainter — or state the decision with \
         `// CHROME-OK: <why>`:\n{}",
        hand_rolled.join("\n")
    );
}

/// **Every overlay declares where it is in the stack.**
///
/// Eight overlays shipped with five z-values and **two with none at all** — the session journal and
/// the problem toast, which therefore stacked by spawn order against siblings that had opinions. A
/// `GlobalZIndex` written as a bare number at a call site is how that happens, so the numbers live
/// in `chrome.rs` and a call site names one.
#[test]
fn every_overlay_declares_its_z() {
    let mut numbered = Vec::new();
    for (file, line_no, line) in panel_source() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let Some(at) = line.find("GlobalZIndex(") else {
            continue;
        };
        let after = &line[at + "GlobalZIndex(".len()..];
        if after
            .trim_start()
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || c == '-')
        {
            numbered.push(format!("{file}:{line_no}: {}", line.trim()));
        }
    }
    assert!(
        numbered.is_empty(),
        "z-order written as a number outside chrome.rs. Name it there, so two overlays cannot \
         disagree about which is in front:\n{}",
        numbered.join("\n")
    );
}
