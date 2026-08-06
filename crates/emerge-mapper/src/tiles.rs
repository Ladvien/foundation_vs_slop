//! **Tile configuration** — bringing meshes in, and saying what they are.
//!
//! The editor's second tab. `emerge_core::import` does the measuring; this is where an author reads
//! it, gives a mesh an id, decides which layer it goes on and what it is tagged as, and accepts it
//! into the library. Separate from the map tab because they are different jobs with different
//! controls, and one panel trying to hold both would be a panel that does neither well.
//!
//! # The scan is lazy and says how big it was
//!
//! This project ships 360 meshes and 41 are in the library, so the candidate list is around 319. That
//! is a second of file reading, and doing it at launch would make every session pay for a mode most
//! of them never open. It happens on the first Tab, and the panel reports what it found — a list of
//! 319 with no count is a list nobody trusts they have seen the end of.
//!
//! # Findings are shown with their fix
//!
//! A warning that does not say what to do about it is a warning that gets read once. Every
//! [`emerge_core::import::Finding`] that has an obvious remedy carries it, and the panel shows both.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton, ScrollArea};
use emerge_core::descriptor::{mount_label, mount_options, Descriptor};
use emerge_core::import::{self, Candidate, Severity};

use crate::chrome::{ACCENT, DANGER, DIM, HEADER_BG, LABEL, PANEL_BG, ROW_BG, ROW_SELECTED, TEXT};
use crate::keys::{self, Action};
use crate::project::Project;

/// Which job the editor is doing.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Place pieces and build the level.
    #[default]
    Map,
    /// Bring meshes in and say what they are.
    Tiles,
    /// Preview and tune a rig's clips.
    Anim,
}

impl Mode {
    /// The tabs, in the order they are shown. Map first: it is the job, and configuring tiles is
    /// what you do in order to do it.
    pub const ALL: [Mode; 3] = [Mode::Map, Mode::Tiles, Mode::Anim];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Map => "MAP",
            Mode::Tiles => "TILES",
            Mode::Anim => "ANIM",
        }
    }

    /// The number key that jumps straight here.
    ///
    /// A direct key per tab as well as `Tab` to cycle, because cycling is fine for two and useless
    /// for four — and `docs/ui.md` §4.2 wants everything reachable by mouse reachable by keyboard.
    pub fn action(self) -> crate::keys::Action {
        match self {
            Mode::Map => crate::keys::Action::MapTab,
            Mode::Tiles => crate::keys::Action::TilesTab,
            Mode::Anim => crate::keys::Action::AnimTab,
        }
    }

    /// This tab as a key context. The census speaks in [`crate::keys::Context`] and the app speaks in
    /// `Mode`; this is the single place the two are the same thing, so a fourth tab is one arm here
    /// rather than a search for every `*mode ==` in the crate.
    pub fn context(self) -> crate::keys::Context {
        match self {
            Mode::Map => crate::keys::Context::Map,
            Mode::Tiles => crate::keys::Context::Tiles,
            Mode::Anim => crate::keys::Context::Anim,
        }
    }
}

#[derive(Resource, Default)]
pub struct ImportState {
    pub candidates: Vec<Candidate>,
    pub selected: usize,
    /// Whether the directory has been walked yet. Separate from `candidates.is_empty()`, which is
    /// also true of a directory with nothing new in it — and those two states want different words.
    pub scanned: bool,
    /// What the last scan found. **Persistent**, and separate from [`Self::status`] on purpose: the
    /// first version had one field, so "319 mesh(es) not in the library" was replaced by "layer: on
    /// support" the moment anyone did anything, and the one number that says whether you have seen
    /// the whole list was gone for the rest of the session.
    pub summary: String,
    /// The last thing that happened. Transient, and lives at the bottom where a changing line belongs.
    pub status: String,
    /// The candidate being renamed and the raw text typed so far, or `None` when not renaming.
    /// Snake case is applied for display and on commit, exactly as the map's name is — one rule, one
    /// behaviour.
    ///
    /// **Carries its target.** The commit used to write to `candidates[self.selected]`, read at
    /// `Enter` — so clicking another row mid-word renamed the piece the author had just moved to
    /// rather than the one they had been typing about. Identified by **mesh path** rather than by
    /// index, because `R` rescans and rebuilds the list: an index would point somewhere else, and
    /// silently.
    pub renaming: Option<Rename>,
    /// The library entry selected for removal, if one is. Separate from [`Self::selected`], which
    /// indexes candidates — the two lists are different things and one index into both would be a
    /// bug waiting for the first time their lengths differ.
    pub selected_library_id: Option<String>,
    /// Packs the author has folded away.
    pub folded_packs: std::collections::HashSet<String>,
}


/// **Where a tile is examined, far from the map.**
///
/// The tiles tab is about ONE piece, and the map behind it was noise — worse than noise, because a
/// candidate spawned at the world origin sat *among* the map's placements and could be mistaken for
/// one of them.
///
/// A staging point rather than hiding the map: `apply_mode` toggles UI `display` only, and hiding
/// world entities would mean touching every placement on every tab change — hundreds of visibility
/// writes to solve a framing problem. `thumbs.rs` already stages its subject at `BOOTH` for the same
/// reason. This is a different corner so the two can never be in shot together.
pub const STAGE: Vec3 = Vec3::new(-4096.0, 0.0, 4096.0);

/// Where the map camera was, so leaving the tab puts it back.
///
/// Without this, coming back from a tile left the author at the origin looking at whatever happened
/// to be there — their pan and zoom silently discarded.
#[derive(Resource, Default)]
struct MapView(Option<crate::view::Rig>);

/// Move the camera to the stage on entering the tab, and back on leaving.
fn stage_camera(
    mode: Res<Mode>,
    mut rig: ResMut<crate::view::Rig>,
    mut saved: ResMut<MapView>,
) {
    if !mode.is_changed() {
        return;
    }
    match *mode {
        Mode::Tiles => {
            if saved.0.is_none() {
                saved.0 = Some(crate::view::Rig {
                    focus: rig.focus,
                    height: rig.height,
                    yaw: rig.yaw,
                    goal_yaw: rig.goal_yaw,
                });
            }
            rig.focus = STAGE;
            // Close enough that one grid cell fills the view — the tab is about a single tile.
            rig.height = TILE_VIEW_HEIGHT;
        }
        _ => {
            if let Some(was) = saved.0.take() {
                rig.focus = was.focus;
                rig.height = was.height;
                rig.yaw = was.yaw;
                rig.goal_yaw = was.goal_yaw;
            }
        }
    }
}

/// Orthographic viewport height on the stage, metres. About three grid cells, so a 1-cell piece has
/// room around it and a 3-cell one still fits.
const TILE_VIEW_HEIGHT: f32 = 4.0;

/// **The divisions readout** — derived, not typed.
///
/// Divisions used to be three editable fields on the descriptor. They are now one number on the
/// project (`project.ron`'s `divisions`), and a piece's lattice is derived from its own size, so
/// there is nothing here to type into: a per-tile override would be a second way to say what the
/// project already says, and the two would disagree the first time anybody used it.
///
/// What is left is worth showing, because an author placing an edge token needs to know what a cell
/// is worth: the derived `x x y x z` and the subunit's size in millimetres.
#[derive(Component)]
pub struct DivReadout;

/// **Which piece an edit was opened against.**
///
/// Every text field here has the same shape: it opens on the focused piece, the author types, and it
/// commits on `Enter`. Reading the focus *at commit* meant a click in between redirected the edit —
/// the review's finding #5. So a field captures this when it opens and writes through
/// [`ImportState::at_target`], and where the focus has moved to by then does not matter.
///
/// A library entry is named by its id, which is what `Library` looks up by. A candidate is named by
/// its **mesh path**: unique across the scan, and stable across the rebuild `R` does, where an index
/// would quietly point at a different piece.
#[derive(Clone, PartialEq, Debug)]
pub enum EditTarget {
    Library(String),
    Candidate(String),
}

/// A candidate id being typed, and which candidate it belongs to.
///
/// See [`ImportState::renaming`] for why the target is captured at open and why it is a mesh path.
#[derive(Clone)]
pub struct Rename {
    /// The candidate's mesh path — unique, and stable across a rescan.
    pub mesh: String,
    pub raw: String,
}

/// Which facet of a cell a token is being typed into.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CellField {
    Edge,
    Anchor,
}

/// A verb that acts on the selected cell.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellVerb {
    /// Occupancy — the common one, so it is first, is a toggle rather than a field, and is what a
    /// header does before any chip has been touched.
    #[default]
    Solid,
    /// Type an edge token.
    Edge,
    /// Type an anchor role.
    Anchor,
    /// Forget the cell entirely.
    Clear,
}

impl CellVerb {
    fn label(self) -> &'static str {
        match self {
            CellVerb::Solid => "solid",
            CellVerb::Edge => "edge",
            CellVerb::Anchor => "anchor",
            CellVerb::Clear => "clear",
        }
    }
}

/// One cell button in the layer grid, carrying its (x, z).
#[derive(Component, Clone, Copy)]
pub struct CellButton(pub u32, pub u32);

/// What a header covers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// Every cell in the layer.
    Layer,
    /// One x column, all z.
    Column(u32),
    /// One z row, all x.
    Row(u32),
}

/// A clickable header that applies the armed verb to a whole [`Span`] of one layer.
///
/// Marking a wall's outward face is nine identical clicks on a 3x3 lattice and rather more on a finer
/// one; the shape being marked is almost always a row, a column or a whole slice, so those are the
/// three shapes with a button.
#[derive(Component, Clone, Copy)]
pub struct FillHeader {
    pub layer: u32,
    pub span: Span,
}

/// Which layer a cell button or glyph belongs to. Every layer is drawn at once, so `(x, z)` alone no
/// longer names a cell.
#[derive(Component, Clone, Copy)]
pub struct CellLayer(pub u32);

/// The glyph inside a [`CellButton`], carrying the same `(x, z)`.
#[derive(Component, Clone, Copy)]
pub struct CellGlyph(pub u32, pub u32);

/// The line under the grid saying what the selected cell holds.
#[derive(Component)]
pub struct SelectedCellLine;

/// The description's text, refreshed in place while it is typed into.
#[derive(Component)]
pub struct NoteReadout;

/// **Which cell is being edited, and what is being typed into it.**
///
/// Its own resource, and the reason is not the one the old comment gave. `CellEdit` *is* watched
/// (`resource_changed::<CellEdit>` in the plugin), and `cell_keys` deref-muts it on every character,
/// so the detail block **does** rebuild per keystroke. It has to: the caret lives in the rebuilt
/// tree, unlike the plain `DivReadout`.
///
/// What it is actually for is separation of concerns — which cell has the author's attention is not a
/// property of the import scan, and folding it into `ImportState` would make every cell click look
/// like a scan result changing.
#[derive(Resource, Default)]
pub struct CellEdit {
    /// The selected cell, if any.
    pub at: Option<(u32, u32, u32)>,
    /// The piece a token field was opened against, so a click elsewhere cannot redirect the edit.
    /// See [`EditTarget`].
    pub target: Option<EditTarget>,
    /// The layer the grid is showing. A 3x3x3 lattice is 27 buttons at once, which is a wall — one
    /// y-slice at a time is nine, which is a shape you can read.
    pub layer: u32,
    /// The field taking keys, and what has been typed.
    active: Option<(CellField, String)>,
    /// **The verb a header applies.** The chips still act on the selected cell as they always did;
    /// this remembers which one was used last, so clicking a row, column or layer header repeats it
    /// over that whole set. A header with no verb behind it would have to invent one.
    pub verb: CellVerb,
    /// The cells a pending token will land on, when a header opened the field rather than a cell.
    /// `None` means the one selected cell.
    pending: Option<Vec<(u32, u32, u32)>>,
}

impl CellEdit {
    pub fn typing(&self) -> bool {
        self.active.is_some()
    }
}

/// The clickable description field.
#[derive(Component, Clone, Copy)]
pub struct NoteField;

/// **What is being typed into the description**, or `None`.
///
/// `Descriptor::note` already existed and nothing could write it, so every description in the shipped
/// libraries is whatever a generator put there. It is free text on purpose — the id says what a piece
/// *is* and the tags say what it *offers*, and neither can carry "the one with the cracked screen".
#[derive(Resource, Default)]
pub struct NoteEdit {
    /// The piece this description was opened against, and the text so far. The target is captured at
    /// open for the reason [`EditTarget`] gives.
    active: Option<(EditTarget, String)>,
}

impl NoteEdit {
    pub fn typing(&self) -> bool {
        self.active.is_some()
    }
}

fn on_note_click(
    activate: On<Activate>,
    fields: Query<&NoteField>,
    project: Res<Project>,
    mut edit: ResMut<NoteEdit>,
    mut state: ResMut<ImportState>,
) {
    if fields.get(activate.entity).is_err() {
        return;
    }
    // Seeded with what is there, unlike the id and the tokens: a description is *edited*, not
    // replaced, and retyping a sentence to change one word is not an interaction.
    // From `measured`, which is what a description is written back to — reading the layered library
    // here would seed the field with a note a policy patch supplied and then write it down a level.
    let now = state
        .editing(&project.measured)
        .and_then(|d| d.note.clone())
        .unwrap_or_default();
    let Some(target) = state.target() else {
        return;
    };
    edit.active = Some((target, now));
    state.status = "describe it — Enter to keep it, Esc to leave it".to_owned();
}

/// Typing a description. Free text, so unlike an id nothing is forced as you type.
fn note_keys(
    mut events: MessageReader<KeyboardInput>,
    mut edit: ResMut<NoteEdit>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    for event in events.read() {
        // Drained even while shut, so the key that opens this field cannot be typed into it — see
        // `cell_keys`.
        if edit.active.is_none() || !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let Some((target, raw)) = edit.active.take() else { return };
                let text = raw.trim().to_owned();
                // The piece this field was opened on, not whatever has the focus now.
                let Some((d, where_to)) = state.at_target(&target, &mut project.measured) else {
                    state.status = "the description was not kept — that tile is gone".to_owned();
                    return;
                };
                // Empty clears, the same rule the edge and anchor tokens follow — one keystroke path
                // for setting and unsetting rather than a second control for "remove".
                d.note = (!text.is_empty()).then(|| text.clone());
                let said = if text.is_empty() {
                    "description cleared".to_owned()
                } else {
                    format!("described: {text}")
                };
                state.status = persist(&mut project, where_to, said);
            }
            Key::Escape => {
                edit.active = None;
                state.status = "description unchanged".to_owned();
            }
            Key::Backspace => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    raw.pop();
                }
            }
            Key::Space => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    raw.push(' ');
                }
            }
            Key::Character(ch) => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    raw.push_str(ch);
                }
            }
            _ => {}
        }
    }
}

/// **The width a piece should stand at, in metres.** Clicking [`ScaleField`] opens it.
///
/// The field asks for a *size*, not a multiplier, because that is what [`Align::scale`] already says
/// it is: *"Uniform art correction: real-world size ÷ authored size"*, and *"Every one is measured,
/// never dialled by eye"*. An author holding a tape measure knows the shelf is 0.6 m; asking them to
/// divide that by whatever the artist exported is asking them to do the arithmetic this field exists
/// to do. It is also the unit the Map tab's `MAP SIZE (m)` field takes, so both tabs ask for metres.
///
/// Width alone determines it — scale is one number by construction, so depth and height follow.
///
/// [`Align::scale`]: emerge_core::descriptor::Align::scale
#[derive(Resource, Default)]
pub struct ScaleEdit {
    /// The piece this was opened against, and the digits so far. The target is captured at open for
    /// the reason [`EditTarget`] gives.
    active: Option<(EditTarget, String)>,
}

impl ScaleEdit {
    pub fn typing(&self) -> bool {
        self.active.is_some()
    }
}

/// The clickable width field.
#[derive(Component)]
pub struct ScaleField;

/// The text inside it.
#[derive(Component)]
pub struct ScaleReadout;

/// **How far from 1.0 counts as a scale.** Below this the field stores `None` instead.
///
/// A stored `Some(1.0)` is an identity that every reader would then multiply by, and it would show up
/// in `library.ron` as an authored fact where there is none — the same rule `rotate_mesh` follows when
/// it writes `(want != (0,0,0)).then_some(want)`. The bound is a tenth of a millimetre on a 1 m piece,
/// which is finer than anything the importer can measure.
const SCALE_EPS: f32 = 1e-4;

/// **The width an author typed, as an `align.scale`** — or a sentence saying why it is not one.
///
/// `measured_w` is the piece's own `extent.footprint.0`, *not* its placed width. Dividing by the
/// placed width would compound: typing back the number already on screen would scale the piece a
/// second time, and doing that twice would halve a 0.5x piece to 0.25x with the field reading right
/// both times. `a_width_that_is_already_set_is_a_no_op` pins that.
///
/// Pure, and split out of [`scale_keys`] for the reason the rest of this crate splits its arithmetic
/// out of its systems: the rule is the whole content of the feature, and proving it through an `App`
/// means driving a text field with a synthetic keyboard.
fn scale_for_width(measured_w: f32, want: f32) -> Result<Option<f32>, String> {
    if !want.is_finite() || want <= 0.0 {
        return Err(format!("a piece cannot stand {want} m wide"));
    }
    if !measured_w.is_finite() || measured_w <= 0.0 {
        return Err(format!(
            "this mesh measures {measured_w} m wide, so no scale reaches {want} m"
        ));
    }
    let scale = want / measured_w;
    // Never an identity — see [`SCALE_EPS`].
    Ok(((scale - 1.0).abs() > SCALE_EPS).then_some(scale))
}

fn on_scale_click(
    activate: On<Activate>,
    fields: Query<&ScaleField>,
    mut edit: ResMut<ScaleEdit>,
    mut state: ResMut<ImportState>,
) {
    if fields.get(activate.entity).is_err() {
        return;
    }
    let Some(target) = state.target() else {
        return;
    };
    // **Starts empty**, the same call `on_size_field_click` makes and for the same measured reason:
    // seeding it with the current number meant the first digit appended to it, and it looked like it
    // had worked. The value being replaced stays on screen until Enter.
    edit.active = Some((target, String::new()));
    state.status =
        "width: type the metres this piece should stand at, Enter to keep it, Esc to leave it alone"
            .to_owned();
}

