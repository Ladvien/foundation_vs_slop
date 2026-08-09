//! **The editor's shared furniture** — one palette, one panel, one key list.
//!
//! Every tab is the same two shapes: a controls panel down one side, and a list down the other. They
//! were written twice and were already drifting. The census key-row block appeared in `editor.rs` and
//! `tiles.rs` byte-for-byte, *including its five-line comment about `min_width`*, and rendered in two
//! different pairs of colours — the same list, a shade darker in one tab, which nobody chose. The
//! colour table itself was declared twice, with two of its entries carrying the same value under two
//! names (`TEXT_DIM`/`DIM`, `ROW_ARMED`/`ROW_SELECTED`), so a change to one tab's greys silently was
//! not a change to the other's.
//!
//! That is the failure `docs/ui.md` §3.5 records for key bindings, in a second place: a fact stated
//! more than once drifts. Ousterhout's *A Philosophy of Software Design* ch. 17 is the general form —
//! *"once you have learned how something is done in one place, you can use that knowledge to
//! immediately understand other places that use the same approach."* Here that is literal: a third tab
//! costs ~30 lines of panel code instead of ~110, and it cannot come out looking like a different
//! program.
//!
//! Nothing here knows what a map or a tile is. It knows what a panel looks like.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::ScrollArea;

use crate::keys::{self, Context};

// ── the palette ──────────────────────────────────────────────────────────────────────────────────

/// The panel's own ground. Opaque, not the game's translucent HUD panel: an editor panel is a work
/// surface, and a researcher in a white coat behind a translucent one is unreadable — measured.
pub const PANEL_BG: Color = Color::srgb(0.058, 0.054, 0.047);
/// A row at rest.
pub const ROW_BG: Color = Color::srgb(0.098, 0.092, 0.082);
/// A row that is armed, selected, or otherwise the one being acted on. **One name.** This was
/// `ROW_ARMED` in the map tab and `ROW_SELECTED` in the tiles tab, at the same value — two names for
/// one idea is two things to keep in step.
pub const ROW_SELECTED: Color = Color::srgb(0.30, 0.28, 0.24);
/// Body text: a value, an id, the thing that changes.
pub const TEXT: Color = Color::srgb(0.86, 0.84, 0.80);
/// Quieter body text. Was `TEXT_DIM` in one tab and `DIM` in the other, same value.
pub const DIM: Color = Color::srgb(0.58, 0.56, 0.53);
/// The heading and the live-edit colour.
pub const ACCENT: Color = Color::srgb(0.90, 0.66, 0.24);
/// The key column. Brighter than the description beside it, because the key is what you scan for.
pub const KEY: Color = Color::srgb(0.74, 0.71, 0.66);
/// A label column — quieter than its value, which is the thing that changes.
pub const LABEL: Color = Color::srgb(0.46, 0.44, 0.42);
/// A refusal, a blocking finding, an expensive number.
pub const DANGER: Color = Color::srgb(0.86, 0.36, 0.30);

/// **The problem banner's fill.** Deeper and more saturated than [`DANGER`], which is a text colour —
/// red text at [`DANGER`] on [`PANEL_BG`] is legible and quiet, and quiet is the failure being fixed
/// here. A filled block is read before it is parsed, which a line of coloured prose is not.
pub const PROBLEM_BG: Color = Color::srgb(0.52, 0.13, 0.10);
/// What is written on [`PROBLEM_BG`]. Warm rather than pure white, so it belongs to the same palette
/// as everything else in the panel.
pub const PROBLEM_TEXT: Color = Color::srgb(1.0, 0.93, 0.90);

/// **Machine-proposed, human-unconfirmed** — the VLM labeler's third state. A cool slate,
/// deliberately neither [`ACCENT`] (amber = a live edit, yours) nor [`DANGER`] (red = wrong):
/// a proposal is a question, and it must not read as either an answer or an alarm.
pub const SUGGEST: Color = Color::srgb(0.42, 0.58, 0.66);
/// Empty preview tile, so an un-baked row reads as "not yet" rather than as a hole in the panel.
/// `thumbs.rs` carries a third copy of this value as `BACKDROP`, for the booth's own background.
pub const SLOT_BG: Color = Color::srgb(0.14, 0.135, 0.125);
/// A group heading — quieter than a row, because it is a signpost rather than a thing to click on
/// most of the time.
pub const HEADER_BG: Color = Color::srgb(0.075, 0.070, 0.063);

