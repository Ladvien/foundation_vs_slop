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
/// **A list**, because the Meshes and Tiles tabs share one detail pane: tagging it `Meshes` alone made
/// `Cmd+C` on Tiles harvest the status lines and none of the tile — no id, no envelope, no member
/// list — in an editor where `bevy_ui` offers no other way to get that text out of the window. It is
/// the only marker left with a tab list; `chrome::ProblemBanner` had one and it decided nothing, so
/// it is a bare marker now.
#[derive(Component, Clone, Copy)]
pub struct CopyPane(pub &'static [Mode]);

/// What the notice panels currently show, so they are rebuilt on a change and not on a frame.
///
/// `badges::ShowingFor` does the same for the key badges, for the same reason: *"static text
/// respawned sixty times a second would be sixty times the work for one picture."* It matters more
/// here — `EditorState` is written at drag rate, so a system gated on `is_changed()` would rebuild
/// the log while the cursor moves.
#[derive(Resource, Default, PartialEq, Eq)]
struct Showing {
    tab: Option<Mode>,
}

/// **How long the toast stays up, and how long it takes to go.**
///
/// Seven seconds is a reading budget rather than a round number: a refusal here is a sentence naming
/// a descriptor or a composition — *"cannot remove: `lab/bench` still places it"* — and the messages
/// run to about twenty words, which is four or five seconds of reading before the eye has to find it
/// on screen at all.
///
/// Nothing is lost when it goes: the session [`crate::chrome::Journal`] keeps every refusal behind
/// `Cmd+E`, out of `Esc`'s reach. See [`crate::chrome::problem_toast`] for why fading is what makes
/// the toast honest rather than what makes it lossy.
const TOAST_SECS: f32 = 7.0;
/// The last stretch, spent going out. Long enough to read as leaving rather than as a glitch.
const TOAST_FADE: f32 = 0.6;

/// **What the toast is showing and how much of its life is left.**
///
/// `shown` is the line, so the same problem does not re-toast every frame — and a *repeated* one
/// does, because `chrome::Problem::line` folds consecutive repeats into `(x2)` and that is a
/// different string. Pressing a refused key again therefore says so again, which is the whole point
/// of feedback on a gesture.
#[derive(Resource, Default)]
struct Toast {
    shown: Option<String>,
    /// Seconds of life left. **Not `left`**: `tests/no_system_writes_every_frame.rs` matches
    /// `.left =` by name to catch `Node::left`, and a resource field sharing the name is charged as
    /// a layout write — which is half of why this module carried the crate's first (and only)
    /// `WRITES-EVERY-FRAME-OK:` marker.
    secs_left: f32,
}

pub struct NoticePlugin;

impl Plugin for NoticePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Showing>()
            .init_resource::<Toast>()
            .init_resource::<crate::chrome::Journal>()
            .add_systems(
                OnEnter(crate::screen::Screen::Editor),
                crate::chrome::journal_panel.after(crate::chrome::FrameSystems),
            )
            .add_systems(
                Update,
                ((
                    (dismiss, copy_out, toggle_journal).in_set(keys::Phase::Act),
                    // **Before the paint**, so a refusal raised this frame is in the journal before
                    // anything reads it — and outside `Phase::Act`, because it watches state rather
                    // than keys.
                    record_problems.after(keys::Phase::Act),
                    paint_journal.after(record_problems),
                    // After the verbs, so a problem raised this frame is on screen this frame.
                    paint_notices.after(keys::Phase::Act),
                    // And after the paint, so a toast raised this frame is up for its whole life
                    // rather than one frame short of it.
                    (tick_toast, paint_toast).chain().after(paint_notices),
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
    mode: Res<Mode>,
    editor: Res<crate::editor::EditorState>,
    import: Res<crate::tiles::ImportState>,
    bench: Res<crate::anim_tab::BenchState>,
    compose: Res<crate::compose::ComposeState>,
    mut showing: ResMut<Showing>,
    mut toast: ResMut<Toast>,
    // **No `Node` here any more.** Whether the toast is on screen is its clock's answer, not this
    // system's — see [`fade_toast`]. This writes what it says.
    mut banners: Query<&mut Text, With<crate::chrome::ProblemBanner>>,
    journals: Query<&Node, With<crate::chrome::JournalPanel>>,
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

    // **The newest problem is what the toast says**, and a change to it is what starts its clock.
    // A tab change starts it too: arriving on a tab that already has a refusal should say so, and
    // `showing.tab` is still the tab we are leaving at this point in the frame.
    let newest = status.problems().last().map(|p| p.line());
    if toast.shown != newest || showing.tab != Some(tab) {
        toast.secs_left = if newest.is_some() { TOAST_SECS } else { 0.0 };
        toast.shown = newest.clone();
    }
    // **The toast stands down for as long as the journal is up.** `toggle_journal` zeroes it at
    // the moment of opening; without this, a refusal raised while the panel was open re-armed it —
    // seven seconds of the same sentence drawn over the journal's own title, the exact overlap the
    // stand-down exists to prevent. `shown` still tracks above, so closing the panel does not
    // resurrect a toast that was already read as the journal's first line.
    if toast.secs_left > 0.0 && journals.iter().any(|n| n.display != Display::None) {
        toast.secs_left = 0.0;
    }
    // **Remembered here, and this is load-bearing.** The re-arm above compares against it, so a
    // `Showing` nothing ever wrote would make every frame look like a tab change and the toast would
    // never go down. It was the log's dedupe key before the log moved behind `Cmd+E`; it is the
    // toast's now, which is why the resource survives with one field.
    if showing.tab != Some(tab) {
        showing.tab = Some(tab);
    }
    // The card is a `Text` write guarded on its own content, so it is cheap every frame. There is
    // one card and it speaks for whatever tab is live — the per-tab filter that used to stand here
    // was reading `ProblemBanner(ALL_TABS)`, which is every tab, so it decided nothing.
    for mut text in &mut banners {
        // The glyph is `▲` and not `⚠`: `FiraMono-Regular.ttf` has no U+26A0 (measured), and a
        // missing codepoint draws as a tofu box.
        //
        // **Cleared when there is nothing to say**, and that is not cosmetic: the card is hidden by
        // its layer's `Display` (see [`paint_toast`]) rather than by being emptied, so a stale line
        // sat in it for the whole life of the process — and `copy_out` harvests the text of every
        // node the walk reaches, so the moment the toast came back up for an unrelated refusal the
        // author's `Cmd+C` carried the wrong sentence.
        let want = match newest.as_deref() {
            Some(newest) => format!("▲  {newest}"),
            None => String::new(),
        };
        if text.0 != want {
            text.0 = want;
        }
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
    mut journal: Query<&mut Node, With<crate::chrome::JournalPanel>>,
) {
    if !keys::just_pressed(&keyboard, *live, Action::Cancel) {
        return;
    }
    // **The innermost thing first.** `Esc` with the journal open puts the journal away and leaves
    // the tab's problems alone — clearing them under a panel the author is reading would empty the
    // list in front of them.
    let mut closed = false;
    for mut node in &mut journal {
        if node.display != Display::None {
            node.display = Display::None;
            closed = true;
        }
    }
    if closed {
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

/// **The toast's clock.** One line, and nothing that lays out.
///
/// Separate from [`paint_toast`] on purpose: a system that both ticks a timer and writes `Node` is
/// one whose layout writes cannot be read as guarded — `a_drawing_system_writes_only_when_something_changed`
/// counts writes against comparisons and cannot tell a resource field from a UI one. Adding a
/// no-op compare to satisfy it would be the Goodhart move that test's own siblings warn about; two
/// systems, each with one job, is the true shape.
fn tick_toast(time: Res<Time>, mut toast: ResMut<Toast>) {
    if toast.secs_left > 0.0 {
        toast.secs_left = (toast.secs_left - time.delta_secs()).max(0.0);
    }
}

/// **What the toast looks like**: up for [`TOAST_SECS`], out over [`TOAST_FADE`], then gone.
///
/// The `Display` lives on the strip rather than the card, so hiding costs one compare and the card's
/// own colours are only touched while it is actually going out.
fn paint_toast(
    toast: Res<Toast>,
    mut layers: Query<&mut Node, With<crate::chrome::ToastLayer>>,
    mut cards: Query<
        (&mut BackgroundColor, &mut TextColor),
        With<crate::chrome::ProblemBanner>,
    >,
) {
    let up = toast.secs_left > 0.0;
    for mut node in &mut layers {
        let want = if up { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    if !up {
        return;
    }
    // Full opacity until the last stretch, then out.
    let alpha = (toast.secs_left / TOAST_FADE).clamp(0.0, 1.0);
    for (mut bg, mut ink) in &mut cards {
        let want_bg = crate::chrome::PROBLEM_BG.with_alpha(alpha);
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want_ink = crate::chrome::PROBLEM_TEXT.with_alpha(alpha);
        if ink.0 != want_ink {
            ink.0 = want_ink;
        }
    }
}

/// **Everything that has gone wrong, kept where `Esc` cannot reach it.**
///
/// Watches [`crate::chrome::Status::raised`] — a counter that only goes up — so the journal learns
/// about a refusal without either side holding the other's list. See [`crate::chrome::Journal`] for
/// why a status cannot simply hand it over.
///
/// All four tabs, because a session log that only recorded the tab you happened to be on would be a
/// log with holes exactly where a batch left them.
fn record_problems(
    editor: Res<crate::editor::EditorState>,
    import: Res<crate::tiles::ImportState>,
    bench: Res<crate::anim_tab::BenchState>,
    compose: Res<crate::compose::ComposeState>,
    mut journal: ResMut<crate::chrome::Journal>,
    // One watermark per tab, in the order below. A `Local` rather than a field on the journal: it is
    // this system's bookkeeping about what it has read, not a fact about the log.
    mut seen: Local<[u64; 4]>,
) {
    let statuses = [
        &editor.status,
        &import.status,
        &bench.status,
        &compose.status,
    ];
    for (i, status) in statuses.into_iter().enumerate() {
        let now = status.raised();
        if now <= seen[i] {
            continue;
        }
        // **Every refusal since the last look, each with its own text.** `raised` counts calls to
        // `Status::problem` — folds included — so it says how far back to look and nothing more; the
        // TEXT comes from the entries themselves. Recording `problem_text()` that many times was one
        // sentence standing in for several: `labels::check_stale` raises a distinct refusal per stale
        // result inside one loop, and the journal kept only the last of them, N times over.
        let problems = status.problems();
        // Newest first, spending the budget. The boundary entry may already be partly recorded — a
        // repeat folds into it across frames — so it contributes only what is left rather than its
        // whole count.
        let mut budget = (now - seen[i]) as usize;
        let mut back = 0usize;
        let mut oldest_times = 0usize;
        for p in problems.iter().rev() {
            if budget == 0 {
                break;
            }
            oldest_times = p.count.min(budget);
            budget -= oldest_times;
            back += 1;
        }
        // Oldest of the tail first, so the journal reads in the order the refusals happened.
        for (n, p) in problems[problems.len() - back..].iter().enumerate() {
            let times = if n == 0 { oldest_times } else { p.count };
            journal.record(&p.text, times);
        }
        // **The watermark moves by what was recorded, not by what was raised.** It moved first, and
        // to `now`, so a raise whose text was already gone by the time this looked — `Esc` or a
        // successful `Cmd+C` clears the tab in the same `Phase::Act` this runs after — was written
        // off rather than still owed. A clock advanced past events it never read is a clock that
        // silently forgets, which is `Status::dropped`'s argument one scope up.
        //
        // The tradeoff, stated: holding the watermark back attributes an unrecordable raise to the
        // *next* refusal on that tab, inflating its `(xN)`. Advancing fully loses it outright. The
        // reachable window is only raises created *and* cleared inside one `Act`, and in both real
        // cases (`copy_out`, `editor::keys`) the refusal being cleared was raised on an earlier frame
        // and is already recorded, so `budget` is zero.
        seen[i] = now - budget as u64;
    }
}

/// **`Cmd+E` opens the journal, and pressing it again puts it away.**
///
/// A toggle rather than a modal: it is a reference panel, not a question, so it takes no scrim and
/// blocks nothing. `Esc` closes it too — see [`dismiss`], which now backs out of the innermost thing
/// rather than always clearing the tab.
fn toggle_journal(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mut toast: ResMut<Toast>,
    mut panels: Query<&mut Node, With<crate::chrome::JournalPanel>>,
) {
    // **Behind a key EDGE, and the lint can see that now.** `just_pressed` is false on every frame
    // but the one where `Cmd+E` went down, so the one write below happens once per press and the
    // change flag it raises is exactly the news the layout wants. This carried the crate's only
    // `WRITES-EVERY-FRAME-OK:` marker, for two false positives rather than a real exemption:
    // `count_writes` matched `.display =` inside `node.display ==` and charged the comparison as a
    // write, and it matched `.left =` on `Toast::left`, an `f32` on a resource that is not
    // `Node::left`. Both are fixed at the source — the lint skips comparison lines, and the field is
    // `secs_left`.
    if !keys::just_pressed(&keyboard, *live, Action::ShowErrors) {
        return;
    }
    for mut node in &mut panels {
        let opening = node.display == Display::None;
        node.display = if opening { Display::Flex } else { Display::None };
        // **The toast stands down when the journal comes up.** Measured in a frame: both are
        // centred at the top of the viewport, so the toast sat squarely over the panel's own title.
        // It is also the same sentence twice — the journal's first line IS what the toast is
        // showing — which is the duplication this whole area was rebuilt to end. `shown` is left
        // alone, so putting the journal away does not raise it again.
        if opening {
            toast.secs_left = 0.0;
        }
    }
}

/// **Rebuild the list when the journal changes or the panel opens**, and not on a frame.
///
/// `chrome::Follow`'s argument: static text respawned sixty times a second is sixty times the work
/// for one picture. The guard is what it would produce, exactly as the old per-panel log's was.
fn paint_journal(
    mut commands: Commands,
    journal: Res<crate::chrome::Journal>,
    panels: Query<&Node, With<crate::chrome::JournalPanel>>,
    lists: Query<Entity, With<crate::chrome::JournalList>>,
    lines: Query<Entity, With<crate::chrome::ProblemLogLine>>,
    mut was_open: Local<bool>,
) {
    let open = panels.iter().any(|n| n.display != Display::None);
    if !open {
        // Nothing to draw and nothing to keep: the next open rebuilds from the journal, which is the
        // only copy that matters.
        *was_open = false;
        return;
    }
    // **Rebuilt when the journal changed or the panel just opened — never on a count.** This
    // guarded on the line count once, and a count is exactly what the journal keeps constant: a
    // repeat folds into the last entry's `(xN)` without adding a line, and at [`crate::chrome::JOURNAL_CAP`]
    // every new entry replaces an old one — so an open panel showed stale tallies and, at the cap,
    // froze for the rest of the session. `Journal` is only dereferenced mutably when a refusal was
    // recorded, so its change flag is the honest clock — and cheaper than rendering 200 lines a
    // frame to compare them.
    let rebuild = journal.is_changed() || !*was_open;
    *was_open = true;
    if !rebuild {
        return;
    }
    let want: Vec<String> = journal
        .entries()
        .iter()
        .rev()
        .map(|p| p.line())
        .chain(match journal.dropped() {
            0 => None,
            n => Some(format!(
                "+{n} earlier, dropped at {}",
                crate::chrome::JOURNAL_CAP
            )),
        })
        .collect();
    for e in &lines {
        commands.entity(e).despawn();
    }
    for list in &lists {
        commands.entity(list).with_children(|p| {
            if want.is_empty() {
                p.spawn((
                    bevy::prelude::Text::new("nothing has gone wrong yet".to_owned()),
                    bevy::prelude::TextColor(crate::chrome::DIM),
                    bevy::prelude::TextFont::from_font_size(crate::chrome::text::LABEL),
                    crate::chrome::ProblemLogLine,
                ));
                return;
            }
            for (i, line) in want.iter().enumerate() {
                // Newest first, and the newest is the one the toast just showed — so the top of this
                // list and the thing that interrupted you agree.
                let colour = if i == 0 { crate::chrome::DANGER } else { crate::chrome::DIM };
                crate::chrome::problem_log_line(p, line, colour);
            }
        });
    }
}
