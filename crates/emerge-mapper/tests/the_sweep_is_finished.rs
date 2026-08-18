//! **Nothing spawns its own scroll container or its own tab strip.**
//!
//! `docs/2026-08-17-mapper-ui-audit.md`'s verdict was that four tabs were drifting into four
//! dialects, and the design's §8.3 names the failure mode of fixing it halfway: *"a widget set
//! adopted by one tab and not the others makes it five."* The overhaul closed that by construction —
//! `chrome::scroll_list` is the one scroll container and `chrome::Frame` is the one layout — but
//! *by construction* is a claim about today, and the audit exists because a claim like that stopped
//! being true four times over.
//!
//! So it is a source ratchet, in the shape `every_list_follows_its_selection.rs` and
//! `chrome_census.rs` already use: the rule that was a belief becomes a rule that fails a build.
//!
//! # What it does not claim
//!
//! It reads source, so it sees what is *written*, not what is spawned at runtime. That is the same
//! limitation `every_list_follows_its_selection.rs` states about itself, and the same answer:
//! `tests/headless.rs::every_pane_that_clips_can_scroll` boots the editor and asserts the runtime
//! half — any node that clips has a `ScrollArea`, whoever made it.

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `src/*.rs` except `chrome.rs`, which is where the shared shapes are allowed to live.
fn panels() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let dir = std::fs::read_dir(src_dir()).expect("src/");
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if name == "chrome.rs" {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        // Test modules are fixtures, not panels.
        let live = src.split("\n#[cfg(test)]").next().unwrap_or(&src).to_string();
        out.push((name, live));
    }
    out
}

/// **A scrolling node is `chrome::scroll_list`'s to make.**
///
/// The defect this remembers: `compose.rs` hand-copied `scroll_list` field for field and omitted the
/// one component that makes it scrollable, on the longest generated pane in the editor — so the
/// overflow was unreachable by any input. Three tabs had copies; the one that dropped `ScrollArea`
/// is why the builder exists at all.
#[test]
fn nobody_spawns_their_own_scroll_container() {
    let mut rogue = Vec::new();
    for (file, src) in panels() {
        for (i, line) in src.lines().enumerate() {
            let l = line.trim();
            if l.starts_with("//") {
                continue;
            }
            if l.contains("Overflow::scroll") || l.contains("OverflowAxis::Scroll") {
                rogue.push(format!("{file}:{}: {l}", i + 1));
            }
        }
    }
    assert!(
        rogue.is_empty(),
        "these spawn their own scrolling node instead of calling `chrome::scroll_list`. That builder \
         carries the `ScrollArea` a hand copy forgets, the `min_height: 0` without which `overflow` \
         has nothing to clip, and the scrollbar — and a pane that clips without any of them is \
         unreachable by input with nothing on screen to say so:\n{}",
        rogue.join("\n")
    );
}

/// **A strip of chips is `chrome`'s or `tiles`'s, and there are exactly two of them.**
///
/// The door strip (`tiles::spawn_tab_strip`) and the shelf strip (`tiles::shelf_strip`) are the two
/// this editor has, and they are different things: one switches *panels*, the other switches which
/// *list* a panel is showing. A third hand-rolled strip is the fifth dialect the audit named.
#[test]
fn there_are_exactly_two_strips_and_both_are_named() {
    let src = std::fs::read_to_string(src_dir().join("tiles.rs")).expect("tiles.rs");
    for wanted in ["fn spawn_tab_strip(", "fn shelf_strip("] {
        assert!(
            src.contains(wanted),
            "`{wanted}` is gone. If a strip was renamed, rename it here too — this test is the \
             list of strips this editor is allowed to have."
        );
    }
    // Anything else building a row of chips out of `Tab`-like markers would be a third.
    let others: Vec<String> = panels()
        .into_iter()
        .flat_map(|(file, src)| {
            src.lines()
                .enumerate()
                .filter(|(_, l)| {
                    let t = l.trim();
                    !t.starts_with("//") && t.contains("fn ") && t.contains("_strip(")
                })
                .map(|(i, l)| format!("{file}:{}: {}", i + 1, l.trim()))
                .collect::<Vec<_>>()
        })
        .filter(|s| !s.contains("spawn_tab_strip") && !s.contains("shelf_strip"))
        .collect();
    assert!(
        others.is_empty(),
        "a third strip. The two this editor has switch different things — panels, and which list a \
         panel shows — and a third is the fifth dialect `docs/2026-08-17-mapper-ui-audit.md` \
         named:\n{}",
        others.join("\n")
    );
}

/// **A panel is `chrome::panel_root`'s to place.**
///
/// Panels used to be `PositionType::Absolute` at fixed widths floating over the world, which is why
/// nothing filled the window. `chrome::Frame` owns position now, and a panel that goes back to
/// placing itself is that layout returning one panel at a time.
///
/// # Why a marker rather than a ban
///
/// The first cut flagged every absolute node and caught two that are perfectly correct: a world-space
/// slot label projected onto the screen, and a hover overlay stacked on a plot image. Neither is a
/// *panel* — they are things that must be placed by something other than flow, which is what
/// absolute positioning is for. A lint that calls those defects is a lint somebody turns off.
///
/// So it takes the `CHROME-OK` / `SORT-OK` shape this repo already uses: absolute is allowed, and it
/// costs one line saying why. The decision goes on the record instead of being absent.
#[test]
fn nobody_places_their_own_panel() {
    let mut rogue = Vec::new();
    for (file, src) in panels() {
        let lines: Vec<&str> = src.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let l = line.trim();
            if l.starts_with("//") || !l.contains("PositionType::Absolute") {
                continue;
            }
            let marked = l.contains("PLACES-ITSELF-OK:")
                || i.checked_sub(1).and_then(|k| lines.get(k)).is_some_and(|p| p.contains("PLACES-ITSELF-OK:"))
                || i.checked_sub(2).and_then(|k| lines.get(k)).is_some_and(|p| p.contains("PLACES-ITSELF-OK:"));
            if !marked {
                rogue.push(format!("{file}:{}: {l}", i + 1));
            }
        }
    }
    assert!(
        rogue.is_empty(),
        "these position themselves absolutely with no reason on the record. `chrome::Frame` owns \
         where a PANEL goes — the floating fixed-width layout is what left two fifths of the window \
         as ground nothing used. If this is not a panel, say so with \
         `// PLACES-ITSELF-OK: <why>` on the line or just above it:\n{}",
        rogue.join("\n")
    );
}
