//! **The editor's shared furniture** — one palette, one panel, one row shape.
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

use crate::keys::{self};

// ── the palette ──────────────────────────────────────────────────────────────────────────────────

/// The panel's own ground. Opaque, not the game's translucent HUD panel: an editor panel is a work
/// surface, and a researcher in a white coat behind a translucent one is unreadable — measured.
pub const PANEL_BG: Color = Color::srgb(0.058, 0.054, 0.047);
/// A row at rest.
pub const ROW_BG: Color = Color::srgb(0.098, 0.092, 0.082);
/// A row (or chip, or field) under the pointer and not yet chosen. **One name.** This value lived
/// as an unnamed literal in the map palette and the tab strip — byte-identical, twice — which is
/// this module's founding failure one more time. One step above [`ROW_BG`], well short of
/// [`ROW_SELECTED`]: hover is a signifier that the thing is actionable (Norman's term, via
/// Seinfeld et al. 2020, `10.1080/07370024.2020.1724790` §3.1), not a claim that it is chosen.
pub const ROW_HOVER: Color = Color::srgb(0.16, 0.15, 0.14);
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

/// **Present but switched off** — dimmer than [`LABEL`], which is already the quietest thing that
/// still asks to be read.
///
/// For a row that exists on purpose and is deliberately not participating: a pack this kit excludes.
/// **The scrim behind a modal** — the application, dimmed, so the question is the only lit thing.
///
/// Black at 55%: enough to push a full panel of text back without hiding what the question is
/// about, which matters because every question here names a piece or a map the author can still
/// see behind it. Named rather than written at the call site so a second modal cannot dim by a
/// different amount, which is exactly the drift `panel_ink_comes_from_the_palette` exists to catch
/// — and did, on this constant's first day.
pub const SCRIM: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

/// **The interface, stood back, while the key badges are up.**
///
/// Lighter than [`SCRIM`], and the difference is what each is for: a scrim dims for a modal you have
/// to answer, so hiding what is behind it is the point. This dims for a layer you read *through* —
/// the badge is only useful next to the control it names, and a veil that hid the control would hide
/// the thing being taught.
///
/// It is not a courtesy. Healey & Enns 2012 (`10.1109/TVCG.2011.127`) measured the feature hierarchy
/// and the interference is **asymmetric**: background variation in colour masks patterns of form,
/// while variation in form does not mask colour. Flattening the ground is therefore what makes a
/// badge a *pop-out* — detectable "regardless of the number of distractors" — rather than a
/// conjunction search at 25–40 ms an item. Named here rather than written at the call site so a
/// second overlay cannot dim by a different amount, which is what `panel_ink_comes_from_the_palette`
/// exists to catch.
pub const VEIL: Color = Color::srgba(0.0, 0.0, 0.0, 0.35);

/// **How far a ground fill steps back while the shortcuts key is held.**
///
/// A panel's fill is the ground its rows stand on ([`Ground`]), and while `K` is down that ground is
/// also what a badge may stand on. At full opacity the badge and the panel are two opaque boxes of
/// the same colour meeting at a border; at this alpha the [`VEIL`] and the world read faintly
/// through the ground, so the badge is plainly the nearest thing and the panel is plainly behind it.
///
/// **Only the fill.** Rows, fields, chips and every word keep their own opacity — a badge points at
/// words, and dimming those in the instant they are being read is the opposite of the point.
pub const GROUND_HELD: f32 = 0.55;

/// **How far the world's own wireframe steps back while the shortcut key is held.**
///
/// Deeper than [`GROUND_HELD`], because a panel's fill is a flat wash a badge sits on and the
/// world's envelope is a *line* crossing the same ground a badge stands on. On the Tiles tab a
/// 1 x 4 x 1 m tile projects to three near-full-height verticals that run straight through the
/// badge stack, and at full strength they read as leaders — which is the one thing on this screen a
/// hairline is supposed to mean. Reported from the keyboard: *"the tiles tab is super muddy."*
///
/// It is a fade and not a hide: the envelope is what the author is building, and a box that
/// vanished when they reached for a key would be a different confusion. `badges::WorldOnScreen`
/// still keeps boxes off it either way — this is about reading, not about placement.
pub const WORLD_HELD: f32 = 0.25;

/// A world gizmo's colour, faded while the shortcut key is held. See [`WORLD_HELD`].
///
/// A free function rather than a resource because the drawers are ordinary `Update` systems with no
/// stated order against anything in `badges`, and a resource written in one phase and read in
/// another would be a frame stale on some runs and not others — the trap `sense_world_ink`'s own
/// header records paying for.
pub fn stepped_back(color: Color, held: bool) -> Color {
    if held {
        color.with_alpha(color.alpha() * WORLD_HELD)
    } else {
        color
    }
}

/// It has to stay visible, because a mesh that has silently vanished looks identical to one that was
/// never scanned — but it must not compete with rows an author can actually act on.
pub const MUTED: Color = Color::srgb(0.34, 0.32, 0.31);

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
/// **A value a model wrote that nobody has checked** — [`SUGGEST`]'s sibling, one state later.
///
/// [`SUGGEST`] is a proposal *pending*: it sits on [`ROW_BG`], where a mid slate reads fine. This is
/// the same idea after `U` — the token is now held, so it sits on [`ROW_SELECTED`], and the same
/// slate against that lighter fill measures about **2.6:1**, under any threshold `docs/ui.md` §1.3
/// will accept for text. Scaled toward white rather than hand-picked, so retinting `SUGGEST` retints
/// this with it — the audit's `ENVELOPE_IDLE` finding, which was `ACCENT` divided by two and
/// transcribed.
///
/// The pair is the whole visible half of `emerge_core::descriptor::LabelOrigin`: this ink means *a
/// machine decided this and you have not*, and [`TEXT`] on the same chip means *you did*.
pub const UNCHECKED: Color = scaled(SUGGEST, 1.45);
/// **A mesh that has been judged** — every axis answered, so it can compose a tile.
///
/// Asked for at the keyboard, 2026-08-15: *"could we add a visual indicator based on color to show
/// whether a mesh has been labeled."* Green because it is the one state that means *ready*, and it
/// has to be told apart at a glance from [`SUGGEST`] — a machine's proposal waiting on a human is a
/// question, and a judged mesh is an answer.
pub const LABELED: Color = Color::srgb(0.50, 0.76, 0.46);

/// Empty preview tile, so an un-baked row reads as "not yet" rather than as a hole in the panel.
/// `thumbs.rs` carries a third copy of this value as `BACKDROP`, for the booth's own background.
pub const SLOT_BG: Color = Color::srgb(0.14, 0.135, 0.125);
/// **A text box that owns the keyboard right now.** The same value as [`SLOT_BG`], and its own name
/// anyway: "this field is focused" and "this thumbnail is not baked yet" are different sentences,
/// and a shared constant would weld them — retinting one would retint the other, which nobody
/// would have chosen (the 2026-08-17 audit's `SLOT_BG`-moonlighting finding).
pub const FOCUS_BG: Color = Color::srgb(0.14, 0.135, 0.125);
/// A group heading — quieter than a row, because it is a signpost rather than a thing to click on
/// most of the time.
pub const HEADER_BG: Color = Color::srgb(0.075, 0.070, 0.063);

// ── world ink ────────────────────────────────────────────────────────────────────────────────────
//
// Gizmo colours the tabs draw INTO the scene — the exception to "nothing here knows what a map is",
// carried because each is a fact two tabs state: a colour defined per-tab drifted into two names
// for one value twice before this block existed (the audit's `BOUNDS_LINE == CELLS` finding).

/// The working grid — the map's cell lattice on the Map tab, the seating lattice on Compose.
///
/// Derived, not chosen: `bevy_dev_tools`' grid drew `srgb(0.2, 0.2, 0.2)` against a `ClearColor`
/// of `srgb(0.035, …)`, a separation of about 0.165. The Map's ground slab raised what the lines
/// are read against to 0.105, so holding that separation puts them here. Warm-neutral rather than
/// pure grey, because every other colour in this editor is.
pub const GRID_LINE: Color = Color::srgb(0.270, 0.262, 0.248);
/// A stated extent, brighter than the grid inside it: the map's bounds wireframe on the Map tab,
/// the subgrid cell lattice on Tiles. Dim enough not to compete with [`GRID_LINE`], bright enough
/// to find. One warm grey — previously two names at one value, one per tab.
pub const BOUNDS_LINE: Color = Color::srgb(0.42, 0.38, 0.30);
/// The void the panels float over — the clear colour both entry points install. Here because it
/// was stated twice, byte-for-byte, in `main.rs` and `harness.rs`, and a fact stated more than
/// once drifts. Darker than [`PANEL_BG`], so a panel reads as a surface laid on nothing.
pub const VOID: Color = Color::srgb(0.035, 0.033, 0.030);

/// **The window's own chrome** — the bar above the door strip and the band at its foot.
///
/// Reported at the keyboard: the two bars read as bands, but only just. They were on [`HEADER_BG`],
/// which is a *group heading inside a panel* — a word for separating blocks on one surface, not for
/// saying "this strip is not part of any panel". Two roles on one colour, which is the drift
/// `chrome.rs` exists to stop, so window chrome gets its own word.
///
/// Derived from [`SLOT_BG`] rather than picked, because the relationship is the point: chrome sits a
/// step above panel ground and a step below an inspector slot, so a back button on `SLOT_BG` still
/// reads as raised *against* the bar it sits on.
pub const BAR_BG: Color = scaled(SLOT_BG, 0.78);

/// **The hairline that ends a bar.** Luminance, not hue — `docs/ui.md` §1.3 is explicit that
/// separation is on luminance alone, and a chrome edge is the last thing that should spend saturation.
pub const BAR_EDGE: Color = scaled(ROW_SELECTED, 0.62);

/// A chrome colour scaled toward black, for a quieter sibling of a named colour — so the derived
/// value cannot drift from its parent the way `compose`'s hand-halved ACCENT could have.
pub const fn scaled(colour: Color, k: f32) -> Color {
    match colour {
        Color::Srgba(c) => Color::srgb(c.red * k, c.green * k, c.blue * k),
        // Every chrome colour is authored as sRGB; anything else reaching here is a caller error,
        // answered loudly in magenta rather than with a silent guess.
        _ => Color::srgb(1.0, 0.0, 1.0),
    }
}

/// A chrome colour as raster bytes, for the CPU-drawn plots — one palette, two encodings, no
/// hand-transcribed mirror (the audit found three byte copies of chrome colours in `anim_plots`).
pub const fn ink(colour: Color) -> [u8; 4] {
    match colour {
        Color::Srgba(c) => [
            (c.red * 255.0 + 0.5) as u8,
            (c.green * 255.0 + 0.5) as u8,
            (c.blue * 255.0 + 0.5) as u8,
            255,
        ],
        _ => [255, 0, 255, 255],
    }
}

// ── layout ───────────────────────────────────────────────────────────────────────────────────────

/// Where the panels start, below the tab strip. One number, so no two panels can disagree about it
/// and leave a tab half-covered.
/// **The editor's interface scale**, and the one `main` starts the application at.
///
/// Named here rather than written as `1.2` in two places, because there is now a second screen that
/// legitimately runs at a different one: the menu multiplies `UiScale` by the display's scale factor
/// so its offscreen capture rasterises sharp (`chooser::fit_capture_to_window`), and hands this back
/// on the way out. Two owners of one value need one name for it.
pub const EDITOR_UI_SCALE: f32 = 1.2;

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

