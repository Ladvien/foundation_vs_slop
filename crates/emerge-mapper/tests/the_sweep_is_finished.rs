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

/// **Every line that is not inside a `#[cfg(test)]` module.**
///
/// It **skips each test module's body** rather than stopping at the first one, and that distinction
/// is the whole value of this file. The first draft split on `"\n#[cfg(test)]"` and took the head —
/// and `tiles.rs`'s first test module sits about a third of the way in, so **two thirds of the
/// largest file in the crate was never scanned by any rule here**. Every ratchet in this file was
/// reporting green over a third of a file.
///
/// `compose_is_read_only.rs` had already been bitten by exactly this and says so: *"a ratchet that
/// cannot fail is worse than no ratchet, because it reads as a guarantee."* This is its
/// implementation, borrowed rather than re-derived — test modules are declared at column zero and
/// closed by a `}` at column zero, which is what makes it a rule rather than a parser.
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
        out.push((name, code_outside_tests(&src)));
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

/// **A global observer must not demand a resource a door owns.**
///
/// This is a crash, not tidiness, and it hid for months. An observer registered with `add_observer`
/// fires for **any** matching event anywhere in the application — and `Project`, `OpenMap`, `Door`
/// and `Mode` are inserted when a door opens and **removed by `screen::close_the_door`**. On
/// `Screen::Menu` they do not exist, and in Bevy 0.19 a missing `Res<T>` **panics its system** rather
/// than skipping it.
///
/// Every one of these observers already had the real guard — a `Query` answering "is this entity
/// mine", which a menu row fails. It just could not run: parameters are validated *before* the body.
///
/// It was invisible for exactly as long as `chrome::list_row` was only ever called inside an editor
/// panel. The moment the menu adopted the shared row vocabulary (FVS-S-34a) its first click took the
/// whole application down, and `FeathersPlugins` being in the graph now means any Feathers widget
/// could have done the same.
///
/// The fix at each site is `Option<Res<..>>` and an early return after the entity check — which is
/// what `CLAUDE.md` has said all along.
/// **Every `fn` and its parameter list, by matching parentheses.**
///
/// The first cut looked for the literal `") {"` that ends a signature — and a function returning
/// something ends `") -> T {"`, so the search ran on to the *next* signature's terminator and
/// skipped every function in between. It found eight observers in `tiles.rs` and silently missed the
/// one this whole test exists for. Caught by breaking the code on purpose and watching the lint stay
/// green, which is the only way that class of hole is ever found.
fn signatures(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut from = 0usize;
    while let Some(at) = src[from..].find("\nfn ") {
        let start = from + at + 1;
        let Some(open_rel) = src[start..].find('(') else { break };
        let open = start + open_rel;
        let name = src[start + 3..open].trim().to_string();
        let mut depth = 0usize;
        let mut i = open;
        let mut close = None;
        while i < bytes.len() {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let Some(close) = close else { break };
        out.push((name, src[open..=close].to_string()));
        from = close;
    }
    out
}

#[test]
fn a_global_observer_never_demands_a_doors_resource() {
    // Inserted by `open_the_door`, removed by `close_the_door` — see
    // `screen::OWNERSHIP`, where these four are the `Project` class.
    const DOOR_OWNED: &[&str] = &["Project", "OpenMap", "Door", "Mode"];

    let mut rogue = Vec::new();
    let mut seen = 0usize;
    for (file, src) in panels() {
        for (name, params) in signatures(&src) {
            // Only observers — the ones that fire application-wide.
            if !params.contains("On<") {
                continue;
            }
            seen += 1;
            for ty in DOOR_OWNED {
                for kind in ["Res<", "ResMut<"] {
                    let needle = format!("{kind}{ty}>");
                    let optional = format!("Option<{kind}{ty}>>");
                    if params.contains(&needle) && !params.contains(&optional) {
                        rogue.push(format!("{file}::{name} takes {kind}{ty}>"));
                    }
                }
            }
        }
    }
    assert!(
        seen >= 10,
        "the scan found only {seen} observers — if the signature parser has stopped seeing them, \
         the assertion below is vacuous"
    );
    assert!(
        rogue.is_empty(),
        "these global observers demand a resource that belongs to a DOOR. They fire for any matching \
         event anywhere, including on `Screen::Menu` where `close_the_door` has removed it — and a \
         missing `Res<T>` panics rather than skipping, so the first click outside the editor takes \
         the application down. Take `Option<Res<..>>` and return early after the entity check, which \
         is the guard these already have and cannot reach:\n{}",
        rogue.join("\n")
    );
}

/// **Nothing carries a `TabIndex` while routing is by `Context`.**
///
/// `keys.rs`'s header records the decision taken 2026-08-18: routing is by `Live(Context, Stance)`,
/// not by focus, because a second answer to "who gets this key" is what this crate's rules forbid
/// and `Live` is decided once per frame in `Phase::Sense` precisely so ownership cannot move
/// mid-frame.
///
/// `FeathersPlugins` brings `acquire_focus` and `click_to_focus` and they are **inert** — they only
/// do anything for an entity with a `TabIndex`, and nothing has one. That is the correct amount of
/// inert, and this is what keeps it true.
///
/// # Why a test rather than a note
///
/// The first thing a `TabIndex` would turn on is click-to-focus, and FVS-R-25 already measured what
/// that does here: `bevy_picking` writes `Hovered` from the **window's** cursor, which
/// `view::sense_pointer` deliberately never moves — so an agent clicking would focus whatever the
/// *physical* pointer happens to be resting on. That finding is three documents deep and would not
/// be found by somebody adding a focus ring on a Tuesday.
///
/// So adding one is allowed, and it costs a line saying the decision was reopened deliberately:
/// `// FOCUS-DECISION-REOPENED: <why>`. The same `CHROME-OK` / `SORT-OK` shape as everything else
/// here — the point is not to forbid it, it is that nobody does it by accident.
#[test]
fn focus_traversal_stays_off_until_somebody_reopens_it() {
    let mut rogue = Vec::new();
    for (file, src) in panels() {
        let lines: Vec<&str> = src.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let l = line.trim();
            if l.starts_with("//") || !l.contains("TabIndex") {
                continue;
            }
            let marked = l.contains("FOCUS-DECISION-REOPENED:")
                || i.checked_sub(1)
                    .and_then(|k| lines.get(k))
                    .is_some_and(|p| p.contains("FOCUS-DECISION-REOPENED:"));
            if !marked {
                rogue.push(format!("{file}:{}: {l}", i + 1));
            }
        }
    }
    assert!(
        rogue.is_empty(),
        "a `TabIndex` turns on focus traversal, and this editor routes by `Context` on purpose — see \
         `keys.rs`'s header. It also turns on click-to-focus, which FVS-R-25 measured as broken for \
         agents here: `bevy_picking` writes `Hovered` from the WINDOW's cursor, which \
         `view::sense_pointer` never moves, so an injected click would focus whatever the physical \
         pointer rests on. Whatever gains focus must be reachable without a click. If that is \
         understood and wanted, say so with `// FOCUS-DECISION-REOPENED: <why>`:\n{}",
        rogue.join("\n")
    );
}