// ── layout ───────────────────────────────────────────────────────────────────────────────────────

/// Where the panels start, below the tab strip. One number, so no two panels can disagree about it
/// and leave a tab half-covered.
pub const TAB_STRIP_BOTTOM: f32 = 46.0;
/// The gap every panel keeps from the window edge.
pub const MARGIN: f32 = 12.0;
/// Inside a panel.
pub const PAD: f32 = 12.0;

/// Width of a controls panel — two aligned text columns, so it is set by the widest chord plus its
/// label.
pub const CONTROLS_W: f32 = 300.0;
/// The tiles tab's controls panel is wider, and earns it: below the key list it carries four rows of
/// tag chips and a findings paragraph, neither of which is a two-column table. At `CONTROLS_W` the
/// chips wrapped to twice the height and pushed the findings off the bottom of the screen.
pub const TILES_CONTROLS_W: f32 = 380.0;
/// Width of a list panel. Narrower than the controls: a row here is a thumbnail, an id and a number
/// rather than two text columns. Both panels together leave a little under half the screen for the
/// map at `UiScale(1.2)`, which is the thing the right edge was chosen to protect.
pub const LIST_W: f32 = 264.0;

// ── spacing ──────────────────────────────────────────────────────────────────────────────────────
//
// **A scale, used as a set.** van den Berg, Cornelissen & Roerdink 2009 (`10.1167/9.4.24`) find that
// clutter is *crowding*, not element count: what makes a group read as a group is that its members
// sit closer to each other than to anything else. So what matters here is the RATIO between these,
// not their absolute values — a panel where every gap is 4 px has no groups, however few things are
// in it, and that is what the tiles detail block was.
//
// `docs/ui.md` §1.2 supplies the other half, and it cuts against reflexive minimalism: the test is
// Vicente & Rasmussen's *"does this force interpretation?"*, not "how many things are on screen",
// and Yang et al. 2017 measured more information **improving** performance. So the answer to a
// crowded panel is spacing and grouping, not deletion.

/// Between items that belong to each other — chips in one axis, cells in one row.
pub const GAP_TIGHT: f32 = 3.0;
/// Between the rows of one block.
pub const GAP_ROW: f32 = 5.0;
/// Between blocks. Several times [`GAP_TIGHT`], which is the whole point.
pub const GAP_GROUP: f32 = 16.0;

/// A block heading: quiet, and separated from what came before it.
///
/// The separation is the work — a heading with the same gap above it as below is a label that could
/// belong to either side.
pub fn section(parent: &mut ChildSpawnerCommands, label: &str) {
    parent.spawn((
        Text::new(label.to_owned()),
        TextColor(LABEL),
        TextFont::from_font_size(9.0),
        Node {
            margin: UiRect::top(Val::Px(GAP_GROUP)).with_bottom(Val::Px(GAP_TIGHT)),
            ..default()
        },
    ));
}

/// Which edge a panel is pinned to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// **A panel.** Absolutely positioned, below the tab strip, opaque, above the world.
///
/// `full_height` pins `bottom` as well as `top`, which is what gives a list inside it a real height to
/// scroll within: a `max_height` inside an unpinned panel is never reached and does nothing — the map
/// palette shipped that way and rendered two rows.
///
/// **Every panel carries `Hovered`.** `view::drive` and `place_on_click` both ask "is the pointer over
/// UI" by looking for any true `Hovered`, and when only the *rows* carried one the gaps between them
/// counted as open map — a wheel turn over a list zoomed the world, and a click that missed a row by a
/// pixel dropped a piece behind the panel. `Hovered` is true for an entity **or any descendant**
/// (`bevy_picking-0.19.0/src/hover.rs:322`), so one on the root answers for the whole surface.
pub fn panel_root<'a>(
    commands: &'a mut Commands,
    side: Side,
    width: f32,
    full_height: bool,
    hidden: bool,
) -> EntityCommands<'a> {
    let mut node = Node {
        position_type: PositionType::Absolute,
        top: Val::Px(TAB_STRIP_BOTTOM),
        width: Val::Px(width),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(6.0),
        padding: UiRect::all(Val::Px(PAD)),
        ..default()
    };
    match side {
        Side::Left => node.left = Val::Px(MARGIN),
        Side::Right => node.right = Val::Px(MARGIN),
    }
    if full_height {
        node.bottom = Val::Px(MARGIN);
    }
    if hidden {
        // `Display::None`, never `Visibility`: a visibility-hidden UI node still occupies layout and
        // still answers hover, which would leave a hidden tab's rows eating clicks aimed at the world.
        node.display = Display::None;
    }
    commands.spawn((
        node,
        BackgroundColor(PANEL_BG),
        GlobalZIndex(100),
        Hovered::default(),
    ))
}