// ── the widget layer ─────────────────────────────────────────────────────────────────────────────

/// **`bevy_feathers`' machinery, with this editor's palette in it.**
///
/// Separate from [`ChromePlugin`] because `add_plugins` tuples cap at **15**
/// (`bevy_app-0.19.0/src/plugin.rs:186`) and `harness::add_editor_plugins` is near it — a plugin
/// that nests its own group keeps the shared list one list.
///
/// **`UiTheme` is inserted, never defaulted.** `UiTheme::default()` is empty and every token miss
/// renders fuchsia with a warning; a theme nobody seeded is a magenta editor, and no test would see
/// it because nothing here reads a colour back. [`theme`] is the seed and
/// `the_theme_is_seeded_from_the_palette` is what notices if it stops being.
pub struct WidgetsPlugin;

impl Plugin for WidgetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy::feathers::FeathersPlugins)
            .insert_resource(bevy::feathers::theme::UiTheme(theme()));
    }
}

// ── theme ────────────────────────────────────────────────────────────────────────────────────────

/// **This editor's palette, expressed as a `bevy_feathers` theme.**
///
/// # Machinery, not greys — and that reconciliation is written down here because nowhere else has it
///
/// `docs/ui.md` §5 says of `bevy_feathers`: *"its visuals are Bevy's editor skin — do not adopt
/// them"*, and *"Do not add a UI framework crate"*. Both stand. What is adopted is the **mechanism**
/// — a token table, a themed scrollbar, and focus outlines that would otherwise be hand-written
/// badly — and every colour this editor states comes from the constants above.
///
/// **What is NOT true, and was written here as if it were:** Feathers' greys are not overwritten
/// wholesale. `create_dark_theme()` populates 137 tokens and the table below replaces 14, so 123 of
/// Bevy's editor greys are live in the resource. Nothing in this crate spawns a Feathers-themed
/// widget today, so none of them reaches the screen — but that is a fact about the call sites, not
/// about this function, and the first themed widget anybody adds will render in somebody else's
/// skin. Seeding from `ThemeProps::default()` instead is what would make the paragraph above true;
/// it is a decision about what a missing token should look like, not a typo, so it is named here
/// rather than taken.
///
/// The editor already deviates from that section once, deliberately and on the record (the full font
/// face, where the game keeps Bevy's 95-codepoint default). This is the second, and the same shape:
/// take the machinery, keep the house's own decisions.
///
/// # Why this is not a fifth dialect
///
/// The 2026-08-17 audit's verdict is that four tabs were drifting into four dialects, and its remedy
/// is *one name per fact*. A token table that **restated** a colour — `srgb(0.098, 0.092, 0.082)`
/// beside `ROW_BG` — would be exactly the leak it measured, one indirection later. So every entry
/// below is a reference to a const, never a literal, and `tests/chrome_census.rs` fails on a literal
/// in this file's theme.
///
/// `UiTheme::color` renders **fuchsia** and warns once for a token it has no entry for — the house
/// convention, which [`scaled`] and [`ink`] also keep (*"loudly in magenta rather than with a silent
/// guess"*). **That alarm cannot currently fire here**, and the difference matters: seeding from
/// `create_dark_theme()` gives every token an entry, so `color`'s `None` arm is unreachable and a
/// token this table misses answers with Bevy's grey rather than shouting. The safety property this
/// paragraph used to claim is the one thing the seeding gives up.
pub fn theme() -> bevy::feathers::theme::ThemeProps {
    use bevy::feathers::tokens;
    let mut props = bevy::feathers::dark_theme::create_dark_theme();
    props.color.extend([
        (tokens::WINDOW_BG, VOID),
        (tokens::PANE_BODY_BG, PANEL_BG),
        (tokens::PANE_HEADER_BG, HEADER_BG),
        (tokens::SUBPANE_BODY_BG, SLOT_BG),
        (tokens::SUBPANE_HEADER_BG, HEADER_BG),
        (tokens::TEXT_MAIN, TEXT),
        (tokens::TEXT_DIM, DIM),
        // The focus ring is the one thing Feathers gives that this editor has never had. `ACCENT` at
        // half alpha, because a ring is a *state* and the audit's rule is that persistent signals sit
        // at medium contrast with the loud end held in reserve.
        (tokens::FOCUS_RING, ACCENT.with_alpha(0.5)),
        (tokens::SCROLLBAR_BG, HEADER_BG),
        (tokens::SCROLLBAR_THUMB, scaled(ROW_SELECTED, 0.8)),
        (tokens::SCROLLBAR_THUMB_HOVER, ROW_SELECTED),
        (tokens::LISTROW_BG, ROW_BG),
        (tokens::LISTROW_BG_HOVER, ROW_HOVER),
        (tokens::LISTROW_BG_SELECTED, ROW_SELECTED),
    ]);
    props
}

// ── type ─────────────────────────────────────────────────────────────────────────────────────────

/// **The type scale, as roles rather than numbers.**
///
/// The palette got named constants and the spacing got a scale; **size never got a name**, and the
/// 2026-08-17 audit measured what that cost: six sizes across a hundred call sites with no rule
/// between them. Section headings were 9 (`chrome::section`), 10 (the editor's and anim's
/// hand-rolled ones) or 11 (Compose's three) — *three dialects for one role in one editor*. Compose
/// showed two "COMPOSITIONS" headings, the real `section` plus an 11 px twin. Label/value pairs were
/// 10/11, 10/10 or flat 11 depending on the tab. And the anim bench rendered its central pairing
/// **inverted** — declared at 10 over measured at 9 — so the number the author is checking was the
/// smaller of the two.
///
/// A role is not the same thing as a distinct size, and two roles sharing a value is not a defect:
/// [`HEADING`] and [`LABEL`] are both 10 because both are quiet supporting text, and what separates
/// a heading from a label is that it is uppercase, in a different ink, and preceded by
/// [`GAP_GROUP`]. `section`'s own doc says so — *"quiet, and separated from what came before it…
/// the separation is the work"* — which is why the fix for three heading dialects is the smallest
/// of them and not the largest. A scale where headings out-shouted values would be a different
/// editor, and nobody asked for that one.
///
/// **What actually moved**, so a capture diff can be read rather than guessed at: `section` 9 → 10,
/// Compose's three hand-made headings 11 → 10, the anim pair un-inverted, and the name box's 18 →
/// [`TITLE`]. Everything else keeps the size it had and gains a name.
pub mod text {
    /// The panel's own name — `EMERGE MAPPER`, `MESHES AND TILES`. One per panel.
    pub const TITLE: f32 = 15.0;
    /// A tab's word in the strip, and the chooser's column headers.
    pub const TAB: f32 = 13.0;
    /// **The readable default.** A row's value, a field's contents, the problem banner, a census
    /// row — anything the author is reading rather than orienting by. The chord on a badge is
    /// this size on purpose: it is the one thing a badge exists to have read.
    pub const BODY: f32 = 11.0;
    /// A block or list heading. Quiet: uppercase, dim ink, and [`super::GAP_GROUP`] above it.
    pub const HEADING: f32 = 10.0;
    /// The dim half of a label/value pair, and a log line.
    pub const LABEL: f32 = 10.0;
    /// Badge descriptions, footnotes, and the always-on hint — the smallest thing on screen, and
    /// never the thing being read. (Badge *chords* moved up to [`BODY`]; the description beside a
    /// chord stays down here, quieter on both axes.)
    pub const HINT: f32 = 9.0;
}

/// **One glyph's advance at [`text::BODY`], logical pixels.** The shipped face is
/// `FiraMono-Regular.ttf` — monospace — so `chars * BODY_CHAR_W` is exact, not an estimate, and a
/// string can be measured without a text-layout round trip. Stated once, beside the scale it
/// belongs to (and outside it — this is a metric of the face, not a type role, and the census that
/// keeps the scale short counts the module): it was measured independently in `badges` (chord
/// columns) and `compose` (centred slot labels), and two hand-copied 6.6s drift the day the face or
/// the size changes. If the face ever goes proportional, layouts drift; they do not break.
pub const BODY_CHAR_W: f32 = 6.6;

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

/// **A chip's (and a compact list row's) inset.** The 6 × 3 pair was an undeclared constant at
/// twelve sites in five files before it had this name — the exact drift this module's header
/// describes. The vertical 3 is [`GAP_TIGHT`] on purpose: a chip's own padding and the gap to its
/// neighbour read as one rhythm. Do not shrink it — the padding is most of a chip's click target
/// (Fitts: selection time grows as the target narrows), and 1 px of vertical padding made a row of
/// chips "a solid bar of text rather than a row of things".
pub const CHIP_PAD: UiRect = UiRect::axes(Val::Px(6.0), Val::Px(GAP_TIGHT));
/// A text field's inner inset — tighter than [`CHIP_PAD`] because a field's box is already drawn
/// by its fill, and the caret needs the width more than the text needs air.
pub const FIELD_PAD: UiRect = UiRect::axes(Val::Px(4.0), Val::Px(2.0));
/// A text field's floor. An unstated height lays out at 7 px while empty — this fact was restated
/// at five sites as a bare `18.0` before it had a name.
pub const MIN_FIELD_H: f32 = 18.0;
/// A block heading: quiet, and separated from what came before it.
///
/// The separation is the work — a heading with the same gap above it as below is a label that could
/// belong to either side.
pub fn section<'a>(parent: &'a mut ChildSpawnerCommands, label: &str) -> EntityCommands<'a> {
    parent.spawn((
        Text::new(label.to_owned()),
        TextColor(LABEL),
        TextFont::from_font_size(text::HEADING),
        Node {
            margin: UiRect::top(Val::Px(GAP_GROUP)).with_bottom(Val::Px(GAP_TIGHT)),
            ..default()
        },
    ))
}

/// Which edge a panel is pinned to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// **A panel.** A child of one of the frame's docks, opaque, and as tall as the frame lets it be.
///
/// It used to be `PositionType::Absolute` at `top: TAB_STRIP_BOTTOM`, pinned to a window edge and
/// floating over the world — which is why nothing on screen filled the window and why a panel's
/// height was a number rather than a consequence. [`Frame`] owns position now; this owns width,
/// which side, and whether the panel wants the dock's whole height.
///
/// `full_height` becomes `flex_grow` rather than a pinned `bottom`, and the reason to keep the
/// distinction is unchanged: a list inside needs a real height to scroll within, and a `max_height`
/// inside a content-sized panel is never reached and does nothing — the map palette shipped that way
/// and rendered two rows. **`min_height: 0` comes with it**, because a flex item's automatic minimum
/// size is its content, so without it the panel grows to fit every row and `overflow` has nothing
/// left to clip.
///
/// **Every panel carries `Hovered`.** `view::drive` and `place_on_click` both ask "is the pointer
/// over UI" by looking for any true `Hovered`, and when only the *rows* carried one the gaps between
/// them counted as open map — a wheel turn over a list zoomed the world, and a click that missed a
/// row by a pixel dropped a piece behind the panel. `Hovered` is true for an entity **or any
/// descendant** (`bevy_picking-0.19.0/src/hover.rs:322`), so one on the root answers for the whole
/// surface. The frame above it deliberately carries none — see [`Frame`].
pub fn panel_root<'a>(
    commands: &'a mut Commands,
    frame: &Frame,
    side: Side,
    width: f32,
    full_height: bool,
    hidden: bool,
) -> EntityCommands<'a> {
    let mut node = Node {
        width: Val::Px(width),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(6.0),
        padding: UiRect::all(Val::Px(PAD)),
        margin: UiRect::all(Val::Px(MARGIN)),
        ..default()
    };
    if full_height {
        node.flex_grow = 1.0;
        node.min_height = Val::Px(0.0);
    }
    if hidden {
        // `Display::None`, never `Visibility`: a visibility-hidden UI node still occupies layout and
        // still answers hover, which would leave a hidden tab's rows eating clicks aimed at the
        // world — and in a dock it would also hold the dock open at its width.
        node.display = Display::None;
    }
    let dock = match side {
        Side::Left => frame.left,
        Side::Right => frame.right,
    };
    let panel = commands
        .spawn((node, BackgroundColor(PANEL_BG), Ground(PANEL_BG), Hovered::default()))
        .id();
    commands.entity(dock).add_child(panel);
    commands.entity(panel)
}

