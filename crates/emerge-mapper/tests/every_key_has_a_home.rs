//! **A verb's badge lands on a real node, or the census is describing a place that does not exist.**
//!
//! `keys::Home` made *"every binding says where its badge goes"* a compile error to get wrong, and
//! `keys::tests::every_control_id_is_named_by_a_binding` closes the other end of the census: a
//! `ControlId` nothing homes to is a word with no meaning. Neither can see the half that lives in the
//! panels — whether anything actually **attaches** `chrome::Control(id)` to a node.
//!
//! That gap is not hypothetical. The whole point of drawing a chord on the thing it acts on is that
//! the chord appears; an id declared in the census and never attached is a verb that silently has no
//! badge, which is the exact failure the badge overlay replaced a table to fix.
//!
//! `tests/headless.rs::every_control_the_census_homes_a_verb_at_is_on_screen` is the stronger
//! statement — it boots the editor and reads the rects. This is the cheap one that fails in a
//! second without a GPU-free app boot, and it names the missing id rather than a count.
//!
//! Scanning style is `chrome_census.rs`'s, including its warning: the scan must skip **past** each
//! `#[cfg(test)]` module rather than stopping at the first, or it reads nothing and passes vacuously.

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every non-test, non-comment line of every `src/*.rs` except `keys.rs`, which is where the
/// vocabulary is *declared* — finding `ControlId::Palette` there would only prove the enum has that
/// variant.
///
/// # A comment used to satisfy the attachment check
///
/// This file's own doc says *"a matcher that quietly matched nothing would pass forever"*, and a
/// string scan over prose is that matcher one step short. This crate's comments name every
/// `ControlId` there is — `keys::ControlId`'s doc, `badges`' placement notes, half the panel
/// builders — so an id whose attachment was deleted would still be found, in the sentence
/// explaining what the deleted attachment was for. `no_system_writes_every_frame::count_writes`
/// strips `//` lines for the same reason; this does it the same way.
fn panel_source() -> String {
    let dir = src_dir();
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .filter(|p| p.file_name().is_some_and(|n| n != "keys.rs"))
        .collect();
    names.sort();

    let mut out = String::new();
    for path in names {
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
            // Comments are not attachments. Leading `//` only, after trimming: a `//` inside a
            // string literal is not a comment, and no attachment is ever written inside one.
            if lines[i].trim_start().starts_with("//") {
                i += 1;
                continue;
            }
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
        }
    }
    assert!(
        out.lines().count() > 5_000,
        "the scan saw only {} lines of panel source — it has stopped reading the crate, which \
         would make the assertion below vacuous",
        out.lines().count()
    );
    out
}

/// **Every control the census names is attached to something.**
#[test]
fn every_control_the_census_names_is_attached_to_something() {
    let src = panel_source();
    let mut orphans = Vec::new();
    for id in emerge_mapper::keys::ControlId::ALL {
        // **The id named anywhere in panel source, not only inside `Control(`.**
        //
        // The first cut matched `Control(…ControlId::X)` literally and flagged three ids that are
        // attached perfectly well — `CellVerb::control()` maps the three cell verbs onto their chips
        // in one match, which is *better* than a literal at each call site and is exactly the shape
        // this crate prefers. A string scan cannot see through a call, so it asks the weaker
        // question: does any panel name this id at all.
        // `every_control_the_census_homes_a_verb_at_is_on_screen` is the one that boots the editor
        // and checks the node is really there.
        if !src.contains(&format!("ControlId::{id:?}")) {
            orphans.push(format!("{id:?}"));
        }
    }
    assert!(
        orphans.is_empty(),
        "the census homes verbs at these controls and no panel attaches `chrome::Control` for them, \
         so those verbs get no badge and nothing else would say so. Mark the node the verb acts \
         through, or move the binding's `Home` to a region:\n  {}",
        orphans.join("\n  ")
    );
}

/// **And the scan finds attachments at all.**
///
/// The companion assertion `chrome_census.rs` taught: a matcher that quietly matched nothing would
/// pass forever. If this ever finds zero, the string shape above has drifted and the test above is
/// checking nothing.
///
/// # It used to count prose and a struct declaration
///
/// It matched the bare substring `Control(`, which is `Home::Control(` in `badges.rs`, the
/// `pub struct Control(pub keys::ControlId)` declaration in `chrome.rs`, and every sentence in this
/// crate that mentions the word followed by a bracket. So the floor could be cleared without a
/// single panel attaching anything — by a paragraph. The two spellings below are the two an
/// attachment actually has: `chrome::Control(..)` from outside that module, and
/// `Control(keys::ControlId::..)` from inside it.
#[test]
fn the_scan_actually_finds_attachments() {
    let src = panel_source();
    let n = src.matches("chrome::Control(").count() + src.matches("Control(keys::ControlId::").count();
    assert!(
        n >= emerge_mapper::keys::ControlId::ALL.len(),
        "the scan found {n} `chrome::Control(..)` attachment(s) across the panels, fewer than the \
         {} ids the census names — the matcher has stopped seeing them, or the panels have stopped \
         making them. One site may serve several ids (`editor::Field::control` and \
         `tiles::CellVerb::control` each map a set onto one insert), so this is a floor rather than \
         a count.",
        emerge_mapper::keys::ControlId::ALL.len()
    );
}