/// A panel's heading.
pub fn title(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        Text::new(text.to_owned()),
        TextColor(ACCENT),
        TextFont::from_font_size(15.0),
    ));
}

// ── what a tab has to say ────────────────────────────────────────────────────────────────────────

/// **A tab's voice: the running commentary, and the thing that went wrong.**
///
/// # The renderer was guessing at severity, and each tab guessed differently
///
/// Every tab used to hold one `status: String` written from ~215 places, and decided at *render* time
/// whether it was bad news. The Map tab sniffed `status.starts_with("NOT SAVED")`; the Anim tab
/// coloured by whether `rigs.ron` had loaded, which is a fact about a file rather than about the
/// sentence; the Compose tab painted everything [`ACCENT`]; the Tiles tab had no colour rule at all,
/// so a refusal was byte-identical to a receipt. `stamp refused:`, `is not a number of metres` and
/// `NOT WRITTEN:` all rendered in the same grey as `stamped 4 piece(s)`.
///
/// **Severity is known at the write site and nowhere else.** So the write site states it, and no
/// renderer is allowed an opinion — the same move `crate::keys` makes for bindings and this module
/// makes for panels, for the third time and the same reason: a fact stated in two places drifts.
///
/// # Two slots, because one cannot both stick and stay current
///
/// A problem **survives every note**. That is the whole point — a refusal used to vanish the moment
/// the cursor moved, because half of these messages fire on ordinary hovering. But a single slot that
/// refused to be overwritten would swallow the receipts too: an author who fixes the problem and saves
/// would watch the old error sit there and never learn the save worked.
///
/// So a note and a problem are different things with different lifetimes, and the panel shows both.
/// `editor.rs`'s `Field::Edges` had already reached this conclusion for the one readout it owns —
/// *"a fault is a state the map is in — it does not happen once and stop being true, so putting it on
/// `Last` would let the next action erase it"*. This generalises that line to every tab.
///
/// # The banner and the log are one list seen twice
///
/// A problem does not replace the last one — it joins it. The banner shows the newest, because that
/// is what just happened; the log down the bottom shows the run, because *"what has gone wrong on
/// this tab"* is a different question from *"what went wrong just now"*, and the second one was
/// unanswerable: each new refusal erased the one before it, so a session that raised five could only
/// ever show the fifth.
///
/// One list means one clearing rule. [`Status::dismiss`] — `Esc` — takes down the banner **and** the
/// log, because they are two views of the same thing and two clearing rules for one datum is the
/// drift this module exists to prevent.
///
/// Not a `Resource`: each tab holds one inside the state it already owns, so there is nothing to
/// register and no question about which tab a bare `Res<Status>` would have meant.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Status {
    note: String,
    problems: Vec<Problem>,
    /// How many fell off the front of [`Self::problems`] at [`MAX_PROBLEMS`].
    ///
    /// Counted rather than forgotten. This crate's caps *"refuse and name rather than truncate"*,
    /// and a log cannot refuse — the newest problem is the one most worth having — so it names what
    /// it dropped instead. A log that silently forgot its first entries would be a log that reads
    /// complete and is not.
    dropped: usize,
}

/// One problem, and how many times it has been raised back to back.
///
/// **Consecutive repeats collapse.** A refusal fires per gesture, and gestures repeat: an author
/// clicking four times at a blocked cell would otherwise push four identical lines and bury
/// everything else on the tab. Only *consecutive* ones fold, so the order stays honest — the same
/// rule `keys::rows` uses to collapse adjacent bindings that share a description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub text: String,
    pub count: usize,
}

impl Problem {
    /// How it reads in the log: the text, and the tally only when there is one worth showing.
    pub fn line(&self) -> String {
        if self.count > 1 {
            format!("{}  (x{})", self.text, self.count)
        } else {
            self.text.clone()
        }
    }
}