/// **The window's own layout, and the one thing on screen that belongs to no panel.**
///
/// # Why there is a frame at all
///
/// There were two layouts and neither filled the window. The editor drew `PositionType::Absolute`
/// panels of fixed pixel width, anchored to the left and right edges and floating over a 3-D camera
/// that owned the whole window; the menu was a fixed-pixel flex grid whose own code said it *"is
/// fixed-size and simply sits in whatever window there is"*. On a 2560x1406 window that left about
/// two fifths of the screen as ground nothing could use, panels sized for twenty rows holding two,
/// and rows wrapping into their own value column — reported as *"it's bad"*, and visible in the
/// capture that started this.
///
/// So position stops being something a panel decides. A panel says how wide it wants to be and which
/// side it lives on; **where it goes is the frame's answer**, and the frame is the window.
///
/// ```text
/// +--------------------------------------------------+
/// |  < kits & maps                 KIT . furniture   |  chrome bar
/// +--------------------------------------------------+
/// |  KIT | MAP | RIGS      1 MESHES 2 TILES 3 COMPOSE |  door strip
/// +----------+---------------------------+-----------+
/// | left     |                           | right     |
/// | dock     |        viewport           | dock      |  body: flex_grow 1
/// |          |                           |           |
/// +----------+---------------------------+-----------+
/// |  stamped 4 pieces        Hold K . Cmd+O back      |  status band
/// +--------------------------------------------------+
/// ```
///
/// # Flex, deliberately, and not `Val::Percent`
///
/// [`EDITOR_UI_SCALE`] multiplies every `Val::Px` and every font size and leaves `Val::Percent`
/// alone (`docs/ui.md` §5). A layout mixing percentage widths with pixel minimums would therefore
/// scale two ways at once on a 2x display, which is a bug that only appears on somebody else's
/// monitor. `flex_grow` on the centre and pixel widths on the docks is responsive *and*
/// density-correct, because the centre is defined as whatever is left.
///
/// # What the frame must not carry
///
/// **No `Hovered`, and `Pickable::IGNORE` throughout.** `view::over_ui` and `view::drive` both ask
/// "is the pointer on the interface" by looking for a true `Hovered`, and a frame node carrying one
/// would answer yes for the entire window — the map would stop taking clicks and the wheel would
/// stop zooming, everywhere, with nothing to point at. `Hovered` belongs on the panels, which is
/// where [`panel_root`] puts it.
#[derive(Resource)]
pub struct Frame {
    pub root: Entity,
    /// Above everything, and the same on every door — see `docs/2026-08-17-one-application.md` §4.1.
    pub chrome_bar: Entity,
    /// The door's own strip of panels.
    pub door_strip: Entity,
    /// Everything between the two bars. The editor fills its three slots; the **menu** puts its own
    /// columns straight in here, because a menu has no docks and no viewport — the frame is the
    /// window's shape, not the editor's.
    pub body: Entity,
    pub left: Entity,
    /// The hole the world is drawn through. Carries [`ViewportSlot`].
    pub viewport: Entity,
    pub right: Entity,
    /// One band at the foot of the window: what happened, what went wrong, what the keys are.
    pub status: Entity,
}

/// **The node the world shows through, and the only one with no background.**
///
/// `crate::surface` reads its rect and gives it to the map camera as a viewport, which is what makes
/// the viewport a region rather than the whole window with panels floating over it.
#[derive(Component)]
pub struct ViewportSlot;

/// Everything that spawns into a dock runs after this.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameSystems;

/// **The census's name for this control** — what makes a key badge land on the thing it acts through.
///
/// Attached by the panel that spawns the node, and joined by id in `crate::badges`. Symbolic on
/// purpose: `keys.rs` declares a [`keys::ControlId`] for a verb's home and never learns what a
/// palette *is*, and a badge system querying fourteen domain markers (`PaletteRow`, `TagChip`,
/// `CellButton`, …) would be the second census `keys.rs` exists to delete.
///
/// **One per node, and one visible node per id.** `crate::badges::anchor` resolves an id to the
/// unique node carrying it with a non-zero [`bevy::ui::ComputedNode`] — which is also the visibility
/// test, since [`panel_root`]'s hidden form is `Display::None` and lays out to nothing. Two visible
/// at once is a bug rather than a tie to break, and
/// `every_home_a_live_binding_names_is_on_screen` is what says so.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Control(pub keys::ControlId);

/// **This node's fill is ground, not ink — and this is the colour it rests at.**
///
/// A panel's [`PANEL_BG`] and a band's [`BAR_BG`] are what their rows and words *stand on*. They
/// carry no information of their own, so a badge parked on bare ground covers nothing a reader
/// needs — which is the whole difference between an overlay that has somewhere to go and one that
/// hugs a dock edge and lands on the map. `badges::ink` reads this to subtract a container from the
/// occupancy census while keeping every child in it.
///
/// **The colour is carried rather than recomputed**, the way `badges::BadgeRest` is: [`dim_the_ground`]
/// restores exactly what was spawned, so it is not a second place that decides what a panel is
/// coloured, and a panel that changes its fill cannot drift out of sync with the system that dims it.
#[derive(Component)]
pub struct Ground(pub Color);

/// Height of the two bars. Fixed, because a bar that changed height as its content changed would
/// move the viewport under the author's hands.
const BAR_H: f32 = 26.0;

/// **Build the frame and remember its slots.**
///
/// The `Frame` resource is how [`panel_root`] finds the dock to put a panel in; BSN-style name
/// lookup does not exist here and a marker query would need a flush between the frame and the first
/// panel. Ordering is [`FrameSystems`], which every panel spawner runs `after`.
pub fn spawn_frame(mut commands: Commands) {
    let bar = |extra: Node| -> Node {
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(BAR_H),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(GAP_GROUP),
            padding: UiRect::axes(Val::Px(MARGIN), Val::Px(0.0)),
            flex_shrink: 0.0,
            ..extra
        }
    };

    let chrome_bar = commands
        .spawn((
            bar(Node {
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            }),
            BackgroundColor(BAR_BG),
            Ground(BAR_BG),
            BorderColor::all(BAR_EDGE),
            bevy::picking::Pickable::IGNORE,
        ))
        .id();

    // The strip sets its own padding — a tab is a box, not a word on a bar — so this one only says
    // how tall the band is and where it starts.
    let door_strip = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                column_gap: Val::Px(2.0),
                padding: UiRect::left(Val::Px(MARGIN)),
                flex_shrink: 0.0,
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ))
        .id();

    // **A dock is a column with no width of its own.** Its width is its widest visible panel, which
    // is what lets four tabs' panels share one dock and each keep the width it argued for
    // (`CONTROLS_W` vs `TILES_CONTROLS_W`) without a dock-level table saying it twice.
    let dock = || {
        (
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                flex_shrink: 0.0,
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        )
    };
    let left = commands.spawn(dock()).id();
    let right = commands.spawn(dock()).id();

    let viewport = commands
        .spawn((
            ViewportSlot,
            Node {
                // **The centre is what is left**, which is the whole idea. `min_width: 0` because a
                // flex item's automatic minimum size is its content, and without it a wide panel
                // would push the viewport off the window instead of narrowing it.
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                ..default()
            },
            // No `BackgroundColor` at all: this is the hole the world shows through.
            bevy::picking::Pickable::IGNORE,
        ))
        .id();

    let body = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ))
        .id();
    commands
        .entity(body)
        .add_children(&[left, viewport, right]);

    let status = commands
        .spawn((
            bar(Node {
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            }),
            BackgroundColor(BAR_BG),
            Ground(BAR_BG),
            BorderColor::all(BAR_EDGE),
            bevy::picking::Pickable::IGNORE,
        ))
        .id();

    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            // See the type doc: no `Hovered`, and nothing here eats a click meant for the map.
            bevy::picking::Pickable::IGNORE,
        ))
        .id();
    commands
        .entity(root)
        .add_children(&[chrome_bar, door_strip, body, status]);

    commands.insert_resource(Frame {
        root,
        chrome_bar,
        door_strip,
        body,
        left,
        viewport,
        right,
        status,
    });
}