/// Digits and a single point, filtered at the keystroke.
///
/// `size_edit_keys` states the rule this follows: a field that accepts a character and then refuses
/// the answer has taught the author it was allowed. This one never shows one that cannot be part of a
/// width.
fn scale_keys(
    mut events: MessageReader<KeyboardInput>,
    mut edit: ResMut<ScaleEdit>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    for event in events.read() {
        // Drained even while shut, so the click that opens this field cannot be typed into it — see
        // `cell_keys`.
        if edit.active.is_none() || !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let Some((target, raw)) = edit.active.take() else {
                    return;
                };
                let text = raw.trim().to_owned();
                if text.is_empty() {
                    state.status = "width unchanged — nothing typed".to_owned();
                    return;
                }
                let Ok(want) = text.parse::<f32>() else {
                    state.status = format!("`{text}` is not a number of metres");
                    return;
                };
                // Everything the commit needs, read before the borrow ends — `state.status` below
                // wants `state` back.
                let found = state
                    .at_target(&target, &mut project.measured)
                    .map(|(d, where_to)| (d.id.clone(), d.extent.footprint, where_to));
                let Some((id, footprint, where_to)) = found else {
                    state.status = "the width was not kept — that tile is gone".to_owned();
                    return;
                };
                // **Divided by the MEASURED width**, which is what `align.scale` is a ratio against.
                // Dividing by the placed width instead would compound: typing the number already on
                // screen would scale the piece a second time.
                let Some((mw, md)) = footprint else {
                    state.status = format!(
                        "`{id}` has no measured footprint, so a width cannot be turned into a scale"
                    );
                    return;
                };
                // Refused rather than clamped. A width of zero is a piece that reserves nothing and
                // sits inside a wall with every rule reporting success — the refusal
                // `Descriptor::resolve` already makes about the scale this would produce, made here
                // where the author can still see what they typed.
                let scale = match scale_for_width(mw, want) {
                    Ok(s) => s,
                    Err(e) => {
                        state.status = format!("`{id}`: {e}");
                        return;
                    }
                };
                let Some((d, _)) = state.at_target(&target, &mut project.measured) else {
                    state.status = "the width was not kept — that tile is gone".to_owned();
                    return;
                };
                d.align.scale = scale;
                let factor = scale.unwrap_or(1.0);
                let said = format!(
                    "{id} — {:.2} x {:.2} m at {factor:.3}x",
                    mw * factor,
                    md * factor
                );
                state.status = persist(&mut project, where_to, said);
            }
            Key::Escape => {
                edit.active = None;
                state.status = "width unchanged".to_owned();
            }
            Key::Backspace => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    raw.pop();
                }
            }
            Key::Character(ch) => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    for c in ch.chars() {
                        // One point, and never as the first character — `.5` parses, but a field that
                        // shows a leading point invites `..5`, which does not.
                        let ok = c.is_ascii_digit() || (c == '.' && !raw.contains('.') && !raw.is_empty());
                        if ok && raw.len() < 6 {
                            raw.push(c);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn on_cell_click(
    activate: On<Activate>,
    cells: Query<(&CellButton, &CellLayer)>,
    mut edit: ResMut<CellEdit>,
) {
    let Ok((b, layer)) = cells.get(activate.entity) else {
        return;
    };
    // The button carries its own layer now that all of them are on screen at once.
    let at = (b.0, layer.0, b.1);
    edit.layer = layer.0;
    edit.at = Some(at);
    edit.active = None;
    // **No status line for a selection.** The line under the grid already says what the selected cell
    // holds, and `refresh_cells` repaints it in place — whereas writing `status` mutates
    // `ImportState`, which is what `rebuild_detail` watches, so saying it twice is what made picking a
    // cell respawn the whole detail block. One fact, one place, no bounce.
}

/// What a layer is called. Bottom, top, and the numbers in between — a three-layer lattice reads as
/// words, and a nine-layer one has to read as numbers.
fn layer_label(y: u32, dy: u32) -> String {
    match (y, dy) {
        (_, 1) => "only layer".to_owned(),
        (0, _) => "bottom".to_owned(),
        (y, dy) if y + 1 == dy => "top".to_owned(),
        (1, 3) => "middle".to_owned(),
        (y, _) => format!("y {y}"),
    }
}

/// One header button. `*` fills the layer, `v` a column, `>` a row — ASCII only, per `docs/ui.md` §5.
fn header_button(row: &mut ChildSpawnerCommands, header: FillHeader, glyph: &str) {
    row.spawn((
        UiButton,
        Hovered::default(),
        header,
        Node {
            min_width: Val::Px(20.0),
            min_height: Val::Px(18.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(HEADER_BG),
    ))
    .with_children(|b| {
        b.spawn((
            Text::new(glyph.to_owned()),
            TextColor(LABEL),
            TextFont::from_font_size(9.0),
        ));
    });
}

/// One cell, in words. Used for the status line and the button glyph's tooltip role.
fn describe_cell(c: &emerge_core::descriptor::SubCell) -> String {
    let mut parts: Vec<String> = Vec::new();
    if c.solid {
        parts.push("solid".to_owned());
    }
    if let Some(e) = &c.edge {
        parts.push(format!("edge `{e}`"));
    }
    if let Some(a) = &c.anchor {
        parts.push(format!("anchor `{a}`"));
    }
    if parts.is_empty() {
        "open".to_owned()
    } else {
        parts.join(", ")
    }
}

/// The glyph a cell button shows. **ASCII only** — `docs/ui.md` §5 records that the shipped face is
/// checked per glyph and that several obvious box-drawing choices are simply absent from it.
fn cell_glyph(c: Option<&emerge_core::descriptor::SubCell>) -> &'static str {
    match c {
        None => ".",
        Some(c) if c.solid && (c.edge.is_some() || c.anchor.is_some()) => "%",
        Some(c) if c.solid => "#",
        Some(c) if c.edge.is_some() => "E",
        Some(c) if c.anchor.is_some() => "A",
        Some(_) => ".",
    }
}

fn on_cell_verb(
    activate: On<Activate>,
    verbs: Query<&CellVerb>,
    mut edit: ResMut<CellEdit>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    let Ok(verb) = verbs.get(activate.entity) else {
        return;
    };
    apply_verb(*verb, &mut edit, &mut project, &mut state);
}

/// **Do one of the four things to the selected cell.** The chip and the key both come here.
///
/// Split out of the observer so the keyboard is not a second implementation of the same four verbs —
/// `docs/ui.md` §4.2 requires everything reachable by mouse to be reachable by keyboard, and the way
/// that requirement usually gets met is by writing it twice and letting the two drift.
fn apply_verb(
    verb: CellVerb,
    edit: &mut CellEdit,
    project: &mut Project,
    state: &mut ImportState,
) {
    let Some(at) = edit.at else {
        state.status = "pick a cell first".to_owned();
        return;
    };
    apply_verb_to(verb, &[at], edit, project, state);
}

/// The button that reads occupancy off the mesh. See [`scan_mesh`].
#[derive(Component, Clone, Copy)]
pub struct ScanMeshButton;

/// Which axis a rotate chip turns about.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum RotateAxis {
    X,
    Y,
    Z,
}

impl RotateAxis {
    fn label(self) -> &'static str {
        match self {
            RotateAxis::X => "rot x",
            RotateAxis::Y => "rot y",
            RotateAxis::Z => "rot z",
        }
    }
    /// This axis's component of a rotation, bumped by a quarter turn.
    fn bumped(self, r: (i32, i32, i32)) -> (i32, i32, i32) {
        let step = |v: i32| (v + 90).rem_euclid(360);
        match self {
            RotateAxis::X => (step(r.0), r.1, r.2),
            RotateAxis::Y => (r.0, step(r.1), r.2),
            RotateAxis::Z => (r.0, r.1, step(r.2)),
        }
    }
}

/// **Turn the mesh a quarter turn about one axis, and re-measure it.**
///
/// The two halves are one action on purpose. `align.rotate` is a render instruction and the `extent`
/// beside it is stored already-rotated, so a rotation that did not re-measure would leave the file
/// describing an orientation the mesh no longer has — with nothing downstream able to notice. That
/// invariant is the price of not making every reader of `extent` rotation-aware, and this is where
/// it is paid.
///
/// A turn about **X or Z swaps the piece's height with a floor axis**, so this is also the one place
/// a tile's lattice changes shape without the project's `divisions` moving.
///
/// # The authored lattice comes with it, or the turn is refused
///
/// This re-measured the extent and left the cells where they were, which is not a smaller version of
/// the right answer — it is a corrupt file. `site/wall`'s ten cells are authored at `1x5x2`; a turn
/// about Y derives `2x5x1`, the five cells at `z = 1` fall outside it, and `write_library` then refuses
/// **every** subsequent edit in the session with `cell (0,0,1) is outside its 2x5x1 lattice`.
///
/// A turn about **Y** has an exact mapping and takes it: `Subgrid::rotated` and
/// `descriptor::rotate_div` are the pair that does this everywhere else in the program, and
/// `adjacency::faults` already uses them together for a placement's yaw.
///
/// A turn about **X or Z** has none — the lattice's Y axis becomes a floor axis, and cells authored
/// against a height do not describe a width. So a piece with authored cells refuses, names the count,
/// and says what `force` would do. `force` then clears them and reports how many, which is the
/// author's call to make explicitly rather than something to do to them quietly.
fn rotate_mesh(axis: RotateAxis, force: bool, project: &mut Project, state: &mut ImportState) {
    let Some(d) = state.editing(&project.measured) else {
        state.status = "no tile is selected".to_owned();
        return;
    };
    let Some(mesh) = d.mesh.clone() else {
        state.status = format!("`{}` has no mesh to turn", d.id);
        return;
    };
    let authored_cells = d.subgrid.as_ref().map_or(0, |g| g.cells.len());
    if axis != RotateAxis::Y && authored_cells > 0 && !force {
        state.status = format!(
            "`{}` has {authored_cells} authored cell(s). A {} turn swaps its height with a floor \
             axis and there is no mapping for that — hold Shift to turn it anyway and clear them.",
            d.id,
            axis.label()
        );
        return;
    }
    // **Before the extent moves.** A Y turn maps the cells onto the turned piece, and the mapping
    // reads the divisions they were authored against.
    let div_before = focused_div(state, project).ok();
    let path = project.root.join("assets").join(&mesh);
    let glb = match emerge_core::glb::Glb::open(&path) {
        Ok(glb) => glb,
        Err(why) => {
            state.status = format!("{mesh}: {why}");
            return;
        }
    };

    let Some((d, where_to)) = state.editing_mut(&mut project.measured) else {
        return;
    };
    let want = axis.bumped(d.align.rotate.unwrap_or((0, 0, 0)));
    let before = d.align.rotate;
    // A rotation of nothing is not a rotation — keep the field absent rather than storing an
    // identity nobody authored, so a descriptor that was never turned still says so.
    d.align.rotate = (want != (0, 0, 0)).then_some(want);
    if let Err(why) = emerge_core::import::remeasure_rotated(d, &glb, before) {
        d.align.rotate = before;
        state.status = why;
        return;
    }
    // The lattice, moved or dropped. Both halves of a Y turn together — the cells and the divisions —
    // because a lattice read off one without the other is the wrong shape; `rotate_div` is not called
    // here only because divisions are derived from the extent that just changed.
    let lattice = match (axis, div_before) {
        (RotateAxis::Y, Some(div)) => {
            d.subgrid = d.subgrid.take().map(|g| g.rotated(1, div));
            String::new()
        }
        // A Y turn on a piece whose divisions would not derive has no cells worth moving either:
        // `divisions` refuses only for a missing footprint, and `Subgrid::validate` would refuse such
        // a lattice at the door.
        (RotateAxis::Y, None) => String::new(),
        _ if authored_cells > 0 => {
            d.subgrid = None;
            format!(" — {authored_cells} authored cell(s) cleared")
        }
        _ => String::new(),
    };
    let (w, dep) = d.extent.footprint.unwrap_or((0.0, 0.0));
    let h = d.extent.height.unwrap_or(0.0);
    let said = format!(
        "{} {},{},{} deg — now {w:.2} x {h:.2} x {dep:.2} m{lattice}",
        d.id, want.0, want.1, want.2
    );
    state.status = persist(project, where_to, said);
}

/// The chip.
fn on_rotate_click(
    activate: On<Activate>,
    axes: Query<&RotateAxis>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    let Ok(axis) = axes.get(activate.entity) else {
        return;
    };
    rotate_mesh(*axis, held_shift(&keyboard), &mut project, &mut state);
}

/// **Shift, held, meaning "I know — do it anyway".**
///
/// Read at the moment the turn is asked for rather than kept as a mode, so there is no state that can
/// be left on. Both entry points go through this, so the chip and the key cannot disagree about what
/// forcing means. `keys::fires_in` is not involved: this is a modifier on an action, not an action.
fn held_shift(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
}

/// **Mark every cell the mesh's geometry reaches, and say how many.**
///
/// The lattice is only worth authoring if occupancy comes off the mesh: at the shipped divisions a
/// 3 m wall is 30 cells, and marking those by hand is not something anyone does twice.
///
/// `emerge_core::import::occupancy` explains the method and why it is vertices rather than a
/// bounding box. Two rules live here rather than there:
///
/// * **`edge` and `anchor` survive.** Occupancy is a measurement; the tokens are an author's
///   judgement, and a scan that erased them would make the button unusable on a tuned piece.
/// * **The count is stated.** Vertex occupancy under-marks a large flat face with no vertex inside a
///   cell — the middle of a tabletop — and "27 of 30 cells" is what tells an author to look.
fn scan_mesh(project: &mut Project, state: &mut ImportState) {
    let div = match focused_div(state, project) {
        Ok(div) => div,
        Err(why) => {
            state.status = why;
            return;
        }
    };
    let Some(d) = state.editing(&project.measured) else {
        return;
    };
    let Some(mesh) = d.mesh.clone() else {
        state.status = format!("`{}` has no mesh to scan", d.id);
        return;
    };
    // **The rotation the divisions were derived with.** `div` comes from the piece's `extent`, which
    // `import::remeasure_rotated` already baked `align.rotate` into — so the rasteriser has to read
    // the mesh in that same frame or it maps these divisions onto the mesh's raw axes and returns a
    // transposed lattice. An off-square `rotate` cannot be baked at all, and that refusal is the
    // author's to see rather than something to round away.
    let rotate = match d.align.rotate {
        Some(r) => match emerge_core::descriptor::quarter_turns_xyz(r, &d.id) {
            Ok(q) => q,
            Err(why) => {
                state.status = why;
                return;
            }
        },
        None => (0, 0, 0),
    };

    let path = project.root.join("assets").join(&mesh);
    let cells = match emerge_core::glb::Glb::open(&path)
        .and_then(|glb| emerge_core::import::occupancy(&glb, div, rotate))
    {
        Ok(cells) => cells,
        Err(why) => {
            state.status = format!("{mesh}: {why}");
            return;
        }
    };

    let Some((d, where_to)) = state.editing_mut(&mut project.measured) else {
        return;
    };
    let grid = d.lattice_mut();
    for &at in &cells {
        grid.set_solid(at, div);
    }
    d.settle_lattice();
    let total = emerge_core::descriptor::Subgrid::volume(div);
    let said = format!(
        "scanned {mesh}: {} of {total} cells solid",
        cells.len()
    );
    state.status = persist(project, where_to, said);
}

/// **Scan a candidate the moment it is selected**, so the common case needs no keypress at all.
///
/// Watches the selection rather than hooking the two places that change it (`on_candidate_click` and
/// `move_selection`) — one rule in one place cannot drift from itself, and a third way to select a
/// tile would get this for free.
///
/// # Candidates only, and only when there is nothing to lose
///
/// A **library entry** is never scanned automatically. Its edits go through `persist`, so a scan on
/// selection would rewrite `library.ron` every time an author clicked a row — browsing the 45-piece
/// kit would be 45 writes — and it would mark cells someone had deliberately left open. That is what
/// the `rescan mesh` chip is for: the same act, asked for.
///
/// A candidate is safe on both counts: nothing reaches disk until Accept. The lattice still has to be
/// empty of solids, so moving away from a candidate and back does not re-mark cells the author
/// unmarked in between.
fn autoscan_candidate(
    mut last: Local<Option<usize>>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    if state.selected_library_id.is_some() {
        // The library list has the focus. Forget where the candidate cursor was, so returning to it
        // is a fresh selection and scans again if it still has nothing.
        *last = None;
        return;
    }
    if *last == Some(state.selected) {
        return;
    }
    *last = Some(state.selected);

    let already_marked = state
        .editing(&project.measured)
        .and_then(|d| d.subgrid.as_ref())
        .is_some_and(|g| g.cells.iter().any(|c| c.solid));
    if already_marked {
        return;
    }
    scan_mesh(&mut project, &mut state);
}

/// The chip.
fn on_scan_mesh(
    activate: On<Activate>,
    buttons: Query<&ScanMeshButton>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    scan_mesh(&mut project, &mut state);
}

/// **The lattice the focused piece stands on**, or the sentence explaining why it has none.
///
/// Every reader and every writer of the lattice goes through this, so the grid an author clicks, the
/// grid the gizmos draw and the grid a write is range-checked against are one grid. Taken by shared
/// reference so a caller can derive the divisions *before* borrowing the descriptor mutably — which
/// is the order every editing path needs.
fn focused_div(state: &ImportState, project: &Project) -> Result<(u32, u32, u32), String> {
    // The piece as PLACED — see [`ImportState::placed`]. A lattice is a division of the thing that
    // ends up in the world, and the range check a write goes through has to be the same one
    // `Library::validate_lattices` will apply to it afterwards.
    let Some(d) = state.placed(project) else {
        return Err("no tile is selected".to_owned());
    };
    project.divisions_of(d)
}

/// **Do one verb to a set of cells.** One cell from a chip, a row, a column or a whole layer from a
/// header — the same code either way, so a header cannot drift from what the chip does.
fn apply_verb_to(
    verb: CellVerb,
    cells: &[(u32, u32, u32)],
    edit: &mut CellEdit,
    project: &mut Project,
    state: &mut ImportState,
) {
    edit.verb = verb;
    let Some(&first) = cells.first() else { return };
    let many = cells.len() > 1;
    // Derived before the mutable borrow, because a write is range-checked against it and the
    // descriptor it comes from is the one about to be edited.
    let div = match focused_div(state, project) {
        Ok(div) => div,
        Err(why) => {
            state.status = why;
            return;
        }
    };
    match verb {
        CellVerb::Solid => {
            let Some((d, where_to)) = state.editing_mut(&mut project.measured) else {
                return;
            };
            // **A span is set, not toggled.** Toggling many cells at once flips a mixed row into its
            // photographic negative, which is never what a click on a header meant; one cell keeps
            // the toggle, because there the before and after are both visible.
            let said = if many {
                for &at in cells {
                    d.lattice_mut().set_solid(at, div);
                }
                format!("{} cells solid", cells.len())
            } else {
                match d.lattice_mut().toggle_solid(first, div) {
                    Some(true) => format!("cell {},{},{} is solid", first.0, first.1, first.2),
                    Some(false) => format!("cell {},{},{} is open", first.0, first.1, first.2),
                    None => "that cell is outside the lattice".to_owned(),
                }
            };
            d.settle_lattice();
            state.status = persist(project, where_to, said);
        }
        CellVerb::Clear => {
            let Some((d, where_to)) = state.editing_mut(&mut project.measured) else {
                return;
            };
            if let Some(grid) = d.subgrid.as_mut() {
                for &at in cells {
                    grid.clear(at);
                }
            }
            d.settle_lattice();
            let said = if many {
                format!("{} cells cleared", cells.len())
            } else {
                format!("cell {},{},{} cleared", first.0, first.1, first.2)
            };
            state.status = persist(project, where_to, said);
        }
        CellVerb::Edge | CellVerb::Anchor => {
            let field = if verb == CellVerb::Edge {
                CellField::Edge
            } else {
                CellField::Anchor
            };
            // Starts empty, and Enter on an empty field CLEARS the token — one keystroke path for
            // setting and unsetting, rather than a second control for "remove".
            edit.active = Some((field, String::new()));
            edit.target = state.target();
            // **The target is captured here, not read at commit.** One cell or a whole span, the
            // same way: the cells this field was opened against are the cells it writes to. Reading
            // the live selection at `Enter` meant a click that moved the cursor mid-word redirected
            // the edit — or, once that was noticed, refused it and threw away what had been typed.
            // Neither is what the author asked for; the cell they opened the field on is.
            edit.pending = Some(cells.to_vec());
            state.status = if many {
                format!(
                    "type a {} token for {} cells, Enter to keep it (empty clears), Esc to leave it",
                    verb.label(),
                    cells.len()
                )
            } else {
                format!(
                    "type a {} token, Enter to keep it (empty clears), Esc to leave it",
                    verb.label()
                )
            };
        }
    }
}

/// **The lattice cell under the cursor, and the face it is seen through.**
///
/// The lattice was reachable only through the panel's chip grid and the cursor keys. That grid is a
/// flat slice of a 3D thing: to put a token on a wall's east face an author had to work out which
/// column that was, in a projection where east is a diagonal. Pointing at the wall is the obvious
/// gesture and it was the one thing the editor could not do.
///
/// `None` when the cursor is off the piece, over a panel, or on a tab that is not showing one.
#[derive(Resource, Default)]
pub struct LatticePick(pub Option<((u32, u32, u32), Option<emerge_core::descriptor::Face>)>);

/// The staged tile's lattice box in world space, or `None` if it has no derivable one.
///
/// The one place this geometry is written down, because `draw_subgrid`, the highlight and the picker
/// must agree about where the lattice is to within nothing at all — a highlight half a cell off the
/// box it highlights is worse than no highlight.
fn stage_box(state: &ImportState, project: &Project) -> Option<(Vec3, Vec3, (u32, u32, u32))> {
    // As placed: the box is the piece a click has to land on, and a stretched wall stands 2.40 m
    // whatever its measurement says.
    let d = state.placed(project)?;
    let div = project.divisions_of(d).ok()?;
    // **Through the same two helpers `divisions` uses.** `div` above says how many cells there are and
    // this says how big the box is; a click is turned into a cell by dividing one by the other, so
    // reading the raw extent here put the cell boundaries somewhere the lattice does not have any —
    // the author clicks one cell and writes another.
    let (w, dep) = emerge_core::descriptor::placed_footprint(d)?;
    // The same floor `draw_subgrid` gives a flat piece, so a decal can still be picked.
    let h = emerge_core::descriptor::placed_height(d).unwrap_or(0.0).max(0.05);
    Some((
        STAGE - Vec3::new(w * 0.5, 0.0, dep * 0.5),
        Vec3::new(w, h, dep),
        div,
    ))
}

fn pick_lattice(
    mode: Res<Mode>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<crate::view::MainCamera>>>,
    hovered_ui: Query<&Hovered>,
    state: Res<ImportState>,
    project: Res<Project>,
    mut pick: ResMut<LatticePick>,
) {
    let clear = |pick: &mut LatticePick| {
        if pick.0.is_some() {
            pick.0 = None;
        }
    };
    if *mode != Mode::Tiles || hovered_ui.iter().any(|h| h.0) {
        clear(&mut pick);
        return;
    }
    let (Some(window), Some(camera)) = (window, camera) else {
        clear(&mut pick);
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(cursor) = window.cursor_position() else {
        clear(&mut pick);
        return;
    };
    let Ok(ray) = cam.viewport_to_world(cam_tf, cursor) else {
        clear(&mut pick);
        return;
    };
    let Some((origin, size, div)) = stage_box(&state, &project) else {
        clear(&mut pick);
        return;
    };
    let got = emerge_core::descriptor::pick_cell(
        ray.origin.into(),
        Vec3::from(ray.direction).into(),
        origin.into(),
        size.into(),
        div,
    );
    if pick.0 != got {
        pick.0 = got;
    }
}

/// Clicking the piece selects the cell the cursor is on.
///
/// Sets the same `CellEdit` the chips and the cursor keys set, so every verb, every fill and the
/// readout under the grid work on a ray-picked cell without knowing one exists.
fn click_lattice(
    mouse: Res<ButtonInput<MouseButton>>,
    pick: Res<LatticePick>,
    mut edit: ResMut<CellEdit>,
    mut state: ResMut<ImportState>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some((at, face)) = pick.0 else {
        return;
    };
    edit.at = Some(at);
    edit.layer = at.1;
    // The face is named because it is the useful half: it says which neighbour will read whatever
    // token goes here. `SubCell::edge` is one token per cell, so this picks the cell — a corner cell
    // sits on two faces and presents the same token to both, which is the schema's own design.
    state.status = match face {
        Some(f) => format!(
            "cell {},{},{} — {} face, the side a neighbour reads",
            at.0,
            at.1,
            at.2,
            f.label()
        ),
        None => format!("cell {},{},{} — picked from above", at.0, at.1, at.2),
    };
}

/// Outline the cell under the cursor, so pointing at the piece has an answer before clicking.
fn draw_pick(
    pick: Res<LatticePick>,
    state: Res<ImportState>,
    project: Res<Project>,
    mut gizmos: Gizmos,
) {
    let Some((at, _)) = pick.0 else { return };
    let Some((origin, size, div)) = stage_box(&state, &project) else {
        return;
    };
    let step = Vec3::new(
        size.x / div.0 as f32,
        size.y / div.1 as f32,
        size.z / div.2 as f32,
    );
    let centre = origin
        + Vec3::new(
            (at.0 as f32 + 0.5) * step.x,
            (at.1 as f32 + 0.5) * step.y,
            (at.2 as f32 + 0.5) * step.z,
        );
    gizmos.cube(Transform::from_translation(centre).with_scale(step), ACCENT);
}

/// **The lattice, by keyboard** — cursor, layer, and the four verbs.
///
/// `docs/ui.md` §4.2 wants everything reachable by mouse reachable by keyboard, and every subgrid
/// control was an `Activate` observer, so none of it was. The verbs go through `apply_verb`, the same
/// function the chips call, rather than being written a second time.
fn lattice_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut edit: ResMut<CellEdit>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    let Ok((dx, dy, dz)) = focused_div(&state, &project) else {
        return;
    };
    // `divisions` refuses a zero, but this is a cursor and `dy - 1` on one would wrap to u32::MAX.
    if dx == 0 || dy == 0 || dz == 0 {
        return;
    }

    let pressed = |a| keys::just_pressed(&keyboard, live.0, a);

    // Everything reachable by mouse is reachable by key (`docs/ui.md` §4.2), so the scan chip has
    // one too — and it goes through the same `scan_mesh` the chip calls, not a second copy.
    if pressed(Action::ScanMesh) {
        scan_mesh(&mut project, &mut state);
        return;
    }
    for (action, axis) in [
        (Action::RotateMeshX, RotateAxis::X),
        (Action::RotateMeshY, RotateAxis::Y),
        (Action::RotateMeshZ, RotateAxis::Z),
    ] {
        if pressed(action) {
            rotate_mesh(axis, held_shift(&keyboard), &mut project, &mut state);
            return;
        }
    }

    for (action, verb) in [
        (Action::CellSolid, CellVerb::Solid),
        (Action::CellEdge, CellVerb::Edge),
        (Action::CellAnchor, CellVerb::Anchor),
        (Action::CellClear, CellVerb::Clear),
    ] {
        if pressed(action) {
            apply_verb(verb, &mut edit, &mut project, &mut state);
            return;
        }
    }

    let mut layer = edit.layer.min(dy - 1);
    if pressed(Action::LayerDown) {
        layer = layer.saturating_sub(1);
    }
    if pressed(Action::LayerUp) {
        layer = (layer + 1).min(dy - 1);
    }
    let moved = pressed(Action::CellLeft)
        || pressed(Action::CellRight)
        || pressed(Action::CellForward)
        || pressed(Action::CellBack);
    if layer == edit.layer && !moved {
        return;
    }

    // **The cursor rides the layer rather than being dropped by it.** Both this and `on_layer_click`
    // keep (x, z) and move y, so walking up a lattice column stays on the column an author is
    // looking at instead of clearing the selection every step.
    let mut at = edit.at.unwrap_or((0, layer, 0));
    at.1 = layer;
    if pressed(Action::CellLeft) {
        at.0 = at.0.saturating_sub(1);
    }
    if pressed(Action::CellRight) {
        at.0 = (at.0 + 1).min(dx - 1);
    }
    if pressed(Action::CellForward) {
        at.2 = (at.2 + 1).min(dz - 1);
    }
    if pressed(Action::CellBack) {
        at.2 = at.2.saturating_sub(1);
    }
    at.0 = at.0.min(dx - 1);
    at.2 = at.2.min(dz - 1);

    edit.layer = layer;
    edit.at = Some(at);
    // Opening a token field and then moving the cursor would leave the buffer pointed at a cell it
    // was not typed for, so moving closes it — the same reason `on_cell_click` clears it.
    edit.active = None;
    // **No status line for a selection.** The line under the grid already says what the selected cell
    // holds, and `refresh_cells` repaints it in place — whereas writing `status` mutates
    // `ImportState`, which is what `rebuild_detail` watches, so saying it twice is what made picking a
    // cell respawn the whole detail block. One fact, one place, no bounce.
}

/// Typing an edge or anchor token.
fn cell_keys(
    mut events: MessageReader<KeyboardInput>,
    mut edit: ResMut<CellEdit>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    for event in events.read() {
        // Drained even while shut, so the cursor never lags: the key that OPENS this field is read
        // and discarded here rather than surviving to be typed into it next frame.
        if edit.active.is_none() || !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let Some((field, raw)) = edit.active.take() else {
                    return;
                };
                // The cells this field was opened against — see `apply_verb_to`. A field with no
                // target was never opened by a verb, so there is nothing it could mean; that is a
                // bug rather than an author error, and it says so instead of guessing at a cell.
                let Some(targets) = edit.pending.take() else {
                    state.status =
                        format!("`{raw}` was not kept — this field was opened without a cell.");
                    return;
                };
                let Some(target) = edit.target.take() else {
                    state.status =
                        format!("`{raw}` was not kept — this field was opened without a tile.");
                    return;
                };
                let token = emerge_core::naming::to_snake_case(&raw);
                // Before the mutable borrow: the write is range-checked against these. Derived from
                // **`target`**, the tile this field was opened on, and not from the live selection —
                // see `ImportState::placed_at_target` for what that cost.
                let div = match state
                    .placed_at_target(&target, &project)
                    .ok_or_else(|| format!("`{raw}` was not kept — that tile is gone."))
                    .and_then(|d| project.divisions_of(d))
                {
                    Ok(div) => div,
                    Err(why) => {
                        state.status = why;
                        return;
                    }
                };
                let Some((d, where_to)) = state.at_target(&target, &mut project.measured) else {
                    state.status = format!("`{raw}` was not kept — that tile is gone.");
                    return;
                };
                let mut wrote = 0usize;
                for &at in &targets {
                    let ok = match field {
                        CellField::Edge => d.lattice_mut().set_edge(at, div, &token),
                        CellField::Anchor => d.lattice_mut().set_anchor(at, div, &token),
                    };
                    if ok.is_some() {
                        wrote += 1;
                    }
                }
                d.settle_lattice();
                let said = match (wrote, token.is_empty(), targets.first()) {
                    (0, _, _) => "that cell is outside the lattice".to_owned(),
                    (1, true, Some(at)) => format!("cell {},{},{} token cleared", at.0, at.1, at.2),
                    (1, false, Some(at)) => format!("cell {},{},{} = `{token}`", at.0, at.1, at.2),
                    (n, true, _) => format!("{n} cells' tokens cleared"),
                    (n, false, _) => format!("{n} cells = `{token}`"),
                };
                // Nothing landed means nothing to write.
                state.status = if wrote == 0 {
                    said
                } else {
                    persist(&mut project, where_to, said)
                };
            }
            Key::Escape => {
                edit.active = None;
                edit.pending = None;
                state.status = "cell unchanged".to_owned();
            }
            Key::Backspace => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    raw.pop();
                }
            }
            Key::Space => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    raw.push(' ');
                }
            }
            Key::Character(ch) => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    raw.push_str(ch);
                }
            }
            _ => {}
        }
    }
}

