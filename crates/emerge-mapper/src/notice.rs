//! **The two verbs that act on what a tab has just said**: read it out, and take it down.
//!
//! [`crate::chrome::Status`] is where a message lives and what makes a refusal stick. This is the
//! other end — the author is finished with it, either because they have read it (`Esc`) or because
//! they have taken it somewhere it can be acted on (`Cmd+C`).
//!
//! # Why one module knows about all four tabs
//!
//! Because the alternative is four copies of one behaviour. Each tab keeps its own `Status` inside
//! the state it already owns, so a system that reads "the live tab's status" has to name all four —
//! and `editor::not_typing` set that precedent for exactly this shape, listing five kinds of state
//! from two tabs with the note that *"every field is listed here and nowhere else, so adding one is
//! adding a line."* Four near-identical `dismiss` systems, one per tab file, is the drift
//! `crate::chrome` exists to prevent, one level up.
//!
//! # Copying is for an agent, and that is why it is Global
//!
//! `bevy_ui` has no selectable text. A refusal an author cannot get out of the window is one that
//! has to be retyped out of a screenshot before anybody else can help with it — so the verb belongs
//! wherever a refusal can happen, which is now every tab. It was `Context::Meshes`, copying that
//! tab's detail pane.

use bevy::prelude::*;

use crate::keys::{self, Action};
use crate::tiles::Mode;

/// **A panel whose text is worth copying**, and which tabs it belongs to.
///
/// The tabs are on the marker rather than inferred from the panel's visibility, because a hidden
/// panel's `Text` still exists — `chrome::panel_root` hides with `Display::None`, which keeps the
/// entities. Asking "is this node drawn" would mean walking ancestors; naming the tabs is one field.
///
/// A list for [`crate::chrome::ProblemBanner`]'s reason: the Meshes and Tiles tabs share one detail
/// pane, and tagging it `Meshes` made `Cmd+C` on Tiles harvest the status lines and none of the tile
/// — no id, no envelope, no member list — in an editor where `bevy_ui` offers no other way to get
/// that text out of the window.
#[derive(Component, Clone, Copy)]
pub struct CopyPane(pub &'static [Mode]);

/// What the notice panels currently show, so they are rebuilt on a change and not on a frame.
///
/// `chrome::ShowingFor` does the same for the shortcuts overlay, for the same reason: *"the rows are
/// static text and respawning them sixty times a second would be sixty times the work for one
/// picture."* It matters more here — `EditorState` is written at drag rate, so a system gated on
/// `is_changed()` would rebuild the log while the cursor moves.
#[derive(Resource, Default, PartialEq, Eq)]
struct Showing {
    tab: Option<Mode>,
    lines: Vec<String>,
}

pub struct NoticePlugin;

impl Plugin for NoticePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Showing>().add_systems(
            Update,
                ((
                (dismiss, copy_out).in_set(keys::Phase::Act),
                // After the verbs, so a problem raised this frame is on screen this frame.
                paint_notices.after(keys::Phase::Act),
            ),)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            );
    }
}