/// A panel's heading.
pub fn title(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        Text::new(text.to_owned()),
        TextColor(ACCENT),
        TextFont::from_font_size(text::TITLE),
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
    /// **Everything this status has ever been handed, repeats included** — and never reset, not
    /// even by [`Status::dismiss`].
    ///
    /// It is the [`Journal`]'s clock. The journal is session-wide and a `Status` is a tab's working
    /// list capped at [`MAX_PROBLEMS`] and cleared by `Esc`, so the journal cannot be derived by
    /// looking at what a status currently holds — by the time it looks, the evidence may be gone.
    /// A number that only goes up says *"there have been three more since you last looked"* without
    /// either side keeping a copy of the other's list.
    raised: u64,
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
        // Counted before the fold, because a repeat is a thing that happened — see [`Self::raised`].
        self.raised += 1;
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

    /// **Drop the running commentary**, leaving the problems alone.
    ///
    /// A note is a receipt for something that just happened *here*; a problem is a state the editor
    /// is in. So changing what "here" means ends a note's relevance and none of a problem's — which
    /// is why this is not [`Self::dismiss`].
    ///
    /// Reported at the keyboard, 2026-08-18, and the report was of a different bug entirely: a
    /// rotate receipt — `lamp_tall 270,270,180 deg` — sat on the panel across every tab switch,
    /// because `note` is one `String` that is only ever overwritten and Meshes and Tiles share one
    /// `ImportState`. The author read a message announcing a rotation, on a tab they had just
    /// arrived at, beside a piece that was lying on its side, and concluded the tab switch had
    /// turned it. Measured over BRP: the rotation quaternion is identical on both tabs and
    /// `library.ron` is not written. **The stale receipt was the whole of the bug.**
    pub fn clear_note(&mut self) {
        self.note.clear();
    }

    /// **Take the notices down** — the banner and the log together, because they are one list.
    ///
    /// `Esc`'s last layer, and the only thing that clears a problem without another replacing it.
    pub fn dismiss(&mut self) {
        self.problems.clear();
        self.dropped = 0;
    }

    /// See [`Self::raised`] — the journal's clock, not a count of what is currently held.
    pub fn raised(&self) -> u64 {
        self.raised
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

/// **Every refusal since the application started**, across tabs and doors.
///
/// # Why this is not four `Status`es
///
/// A [`Status`] is a tab's working list: capped at [`MAX_PROBLEMS`], cleared by `Esc`, and thrown
/// away with the door that owned it. That is right for *"what has gone wrong here"* and useless for
/// *"what has gone wrong at all"* — asked for at the keyboard: *"a hidden log that pops up if you
/// press the key… and that log shows every error message that's happened since the beginning of the
/// application."*
///
/// So this outlives both: `Ownership::Session`, which is the only class in `screen::OWNERSHIP` that
/// survives a door change, and exactly what a session log means.
///
/// # Written by watching, not by being told
///
/// `Status::problem` is a method on a plain struct four tabs own — it has no `Commands` and no way
/// to reach a resource, and giving it one would mean routing every refusal in the editor through an
/// event just so a log could exist. `notice::record_problems` watches [`Status::raised`] instead: a
/// counter that only goes up, so the journal can tell *"three more since I looked"* without either
/// side holding a copy of the other's list — which is what makes `Esc` clearing a tab harmless here.
#[derive(Resource, Default)]
pub struct Journal {
    entries: Vec<Problem>,
    dropped: usize,
}

/// How many the journal keeps. Ten times a panel's [`MAX_PROBLEMS`], because this one is read by
/// scrolling rather than at a glance — and capped at all for the reason that cap exists: a batch
/// that raises one refusal per frame is a real thing this editor does.
pub const JOURNAL_CAP: usize = 200;

impl Journal {
    /// **Record `times` occurrences of one refusal.** Consecutive repeats fold, exactly as they do
    /// in a [`Status`] — the same rule, so the journal reads like the toast that raised it.
    pub fn record(&mut self, text: &str, times: usize) {
        if times == 0 {
            return;
        }
        if let Some(last) = self.entries.last_mut()
            && last.text == text
        {
            last.count += times;
            return;
        }
        self.entries.push(Problem { text: text.to_owned(), count: times });
        if self.entries.len() > JOURNAL_CAP {
            self.entries.remove(0);
            self.dropped += 1;
        }
    }

    /// Oldest first, the order they happened in.
    pub fn entries(&self) -> &[Problem] {
        &self.entries
    }

    /// Named rather than silently forgotten — [`Status::dropped`]'s argument, one scope up.
    pub fn dropped(&self) -> usize {
        self.dropped
    }
}

/// The banner a panel shows its newest problem in. Carries **which tabs it speaks for**, so a shared
/// painter cannot write another tab's block.
///
/// # A list, because one panel can serve two tabs
///
/// The TILES panel is the whole reason: Meshes and Tiles share it. A banner is one line and there is
/// one per tab, so each of those names a single tab — but the **detail pane** and the **problem log**
/// in that panel are single nodes whose contents the live tab decides, and tagging them with a lone
/// `Mode::Meshes` silently made them belong to Meshes alone. Every refusal the Tiles tab's verbs
/// wrote landed in a log `paint_notices` then skipped, and `Cmd+C` harvested a pane it could not see.
///
/// A node saying which tabs it serves is the true thing to say; duplicating a shared node once per
/// tab would be two copies to keep in step. `tiles::TILES_PANEL_TABS` is the list.
#[derive(Component, Clone, Copy)]
pub struct ProblemBanner(pub &'static [crate::tiles::Mode]);

/// **The problem, as a toast over the viewport.**
///
/// # It was a block in the status band, and that is what went wrong
///
/// The band is twenty-six pixels of chrome. A padded, wrapping block of the longest text this editor
/// renders was a flow child of it, so it stood proud of the band it was supposed to be inside.
/// Reported from the keyboard: *"when there's an error, it appears over the status bar at the bottom
/// and isn't quite in alignment, but it's pretty close. So it looks really bad."* Close-but-not is
/// worse than either — a block that plainly floats reads as deliberate, and a block a few pixels out
/// reads as broken.
///
/// # Why a toast is not a loss of information
///
/// A problem in this editor is **sticky by design** — it is a state, not an event — and a toast that
/// fades would normally throw that away. It does not here, because the sticky half already exists
/// somewhere better: the session [`Journal`] behind `Cmd+E` keeps every refusal, across tabs, out
/// of `Esc`'s reach. The two answer different questions — *the toast answers "what just happened",
/// the journal answers "what has gone wrong at all"* — and fading is what makes the first of those
/// honest: an event that never leaves is not an event.
///
/// So the toast shows the newest problem for a few seconds and stands down, and nothing is lost:
/// `notice::record_problems` has already written the journal's copy before the toast ever fades.
///
/// # Where it goes
///
/// Centred at the top of the **viewport**, which is the one region that belongs to nothing else:
/// the docks own both sides, `compass` owns the bottom-left, and `badges` puts its legend in the
/// bottom-right. Asked for as *"a place that isn't going to occlude much"*.
///
/// The glyph is `▲` and not `⚠`: the shipped face is `FiraMono-Regular.ttf`, which **has no U+26A0**
/// (measured), and a missing codepoint draws as a tofu box.
pub fn problem_toast(mut commands: Commands, frame: Res<Frame>) {
    // The strip is full-width so the card can centre in it; the card is what is seen.
    let card = commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(GAP_GROUP), Val::Px(GAP_ROW + 1.0)),
                // Wraps rather than spanning the viewport: a refusal here names descriptors and
                // compositions, which is the longest text this editor renders.
                max_width: Val::Percent(72.0),
                // **A field of `Node` in 0.19, not a component** (`bevy_ui-0.19.0/src/ui_node.rs:738`)
                // — as a sibling in the bundle it is simply not a `Bundle` and the error says so
                // without naming which member is wrong.
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(PROBLEM_BG),
            Text::new(String::new()),
            TextColor(PROBLEM_TEXT),
            TextFont::from_font_size(text::BODY),
            TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
            bevy::picking::Pickable::IGNORE,
            ProblemBanner(ALL_TABS),
        ))
        .id();
    commands
        .spawn((
            Node {
                // PLACES-ITSELF-OK: a toast is not a panel. It is over the viewport rather than in
                // the layout, which is the whole point — `Frame` owns where panels go and this is
                // deliberately not one of them.
                position_type: PositionType::Absolute,
                top: Val::Px(GAP_GROUP),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            ToastLayer,
            bevy::picking::Pickable::IGNORE,
        ))
        .add_child(card)
        .insert(ChildOf(frame.viewport));
}

/// **The strip the toast centres in.** Its `Display` is what shows and hides the toast; the card
/// inside carries the words. See [`problem_toast`].
#[derive(Component)]
pub struct ToastLayer;

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
pub fn journal_panel(mut commands: Commands, frame: Res<Frame>) {
    let panel = commands
        .spawn((
            Node {
                // PLACES-ITSELF-OK: an overlay, not a panel in the frame's flow. It is asked for,
                // read, and dismissed — `Frame` owns the docks and this is deliberately not one.
                position_type: PositionType::Absolute,
                top: Val::Percent(8.0),
                left: Val::Percent(10.0),
                width: Val::Percent(80.0),
                max_height: Val::Percent(76.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(GAP_ROW),
                padding: UiRect::all(Val::Px(PAD)),
                border: UiRect::all(Val::Px(1.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(KEY),
            JournalPanel,
            // No `CopyPane` marker: `notice::copy_out` harvests the TEXT of every visible node by
            // walking the roots, so an open journal is carried by `Cmd+C` already — the marker
            // decides nothing there any more, and stamping it here would read as a second path.
            //
            // **`Hovered`, and deliberately not `Pickable::IGNORE`** — the rule [`panel_root`]
            // states, for a panel that is not in a dock. This was an opaque 80%x76% box that no
            // "is the pointer on UI" gate could see: neither `view::over_ui`, which filters to the
            // nodes carrying `Hovered`, nor the mouse verbs, which read a true one. So a click on
            // the open journal stamped or deleted a placement on the map behind it — invisibly,
            // under a solid panel — and the wheel zoomed the world instead of scrolling this list.
            // Exactly the pair the name box's inner box carries a `Hovered` to close, and the same
            // pair `panel_root`'s own note records from when only the rows had one.
            //
            // Nothing changes while it is away: a `Display::None` node has a zero rect, which both
            // the picking backend and `over_ui` skip for the same reason.
            Hovered::default(),
        ))
        .id();
    commands.entity(panel).with_children(|p| {
        title(p, "EVERY ERROR THIS SESSION");
        p.spawn((
            Text::new(format!(
                "{} closes  ·  {} copies this list",
                keys::chord(keys::Action::Cancel),
                keys::chord(keys::Action::CopyInfo)
            )),
            TextColor(DIM),
            TextFont::from_font_size(text::LABEL),
        ));
        // A list to scroll: a session's worth of refusals is longer than a screen by design.
        //
        // FOLLOW-OK: prose, not a selection. Nothing walks this — there is no highlight for the
        // arrows to move and no row to keep on screen, so the scroll exists for HEIGHT alone. The
        // detail pane makes the same declaration for the same reason.
        scroll_list(p, JournalList)
            .entry::<Node>()
            // **Room for the bar, because here the bar lands on words.** `scroll_list` overlays its
            // scrollbar rather than reserving a gutter — right for a list of short rows, and wrong
            // for wrapped prose, which runs under it. Measured in a frame: `…choose a piece in the
            // list|` with the thumb through the last word. Stated at this call site rather than in
            // the builder, because it is a fact about what THIS list holds.
            .and_modify(|mut n| n.padding.right = Val::Px(BAR_W + BAR_INSET * 2.0));
    });
    commands.entity(panel).insert(ChildOf(frame.viewport));
}

/// **The session journal's panel**, hidden until `Cmd+E`. See [`Journal`] for why it is not four
/// panels, and [`journal_panel`] for where it sits.
#[derive(Component)]
pub struct JournalPanel;

/// The scrolling list inside it, rebuilt from [`Journal`] by `notice::paint_journal`.
#[derive(Component)]
pub struct JournalList;

/// One bullet. `•` and not `-`: the shipped face has U+2022 (checked, as U+26A0 was not), and a
/// bullet reads as a list where a hyphen reads as a range.
///
/// # The bullet and the text are siblings, so it can wrap
///
/// This was one `Text` reading `"• {text}"` with `LineBreak::NoWrap`, on the argument that a wrapped
/// continuation restarts at column zero and breaks the bullet column. The argument was right and the
/// remedy was wrong: refusing to wrap does not keep a long refusal inside the panel, it runs it out
/// through the side of the box, which is what an author reported. This tab's messages name
/// descriptors, compositions and counts, so they are routinely longer than 380 px.
///
/// A row with the bullet as its own child fixes both at once: the text wraps, and its continuations
/// align under the text rather than under the bullet, because the bullet is not in that column.
///
/// `min_width: 0` is load-bearing. A flex item will not shrink below its min-content width by
/// default, and for text that is the longest word — so without it the row grows to fit and the
/// wrapping never happens. Same trick as `min_height: 0` on the scroll areas above.
pub fn problem_log_line(parent: &mut ChildSpawnerCommands, text: &str, colour: Color) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(4.0),
                width: Val::Percent(100.0),
                ..default()
            },
            ProblemLogLine,
        ))
        .with_children(|row| {
            row.spawn((
                Text::new("•"),
                TextColor(colour),
                TextFont::from_font_size(text::LABEL),
                TextLayout::new(Justify::Left, LineBreak::NoWrap),
            ));
            row.spawn((
                // `flex_grow` hands the text the row's remaining width as a DEFINITE size —
                // without it a wrapping text item's flex base can resolve to zero width and lay
                // out one glyph per line, clipped to nothing (measured live in the Compose pane,
                // 2026-08-17). `min_width: 0` is what then lets it shrink below its longest word.
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                },
                Text::new(text.to_owned()),
                TextColor(colour),
                TextFont::from_font_size(text::LABEL),
            ));
        });
}