/// How many problems a tab keeps. Past this the oldest is dropped and counted — see
/// [`Status::dropped`].
///
/// Sized to a panel rather than to memory: this is a list somebody reads down, and past a dozen or
/// so a list stops being read and starts being scrolled, which is the same argument
/// `keys::no_context_carries_more_than_a_learnable_vocabulary` makes one panel over.
pub const MAX_PROBLEMS: usize = 12;

impl Status {
    /// **What just happened, and went as asked.** Replaces the last note; never touches the problem.
    ///
    /// This is the right call for a refusal the author is not owed an alarm about — clicking bare
    /// ground with the remove tool armed, dragging a box round nothing. Those are answers, not
    /// failures, and a red block for each would teach an author to stop reading red blocks.
    pub fn note(&mut self, text: impl Into<String>) {
        self.note = text.into();
    }

    /// **The editor could not do what was asked, or did only part of it.**
    ///
    /// The line, in practice: a write that did not reach disk, a refusal handed back by `emerge-core`,
    /// input that would not parse, a piece that changed underneath the author, or an edit that
    /// succeeded and then failed to redraw. Anything a person would want to still be on screen a
    /// minute later.
    pub fn problem(&mut self, text: impl Into<String>) {
        let text = text.into();
        if let Some(last) = self.problems.last_mut() {
            if last.text == text {
                last.count += 1;
                return;
            }
        }
        self.problems.push(Problem { text, count: 1 });
        if self.problems.len() > MAX_PROBLEMS {
            self.problems.remove(0);
            self.dropped += 1;
        }
    }

    /// **A call that either worked or refused**, routed by which it did.
    ///
    /// For the helpers that return `Result<String, String>` — `Ok` is the receipt, `Err` is the
    /// refusal. Written once here so no call site can talk itself into stringifying an error into a
    /// note, which is exactly how `NOT WRITTEN:` ended up rendering grey.
    pub fn say(&mut self, outcome: Result<String, String>) {
        match outcome {
            Ok(receipt) => self.note(receipt),
            Err(refusal) => self.problem(refusal),
        }
    }

    /// **Take the notices down** — the banner and the log together, because they are one list.
    ///
    /// `Esc`'s last layer, and the only thing that clears a problem without another replacing it.
    pub fn dismiss(&mut self) {
        self.problems.clear();
        self.dropped = 0;
    }

    pub fn note_text(&self) -> &str {
        &self.note
    }

    /// **The newest problem** — what the banner shows. Empty when there is none.
    pub fn problem_text(&self) -> &str {
        self.problems.last().map_or("", |p| p.text.as_str())
    }

    /// The whole run, oldest first — what the log shows and what a copy carries.
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// How many fell off the front at [`MAX_PROBLEMS`].
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn has_problem(&self) -> bool {
        !self.problems.is_empty()
    }

    /// **The one thing to say, when only one thing fits** — the problem if there is one, else the
    /// note.
    ///
    /// For a log line and for the sentinel driver, whose whole job is to make a refusal visible in a
    /// captured frame. Printing the note there would report `stamped 4 piece(s)` from an earlier verb
    /// while the verb being logged had just refused.
    pub fn line(&self) -> &str {
        if self.has_problem() {
            self.problem_text()
        } else {
            &self.note
        }
    }

    /// Nothing to say at all — neither a receipt nor a refusal.
    pub fn is_empty(&self) -> bool {
        self.note.is_empty() && self.problems.is_empty()
    }
}

/// The banner one tab shows its newest problem in. Carries which tab, so a shared painter cannot
/// write another tab's block.
#[derive(Component, Clone, Copy)]
pub struct ProblemBanner(pub crate::tiles::Mode);

/// **The problem block: filled, not tinted, and directly under the title.**
///
/// Spawned once per panel and hidden until there is something to say — `Display::None` rather than a
/// zero-height node, so a quiet panel has no gap where the banner would be.
///
/// The glyph is `▲` and not `⚠`: the shipped face is `FiraMono-Regular.ttf`, which **has no U+26A0**
/// (measured), and a missing codepoint draws as a tofu box — the same class of trap `CLAUDE.md`
/// records for Bevy's own 95-codepoint default font, one font along.
pub fn problem_banner(parent: &mut ChildSpawnerCommands, tab: crate::tiles::Mode) {
    parent.spawn((
        Node {
            display: Display::None,
            padding: UiRect::axes(Val::Px(GAP_ROW + 1.0), Val::Px(GAP_ROW)),
            margin: UiRect::top(Val::Px(GAP_ROW)).with_bottom(Val::Px(GAP_TIGHT)),
            ..default()
        },
        BackgroundColor(PROBLEM_BG),
        Text::new(String::new()),
        TextColor(PROBLEM_TEXT),
        TextFont::from_font_size(11.0),
        ProblemBanner(tab),
    ));
}

