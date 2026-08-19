//! **Every scrollable list keeps its highlight on screen.**
//!
//! A source ratchet, and it exists because this defect has been reported twice and passed its tests
//! both times.
//!
//! - 2026-08-14, mid-guide: *"if I arrow down and the scroll view, it just goes off the screen. The
//!   scroll doesn't actually happen."*
//! - 2026-08-16: *"I still have the same bug where I press the arrow keys in the scroll view area,
//!   and it doesn't fall on my selection. Can we fix that and get it pinned across the board?"*
//!
//! # Why a behaviour test did not catch it
//!
//! The arithmetic (`chrome::scroll_to_reveal`) was always right and always unit-tested. What was
//! broken was the *arming*: both followers watched `is_changed()` on the resource holding the
//! selection, and both `EditorState` and `ImportState` are written most frames — a status line, a
//! hover, a preview watchdog. So the flag re-armed every frame and the scroll never ran.
//!
//! In a headless test none of that churn exists: only the systems under test run, `is_changed` goes
//! false on the next frame, and the correction fires. **The test was measuring a world that only
//! exists in tests.** `chrome::Follow` fixes the arming; this file makes sure the fix is applied
//! everywhere and stays that way.
//!
//! # What it pins
//!
//! Two things a running editor cannot tell you and a unit test will not notice:
//!
//! 1. Every marker handed to `chrome::scroll_list` has a system that scrolls it.
//! 2. No follower arms itself on `is_changed()` again.
//! 3. What is on screen is decided in ONE place, so the rows drawn and the rows the arrows walk
//!    cannot drift apart.

use std::path::Path;

fn sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{dir:?}: {e}")) {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        if path.extension().is_some_and(|e| e == "rs") {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            out.push((name, text));
        }
    }
    out
}

/// **A scrollable list without a follower is a list whose highlight can leave the screen.**
///
/// The list's marker is the link: `scroll_list(p, RigList)` spawns it, and a follower has to query
/// `With<RigList>` to scroll it. So every marker passed to `scroll_list` must appear in a
/// `With<..>` beside a `ScrollPosition`.
///
/// `RigList` was exactly this case — the one scrollable list in the editor with no follower at all,
/// found only because somebody asked for the fix to be applied "across the board".
#[test]
fn every_scrollable_list_has_something_that_follows_its_selection() {
    let src = sources();
    let all: String = src.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");

    let mut markers: Vec<(String, String)> = Vec::new();
    for (file, text) in &src {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // **`scroll_list` takes content, not only lists**, and the two want different things.
            // A pane whose HEIGHT is variable — the tiles detail block is a size, a layer, four
            // rows of chips and however many sentences its findings need — scrolls because it
            // would otherwise run off the bottom edge, and it has no selection for anything to
            // follow. Wiring a follower to it would be a system with nothing to key on.
            //
            // So the exemption is stated at the spawn, in the register `SORT-OK` and `CHROME-OK`
            // already use here. Read from the whole comment block above the call rather than one
            // line up: the reason is a paragraph, and a rule that forced it onto the last line
            // would decide where the prose ends. Anything that does not say `FOLLOW-OK:` is a
            // list, and a list must follow its selection.
            let exempt = line.contains("FOLLOW-OK:")
                || lines[..i]
                    .iter()
                    .rev()
                    .take_while(|l| l.trim_start().starts_with("//"))
                    .any(|l| l.contains("FOLLOW-OK:"));
            if exempt {
                continue;
            }
            let Some(rest) = line.split_once("scroll_list(").map(|(_, r)| r) else {
                continue;
            };
            // `scroll_list(p, RigList)` — the second argument is the marker. It may also be a
            // BUNDLE: `scroll_list(p, (DetailPane, CopyPane(..)))`, where the marker to follow is
            // the first component. Splitting on ',' alone yielded `(DetailPane`, which matches no
            // `With<..>` anywhere and so reported a real pane under a name that does not exist.
            let Some(arg) = rest.split(',').nth(1) else { continue };
            let marker = arg
                .trim()
                .trim_start_matches('(')
                .trim_end_matches(&[')', ';'][..])
                .trim()
                .to_owned();
            if marker.is_empty() || marker.starts_with("marker") {
                continue; // the definition itself
            }
            markers.push((file.clone(), marker));
        }
    }

    assert!(
        markers.len() >= 3,
        "expected to find the editor's scrollable lists; found {markers:?}. If `scroll_list` was \
         renamed, rename it here too — a ratchet that silently matches nothing is worse than none."
    );

    // **A follower is a query holding BOTH the marker and a `ScrollPosition`.** Checking only that
    // the marker appears somewhere is too weak, and provably so: `RigList` is named by a second,
    // unrelated query, so the first version of this test passed with the follower deleted. A ratchet
    // that cannot fail is worse than no ratchet, so it is checked against the defect put back.
    let follows = |marker: &str| -> bool {
        let lines: Vec<&str> = all.lines().collect();
        lines.iter().enumerate().any(|(i, l)| {
            l.contains("ScrollPosition")
                && lines[i.saturating_sub(6)..(i + 6).min(lines.len())]
                    .iter()
                    .any(|near| near.contains(&format!("With<{marker}>")))
        })
    };

    for (file, marker) in &markers {
        assert!(
            follows(marker),
            "`{marker}` (spawned in {file}) is a scrollable list that nothing follows. The arrows \
             move its highlight and the list stands still, so the selection walks off the screen — \
             reported from the keyboard twice. Add a system querying \
             `(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<{marker}>` that calls \
             `chrome::scroll_to_reveal`, armed by `chrome::Follow`."
        );
    }
}