/// **A scrolling list that fills what is left of its panel.**
///
/// `flex_grow` with `min_height: 0`, not `max_height`: a flex item's automatic minimum size is its
/// content, which would grow the node to fit every row and leave `overflow` with nothing to clip. The
/// panel must be `full_height` for this to bound anything.
/// **Where a scroll must move so a row is on screen** — the one arithmetic every list-follow
/// system shares (`tiles::keep_selection_on_screen`, `editor::keep_palette_selection_on_screen`).
///
/// Extracted when the palette gained the same correction the candidates list had (F-9,
/// **Arms when the SELECTION changes — not when the resource holding it does.**
///
/// Every list that follows its highlight needs the same two-step: notice the selection moved, then
/// scroll *next* frame, because the rows are rebuilt on that same change and this frame's
/// `ComputedNode` still describes the previous layout.
///
/// # Why this is a type and not two lines in each system
///
/// The two lines were `if state.is_changed() { pending = true; return; }`, written twice, and both
/// were quietly broken: `is_changed` is true whenever **anything** touches the resource, and both
/// `EditorState` and `ImportState` are written most frames — a status line, a hover, a preview
/// watchdog. So the flag was re-armed every frame and the correction never ran.
///
/// Reported from the keyboard twice. First on 2026-08-14 — *"if I arrow down and the scroll view, it
/// just goes off the screen. The scroll doesn't actually happen."* — and again on 2026-08-16, *"I
/// still have the same bug."* Both times the headless test passed, because in a test nothing else
/// writes the resource, so `is_changed` goes false on the next frame and the correction fires. The
/// test was measuring a world that only exists in tests.
///
/// Keyed on the selection itself, this cannot happen: unrelated writes are invisible to it, and the
/// only thing that arms it is the thing it exists to follow.
pub struct Follow<K> {
    last: Option<K>,
    pending: bool,
}

// A manual impl, because `#[derive(Default)]` would demand `K: Default` and a selection has no
// meaningful zero — `None` is "nothing selected", which is what this starts at.
impl<K> Default for Follow<K> {
    fn default() -> Self {
        Follow { last: None, pending: false }
    }
}

impl<K: PartialEq> Follow<K> {
    /// Give it this frame's selection; it answers whether to scroll **now**.
    ///
    /// `false` on the frame the selection moves (the layout is still last frame's), `true` on the
    /// one after, and `false` for ever until it moves again — so the scroll position is written once
    /// per move rather than sixty times a second, which is what keeps `ScrollPosition`'s change
    /// detection meaningful for anything else reading it.
    pub fn should_scroll(&mut self, now: Option<K>) -> bool {
        if self.last != now {
            self.last = now;
            self.pending = true;
            return false;
        }
        std::mem::take(&mut self.pending)
    }
}

/// 2026-08-14): two hand-copied versions of fold geometry is how the two lists drift a half-pixel
/// apart, and the arithmetic is the testable part — fold detection, the physical→logical
/// conversion, the clamp, and the dead-band all hold in a unit test, where the pixel scroll itself
/// needs a window.
///
/// Inputs are **physical** pixels — `ComputedNode` and `UiGlobalTransform`, centre and half-size —
/// and the answer is the new **logical** `ScrollPosition::y`, or `None` when the row is already
/// visible or the correction is under half a pixel (the dead-band that keeps a change-detected
/// write from re-firing layout every frame). A row taller than the list scrolls to its **top**,
/// the half you read first.
pub fn scroll_to_reveal(
    row: (f32, f32),
    list: (f32, f32),
    scroll_y: f32,
    inverse_scale: f32,
) -> Option<f32> {
    let (row_top, row_bottom) = (row.0 - row.1, row.0 + row.1);
    let (top, bottom) = (list.0 - list.1, list.0 + list.1);
    // Above the fold, or below it. Never both — the above check winning is what sends an
    // over-tall row to its top.
    let delta = if row_top < top {
        row_top - top
    } else if row_bottom > bottom {
        row_bottom - bottom
    } else {
        return None;
    };
    let want = (scroll_y + delta * inverse_scale).max(0.0);
    ((scroll_y - want).abs() > 0.5).then_some(want)
}

/// The scrollbar's track. Marked so [`hide_idle_scrollbars`] can find it and its own width is
/// stated once.
#[derive(Component)]
pub struct ScrollTrack;

/// Width of the bar, and how far it is inset from the panel's inner edge.
const BAR_W: f32 = 5.0;
const BAR_INSET: f32 = 1.0;
/// The thumb never shrinks below this, or a long list makes it a dot nobody can grab (Fitts, and
/// upstream's own reason for the parameter).
const MIN_THUMB: f32 = 18.0;

/// **A list that scrolls, and now says so.**
///
/// # It scrolled and nothing on screen admitted it
///
/// This spawned one node with `overflow: scroll_y` and a `ScrollArea`, and **no bar of any kind** —
/// so a list longer than its panel clipped, answered the wheel, and gave the author no indication
/// that there was more. It is visible in the 2026-08-18 captures: the Kit door's candidate list
/// opens part-way down, scrolled to the selection, with nothing saying what is above it.
///
/// # Shape, and why there is a wrapper
///
/// `bevy_ui_widgets::Scrollbar` goes on a **track** entity pointing at the scrolled one, and
/// `ScrollbarThumb` is its child. The track has to be positioned against the list's own box, so the
/// list gains a wrapper and the two are siblings inside it. The **marker stays on the scrolling
/// node** and that is load-bearing: every caller's queries, `chrome::Follow`'s reveal arithmetic and
/// `tests/every_list_follows_its_selection.rs` all key on it, and this returns the viewport rather
/// than the wrapper so not one call site changes.
///
/// **The thumb deliberately has no `Node`.** Upstream lays it out itself in `PostUpdate` after
/// `ui_layout_system`; giving it one looks like it works and then fights `update_scrollbar_thumb`.
///
/// **The bar overlays rather than reserving a gutter.** `Node.scrollbar_width` would reserve space,
/// and it also turns on a shipped disagreement: `ScrollArea`'s wheel handler computes its maximum
/// from `size()` while the scrollbar's own code subtracts `scrollbar_size`
/// (`scrollarea.rs:27` vs `scrollbar.rs:173`), so the two disagree by exactly the gutter width.
/// Overlaying sidesteps it.
///
/// # It bounds itself with `flex_grow`, so it needs a bounded parent
///
/// Right for a pane that owns the rest of its dock, and useless **nested inside another scroll**:
/// there the parent is content-sized, `flex_grow` resolves to nothing, and the whole thing collapses
/// to zero — measured, when the tag block vanished outright. A `scroll_box` taking a stated height
/// was written for that case and then deleted with its one caller: the answer was that a scroll
/// inside a scroll is two bars a hand has to choose between, and the pane already scrolled.
pub fn scroll_list<'a>(
    parent: &'a mut ChildSpawnerCommands,
    marker: impl Bundle,
) -> EntityCommands<'a> {
    let viewport = parent
        .commands_mut()
        .spawn((
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
        ))
        .id();

    let thumb = parent
        .commands_mut()
        .spawn((
            bevy::ui_widgets::ScrollbarThumb {
                border_radius: BorderRadius::all(Val::Px(BAR_W / 2.0)),
                border: UiRect::ZERO,
            },
            BackgroundColor(scaled(ROW_SELECTED, 0.9)),
        ))
        .id();

    let track = parent
        .commands_mut()
        .spawn((
            ScrollTrack,
            bevy::ui_widgets::Scrollbar::new(
                viewport,
                bevy::ui_widgets::ControlOrientation::Vertical,
                MIN_THUMB,
            ),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(BAR_INSET),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(BAR_W),
                // Hidden until there is somewhere to scroll — see [`hide_idle_scrollbars`].
                display: Display::None,
                ..default()
            },
            // The track is a drag target; the content under it is not. The panel root is
            // `Pickable::IGNORE`, so this narrows that for the one node here that is grabbable.
            bevy::picking::Pickable::default(),
        ))
        .id();
    parent.commands_mut().entity(track).add_child(thumb);

    let wrapper = parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            ..default()
        })
        .id();
    parent
        .commands_mut()
        .entity(wrapper)
        .add_children(&[viewport, track]);

    parent.commands_mut().entity(viewport)
}

/// **A bar only while there is somewhere to scroll** — the browser default, and the reason a panel
/// that fits its content shows no furniture at all.
///
/// `Display::None` rather than alpha, so a bar nobody needs costs no draw and no hit test.
///
/// **Compares before writing**, per the standing rule this crate keeps: `chrome::Follow`'s doc
/// records what unconditional per-frame writes cost, and `Node` is change-detected by the layout
/// system. Reading the target's `ComputedNode` from the track side is deliberate — the link runs
/// track → viewport and only that way, so a `Changed<ComputedNode>` filter here would be watching
/// the wrong entity.
pub fn hide_idle_scrollbars(
    mut tracks: Query<(&mut Node, &bevy::ui_widgets::Scrollbar), With<ScrollTrack>>,
    viewports: Query<&bevy::ui::ComputedNode>,
) {
    for (mut node, bar) in &mut tracks {
        let Ok(view) = viewports.get(bar.target) else {
            continue;
        };
        // The same clamp upstream uses, so the bar appears exactly when the wheel has somewhere to
        // go rather than one pixel before or after.
        let visible = view.size().y - view.scrollbar_size.y;
        let want = if view.content_size().y > visible + 0.5 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
}

// ── the row vocabulary ───────────────────────────────────────────────────────────────────────────
//
// The builders below were each written AFTER their shape had been hand-rolled at least three times
// (the 2026-08-17 audit counted 8 label/value rows, 7 list rows, 5 chip variants, 6 text fields, 3
// list headings and 2 severity-rail dialects). Each one returns or takes just enough for the call
// sites that exist — a parameter with no caller is a stub, which is why several return
// `EntityCommands` for the caller to finish rather than trying to hold every variation.

/// **A list panel's heading** — "PLACE", "RIGS": the word at the top of a whole panel, over its
/// filter and list. One step louder than [`section`] (10 vs 9) because it heads the panel, not a
/// block within one — the two roles the 2026-08-17 type-role decision named.
pub fn list_heading(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        Text::new(text.to_owned()),
        TextColor(LABEL),
        TextFont::from_font_size(text::HEADING),
    ));
}

/// **The fixed label column of a `LABEL  value` row** — the shape the audit found hand-rolled
/// eight times, differing only in this width. 10 px [`LABEL`], never wrapping, never shrinking:
/// the label is what keeps the values aligned, so it must hold its column.
pub fn row_label(row: &mut ChildSpawnerCommands, width: f32, label: &str) {
    row.spawn((
        Node {
            width: Val::Px(width),
            flex_shrink: 0.0,
            ..default()
        },
        Text::new(label.to_owned()),
        TextColor(LABEL),
        TextFont::from_font_size(text::LABEL),
        TextLayout::new(Justify::Left, LineBreak::NoWrap),
    ));
}