/// A tab's error log, and which tab it belongs to.
#[derive(Component, Clone, Copy)]
pub struct ProblemLog(pub crate::tiles::Mode);

/// One line of it. Rebuilt wholesale, so it carries nothing — `compose::ComposeLine`'s argument.
#[derive(Component)]
pub struct ProblemLogLine;

/// **Everything that has gone wrong on this tab, at the bottom of its panel.**
///
/// The banner answers *"what just happened"*; this answers *"what has gone wrong here"*, which was
/// unanswerable — each refusal replaced the last, so a session that raised five could only show the
/// fifth. Bulleted and one line each, because it is a list to scan rather than prose to read.
///
/// `margin-top: auto` pushes it to the bottom of whatever panel holds it, which is the bottom-left
/// of the screen for the panels pinned `full_height` and the end of the panel for the one that is
/// not — without this needing to know which is which.
pub fn problem_log(parent: &mut ChildSpawnerCommands, tab: crate::tiles::Mode) {
    parent.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(GAP_TIGHT),
            margin: UiRect::top(Val::Auto),
            padding: UiRect::top(Val::Px(GAP_ROW)),
            display: Display::None,
            ..default()
        },
        ProblemLog(tab),
    ));
}

/// One bullet. `•` and not `-`: the shipped face has U+2022 (checked, as U+26A0 was not), and a
/// bullet reads as a list where a hyphen reads as a range.
pub fn problem_log_line(parent: &mut ChildSpawnerCommands, text: &str, colour: Color) {
    parent.spawn((
        Text::new(format!("• {text}")),
        TextColor(colour),
        TextFont::from_font_size(10.0),
        // A wrapped continuation restarts at column zero and breaks the bullet column — the same
        // thing that set the Compose panel's width. The full text is on the banner and in `Cmd+C`.
        TextLayout::new(Justify::Left, LineBreak::NoWrap),
        ProblemLogLine,
    ));
}

/// **The key list, read from the census and never retyped.**
///
/// `docs/ui.md` §3.5 records what happens otherwise: key allocation lived in five prose censuses and
/// all five drifted to the same wrong answer. A panel that types its own key list is a sixth. This
/// renders `keys::rows`, so a binding that changes changes here — in every tab at once.
///
/// Two aligned columns rather than a run-on line: a run-on wraps unpredictably at any width, and the
/// eye finds a row in a table without reading the others.
pub fn key_census(parent: &mut ChildSpawnerCommands, contexts: &[Context]) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(1.0),
            margin: UiRect::top(Val::Px(2.0)),
            ..default()
        })
        .with_children(|list| {
            for row_def in contexts.iter().flat_map(|c| keys::rows(*c)) {
                list.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    // A guaranteed gutter, so the widest chord still has air before its label.
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Node {
                            // **`min_width`, not `width`.** A fixed width does not clip or shrink its
                            // text — an over-long chord simply draws past the column and lands on top
                            // of the label beside it, which is exactly what "W, A, S, D" did to
                            // "pan". `min_width` keeps the column aligned for every row that fits and
                            // lets the one that does not push its label right instead of through it.
                            min_width: Val::Px(78.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(row_def.chord.clone()),
                        TextColor(KEY),
                        TextFont::from_font_size(11.0),
                        // No wrap: a chord with a space in it is one token to a reader and two to a
                        // line-breaker.
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    ));
                    row.spawn((
                        Text::new(row_def.does),
                        TextColor(DIM),
                        TextFont::from_font_size(11.0),
                    ));
                });
            }
        });
}

/// **A scrolling list that fills what is left of its panel.**
///
/// `flex_grow` with `min_height: 0`, not `max_height`: a flex item's automatic minimum size is its
/// content, which would grow the node to fit every row and leave `overflow` with nothing to clip. The
/// panel must be `full_height` for this to bound anything.
pub fn scroll_list(parent: &mut ChildSpawnerCommands, marker: impl Bundle) {
    parent.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        ScrollArea::default(),
        marker,
    ));
}

// A `LABEL  value` row builder belongs here too — the shape is repeated four times across the two
// tabs, differing only in the label column's width. It is not written yet because nothing has been
// moved onto it, and a builder with no caller is a stub.