/// **Both views of the live tab's problems: the banner and the log.**
///
/// One system rather than four, and one for both views rather than two, because they are one list —
/// the banner is its newest entry and the log is the run. Four tabs each painting their own was how
/// the severity rule drifted four ways in the first place.
///
/// Only the live tab is painted, and that is enough: `chrome::panel_root` hides an inactive tab with
/// `Display::None`, so the other three panels are not on screen to be wrong.
fn paint_notices(
    mut commands: Commands,
    mode: Res<Mode>,
    editor: Res<crate::editor::EditorState>,
    import: Res<crate::tiles::ImportState>,
    bench: Res<crate::anim_tab::BenchState>,
    compose: Res<crate::compose::ComposeState>,
    mut showing: ResMut<Showing>,
    mut banners: Query<(&mut Node, &mut Text, &crate::chrome::ProblemBanner)>,
    logs: Query<(Entity, &crate::chrome::ProblemLog)>,
    lines: Query<Entity, With<crate::chrome::ProblemLogLine>>,
    mut nodes: Query<&mut Node, (With<crate::chrome::ProblemLog>, Without<crate::chrome::ProblemBanner>)>,
) {
    let tab = *mode;
    let status = match tab {
        Mode::Map => &editor.status,
        // **Both tabs report through `ImportState`.** The Tiles tab's own verbs write there too —
        // one status line for one file's worth of work, rather than a second channel to keep in step.
        Mode::Meshes | Mode::Tiles => &import.status,
        Mode::Anim => &bench.status,
        Mode::Compose => &compose.status,
    };

    // The banner is a `Text` write guarded on its own content, so it is cheap every frame.
    for (mut node, mut text, banner) in banners.iter_mut() {
        // **A banner that is not the live tab's is hidden, not skipped.** Skipping was safe only
        // while every banner sat in a panel `apply_mode` hid for it — the Meshes and Tiles tabs now
        // share one panel, so a refusal raised on one would go on showing after switching to the
        // other. Hiding costs a `Display` compare and stops the panel's own visibility being
        // load-bearing for whether a stale line is on screen.
        let mine = banner.0.contains(&tab);
        let want_display =
            if mine && status.has_problem() { Display::Flex } else { Display::None };
        if !mine {
            if node.display != want_display {
                node.display = want_display;
            }
            continue;
        }
        if node.display != want_display {
            node.display = want_display;
        }
        if let Some(newest) = status.problems().last() {
            // The glyph is `▲` and not `⚠`: `FiraMono-Regular.ttf` has no U+26A0 (measured), and a
            // missing codepoint draws as a tofu box.
            let want = format!("▲  {}", newest.line());
            if text.0 != want {
                text.0 = want;
            }
        }
    }

    // The log is a rebuild, so it is guarded on what it would produce.
    let want: Vec<String> = status
        .problems()
        .iter()
        .rev()
        .map(|p| p.line())
        .chain(match status.dropped() {
            // Named rather than silently forgotten — this crate's caps refuse and name, and a log
            // that quietly dropped its oldest entries would read complete and not be.
            0 => None,
            n => Some(format!("+{n} earlier, dropped at {}", crate::chrome::MAX_PROBLEMS)),
        })
        .collect();
    if showing.tab == Some(tab) && showing.lines == want {
        return;
    }
    showing.tab = Some(tab);
    showing.lines = want.clone();

    for e in &lines {
        commands.entity(e).despawn();
    }
    for (entity, log) in &logs {
        // **A log that is not the live tab's is hidden, not skipped** — the banner's rule above, for
        // the same reason and with the same cost. The despawn just above takes every log line in the
        // editor, so a log left `Display::Flex` by an earlier tab is an empty bordered box where the
        // problem list used to be. Skipping made that unreachable only while each log sat in a panel
        // of its own.
        let mine = log.0.contains(&tab);
        if let Ok(mut node) = nodes.get_mut(entity) {
            let display =
                if mine && !want.is_empty() { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        }
        if !mine || want.is_empty() {
            continue;
        }
        commands.entity(entity).with_children(|p| {
            // **Marked, because the rebuild sweeps by marker.** The despawn above takes every
            // `ProblemLogLine`; this heading carried no marker, so each rebuild appended another
            // copy and none was ever removed. A batch that raised one problem per frame stacked
            // ~190 headings down the panel before anybody could read the one message underneath
            // them. The `Esc clears them` line below already had the marker, which is why it did
            // not multiply and why the fault looked like a heading bug rather than a sweep bug.
            crate::chrome::section(p, "PROBLEMS ON THIS TAB")
                .insert(crate::chrome::ProblemLogLine);
            // **Say how to take them down.** These are sticky by design — a refusal that vanished
            // before it was read is the failure `Status` exists to prevent — but sticky with no
            // stated way out reads as an editor filling up with complaints. An author watching the
            // same refusal collect an `(x14)` has no reason to know `Esc` is the answer, which is
            // Cockburn et al.'s intermodal-transition point that the shortcuts overlay already cites:
            // a fast path offered beside no slow one is not offered.
            p.spawn((
                bevy::prelude::Text::new(format!(
                    "{} clears them",
                    crate::keys::chord(crate::keys::Action::Cancel)
                )),
                bevy::prelude::TextColor(crate::chrome::DIM),
                bevy::prelude::TextFont::from_font_size(crate::chrome::text::LABEL),
                crate::chrome::ProblemLogLine,
            ));
            for (i, line) in want.iter().enumerate() {
                // Newest first, and it is the one the banner is also showing — so the top of the
                // log and the block above it agree, and the older ones recede.
                let colour = if i == 0 { crate::chrome::DANGER } else { crate::chrome::DIM };
                crate::chrome::problem_log_line(p, line, colour);
            }
        });
    }
}

/// Everything a tab has to say, in the order an agent wants to read it.
///
/// **The problem first.** The old Tiles-only copy put the status line last, after the pane, on the
/// argument that it was the line the pane did not hold — true, but it buried the one line somebody
/// pasting this into a message is usually asking about.
fn harvest(
    status: &crate::chrome::Status,
    tab: Mode,
    panes: &Query<(Entity, &CopyPane)>,
    roots: &[Entity],
    children: &Query<&Children>,
    texts: &Query<&Text>,
    nodes: &Query<&Node>,
) -> Vec<String> {
    let mut lines = Vec::new();
    // **The whole log, not just the newest.** The banner shows one; a copy is what somebody pastes
    // into a message for help, and the run is what makes a sequence of failures diagnosable.
    // Newest first, matching the order on screen.
    for p in status.problems().iter().rev() {
        lines.push(format!("problem: {}", p.line()));
    }
    if status.dropped() > 0 {
        lines.push(format!(
            "problem: (+{} earlier, dropped at {})",
            status.dropped(),
            crate::chrome::MAX_PROBLEMS
        ));
    }
    // **Everything on the screen, not just this tab's detail pane.**
    //
    // Asked for at the keyboard, 2026-08-18, and the reason was visible in the request: the author
    // was hand-transcribing the mesh list and the header into a message because the copy did not
    // reach them. It carried the `CopyPane` panes and nothing else — so the id, the measurements
    // and the findings came through, while the list they were chosen from, the kit the header
    // names, and the tab you were on did not. A paste that needs a covering note to be legible is
    // not a copy of the screen.
    //
    // Walked from the frame's root in child order, which IS reading order: `chrome::spawn_frame`
    // builds a column of chrome bar, door strip, body — itself a row of left dock, viewport, right
    // dock — and status bar. Overlays are roots of their own and follow.
    for root in roots {
        collect_visible(*root, children, texts, nodes, &mut lines);
    }
    // The note is on screen and therefore already harvested; `CopyPane` is now only a marker of
    // where the panes are, which nothing here needs to consult.
    let _ = (panes, tab);
    lines
}

/// Depth-first text harvest under one UI node, in child order — which is the order it reads.
///
/// **`Display::None` prunes the whole subtree**, and that is what makes walking from the root safe
/// rather than absurd: every tab's panel exists at once and the four that are not showing are
/// hidden exactly that way (`chrome::panel_root`, which chose `Display::None` over `Visibility` so
/// a hidden panel holds no layout and answers no hover). Without this check a copy of the Meshes
/// tab would arrive carrying the Map, Anim and Compose panels underneath it.
fn collect_visible(
    root: Entity,
    children: &Query<&Children>,
    texts: &Query<&Text>,
    nodes: &Query<&Node>,
    out: &mut Vec<String>,
) {
    if nodes.get(root).is_ok_and(|n| n.display == Display::None) {
        return;
    }
    if let Ok(t) = texts.get(root)
        && !t.0.trim().is_empty()
    {
        out.push(t.0.clone());
    }
    if let Ok(kids) = children.get(root) {
        for kid in kids {
            collect_visible(*kid, children, texts, nodes, out);
        }
    }
}

/// **Take the problem block down** — `Esc`, the census's one key for "not that".
///
/// **The Map tab is deliberately absent from this match.** `Action::Cancel` peels one layer per
/// press there — a piece in hand, an armed tool, an armed piece — and the block is the outermost of
/// them, so `editor::keys` clears it itself and returns rather than letting one press do two things.
/// Clearing it here as well would take the block down *and* put the held piece back on the same
/// keystroke, which is exactly the promise that comment makes and this would break.
fn dismiss(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mode: Res<Mode>,
    mut import: ResMut<crate::tiles::ImportState>,
    mut bench: ResMut<crate::anim_tab::BenchState>,
    mut compose: ResMut<crate::compose::ComposeState>,
) {
    if !keys::just_pressed(&keyboard, *live, Action::Cancel) {
        return;
    }
    match *mode {
        Mode::Map => {}
        Mode::Meshes | Mode::Tiles => import.status.dismiss(),
        Mode::Anim => bench.status.dismiss(),
        Mode::Compose => compose.status.dismiss(),
    }
}

/// **`Cmd+C`: this tab's text into the clipboard, and the block comes down with it.**
///
/// Dismissing on success is the point rather than a convenience: copying a refusal is the strongest
/// evidence there is that somebody has read it, and a block still shouting after you have pasted it
/// into a message is a block you learn to stop looking at.
///
/// **Only on success.** A clipboard that could not be reached leaves the block exactly where it was
/// and raises a second problem over it — the one case where the author has *not* got the text out,
/// and the worst possible moment to take it off the screen.
fn copy_out(
    // Pins the system to the main thread. macOS's pasteboard is not promised to tolerate any
    // other, and a clipboard that works four times in five is worse than none — inherited from the
    // Tiles-only verb this replaces, where it was learned.
    _main_thread: bevy::ecs::system::NonSendMarker,
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mode: Res<Mode>,
    mut editor: ResMut<crate::editor::EditorState>,
    mut import: ResMut<crate::tiles::ImportState>,
    mut bench: ResMut<crate::anim_tab::BenchState>,
    mut compose: ResMut<crate::compose::ComposeState>,
    panes: Query<(Entity, &CopyPane)>,
    children: Query<&Children>,
    texts: Query<&Text>,
    nodes: Query<&Node>,
    // **The frame first, then every other root.** `Frame::root` is the window's own column and
    // therefore reads top-to-bottom; the overlays (the shortcut card, the name box, a guide step)
    // are roots of their own and belong after it rather than interleaved. `Option` because
    // `Screen::Editor` can run a pass before the door is built, and a missing `Res` panics in 0.19.
    frame: Option<Res<crate::chrome::Frame>>,
    other_roots: Query<Entity, (With<Node>, Without<ChildOf>)>,
) {
    if !keys::just_pressed(&keyboard, *live, Action::CopyInfo) {
        return;
    }
    let tab = *mode;
    let mut roots: Vec<Entity> = Vec::new();
    if let Some(frame) = frame.as_ref() {
        roots.push(frame.root);
    }
    // Sorted, so two runs of this key on an unchanged screen produce the same text — entity
    // iteration order is not stable across `App`s and a copy that reshuffled would be a poor thing
    // to paste into a bug report twice.
    let mut rest: Vec<Entity> = other_roots
        .iter()
        .filter(|e| Some(*e) != frame.as_ref().map(|f| f.root))
        .collect();
    rest.sort();
    roots.extend(rest);
    // One borrow of the live tab's status, used to read and then to answer. `match` on the mode
    // rather than a helper returning `&mut Status`, because four `ResMut` params cannot be handed
    // to one function and returned from it without borrowing all four for the rest of the system.
    let status: &mut crate::chrome::Status = match tab {
        Mode::Map => &mut editor.status,
        Mode::Meshes | Mode::Tiles => &mut import.status,
        Mode::Anim => &mut bench.status,
        Mode::Compose => &mut compose.status,
    };
    let lines = harvest(status, tab, &panes, &roots, &children, &texts, &nodes);
    if lines.is_empty() {
        status.note("nothing on this tab to copy");
        return;
    }
    let count = lines.len();
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(lines.join("\n"))) {
        Ok(()) => {
            status.dismiss();
            status.note(format!("copied {count} line(s) — the whole {} screen", tab.label()));
        }
        Err(e) => status.problem(format!("could not reach the clipboard: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use crate::chrome::Status;

    /// The rule `copy_out` turns on, stated where it can be checked without an `App`: a copy that
    /// worked takes the block down, and a copy that failed must not.
    #[test]
    fn only_a_copy_that_worked_takes_the_block_down() {
        let mut ok = Status::default();
        ok.problem("NOT SAVED: read-only file system");
        ok.dismiss();
        ok.note("copied 9 line(s) from the map tab");
        assert!(!ok.has_problem());

        let mut failed = Status::default();
        failed.problem("NOT SAVED: read-only file system");
        failed.problem("could not reach the clipboard: no display");
        assert!(
            failed.has_problem(),
            "a failed copy is the one moment the text must stay on screen"
        );
    }
}