/// **The value beside a [`row_label`]** — 11 px, one step louder than its label, which is the
/// role map's one value size (the audit found 10/11 and 10/10 coexisting; 10/11 won).
pub fn row_value(row: &mut ChildSpawnerCommands, text: impl Into<String>, colour: Color, marker: impl Bundle) {
    row.spawn((
        Text::new(text.into()),
        TextColor(colour),
        TextFont::from_font_size(text::BODY),
        marker,
    ));
}

/// **A chip** — one small clickable word. [`CHIP_PAD`] and a permanent 1 px border whose COLOUR
/// asks the questions (a ghost proposal lights it [`SUGGEST`]; everything else runs [`Color::NONE`]),
/// so a chip never changes size when its state does.
///
/// **`marker` must not contain a `Node`, and the failure is a panic rather than a warning.** This
/// builder spawns one, and a bundle carrying two of the same component panics at spawn in Bevy 0.19
/// naming the component and not the call site — the trap the repo's `CLAUDE.md` already records for
/// `button_visual()`. To adjust the node, `.insert(Node { .. })` on the returned `EntityCommands`,
/// which overrides rather than duplicates. The same holds for [`list_row`], [`quiet_row`] and
/// [`text_field`], which each carry a `Node` of their own.
pub fn chip<'a>(
    parent: &'a mut ChildSpawnerCommands,
    marker: impl Bundle,
    label: &str,
    px: f32,
    ink: Color,
    fill: Color,
    border: Color,
) -> EntityCommands<'a> {
    let mut c = parent.spawn((
        bevy::ui_widgets::Button,
        Hovered::default(),
        // **A chip answers the pointer, like every row does.** It carried `Hovered` from the day it
        // was written — but only as a hit-test, for the "is the pointer over UI" question, and
        // nothing ever painted it. The 2026-08-17 audit found that as its seventh defect: *"hover
        // exists as a hit-test everywhere and as feedback almost nowhere"*, and the tag block is 55
        // clickable things that gave no sign of being clickable. `RowRest` is the whole fix —
        // `style_list_rows` already keys on it and has never needed to know what it is painting.
        RowRest(fill),
        marker,
        Node {
            padding: CHIP_PAD,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(border),
        BackgroundColor(fill),
    ));
    c.with_children(|chip| {
        chip.spawn((
            Text::new(label.to_owned()),
            TextColor(ink),
            TextFont::from_font_size(px),
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
        ));
    });
    c
}

/// The fill a list row returns to when the pointer leaves it. Carried on every [`list_row`] so ONE
/// system can give the whole editor's lists their hover state — the rebuilt lists are painted
/// inside change-gated `rebuild_*` systems that never see mouse motion, which is why hover was
/// deferred until the shared builder existed (the 2026-08-17 hover-scope decision).
#[derive(Component, Clone, Copy)]
pub struct RowRest(pub Color);

/// **A selectable list row** — full width, [`CHIP_PAD`] inset, [`ROW_SELECTED`] when it is the one
/// being acted on, [`ROW_HOVER`] under the pointer. Returns the row for the caller to fill; what a
/// row holds is the tab's business, that it looks like every other row is not.
pub fn list_row<'a>(
    parent: &'a mut ChildSpawnerCommands,
    selected: bool,
    marker: impl Bundle,
) -> EntityCommands<'a> {
    // `Button` is what puts a row on the editor's `Activate` bus — see [`quiet_row`] for who is
    // deliberately not on it.
    parent.spawn((bevy::ui_widgets::Button, row_shape(selected), marker))
}

/// **The same row, not on the `Activate` bus.**
///
/// # Why this exists, which is worth reading before merging the two back together
///
/// This editor has **twenty-four global `Activate` observers**, and most take a Map-door resource:
/// `on_row_click` wants `Res<Project>`, `on_tag_chip` wants `ResMut<Project>`. A global observer
/// fires for *any* `Activate` anywhere in the application, and in Bevy 0.19 a missing `Res<T>`
/// **panics** rather than skipping — so the first click on a `Button` outside the editor takes the
/// whole application down.
///
/// That was invisible while `list_row` was only ever called by editor panels. The moment the menu
/// adopted the shared row vocabulary, its first click panicked: `Res<Project>` does not exist on
/// `Screen::Menu`. Two observers were fixed to take `Option`; twenty-two were not, and each is the
/// same landmine for the next caller.
///
/// So a row that brings its own click handling says so, and stays off the bus. It keeps everything
/// that makes a row *look* like a row — [`RowRest`], `Hovered`, the fill, the padding — and
/// `style_list_rows` repaints it exactly the same, because that system keys on `RowRest` and has
/// never needed `Button`.
pub fn quiet_row<'a>(
    parent: &'a mut ChildSpawnerCommands,
    selected: bool,
    marker: impl Bundle,
) -> EntityCommands<'a> {
    parent.spawn((row_shape(selected), marker))
}

/// What a list row looks like, shared by [`list_row`] and [`quiet_row`] so the two cannot drift into
/// two row shapes — which is the drift this whole module exists to stop.
fn row_shape(selected: bool) -> impl Bundle {
    let rest = if selected { ROW_SELECTED } else { ROW_BG };
    (
        Hovered::default(),
        RowRest(rest),
        Node {
            width: Val::Percent(100.0),
            padding: CHIP_PAD,
            ..default()
        },
        BackgroundColor(rest),
    )
}