/// **No follower may arm itself on `is_changed()`.**
///
/// That is the defect, stated directly. `Follow` keys on the selection, which unrelated writes to
/// the resource cannot perturb; `is_changed()` keys on the resource, which they are.
///
/// Scoped to the scrolling systems rather than banning `is_changed` outright — it is the right tool
/// almost everywhere else in this crate, and a rule that flagged every use would be turned off.
#[test]
fn no_follower_arms_itself_on_a_resource_change() {
    for (file, text) in sources() {
        for (i, line) in text.lines().enumerate() {
            if !line.contains("scroll_to_reveal") {
                continue;
            }
            // The 40 lines above a `scroll_to_reveal` call are its system's body.
            let start = i.saturating_sub(40);
            let body: String = text.lines().skip(start).take(i - start).collect::<Vec<_>>().join("\n");
            assert!(
                !body.contains("is_changed()"),
                "the follower in {file} arms on `is_changed()`. Both `EditorState` and \
                 `ImportState` are written most frames, so that flag is re-armed every frame and \
                 the scroll never happens — the exact bug reported on 2026-08-14 and again on \
                 2026-08-16. Key it on the selection with `chrome::Follow` instead."
            );
        }
    }
}

/// **Folding is decided once, by `pack_is_open`.**
///
/// It was decided twice — inline where the rows are drawn, and not at all where the arrows walk them
/// — so the highlight stepped into packs nobody could see. Reported at the keyboard, 2026-08-16:
/// *"whenever I scroll up, it doesn't skip collapsed groups."*
///
/// Two copies of a visibility rule always drift, because only one of them is in front of you when
/// you change it. This pins the single reader by pinning the single writer: `folded_packs` may be
/// consulted only inside the function that answers the question, and everywhere else asks that.
///
/// Writes are exempt — inserting and removing is how folding is *toggled*, and a rule that banned
/// those would ban the feature.
#[test]
fn what_is_on_screen_is_decided_in_one_place() {
    for (file, text) in sources() {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // Code, not prose: a comment naming the call is how it gets explained, and a ratchet
            // that flags its own explanation teaches people to delete it.
            if !line.contains("folded_packs.contains") || line.trim_start().starts_with("//") {
                continue;
            }
            // The 25 lines above tell us whose body this is.
            let start = i.saturating_sub(25);
            let body = lines[start..i].join("\n");
            assert!(
                body.contains("fn pack_is_open"),
                "{file}:{} reads `folded_packs` outside `pack_is_open`. Whether a pack's rows are \
                 on screen has to be one answer: it was two — the renderer decided it inline and the \
                 arrow walk never asked at all — and the highlight walked into folded packs, where \
                 `Accept` would then have imported a mesh nobody could see.",
                i + 1
            );
        }
    }
}

/// **The follower looks up the row it armed on** — one decision, not two that agree until they do
/// not.
///
/// `chrome::Follow` is keyed on `Selected::now`, and the system then has to find that row's geometry
/// to scroll to it. Deriving "which row is selected" a second time inside the lookup is how the two
/// drift, and they did: once headings became walkable the lookup checked `focused_pack` first while
/// `Selected::now` ranks the library above it, so focusing the imported list with a heading still
/// remembered scrolled to the heading and left the selection off screen.
///
/// Reported at the keyboard three times in a row for three different symptoms, all this shape.
#[test]
fn the_follower_asks_the_same_question_it_armed_on() {
    let text = sources()
        .into_iter()
        .find(|(name, _)| name == "tiles.rs")
        .map(|(_, t)| t)
        .unwrap_or_else(|| panic!("tiles.rs is where the shared list lives"));

    let lines: Vec<&str> = text.lines().collect();
    let scroll = lines
        .iter()
        .position(|l| l.contains("scroll_to_reveal"))
        .unwrap_or_else(|| panic!("the panel's follower calls `scroll_to_reveal`"));
    // The system body above that call must reach its row through `Selected::now`.
    let body = lines[scroll.saturating_sub(60)..scroll].join("\n");
    assert!(
        body.contains("Selected::now"),
        "the follower re-derives which row is selected instead of asking `Selected::now` — the \
         value it armed on. Two copies of that precedence drift the moment a new cursor state is \
         added, which is exactly what happened when pack headings became walkable."
    );
}