/// Fill a row, a column or a whole layer with the verb the chips last used.
fn on_fill_header(
    activate: On<Activate>,
    headers: Query<&FillHeader>,
    mut edit: ResMut<CellEdit>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    let Ok(header) = headers.get(activate.entity) else {
        return;
    };
    let Ok((dx, _, dz)) = focused_div(&state, &project) else {
        return;
    };
    let y = header.layer;
    let cells: Vec<(u32, u32, u32)> = match header.span {
        Span::Layer => (0..dz).flat_map(|z| (0..dx).map(move |x| (x, y, z))).collect(),
        Span::Column(x) => (0..dz).map(|z| (x, y, z)).collect(),
        Span::Row(z) => (0..dx).map(|x| (x, y, z)).collect(),
    };
    // The selection follows the fill, so the readout under the grid describes something the author
    // just acted on rather than wherever the cursor happened to be.
    if let Some(&first) = cells.first() {
        edit.at = Some(first);
        edit.layer = y;
    }
    let verb = edit.verb;
    apply_verb_to(verb, &cells, &mut edit, &mut project, &mut state);
}

/// **Repaint the lattice controls in place**, rather than rebuilding the pane around them.
///
/// Picking a cell or a layer changes four things — which button is lit, which glyphs the slice shows,
/// the line under the grid, and the caret in a token — and none of them changes the *shape* of the
/// pane. `rebuild_detail` used to run on every `CellEdit` change, so a click despawned every node in
/// the detail block and respawned it a frame later: the visible bounce.
///
/// The structural rebuild stays for things that really do change the shape — a different tile, or a
/// different number of divisions — and both of those write `ImportState`, which is what it watches.
#[allow(clippy::type_complexity)]
fn refresh_cells(
    state: Res<ImportState>,
    project: Res<Project>,
    cell_edit: Res<CellEdit>,
    note_edit: Res<NoteEdit>,
    scale_edit: Res<ScaleEdit>,
    mut cells: Query<(&CellButton, &CellLayer, &mut BackgroundColor)>,
    mut glyphs: Query<(&CellGlyph, &CellLayer, &mut Text, &mut TextColor), (Without<SelectedCellLine>, Without<NoteReadout>, Without<ScaleReadout>)>,
    mut lines: Query<(&mut Text, &mut TextColor), (With<SelectedCellLine>, Without<CellGlyph>, Without<NoteReadout>, Without<ScaleReadout>)>,
    mut notes: Query<(&mut Text, &mut TextColor), (With<NoteReadout>, Without<CellGlyph>, Without<SelectedCellLine>, Without<ScaleReadout>)>,
    mut widths: Query<(&mut Text, &mut TextColor), (With<ScaleReadout>, Without<CellGlyph>, Without<SelectedCellLine>, Without<NoteReadout>)>,
) {
    // As placed, so the cells shown are the cells that exist. See [`ImportState::placed`].
    let Some(d) = state.placed(&project) else {
        return;
    };
    // A piece with no marked cells reads as an empty lattice rather than as a missing one — the
    // grid is still drawn, it just has nothing in it.
    let empty = emerge_core::descriptor::Subgrid::default();
    let grid = d.subgrid.as_ref().unwrap_or(&empty);

    for (button, layer, mut bg) in &mut cells {
        let selected = cell_edit.at == Some((button.0, layer.0, button.1));
        let want = if selected { ROW_SELECTED } else { ROW_BG };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    for (g, layer, mut text, mut colour) in &mut glyphs {
        let cell = grid.at((g.0, layer.0, g.1));
        let want = cell_glyph(cell);
        if text.0 != want {
            text.0 = want.to_owned();
        }
        let tint = if cell.is_some() { ACCENT } else { LABEL };
        if colour.0 != tint {
            colour.0 = tint;
        }
    }

    let detail = match cell_edit.at {
        Some(at) => match &cell_edit.active {
            Some((field, raw)) => format!(
                "{},{},{}  {} `{raw}_`",
                at.0,
                at.1,
                at.2,
                if *field == CellField::Edge { "edge" } else { "anchor" }
            ),
            None => format!(
                "{},{},{}  {}",
                at.0,
                at.1,
                at.2,
                grid.at(at).map(describe_cell).unwrap_or_else(|| "open".to_owned())
            ),
        },
        None => "no cell picked".to_owned(),
    };
    for (mut text, mut colour) in &mut lines {
        if text.0 != detail {
            text.0 = detail.clone();
        }
        let tint = if cell_edit.active.is_some() { ACCENT } else { DIM };
        if colour.0 != tint {
            colour.0 = tint;
        }
    }

    let (note_text, note_tint) = match &note_edit.active {
        Some((_, raw)) => (format!("{raw}_"), ACCENT),
        None => match d.note.as_deref() {
            Some(n) if !n.is_empty() => (n.to_owned(), TEXT),
            _ => ("describe it\u{2026}".to_owned(), LABEL),
        },
    };
    for (mut text, mut colour) in &mut notes {
        if text.0 != note_text {
            text.0 = note_text.clone();
        }
        if colour.0 != note_tint {
            colour.0 = note_tint;
        }
    }

    // **The width caret, repainted in place.** `rebuild_detail` only runs on
    // `resource_changed::<ImportState>`, so without this the digits would not appear until something
    // else touched the pane — the same reason the note and the cell tokens are refreshed here.
    //
    // Off the *measurement* layer, matching what the field writes and what `rebuild_detail` shows.
    let width_text = match &scale_edit.active {
        Some((_, raw)) => format!("{raw}_"),
        None => match state
            .editing(&project.measured)
            .and_then(emerge_core::descriptor::placed_footprint)
        {
            Some((w, _)) => format!("{w:.2}"),
            None => "--".to_owned(),
        },
    };
    let width_tint = if scale_edit.typing() { ACCENT } else { TEXT };
    for (mut text, mut colour) in &mut widths {
        if text.0 != width_text {
            text.0 = width_text.clone();
        }
        if colour.0 != width_tint {
            colour.0 = width_tint;
        }
    }
}

/// Candidates grouped by the directory they came from, in scan order.
///
/// Scan order is sorted path order, so the groups come out stable across machines and never reorder
/// — the same rule the palette follows and for the same reason (Samp 2011, via `docs/ui.md` §3.5).
fn packs(candidates: &[Candidate]) -> Vec<(String, Vec<usize>)> {
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    for (ix, c) in candidates.iter().enumerate() {
        let dir = c.mesh.rsplit_once('/').map_or(".", |(d, _)| d).to_owned();
        match out.iter_mut().find(|(name, _)| *name == dir) {
            Some((_, members)) => members.push(ix),
            None => out.push((dir, vec![ix])),
        }
    }
    out
}

/// The file name out of a path.
fn leaf(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

impl ImportState {
    pub fn current(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected)
    }

    /// **The descriptor the detail pane shows, and the one every edit lands on.**
    ///
    /// A library entry when one is selected, otherwise the selected candidate. Before this existed
    /// the pane returned early unless a *candidate* was selected, so an accepted tile's lattice was
    /// reachable only by hand-editing `library.ron` — which is why all 42 shipped descriptors still
    /// have `cells: []`. Authoring a lattice was, in practice, impossible through the tool that has
    /// the lattice editor in it.
    ///
    /// **One discriminant, not two selections.** `selected_library_id.is_some()` *is* "the library
    /// list has focus"; picking a candidate clears it. Two lists that could each claim the pane is
    /// how an edit lands on the piece the author is not looking at.
    ///
    /// **Pass `Project::measured`, not `Project::library`.** The pane edits the measurements file,
    /// so it must show the measurements: displaying a wall stretched to this facility's 2.40 m while
    /// writing the mesh's authored height back is the "preview that lies" this crate keeps being
    /// written to avoid.
    pub fn editing<'a>(&'a self, library: &'a emerge_core::library::Library) -> Option<&'a Descriptor> {
        match &self.selected_library_id {
            Some(id) => library.get(id),
            None => self.current().map(|c| &c.proposed),
        }
    }

    /// **The focused piece as it will stand in the world** — for anything that reads its *shape*.
    ///
    /// [`Self::editing`] takes whichever library it is handed, and which one that is turns out to be
    /// a real decision rather than a detail. `Project::measured` is what an edit is written *to*.
    /// The layered library is what the piece *is*: under `--kit site_greybox`, `site/wall` is measured
    /// at 1.0 m and the kit's `project.ron` stretches it to 2.40 — so the measurement derives a
    /// 2-row lattice and the piece that gets placed has 5.
    ///
    /// Every reader of the lattice's *shape* was asking `measured`, so an author authored on a grid
    /// nothing else in the program used: `validate_lattices`, `adjacency::faults` and the game all
    /// read the layered one. A token put on what the editor drew as the top row landed on row 1 of 5,
    /// and the three rows above it could not be reached at all — not refused, just absent. In the
    /// other direction (`wall_header`, patched shorter) every edit failed the range check instead.
    ///
    /// A **candidate** has no layered entry — it is not in the project yet — so this returns its
    /// proposal, which is the whole truth about it. That comes out of `selected_library_id` rather
    /// than from looking its id up, deliberately: an author who types a name that is already taken
    /// gets told so by `commit_candidate`, and until then their candidate must not quietly start
    /// reporting the shape of the piece that already owns the name.
    pub fn placed<'a>(&'a self, project: &'a crate::project::Project) -> Option<&'a Descriptor> {
        self.editing(&project.library)
    }

    /// The mutable half of [`Self::editing`]. `Ok(true)` from a caller's point of view means "this
    /// edit landed on a library entry", which is the caller's cue to write the file — a candidate's
    /// edits live in memory until Accept, a library entry's are the file.
    pub fn editing_mut<'a>(
        &'a mut self,
        library: &'a mut emerge_core::library::Library,
    ) -> Option<(&'a mut Descriptor, Persist)> {
        match &self.selected_library_id {
            Some(id) => library
                .descriptors
                .iter_mut()
                .find(|d| &d.id == id)
                .map(|d| (d, Persist::Library)),
            None => self
                .candidates
                .get_mut(self.selected)
                .map(|c| (&mut c.proposed, Persist::InMemory)),
        }
    }
}