/// Selection beats hover, hover beats rest — the same priority every hand-rolled repainter uses
/// (`editor::style_rows`, `tiles::style_tabs`), stated once for every [`list_row`] in the editor.
fn style_list_rows(mut rows: Query<(&RowRest, &Hovered, &mut BackgroundColor)>) {
    for (rest, hovered, mut bg) in &mut rows {
        let want = if hovered.0 && rest.0 == ROW_BG { ROW_HOVER } else { rest.0 };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

/// **A text field's box** — [`MIN_FIELD_H`] floor, [`FIELD_PAD`] inset, [`ROW_BG`] fill, with its
/// readout text spawned inside. The audit found this shape six times with three paddings.
pub fn text_field<'a>(
    parent: &'a mut ChildSpawnerCommands,
    width: Val,
    field: impl Bundle,
    px: f32,
    initial: (String, Color),
    readout: impl Bundle,
) -> EntityCommands<'a> {
    let mut boxed = parent.spawn((
        bevy::ui_widgets::Button,
        Hovered::default(),
        field,
        Node {
            width,
            min_height: Val::Px(MIN_FIELD_H),
            padding: FIELD_PAD,
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(ROW_BG),
    ));
    boxed.with_children(|f| {
        f.spawn((
            Text::new(initial.0),
            TextColor(initial.1),
            TextFont::from_font_size(px),
            readout,
        ));
    });
    boxed
}

/// **What a field's readout shows**: the live keystrokes with a caret while focused, the committed
/// value otherwise. The `{raw}_` + [`ACCENT`] idiom was written five times before it had one home;
/// the caret is what makes "empty because you cleared it" distinguishable from "empty".
pub fn field_text(editing: Option<&str>, idle: (String, Color)) -> (String, Color) {
    match editing {
        Some(raw) => (format!("{raw}_"), ACCENT),
        None => idle,
    }
}

/// **One severity's tint and word, for every rail in the editor.** Two tabs printed the same words
/// under forked maps (tiles: Warn→ACCENT, Note→DIM; anim: everything-not-blocking→LABEL) — same
/// vocabulary, three colours, which is the drift this module exists to stop. The word and the hue
/// travel together so no site can pair "worth checking" with the wrong ink.
pub fn severity_style(severity: emerge_core::import::Severity) -> (Color, &'static str) {
    match severity {
        emerge_core::import::Severity::Blocking => (DANGER, "blocking"),
        emerge_core::import::Severity::Warn => (ACCENT, "worth checking"),
        emerge_core::import::Severity::Note => (DIM, "note"),
    }
}

/// **A severity rail** — the tinted left border that makes a finding's weight visible before it is
/// read. 2 px (the tiles dialect; the decision of 2026-08-17), [`GAP_TIGHT`] breathing room, a
/// [`GAP_ROW`] gap to the next. Returns the block for the caller to fill — the severity word first,
/// by convention, in the same tint. A clickable rail (anim's jump rows) adds `Button`/`Hovered`
/// and a fill through `marker`.
pub fn severity_rail<'a>(
    parent: &'a mut ChildSpawnerCommands,
    tint: Color,
    marker: impl Bundle,
) -> EntityCommands<'a> {
    parent.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            border: UiRect::left(Val::Px(2.0)),
            padding: UiRect::left(Val::Px(7.0))
                .with_top(Val::Px(GAP_TIGHT))
                .with_bottom(Val::Px(GAP_TIGHT)),
            margin: UiRect::bottom(Val::Px(GAP_ROW)),
            ..default()
        },
        BorderColor::all(tint),
        marker,
    ))
}

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
            // Above the panels and the tab strip (101), and below the key badges (500) and the
            // confirm prompt (900) — the tier for "the screen is asking you something", which
            // outranks a panel and is outranked by a question you have to answer.
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
                // **The dialog itself is UI; the dimmed backdrop behind it is not.**
                //
                // "Is the pointer over UI" is asked everywhere as "is any `Hovered` true"
                // (`view::drive`, `place_on_click`, `compose::pick_slot`), and this panel carried no
                // `Hovered` — so scrolling over a visible, open dialog zoomed the world behind it.
                //
                // This narrows the root's deliberate `Pickable::IGNORE` rather than undoing it. The
                // root stays click-through, because a prompt is not a modal that has to be dismissed
                // before anything else works; the 360 px box stops being, because a click landing *on*
                // the dialog and placing a piece behind it is the same bug as the scroll.
                //
                // Not `Pickable::IGNORE` here — that would make it unhoverable and reopen the hole.
                // `Hovered` is true for an entity or any descendant, so one on this panel answers for
                // the title, the value and the hint.
                Hovered::default(),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(String::new()),
                    TextFont::from_font_size(text::BODY),
                    TextColor(LABEL),
                    NameBoxTitle,
                ));
                b.spawn((
                    Text::new(String::new()),
                    TextFont::from_font_size(text::TITLE),
                    TextColor(ACCENT),
                    NameBoxValue,
                ));
                b.spawn((
                    Text::new(String::new()),
                    TextFont::from_font_size(text::BODY),
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
    editor: Res<crate::editor::EditorState>,
    build: Res<crate::build::Build>,
    mut roots: Query<&mut Node, With<NameBox>>,
    mut titles: Query<
        &mut Text,
        (
            With<NameBoxTitle>,
            Without<NameBoxValue>,
            Without<NameBoxHint>,
        ),
    >,
    mut values: Query<
        &mut Text,
        (
            With<NameBoxValue>,
            Without<NameBoxTitle>,
            Without<NameBoxHint>,
        ),
    >,
    mut hints: Query<
        &mut Text,
        (
            With<NameBoxHint>,
            Without<NameBoxTitle>,
            Without<NameBoxValue>,
        ),
    >,
) {
    // **`grouping.is_some()` and the box being visible are the same condition**, and that is the
    // invariant rather than a description of the code.
    //
    // This used to also match on `Mode::Map`, which looked like a harmless guard and was not:
    // `EditorState::grouping` has no mode scope, so clicking the tab strip mid-name hid the box while
    // `group_name_keys` kept consuming the keyboard — every keystroke vanished until `Esc`, with
    // nothing on screen to say where they were going. Two conditions for one question is what made
    // that state reachable.
    //
    // The tab switch now clears `grouping` (see `tiles::leaving_a_tab_puts_the_name_prompt_down`), so
    // there is one owner and this reads it. The prompt is the Map's, and if a second tab ever asks
    // again the answer is another field, not another condition on this one.
    // **Two fields, one box** — the Map names a composition, the Tiles tab names a tile, and each
    // owns its own `Option<String>`. That is what this function's own note asked for when the
    // second one arrived: *"another field, not another condition on this one."* They cannot both be
    // open, because the tabs are never live together, and the title says which is being answered.
    let asking: Option<(&str, String, String)> = editor
        .grouping
        .as_ref()
        .map(|raw| {
            (
                "NAME THIS COMPOSITION",
                // Forced to snake_case as it is typed, so the naming rule teaches itself.
                format!("{}_", emerge_core::naming::to_snake_case(raw)),
                "Enter keeps it.   Esc leaves the set in hand.".to_owned(),
            )
        })
        .or_else(|| {
            build.naming.as_ref().map(|prompt| {
                (
                    "NAME THIS TILE",
                    format!("{}_", emerge_core::naming::to_snake_case(&prompt.raw)),
                    "Enter opens it.   Esc leaves things as they are.".to_owned(),
                )
            })
        });
    let display = if asking.is_some() {
        Display::Flex
    } else {
        Display::None
    };
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
    //
    // **It takes `&mut Mut<Text>`, not `&mut Text`.** Coercing a `Mut<Text>` down to `&mut Text` at
    // the call site runs `Mut::deref_mut`, which calls `set_changed()` — so the string was compared
    // and the component was dirtied anyway, every frame, which is exactly what the comment above
    // says this guard exists to stop. Reading `text.0` through `&mut Mut<_>` uses `Deref`; only the
    // assignment reaches `DerefMut`.
    fn set(text: &mut Mut<Text>, want: &str) {
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


/// **The window's own furniture** — the frame, the chrome bar, the status band, the name box, and
/// the hover every list row shares.
///
/// The key list used to live here too, as a centred two-column table behind a scrim. It is
/// `crate::badges` now, and the argument for moving it is worth keeping where the deletion happened:
/// a table asks you to map a phrase back onto something on screen, and two verbs that had been bound
/// all along (`R`, `Shift+Delete`) stayed invisible for two sessions because their phrase was
/// collapsed onto one row. Drawing the chord *on the thing it acts on* removes that step.
///
/// What survives the move is the line that says the key exists. That line is not optional: Cockburn,
/// Gutwin, Scarr & Malacria 2014 (`10.1145/2659796`, already cited by `crate::keys`) document the
/// intermodal-transition failure — a fast path offered *beside* a slow one does not get adopted on
/// its own — and ExposeHK names its own weakness as exactly this, that an overlay behind a modifier
/// *"has no visual representation to aid discovery until the user … presses its trigger."* See
/// [`shortcut_hint`].
pub struct ChromePlugin;

impl Plugin for ChromePlugin {
    fn build(&self, app: &mut App) {
        app
            // **The frame first, and everything that puts a panel in it after.** `Res<Frame>` is
            // how `panel_root` finds its dock, and in Bevy 0.19 a missing `Res<T>` panics its
            // system rather than skipping it — so this is an ordering the build must state, not one
            // that happens to hold. Ordering across a set also gets a sync point inserted, which is
            // what makes the deferred `insert_resource` visible to the spawners.
            .add_systems(
                OnEnter(crate::screen::Screen::Editor),
                spawn_frame.in_set(FrameSystems),
            )
            // **The menu gets the same frame**, and that is the point of it being the window's shape
            // rather than the editor's: one answer to "how is this application laid out" instead of
            // the two that let the menu drift into a fixed-pixel grid sitting in a window it did not
            // fill.
            .add_systems(
                OnEnter(crate::screen::Screen::Menu),
                spawn_frame.in_set(FrameSystems),
            )
            .add_systems(
                OnEnter(crate::screen::Screen::Editor),
                (
                    spawn_name_box,
                    spawn_chrome_bar,
                    spawn_status_band,
                    problem_toast,
                )
                    .after(FrameSystems),
            )
            // Every list row's hover, in one place — the rows themselves are spawned by
            // change-gated rebuilds that never see mouse motion.
            //
            // **Ungated by screen**, unlike the systems below it: the menu draws rows too, and the
            // query simply matches nothing on a screen that has none. Gating it would be a second
            // place that has to know which screens have lists.
            .add_systems(
                Update,
                (
                    style_list_rows,
                    hide_idle_scrollbars,
                    repaint_where_you_are,
                ),
            )
            // **After `Phase::Text`, not in it.** The field consumes the keystroke there; painting
            // before it would show the box one character behind what has been typed.
            .add_systems(Update,
                (paint_name_box.after(keys::Phase::Text))
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            .add_systems(Update,
                (light_the_back_button, stand_down_the_back_chord, dim_the_ground)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            );
    }
}



#[cfg(test)]
mod scroll_tests {
    use super::{scroll_to_reveal, Follow};

    /// **The arming rule, which is the half that was broken for two days.**
    ///
    /// `scroll_to_reveal`'s arithmetic was always right and always tested; nothing ever called it,
    /// because the flag that should have said "now" was re-armed every frame by an unrelated write.
    /// So this tests the flag, not the sums.
    #[test]
    fn a_follower_arms_on_the_selection_and_fires_exactly_once() {
        let mut f: Follow<usize> = Follow::default();

        // The frame the selection appears: do NOT scroll — the rows were rebuilt this frame and the
        // geometry still describes the previous layout.
        assert!(!f.should_scroll(Some(3)), "the move frame reads stale layout");
        // The next frame, with the selection unchanged: scroll, once.
        assert!(f.should_scroll(Some(3)), "the frame after is when the rows are real");
        assert!(!f.should_scroll(Some(3)), "and it does not write every frame after that");
        assert!(!f.should_scroll(Some(3)));

        // Moving again re-arms the same two-step.
        assert!(!f.should_scroll(Some(4)));
        assert!(f.should_scroll(Some(4)));

        // **The regression itself**: unrelated churn must not touch this. A resource-keyed follower
        // saw `is_changed` every frame — a status line, a hover, a preview watchdog — re-armed
        // itself, and never fired. Nothing here can observe that, which is the point.
        assert!(!f.should_scroll(Some(4)), "still quiet however busy the resource is");

        // Losing the selection arms too, so a list that had a highlight and now has none does not
        // scroll to a row that is gone.
        assert!(!f.should_scroll(None));
        assert!(f.should_scroll(None));
    }



    /// A row already inside the fold asks for nothing — the common case, and the one that keeps a
    /// change-detected `ScrollPosition` from being touched sixty times a second.
    #[test]
    fn a_visible_row_asks_for_no_scroll() {
        // List centred at 200, half 100 → fold [100, 300]. Row at 150, half 10 → inside.
        assert_eq!(
            scroll_to_reveal((150.0, 10.0), (200.0, 100.0), 0.0, 1.0),
            None
        );
        // Touching the edges exactly is still inside — flush is not off-screen.
        assert_eq!(
            scroll_to_reveal((110.0, 10.0), (200.0, 100.0), 0.0, 1.0),
            None
        );
        assert_eq!(
            scroll_to_reveal((290.0, 10.0), (200.0, 100.0), 0.0, 1.0),
            None
        );
    }

    /// Walking down past the fold scrolls down by exactly the overshoot; walking up, up.
    #[test]
    fn an_off_screen_row_scrolls_by_its_overshoot() {
        // Row bottom at 320 against a fold ending at 300: 20 px further down.
        assert_eq!(
            scroll_to_reveal((310.0, 10.0), (200.0, 100.0), 40.0, 1.0),
            Some(60.0)
        );
        // Row top at 80 against a fold starting at 100: 20 px back up.
        assert_eq!(
            scroll_to_reveal((90.0, 10.0), (200.0, 100.0), 40.0, 1.0),
            Some(20.0)
        );
    }

    /// The answer is logical pixels: a 2x display (inverse scale 0.5) halves the physical delta.
    #[test]
    fn the_correction_converts_physical_to_logical() {
        assert_eq!(
            scroll_to_reveal((310.0, 10.0), (200.0, 100.0), 40.0, 0.5),
            Some(50.0)
        );
    }

    /// The scroll never goes negative — the top of the list is the top.
    #[test]
    fn the_scroll_clamps_at_zero() {
        assert_eq!(
            scroll_to_reveal((50.0, 10.0), (200.0, 100.0), 10.0, 1.0),
            Some(0.0)
        );
    }

    /// A correction under half a pixel is noise, not a scroll — the dead-band that stops a
    /// float-jittering layout from re-marking the resource changed every frame.
    #[test]
    fn a_sub_pixel_correction_is_swallowed() {
        assert_eq!(
            scroll_to_reveal((300.3, 0.1), (200.0, 100.0), 0.0, 1.0),
            None
        );
    }

    /// A row taller than the whole list aligns its TOP — the half you read first — rather than
    /// oscillating between its two unsatisfiable edges.
    #[test]
    fn an_over_tall_row_aligns_its_top() {
        // Row spans [40, 560] against fold [100, 300]: top wins, scroll up by 60.
        assert_eq!(
            scroll_to_reveal((300.0, 260.0), (200.0, 100.0), 100.0, 1.0),
            Some(40.0)
        );
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
        assert_eq!(
            s.note_text(),
            "filled 12",
            "the receipt line stopped being current"
        );
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
        assert_eq!(
            s.problems().len(),
            3,
            "a repeat after something else is a new entry"
        );
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
        assert_eq!(
            s.note_text(),
            "recorded 3 member(s)",
            "an Err overwrote the receipt"
        );
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
/// **What this door is open on** — the map's name, or the kit's if no map is open.
///
/// One function because two things say it: the window's title (`main::name_the_window`) and the
/// chrome bar. Which *name* is the right one is a decision, and a decision made twice is a decision
/// that drifts — the window would go on saying `furniture` while the bar said the map, and nobody
/// would notice for a week.
pub fn subject(
    open_map: Option<&crate::project::OpenMap>,
    project: Option<&crate::project::Project>,
) -> Option<String> {
    match (open_map, project) {
        (Some(m), _) => Some(m.map.name.clone()),
        (None, Some(p)) => Some(p.namespace.clone()),
        _ => None,
    }
}

/// **The way back to the chooser, and the one piece of the window that belongs to no door.**
///
/// # It failed twice as a row in a panel
///
/// Asked for at the keyboard: *"when we go into the map editor, we actually need a button to go back
/// to the main UI."* There was only `Cmd+O`, and a key nothing on screen mentions is a key nobody
/// finds. So a button was added — **inside the left panel**, under that panel's own heading, on
/// `SLOT_BG`, the ground this editor uses for an inspector slot. It was reported missing again:
/// *"When I enter kit editing, there's no clear way to get back to the main menu."*
///
/// `docs/2026-08-17-one-application.md` §3 diagnosed why, and it was not the contrast: the encoding
/// said *"a field of the thing you are looking at"* rather than *"a way out of it"*, because it read
/// as a row in a list of rows, and **nothing at window level was navigation at all**. This repo
/// already settled what to do when a signal fails twice — *"the encoding was not weak, it was
/// wrong"*.
///
/// So it is chrome now, above the door's own strip, the same on every door. **Four call sites
/// became one**, which is the other half of the point: a way out that each panel places is a way out
/// each panel can forget, and the Rigs door drew it in a different place from the Map door.
///
/// It still names its chord beside itself, which is `ExposeHK`'s rehearsal argument — the pointer is
/// the way in, and the label beside it is what turns a pointing habit into a typing one. The click
/// and the key both go through `editor::leave_for_menu`, so the unsaved-work refusal cannot differ
/// between them.
pub fn spawn_chrome_bar(
    mut commands: Commands,
    frame: Res<Frame>,
    door: Option<Res<crate::tiles::Door>>,
    open_map: Option<Res<crate::project::OpenMap>>,
    project: Option<Res<crate::project::Project>>,
) {
    let here = door.as_deref().map(|d| {
        match subject(open_map.as_deref(), project.as_deref()) {
            Some(name) => format!("{} · {name}", d.label()),
            None => d.label().to_owned(),
        }
    });

    commands.entity(frame.chrome_bar).with_children(|bar| {
        bar.spawn((
            BackButton,
            Control(keys::ControlId::Back),
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(GAP_ROW),
                padding: CHIP_PAD,
                ..default()
            },
            BackgroundColor(SLOT_BG),
            // The bar is `Pickable::IGNORE` so the frame answers no clicks; this narrows that for
            // the one node on it that is a button — the same move `spawn_name_box` makes.
            bevy::picking::Pickable::default(),
            Hovered::default(),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new("\u{2039} kits & maps".to_owned()),
                TextColor(KEY),
                TextFont::from_font_size(text::BODY),
                bevy::picking::Pickable::IGNORE,
            ));
            b.spawn((
                BackChord,
                Text::new(keys::chord(keys::Action::MainMenu)),
                TextColor(LABEL),
                TextFont::from_font_size(text::BODY),
                bevy::picking::Pickable::IGNORE,
            ));
        });

        // Pushes the subject to the far end. A spacer rather than `SpaceBetween` on the bar, so a
        // third thing added later lands where it is put rather than being redistributed.
        bar.spawn((
            Node {
                flex_grow: 1.0,
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ));

        if let Some(here) = here {
            bar.spawn((
                WhereYouAre,
                // Where you are is what `N` renames, so its badge goes on the words it changes.
                Control(keys::ControlId::Title),
                Text::new(here),
                TextColor(DIM),
                TextFont::from_font_size(text::BODY),
                bevy::picking::Pickable::IGNORE,
            ));
        }
    });
}

/// The bar's right-hand end: which door, on what. Marked so a test can find it — and so
/// [`repaint_where_you_are`] can keep it true.
#[derive(Component)]
pub struct WhereYouAre;

/// **The bar says what the title says, for as long as both are on screen.**
///
/// [`spawn_chrome_bar`] runs once, at `OnEnter(Editor)`, and nothing repainted this — so renaming a
/// map with `N` moved `main::name_the_window`'s title and left the bar reading the old name for the
/// rest of the session. That is exactly the drift [`subject`] was extracted to make impossible,
/// arriving through the half that was never wired: one function deciding the name is worth nothing
/// if only one of its two readers ever asks it.
///
/// Compared before writing, per the standing rule — `Text` is change-detected and this runs every
/// frame.
pub fn repaint_where_you_are(
    door: Option<Res<crate::tiles::Door>>,
    open_map: Option<Res<crate::project::OpenMap>>,
    project: Option<Res<crate::project::Project>>,
    mut bar: Query<&mut Text, With<WhereYouAre>>,
) {
    let Some(door) = door else { return };
    let want = match subject(open_map.as_deref(), project.as_deref()) {
        Some(name) => format!("{} \u{00b7} {name}", door.label()),
        None => door.label().to_owned(),
    };
    for mut text in &mut bar {
        if text.0 != want {
            text.0 = want.clone();
        }
    }
}

/// **Every panel this editor has.** The status band speaks for whichever one is live, so it names
/// them all and `notice::paint_notices` picks — rather than five bands each hidden by its own tab.
pub const ALL_TABS: &[crate::tiles::Mode] = &[
    crate::tiles::Mode::Map,
    crate::tiles::Mode::Meshes,
    crate::tiles::Mode::Tiles,
    crate::tiles::Mode::Anim,
    crate::tiles::Mode::Compose,
];


/// **One band at the foot of the window: what went wrong, and what the keys are.**
///
/// Both of these were spawned *inside* each panel, once per tab, which had two costs. The banner
/// carries the longest text this editor renders — refusals naming descriptors and compositions —
/// and a 300 px controls panel is the narrowest place on screen to put it. And the hint line, at
/// `CONTROLS_W`, wrapped: *"Hold K for shortcuts · Cmd+O back to kits & maps"* broke across two
/// lines on the Map door, which is visible in the capture that started this work.
///
/// Full window width fixes both by construction, and it deletes four copies of the same two calls.
///
/// **The hint goes at the far end behind a spacer, not in a `SpaceBetween` row**, so it does not
/// move when the banner appears — a hint that shifts sideways the moment something goes wrong is a
/// hint you have to re-find exactly when you are least able to.
///
/// The problem **log** deliberately stays in the panels. The banner answers *"what just happened"*
/// and belongs to the window; the log answers *"what has gone wrong here"*, is a stack of lines
/// rather than one, and a band that grew to hold it would move the viewport every time a refusal
/// landed.
pub fn spawn_status_band(mut commands: Commands, frame: Res<Frame>) {
    commands.entity(frame.status).with_children(|band| {
        // **The problem is not here any more.** It used to be a flow child of this band — a padded,
        // wrapping block inside twenty-six pixels of chrome, so it stood proud of the band it was
        // supposed to be in. Reported from the keyboard: *"it appears over the status bar at the
        // bottom and isn't quite in alignment… so it looks really bad."* It is a toast over the
        // viewport now; see [`problem_toast`].
        // **Which model is answering, once for the session.** It used to be printed on every
        // proposal, which is the same fact restated per piece — see `labels::Labeler`, which owns
        // both the text and the rule about when the word `connected` may be used. Empty until the
        // config has been read, so an unconfigured project shows nothing rather than a placeholder.
        band.spawn((
            LabelerLine,
            Text::new(String::new()),
            TextColor(DIM),
            TextFont::from_font_size(text::LABEL),
            bevy::picking::Pickable::IGNORE,
        ));
        band.spawn((
            Node {
                flex_grow: 1.0,
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ));
        shortcut_hint(band);
    });
}

/// **The status band's labeler readout.** The node is here because the band is; what it says is
/// `labels::Labeler`'s, so this module does not learn what a model is.
#[derive(Component)]
pub struct LabelerLine;

/// The clickable way back. Marked so `editor` can hang one observer on it wherever a panel put it.
#[derive(Component)]
pub struct BackButton;

/// **The chord printed on the button**, so it can stand down while the badges are up.
#[derive(Component)]
pub struct BackChord;

/// **One `Cmd+O` on the button, never two.**
///
/// The chord is printed beside `‹ kits & maps` for the reason [`spawn_chrome_bar`] argues at
/// length — ExposeHK's rehearsal path, and an author who pressed `Esc` three times looking for the
/// exit. Holding the shortcuts key then draws that same chord *again*, as the button's badge, one
/// gap away from itself. Reported from the keyboard: the back button prints `Cmd+O` twice.
///
/// So while the overlay is up, the inline copy stands down and the badge is the one that speaks.
/// This is the move `compass::show_by_tab` already makes for the gizmo, and for the same reason: two
/// things that are each right on their own are not both wanted in the same instant.
///
/// Compares before writing — `Node` is change-detected by the layout system, and `Mut::deref_mut`
/// marks it changed whether or not the value moved.
fn stand_down_the_back_chord(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mut chords: Query<&mut Node, With<BackChord>>,
) {
    let want = if keys::pressed(&keyboard, *live, keys::Action::Shortcuts) {
        Display::None
    } else {
        Display::Flex
    };
    for mut node in &mut chords {
        if node.display != want {
            node.display = want;
        }
    }
}

/// **While the shortcuts key is held, the grounds step back and the badges are the nearest thing.**
///
/// This is the other half of letting a badge stand on a panel's empty middle. `badges::ink` reads
/// [`Ground`] to say that a container's fill carries nothing a reader needs, so the ground under a
/// short list is placeable — but a badge is itself a [`PANEL_BG`] box, and an opaque box on an
/// opaque box of the same colour reads as one shape with a line through it. Dropping the ground to
/// [`GROUND_HELD`] lets the [`VEIL`] and the window behind it show through, and the badge — which
/// keeps its own opacity — separates by depth rather than by its border alone.
///
/// **Fill only, and restored from the carried colour.** Every row, field, chip and word inside the
/// panel keeps its opacity, because those are exactly what the badges are pointing at.
///
/// Compares before writing: `BackgroundColor` is change-detected, and touching every panel every
/// frame would mark the whole interface dirty sixty times a second for one held key
/// (`tests/no_system_writes_every_frame.rs`).
fn dim_the_ground(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mut grounds: Query<(&Ground, &mut BackgroundColor)>,
) {
    let held = keys::pressed(&keyboard, *live, keys::Action::Shortcuts);
    for (ground, mut bg) in &mut grounds {
        let want = if held {
            ground.0.with_alpha(ground.0.alpha() * GROUND_HELD)
        } else {
            ground.0
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

/// Lift the button while the pointer is on it, so it reads as pressable before it is pressed.
pub fn light_the_back_button(
    mut buttons: Query<(&Hovered, &mut BackgroundColor), (With<BackButton>, Changed<Hovered>)>,
) {
    for (hovered, mut bg) in &mut buttons {
        bg.0 = if hovered.get() { ROW_SELECTED } else { SLOT_BG };
    }
}

pub fn shortcut_hint(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        // **The way out is named here, not only on the button above it.**
        //
        // The button says `‹ kits & maps` at 11px and this line said `Hold K for shortcuts` at 10px,
        // and an author looking for the exit found neither — they pressed `Esc` three times instead,
        // which is the one key deliberately wired to mean "not that" rather than "out". Naming the
        // chord where the eye already goes for keys is ExposeHK's rehearsal argument: the novice
        // path and the expert path are the same path.
        //
        // **It says what the key does now, not what it opens.** `K` used to raise a table you read;
        // it labels the interface in place. "For shortcuts" described a document, and the whole
        // reason the document was replaced is that a reader had to carry a phrase from it back to
        // something on screen.
        Text::new(format!(
            "Hold {} — every key lands on what it does   ·   {} back to kits & maps",
            keys::chord(keys::Action::Shortcuts),
            keys::chord(keys::Action::MainMenu),
        )),
        TextColor(LABEL),
        TextFont::from_font_size(text::LABEL),
        // **And holding it puts that key's own badge on this line.** The one control whose verb is
        // "show me the verbs" — ExposeHK's admitted weakness is that a modifier-triggered overlay
        // *"has no visual representation to aid discovery until the user … presses its trigger"*, and
        // this is the loop closing: the line that tells you about `K` is where `K` labels itself.
        Control(keys::ControlId::Hint),
    ));
}