/// **The name box** — the centred prompt for naming a new composition.
///
/// `M` on the Map keeps the set in hand as one composition, and this is where it is named. It used to
/// be a field in the Map's status readout, which put the question in the corner of the screen while
/// the whole screen waited for it.
///
/// Two tabs asked it when Compose could also make a composition. That is why this is a shared widget
/// painted by [`paint_name_box`] rather than a Map-local one: authoring collapsed onto the Map, and a
/// widget that survives losing one of its two callers is cheaper than one that has to be rebuilt if a
/// second ever returns.
#[derive(Component)]
pub struct NameBox;

#[derive(Component)]
struct NameBoxTitle;

#[derive(Component)]
struct NameBoxValue;

#[derive(Component)]
struct NameBoxHint;

fn spawn_name_box(mut commands: Commands) {
    commands
        .spawn((
            NameBox,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            // **Per `docs/ui.md` §5 trap 7.** A full-screen container without this eats every world
            // click. It also means a click while naming falls through to the stage, which is right:
            // the box is a prompt, not a modal that has to be dismissed before anything else works.
            bevy::picking::Pickable::IGNORE,
            // Above the panels and the tab strip (101), below nothing else — the same tier the
            // shortcuts overlay uses, because both are "the screen is asking you something".
            GlobalZIndex(400),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(PAD * 1.5)),
                    row_gap: Val::Px(GAP_ROW * 2.0),
                    min_width: Val::Px(360.0),
                    ..default()
                },
                BackgroundColor(PANEL_BG),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(String::new()),
                    TextFont::from_font_size(11.0),
                    TextColor(LABEL),
                    NameBoxTitle,
                ));
                b.spawn((
                    Text::new(String::new()),
                    TextFont::from_font_size(18.0),
                    TextColor(ACCENT),
                    NameBoxValue,
                ));
                b.spawn((
                    Text::new(String::new()),
                    TextFont::from_font_size(11.0),
                    TextColor(DIM),
                    NameBoxHint,
                ));
            });
        });
}

/// **Show the live tab's name field, or nothing.**
///
/// Reads the Map's own state rather than a shared projection, and formats the value the way the Map
/// commits it — snake_case as you type. Relocating the prompt must not quietly change what is saved.
fn paint_name_box(
    mode: Res<crate::tiles::Mode>,
    editor: Res<crate::editor::EditorState>,
    mut roots: Query<&mut Node, With<NameBox>>,
    mut titles: Query<&mut Text, (With<NameBoxTitle>, Without<NameBoxValue>, Without<NameBoxHint>)>,
    mut values: Query<&mut Text, (With<NameBoxValue>, Without<NameBoxTitle>, Without<NameBoxHint>)>,
    mut hints: Query<&mut Text, (With<NameBoxHint>, Without<NameBoxTitle>, Without<NameBoxValue>)>,
) {
    // **The Map's field, and only the Map's.** Compose used to ask this too; authoring moved to
    // the Map, so there is one asker and the match is a guard rather than a choice. Kept as a match
    // on `Mode` because that is what makes "which tab is asking" answerable in one place if a second
    // tab ever asks again.
    let asking: Option<(&str, String, String)> = match *mode {
        crate::tiles::Mode::Map => editor.grouping.as_ref().map(|raw| {
            (
                "NAME THIS COMPOSITION",
                // Forced to snake_case as it is typed, so the naming rule teaches itself.
                format!("{}_", emerge_core::naming::to_snake_case(raw)),
                "Enter keeps it.   Esc leaves the set in hand.".to_owned(),
            )
        }),
        _ => None,
    };
    let display = if asking.is_some() { Display::Flex } else { Display::None };
    for mut node in &mut roots {
        if node.display != display {
            node.display = display;
        }
    }
    let Some((title, value, hint)) = asking else {
        return;
    };
    // Guarded against the no-op write, like `editor::refresh_status`: this runs every frame and
    // `Text` is change-detected, so writing an unchanged string would re-lay the box continuously.
    // Three separate loops rather than one over a tuple: the queries have different filters, so they
    // are different types and cannot share an array.
    fn set(text: &mut Text, want: &str) {
        if text.0 != want {
            text.0 = want.to_owned();
        }
    }
    for mut t in titles.iter_mut() {
        set(&mut t, title);
    }
    for mut t in values.iter_mut() {
        set(&mut t, &value);
    }
    for mut t in hints.iter_mut() {
        set(&mut t, &hint);
    }
}