impl ImportState {
    /// The piece the pane is focused on, as something an edit can hold on to. See [`EditTarget`].
    pub fn target(&self) -> Option<EditTarget> {
        match &self.selected_library_id {
            Some(id) => Some(EditTarget::Library(id.clone())),
            None => self.current().map(|c| EditTarget::Candidate(c.mesh.clone())),
        }
    }

    /// The descriptor a captured target names, wherever the focus has since moved to.
    ///
    /// `None` when the target is gone — a library entry removed, or a candidate dropped by a rescan.
    /// That is a real thing to happen mid-edit, and the callers say so rather than writing to
    /// whatever is selected instead.
    /// **The piece an edit is held on, as it will stand** — the read-only, placed-layer sibling of
    /// [`Self::at_target`].
    ///
    /// `at_target` exists because the list can move under a field that is still open, and a write has
    /// to land on the tile the field was opened against. The **range check** that write goes through
    /// has to come from the same tile, and it did not: it came from `focused_div`, which reads the
    /// live selection. Clicking another library row mid-word — an unconditional observer, and
    /// `keys::fires_in` only suppresses *keyboard* actions while typing — then checked one tile's
    /// cells against another tile's divisions. Selecting something smaller made every cell fail the
    /// check and the token was silently discarded; selecting something larger wrote a cell outside
    /// the small piece's lattice.
    pub fn placed_at_target<'a>(
        &'a self,
        target: &EditTarget,
        project: &'a crate::project::Project,
    ) -> Option<&'a Descriptor> {
        match target {
            EditTarget::Library(id) => project.library.get(id),
            EditTarget::Candidate(mesh) => self
                .candidates
                .iter()
                .find(|c| &c.mesh == mesh)
                .map(|c| &c.proposed),
        }
    }

    pub fn at_target<'a>(
        &'a mut self,
        target: &EditTarget,
        library: &'a mut emerge_core::library::Library,
    ) -> Option<(&'a mut Descriptor, Persist)> {
        match target {
            EditTarget::Library(id) => library
                .descriptors
                .iter_mut()
                .find(|d| &d.id == id)
                .map(|d| (d, Persist::Library)),
            EditTarget::Candidate(mesh) => self
                .candidates
                .iter_mut()
                .find(|c| &c.mesh == mesh)
                .map(|c| (&mut c.proposed, Persist::InMemory)),
        }
    }
}

/// **Write the measurements back to disk, or say why they could not be.** The one writer.
///
/// `commit_candidate` and `remove_tile` each grew their own copy of resolve → remeasure → `to_ron` →
/// `save_atomic`; a third copy, for lattice edits, is the point at which they would have drifted. The
/// masks and triangle counts are derived from the library, so they move with it or they are stale.
///
/// # It writes `measured`, not `library`
///
/// This used to serialize the **layered** library over `library_path`. Under `--kit site` — whose
/// `project.ron` stretches `site/wall` to a 2.40 m facility — toggling one lattice cell therefore
/// wrote that facility's wall height into the measurements file the kit exists to share, and the
/// next load applied the patches again on top of it. A library is measurements; a project's
/// architecture belongs in `project.ron` and must not leak downward into it.
///
/// # Re-layering happens *before* the write
///
/// Order is not incidental. `Policy::apply` fails when a patch matches nothing, so removing the last
/// piece a rule named is a real, reachable failure — and if the file were already written, `measured`
/// would be on disk while `library` still described the world before the edit. Re-layering first
/// means a refusal costs nothing but the message.
fn write_library(project: &mut Project) -> Result<std::path::PathBuf, String> {
    let edited = project.measured.clone();
    commit_measured(project, edited)
}

/// **Write these measurements, and adopt them only if that worked.**
///
/// The structural edits — adding an imported piece, removing one — need to *propose* a library rather
/// than mutate the live one, because both can be refused: by the two-sided surface check, by a policy
/// patch that now matches nothing, or by the file system. So the candidate comes in as a value and
/// nothing in `project` moves until it is on disk.
///
/// Both of those edits used to push and pop `project.library` — the **derived** view — and then call
/// [`write_library`], which serializes `measured` and rebuilds `library` from it. The write was
/// therefore byte-identical to what was already there, the change vanished, and the status line
/// reported success: *"added `crate_b` — it is in the palette now"* about a palette that never gained
/// it. A derived layer is not a place to keep anything; this is the only door to the one that is.
fn commit_measured(
    project: &mut Project,
    measured: emerge_core::library::Library,
) -> Result<std::path::PathBuf, String> {
    // Rebuild the layered view from the edited measurements, and prove it still holds together,
    // before anything touches the disk.
    let library = project.policy.apply(&measured)?;
    library.validate_lattices(project.policy.divisions)?;
    let masks = library.resolve(&project.vocab)?;

    let path = project.library_path.clone();
    let text = measured.to_ron()?;
    emerge_core::ron_surgery::save_atomic(&path, &text)?;

    project.measured = measured;
    project.library = library;
    project.masks = masks;
    project.remeasure_triangles();
    Ok(path)
}

/// Persist a lattice edit if it landed on a library entry, and fold the outcome into the status line.
///
/// Takes the message the edit already composed, so a successful write reads as the edit rather than
/// as a file operation — and a failed one **replaces** it, because an author told "cell 1,0,2 is
/// solid" by a program that could not write the file has been told something untrue.
fn persist(project: &mut Project, where_to: Persist, said: String) -> String {
    match where_to {
        Persist::InMemory => said,
        Persist::Library => match write_library(project) {
            Ok(_) => said,
            Err(e) => format!("NOT WRITTEN: {e}"),
        },
    }
}

/// Where an edit has to end up to survive.
///
/// Not a fallback pair — the two are different *destinations*, decided by which list has focus, and
/// every caller matches both arms.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Persist {
    /// A candidate. It is not in the library yet, so nothing is written until Accept.
    InMemory,
    /// A library entry. The file IS the state, so the edit is written now — the same rule
    /// `commit_candidate` and `remove_tile` already follow, and for the reason stated on the first:
    /// an editor that batches until some later save is one where a crash loses work an author
    /// believes they did.
    Library,
}

/// One tab in the strip, carrying the mode it selects.
#[derive(Component, Clone, Copy)]
struct Tab(Mode);

/// The tab's name, so the active one can be lit without touching its key.
#[derive(Component)]
struct TabLabel;

/// The tab's shortcut, styled a step quieter than the name.
#[derive(Component)]
struct TabKey;

/// Root of the tiles panel, shown and hidden with the mode.
#[derive(Component)]
struct TilesRoot;

/// Root of the map panel, so the mode can hide it.
#[derive(Component)]
pub struct MapRoot;

/// Root of an animation-bench panel.
#[derive(Component)]
pub struct AnimRoot;

/// One tag chip: which axis, and which token.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
struct TagChip {
    axis: Axis,
    /// Index into that axis's token table. The token itself lives in the vocabulary; carrying an
    /// index rather than a `String` keeps the component `Copy` and cannot drift from the table.
    token: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Kind,
    Effects,
    Look,
    /// What this piece OFFERS a top for — the two-sided axis.
    Surfaces,
}

impl Axis {
    fn label(self) -> &'static str {
        match self {
            Axis::Kind => "KIND",
            Axis::Effects => "DOES",
            Axis::Look => "LOOKS",
            Axis::Surfaces => "OFFERS",
        }
    }
    fn tokens<'a>(self, v: &'a emerge_core::vocab::Vocabularies) -> &'a emerge_core::vocab::Vocabulary {
        match self {
            Axis::Kind => &v.kind,
            Axis::Effects => &v.effects,
            Axis::Look => &v.look,
            Axis::Surfaces => &v.surfaces,
        }
    }
    fn list<'a>(self, d: &'a mut emerge_core::descriptor::Descriptor) -> &'a mut Vec<String> {
        match self {
            Axis::Kind => &mut d.kind,
            Axis::Effects => &mut d.effects,
            Axis::Look => &mut d.look,
            Axis::Surfaces => &mut d.offers.surfaces,
        }
    }
}

/// One candidate row, carrying its index.
#[derive(Component, Clone, Copy)]
struct CandidateRow(usize);

/// A pack heading in the candidate list. Clicking it folds the pack away.
#[derive(Component, Clone)]
struct PackHeader(String);

/// One row for a tile already in the library, carrying its id.
#[derive(Component, Clone)]
struct LibraryRow(String);

/// The node the candidate list is rebuilt into.
#[derive(Component)]
struct CandidateList;

/// The node the selected candidate's detail is rebuilt into.
#[derive(Component)]
struct DetailPane;

/// The candidate standing on the grid, so an author can see what they are about to accept.
#[derive(Component)]
struct Preview;

/// Which descriptor the live preview shows, so it is rebuilt only when the focus actually moves —
/// respawning a GLB every frame would thrash the asset server and never finish loading.
///
/// **Keyed by mesh path.** Not by candidate index — the pane can be focused on a library entry,
/// which has no index into `candidates`. And not by id, which was the previous key and is **not
/// unique among candidates**: a candidate's id comes from its file stem, and this tree really does
/// contain four collisions — `wall`, `column`, `pipe` and `crate` each appear as a `.glb` in more
/// than one pack. Selecting the second of a colliding pair left the first one's preview standing,
/// because the reuse check saw a matching id and returned; the author was shown one mesh while the
/// panel described another.
///
/// A mesh path is unique across both halves of the focus: `import::scan` skips any mesh the library
/// already has, so a candidate and a library entry can never name the same file.
#[derive(Component)]
struct PreviewOf(String);

/// The persistent line saying what the scan found.
#[derive(Component)]
struct ScanSummary;

/// The transient line saying what just happened.
#[derive(Component)]
struct ActionLine;

/// The measured footprint — what the placement rules reserve.
const FOOTPRINT: Color = Color::srgb(0.35, 0.72, 0.85);
/// The grid cells it occupies. Where this and the footprint differ is the tiling slack.
const CELLS: Color = Color::srgb(0.42, 0.38, 0.30);
/// The volume, so a height is seen rather than only read.
const EXTENT: Color = Color::srgb(0.24, 0.42, 0.50);

pub struct TilesPlugin;

impl Plugin for TilesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mode>()
            .init_resource::<ImportState>()
            .init_resource::<MapView>()
            .init_resource::<CellEdit>()
            .init_resource::<LatticePick>()
            .init_resource::<NoteEdit>()
            // Registered in the same commit as `scale_keys` and `rebuild_detail` read it — a missing
            // `Res<T>` panics its system in Bevy 0.19 rather than skipping it (`CLAUDE.md`).
            .init_resource::<ScaleEdit>()
            .add_systems(Startup, (spawn_tab_strip, spawn_tiles_panel))
            .add_systems(
                Update,
                (
                    // **Gated by the context, not by a run condition.** `2` typed into a filter or
                    // an id used to jump the author to another tab mid-word; `keys::just_pressed`
                    // now refuses every one of these unless the Tiles tab owns the keyboard, so the
                    // `not_typing` conditions that used to say the same thing are gone rather than
                    // duplicated. `Phase::Act` puts them all ahead of the text fields below.
                    toggle_mode.in_set(crate::keys::Phase::Act),
                    move_selection.in_set(crate::keys::Phase::Act),
                    lattice_keys.in_set(crate::keys::Phase::Act),
                    autoscan_candidate.run_if(in_tiles_mode),
                    // Nested: a system tuple caps out, and these three are one feature anyway —
                    // point at the piece, click a cell, see which one is under the cursor.
                    // Nested: a system tuple caps out at twenty, and these five are one feature —
                    // the staged piece, what the cursor is on, and what a click does to it.
                    (
                        pick_lattice,
                        click_lattice.run_if(in_tiles_mode),
                        draw_pick.run_if(in_tiles_mode),
                        draw_preview_footprint.run_if(in_tiles_mode),
                        draw_subgrid.run_if(in_tiles_mode),
                    ),
                    // Nested as a pair, because a system tuple caps out at twenty and these two are
                    // one rule: a selection the filter has hidden must not stay selected, in either
                    // list. Accept and Remove both act on a selection.
                    (
                        keep_library_selection_visible.run_if(in_tiles_mode),
                        keep_candidate_selection_visible.run_if(in_tiles_mode),
                    ),
                    cycle_mount.in_set(crate::keys::Phase::Act),
                    commit_candidate.in_set(crate::keys::Phase::Act),
                    remove_tile.in_set(crate::keys::Phase::Act),
                    tab_shortcuts.in_set(crate::keys::Phase::Act),
                    apply_mode,
                    stage_camera,
                    // **The text fields, last.** `cell_keys` and `commit_candidate` both take
                    // `ResMut<ImportState>` and used to sit in this tuple unordered — so Bevy could
                    // run the field first, clear its own typing flag, and let the same `Enter` fall
                    // through to "add to library". Six descriptors arrived in `library.ron` that way.
                    rename_candidate.in_set(crate::keys::Phase::Text),
                    cell_keys.in_set(crate::keys::Phase::Text),
                    style_tabs,
                    rebuild_candidates.run_if(resource_changed::<ImportState>.or_else(resource_changed::<crate::filter::Filters>)),
                    // **Structure only.** The selection and the carets are repainted in place by
                    // `refresh_cells`; rebuilding the pane for them is the bounce.
                    rebuild_detail.run_if(resource_changed::<ImportState>),
                    refresh_lines,
                    drive_preview,
                ),
            )
            // A second `add_systems` rather than a nested tuple — `add_systems` caps a tuple at 20
            // in 0.19, and nesting would imply these belong together for a reason.
            .add_systems(
                Update,
                (
                    note_keys.in_set(crate::keys::Phase::Text),
                    scale_keys.in_set(crate::keys::Phase::Text),
                    refresh_cells,
                ),
            )
            .add_observer(on_tab_click)
            .add_observer(on_cell_click)
            .add_observer(on_cell_verb)
            .add_observer(on_scan_mesh)
            .add_observer(on_rotate_click)
            .add_observer(on_fill_header)
            .add_observer(on_note_click)
            .add_observer(on_scale_click)
            .add_observer(on_candidate_click)
            .add_observer(on_library_click)
            .add_observer(on_pack_click)
            .add_observer(on_tag_chip);
    }
}

/// The tab strip. Always visible, above whichever panel is showing.
///
/// A key alone was not enough. `Tab` cycles the mode and always did, but a mode you can only reach by
/// pressing something is a mode you have to be told about — and an editor that has to be explained
/// has a bug in its front page. The strip says both things at once: which jobs exist, and which one
/// you are doing.
fn spawn_tab_strip(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(12.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(2.0),
                ..default()
            },
            GlobalZIndex(101),
        ))
        .with_children(|p| {
            for mode in Mode::ALL {
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    Tab(mode),
                    Node {
                        padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                        align_items: AlignItems::Center,
                        // A thick bottom border is the active marker: it reads at a glance and does
                        // not depend on telling two dark greys apart, which `docs/ui.md` §1.3 rules
                        // out as an encoding on its own.
                        border: UiRect::bottom(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(Color::NONE),
                    BackgroundColor(ROW_BG),
                ))
                .with_children(|tab| {
                    // **The tab states its key.** Cockburn et al. 2014 on the intermodal-transition
                    // failure: offering a fast path beside a slow one does not work on its own, and
                    // users plateau on the slow one. The key has to be visible at the moment of use,
                    // which is `docs/ui.md` §4.2's "each chip states its key".
                    tab.spawn((
                        Text::new(crate::keys::chord(mode.action())),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                        Node {
                            margin: UiRect::right(Val::Px(7.0)),
                            ..default()
                        },
                        TabKey,
                    ));
                    tab.spawn((
                        Text::new(mode.label()),
                        TextColor(LABEL),
                        TextFont::from_font_size(13.0),
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                        TabLabel,
                    ));
                });
            }
        });
}

/// Clicking a tab selects it — and scans on the first visit to the tiles tab, exactly as the key
/// does, so the two ways in behave the same.
fn on_tab_click(
    activate: On<Activate>,
    tabs: Query<&Tab>,
    project: Res<Project>,
    mut mode: ResMut<Mode>,
    mut state: ResMut<ImportState>,
) {
    let Ok(tab) = tabs.get(activate.entity) else {
        return;
    };
    if *mode == tab.0 {
        return;
    }
    *mode = tab.0;
    if *mode == Mode::Tiles && !state.scanned {
        scan(&project, &mut state);
    }
}

/// Light the active tab. The inactive one stays legible rather than greyed to nothing — a tab you
/// cannot read is a tab you do not know is there.
fn style_tabs(
    mode: Res<Mode>,
    mut tabs: Query<(&Tab, &Hovered, &mut BackgroundColor, &mut BorderColor, &Children)>,
    mut names: Query<&mut TextColor, (With<TabLabel>, Without<TabKey>)>,
    mut chords: Query<&mut TextColor, (With<TabKey>, Without<TabLabel>)>,
) {
    for (tab, hovered, mut bg, mut border, children) in &mut tabs {
        let active = tab.0 == *mode;
        // The active tab continues the panel beneath it, so the two read as one surface rather than
        // as a button sitting on top of a box.
        let want_bg = if active {
            PANEL_BG
        } else if hovered.0 {
            Color::srgb(0.16, 0.15, 0.14)
        } else {
            ROW_BG
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want_border = if active { ACCENT } else { Color::NONE };
        *border = BorderColor::all(want_border);

        for child in children.iter() {
            if let Ok(mut colour) = names.get_mut(child) {
                let want = if active { TEXT } else { DIM };
                if colour.0 != want {
                    colour.0 = want;
                }
            }
            if let Ok(mut colour) = chords.get_mut(child) {
                let want = if active { ACCENT } else { LABEL };
                if colour.0 != want {
                    colour.0 = want;
                }
            }
        }
    }
}

/// The number keys jump straight to a tab, and scan on first arrival exactly as `Tab` and a click do.
///
/// Three ways in, one behaviour — `docs/ui.md` §4.2: everything reachable by mouse is reachable by
/// keyboard and vice versa.
fn tab_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Res<Project>,
    mut mode: ResMut<Mode>,
    mut state: ResMut<ImportState>,
) {
    for want in Mode::ALL {
        if keys::just_pressed(&keyboard, live.0, want.action()) && *mode != want {
            *mode = want;
            if want == Mode::Tiles && !state.scanned {
                scan(&project, &mut state);
            }
            return;
        }
    }
}

/// **Two panels, the same two the map tab has.** Controls down the left, the list down the right.
///
/// The list used to be a `max_height: 300` box a third of the way down this panel — the same shape
/// the map palette was fixed out of, and for the same reason it did not work: a `max_height` inside a
/// panel that is not pinned to the bottom of the viewport is never reached. 318 candidates scrolled
/// in a 300 px window. Now it is `chrome::scroll_list` inside a `full_height` panel, which is bounded
/// by construction.
fn spawn_tiles_panel(mut commands: Commands) {
    crate::chrome::panel_root(
        &mut commands,
        crate::chrome::Side::Left,
        crate::chrome::TILES_CONTROLS_W,
        // Pinned to the bottom as well, so the detail block below has a bounded height to scroll
        // inside rather than running off the screen edge the way it did at first.
        true,
        // Starts hidden: the editor opens in map mode.
        true,
    )
    .insert(TilesRoot)
    .with_children(|p| {
        crate::chrome::title(p, "TILE CONFIGURATION");
        crate::chrome::shortcut_hint(p);

        p.spawn((
            Text::new(""),
            TextColor(DIM),
            TextFont::from_font_size(10.0),
            Node {
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            ScanSummary,
        ));

        // **The detail scrolls.** A candidate's block is a size, a layer, four rows of tag chips and
        // however many sentences its findings need — variable, and for a mesh with several findings
        // taller than the panel. Bounded and scrollable beats running off the bottom edge, which is
        // where the "no facing is derived" note was going.
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                margin: UiRect::top(Val::Px(8.0)),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollArea::default(),
            DetailPane,
        ));

        p.spawn((
            Text::new(""),
            TextColor(ACCENT),
            TextFont::from_font_size(10.0),
            Node {
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            },
            ActionLine,
        ));
    });

    // **The candidate list, in its own panel against the right edge** — the same shape, the same
    // builders and the same place on screen as the map tab's palette, so moving between the two tabs
    // does not mean learning a second layout.
    crate::chrome::panel_root(
        &mut commands,
        crate::chrome::Side::Right,
        crate::chrome::LIST_W,
        true,
        true,
    )
    .insert(TilesRoot)
    // No heading here: `rebuild_candidates` writes its own section headers ("IN LIBRARY (43)",
    // "NOT YET IMPORTED (317)") with live counts, and a static one above them said the same thing
    // twice and disagreed with the first section.
    .with_children(|p| {
        crate::filter::spawn(p, crate::filter::Pane::Candidates);
        crate::chrome::scroll_list(p, CandidateList);
    });
}

/// Tab swaps the job. `R` rescans, because meshes arrive while the editor is open — an importer that
/// only sees what was on disk at launch is one you have to restart to use.
fn toggle_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Res<Project>,
    mut mode: ResMut<Mode>,
    mut state: ResMut<ImportState>,
) {
    let want_scan = if keys::just_pressed(&keyboard, live.0, Action::NextTab) {
        // Cycle, not toggle: a third tab then costs a row in `Mode::ALL` and nothing else.
        let at = Mode::ALL.iter().position(|m| m == &*mode).unwrap_or(0);
        *mode = Mode::ALL[(at + 1) % Mode::ALL.len()];
        *mode == Mode::Tiles && !state.scanned
    } else {
        *mode == Mode::Tiles && keys::just_pressed(&keyboard, live.0, Action::Rescan)
    };

    if want_scan {
        scan(&project, &mut state);
    }
}

fn scan(project: &Project, state: &mut ImportState) {
    let root = project.root.join("assets");
    match import::scan(&root, &root, &project.library) {
        Ok(found) => {
            let blocked = found.iter().filter(|c| c.blocked()).count();
            let warned = found
                .iter()
                .filter(|c| c.worst() == Some(Severity::Warn))
                .count();
            state.summary = format!(
                "{} mesh(es) not in the library — {warned} with warnings, {blocked} unmeasurable",
                found.len()
            );
            state.candidates = found;
            state.selected = 0;
            state.scanned = true;
        }
        Err(e) => {
            state.summary = e;
            state.scanned = true;
        }
    }
}

/// Type an id for the selected candidate. Same rule as the map's name and the same behaviour: the
/// spelling is forced as you type, and the field starts EMPTY so the first keystroke replaces rather
/// than appends.
fn rename_candidate(
    mut events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut state: ResMut<ImportState>,
) {
    if state.renaming.is_none() {
        if keys::just_pressed(&keyboard, live.0, Action::TypeId) {
            if let Some(id) = state.selected_library_id.clone() {
                state.status = format!(
                    "`{id}` is in the library — renaming it would strand every placement that names it"
                );
            } else if let Some(mesh) = state.current().map(|c| c.mesh.clone()) {
                // The target, captured now. See `ImportState::renaming`.
                state.renaming = Some(Rename {
                    mesh,
                    raw: String::new(),
                });
                state.status = "type an id — Enter to keep it, Esc to leave it alone".to_owned();
            }
        }
        // **Drain before leaving**, so the `I` that opened the field is not still waiting to be read
        // as the field's first character next frame. Same invariant as `cell_keys`.
        events.clear();
        return;
    }

    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let Some(rename) = state.renaming.take() else {
                    return;
                };
                let id = emerge_core::naming::to_snake_case(&rename.raw);
                if id.is_empty() {
                    state.status = "an id cannot be empty; nothing was changed".to_owned();
                } else {
                    // The candidate this field was opened on, found by the mesh it names rather than
                    // by wherever the selection has since moved to.
                    match state
                        .candidates
                        .iter_mut()
                        .find(|c| c.mesh == rename.mesh)
                    {
                        Some(c) => {
                            c.proposed.id = id.clone();
                            state.status = format!("id is `{id}`");
                        }
                        // Only reachable if a rescan dropped the mesh mid-rename, which is a real
                        // thing `R` can do. Saying so beats renaming whatever is selected now.
                        None => {
                            state.status = format!(
                                "`{id}` was not kept — `{}` is no longer in the scan.",
                                rename.mesh
                            )
                        }
                    }
                }
            }
            Key::Escape => {
                state.renaming = None;
                state.status = "id unchanged".to_owned();
            }
            Key::Backspace => {
                if let Some(r) = state.renaming.as_mut() {
                    r.raw.pop();
                }
            }
            Key::Space => {
                if let Some(r) = state.renaming.as_mut() {
                    r.raw.push(' ');
                }
            }
            Key::Character(s) => {
                if let Some(r) = state.renaming.as_mut() {
                    r.raw.push_str(s);
                }
            }
            _ => {}
        }
    }
}

/// `M` steps the mount this piece goes on.
///
/// A cycle rather than a menu because there are nine of them and the list is short enough to walk;
/// the label says where you are, so nobody has to count presses.
fn cycle_mount(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    if !keys::just_pressed(&keyboard, live.0, Action::CycleMount) {
        return;
    }
    let surfaces: Vec<String> = project.vocab.surfaces.names().map(str::to_owned).collect();
    let options = mount_options(&surfaces);
    let Some((d, where_to)) = state.editing_mut(&mut project.measured) else {
        return;
    };
    let next = d
        .mount
        .as_ref()
        .and_then(|m| options.iter().position(|o| o == m))
        .map_or(0, |i| (i + 1) % options.len());
    d.mount = Some(options[next].clone());
    let said = format!("mount: {}", mount_label(d.mount.as_ref()));
    state.status = persist(&mut project, where_to, said);
}

/// **Accept a candidate into the library.**
///
/// Validated first, and refused rather than repaired: a descriptor that fails the vocabulary is one
/// an author has not finished, and writing a broken entry would make the next `Library::parse` fail
/// for everyone rather than for the person who caused it.
///
/// The library is written immediately. An importer that batches its additions until some later save
/// is one where a crash loses work an author believes they did — and the file is generated from the
/// manifests today, so an unwritten addition would simply be regenerated away.
fn commit_candidate(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    if !keys::just_pressed(&keyboard, live.0, Action::Accept) {
        return;
    }
    if let Some(id) = state.selected_library_id.clone() {
        state.status =
            format!("`{id}` is already in the library — pick a candidate below to add one");
        return;
    }
    let Some(candidate) = state.current().cloned() else {
        return;
    };
    if candidate.blocked() {
        state.status = "this mesh cannot be measured, so there is nothing to add".to_owned();
        return;
    }
    let descriptor = candidate.proposed.clone();
    if descriptor.id.trim().is_empty() {
        state.status = "give it an id first (I)".to_owned();
        return;
    }
    if project.library.get(&descriptor.id).is_some() {
        state.status = format!("`{}` is already in the library — rename it (I)", descriptor.id);
        return;
    }

    // **Into `measured`** — the layer that is written and the layer an import belongs in: what a mesh
    // scan produces is a measurement, and the project's architecture is layered over it afterwards.
    //
    // A proposal, not a mutation. `commit_measured` layers the policy over it and runs the two-sided
    // surface check on the result, which is the right shape for that check: it is about the finished
    // set, so a piece that offers `worktop` makes another piece's `on worktop` legal, and checking it
    // in isolation would reject the pair that fixes each other.
    let mut trial = project.measured.clone();
    trial.descriptors.push(descriptor.clone());
    match commit_measured(&mut project, trial) {
        Ok(path) => {
            // Drop it from the candidate list: it is in the library now, and an importer that keeps
            // offering what you have already taken is one you cannot tell your progress from.
            let at = state.selected;
            state.candidates.remove(at);
            state.selected = at.min(state.candidates.len().saturating_sub(1));
            state.summary = format!("{} mesh(es) left to import", state.candidates.len());
            state.status = format!(
                "added `{}` — it is in the palette now",
                descriptor.id
            );
            info!("added `{}` to {}", descriptor.id, path.display());
        }
        Err(e) => {
            // Nothing was added and nothing was written — `commit_measured` refuses before it
            // touches the disk — so this says the one thing that is true of both.
            state.status = format!("not added: {e}");
            error!("{e}");
        }
    }
}

/// **Take a tile back out of the library.**
///
/// The tiles tab lists what is IN the library above what could be added to it, because "configure the
/// tiles" is both halves of that and an editor with an add and no remove is one where a mistyped
/// import is permanent.
///
/// It refuses to remove a descriptor the open map is using. An orphaned placement is not an error the
/// map can carry — it names a descriptor nothing defines, so the piece silently fails to appear and
/// the author finds out by counting crates. Saying "12 placements use this" is the answer; deleting
/// them on their behalf is not.
fn remove_tile(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    if !keys::just_pressed(&keyboard, live.0, Action::RemoveTile) {
        return;
    }
    let Some(id) = state.selected_library_id.clone() else {
        state.status = "select a library tile to remove it".to_owned();
        return;
    };

    let used = project
        .map
        .placements
        .iter()
        .filter(|p| p.descriptor == id)
        .count();
    if used > 0 {
        state.status = format!(
            "`{id}` is used by {used} placement(s) in this map — remove those first"
        );
        return;
    }

    // **Out of `measured`**, for the reason `commit_candidate` writes into it: the derived layer is
    // rebuilt from the measurements on every write, so removing a piece from it removed nothing.
    // Ids survive layering — `Policy::apply` patches entries, it does not rename them — so the piece
    // named by the palette is the piece named here.
    let Some(at) = project.measured.descriptors.iter().position(|d| d.id == id) else {
        return;
    };
    let mut trial = project.measured.clone();
    trial.descriptors.remove(at);
    // `commit_measured` re-validates, which is what catches the interesting failure: removing a piece
    // can strand another that rested on a surface only it offered, and it can leave a policy patch
    // matching nothing.
    match commit_measured(&mut project, trial) {
        Ok(path) => {
            state.selected_library_id = None;
            state.status = format!("removed `{id}` from the library");
            info!("removed `{id}` from {}", path.display());
        }
        Err(e) => state.status = format!("not removed: {e}"),
    }
}

/// Toggle one token on one axis.
fn on_tag_chip(
    activate: On<Activate>,
    chips: Query<&TagChip>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    let Ok(chip) = chips.get(activate.entity) else {
        return;
    };
    // **Both the token and the vocabulary order come out first, owned.** What follows needs a mutable
    // borrow of the same `Project`, and the sort key cannot be read from it while that is held.
    let (token, order) = {
        let names: Vec<String> = chip
            .axis
            .tokens(&project.vocab)
            .names()
            .map(str::to_owned)
            .collect();
        match names.get(chip.token) {
            Some(t) => (t.clone(), names),
            None => return,
        }
    };
    // **Through the focus, like every other mutator.** This wrote to `candidates[selected]` while the
    // pane had already been switched to read whatever the focus points at, so clicking a tag on a
    // library tile edited an invisible candidate: the chip did not light, the descriptor did not
    // change, and nothing reached disk. The one accessor exists so this cannot happen; missing it
    // here is what it looks like when it does.
    let Some((d, where_to)) = state.editing_mut(&mut project.measured) else {
        return;
    };
    let list = chip.axis.list(d);
    match list.iter().position(|t| *t == token) {
        Some(i) => {
            list.remove(i);
        }
        // Kept in vocabulary order rather than click order, so two descriptors with the same tags
        // serialize identically and a diff of the library shows real changes only.
        None => {
            list.push(token.clone());
            list.sort_by_key(|t| order.iter().position(|o| o == t).unwrap_or(usize::MAX));
        }
    }
    let said = format!("{} tags updated", chip.axis.label().to_lowercase());
    state.status = persist(&mut project, where_to, said);
}