/// The held-key overlay's root. `Display::None` when it is not being asked for.
#[derive(Component)]
pub struct ShortcutsOverlay;

/// Where the rows are rebuilt, so the frame around them is spawned once.
#[derive(Component)]
struct ShortcutsBody;

/// Which tab's list the overlay is currently showing, so it is rebuilt on a tab change and not on
/// every frame the key is held.
#[derive(Resource, Default)]
struct ShowingFor(Option<crate::tiles::Mode>);

/// **The key list, on demand.**
///
/// It used to be printed down the side of every panel: eighteen rows in the Tiles tab, above the
/// controls, which is what pushed the subgrid's own cell grid below the fold (`FVS-Q-11`). A key
/// list is *reference*, consulted rarely and never while acting — and `docs/ui.md` §1.2's test is
/// "does this force interpretation?", which a permanent eighteen-row table beside the thing you are
/// trying to use does.
///
/// So it is held rather than printed, and the panels carry one line saying so. That line is not
/// optional: Cockburn, Gutwin, Scarr & Malacria 2014 (`10.1145/2659796`, already cited by
/// `crate::keys`) document the intermodal-transition failure — a fast path offered *beside* a slow
/// one does not get adopted on its own. A hidden list nobody is told about is that failure exactly.
pub struct ChromePlugin;

impl Plugin for ChromePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowingFor>()
            .add_systems(Startup, (spawn_shortcuts_overlay, spawn_name_box))
            .add_systems(Update, drive_shortcuts_overlay.in_set(keys::Phase::Act))
            // **After `Phase::Text`, not in it.** The field consumes the keystroke there; painting
            // before it would show the box one character behind what has been typed.
            .add_systems(Update, paint_name_box.after(keys::Phase::Text));
    }
}

fn spawn_shortcuts_overlay(mut commands: Commands) {
    commands
        .spawn((
            ShortcutsOverlay,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            // **Per `docs/ui.md` §5 trap 7.** A full-screen container without this eats every world
            // click; its children keep their own hit targets, which this one does not need anyway.
            bevy::picking::Pickable::IGNORE,
            // Above the panels and the tab strip (101), which is the whole point of an overlay.
            GlobalZIndex(400),
            // A scrim, so the list reads against whatever is behind it without hiding the map.
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(PAD * 1.5)),
                    row_gap: Val::Px(GAP_ROW),
                    ..default()
                },
                BackgroundColor(PANEL_BG),
                ShortcutsBody,
            ));
        });
}