fn move_selection(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Res<Project>,
    // The arrows walk what is on screen, which is what the filter decides.
    filters: Res<crate::filter::Filters>,
    mut state: ResMut<ImportState>,
) {
    // **Read the keys before touching the focus.** Clearing `selected_library_id` unconditionally
    // would steal the focus back on the very next frame after a library row was clicked — the system
    // runs every frame, not only when an arrow arrives.
    let down = keys::just_pressed(&keyboard, live.0, Action::NextCandidate);
    let up = keys::just_pressed(&keyboard, live.0, Action::PrevCandidate);
    let to_library = keys::just_pressed(&keyboard, live.0, Action::FocusLibrary);
    let to_candidates = keys::just_pressed(&keyboard, live.0, Action::FocusCandidates);

    // Left/right choose which list the arrows walk. `selected_library_id` is already the one
    // discriminant the detail pane reads, so this sets it and everything else follows.
    if to_candidates {
        state.selected_library_id = None;
        state.status = "candidates".to_owned();
    }
    if to_library {
        match library_ids(project.as_ref(), &filters).first() {
            Some(first) => {
                if state.selected_library_id.is_none() {
                    state.selected_library_id = Some(first.clone());
                }
                state.status = "library".to_owned();
            }
            None => state.status = "the library is empty".to_owned(),
        }
    }
    if !down && !up {
        return;
    }

    // Walk whichever list has the focus. Two lists, one pair of keys — the alternative was a second
    // pair nobody would remember, on a tab already carrying ten rows of its twelve.
    match state.selected_library_id.clone() {
        Some(id) => {
            let ids = library_ids(project.as_ref(), &filters);
            let Some(at) = ids.iter().position(|d| *d == id) else {
                return;
            };
            let want = if down { at + 1 } else { at.saturating_sub(1) };
            if let Some(next) = ids.get(want.min(ids.len().saturating_sub(1))) {
                state.selected_library_id = Some(next.clone());
                state.status = format!("`{next}` selected — {} removes it", keys::binding(Action::RemoveTile).chord);
            }
        }
        None => {
            // The visible rows, for the reason the library branch above walks `library_ids`. This
            // branch was left stepping `state.selected` through the unfiltered list, which is the
            // worse half of the same defect: the candidate list is where **Accept** acts, so with the
            // list filtered to three rows, one Down moved the focus to an unrelated mesh that was not
            // on screen — `autoscan_candidate` then scanned it, and Enter imported it.
            let rows = candidate_rows(&state, &filters);
            let at = match rows.iter().position(|&i| i == state.selected) {
                Some(at) => at,
                // Not on screen at all. An arrow then means "start from the top of what I can see",
                // which is where the eye already is.
                None => {
                    if let Some(&first) = rows.first() {
                        state.selected = first;
                    }
                    return;
                }
            };
            let want = if down { at + 1 } else { at.saturating_sub(1) };
            if let Some(&next) = rows.get(want.min(rows.len().saturating_sub(1))) {
                state.selected = next;
            }
        }
    }
}

/// **The candidate rows the author can actually see**, as indices into [`ImportState::candidates`].
///
/// The sibling of [`library_ids`], filtered with the same predicate `rebuild_candidates` renders
/// with. Indices rather than mesh paths because `ImportState::selected` is an index and every other
/// reader of the focus already goes through it.
fn candidate_rows(state: &ImportState, filters: &crate::filter::Filters) -> Vec<usize> {
    let pane = crate::filter::Pane::Candidates;
    state
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| filters.keeps(pane, &c.mesh))
        .map(|(i, _)| i)
        .collect()
}

/// **Keep the candidate selection on a row that is showing.**
///
/// [`keep_library_selection_visible`] for the other list, and it matters more here: filtering could
/// hide the selected candidate while it stayed selected, and Accept acts on the selection — so the
/// most natural way to find a mesh was also the way to import a different one. Same last rule as its
/// sibling: if nothing survives the filter, leave the selection alone rather than jumping it
/// somewhere arbitrary the moment a half-typed query matches nothing.
fn keep_candidate_selection_visible(
    filters: Res<crate::filter::Filters>,
    mut state: ResMut<ImportState>,
) {
    if !filters.is_changed() {
        return;
    }
    let visible = candidate_rows(&state, &filters);
    if visible.iter().any(|&i| i == state.selected) {
        return;
    }
    if let Some(&first) = visible.first() {
        state.selected = first;
    }
}

/// The library's ids in the order the panel lists them — declaration order, which is the order the
/// rows are drawn in, so the arrows walk what the eye reads.
/// **The library ids the author can actually see**, in list order.
///
/// Filtered with the same predicate `rebuild_candidates` renders with. Walking the unfiltered
/// library meant the arrows stepped through rows that were not on screen — and `Delete` acts on the
/// selection, so the key removed a tile the author never saw. The list and the keys have to agree
/// about what "next" means or one of them is lying.
fn library_ids(project: &Project, filters: &crate::filter::Filters) -> Vec<String> {
    let pane = crate::filter::Pane::Candidates;
    project
        .library
        .descriptors
        .iter()
        .filter(|d| filters.keeps(pane, &d.id))
        .map(|d| d.id.clone())
        .collect()
}

/// **Keep the library selection on a row that is showing.**
///
/// The other half of the same defect: filtering after selecting could hide the selected tile while
/// it stayed selected, and `Delete` would still remove it. Copied from
/// `anim_tab::keep_selection_visible`, including its last rule — if nothing survives the filter,
/// leave the selection alone rather than jumping it somewhere arbitrary the moment a half-typed
/// filter matches nothing.
fn keep_library_selection_visible(
    filters: Res<crate::filter::Filters>,
    project: Res<Project>,
    mut state: ResMut<ImportState>,
) {
    if !filters.is_changed() {
        return;
    }
    let Some(id) = state.selected_library_id.clone() else {
        return;
    };
    let visible = library_ids(&project, &filters);
    if visible.iter().any(|v| *v == id) {
        return;
    }
    if let Some(first) = visible.first() {
        state.selected_library_id = Some(first.clone());
    }
}

fn on_pack_click(
    activate: On<Activate>,
    headers: Query<&PackHeader>,
    mut state: ResMut<ImportState>,
) {
    let Ok(header) = headers.get(activate.entity) else {
        return;
    };
    if !state.folded_packs.remove(&header.0) {
        state.folded_packs.insert(header.0.clone());
    }
}

fn on_library_click(
    activate: On<Activate>,
    rows: Query<&LibraryRow>,
    mut state: ResMut<ImportState>,
) {
    if let Ok(row) = rows.get(activate.entity) {
        state.selected_library_id = Some(row.0.clone());
        state.status = format!("`{}` selected — Del removes it", row.0);
    }
}

fn on_candidate_click(
    activate: On<Activate>,
    rows: Query<&CandidateRow>,
    mut state: ResMut<ImportState>,
) {
    if let Ok(row) = rows.get(activate.entity) {
        state.selected = row.0;
        // One selection at a time, or `Del` would have to guess which list it meant.
        state.selected_library_id = None;
    }
}

/// Show one panel and hide the other. `Display::None` rather than `Visibility`, because a hidden-by-
/// visibility UI node still occupies layout and still answers hover — which would leave the map
/// panel's rows eating clicks aimed at the world.
fn apply_mode(
    mode: Res<Mode>,
    // **One query with `Has`, not one per marker.** Three disjoint queries would each need `Without`
    // of both others, and a fourth tab would need three more — this asks each panel which tab it
    // belongs to instead.
    mut panels: Query<(&mut Node, Has<MapRoot>, Has<TilesRoot>, Has<AnimRoot>)>,
) {
    if !mode.is_changed() {
        return;
    }
    for (mut node, is_map, is_tiles, is_anim) in &mut panels {
        let mine = match *mode {
            Mode::Map => is_map,
            Mode::Tiles => is_tiles,
            Mode::Anim => is_anim,
        };
        // A panel belonging to no tab is not ours to touch — the tab strip and the cost readout are
        // both unmarked and must stay visible in every mode.
        if !(is_map || is_tiles || is_anim) {
            continue;
        }
        let want = if mine { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

fn in_tiles_mode(mode: Res<Mode>) -> bool {
    *mode == Mode::Tiles
}

/// Keep one preview alive, showing the selected candidate at the origin with its PROPOSED alignment
/// applied.
///
/// Proposed, not raw: the whole question an author is answering here is "will this sit right when the
/// game places it", and showing the mesh as exported answers a different one. A candidate whose origin
/// is 2 m off its base looks wrong in the file and correct here, which is the importer saying "I have
/// a fix for that" in the only language that settles it.
fn drive_preview(
    mut commands: Commands,
    mode: Res<Mode>,
    assets: Res<AssetServer>,
    state: Res<ImportState>,
    project: Res<Project>,
    previews: Query<(Entity, &PreviewOf), With<Preview>>,
) {
    let clear = |commands: &mut Commands| {
        for (e, _) in &previews {
            commands.entity(e).despawn();
        }
    };
    if *mode != Mode::Tiles {
        clear(&mut commands);
        return;
    }
    let Some(d) = state.editing(&project.measured) else {
        clear(&mut commands);
        return;
    };
    // A blocked candidate has no trustworthy alignment, so a preview of it would be a picture of a
    // guess. The findings say why; an empty grid is the honest illustration. A library entry was
    // measured when it was accepted, so it has no such doubt.
    if state.selected_library_id.is_none() && state.current().is_some_and(|c| c.blocked()) {
        clear(&mut commands);
        return;
    }

    // The key, and the reason there is nothing to show without one: a descriptor with no mesh has
    // no preview, and leaving the previous one up would caption it with the wrong piece.
    let Some(mesh) = d.mesh.as_ref() else {
        clear(&mut commands);
        return;
    };

    for (e, of) in &previews {
        if &of.0 != mesh {
            commands.entity(e).despawn();
        }
    }
    if previews.iter().any(|(_, of)| &of.0 == mesh) {
        return;
    }
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(mesh.clone()));
    let a = &d.align;
    // The pivot shifts the model so its bounding-box centre lands on the placement point, which is
    // what makes the symmetric footprint an accurate reservation rather than an approximation.
    let pivot = a.pivot.unwrap_or((0.0, 0.0));
    commands
        .spawn((
            Preview,
            PreviewOf(mesh.clone()),
            Transform::from_xyz(
                STAGE.x - pivot.0,
                STAGE.y + a.y_offset.unwrap_or(0.0),
                STAGE.z - pivot.1,
            )
            // **The transform a real placement applies**, which is
            // `(scale, scale * stretch_y, scale)` — see `emerge_bevy::spawn_descriptor`. This was a
            // uniform `splat(scale)`, so the preview showed an unstretched piece while
            // `draw_subgrid` and `stage_box` sized their box off `placed_height`, which includes the
            // stretch: the lattice floated above a mesh that did not reach it. The same class as the
            // rotate bug on the child below — the one tab that edits these fields was the one place
            // they did not all show.
            .with_scale(Vec3::new(
                a.scale.unwrap_or(1.0),
                a.scale.unwrap_or(1.0) * a.stretch_y.unwrap_or(1.0),
                a.scale.unwrap_or(1.0),
            )),
            Visibility::Inherited,
        ))
        // **The mesh child carries `align.rotate`**, the same way `emerge_bevy::spawn_world` does —
        // see its note on why the export correction belongs here and not on the parent.
        //
        // This was `Transform::default()`, so the one tab that *has* the rotate chips and the
        // `RotateMesh*` keys was the one place a rotation had no visible effect: `rotate_mesh`
        // rewrote the footprint, height, pivot and y_offset, `draw_preview_footprint` and
        // `draw_subgrid` redrew for the standing piece, and the mesh stayed lying down — offset by
        // the *rotated* pivot, so it also floated off its own footprint rectangle.
        .with_child((
            WorldAssetRoot(scene),
            Transform::from_rotation(emerge_bevy::mesh_rotation(d)),
        ));
}

/// Draw the footprint the placement rules will reserve, and the grid cells it occupies.
///
/// Two rectangles, deliberately: the measured footprint, and the cells a flood fill would step on.
/// Where they differ is exactly the gap-or-overlap the findings describe in words, and a number in a
/// sentence is much easier to skip than a line that plainly does not meet its neighbour.
/// The subgrid lattice, over the staged tile.
const LATTICE: Color = Color::srgb(0.38, 0.34, 0.46);
/// A cell an author has said something about — solid, an edge, or an anchor.
const LATTICE_SET: Color = Color::srgb(0.62, 0.52, 0.82);

/// **Draw the tile's internal lattice.**
///
/// The subgrid is the thing that lets two pieces agree on where they meet, and it is invisible in the
/// file. Drawn over the staged piece it becomes something an author can point at: the divisions they
/// chose, and which cells they have marked.
///
/// Only the marked cells get a filled box. Twenty-seven wireframe cubes over every prop would be the
/// crowding failure `docs/ui.md` §1.2 names, on a tile instead of a panel — the lattice would hide the
/// mesh it describes.
fn draw_subgrid(state: Res<ImportState>, project: Res<Project>, mut gizmos: Gizmos) {
    // As placed — the same layer `focused_div` range-checks against, so the grid an author clicks is
    // the grid their click is written into. See [`ImportState::placed`].
    let Some(desc) = state.placed(&project) else {
        return;
    };
    let Some((w, d)) = emerge_core::descriptor::placed_footprint(desc) else {
        return;
    };
    let h = emerge_core::descriptor::placed_height(desc).unwrap_or(0.0);
    let empty = emerge_core::descriptor::Subgrid::default();
    let g = desc.subgrid.as_ref().unwrap_or(&empty);
    let Ok((dx, dy, dz)) = project.divisions_of(desc) else {
        return;
    };
    if dx == 0 || dy == 0 || dz == 0 {
        return;
    }
    let step = Vec3::new(w / dx as f32, h.max(0.05) / dy as f32, d / dz as f32);
    let origin = STAGE - Vec3::new(w * 0.5, 0.0, d * 0.5);

    // The division planes, drawn as a wire box per column rather than per cell: a floor grid plus the
    // vertical extent reads as a lattice without 27 outlines competing with the mesh.
    for ix in 0..dx {
        for iz in 0..dz {
            let centre = origin
                + Vec3::new(
                    (ix as f32 + 0.5) * step.x,
                    0.002,
                    (iz as f32 + 0.5) * step.z,
                );
            gizmos.rect(
                Isometry3d::new(centre, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
                Vec2::new(step.x, step.z),
                LATTICE,
            );
        }
    }

    // What the author has actually marked, as a solid box each — this is the part worth seeing.
    for cell in &g.cells {
        let (cx, cy, cz) = cell.at;
        if cx >= dx || cy >= dy || cz >= dz {
            continue;
        }
        let centre = origin
            + Vec3::new(
                (cx as f32 + 0.5) * step.x,
                (cy as f32 + 0.5) * step.y,
                (cz as f32 + 0.5) * step.z,
            );
        gizmos.cube(
            Transform::from_translation(centre).with_scale(step * 0.92),
            LATTICE_SET,
        );
    }
}

fn draw_preview_footprint(state: Res<ImportState>, project: Res<Project>, mut gizmos: Gizmos) {
    let Some(desc) = state.editing(&project.measured) else {
        return;
    };
    // **As placed, both rectangles.** This function's own doc says it draws "the footprint the
    // placement rules will reserve", and after `stack::covers` learned about `align.scale` that is the
    // placed footprint — drawing the measured one would have shown an author a reservation nothing
    // uses, on the one tab where the size is now editable.
    let Some((w, d)) = emerge_core::descriptor::placed_footprint(desc) else {
        return;
    };
    let height = emerge_core::descriptor::placed_height(desc).unwrap_or(0.0);

    // The mesh's own footprint, at the floor.
    gizmos.rect(
        Isometry3d::new(
            STAGE + Vec3::new(0.0, 0.005, 0.0),
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        ),
        Vec2::new(w, d),
        FOOTPRINT,
    );
    // The cells it will actually occupy.
    let (cx, _) = emerge_core::grid::cells(w);
    let (cz, _) = emerge_core::grid::cells(d);
    gizmos.rect(
        Isometry3d::new(
            STAGE + Vec3::new(0.0, 0.01, 0.0),
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        ),
        Vec2::new(cx as f32 * emerge_core::grid::SNAP, cz as f32 * emerge_core::grid::SNAP),
        CELLS,
    );
    // And the volume, so height is visible rather than only stated.
    //
    // **Anchored on `STAGE`**, like the two rectangles above. This read `from_xyz(0.0, ..)`, which is
    // the world origin — where the *map* is, not where the tile is staged — so the one gizmo that
    // shows a piece's height was drawn 4 km from the piece and nobody had ever seen it.
    if height > 0.0 {
        gizmos.cube(
            Transform::from_translation(STAGE + Vec3::new(0.0, height * 0.5, 0.0))
                .with_scale(Vec3::new(w, height, d)),
            EXTENT,
        );
    }
}

/// The two one-line readouts. Cheap enough every frame, and guarded so they only write on change.
fn refresh_lines(
    state: Res<ImportState>,
    mut summaries: Query<&mut Text, (With<ScanSummary>, Without<ActionLine>)>,
    mut actions: Query<&mut Text, (With<ActionLine>, Without<ScanSummary>)>,
) {
    for mut t in &mut summaries {
        if t.0 != state.summary {
            t.0 = state.summary.clone();
        }
    }
    for mut t in &mut actions {
        if t.0 != state.status {
            t.0 = state.status.clone();
        }
    }
}

/// Rebuild the candidate list.
///
/// Wholesale rather than diffed: it changes on a rescan and on nothing else, and a diffing rebuild of
/// a 319-row list would be more code than the thing it saves.
fn rebuild_candidates(
    mut commands: Commands,
    state: Res<ImportState>,
    project: Res<Project>,
    filters: Res<crate::filter::Filters>,
    lists: Query<Entity, With<CandidateList>>,
) {
    // **The counts are of what is SHOWN.** A heading reading 318 above a filtered list of four is a
    // heading lying about the thing directly under it — and the count is the one number that says
    // whether you have seen the end of the list, which is why it is here at all.
    let pane = crate::filter::Pane::Candidates;
    let in_library = project
        .library
        .descriptors
        .iter()
        .filter(|d| filters.keeps(pane, &d.id))
        .count();
    let not_imported = state
        .candidates
        .iter()
        .filter(|c| filters.keeps(pane, &c.mesh))
        .count();

    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|p| {
            // **What is already a tile**, above what could become one. Both halves are "configuring
            // the tiles", and an editor that can add but not remove makes a mistyped import permanent.
            p.spawn((
                Text::new(format!("IN LIBRARY  ({in_library})")),
                TextColor(LABEL),
                TextFont::from_font_size(9.0),
            ));
            for d in project
                .library
                .descriptors
                .iter()
                .filter(|d| filters.keeps(pane, &d.id))
            {
                let selected = state.selected_library_id.as_deref() == Some(d.id.as_str());
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    LibraryRow(d.id.clone()),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(if selected { ROW_SELECTED } else { ROW_BG }),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new(d.id.clone()),
                        TextColor(TEXT),
                        TextFont::from_font_size(10.0),
                    ));
                });
            }

            p.spawn((
                Text::new(format!("NOT YET IMPORTED  ({not_imported})")),
                TextColor(LABEL),
                TextFont::from_font_size(9.0),
                Node {
                    margin: UiRect::top(Val::Px(6.0)),
                    ..default()
                },
            ));
            if state.candidates.is_empty() {
                p.spawn((
                    Text::new(if state.scanned {
                        "every mesh under assets/ is already in the library"
                    } else {
                        "press Tab to scan"
                    }),
                    TextColor(DIM),
                    TextFont::from_font_size(11.0),
                ));
                return;
            }
            // **Grouped by pack.** 319 rows is a list you scroll past; grouped by where they came
            // from it is a dozen headings, and an author importing a kit wants that kit rather than
            // an alphabet.
            //
            // The directory, not `kind` — a candidate has no `kind` yet, that being the thing import
            // is FOR. The folder an artist put it in is the only categorisation that exists before
            // anyone has looked at it, and it is usually the right one.
            for (pack, mut members) in packs(&state.candidates) {
                // Narrowed, never reordered — the same rule the palette follows.
                members.retain(|ix| {
                    state
                        .candidates
                        .get(*ix)
                        .is_some_and(|c| filters.keeps(pane, &c.mesh))
                });
                // A pack heading with nothing under it is a heading about nothing.
                if members.is_empty() {
                    continue;
                }
                let folded = state.folded_packs.contains(&pack);
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    PackHeader(pack.clone()),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(6.0),
                        margin: UiRect::top(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(HEADER_BG),
                ))
                .with_children(|row| {
                    row.spawn((
                        Node {
                            width: Val::Px(10.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(if folded { ">" } else { "v" }),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                    ));
                    row.spawn((
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        Text::new(pack.clone()),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                    ));
                    row.spawn((
                        Text::new(format!("{}", members.len())),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                    ));
                });
                if folded {
                    continue;
                }
                for ix in members {
                let Some(c) = state.candidates.get(ix) else {
                    continue;
                };
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    CandidateRow(ix),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        flex_direction: FlexDirection::Row,
                        ..default()
                    },
                    BackgroundColor(if ix == state.selected {
                        ROW_SELECTED
                    } else {
                        ROW_BG
                    }),
                ))
                .with_children(|row| {
                    // The severity mark first, so a list of 300 can be skimmed for the ones that
                    // need attention rather than read.
                    row.spawn((
                        Node {
                            width: Val::Px(14.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(match c.worst() {
                            Some(Severity::Blocking) => "x",
                            Some(Severity::Warn) => "!",
                            _ => "",
                        }),
                        TextColor(match c.worst() {
                            Some(Severity::Blocking) => DANGER,
                            Some(Severity::Warn) => ACCENT,
                            _ => LABEL,
                        }),
                        TextFont::from_font_size(11.0),
                    ));
                    row.spawn((
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        // The file's own name, not the full path — the pack heading already said
                        // where it came from, and repeating it on 145 rows is the same word 145 times.
                        Text::new(leaf(&c.mesh)),
                        TextColor(TEXT),
                        TextFont::from_font_size(10.0),
                    ));
                });
                }
            }
        });
    }
}