/// Show the list while the key is down, and rebuild it when the tab under it changes.
fn drive_shortcuts_overlay(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mode: Res<crate::tiles::Mode>,
    mut showing: ResMut<ShowingFor>,
    mut roots: Query<&mut Node, With<ShortcutsOverlay>>,
    bodies: Query<Entity, With<ShortcutsBody>>,
) {
    let want = keys::pressed(&keyboard, live.0, keys::Action::Shortcuts);
    for mut node in &mut roots {
        let display = if want { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    // Rebuilt on the transition and on a tab change, never per frame: the rows are static text and
    // respawning them sixty times a second would be sixty times the work for one picture.
    let key = want.then_some(*mode);
    if showing.0 == key {
        return;
    }
    showing.0 = key;
    let Some(tab) = key else { return };
    for body in &bodies {
        commands.entity(body).despawn_related::<Children>();
        commands.entity(body).with_children(|p| {
            title(p, &format!("{} KEYS", tab.label()));
            // This tab first, then the frame around it — the same order the panels used, so anyone
            // who learned the old list finds the rows where they were.
            key_census(p, &[tab.context(), Context::Global]);
        });
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    /// **The whole point.** A refusal survives every receipt that follows it — which is what the
    /// single `status: String` could not do, because half these messages fire on ordinary hovering
    /// and the next one along overwrote the last.
    #[test]
    fn a_problem_outlives_every_note() {
        let mut s = Status::default();
        s.problem("NOT SAVED: read-only file system");
        for receipt in ["placed crate@7", "removal mode off", "saved", "filled 12"] {
            s.note(receipt);
            assert_eq!(
                s.problem_text(),
                "NOT SAVED: read-only file system",
                "`{receipt}` erased the refusal"
            );
        }
        assert_eq!(s.note_text(), "filled 12", "the receipt line stopped being current");
    }

    /// The other half, and the reason there are two slots rather than one that refuses to be
    /// overwritten: an author who fixes the problem and saves has to be able to see the save work.
    #[test]
    fn a_note_is_still_shown_while_a_problem_stands() {
        let mut s = Status::default();
        s.problem("cannot record `break_table`: no such member");
        s.note("`break_table` armed");
        assert_eq!(s.note_text(), "`break_table` armed");
        assert!(s.has_problem());
    }

    /// **A problem joins the last one; it does not replace it.** The banner shows the newest, the
    /// log shows the run — which is the question that was unanswerable when a new refusal erased
    /// the one before it.
    #[test]
    fn problems_accumulate_and_the_newest_is_the_banner() {
        let mut s = Status::default();
        s.problem("first");
        s.problem("second");
        assert_eq!(s.problem_text(), "second", "the banner shows the newest");
        assert_eq!(s.problems().len(), 2, "and the log keeps both");
        // One list, one clearing rule.
        s.dismiss();
        assert!(!s.has_problem());
        assert!(s.problems().is_empty());
        assert!(s.is_empty(), "dismissing left something behind");
    }

    /// **Consecutive repeats fold.** A refusal fires per gesture and gestures repeat; four identical
    /// lines would bury everything else on the tab.
    #[test]
    fn a_repeated_problem_folds_into_a_count() {
        let mut s = Status::default();
        for _ in 0..4 {
            s.problem("blocked: `floor@3` already covers that spot");
        }
        assert_eq!(s.problems().len(), 1);
        assert_eq!(s.problems()[0].count, 4);
        assert!(s.problems()[0].line().ends_with("(x4)"));

        // Only CONSECUTIVE ones, so the order stays honest.
        s.problem("NOT SAVED: read-only file system");
        s.problem("blocked: `floor@3` already covers that spot");
        assert_eq!(s.problems().len(), 3, "a repeat after something else is a new entry");
    }

    /// **The cap names what it dropped.** This crate's caps refuse and name rather than truncate; a
    /// log cannot refuse the newest entry, so it counts the ones it let go instead.
    #[test]
    fn the_log_caps_and_says_how_many_it_dropped() {
        let mut s = Status::default();
        for i in 0..MAX_PROBLEMS + 5 {
            s.problem(format!("problem {i}"));
        }
        assert_eq!(s.problems().len(), MAX_PROBLEMS);
        assert_eq!(s.dropped(), 5);
        assert_eq!(
            s.problem_text(),
            format!("problem {}", MAX_PROBLEMS + 4),
            "the newest must survive the cap — it is the one worth having"
        );
        s.dismiss();
        assert_eq!(s.dropped(), 0, "dismissing clears the tally with the list");
    }

    /// `say` is the one place a `Result` becomes a message, so no call site can stringify an error
    /// into the quiet slot — the bug that had `NOT WRITTEN:` rendering as a receipt at nine
    /// `tiles::persist` call sites.
    #[test]
    fn say_routes_by_the_result_and_not_by_the_wording() {
        let mut s = Status::default();
        s.say(Ok("recorded 3 member(s)".to_owned()));
        assert_eq!(s.note_text(), "recorded 3 member(s)");
        assert!(!s.has_problem());

        // Deliberately worded like a receipt: routing must not depend on how it reads.
        s.say(Err("everything went fine, honestly".to_owned()));
        assert_eq!(s.problem_text(), "everything went fine, honestly");
        assert_eq!(s.note_text(), "recorded 3 member(s)", "an Err overwrote the receipt");
    }

    /// One line for a log or a captured frame, and it must be the bad news when there is any.
    #[test]
    fn the_one_line_form_prefers_the_problem() {
        let mut s = Status::default();
        s.note("stamped 4 piece(s)");
        assert_eq!(s.line(), "stamped 4 piece(s)");
        s.problem("stamp refused: nothing offers a surface");
        assert_eq!(s.line(), "stamp refused: nothing offers a surface");
    }
}

/// **One line where eighteen rows used to be.** See [`ChromePlugin`] for why it is not optional.
pub fn shortcut_hint(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Text::new(format!(
            "Hold {} for shortcuts",
            keys::chord(keys::Action::Shortcuts)
        )),
        TextColor(LABEL),
        TextFont::from_font_size(10.0),
        Node {
            margin: UiRect::top(Val::Px(2.0)),
            ..default()
        },
    ));
}