/// Rebuild the detail for whichever candidate is selected.
fn rebuild_detail(
    mut commands: Commands,
    state: Res<ImportState>,
    cell_edit: Res<CellEdit>,
    note_edit: Res<NoteEdit>,
    scale_edit: Res<ScaleEdit>,
    project: Res<Project>,
    panes: Query<Entity, With<DetailPane>>,
) {
    for pane in &panes {
        commands.entity(pane).despawn_related::<Children>();
        commands.entity(pane).with_children(|p| {
            // **The pane follows the focus, not the candidate list.** It used to return here unless a
            // candidate was selected, which is why an accepted tile's lattice could only be reached
            // by hand-editing `library.ron`.
            // **Both layers, because this pane shows both things.** `d` is the measurement — the
            // MEASURED block below is about exactly that, and the id and note it displays are the
            // ones an edit would be written back to. `placed` is the same piece as it will stand,
            // which is the only honest source for the lattice's shape: see [`ImportState::placed`].
            //
            // One guard for the pair. They are `Some` together or `None` together — the layered
            // library is derived from the measurements and `Policy::apply` patches entries without
            // renaming them — so a second `else` arm here would be a branch nothing can reach.
            let (Some(d), Some(placed)) = (state.editing(&project.measured), state.placed(&project))
            else {
                return;
            };
            // The candidate behind the focus, when the focus IS a candidate. `measured` and the
            // findings are import measurement — a library entry has no such thing, and showing an
            // empty MEASURED block for one would be inventing a fact.
            let cand = match state.selected_library_id {
                Some(_) => None,
                None => state.current(),
            };

            // The id, showing what is being typed when it is being typed — with a caret, so an
            // empty field reads as "waiting for you" rather than as the id having been wiped.
            let (id_text, id_tint) = match &state.renaming {
                Some(raw) => (
                    format!("id  {}_", emerge_core::naming::to_snake_case(&raw.raw)),
                    ACCENT,
                ),
                None => (format!("id  {}", d.id), TEXT),
            };
            // **The headline.** It is the thing being named, so it is the only 13 px line in the
            // block — hierarchy by size, so the eye lands here first without reading anything.
            p.spawn((
                Text::new(id_text),
                TextColor(id_tint),
                TextFont::from_font_size(13.0),
                Node {
                    margin: UiRect::bottom(Val::Px(crate::chrome::GAP_ROW)),
                    ..default()
                },
            ));

            // **The description.** `Descriptor::note` is free text and nothing could write it before,
            // so every description in the shipped libraries is whatever a generator left there. The id
            // says what a piece *is* and the tags say what it *offers*; neither can carry "the one
            // with the cracked screen", which is the sort of thing a later reader — human or model —
            // needs to tell two crates apart.
            let (note_text, note_tint) = match &note_edit.active {
                Some((_, raw)) => (format!("{raw}_"), ACCENT),
                None => match d.note.as_deref() {
                    Some(n) if !n.is_empty() => (n.to_owned(), TEXT),
                    _ => ("describe it\u{2026}".to_owned(), LABEL),
                },
            };
            p.spawn((
                UiButton,
                Hovered::default(),
                NoteField,
                Node {
                    // Stated, because the text is empty until somebody types — an unstated height
                    // lays this out at 7 logical px (`docs/ui.md` §5).
                    min_height: Val::Px(18.0),
                    padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                    margin: UiRect::bottom(Val::Px(crate::chrome::GAP_ROW)),
                    ..default()
                },
                BackgroundColor(ROW_BG),
            ))
            .with_children(|f| {
                f.spawn((
                    Text::new(note_text),
                    TextColor(note_tint),
                    TextFont::from_font_size(10.0),
                    NoteReadout,
                ));
            });

            if let Some((c, m)) = cand.and_then(|c| c.measured.map(|m| (c, m))) {
                // Measured facts, not controls. Given their own heading so the eye can skip them
                // when it is looking for something to click.
                crate::chrome::section(p, "MEASURED");
                // **As the piece will stand, not as the file happens to store it.**
                //
                // These come from `proposed.extent` rather than from the raw `Measured`, because a
                // rotation is baked into the extent at import (`import::remeasure_rotated`) and the
                // raw measurement is pre-rotation. Reading the file's own numbers here meant a
                // barrel turned on its side still reported `cells 1 x 1` while it occupied 1 x 2 —
                // and `size` and `cells` are the same fact twice, so rotating one without the other
                // would leave the block contradicting itself.
                //
                // No re-derivation: the extent was already rotated and validated when it was
                // written, so this reads it rather than computing a second answer that could differ.
                let (fw, fd) = c.proposed.extent.footprint.unwrap_or(m.footprint);
                let fh = c.proposed.extent.height.unwrap_or(m.height);
                let (cells_x, _) = emerge_core::grid::cells(fw);
                let (cells_z, _) = emerge_core::grid::cells(fd);
                // Shown only when there is one, so the common case stays three plain rows and a
                // turned piece explains why its numbers differ from the file's.
                let turned = c
                    .proposed
                    .align
                    .rotate
                    .map(|(x, y, z)| format!("{x},{y},{z} deg"));
                for (label, value) in [
                    ("size", format!("{fw:.2} x {fh:.2} x {fd:.2} m")),
                    ("cells", format!("{cells_x} x {cells_z}")),
                    ("tris", format!("{}", c.triangles)),
                    (
                        "front",
                        match c.proposed.align.front {
                            Some(face) => format!("{} face", face.label()),
                            None => "none".to_owned(),
                        },
                    ),
                ]
                .into_iter()
                .chain(turned.map(|t| ("turned", t)))
                {
                    p.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        margin: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: Val::Px(48.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            Text::new(label),
                            TextColor(LABEL),
                            TextFont::from_font_size(10.0),
                        ));
                        row.spawn((
                            Text::new(value),
                            TextColor(TEXT),
                            TextFont::from_font_size(11.0),
                        ));
                    });
                }
            }

            // **The width this piece stands at.**
            //
            // Outside the MEASURED block above on purpose: that block is candidates-only, because a
            // measurement is an import fact and a library entry has none — but a *size* is editable
            // for both, and the tiles an author most wants to re-proportion are the ones already in
            // the library.
            //
            // Read off `d`, the measurement layer, which is the same layer this field writes to and
            // the same call `on_note_click` makes. Reading the layered `placed` here would show a
            // width a project patch supplied and then write the author's answer one level below it.
            crate::chrome::section(p, "SIZE (m)");
            let measured_w = d.extent.footprint.map(|(w, _)| w);
            // Not `placed` — that name is already the layered *descriptor* in this scope, and
            // shadowing it here would have handed the lattice code below a footprint tuple.
            let placed_fp = emerge_core::descriptor::placed_footprint(d);
            let (width_text, width_tint) = match &scale_edit.active {
                Some((_, raw)) => (format!("{raw}_"), ACCENT),
                None => match placed_fp {
                    Some((w, _)) => (format!("{w:.2}"), TEXT),
                    None => ("--".to_owned(), LABEL),
                },
            };
            // What the number means, spelled out rather than left as a ratio to work back to: the
            // footprint it produces and the multiplier it stores. `docs/ui.md` §1.2 — more
            // information measured *better*, so the derivation is shown rather than hidden.
            let width_note = match (placed_fp, measured_w) {
                (Some((w, dep)), Some(mw)) if mw > 0.0 => {
                    format!("  {w:.2} x {dep:.2} m at {:.3}x", w / mw)
                }
                _ => "  no measured footprint to scale".to_owned(),
            };
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                margin: UiRect::bottom(Val::Px(crate::chrome::GAP_ROW)),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    UiButton,
                    Hovered::default(),
                    ScaleField,
                    Node {
                        width: Val::Px(62.0),
                        // Stated, because the text is empty the moment somebody clicks in — an
                        // unstated height lays this out at 7 logical px (`docs/ui.md` §5).
                        min_height: Val::Px(18.0),
                        padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(ROW_BG),
                ))
                .with_children(|f| {
                    f.spawn((
                        Text::new(width_text),
                        TextColor(width_tint),
                        TextFont::from_font_size(11.0),
                        ScaleReadout,
                    ));
                });
                row.spawn((
                    Text::new(width_note),
                    TextColor(LABEL),
                    TextFont::from_font_size(10.0),
                ));
            });

            // **The mount.** It is what replaced `Role`, `rests_on` and the height heuristic that
            // once decided a 10.9 cm mug was a floor decal — so it is the one field worth putting on
            // its own line rather than in a list of tags.
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                margin: UiRect::top(Val::Px(crate::chrome::GAP_GROUP)),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Node {
                        width: Val::Px(48.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    // **"mount", not "layer".** The subgrid below has its own `layer y` picker, and
                    // one panel saying "layer" twice about two different things is the confusion the
                    // key census already fixed on its side (`Action::CycleMount`).
                    Text::new("mount"),
                    TextColor(LABEL),
                    TextFont::from_font_size(10.0),
                ));
                row.spawn((
                    Text::new(mount_label(d.mount.as_ref())),
                    TextColor(if d.mount.is_some() { TEXT } else { ACCENT }),
                    TextFont::from_font_size(11.0),
                ));
            });

            // A piece whose size is not measured yet has no derivable lattice, and the honest thing
            // is to say which piece and why rather than draw an empty grid that looks authored.
            let div = match project.divisions_of(placed) {
                Ok(div) => div,
                Err(why) => {
                    crate::chrome::section(p, "SUBGRID");
                    p.spawn((
                        Text::new(why),
                        TextColor(ACCENT),
                        TextFont::from_font_size(9.0),
                    ));
                    return;
                }
            };
            let (dx, dy, dz) = div;
            let empty = emerge_core::descriptor::Subgrid::default();
            let grid = d.subgrid.as_ref().unwrap_or(&empty);
            let marked = grid.cells.len();

            // **The subgrid.** Its shape first, because it frames every cell below it.
            crate::chrome::section(p, "SUBGRID");
            // **Derived, so it reads rather than edits.** The divisions come from this piece's own
            // size and the project's `divisions`, which is what lets an edge token on a 3 m wall
            // mean the same thing as one on a 0.5 m chair. The subunit's size in millimetres is
            // there because that is the number an author placing a token actually needs.
            let subunit_mm = emerge_core::grid::SNAP / project.policy.divisions as f32 * 1000.0;
            p.spawn((
                Text::new(format!("{dx} x {dy} x {dz} cells of {subunit_mm:.0} mm")),
                TextColor(TEXT),
                TextFont::from_font_size(11.0),
                DivReadout,
            ));
            p.spawn((
                Text::new(format!(
                    "{marked} of {} marked — {} division(s) per {:.1} m tile, from project.ron",
                    emerge_core::descriptor::Subgrid::volume(div),
                    project.policy.divisions,
                    emerge_core::grid::SNAP,
                )),
                TextColor(DIM),
                TextFont::from_font_size(9.0),
                Node {
                    margin: UiRect::top(Val::Px(crate::chrome::GAP_TIGHT)),
                    ..default()
                },
            ));

            // **Every layer at once, side by side, bottom on the left.** A picker showed one slice
            // and hid the other two, so reading a shape meant clicking through it and holding the
            // rest in your head — and a lattice is a 3D shape, which is the one thing a single slice
            // cannot show.
            //
            // Side by side rather than stacked because three 3x3 grids in a column is a tall thin
            // strip the eye has to scan, while three in a row is one picture. Bottom on the left, and
            // the labels say so rather than leaving `y = 0` to be inferred.
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexStart,
                column_gap: Val::Px(crate::chrome::GAP_GROUP),
                // Wraps rather than overflowing: a finer lattice, or a narrower panel, puts the last
                // layer on a second line instead of off the edge.
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(crate::chrome::GAP_ROW),
                margin: UiRect::top(Val::Px(crate::chrome::GAP_ROW)),
                ..default()
            })
            .with_children(|layers| {
                for y in 0..dy {
                    layers
                        .spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(2.0),
                            ..default()
                        })
                        .with_children(|col| {
                            col.spawn((
                                Text::new(layer_label(y, dy)),
                                TextColor(LABEL),
                                TextFont::from_font_size(9.0),
                            ));

                            // The column headers, with the layer header in the corner above the row
                            // ones — the spreadsheet arrangement, because that is where a hand
                            // already looks for them.
                            col.spawn(Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(crate::chrome::GAP_TIGHT),
                                ..default()
                            })
                            .with_children(|row| {
                                header_button(row, FillHeader { layer: y, span: Span::Layer }, "*");
                                for x in 0..dx {
                                    header_button(
                                        row,
                                        FillHeader { layer: y, span: Span::Column(x) },
                                        "v",
                                    );
                                }
                            });

                            for z in 0..dz {
                                col.spawn(Node {
                                    flex_direction: FlexDirection::Row,
                                    column_gap: Val::Px(crate::chrome::GAP_TIGHT),
                                    ..default()
                                })
                                .with_children(|row| {
                                    header_button(
                                        row,
                                        FillHeader { layer: y, span: Span::Row(z) },
                                        ">",
                                    );
                                    for x in 0..dx {
                                        let at = (x, y, z);
                                        let cell = grid.at(at);
                                        let selected = cell_edit.at == Some(at);
                                        row.spawn((
                                            UiButton,
                                            Hovered::default(),
                                            CellButton(x, z),
                                            CellLayer(y),
                                            Node {
                                                min_width: Val::Px(20.0),
                                                min_height: Val::Px(18.0),
                                                justify_content: JustifyContent::Center,
                                                align_items: AlignItems::Center,
                                                ..default()
                                            },
                                            BackgroundColor(if selected {
                                                ROW_SELECTED
                                            } else {
                                                ROW_BG
                                            }),
                                        ))
                                        .with_children(|b| {
                                            b.spawn((
                                                Text::new(cell_glyph(cell).to_owned()),
                                                // A marked cell is brighter than an empty one —
                                                // luminance, not hue, per `docs/ui.md` §1.3.
                                                TextColor(if cell.is_some() { ACCENT } else { LABEL }),
                                                TextFont::from_font_size(11.0),
                                                CellGlyph(x, z),
                                                CellLayer(y),
                                            ));
                                        });
                                    }
                                });
                            }
                        });
                }
            });

            // What the selected cell is, and the verbs that change it. Each chip states its own
            // job — `docs/ui.md` §4.2's rule that a verb is clickable and named.
            let detail = match cell_edit.at {
                Some(at) => match &cell_edit.active {
                    Some((field, raw)) => format!(
                        "{},{},{}  {} `{raw}_`",
                        at.0,
                        at.1,
                        at.2,
                        if *field == CellField::Edge { "edge" } else { "anchor" }
                    ),
                    None => format!(
                        "{},{},{}  {}",
                        at.0,
                        at.1,
                        at.2,
                        grid.at(at).map(describe_cell).unwrap_or_else(|| "open".to_owned())
                    ),
                },
                None => "no cell picked".to_owned(),
            };
            p.spawn((
                Text::new(detail),
                TextColor(if cell_edit.active.is_some() { ACCENT } else { DIM }),
                TextFont::from_font_size(9.0),
                SelectedCellLine,
                Node {
                    margin: UiRect::top(Val::Px(crate::chrome::GAP_ROW)),
                    ..default()
                },
            ));
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(crate::chrome::GAP_TIGHT),
                row_gap: Val::Px(crate::chrome::GAP_TIGHT),
                margin: UiRect::top(Val::Px(crate::chrome::GAP_TIGHT)),
                ..default()
            })
            .with_children(|chips| {
                for verb in [CellVerb::Solid, CellVerb::Edge, CellVerb::Anchor, CellVerb::Clear] {
                    let on = matches!(
                        (verb, cell_edit.at.and_then(|a| grid.at(a))),
                        (CellVerb::Solid, Some(c)) if c.solid
                    );
                    chips
                        .spawn((
                            UiButton,
                            Hovered::default(),
                            verb,
                            Node {
                                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(if on { ROW_SELECTED } else { ROW_BG }),
                        ))
                        .with_children(|chip| {
                            chip.spawn((
                                Text::new(verb.label().to_owned()),
                                TextColor(if verb == CellVerb::Clear { DANGER } else { TEXT }),
                                TextFont::from_font_size(10.0),
                            ));
                        });
                }

                // **Turning the mesh sits beside the lattice, because it reshapes it.** A quarter
                // turn about X or Z swaps the piece's height with a floor axis, so the grid above
                // changes shape — putting these anywhere else would hide the cause of that.
                for axis in [RotateAxis::X, RotateAxis::Y, RotateAxis::Z] {
                    chips
                        .spawn((
                            UiButton,
                            Hovered::default(),
                            axis,
                            Node {
                                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                                margin: UiRect::left(Val::Px(crate::chrome::GAP_ROW)),
                                ..default()
                            },
                            BackgroundColor(ROW_BG),
                        ))
                        .with_children(|chip| {
                            chip.spawn((
                                Text::new(axis.label()),
                                TextColor(LABEL),
                                TextFont::from_font_size(10.0),
                            ));
                        });
                }

                // **Occupancy from the mesh, on its own chip.** Nobody hand-marks a lattice this
                // size, so the cells have to come off the geometry — but this is a button and never
                // runs on import, because it overwrites hand-authored cells and an author who tuned
                // a lattice must not lose it to re-importing.
                chips
                    .spawn((
                        UiButton,
                        Hovered::default(),
                        ScanMeshButton,
                        Node {
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                            margin: UiRect::left(Val::Px(crate::chrome::GAP_ROW)),
                            ..default()
                        },
                        BackgroundColor(ROW_BG),
                    ))
                    .with_children(|chip| {
                        chip.spawn((
                            Text::new("rescan mesh"),
                            TextColor(ACCENT),
                            TextFont::from_font_size(10.0),
                        ));
                    });
            });

            // Tag chips, one row per axis. Every token the project has, lit when this piece carries
            // it — so an author sees the whole vocabulary rather than having to remember it, which is
            // the difference between a closed vocabulary being a help and being an obstacle.
            for axis in [Axis::Kind, Axis::Effects, Axis::Look, Axis::Surfaces] {
                let vocab = axis.tokens(&project.vocab);
                if vocab.tokens.is_empty() {
                    continue;
                }
                let held: Vec<String> = match axis {
                    Axis::Kind => d.kind.clone(),
                    Axis::Effects => d.effects.clone(),
                    Axis::Look => d.look.clone(),
                    Axis::Surfaces => d.offers.surfaces.clone(),
                };
                crate::chrome::section(p, axis.label());
                p.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(crate::chrome::GAP_TIGHT),
                    row_gap: Val::Px(crate::chrome::GAP_TIGHT),
                    ..default()
                })
                .with_children(|chips| {
                    for (ix, name) in vocab.names().enumerate() {
                        let on = held.iter().any(|h| h == name);
                        chips
                            .spawn((
                                UiButton,
                                Hovered::default(),
                                TagChip { axis, token: ix },
                                Node {
                                    // A chip is a click target, and 1 px of vertical padding made a
                                    // row of them a solid bar of text rather than a row of things.
                                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(if on { ROW_SELECTED } else { ROW_BG }),
                            ))
                            .with_children(|chip| {
                                chip.spawn((
                                    Text::new(name.to_owned()),
                                    TextColor(if on { TEXT } else { LABEL }),
                                    TextFont::from_font_size(10.0),
                                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                                ));
                            });
                    }
                });
            }

            // **What the importer noticed**, one block per finding.
            //
            // This was a bare run of `Text` nodes: every message in its severity's colour, and the
            // remedy prefixed with three literal spaces. Three spaces indent the *first* line only,
            // so the moment a remedy wrapped — which at this width is always — its continuation went
            // flush left and ran into the next finding. Nothing said where one finding ended and the
            // next began, and a whole paragraph in DANGER red is a paragraph nobody reads twice.
            //
            // So: a coloured rail down the left groups a message with its own remedy, the severity is
            // a *word* rather than only a hue, and the prose is plain `TEXT` — colour locates the
            // thing, it does not shout it. `docs/ui.md` §1.2 (Vicente & Rasmussen): the test is "does
            // this force interpretation?", and the fix for a crowded panel is grouping and spacing
            // rather than deleting readouts.
            let findings: Vec<_> = cand.iter().flat_map(|c| c.findings.iter()).collect();
            if !findings.is_empty() {
                crate::chrome::section(p, "FINDINGS");
                p.spawn((
                    Text::new("what the importer noticed about this mesh"),
                    TextColor(DIM),
                    TextFont::from_font_size(9.0),
                    Node {
                        margin: UiRect::bottom(Val::Px(crate::chrome::GAP_ROW)),
                        ..default()
                    },
                ));
                for f in findings {
                    let (tint, word) = match f.severity {
                        Severity::Blocking => (DANGER, "blocking"),
                        Severity::Warn => (ACCENT, "worth checking"),
                        Severity::Note => (DIM, "note"),
                    };
                    p.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            border: UiRect::left(Val::Px(2.0)),
                            padding: UiRect::left(Val::Px(7.0))
                                .with_top(Val::Px(crate::chrome::GAP_TIGHT))
                                .with_bottom(Val::Px(crate::chrome::GAP_TIGHT)),
                            margin: UiRect::bottom(Val::Px(crate::chrome::GAP_ROW)),
                            ..default()
                        },
                        BorderColor::all(tint),
                    ))
                    .with_children(|block| {
                        block.spawn((
                            Text::new(word),
                            TextColor(tint),
                            TextFont::from_font_size(9.0),
                        ));
                        block.spawn((
                            Text::new(f.message.clone()),
                            TextColor(TEXT),
                            TextFont::from_font_size(10.0),
                            // Wrapped prose at 10 px needs the leading; the 1.2 default packs these
                            // into the block of text the screenshot showed.
                            bevy::text::LineHeight::RelativeToFont(1.35),
                        ));
                        // The remedy, under what it fixes. A warning with no answer is a warning read
                        // once.
                        if let Some(fix) = &f.fix {
                            block.spawn((
                                Text::new(fix.clone()),
                                TextColor(LABEL),
                                TextFont::from_font_size(10.0),
                                bevy::text::LineHeight::RelativeToFont(1.35),
                                Node {
                                    margin: UiRect::top(Val::Px(3.0)),
                                    ..default()
                                },
                            ));
                        }
                    });
                }
            }
        });
    }
}

/// **What `write_library` writes**, which is the whole of the kit-corruption fix.
///
/// The defect these pin: `write_library` used to serialize the *layered* library over
/// `library_path`, so under `--kit site` — whose `project.ron` stretches walls to a 2.40 m facility
/// — toggling one lattice cell wrote that facility's wall height into the measurements file the kit
/// exists to share, and the next load applied the patch again on top of it.
/// **The `SIZE (m)` field's arithmetic.** See [`scale_for_width`].
#[cfg(test)]
mod scale_field_tests {
    use super::*;

    /// **The trap this function exists to avoid.** Typing back the width already on screen must
    /// change nothing — which is only true because the divisor is the *measured* width, not the
    /// placed one. Divide by the placed width and every commit rescales what was already rescaled, so
    /// setting 0.5 twice silently gives 0.25 with the field reading `0.50` both times.
    #[test]
    fn a_width_that_is_already_set_is_a_no_op() {
        let measured = 1.0_f32;
        // First edit: 1.0 m mesh asked to stand 0.6 m.
        let once = scale_for_width(measured, 0.6)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("0.6 is not unity"));
        assert!((once - 0.6).abs() < 1e-6, "{once}");

        // The field now reads the PLACED width, 0.6. Committing that again must land on the same
        // scale rather than on 0.36.
        let placed = measured * once;
        let twice = scale_for_width(measured, placed)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("still not unity"));
        assert!((twice - once).abs() < 1e-6, "committing the shown width moved it: {twice}");
    }

    /// Unity is stored as absence. A `Some(1.0)` in `library.ron` would be an authored fact where
    /// there is none, and every reader would then multiply by it — the rule `rotate_mesh` follows
    /// when it refuses to write an identity rotation.
    #[test]
    fn returning_to_the_measured_size_clears_the_scale() {
        assert_eq!(scale_for_width(1.0, 1.0), Ok(None));
        assert_eq!(scale_for_width(0.306, 0.306), Ok(None));
        // And a hair off unity is still unity, rather than a scale of 1.00001.
        assert_eq!(scale_for_width(1.0, 1.0 + SCALE_EPS / 2.0), Ok(None));
    }

    /// A width that cannot be a size is refused by name, not clamped. A zero footprint overlaps
    /// nothing, so every placement rule would report success while the piece sat inside a wall.
    #[test]
    fn a_width_that_is_not_a_size_is_refused() {
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                scale_for_width(1.0, bad).is_err(),
                "{bad} must be refused, not stored"
            );
        }
        // And an unmeasurable mesh cannot produce a ratio at all.
        assert!(scale_for_width(0.0, 0.6).is_err());
    }

    /// The shipped case, end to end: `site/books` is 0.306 m wide and stands at 0.6x, so an author
    /// typing `0.1836` reproduces exactly the scale that is in the file today.
    #[test]
    fn the_one_shipped_scaled_piece_round_trips() {
        let measured = 0.306_f32;
        let placed = measured * 0.6;
        let scale = scale_for_width(measured, placed)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("0.6 is not unity"));
        assert!((scale - 0.6).abs() < 1e-5, "{scale}");
    }
}

#[cfg(test)]
mod write_library_tests {
    use super::*;
    use emerge_core::descriptor::{Align, Descriptor, Extent};
    use emerge_core::library::{Library, LIBRARY_VERSION};
    use emerge_core::policy::{Match, Patch, Policy};

    /// The mesh is authored at 1.00 m; this facility builds its walls to 2.40 m.
    const AUTHORED_HEIGHT: f32 = 1.0;
    const STRETCH: f32 = 2.4;

    fn wall() -> Descriptor {
        Descriptor {
            id: "wall".into(),
            mesh: Some("wall.glb".into()),
            extent: Extent {
                footprint: Some((3.0, 0.5)),
                height: Some(AUTHORED_HEIGHT),
            },
            mount: Some(emerge_core::descriptor::Mount::OnFloor),
            ..Descriptor::default()
        }
    }

    fn stretching_policy() -> Policy {
        Policy {
            patches: vec![Patch {
                // By id rather than kind, so these stay about the layering and do not need a
                // vocabulary to hold a token.
                matches: Match::Id("wall".into()),
                because: "this facility builds walls to 2.40 m".into(),
                patch: Descriptor {
                    align: Align {
                        stretch_y: Some(STRETCH),
                        ..Align::default()
                    },
                    ..Descriptor::default()
                },
            }],
            ..Policy::default()
        }
    }

    /// A project in a temp dir, opened the way the editor opens one.
    fn project_in(dir: &std::path::Path, policy: Policy) -> Project {
        let measured = Library {
            version: LIBRARY_VERSION,
            note: None,
            descriptors: vec![wall()],
        };
        let library = policy.apply(&measured).unwrap_or_else(|e| panic!("{e}"));
        Project {
            root: dir.to_path_buf(),
            emerge_dir: dir.to_path_buf(),
            library_path: dir.join("library.ron"),
            vocab: emerge_core::vocab::Vocabularies::default(),
            measured,
            library,
            policy,
            masks: Vec::new(),
            map: emerge_core::map::Map {
                name: "t".into(),
                ..emerge_core::map::Map::default()
            },
            map_path: dir.join("t.map.ron"),
            dirty: false,
            triangles: vec![0],
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("emerge_mapper_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        dir
    }

    /// **The bug, end to end.** Editing a lattice under a stretching policy must leave the authored
    /// height in the file, not the facility's.
    #[test]
    fn the_policy_layer_never_reaches_the_measurements_file() {
        let dir = temp_dir("policy_leak");
        let mut project = project_in(&dir, stretching_policy());

        // The layered view really is stretched — otherwise this test proves nothing.
        assert_eq!(
            project.library.get("wall").and_then(|d| d.align.stretch_y),
            Some(STRETCH),
            "the policy must actually apply, or this test cannot fail"
        );

        project
            .measured
            .descriptors[0]
            .lattice_mut()
            .set_solid((0, 0, 0), (6, 2, 1))
            .unwrap_or_else(|| panic!("in range"));
        write_library(&mut project).unwrap_or_else(|e| panic!("{e}"));

        let written = std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));
        let back = Library::parse(&written).unwrap_or_else(|e| panic!("{e}"));
        let wall = back.get("wall").unwrap_or_else(|| panic!("wall survives"));
        assert_eq!(
            wall.extent.height,
            Some(AUTHORED_HEIGHT),
            "the measurements file must keep the authored height, not this facility's"
        );
        assert_eq!(
            wall.align.stretch_y, None,
            "`stretch_y` is this project's architecture and belongs in project.ron alone"
        );
        assert!(
            wall.subgrid.as_ref().is_some_and(|g| g.cells.len() == 1),
            "the lattice edit itself must still have been written"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **An import reaches the file, or the author is told it did not.**
    ///
    /// `commit_candidate` pushed the new descriptor onto `project.library` — the *derived* layer — and
    /// then called `write_library`, which serializes `measured` and rebuilds `library` from it. So the
    /// file was rewritten byte-identical, the palette never gained the piece, and the status line said
    /// *"added `crate_b` — it is in the palette now"*. Nothing here exercised that path, which is why
    /// it shipped.
    ///
    /// Driven through `commit_measured` rather than through the observer, because the defect was the
    /// *destination* of the write and that is what this pins. Which key reaches it is
    /// `crates/emerge-mapper/tests/headless.rs`' concern.
    #[test]
    fn an_accepted_import_lands_in_the_measurements_file() {
        let dir = temp_dir("import_lands");
        let mut project = project_in(&dir, stretching_policy());

        let mut trial = project.measured.clone();
        trial.descriptors.push(Descriptor {
            id: "crate_b".into(),
            mesh: Some("crate_b.glb".into()),
            extent: Extent {
                footprint: Some((0.5, 0.5)),
                height: Some(0.5),
            },
            mount: Some(emerge_core::descriptor::Mount::OnFloor),
            ..Descriptor::default()
        });
        commit_measured(&mut project, trial).unwrap_or_else(|e| panic!("{e}"));

        let written =
            std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));
        let back = Library::parse(&written).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            back.get("crate_b").is_some(),
            "the import has to be IN the file that was written:\n{written}"
        );
        // And in both live layers, so the palette shows it without a reload.
        assert!(project.measured.get("crate_b").is_some(), "measured");
        assert!(project.library.get("crate_b").is_some(), "the derived palette");
        // The piece it was added beside is untouched, and the policy still did not leak downward.
        assert_eq!(
            back.get("wall").and_then(|d| d.extent.height),
            Some(AUTHORED_HEIGHT)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The derived layer is not a place to keep anything**, and this is what that costs.
    ///
    /// The characterisation half of the fix above. `commit_candidate` and `remove_tile` both edited
    /// `project.library`, which reads exactly like editing the library — and it is a no-op, because
    /// every write rebuilds that layer from `measured`. Asserting the loss here means the next person
    /// who reaches for `project.library` finds a test that names the trap, and it also pins the
    /// direction of the layering: a "fix" that made the writer serialize `library` instead would put
    /// the facility's stretched wall heights back into the measurements file the kit exists to share.
    #[test]
    fn an_edit_to_the_derived_layer_alone_is_lost() {
        let dir = temp_dir("derived_lost");
        let mut project = project_in(&dir, stretching_policy());

        project.library.descriptors.push(Descriptor {
            id: "ghost".into(),
            extent: Extent {
                footprint: Some((0.5, 0.5)),
                height: Some(0.5),
            },
            ..Descriptor::default()
        });
        write_library(&mut project).unwrap_or_else(|e| panic!("{e}"));

        let written =
            std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            !written.contains("ghost"),
            "a write serializes `measured`; anything only in `library` cannot reach the file"
        );
        assert!(
            project.library.get("ghost").is_none(),
            "and the write rebuilds `library` from `measured`, so it is gone from memory too — which \
             is why the old code's status line could report success"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A refused write leaves the editor holding what the disk holds.**
    ///
    /// The other half of proposing rather than mutating. `Policy::apply` fails when a patch matches
    /// nothing, so removing the last piece a rule names is a reachable refusal — and the old code had
    /// already assigned `project.library` and `project.masks` before it found out.
    #[test]
    fn a_refused_commit_changes_nothing() {
        let dir = temp_dir("refused");
        let mut project = project_in(&dir, stretching_policy());
        write_library(&mut project).unwrap_or_else(|e| panic!("{e}"));
        let before = std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));

        // Removing `wall` strands the policy patch that names it.
        let mut trial = project.measured.clone();
        trial.descriptors.retain(|d| d.id != "wall");
        let err = commit_measured(&mut project, trial)
            .err()
            .unwrap_or_else(|| panic!("removing the last piece a patch names must be refused"));
        assert!(err.contains("matches no descriptor"), "{err}");

        assert!(
            project.measured.get("wall").is_some(),
            "the refusal must not have taken the piece out of the live measurements"
        );
        assert!(project.library.get("wall").is_some(), "nor out of the derived layer");
        assert_eq!(
            std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}")),
            before,
            "and the file must be byte-identical"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Re-opening must not stack the patch.** The other half of the same defect: a baked-in
    /// stretch gets multiplied again by the next load.
    #[test]
    fn a_second_write_does_not_apply_the_policy_twice() {
        let dir = temp_dir("no_double");
        let mut project = project_in(&dir, stretching_policy());

        for _ in 0..3 {
            write_library(&mut project).unwrap_or_else(|e| panic!("{e}"));
        }
        assert_eq!(
            project.library.get("wall").and_then(|d| d.align.stretch_y),
            Some(STRETCH),
            "three writes must leave one stretch, not three"
        );

        let written = std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));
        let back = Library::parse(&written).unwrap_or_else(|e| panic!("{e}"));
        let reopened = project.policy.apply(&back).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            reopened.get("wall").and_then(|d| d.align.stretch_y),
            Some(STRETCH),
            "re-opening the written file must reproduce the same layered library"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A policy that no longer matches aborts the write rather than half-doing it.** Removing the
    /// last piece a rule named is reachable, and the file must be left as it was.
    #[test]
    fn a_policy_that_stops_matching_refuses_the_write_and_leaves_the_file_alone() {
        let dir = temp_dir("abort");
        let mut project = project_in(&dir, stretching_policy());
        write_library(&mut project).unwrap_or_else(|e| panic!("{e}"));
        let before = std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));

        // The only `wall` is gone, so the rule about walls matches nothing.
        project.measured.descriptors.clear();
        let err = write_library(&mut project).err().unwrap_or_default();
        assert!(err.contains("matches no descriptor"), "{err}");

        let after = std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(before, after, "a refused write must not have touched the file");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
