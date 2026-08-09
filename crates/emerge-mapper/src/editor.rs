//! **The editing loop** — a palette, a ghost, a click, a file.
//!
//! # The ghost is the contract
//!
//! Everything an author does here goes through a preview that stands where the thing will actually
//! be. That is not polish; it is the rule `containment::cordon` states and the Site editor had to
//! learn twice: a preview drawn somewhere the piece will not end up *"is worse than no preview,
//! because it is a promise the game then breaks."* So the ghost is snapped, aimed, and lifted onto
//! its host exactly as the real placement will be.
//!
//! # Aiming happens before placing
//!
//! `[` and `]` turn the **brush**, never the last thing placed. The Site editor bound them to the
//! selection and it made rotation feel broken: placing selects, so the next `]` turned the piece you
//! had just put down while the ghost — the only thing on screen showing a facing — sat still.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::camera::visibility::ViewVisibility;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton};
use emerge_core::map::Placed;

use crate::chrome::{
    ACCENT, DANGER, DIM, HEADER_BG, LABEL, LIST_W, PANEL_BG, ROW_BG, ROW_SELECTED, SLOT_BG,
    TEXT,
};
use crate::keys::{self, Action};
use crate::project::Project;
use crate::view::{cursor_ground, MainCamera};

/// Translation snap, metres. Half a metre is the unit the kits are authored on.
const SNAP: f32 = 0.5;
/// Yaw snap, degrees.
const YAW_STEP: f32 = 15.0;

/// Where a descriptor with no `kind` goes. Named rather than hidden: an untagged piece is work to do,
/// and a palette that quietly omitted it would be a palette missing pieces.
const UNSORTED: &str = "unsorted";
/// The map's edge. Dim enough not to compete with the grid, bright enough to find.
const BOUNDS_LINE: Color = Color::srgb(0.42, 0.38, 0.30);

/// The removal marker. Red because it is the one destructive tool here, and translucent because the
/// thing it covers is the thing being asked about — an opaque marker would hide the answer.
const REMOVE_TINT: Color = Color::srgba(0.86, 0.20, 0.16, 0.38);

/// The clone tool's marker tint — cool where removal is hot, because the two rectangles make
/// opposite promises and an author reads the colour before the status line.
const CLONE_TINT: Color = Color::srgba(0.20, 0.55, 0.86, 0.32);

/// The target lock's tint — gold: neither removal's threat nor clone's copy, just "this one".
const TARGET_TINT: Color = Color::srgba(0.90, 0.72, 0.20, 0.35);
/// How far the marker floats above the floor, metres. Enough to beat z-fighting against a floor tile
/// it is lying exactly on top of, small enough to still read as flat on the ground.
const MARKER_LIFT: f32 = 0.02;
/// A press-and-release inside this many metres is a click, not a drag — so "click one piece" and
/// "drag a box" are the same gesture told apart by distance rather than by a second modifier.
const CLICK_EPS: f32 = 0.2;

/// Edge of a palette row's preview box, logical px.
const THUMB_SLOT: f32 = 30.0;

/// Scene triangle counts worth colouring. Not limits — this machine draws far more than either — but
/// the points at which an author should know what they have built.
const BUSY_SCENE: usize = 1_000_000;
const HEAVY_SCENE: usize = 5_000_000;

#[derive(Resource)]
pub struct EditorState {
    /// Index into the library — what a click would place, or `None` for **nothing armed**.
    ///
    /// This was a bare `usize`, so index 0 was always armed and "I am not placing anything" was a
    /// state the editor could not be in. There was therefore nothing for `Esc` to clear, and no way to
    /// put the cursor over the map without a piece following it — which matters most for the two tools
    /// that are *about* pieces already on the map rather than about the palette.
    ///
    /// `None` is a real answer everywhere it is read: no ghost, no placement, no highlighted row, and
    /// `BRUSH  none` on the status block.
    pub brush: Option<usize>,
    pub brush_yaw: f32,
    pub status: String,
    /// The ghost's running commentary. Written every frame; never mixed with [`Self::status`].
    pub hint: String,
    /// Monotonic counter behind generated placement ids, so two crates never share a name.
    next_id: u32,
    /// Advanced on every solve, so pressing `G` twice offers a different arrangement rather than the
    /// same one — a generator that cannot be asked again is one you have to undo to disagree with.
    seed: u64,
    /// Categories the author has folded away.
    ///
    /// A set of names rather than a per-row flag: the grouping is derived from the library every
    /// rebuild, so a flag stored on a row would be lost the moment the library changed.
    collapsed: std::collections::HashSet<String>,
    /// A pin waiting for its reason: the placement index, and what has been typed so far.
    ///
    /// `Placed::owned_because` is *a reason, never a bool*, on the schema's own argument: a bool lets
    /// "I could not be bothered" and "this is the cell block's only entrance" look identical in a
    /// diff. A canned reason supplied by the editor would be that bool wearing a sentence, so pinning
    /// asks.
    pinning: Option<(usize, String)>,
    /// The raw text being typed into the name, or `None` when not renaming.
    ///
    /// Raw, with the snake_case spelling applied for display and on commit — so a backspace undoes a
    /// keystroke rather than an underscore the transform inserted, and "Site 67" reads back as
    /// `site_67` while it is still being typed.
    renaming: Option<String>,
    /// What can be undone, most recent last. See [`Undo`].
    undo: Vec<Undo>,
    /// What has been undone and can be put back, most recent last.
    ///
    /// **Cleared by any new edit**, which is what stops a redo from replaying an operation against a
    /// map that has moved on underneath it — `Undo` addresses rows by index, so a redo across an
    /// intervening edit would put pieces back at positions that now mean something else.
    ///
    /// It is **not** cleared by changing tabs. The Tiles tab keeps its own pair (`ImportState::undo`)
    /// and neither can reach the other's, so leaving a tab is not a reason to forget what you did on
    /// it.
    redo: Vec<Undo>,
    /// Which tool the next click belongs to. See [`Tool`].
    ///
    /// **The tool here and the drag in [`RemovalDrag`] / [`MoveDrag`]**, not one struct:
    /// `rebuild_palette` runs on `resource_changed::<EditorState>`, and a drag corner written every
    /// frame would tear the whole palette down and rebuild it at frame rate. This changes twice per
    /// use; those change every frame, so they live elsewhere.
    pub tool: Tool,
}

impl EditorState {
    /// **Record an edit**, and drop anything that was waiting to be redone.
    ///
    /// One place, so no edit site can forget the second half. A redo stack that survived a new edit
    /// would replay operations against a map that had moved on under them — and `Undo` addresses rows
    /// by index, so "put these three back at 7, 8 and 9" means something different once anything else
    /// has been inserted or removed.
    fn record(&mut self, op: Undo) {
        self.undo.push(op);
        self.redo.clear();
    }

    /// Where the id mint stands — read-only, so the harness can check [`next_id_after`]'s seed
    /// really landed.
    pub fn minted(&self) -> u32 {
        self.next_id
    }
}

/// **What a click on the map does.** Exactly one of these at a time.
///
/// This was `removing: bool`. A second bool beside it for the move tool would have made "both armed"
/// and "neither armed" expressible states that mean nothing, and every reader would then have had to
/// decide which one won — two paths where the author only ever has one tool in hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Click to put the armed piece down. The default, and what every other tool returns to.
    #[default]
    Place,
    /// Click a piece to delete it; drag a box to delete everything inside it.
    Remove,
    /// Click a piece to pick it up, click again to put it down. See [`MoveDrag`].
    Move,
    /// Drag a box to take a copy of everything inside it; click to stamp the set — as many times
    /// as it is held. See [`CloneDrag`].
    Clone,
}

impl Tool {
    /// How it reads in a status line.
    fn label(self) -> &'static str {
        match self {
            Tool::Place => "placing",
            Tool::Remove => "removal mode",
            Tool::Move => "move mode",
            Tool::Clone => "clone mode",
        }
    }
}

/// The rectangle being dragged out, in map space. Deliberately its own resource — see
/// [`EditorState::tool`] for why it is not a field on the state everything else watches.
#[derive(Resource, Default)]
pub struct RemovalDrag {
    /// Where the button went down, or `None` while only hovering.
    from: Option<(f32, f32)>,
}

/// **Which placement is in hand**, under [`Tool::Move`] — by `Placed::id`, never by index.
///
/// The id is the reference for the reason `Placed::id`'s own doc gives for being a string rather than
/// an index, and a carry is exactly the window where it matters: `Cmd+Z`, `F` and `G` all fire while a
/// piece is held and all of them can insert or remove rows. A stored index would then be pointing at
/// whatever slid into that slot, and the next click would move the wrong piece — silently, because an
/// index is always valid-looking.
///
/// Its own resource for the reason [`RemovalDrag`] gives: this is read every frame by the ghost, and
/// `rebuild_palette` watches `EditorState`.
///
/// # Nothing moves until the piece is put down
///
/// Picking up records *only* the index. The placement keeps its position, its `on` and its entity for
/// the whole carry, and the cursor is previewed by the same translucent ghost that previews the brush.
/// So cancelling is `held = None` — there is no half-applied move to reverse, no entity to put back,
/// and no window in which the file on disk disagrees with the screen. The alternative, moving the row
/// as the cursor moves, would have made `Esc` a second implementation of the move that had to undo
/// exactly what the first one did.
#[derive(Resource, Default)]
pub struct MoveDrag {
    pub held: Option<String>,
}

/// The translucent red marker: the hovered piece's footprint, or the dragged rectangle.
#[derive(Component)]
struct RemovalTile;

/// The clone tool's marker — the box being dragged out, then the held set's bounds riding the
/// cursor. Its own component so the two tools' rectangles can never claim each other's colour.
#[derive(Component)]
struct CloneTile;

/// One piece of a held clone set, stored **relative to the set's anchor** so the set's internal
/// geometry — every flush edge, every offset a lamp keeps from its table's centre — survives the
/// trip exactly. Everything authored rides along: yaw, tip, lift, note, even the pin, because a
/// copied barricade is still a barricade.
#[derive(Clone)]
struct ClonePiece {
    descriptor: String,
    offset: (f32, f32),
    yaw: f32,
    tip: (u8, u8),
    lift: f32,
    note: Option<String>,
    owned: bool,
    owned_because: Option<String>,
    on: CloneHost,
}

/// What a cloned piece rests on — resolved at capture, applied at stamp.
#[derive(Clone)]
enum CloneHost {
    /// Its own layer: floor, wall, ceiling — the map answers, nothing to carry.
    Layer,
    /// Its host was caught in the same box, by index into the set — repointed to the host's fresh
    /// id at stamp, so the cloned lamp stands on the cloned table.
    InSet(usize),
    /// Its host stayed behind, so each stamp re-seats it on whatever offers the surface at the new
    /// spot — `host_under`'s answer, and a stamp with no answer refuses whole.
    Outside,
}

/// The set in hand, plus the bounds the marker shows: centre offset from the anchor and
/// half-extents, both from the pieces' own footprints at capture.
struct CloneSet {
    pieces: Vec<ClonePiece>,
    centre_off: (f32, f32),
    half: (f32, f32),
}

/// The clone tool's state: the box being dragged, or the set in hand. Its own resource for the
/// reason [`RemovalDrag`] gives — written at drag rate, and `rebuild_palette` watches
/// `EditorState`.
#[derive(Resource, Default)]
pub struct CloneDrag {
    from: Option<(f32, f32)>,
    held: Option<CloneSet>,
}

/// **Which piece of a stack the piece-verbs mean** — `H`'s answer, held as `(placement id, the
/// snapped cell it was taken on)`.
///
/// A floor tile, a wall and its header legally share a cell (different layers pass the overlap
/// rule), and "the placement under the cursor" cannot name one of three — a nudge aimed at the
/// header moved the wall. The id, never an index, for `MoveDrag`'s reason; the cell, so the lock
/// lapses the moment the cursor walks away rather than following it around the map.
#[derive(Resource, Default)]
pub struct TargetLock(Option<(String, (f32, f32))>);

/// The locked target's highlight — a third marker quad beside removal's red and clone's blue,
/// because three tools making three different promises must not share a colour.
#[derive(Component)]
struct TargetTile;

impl CloneDrag {
    /// Whether a set is in hand — the one question `keys` asks (for `Esc`'s layering).
    fn holding(&self) -> bool {
        self.held.is_some()
    }
}

/// One reversible edit.
///
/// Only placements, and deliberately: the map's *size* and *name* are settings rather than edits, and
/// folding them into the same stack would mean Ctrl+Z sometimes resized the map when an author meant
/// to take back a crate. One kind of thing in the stacks.
///
/// # Closed under inversion, which is what makes redo one mechanism instead of two
///
/// Every variant's inverse is another variant of this same enum: undoing an `Added` produces a
/// `RemovedMany` holding what it took out, undoing that produces an `Added` again, and `Moved`,
/// `Turned` and `Pinned` each invert to themselves carrying the other value. So [`apply`] does the
/// work and *returns the inverse*, and undo and redo are the same function reading opposite stacks —
/// rather than a second body that has to be kept in step with the first.
enum Undo {
    /// Remove the last `count` placements — the inverse of a place or a fill.
    Added { count: usize },
    /// Put back everything one drag took out. **Ascending by index**, which is what lets them go
    /// back in that order and each land where it came from — an earlier row returning first shifts
    /// the later ones into place. One entry for the whole rectangle, on the same argument the fill
    /// makes: a box the author drew once is one act to undo once.
    RemovedMany { items: Vec<(usize, Box<Placed>)> },
    /// Put a moved group back where it came from. **One entry for the whole group** — the piece and
    /// everything that was riding on it — on the same argument `RemovedMany` makes: one act the
    /// author performed is one act to take back. `emerge_core::stack::Moved` records what changed and
    /// `restore_moved` is its inverse, so the undo cannot drift from the move.
    Moved { moved: emerge_core::stack::Moved },
    /// Remove exactly these rows again — the inverse of [`Undo::RemovedMany`].
    ///
    /// **Not `Added { count }`.** That one removes the *last* `count`, which is right for a place or a
    /// fill because both append; but `RemovedMany` puts rows back at their original indices, which are
    /// not generally the tail. Redoing a delete through `Added` would have removed whatever happened to
    /// be at the end of the list instead.
    RemoveAt { indices: Vec<usize> },
    /// Put a placement's yaw back. Its own inverse, carrying the other angle.
    Turned { index: usize, yaw: f32 },
    /// Put a placement's authored lift back. Its own inverse, carrying the other offset — and its
    /// apply redraws the piece's dependents too, because raising a table moves the lamp.
    Lifted { index: usize, lift: f32 },
    /// Put a placement's tip back. Its own inverse, carrying the other quarter turns.
    Tipped { index: usize, tip: (u8, u8) },
    /// **Several reversals that are one act** — applied in order, inverted by reversing.
    ///
    /// `generate` needs it: undoing a `G` press must strip the solver rows AND put the removed sketch
    /// back, and two separate entries would make one keypress two undos. The group's inverse is the
    /// sub-inverses in reverse order — the standard composition rule, and what keeps the enum closed
    /// under inversion with no new mechanism.
    Group { ops: Vec<Undo> },
    /// **Remove the last `count` stamps** — the inverse of stamping a group.
    ///
    /// Its own list, not `Added`: stamps live in `map.stamps`, and a map that expands them is a
    /// different list from the one an author placed into by hand. Folding the two would make one
    /// Ctrl+Z sometimes take back a crate and sometimes a whole nurse station.
    Stamped { count: usize },
    /// Put stamps back at their original indices — the inverse of [`Undo::Stamped`], on exactly the
    /// argument [`Undo::RemovedMany`] makes about rows.
    UnstampedMany {
        items: Vec<(usize, Box<emerge_core::composition::Stamped>)>,
    },
    /// Put a placement's pin back, reason included. Its own inverse.
    Pinned {
        index: usize,
        owned: bool,
        because: Option<String>,
    },
}

impl Default for EditorState {
    fn default() -> Self {
        EditorState {
            // Armed on the first piece at startup, as it has always been — an editor that opens with
            // nothing selected makes the first click do nothing and reads as broken.
            brush: Some(0),
            brush_yaw: 0.0,
            status: String::new(),
            hint: String::new(),
            next_id: 0,
            seed: 1,
            collapsed: std::collections::HashSet::new(),
            pinning: None,
            renaming: None,
            undo: Vec::new(),
            redo: Vec::new(),
            tool: Tool::default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn label(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::Y => "Y",
            Axis::Z => "Z",
        }
    }
    fn get(self, b: (f32, f32, f32)) -> f32 {
        match self {
            Axis::X => b.0,
            Axis::Y => b.1,
            Axis::Z => b.2,
        }
    }
    fn set(self, b: &mut (f32, f32, f32), v: f32) {
        match self {
            Axis::X => b.0 = v,
            Axis::Y => b.1 = v,
            Axis::Z => b.2 = v,
        }
    }
}

/// A map may not be smaller than this on any axis, metres. One, not zero: a zero-width map has no
/// floor to point at, and `fill` would refuse every cell in it for a reason that reads like a bug.
const MIN_BOUND: u32 = 1;
/// And no larger. 512 m at 0.5 m cells is past what one fill can cover and far past what an author
/// can see; a typo of one extra digit should be refused rather than resolved into a map nobody meant.
const MAX_BOUND: u32 = 512;

/// One clickable size field.
#[derive(Component, Clone, Copy)]
struct SizeField(Axis);

/// Which size field is being typed into, and the digits so far.
///
/// Its own resource for the reason [`RemovalDrag`] is: `rebuild_palette` watches `EditorState`, and a
/// keystroke landing there would tear down and rebuild all forty-one palette rows per character.
#[derive(Resource, Default)]
pub struct SizeEdit {
    active: Option<(Axis, String)>,
}

/// The value text of one axis row, refreshed as the size changes.
#[derive(Component, Clone, Copy)]
struct SizeReadout(Axis);

/// The node the palette is rebuilt into, so collapsing a category can redraw it.
#[derive(Component)]
struct PaletteList;

/// A category header. Clicking it folds the group away.
#[derive(Component, Clone)]
struct CategoryHeader(String);

/// A palette row, carrying its library index so one observer can serve all of them.
#[derive(Component, Clone, Copy)]
struct PaletteRow(usize);

/// A spawned instance of a map placement, tagged with the id it came from.
#[derive(Component)]
pub struct Placement(pub String);

/// **The grid step the map is read on**, metres.
///
/// Shared by `generate` and `check_edges` on purpose: the learned grammar and the edge check must
/// agree about which two placements are neighbours, or the tool would report a fault between pieces
/// the solver does not think touch.
const CELL: f32 = 1.0;

/// The see-through preview of the armed brush.
#[derive(Component)]
struct Ghost;

/// Which descriptor the live ghost is showing, so it is rebuilt only when the brush changes —
/// respawning a GLB every frame would thrash the asset server and never finish loading.
#[derive(Component)]
struct GhostOf(usize);

#[derive(Component)]
struct Ghosted;

/// The readout block, so the whole thing can be found in one query.
#[derive(Component)]
struct StatusBlock;

/// The live cost readout, bottom right.
#[derive(Component)]
struct TriangleTotal;

/// One labelled row of the readout. A field per line, rather than one string with separators in it,
/// because a separator is a thing the reader has to parse and a column is not.
#[derive(Component, Clone, Copy, PartialEq)]
enum Field {
    Name,
    Brush,
    Yaw,
    Map,
    /// The last thing that happened — the only line that is prose.
    Last,
    /// **What the cursor is telling you right now**, as opposed to what just happened.
    ///
    /// Its own line because the ghost writes it EVERY FRAME while a surface piece hovers over bare
    /// floor, and sharing [`Self::Last`] meant every message an action produced was erased before it
    /// could be read. Two different questions — "what did I just do" and "why can't I place this" —
    /// so two lines.
    Hint,
    /// **Where the map disagrees with the tokens the tiles declare.**
    ///
    /// See `emerge_core::adjacency`. A standing readout rather than a message, because a fault is a
    /// state the map is in — it does not happen once and stop being true, so putting it on
    /// [`Self::Last`] would let the next action erase it.
    Edges,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Field::Edges => "EDGES",
            Field::Name => "NAME",
            Field::Brush => "BRUSH",
            Field::Yaw => "YAW",
            Field::Map => "MAP",
            Field::Last => "",
            Field::Hint => "",
        }
    }
}

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
            .init_resource::<RemovalDrag>()
            // Registered in the same commit as `drive_move` and `keys` read it — a missing `ResMut`
            // panics its system in Bevy 0.19 rather than skipping it (`CLAUDE.md`).
            .init_resource::<MoveDrag>()
            .init_resource::<CloneDrag>()
            .init_resource::<TargetLock>()
            .init_resource::<FineAnchor>()
            .init_resource::<PlaceDrag>()
            // **This plugin reads it, so this plugin registers it** (CLAUDE.md's rule, and the
            // documented 0.19 trap: every run condition is evaluated, and `rebuild_palette`'s
            // `resource_changed::<ThumbGeneration>` panics if only ThumbsPlugin — which also inits
            // it, idempotently — happens to be absent from the app.
            .init_resource::<crate::thumbs::ThumbGeneration>()
            .init_resource::<SizeEdit>()
            .init_resource::<EdgeFaults>()
            // Shared by both tabs' lists, so it is registered once here rather than by whichever
            // plugin happens to build first.
            .init_resource::<crate::filter::Filters>()
            .add_systems(
                Update,
                (
                    // Before `sense_context`, which computes `Live` from `Filters::typing` —
                    // so the click that blurs is also the click that places.
                    crate::filter::blur_on_world_click
                        .in_set(keys::Phase::Sense)
                        .before(sense_context),
                    // Sensed before anything reads the cursor, so a click sees the anchor captured
                    // for its own press rather than one frame stale.
                    sense_fine_anchor.in_set(keys::Phase::Sense),
                    crate::filter::keys.in_set(keys::Phase::Text),
                    crate::filter::refresh,
                ),
            )
            .add_observer(crate::filter::on_click)
            .add_systems(
                Startup,
                (
                    crate::thumbs::setup,
                    spawn_panel,
                    spawn_palette_panel,
                    spawn_cost_readout,
                    spawn_removal_tile,
                    spawn_clone_tile,
                    spawn_target_tile,
                    spawn_existing,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    // **The fields run last and the actions run first**, so no census action can
                    // fire on a keystroke a field has already swallowed. See `keys::Phase`.
                    rename_keys.in_set(keys::Phase::Text),
                    pin_reason_keys.in_set(keys::Phase::Text),
                    size_edit_keys.in_set(keys::Phase::Text),
                    // No `not_typing`, no `in_map_mode`: `keys::just_pressed` now refuses on both
                    // counts, and a run condition repeating it would be a second census. Dropping
                    // `in_map_mode` also makes `Cmd+S` and `Cmd+Z` work from every tab, which is what
                    // the census has always said they do (`Context::Global`) and what the guard
                    // quietly contradicted.
                    keys.in_set(keys::Phase::Act),
                    // These two stay gated: they read the MOUSE, which the census does not model, so
                    // context is not their guard. Mode is.
                    drive_place.run_if(not_typing).run_if(in_map_mode),
                    drive_removal.run_if(not_typing).run_if(in_map_mode),
                    drive_move.run_if(not_typing).run_if(in_map_mode),
                    drive_clone.run_if(not_typing).run_if(in_map_mode),
                    drive_target_marker.run_if(in_map_mode),
                    hide_carried.run_if(in_map_mode),
                    drive_ghost.run_if(in_map_mode),
                    // **Not gated on the mode.** The stamped rows are part of the map, so they stay
                    // drawn while an author is on Tiles or Compose looking at the group that made
                    // them — a world that empties out when you change tabs would read as the stamp
                    // having failed.
                    // Nested: Bevy 0.19 caps an `add_systems` tuple at 20 and this one is full.
                    (redraw_stamps, fade_ghost),
                    style_rows,
                    refresh_status,
                    rebuild_palette.run_if(
                        // `or_else`, not the deprecated `or`: 0.19 spells the lazy form this way,
                        // and this project has already paid for the eager one — every run condition
                        // being evaluated is what made a bare `Res<T>` behind an earlier `false`
                        // panic on launch.
                        resource_changed::<Project>
                            .or_else(resource_changed::<EditorState>)
                            .or_else(resource_changed::<crate::filter::Filters>)
                            // A newly created portrait handle has to be bound, and binding happens
                            // when the row is built. See `thumbs::ThumbGeneration` for why this is not
                            // `resource_changed::<Thumbnails>`.
                            .or_else(resource_changed::<crate::thumbs::ThumbGeneration>)
                            .or_else(run_once),
                    ),
                    refresh_size,
                    refresh_triangle_total,
                    draw_bounds,
                    check_edges.run_if(resource_changed::<Project>.or_else(run_once)),
                    draw_edge_faults.run_if(in_map_mode),
                ),
            )
            .add_observer(on_row_click)
            .add_observer(on_category_click)
            .add_observer(on_size_field_click);
    }
}

// ── chrome ───────────────────────────────────────────────────────────────────────────────────────

/// The cost readout, in its own root anchored bottom right.
///
/// Separate from the panel rather than a row in it: it is about the *scene*, not about the tool, and
/// it belongs where the eye goes last rather than in the middle of the controls.
fn spawn_cost_readout(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(12.0),
                bottom: Val::Px(10.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            GlobalZIndex(100),
            // Nothing here is clickable, and a readout that eats clicks is a readout that steals the
            // corner of the map underneath it.
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                TextColor(DIM),
                TextFont::from_font_size(11.0),
                TriangleTotal,
            ));
        });
}

/// Build the panel's fixed furniture. The palette itself is `rebuild_palette`'s, which is why neither
/// the project nor the thumbnails are read here — they were parameters that had stopped being used.
fn spawn_panel(mut commands: Commands) {
    let root = crate::chrome::panel_root(
        &mut commands,
        crate::chrome::Side::Left,
        crate::chrome::CONTROLS_W,
        false,
        false,
    )
    .insert(crate::tiles::MapRoot)
    .id();

    commands.entity(root).with_children(|p| {
        crate::chrome::title(p, "EMERGE MAPPER");
        crate::chrome::shortcut_hint(p);

        // The readout, in the same two columns as the keys so the whole panel shares one left edge.
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(1.0),
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            StatusBlock,
        ))
        .with_children(|s| {
            for field in [
                Field::Name,
                Field::Brush,
                Field::Yaw,
                Field::Map,
                Field::Last,
                Field::Hint,
                Field::Edges,
            ] {
                s.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Node {
                            width: Val::Px(62.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(field.label()),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                    ));
                    row.spawn((
                        Text::new(""),
                        TextColor(TEXT),
                        TextFont::from_font_size(11.0),
                        field,
                    ));
                });
            }
        });

        // **Map size.** Stated, adjustable, and drawn in the world — an edge nothing shows is an
        // edge nobody believes. It is also what gives the flood fill somewhere to stop.
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            },
        ))
        .with_children(|s| {
            s.spawn((
                Text::new("MAP SIZE  (m)"),
                TextColor(LABEL),
                TextFont::from_font_size(10.0),
            ));
            for axis in [Axis::X, Axis::Y, Axis::Z] {
                s.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Node {
                            width: Val::Px(14.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(axis.label()),
                        TextColor(LABEL),
                        TextFont::from_font_size(11.0),
                    ));
                    // **A field, not a pair of nudges.** Stepping to 48 m from 32 was four clicks
                    // and no way to say the number; the author knows the size they want, so the
                    // control should let them state it. Click to focus, type digits, Enter to keep.
                    row.spawn((
                        UiButton,
                        Hovered::default(),
                        SizeField(axis),
                        Node {
                            width: Val::Px(56.0),
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(ROW_BG),
                    ))
                    .with_children(|f| {
                        f.spawn((
                            Text::new(""),
                            TextColor(TEXT),
                            TextFont::from_font_size(11.0),
                            SizeReadout(axis),
                        ));
                    });
                });
            }
        });

    });
}

/// **The palette, in its own panel against the right edge.**
///
/// It used to be the last child of the panel above, which meant it was handed whatever vertical space
/// the keys, the status block and the map-size rows had not already taken. At `UiScale(1.2)` on a
/// 1280×802 logical screen that is 668 UI px of budget against ~447 px of everything else — so the
/// list rendered *two* rows and the rest ran off the bottom of the screen. The `max_height` it carried
/// was never reached and never did anything.
///
/// A panel pinned to BOTH insets cannot have that failure: `top` and `bottom` together give the node a
/// height taken from the viewport rather than from its content, so the list has a bottom to stop at
/// and the scroll area is bounded by construction. Nothing above it can push it off-screen, because
/// there is no longer anything above it.
///
/// Right edge rather than a second column beside the keys: the left panel keeps its width, and the map
/// stays visible in the band between them, which is the thing being authored.
fn spawn_palette_panel(mut commands: Commands) {
    crate::chrome::panel_root(
        &mut commands,
        crate::chrome::Side::Right,
        LIST_W,
        // Pinned top AND bottom — the whole point of this panel.
        true,
        false,
    )
    // Tagged like the panel above so `tiles::apply_mode` hides it with the rest of the map tab — it
    // iterates every `MapRoot`, so a second one needs no change there. Without this the palette would
    // sit over the tiles tab offering pieces that tab cannot place.
    .insert(crate::tiles::MapRoot)
    .with_children(|p| {
        p.spawn((
            Text::new("PLACE"),
            TextColor(LABEL),
            TextFont::from_font_size(10.0),
        ));
        crate::filter::spawn(p, crate::filter::Pane::Palette);
        crate::chrome::scroll_list(p, PaletteList);
    });
}

/// **The palette, grouped.**
///
/// Forty-one rows is a list you scroll past; grouped by what a thing IS it becomes a handful of
/// headings you can skip. The grouping is the `kind` axis of the project's own vocabulary rather than
/// a second taxonomy invented for the panel — `docs/ui.md` §1.2 names over-informing as the failure
/// mode, and two competing category systems is the version of that which also makes people wrong.
///
/// Categories appear in VOCABULARY order and never in frequency or recency order. Samp 2011, via
/// §3.5: a menu's cost is paid at first sight, so fix positions permanently — a palette that
/// reshuffles as a map fills is one nobody builds a memory of.
fn rebuild_palette(
    mut commands: Commands,
    project: Res<Project>,
    state: Res<EditorState>,
    thumbs: Option<Res<crate::thumbs::Thumbnails>>,
    filters: Res<crate::filter::Filters>,
    lists: Query<Entity, With<PaletteList>>,
) {
    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|p| {
            for (category, mut members) in categories(&project) {
                // **The filter narrows; it never reorders.** Rows that survive keep the positions
                // they had, so what an author learned about where a piece sits is still true —
                // Samp 2011, via `docs/ui.md` §3.5.
                members.retain(|ix| {
                    project
                        .library
                        .descriptors
                        .get(*ix)
                        .is_some_and(|d| filters.keeps(crate::filter::Pane::Palette, &d.id))
                });
                // A heading with nothing under it is a heading about nothing.
                if members.is_empty() {
                    continue;
                }
                let folded = state.collapsed.contains(&category);
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    CategoryHeader(category.clone()),
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
                    // A caret, so folded and unfolded differ by shape and not only by what is below
                    // them — an encoding that survives being glanced at.
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
                        Text::new(category.to_uppercase()),
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
                    let Some(d) = project.library.descriptors.get(ix) else {
                        continue;
                    };
                    p.spawn((
                        UiButton,
                        Hovered::default(),
                        PaletteRow(ix),
                        Node {
                            width: Val::Percent(100.0),
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(ROW_BG),
                    ))
                    .with_children(|row| {
                        let mut slot = row.spawn((
                            Node {
                                width: Val::Px(THUMB_SLOT),
                                height: Val::Px(THUMB_SLOT),
                                margin: UiRect::right(Val::Px(8.0)),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            BackgroundColor(SLOT_BG),
                        ));
                        // **By id, not by index.** The library grows on Accept and shrinks on
                        // remove, and an index into a portrait list that was sized at startup showed
                        // a blank tile in the first case and the neighbour's picture in the second.
                        if let Some(image) = thumbs
                            .as_ref()
                            .zip(project.library.descriptors.get(ix))
                            .and_then(|(t, d)| t.image(&d.id))
                        {
                            slot.insert(ImageNode::new(image));
                        }
                        row.spawn((
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                            Text::new(d.id.clone()),
                            TextColor(TEXT),
                            TextFont::from_font_size(11.0),
                        ));
                        let tris = project.triangles.get(ix).copied().unwrap_or(0);
                        row.spawn((
                            Text::new(brief_count(tris)),
                            TextColor(cost_tint(tris)),
                            TextFont::from_font_size(10.0),
                            TextLayout::new(Justify::Right, LineBreak::NoWrap),
                        ));
                    });
                }
            }
        });
    }
}

/// The library grouped by its primary `kind`, in vocabulary order.
///
/// A descriptor's FIRST kind token is its category. Most carry exactly one — the converter maps the
/// old `category` field straight across — and for the few that carry several, the first is the one
/// the author wrote first, which is a better guess at "what this mainly is" than any rule this
/// function could invent.
fn categories(project: &Project) -> Vec<(String, Vec<usize>)> {
    let mut out: Vec<(String, Vec<usize>)> = project
        .vocab
        .kind
        .names()
        .map(|n| (n.to_owned(), Vec::new()))
        .collect();
    // A home for anything untagged, LAST — it is a to-do list, not a category, and putting it first
    // would give the least useful group the most valuable position.
    out.push((UNSORTED.to_owned(), Vec::new()));

    for (ix, d) in project.library.descriptors.iter().enumerate() {
        let key = d.kind.first().map(String::as_str).unwrap_or(UNSORTED);
        match out.iter_mut().find(|(name, _)| name == key) {
            Some((_, members)) => members.push(ix),
            // A token the vocabulary does not have cannot reach here — `Library::resolve` refuses it
            // at load — but grouping it visibly beats dropping the row from the palette entirely.
            None => {
                if let Some((_, members)) = out.iter_mut().find(|(name, _)| name == UNSORTED) {
                    members.push(ix);
                }
            }
        }
    }
    // Empty categories are noise: the vocabulary has sixteen and a project may use six.
    out.retain(|(_, members)| !members.is_empty());
    out
}

/// Fold or unfold a category.
fn on_category_click(
    activate: On<Activate>,
    headers: Query<&CategoryHeader>,
    mut state: ResMut<EditorState>,
) {
    let Ok(header) = headers.get(activate.entity) else {
        return;
    };
    if !state.collapsed.remove(&header.0) {
        state.collapsed.insert(header.0.clone());
    }
}

/// One observer for the whole palette. `Activate` carries the entity, so the index lives on the row
/// as a component and there is no macro-unrolled observer per row.
fn on_row_click(
    activate: On<Activate>,
    rows: Query<&PaletteRow>,
    project: Res<Project>,
    mut state: ResMut<EditorState>,
    mut filters: ResMut<crate::filter::Filters>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    state.brush = Some(row.0);
    // **Arming a piece returns to placing.** Picking something to place is an unambiguous statement
    // that you are done deleting or moving, and a tool is otherwise only escapable by a key — which a
    // borderless-fullscreen window can have taken from it before Bevy ever sees it. A mode you can
    // enter with a click and only leave with a keystroke is a trap.
    let was = std::mem::take(&mut state.tool);
    // **And it takes the keyboard back from the filter box.**
    //
    // `filter::blur_on_world_click` already does this for a click on the world, for the reason the
    // status doc records: `drive_place` is gated on `not_typing`, so leaving the box focused made
    // *the most natural way to find a piece* — type a few letters — the way to break placing it, with
    // no message. Clicking the row you just filtered for is the other half of that same click, and it
    // did not blur. So the search that finds a piece could still be the search that stops you using
    // it.
    filters.blur();
    if let Some(d) = project.library.descriptors.get(row.0) {
        state.status = if was == Tool::Place {
            format!("{} armed", d.id)
        } else {
            format!("{} armed — {} off", d.id, was.label())
        };
    }
}

/// Grow or shrink one axis. Clamped at one cell rather than at zero: a map has to enclose something,
/// and `Map::validate` refuses a non-positive extent — better to stop at the floor than to write a
/// map the save will then reject.
fn on_size_field_click(
    activate: On<Activate>,
    fields: Query<&SizeField>,
    mut edit: ResMut<SizeEdit>,
    mut state: ResMut<EditorState>,
) {
    let Ok(field) = fields.get(activate.entity) else {
        return;
    };
    // **Starts empty, and says so.** Seeding it with the current number meant the first digit typed
    // appended to it — 32 became 328 — and it looked like it had worked. `rename_keys` records the
    // same trap and the same answer: a field with no selection model starts blank, and the value it
    // is replacing is still on screen until Enter.
    edit.active = Some((field.0, String::new()));
    state.status = format!(
        "{} size: type a whole number of metres, Enter to keep it, Esc to leave it alone",
        field.0.label()
    );
}

/// Digits, and nothing else.
///
/// Filtered at the keystroke rather than validated at commit: a field that accepts `4.5` and then
/// refuses it has taught the author it was allowed. This one simply never shows a character that
/// cannot be part of the answer.
fn size_edit_keys(
    mut events: MessageReader<KeyboardInput>,
    mut edit: ResMut<SizeEdit>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    if edit.active.is_none() {
        return;
    }
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let Some((axis, raw)) = edit.active.take() else {
                    return;
                };
                if raw.is_empty() {
                    state.status = "nothing typed; the size is unchanged".to_owned();
                    return;
                }
                // **Metres, not whole metres.** The old +/- nudges stepped Y by 0.5 m; the field that
                // replaced them took `u32`, so a ceiling saved at 3.5 m could be read on screen and
                // never typed back — the author's only way to change it snapped it to 3 or 4. Bounds
                // are `f32` in the schema and always were.
                let Ok(want) = raw.parse::<f32>() else {
                    state.status = format!("`{raw}` is not a number of metres");
                    return;
                };
                if !want.is_finite() || !(MIN_BOUND as f32..=MAX_BOUND as f32).contains(&want) {
                    state.status =
                        format!("a map axis runs {MIN_BOUND}..{MAX_BOUND} m; `{raw}` is outside it");
                    return;
                }
                let mut bounds = project.map.bounds;
                axis.set(&mut bounds, want);
                if bounds != project.map.bounds {
                    project.map.bounds = bounds;
                    project.dirty = true;
                }
                state.status = format!(
                    "map is {} x {} x {} m",
                    trim_metres(bounds.0),
                    trim_metres(bounds.1),
                    trim_metres(bounds.2)
                );
            }
            Key::Escape => {
                edit.active = None;
                state.status = "size unchanged".to_owned();
            }
            Key::Backspace => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    raw.pop();
                }
            }
            Key::Character(s) => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    // Digits and one decimal point — room for `MAX_BOUND`'s three digits plus
                    // `.5`, and at most one point, so the buffer cannot grow into something the
                    // parse has to refuse later.
                    let point = s == "." && !raw.contains('.');
                    let digit = s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty();
                    if (digit || point) && raw.len() < 5 {
                        raw.push_str(s);
                    }
                }
            }
            _ => {}
        }
    }
}

fn refresh_size(
    project: Res<Project>,
    edit: Res<SizeEdit>,
    mut readouts: Query<(&SizeReadout, &mut Text, &mut TextColor)>,
    mut fields: Query<(&SizeField, &mut BackgroundColor)>,
) {
    for (readout, mut text, mut colour) in &mut readouts {
        // While a field is being typed into it shows what has been typed, with a caret — so the
        // number on screen is always the number Enter would commit, never the one being replaced.
        let editing = match &edit.active {
            Some((axis, raw)) if *axis == readout.0 => Some(raw),
            _ => None,
        };
        let (want, want_colour) = match editing {
            Some(raw) => (format!("{raw}_"), ACCENT),
            None => {
                let v = readout.0.get(project.map.bounds);
                // Whole metres now that the field only accepts them. A map loaded with a fractional
                // bound still reads truthfully rather than being silently rounded on screen.
                let text = if (v - v.round()).abs() < 1e-3 {
                    format!("{v:.0}")
                } else {
                    format!("{v:.1}")
                };
                (text, TEXT)
            }
        };
        if text.0 != want {
            text.0 = want;
        }
        if colour.0 != want_colour {
            colour.0 = want_colour;
        }
    }

    for (field, mut bg) in &mut fields {
        let focused = matches!(&edit.active, Some((axis, _)) if *axis == field.0);
        let want = if focused { SLOT_BG } else { ROW_BG };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

/// Draw the map's extent as a wireframe box.
///
/// Gizmos rather than a mesh: the bounds are a statement about the map, not a thing in it, and a real
/// box would be pickable, shadow-casting, and something a click could land on.
/// **Every place the map disagrees with the tiles' declared edge tokens.**
///
/// Recomputed only when the project changes — it is O(placements^2) in the worst case and the answer
/// cannot move while nothing has been placed, turned or removed.
#[derive(Resource, Default)]
pub struct EdgeFaults(pub Vec<emerge_core::adjacency::Fault>);

fn check_edges(project: Res<Project>, mut faults: ResMut<EdgeFaults>) {
    // The **layered** library, because that is what the map places — and this project's divisions,
    // because a face's length is derived from a piece's size and how finely the project divides.
    faults.0 = emerge_core::adjacency::faults(
        &project.map,
        &project.library,
        project.policy.divisions,
    );
}

/// Outline both halves of every fault, so the sentence in the panel has something to point at.
///
/// A gizmo rather than a spawned marker: it is derived from a resource that is already recomputed on
/// change, so there is nothing to keep in step and nothing to clean up.
fn draw_edge_faults(
    faults: Res<EdgeFaults>,
    project: Res<Project>,
    heights: Query<(&Placement, &Transform)>,
    mut gizmos: Gizmos,
) {
    if faults.0.is_empty() {
        return;
    }
    for fault in &faults.0 {
        for id in [fault.a.as_str(), fault.b.as_str()] {
            if id.is_empty() {
                continue;
            }
            let Some((_, tf)) = heights.iter().find(|(p, _)| p.0 == id) else {
                continue;
            };
            let footprint = project
                .map
                .placements
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| project.library.get(&p.descriptor))
                .and_then(emerge_core::descriptor::placed_footprint)
                .unwrap_or((CELL, CELL));
            gizmos.rect(
                Isometry3d::new(
                    tf.translation + Vec3::Y * 0.02,
                    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                ),
                Vec2::new(footprint.0, footprint.1),
                FAULT_LINE,
            );
        }
    }
}

/// The colour a disagreeing face is outlined in — the palette's refusal colour, because a fault is
/// the map and the tokens contradicting each other and that is a thing to fix, not a warning.
const FAULT_LINE: Color = DANGER;

fn draw_bounds(project: Res<Project>, mut gizmos: Gizmos) {
    // `floor_rect` is the floor PLAN — map space, centred on zero. Drawing happens in the world, so
    // the origin goes back on here.
    let (min_x, min_z, max_x, max_z) = project.map.floor_rect();
    let (floor, ceiling) = project.map.height_span();
    let (w, h, d) = project.map.bounds;
    let centre = Vec3::new(
        project.map.origin.0 + (min_x + max_x) * 0.5,
        (floor + ceiling) * 0.5,
        project.map.origin.2 + (min_z + max_z) * 0.5,
    );
    // `cube`, not `cuboid` — 0.19 spells it `Gizmos::cube` and takes a transform whose SCALE is
    // the box's size (`bevy_gizmos-0.19.0/src/gizmos.rs:637`).
    gizmos.cube(
        Transform::from_translation(centre).with_scale(Vec3::new(w, h, d)),
        BOUNDS_LINE,
    );
}

fn style_rows(
    state: Res<EditorState>,
    mut rows: Query<(&PaletteRow, &Hovered, &mut BackgroundColor)>,
) {
    for (row, hovered, mut bg) in &mut rows {
        let want = if state.brush == Some(row.0) {
            ROW_SELECTED
        } else if hovered.0 {
            Color::srgb(0.16, 0.15, 0.14)
        } else {
            ROW_BG
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

fn refresh_status(
    project: Res<Project>,
    faults: Res<EdgeFaults>,
    state: Res<EditorState>,
    mut fields: Query<(&Field, &mut Text, &mut TextColor)>,
) {
    // `none` covers both "nothing armed" and "the armed index no longer exists", which read the same
    // to an author and want the same word.
    let brush = state
        .brush
        .and_then(|ix| project.library.descriptors.get(ix))
        .map(|d| d.id.as_str())
        .unwrap_or("none");

    for (field, mut text, mut colour) in &mut fields {
        let (want, tint) = match field {
            // Silent when the map agrees with itself, which is the common case and the one a standing
            // line must not shout about. `docs/ui.md` §1.2: more information helps, noise does not.
            Field::Edges => match faults.0.len() {
                0 => ("ok".to_owned(), DIM),
                1 => (faults.0[0].message.clone(), DANGER),
                n => (
                    format!("{n} — {}", faults.0[0].message),
                    DANGER,
                ),
            },
            // While renaming, the field shows what the name WILL be, not what was typed — that is
            // what "forced" means, and seeing `site_67` appear as you type "Site 67" teaches the rule
            // without anyone having to read it.
            Field::Name => match &state.renaming {
                // A caret, so an empty field reads as "waiting for you" rather than as the name
                // having been wiped.
                Some(raw) => (
                    format!("{}_", emerge_core::naming::to_snake_case(raw)),
                    ACCENT,
                ),
                None => (project.map.name.clone(), TEXT),
            },
            Field::Brush => (brush.to_owned(), TEXT),
            Field::Yaw => (format!("{} deg", state.brush_yaw), TEXT),
            Field::Map => (
                // Counted by `emerge_core::census`, never here — see
                // `tests/census_is_the_one_counter.rs` for why a rendered count is the one that
                // drifts. Stamps are named separately because a stamp is one reference however many
                // rows it stands for, and folding them into "placed" would make the number disagree
                // with what is on screen.
                {
                    let counted = emerge_core::census::of_map(&project.map);
                    let stamps = match counted.stamps {
                        0 => String::new(),
                        n => format!(", {n} stamped"),
                    };
                    let unsaved = if project.dirty { ", unsaved" } else { "" };
                    format!("{} placed{stamps}{unsaved}", counted.placements)
                },
                if project.dirty { ACCENT } else { TEXT },
            ),
            // A refusal has to read differently from a success, or "NOT SAVED" scrolls past as if it
            // were a receipt.
            Field::Hint => (state.hint.clone(), ACCENT),
            Field::Last => (
                state.status.clone(),
                if state.status.starts_with("NOT SAVED") {
                    DANGER
                } else {
                    DIM
                },
            ),
        };
        if text.0 != want {
            text.0 = want;
        }
        if colour.0 != tint {
            colour.0 = tint;
        }
    }
}

/// While a name is being typed, every other key belongs to the name.
///
/// `Option<Res<_>>` is not needed here because `EditorState` is `init_resource`d by this same plugin
/// — but every run condition IS evaluated in Bevy 0.19, with no short-circuit, so a condition reading
/// a resource some *other* plugin owns must take the option. Worth stating next to the one place that
/// legitimately does not.
/// **Nothing that reads a tab's keys may fire while a field is taking them.**
///
/// This is [`crate::keys::Context::Typing`] in the census's terms — the context that overlaps every
/// other one and suppresses all of them.
///
/// **One condition, every field.** There were two (`not_typing` here and `not_renaming_candidate` in
/// `tiles.rs`), each knowing about the fields of its own tab, and a filter box would have made three:
/// a system gated on the wrong one fires while you type, which is how `2` in a text box lands you on
/// another tab. Every field is listed here and nowhere else, so adding one is adding a line.
///
/// The buffers stay with their fields — a name, a pin's reason, an axis, a candidate id and a filter
/// hold different things and commit differently. That is five kinds of state, not one fact written
/// five times, so this reads them rather than owning them.
pub fn not_typing(
    state: Res<EditorState>,
    edit: Res<SizeEdit>,
    import: Res<crate::tiles::ImportState>,
    filters: Res<crate::filter::Filters>,
    cell: Res<crate::tiles::CellEdit>,
    note: Res<crate::tiles::NoteEdit>,
    width: Res<crate::tiles::ScaleEdit>,
    height: Res<crate::tiles::HeightEdit>,
) -> bool {
    state.renaming.is_none()
        && state.pinning.is_none()
        && edit.active.is_none()
        && import.renaming.is_none()
        && !filters.typing()
        && !cell.typing()
        && !note.typing()
        && !width.typing()
        && !height.typing()
}

/// **Decide who owns the keyboard, once, before anything reads a key.**
///
/// Runs in [`keys::Phase::Sense`], which is ordered before both the action systems and the text
/// fields. That ordering is not tidiness — see [`keys::Phase`] for the defect it fixes, in which a
/// text field cleared its own flag mid-frame and the run condition guarding `Enter` re-evaluated to
/// true behind it, importing six descriptors into `library.ron` by accident.
///
/// This reads exactly the fields [`not_typing`] reads, and for the same reason: the buffers stay with
/// their fields, so adding a field is adding a line *here* — the one list — rather than a new guard
/// somebody else has to remember to consult.
pub fn sense_context(
    mode: Res<crate::tiles::Mode>,
    state: Res<EditorState>,
    edit: Res<SizeEdit>,
    import: Res<crate::tiles::ImportState>,
    filters: Res<crate::filter::Filters>,
    cell: Res<crate::tiles::CellEdit>,
    note: Res<crate::tiles::NoteEdit>,
    width: Res<crate::tiles::ScaleEdit>,
    height: Res<crate::tiles::HeightEdit>,
    mut live: ResMut<keys::Live>,
) {
    let typing = state.renaming.is_some()
        || state.pinning.is_some()
        || edit.active.is_some()
        || import.renaming.is_some()
        || filters.typing()
        || cell.typing()
        || note.typing()
        || width.typing()
        || height.typing();
    let want = keys::Live(keys::live(mode.context(), typing));
    // Written through the change detector only when it actually moves, so `Live` staying put does not
    // wake every `resource_changed` reader in the editor every frame.
    if *live != want {
        *live = want;
    }
}

/// Placing belongs to map mode. Without this, `F` in import mode would flood the map with whatever
/// the palette last had armed, which is a surprising amount of work to undo.
fn in_map_mode(mode: Res<crate::tiles::Mode>) -> bool {
    *mode == crate::tiles::Mode::Map
}

/// Type a name. Snake case is applied to what is shown and to what is committed, so the illegal state
/// is never reachable rather than merely being rejected at the end.
/// A metre count with no trailing `.0` — `32` rather than `32.0`, `3.5` as itself.
fn trim_metres(m: f32) -> String {
    if (m - m.round()).abs() < 1e-4 {
        format!("{m:.0}")
    } else {
        format!("{m:.1}")
    }
}

fn rename_keys(
    mut events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    if state.renaming.is_none() {
        // `N` starts a rename. The comment here used to claim reading from the buffered events was
        // enough to stop a keypress both starting the rename and being typed into it; it is not —
        // the reader's cursor only advances when `read()` is called, and this branch returns without
        // calling it. `events.clear()` below is what actually holds the invariant.
        if keys::just_pressed(&keyboard, live.0, Action::RenameMap) {
            // **Empty, not seeded with the current name.** Seeding it meant the first keystroke
            // appended, so renaming `site_67_hub` to `galley_deck` produced
            // `site_67_hubgalley_deck` — and it looked like it had worked, because the panel showed a
            // name growing as expected. A real rename field starts with the old name SELECTED so
            // typing replaces it; there is no selection model here, so the honest equivalent is to
            // start blank. The old name is still on screen the moment you press Esc.
            state.renaming = Some(String::new());
            state.status = format!(
                "type a new name for `{}` — Enter to keep it, Esc to leave it alone",
                project.map.name
            );
        }
        // Drain before leaving, so the `N` that opened the field is not read as its first character
        // next frame. Same invariant as `tiles::cell_keys`.
        events.clear();
        return;
    }

    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let raw = state.renaming.take().unwrap_or_default();
                let name = emerge_core::naming::to_snake_case(&raw);
                if name.is_empty() {
                    state.status = "a map needs a name; nothing was changed".to_owned();
                } else if name != project.map.name {
                    let was = std::mem::replace(&mut project.map.name, name.clone());
                    project.dirty = true;
                    // The file follows the name on the next save, and the old one stays where it is
                    // — deleting it here would destroy a file on a keystroke.
                    // The modifier from the census, not typed: this sentence naming a key the build
                    // does not read is the same failure as the panel doing it.
                    state.status = format!(
                        "renamed `{was}` to `{name}` ({}+S writes the new file)",
                        keys::MOD_NAME
                    );
                }
            }
            Key::Escape => {
                state.renaming = None;
                state.status = "name unchanged".to_owned();
            }
            Key::Backspace => {
                if let Some(raw) = state.renaming.as_mut() {
                    raw.pop();
                }
            }
            Key::Space => {
                if let Some(raw) = state.renaming.as_mut() {
                    raw.push(' ');
                }
            }
            Key::Character(s) => {
                if let Some(raw) = state.renaming.as_mut() {
                    raw.push_str(s);
                }
            }
            _ => {}
        }
    }
}

/// **Triangles actually drawn this frame**, bottom right.
///
/// Visible entities, not resident ones — the same distinction `perf_probe` draws in the game, and for
/// the same reason: a culled mesh costs memory, not frame time, and mixing them is how you end up
/// optimising the wrong thing. It is here because a flood fill can lay down 1,400 pieces in one
/// keystroke, and the moment to notice that costs 40 million triangles is while it is happening.
fn refresh_triangle_total(
    drawn: Query<(&ViewVisibility, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
    mut readout: Query<(&mut Text, &mut TextColor), With<TriangleTotal>>,
) {
    let total: usize = drawn
        .iter()
        .filter(|(v, _)| v.get())
        .filter_map(|(_, m)| meshes.get(&m.0))
        .map(|m| match m.indices() {
            Some(i) => i.len() / 3,
            None => m.count_vertices() / 3,
        })
        .sum();

    let want = format!("{} tris drawn", with_thousands(total));
    for (mut text, mut colour) in &mut readout {
        if text.0 != want {
            text.0 = want.clone();
        }
        let tint = if total > HEAVY_SCENE {
            DANGER
        } else if total > BUSY_SCENE {
            ACCENT
        } else {
            DIM
        };
        if colour.0 != tint {
            colour.0 = tint;
        }
    }
}

/// A triangle count at a glance: `1.5k`, `70k`, `4.2M`. Exact numbers belong in the total at the
/// bottom; a palette row needs an order of magnitude.
fn brief_count(n: usize) -> String {
    match n {
        0 => String::new(),
        n if n < 1_000 => format!("{n}"),
        n if n < 100_000 => format!("{:.1}k", n as f32 / 1_000.0),
        n if n < 1_000_000 => format!("{}k", n / 1_000),
        n => format!("{:.1}M", n as f32 / 1_000_000.0),
    }
}

/// Group a number for reading. The total is meant to be compared against itself over time, so the
/// digits matter.
fn with_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A piece worth a second look before filling a room with it, and one worth a first look.
///
/// Calibrated on the shipped kit rather than picked: its median is 1,526 triangles and its
/// second-densest piece is 7,996, so 20k is well clear of everything except the one genuine outlier.
fn cost_tint(triangles: usize) -> Color {
    if triangles > emerge_core::import::BUSY_TRIANGLES {
        DANGER
    } else if triangles > 5_000 {
        ACCENT
    } else {
        LABEL
    }
}

// ── placing ──────────────────────────────────────────────────────────────────────────────────────

fn snap(v: f32) -> f32 {
    (v / SNAP).round() * SNAP
}

/// A world ground point as a **map-space** `at`, snapped to the authoring grid unless `free`.
///
/// The conversion has to happen somewhere, and here is the only somewhere: `cursor_ground` answers in
/// world metres and `Placed::at` is in map space. They agree only for a map at the origin — which is
/// every map the editor has ever authored, so writing world coordinates straight into `at` looked
/// right for as long as nobody moved a map.
///
/// # Holding the platform modifier places freely
///
/// The grid is what makes pieces meet, so it is the default and stays the default. But a prop that
/// wants to sit at an angle against a wall, or a bit of clutter that should not read as laid out on
/// a grid, has nowhere to go on a half-metre lattice — and the alternative was hand-editing the
/// map's RON afterwards.
///
/// Only X and Z: they are the axes the grid quantises and the axes a cursor on the ground plane can
/// express. **Y is not freed** — it comes from the piece's `mount` through `stack::datum`, which is
/// what puts a lamp on a table rather than through it, and there is no second mouse axis to say
/// otherwise with.
/// # Free placement stays inside the cell it started in
///
/// Fine control is for nudging a piece **within** the cell an author already chose, not for sliding it
/// across the map with the snap off. Unbounded, a small hand movement while the modifier was down
/// walked the piece a cell or two over and the author had to notice and undo it — the grid's whole job
/// is to stop that, and holding the modifier used to switch the job off entirely rather than turn it
/// down.
///
/// So the cell under the cursor is captured the moment the modifier goes down ([`FineAnchor`]) and the
/// free position is clamped to it: anywhere inside that half-metre, nowhere outside it. Release and
/// press again over a different cell to nudge a different one. An assist, not a restriction — every
/// position that was reachable is still reachable, in two gestures instead of one.
fn map_at(project: &Project, hit: Vec3, free: bool, anchor: &FineAnchor) -> (f32, f32) {
    let (x, z) = project.map.to_map_space((hit.x, hit.z));
    match (free, anchor.cell) {
        // `snap` rounds to the nearest multiple of `SNAP`, so the cell around a snapped point reaches
        // half a step either side of it.
        (true, Some((cx, cz))) => {
            let half = SNAP * 0.5;
            (
                x.clamp(cx - half, cx + half),
                z.clamp(cz - half, cz + half),
            )
        }
        // The modifier went down off the ground plane, so there is no cell to hold to. Free, as it
        // was — refusing to place at all would be a worse answer than the one the author asked for.
        (true, None) => (x, z),
        (false, _) => (snap(x), snap(z)),
    }
}

/// The rectangle being dragged out to fill, in map space. Its own resource on the same argument
/// [`RemovalDrag`] makes: written every frame of a drag, and `rebuild_palette` watches
/// [`EditorState`].
#[derive(Resource, Default)]
pub struct PlaceDrag {
    /// Where the button went down, or `None` while only hovering.
    from: Option<(f32, f32)>,
}

/// Fill a dragged box with the brush, as one act.
///
/// Split out of [`drive_place`] so the release path reads as the two things it chooses between —
/// place one, or fill a box — rather than as one function with a second half.
fn box_fill_between(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &mut Project,
    state: &mut EditorState,
    brush: &emerge_core::descriptor::Descriptor,
    corners: ((f32, f32), (f32, f32)),
) {
    let mut n = state.next_id;
    let short = short_id(&brush.id).to_owned();
    let filled = match crate::fill::box_fill(&project.map, brush, corners, state.brush_yaw, || {
        n += 1;
        format!("{short}@{n}")
    }) {
        Ok(f) => f,
        // A refusal is the answer, not a failure — the same call `flood_from_cursor` makes.
        Err(e) => {
            state.status = e;
            return;
        }
    };
    state.next_id = n;

    let count = filled.placements.len();
    let first = project.map.placements.len();
    project.map.placements.extend(filled.placements);
    // Into the map first, drawn second: how high a piece sits is a question about the finished map.
    spawn_range(commands, assets, project, state, first);
    project.dirty = true;
    // **One entry for the whole box**, the rule `RemovedMany` states: one act the author performed is
    // one act to take back.
    state.record(Undo::Added { count });
    state.status = if filled.truncated {
        format!(
            "filled {count} — stopped at the {} cell cap",
            crate::fill::MAX_CELLS
        )
    } else {
        format!("filled {count}")
    };
}

/// The clone tool's marker quad — [`spawn_removal_tile`]'s twin in the clone tint, and spawned
/// once for the same leak-shaped reason.
fn spawn_clone_tile(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        CloneTile,
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: CLONE_TINT,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// **The clone tool** — [`drive_removal`]'s shape with the opposite payload: the box takes a COPY
/// of everything whose centre it contains, and every later click stamps the whole set at the
/// cursor, fresh ids and all.
///
/// Preview and commit in one system for the reason `drive_removal` gives: the rectangle drawn IS
/// the rectangle captured, because they are the same numbers.
#[allow(clippy::too_many_arguments)]
fn drive_clone(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mouse: Res<ButtonInput<MouseButton>>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    mut marker: Query<(&mut Transform, &mut Visibility), With<CloneTile>>,
    mut drag: ResMut<CloneDrag>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    // Read, never write, unless something happened — `rebuild_palette` watches `state`.
    if state.tool != Tool::Clone {
        if drag.from.is_some() || drag.held.is_some() {
            drag.from = None;
            drag.held = None;
        }
        for (_, mut vis) in &mut marker {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        }
        return;
    }

    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    let hit = cursor_ground(&window, cam, cam_tf).filter(|_| !hovered_ui.iter().any(|h| h.0));
    let Some(hit) = hit else {
        // A release off the world ends the drag — the defect `drive_removal` records.
        if mouse.just_released(MouseButton::Left) {
            drag.from = None;
        }
        for (_, mut vis) in &mut marker {
            *vis = Visibility::Hidden;
        }
        return;
    };
    let at = project.map.to_map_space((hit.x, hit.z));

    if mouse.just_pressed(MouseButton::Left) && drag.held.is_none() {
        drag.from = Some(at);
    }

    // What the marker shows: the held set's bounds riding the (snapped) cursor, else the box being
    // dragged out.
    let rect = if let Some(set) = &drag.held {
        let target = (snap(at.0), snap(at.1));
        let c = (target.0 + set.centre_off.0, target.1 + set.centre_off.1);
        Some((c.0 - set.half.0, c.1 - set.half.1, c.0 + set.half.0, c.1 + set.half.1))
    } else {
        drag.from.map(|from| {
            (from.0.min(at.0), from.1.min(at.1), from.0.max(at.0), from.1.max(at.1))
        })
    };
    for (mut tf, mut vis) in &mut marker {
        match rect {
            Some((x0, z0, x1, z1)) => {
                *vis = Visibility::Visible;
                *tf = Transform::from_xyz(
                    project.map.origin.0 + (x0 + x1) * 0.5,
                    project.map.origin.1 + MARKER_LIFT,
                    project.map.origin.2 + (z0 + z1) * 0.5,
                )
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::new((x1 - x0).max(0.05), (z1 - z0).max(0.05), 1.0));
            }
            None => *vis = Visibility::Hidden,
        }
    }

    if !mouse.just_released(MouseButton::Left) {
        return;
    }

    // **A set in hand stamps on every click.** The drag that took it is long finished, so the
    // release IS the gesture.
    if drag.held.is_some() {
        let Some(set) = drag.held.take() else {
            return;
        };
        stamp_set(&mut commands, &assets, &mut project, &mut state, &set, (snap(at.0), snap(at.1)));
        // Back in hand whatever happened: a refusal's fix is usually "two cells to the left".
        drag.held = Some(set);
        return;
    }

    let Some(from) = drag.from.take() else {
        return;
    };
    if (from.0 - at.0).abs() <= CLICK_EPS && (from.1 - at.1).abs() <= CLICK_EPS {
        state.status = "drag a box to take a copy of what is inside it".to_owned();
        return;
    }

    // **Capture.** Centre-in-box, the same containment the removal box applies, so the two box
    // gestures agree about what a rectangle contains.
    let (x0, z0) = (from.0.min(at.0), from.1.min(at.1));
    let (x1, z1) = (from.0.max(at.0), from.1.max(at.1));
    let caught: Vec<usize> = project
        .map
        .placements
        .iter()
        .enumerate()
        .filter(|(_, p)| p.at.0 >= x0 && p.at.0 <= x1 && p.at.1 >= z0 && p.at.1 <= z1)
        .map(|(i, _)| i)
        .collect();
    if caught.is_empty() {
        state.status = "nothing in that box to clone".to_owned();
        return;
    }

    // The anchor is the snapped centroid, so the set rides centred under the hand and stamps land
    // on the grid while every internal offset stays exact.
    let n = caught.len() as f32;
    let cx: f32 = caught.iter().map(|&i| project.map.placements[i].at.0).sum::<f32>() / n;
    let cz: f32 = caught.iter().map(|&i| project.map.placements[i].at.1).sum::<f32>() / n;
    let anchor = (snap(cx), snap(cz));

    let mut pieces = Vec::with_capacity(caught.len());
    let (mut bx0, mut bz0, mut bx1, mut bz1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &i in &caught {
        let p = &project.map.placements[i];
        let on = match &p.on {
            None => CloneHost::Layer,
            Some(host) => caught
                .iter()
                .position(|&j| project.map.placements[j].id == *host)
                .map(CloneHost::InSet)
                .unwrap_or(CloneHost::Outside),
        };
        let offset = (p.at.0 - anchor.0, p.at.1 - anchor.1);
        // Bounds from the real footprints, so the marker claims what the stamp will occupy.
        let (w, depth) = project
            .library
            .get(&p.descriptor)
            .map(|d| crate::fill::cell_extents(d, p.yaw))
            .unwrap_or((crate::fill::MIN_CELL, crate::fill::MIN_CELL));
        bx0 = bx0.min(offset.0 - w * 0.5);
        bz0 = bz0.min(offset.1 - depth * 0.5);
        bx1 = bx1.max(offset.0 + w * 0.5);
        bz1 = bz1.max(offset.1 + depth * 0.5);
        pieces.push(ClonePiece {
            descriptor: p.descriptor.clone(),
            offset,
            yaw: p.yaw,
            tip: p.tip,
            lift: p.lift,
            note: p.note.clone(),
            owned: p.owned,
            owned_because: p.owned_because.clone(),
            on,
        });
    }
    let count = pieces.len();
    drag.held = Some(CloneSet {
        pieces,
        centre_off: ((bx0 + bx1) * 0.5, (bz0 + bz1) * 0.5),
        half: ((bx1 - bx0) * 0.5, (bz1 - bz0) * 0.5),
    });
    state.status =
        format!("{count} piece(s) in hand — click stamps the set, Esc puts it away");
}

/// **Stamp the held set at `target`** — all of it, or none of it, the rule `move_placement` states.
///
/// Fresh ids in set order; in-set hosts repointed to the fresh ids; outside hosts re-found under
/// each new position. Every piece answers the overlap rule against the standing map, and the
/// finished trial re-resolves before anything is committed — one refusal anywhere and the map has
/// not changed.
fn stamp_set(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &mut Project,
    state: &mut EditorState,
    set: &CloneSet,
    target: (f32, f32),
) {
    let ys = match heights(project) {
        Ok(ys) => ys,
        Err(e) => {
            state.status = e;
            return;
        }
    };
    let mut n = state.next_id;
    let new_ids: Vec<String> = set
        .pieces
        .iter()
        .map(|piece| {
            n += 1;
            format!("{}@{}", short_id(&piece.descriptor), n)
        })
        .collect();

    let mut rows: Vec<Placed> = Vec::with_capacity(set.pieces.len());
    for (i, piece) in set.pieces.iter().enumerate() {
        let at = (target.0 + piece.offset.0, target.1 + piece.offset.1);
        let Some(d) = project.library.get(&piece.descriptor) else {
            state.status = format!(
                "stamp refused: `{}` is not in the library any more",
                piece.descriptor
            );
            return;
        };
        let on = match &piece.on {
            CloneHost::Layer => None,
            CloneHost::InSet(h) => Some(new_ids[*h].clone()),
            CloneHost::Outside => {
                match emerge_core::stack::host_under(&project.map, &project.library, &ys, d, at) {
                    Some((host, _)) => Some(host.id.clone()),
                    None => {
                        state.status = format!(
                            "stamp refused: `{}` needs a surface and nothing offers one where it \
                             would land",
                            new_ids[i]
                        );
                        return;
                    }
                }
            }
        };
        if let Some(block) = emerge_core::stack::blocking(
            &project.map,
            &project.library,
            d,
            at,
            piece.yaw,
            piece.tip,
            on.as_deref(),
        ) {
            state.status = format!(
                "stamp refused: `{}` already covers where `{}` would land",
                block.id, new_ids[i]
            );
            return;
        }
        rows.push(Placed {
            id: new_ids[i].clone(),
            descriptor: piece.descriptor.clone(),
            at,
            yaw: piece.yaw,
            lift: piece.lift,
            tip: piece.tip,
            on,
            owned: piece.owned,
            owned_because: piece.owned_because.clone(),
            patch: None,
            note: piece.note.clone(),
        });
    }

    // The finished trial is the last door — it catches whatever the per-piece checks missed, and
    // nothing has been committed when it refuses.
    let mut trial = project.map.clone();
    trial.placements.extend(rows.iter().cloned());
    if let Err(e) = emerge_core::stack::resolve_y(&trial, &project.library) {
        state.status = format!("stamp refused: {e}");
        return;
    }

    let count = rows.len();
    let first = project.map.placements.len();
    state.next_id = n;
    project.map.placements.extend(rows);
    spawn_range(commands, assets, project, state, first);
    project.dirty = true;
    // One entry for the whole set: one act the author performed is one act to take back.
    state.record(Undo::Added { count });
    state.status = format!("stamped {count} piece(s) — click again stamps another");
}

/// **The cell fine placement is confined to** — captured when the platform modifier goes down.
///
/// Its own resource, and written only on the two frames the modifier changes state, so it does not
/// wake `rebuild_palette` the way a field on [`EditorState`] would. See [`map_at`] for the rule.
#[derive(Resource, Default)]
pub struct FineAnchor {
    /// The snapped map-space point the cursor was over when the modifier went down.
    cell: Option<(f32, f32)>,
}

/// Capture the cell when the modifier goes down, and let it go when the modifier comes up.
///
/// Runs in [`keys::Phase::Sense`], before anything reads the cursor, so the anchor a click sees is the
/// one captured for that press rather than one frame stale.
fn sense_fine_anchor(
    keyboard: Res<ButtonInput<KeyCode>>,
    project: Res<Project>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    hovered_ui: Query<&Hovered>,
    mut anchor: ResMut<FineAnchor>,
) {
    let held = keys::mod_held(&keyboard);
    if !held {
        // Only when it changes — `ResMut` marks the resource changed on every mutable deref.
        if anchor.cell.is_some() {
            anchor.cell = None;
        }
        return;
    }
    // Already anchored: the cell is decided by the press, not re-decided every frame the key is down.
    // Re-capturing here would follow the cursor and clamp to nothing.
    if anchor.cell.is_some() {
        return;
    }
    // **A cursor over a panel is not a cursor over a cell.** The modifier is also every chord's
    // modifier — `Cmd+Z`, `Cmd+S`, `Cmd+2` — so it goes down over UI constantly, and an anchor
    // captured there would clamp a later free placement to whatever cell happened to lie under the
    // palette. No anchor at all is the honest answer: `map_at`'s `(true, None)` arm places free and
    // unclamped, exactly as it does when the modifier goes down off the ground plane.
    if hovered_ui.iter().any(|h| h.0) {
        return;
    }
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        return;
    };
    let (x, z) = project.map.to_map_space((hit.x, hit.z));
    anchor.cell = Some((snap(x), snap(z)));
}

/// Put a piece in the world — **through `emerge-bevy`**, which is also what the game uses.
///
/// The editor used to own this arithmetic. It cannot: a preview that disagrees with the runtime by
/// half a degree of `front` or a centimetre of `y_offset` is a preview that lies, and the only way to
/// be sure two places agree is for there to be one place. `emerge_bevy::spawn_descriptor` is it.
fn spawn_piece(
    commands: &mut Commands,
    assets: &AssetServer,
    d: &emerge_core::descriptor::Descriptor,
    at: (f32, f32),
    yaw: f32,
    tip: (u8, u8),
    origin: (f32, f32, f32),
    y: f32,
) -> Option<Entity> {
    emerge_bevy::spawn_descriptor(
        commands,
        assets,
        d,
        // The editor draws before tags matter; the runtime resolves them from the library. Defaulting
        // here keeps the signature honest rather than pretending the editor knows.
        emerge_core::vocab::Masks::default(),
        at,
        yaw,
        tip,
        // The editor authors in the map's own space, so the origin it draws at is the map's own.
        origin,
        y,
    )
}

/// **Where every piece in the map stands.** Resolved together, because a lamp's height is its table's.
///
/// Returns the reason rather than a partial answer. A map whose stacking will not resolve is one an
/// author has to be told about — drawing it half-right is how a lamp ends up looking badly authored
/// when the real problem is the shelf it names.
fn heights(project: &Project) -> Result<Vec<f32>, String> {
    emerge_core::stack::resolve_y(&project.map, &project.library)
}

/// Draw the placements from `first` onward — what a fill or a generate just added.
///
/// The map is already complete when this runs, which is the point: heights are resolved once, over the
/// finished map, so a piece added by the solver stands wherever the finished map says it does.
fn spawn_range(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &Project,
    state: &mut EditorState,
    first: usize,
) {
    let ys = match heights(project) {
        Ok(ys) => ys,
        Err(e) => {
            state.status = e.clone();
            error!("{e}");
            return;
        }
    };
    for (i, p) in project.map.placements.iter().enumerate().skip(first) {
        let (Some(d), Some(&y)) = (project.library.get(&p.descriptor), ys.get(i)) else {
            continue;
        };
        if let Some(e) = spawn_piece(commands, assets, d, p.at, p.yaw, p.tip, project.map.origin, y) {
            commands.entity(e).insert(Placement(p.id.clone()));
        }
    }
}

/// The removal marker, spawned once and then only moved.
///
/// Once, because a mesh and a material built per frame are a new asset per frame — the editor would
/// grow a handle a tick until it ran out of memory, which is the shape of bug that looks like "it
/// gets slow after a while" rather than like a leak.
fn spawn_removal_tile(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        RemovalTile,
        // A unit rectangle, scaled to whatever it has to cover. `Rectangle` is authored in the XY
        // plane, so it is laid flat by a quarter turn about X in the system below.
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: REMOVE_TINT,
            // **Unlit and blended.** Lit, the marker's brightness would report where the key light
            // happens to be rather than what it means; opaque, it would hide the piece it is
            // pointing at, which is the one thing the author is trying to look at.
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// **The removal tool**: show what would go, and take it when the button comes up.
///
/// Preview and commit in one system because they are one question asked twice — the rectangle the
/// marker draws IS the rectangle the release removes, and computing it in two places is how a
/// selection box comes to disagree with what it deletes.
#[allow(clippy::too_many_arguments)]
fn drive_removal(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    placed: Query<(Entity, &Placement)>,
    mut marker: Query<(&mut Transform, &mut Visibility), With<RemovalTile>>,
    mut drag: ResMut<RemovalDrag>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    // Read, never write, unless something actually happened: `state` is watched by
    // `rebuild_palette`, so an unconditional deref here rebuilds the palette every frame.
    if state.tool != Tool::Remove {
        if drag.from.is_some() {
            drag.from = None;
        }
        for (_, mut vis) in &mut marker {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        }
        return;
    }

    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    // A cursor over the panel is not a cursor over the map, and a marker under the palette is a
    // promise about a click that will never reach the world — the same rule the ghost follows.
    let hit = cursor_ground(&window, cam, cam_tf).filter(|_| !hovered_ui.iter().any(|h| h.0));
    let Some(hit) = hit else {
        // **A release ends the drag wherever it happens.** This used to return before the
        // `just_released` branch, so letting go over a panel or off the map left `drag.from` set —
        // and the red "these will be removed" rectangle then followed the cursor around with no
        // button held, claiming a deletion nobody was making, until the next press reset it. A drag
        // that ends outside the world removes nothing, but it does end.
        if mouse.just_released(MouseButton::Left) {
            drag.from = None;
        }
        for (_, mut vis) in &mut marker {
            *vis = Visibility::Hidden;
        }
        return;
    };
    let at = project.map.to_map_space((hit.x, hit.z));

    if mouse.just_pressed(MouseButton::Left) {
        drag.from = Some(at);
    }

    // What the marker covers: the box being dragged, or — before a drag starts — the footprint of
    // the piece that a click would take, which is what makes "this one" a claim rather than a guess.
    let rect = match drag.from {
        Some(from) => Some((
            from.0.min(at.0),
            from.1.min(at.1),
            from.0.max(at.0),
            from.1.max(at.1),
        )),
        None => pick_at(&project, at).and_then(|i| {
            let p = project.map.placements.get(i)?;
            let d = project.library.get(&p.descriptor)?;
            let (w, depth) = crate::fill::cell_extents(d, p.yaw);
            Some((
                p.at.0 - w * 0.5,
                p.at.1 - depth * 0.5,
                p.at.0 + w * 0.5,
                p.at.1 + depth * 0.5,
            ))
        }),
    };

    for (mut tf, mut vis) in &mut marker {
        match rect {
            Some((x0, z0, x1, z1)) => {
                *vis = Visibility::Visible;
                *tf = Transform::from_xyz(
                    project.map.origin.0 + (x0 + x1) * 0.5,
                    project.map.origin.1 + MARKER_LIFT,
                    project.map.origin.2 + (z0 + z1) * 0.5,
                )
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                // A floor of a few centimetres so a zero-area box is still visible as a box.
                .with_scale(Vec3::new((x1 - x0).max(0.05), (z1 - z0).max(0.05), 1.0));
            }
            None => *vis = Visibility::Hidden,
        }
    }

    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some(from) = drag.from.take() else {
        return;
    };

    if (from.0 - at.0).abs() <= CLICK_EPS && (from.1 - at.1).abs() <= CLICK_EPS {
        match pick_at(&project, at) {
            Some(i) => delete_index(&mut commands, i, &mut project, &mut state, &placed),
            None => state.status = "nothing here to remove".to_owned(),
        }
        return;
    }

    let (x0, z0) = (from.0.min(at.0), from.1.min(at.1));
    let (x1, z1) = (from.0.max(at.0), from.1.max(at.1));
    // **Each group the box touches goes whole**, the same rule the single delete follows: a rider
    // whose host is inside the box but which is itself a few centimetres outside it would otherwise
    // be left pointing at a placement that no longer exists, and `resolve_y` refuses the whole map for
    // it. Sorted and deduped, so a group caught twice — box over both a table and its lamp — is
    // removed once.
    let mut doomed: Vec<usize> = project
        .map
        .placements
        .iter()
        .enumerate()
        .filter(|(_, p)| p.at.0 >= x0 && p.at.0 <= x1 && p.at.1 >= z0 && p.at.1 <= z1)
        .flat_map(|(i, _)| emerge_core::stack::group_of(&project.map, i))
        .collect();
    doomed.sort_unstable();
    doomed.dedup();
    if doomed.is_empty() {
        state.status = "nothing inside that box".to_owned();
        return;
    }

    // **Taken back to front.** Removing an earlier row shifts every later one down, so a forward
    // pass would delete the wrong pieces the moment the box held more than one.
    let mut items: Vec<(usize, Box<Placed>)> = Vec::with_capacity(doomed.len());
    for i in doomed.iter().rev() {
        let removed = project.map.placements.remove(*i);
        for (entity, mark) in &placed {
            if mark.0 == removed.id {
                commands.entity(entity).despawn();
            }
        }
        items.push((*i, Box::new(removed)));
    }
    // Ascending again, which is the order `Undo::RemovedMany` puts them back in.
    items.reverse();

    let n = items.len();
    state.record(Undo::RemovedMany { items });
    project.dirty = true;
    // The whole chord, rendered by the census — naming just the modifier told the author to press
    // `Cmd`, which is not a thing anyone can do.
    state.status = format!(
        "removed {n} placement(s) — {} puts them back",
        keys::chord_text(keys::binding(Action::Undo))
    );
}

/// Bring up whatever the map already holds.
/// The first id counter value that cannot collide with anything the loaded map already names.
///
/// Every id this editor mints is `{short}@{n}`, and the counter used to start at ZERO in every
/// session — so reopening a saved map re-minted the exact `wall@1`, `wall@2`, … the file already
/// carried. The map held two placements with one name, and **undo despawns by id match**: taking
/// back one fill swept every same-named entity off the screen, the originals included. The rows
/// were all still in the file — the screen and the map disagreeing, the exact failure the one-path
/// rule exists to prevent. Seeding past the largest `@n` in the file makes minted ids unique by
/// construction; ids with no `@n` tail (hand-authored names) cannot collide with minted ones.
pub fn next_id_after(map: &emerge_core::map::Map) -> u32 {
    // **Both lists, because both are minted from this one counter.** `stamp_here` names a stamp
    // `<short>@<n>` from the same `next_id` a placement uses, so scanning only `placements` seeds the
    // counter below every stamp id the file already carries — and the next stamp collides with one,
    // which `Map::validate` then refuses. One mint, one high-water mark.
    let suffix = |id: &str| id.rsplit_once('@').and_then(|(_, n)| n.parse::<u32>().ok());
    let placements = map.placements.iter().filter_map(|p| suffix(&p.id));
    let stamps = map.stamps.iter().filter_map(|st| suffix(&st.id));
    placements.chain(stamps).max().unwrap_or(0)
}

fn spawn_existing(
    mut commands: Commands,
    assets: Res<AssetServer>,
    project: Res<Project>,
    mut state: ResMut<EditorState>,
) {
    state.next_id = next_id_after(&project.map);
    let ys = match heights(&project) {
        Ok(ys) => ys,
        Err(e) => {
            error!("{e}");
            return;
        }
    };
    for (i, p) in project.map.placements.iter().enumerate() {
        let Some(d) = project.library.get(&p.descriptor) else {
            // Loud, not silent: a placement naming a descriptor the library does not have is a hole
            // in the map, and an author must be told which one rather than counting missing crates.
            warn!(
                "placement `{}` names descriptor `{}`, which this library does not have",
                p.id, p.descriptor
            );
            continue;
        };
        let Some(&y) = ys.get(i) else { continue };
        if let Some(e) = spawn_piece(&mut commands, &assets, d, p.at, p.yaw, p.tip, project.map.origin, y) {
            commands.entity(e).insert(Placement(p.id.clone()));
        }
    }
}

/// **The placing tool**: a click puts one piece down, a dragged box fills the area with them.
///
/// The box is the same gesture the removal tool uses — press, drag, release — because they are the
/// same question about the same rectangle, and an author who has learnt one has learnt the other. The
/// difference is what happens to it and how it is drawn: removal fills its box translucent red and
/// takes what is inside, this outlines its box and fills it in.
///
/// Preview and commit live in one system for the reason `drive_removal` gives: the rectangle drawn IS
/// the rectangle committed, and computing it twice is how a selection box comes to disagree with what
/// it does.
#[allow(clippy::too_many_arguments)]
fn drive_place(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    // Held, the platform modifier drops the grid snap — see `map_at`.
    keyboard: Res<ButtonInput<KeyCode>>,
    assets: Res<AssetServer>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
    anchor: Res<FineAnchor>,
    mut drag: ResMut<PlaceDrag>,
    mut gizmos: Gizmos,
    mut compose: ResMut<crate::compose::ComposeState>,
) {
    // Placing is one tool among three, and a click belongs to whichever is armed. While removal is
    // armed a click removes and must not also place, or a box dragged over a crowded corner would
    // delete what was there and leave a new piece behind it; while move is armed a click picks a
    // piece up, and placing a second one under it would be two answers to one click.
    if state.tool != Tool::Place {
        // Read, never write, unless something happened — `rebuild_palette` watches `EditorState` and
        // `drag` is deliberately not on it, but the same discipline applies.
        if drag.from.is_some() {
            drag.from = None;
        }
        return;
    }
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    // A click on a control is not a click on the world. Without this, arming a piece from the palette
    // also dropped one wherever the panel happened to be over.
    let over_ui = hovered_ui.iter().any(|h| h.0);
    let Some(hit) = cursor_ground(&window, cam, cam_tf).filter(|_| !over_ui) else {
        // **A release ends the drag wherever it happens** — the defect `drive_removal` records: a
        // release over a panel used to leave `from` set, and the box then followed the cursor with no
        // button held, claiming an edit nobody was making.
        if mouse.just_released(MouseButton::Left) {
            drag.from = None;
        }
        return;
    };

    // **Nothing armed places nothing** — and takes no drag with it, so a press-and-sweep over the map
    // with the palette cleared leaves no box hanging on screen waiting for a release.

    // **An armed group answers the click before the brush does.** Compose's arm and the palette's
    // brush are two ways of saying what the next click puts down; letting both fire would place a
    // piece *and* a group in one gesture. Arming a group is the later, more deliberate act, so it
    // wins — and a group stamps one at a time, never by the box, because a dragged rectangle of
    // nurse stations is not a gesture anybody makes by accident.
    if compose.armed.is_some() {
        if drag.from.is_some() {
            drag.from = None;
        }
        if mouse.just_released(MouseButton::Left) {
            let free = keys::mod_held(&keyboard);
            let at = map_at(&project, hit, free, &anchor);
            stamp_here(&mut project, &mut state, &mut compose, at);
        }
        return;
    }

    let Some(d) = state
        .brush
        .and_then(|ix| project.library.descriptors.get(ix))
        .cloned()
    else {
        if drag.from.is_some() {
            drag.from = None;
        }
        return;
    };
    let free = keys::mod_held(&keyboard);
    let at = map_at(&project, hit, free, &anchor);

    if mouse.just_pressed(MouseButton::Left) {
        drag.from = Some(at);
    }

    // The box, while it is being dragged. An outline rather than the removal tool's filled quad:
    // additive and destructive should not look the same, and an outline leaves the floor it is about
    // to cover visible underneath.
    if let Some(from) = drag.from {
        // The same condition the release below commits on, `free` included — a box drawn for a drag
        // that is going to place one piece would be a preview of something that will not happen,
        // which is the one thing this editor's previews are held to.
        let dragging =
            !free && ((from.0 - at.0).abs() > CLICK_EPS || (from.1 - at.1).abs() > CLICK_EPS);
        if dragging {
            let (x0, z0) = (from.0.min(at.0), from.1.min(at.1));
            let (x1, z1) = (from.0.max(at.0), from.1.max(at.1));
            gizmos.rect(
                Isometry3d::new(
                    Vec3::new(
                        project.map.origin.0 + (x0 + x1) * 0.5,
                        project.map.origin.1 + MARKER_LIFT,
                        project.map.origin.2 + (z0 + z1) * 0.5,
                    ),
                    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                ),
                Vec2::new(x1 - x0, z1 - z0),
                ACCENT,
            );
        }
    }

    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some(from) = drag.from.take() else {
        return;
    };

    // **A drag fills; a click places one.** `CLICK_EPS` is the same threshold the removal tool uses
    // to tell the two apart, so a hand that moved a hair while clicking is still a click in both.
    //
    // **Never while the modifier is held.** Snapped, `from` and `at` are both multiples of `SNAP`, so
    // they are either equal or a whole cell apart and the threshold is really asking "did the cell
    // change". Free, they are continuous and confined to one 0.5 m cell — so an ordinary hand tremor
    // clears 0.2 m without leaving the cell, and the box path would then quantise the result back to
    // that cell's centre. That is the fine placement the modifier exists for, silently discarded.
    // Holding it means "place one, exactly here".
    if !free && ((from.0 - at.0).abs() > CLICK_EPS || (from.1 - at.1).abs() > CLICK_EPS) {
        box_fill_between(
            &mut commands,
            &assets,
            &mut project,
            &mut state,
            &d,
            (from, at),
        );
        return;
    }

    // **What it lands on.** A piece that mounts on a surface must find one under the cursor; the same
    // question the ghost has been answering while the author moved the mouse here, asked once more at
    // the moment it matters.
    let ys = match heights(&project) {
        Ok(ys) => ys,
        Err(e) => {
            state.status = e;
            return;
        }
    };
    let (y, host) =
        match emerge_core::stack::placement_at(&project.map, &project.library, &ys, &d, at) {
            Ok(found) => found,
            // Refused, not floored. Dropping it at floor level is the behaviour this replaced: the
            // piece appears, in the wrong place, and looks like an authoring mistake.
            Err(e) => {
                state.status = e;
                return;
            }
        };
    let on = host.map(|h| h.id.clone());

    // **Occupied space refuses, and names its occupant.** Flush is fine — kitbashing lays pieces
    // end to end, and `stack::OVERLAP_EPS` is what keeps "touching" from reading as "overlapping" —
    // but a piece INSIDE another is an accident that otherwise surfaces as a doubled draw call
    // nobody can see. Same-layer only: the floor a crate stands on is not in its way.
    if let Some(block) = emerge_core::stack::blocking(
        &project.map,
        &project.library,
        &d,
        at,
        state.brush_yaw,
        (0, 0),
        on.as_deref(),
    ) {
        state.status = format!(
            "blocked: `{}` already covers that spot — remove it or place beside it",
            block.id
        );
        return;
    }



    state.next_id += 1;
    let id = format!("{}@{}", short_id(&d.id), state.next_id);

    let placed = Placed {
        id: id.clone(),
        descriptor: d.id.clone(),
        at,
        yaw: state.brush_yaw,
        on: on.clone(),
        ..Placed::default()
    };
    project.map.placements.push(placed);
    project.dirty = true;
    state.record(Undo::Added { count: 1 });

    if let Some(e) = spawn_piece(
        &mut commands,
        &assets,
        &d,
        at,
        state.brush_yaw,
        (0, 0),
        project.map.origin,
        y,
    ) {
        commands.entity(e).insert(Placement(id.clone()));
    }
    // **Free placement says so.** It is a modifier on a mouse click, so it is not an `Action` and
    // cannot appear in the key panel — that panel is generated from the census, and hand-adding a
    // row to it would be the second census this crate keeps deleting. Naming it at the moment it
    // happens is the honest alternative: an author who did it by accident finds out immediately, and
    // one who wanted it sees that it worked.
    let how = if free { " free" } else { "" };
    state.status = match on {
        Some(host) => format!("placed {id} on {host}{how}"),
        None => format!("placed {id} at ({:.2}, {:.2}){how}", at.0, at.1),
    };
}

/// **Put an armed group down** — one line in `map.stamps`, never the rows it stands for.
///
/// This is the verb the whole reference model was chosen for. What lands in the file is a reference,
/// so editing the group later changes this stamp and every other one; baking the rows here would have
/// made each stamp an independent copy that silently stopped tracking its source.
///
/// It refuses whole. `expand` is asked first, against a map that already carries the new stamp, and
/// if any member has nowhere to rest the stamp never joins the list — the same rule a single
/// placement follows, applied to a group.
fn stamp_here(
    project: &mut Project,
    state: &mut EditorState,
    compose: &mut crate::compose::ComposeState,
    at: (f32, f32),
) {
    let Some(of) = compose.armed.clone() else {
        return;
    };
    let comps = project.compositions.compositions.clone();
    if !comps.iter().any(|c| c.id == of) {
        state.status = format!("`{of}` is armed but the project no longer defines it");
        compose.armed = None;
        return;
    }
    state.next_id += 1;
    let id = format!("{}@{}", short_id(&of), state.next_id);

    let stamped = emerge_core::composition::Stamped {
        id: id.clone(),
        of: of.clone(),
        at,
        yaw: state.brush_yaw,
        ..Default::default()
    };
    // **Tried before it is kept.** A trial map rather than a push-then-pop, so a refusal cannot leave
    // the real map holding a stamp that does not resolve.
    let mut trial = project.map.clone();
    trial.stamps.push(stamped.clone());
    if let Err(e) = emerge_core::composition::expand(&trial, &trial.stamps, &comps, &project.library)
    {
        state.status = format!("cannot stamp `{of}` here: {e}");
        return;
    }
    project.map.stamps.push(stamped);
    project.dirty = true;
    state.record(Undo::Stamped { count: 1 });
    state.status = format!("stamped `{of}` as {id}");
}

/// Draw every stamped row, and nothing else.
///
/// **Rebuilt wholesale from `map.stamps` whenever it changes.** A diffing version would be faster and
/// would be a second opinion about what is on screen; this way the picture is a pure function of the
/// list, which is the same contract `Placement` gives the hand-placed half.
///
/// These entities deliberately carry **no `Placement`**, so the remove, move and clone tools cannot
/// see them at all. That is the non-override principle made structural rather than checked: a higher
/// scope must not rewrite what a lower one authored, and here it cannot reach it.
fn redraw_stamps(
    mut commands: Commands,
    assets: Res<AssetServer>,
    project: Res<Project>,
    mut state: ResMut<EditorState>,
    drawn: Query<Entity, With<StampedPiece>>,
) {
    if !project.is_changed() {
        return;
    }
    for e in &drawn {
        commands.entity(e).despawn();
    }
    if project.map.stamps.is_empty() {
        return;
    }
    let expanded = match emerge_core::composition::expand(
        &project.map,
        &project.map.stamps,
        &project.compositions.compositions,
        &project.library,
    ) {
        Ok(e) => e,
        // Loud, and only once per change: a map whose stamps stopped resolving is a map that will not
        // load in the game either, and an empty patch of floor is not how anybody should find out.
        Err(e) => {
            state.status = format!("stamps do not resolve: {e}");
            error!("emerge-mapper: {e}");
            return;
        }
    };
    // The expanded rows are not in `map.placements`, so their heights cannot come from the map's own
    // resolve. A scratch map carrying both answers the question `stack::resolve_y` was written for.
    let mut scratch = project.map.clone();
    scratch.placements.extend(expanded.placements.iter().cloned());
    // Loud. A silent return here is the failure mode this editor's own notes call the worst it had:
    // an empty patch of floor where a group should be, with nothing anywhere saying why.
    let ys = match emerge_core::stack::resolve_y(&scratch, &project.library) {
        Ok(ys) => ys,
        Err(e) => {
            state.status = format!("stamped rows have no height: {e}");
            error!("emerge-mapper: {e}");
            return;
        }
    };
    let first = project.map.placements.len();
    let mut drawn = 0usize;
    for (k, p) in expanded.placements.iter().enumerate() {
        let Some(base) = project.library.get(&p.descriptor) else {
            error!("emerge-mapper: stamped row `{}` names descriptor `{}`, which the library does not define", p.id, p.descriptor);
            continue;
        };
        let d = match &p.patch {
            Some(patch) => base.patched_with(patch),
            None => base.clone(),
        };
        let Some(&y) = ys.get(first + k) else { continue };
        if let Some(e) = spawn_piece(
            &mut commands,
            &assets,
            &d,
            p.at,
            p.yaw,
            p.tip,
            project.map.origin,
            y,
        ) {
            commands.entity(e).insert(StampedPiece);
            drawn += 1;
        }
    }
    let counted = emerge_core::census::of_map(&project.map);
    info!(
        "emerge-mapper: redrew {drawn} stamped row(s) from {} stamp(s)",
        counted.stamps
    );
}

/// A drawn row belonging to a stamp. Carries no id: the stamp list is the truth and this is a
/// picture of it, rebuilt whole.
#[derive(Component)]
struct StampedPiece;

/// Undo and redo, reachable from `tests/headless.rs`.
///
/// One-shot systems rather than re-implemented bodies: `undo` and `redo` are the same function
/// reading opposite stacks, and a test that drove a copy of them would pass while the real pair was
/// broken. `RunSystemError` is surfaced rather than swallowed — a test whose driver silently did
/// nothing is a test that always passes.
pub fn undo_for_test(world: &mut World) {
    use bevy::ecs::system::RunSystemOnce;
    world
        .run_system_once(
            |mut commands: Commands,
             assets: Res<AssetServer>,
             mut project: ResMut<Project>,
             mut state: ResMut<EditorState>,
             placed: Query<(Entity, &Placement)>| {
                undo(&mut commands, &assets, &mut project, &mut state, &placed);
            },
        )
        .unwrap_or_else(|e| panic!("undo_for_test: {e}"));
}

pub fn redo_for_test(world: &mut World) {
    use bevy::ecs::system::RunSystemOnce;
    world
        .run_system_once(
            |mut commands: Commands,
             assets: Res<AssetServer>,
             mut project: ResMut<Project>,
             mut state: ResMut<EditorState>,
             placed: Query<(Entity, &Placement)>| {
                redo(&mut commands, &assets, &mut project, &mut state, &placed);
            },
        )
        .unwrap_or_else(|e| panic!("redo_for_test: {e}"));
}

/// The stamp verb, reachable from `tests/headless.rs`.
///
/// The lib/bin split exists so tests can drive the real thing; this is that split applied to one
/// function, so the test exercises the call the click makes rather than a re-implementation of it.
pub fn stamp_here_for_test(
    project: &mut Project,
    state: &mut EditorState,
    compose: &mut crate::compose::ComposeState,
    at: (f32, f32),
) {
    stamp_here(project, state, compose, at);
}

/// The tail of a descriptor id, so a generated placement id reads as `crate@7` rather than
/// `kenney_prototype-kit/crate@7`.
fn short_id(descriptor_id: &str) -> &str {
    descriptor_id.rsplit('/').next().unwrap_or(descriptor_id)
}

#[allow(clippy::too_many_arguments)]
fn keys(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    assets: Res<AssetServer>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    placed: Query<(Entity, &Placement)>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
    // The aim keys repeat while held, so this system needs a clock and somewhere to keep the
    // countdown. Both are `Res`, and `KeysPlugin` owns `Repeat` for the reason its comment gives:
    // a missing `Res<T>` panics its system in Bevy 0.19 rather than skipping it.
    time: Res<Time>,
    mut repeat: ResMut<keys::Repeat>,
    // One tuple param, three tools: a Bevy system takes at most sixteen parameters, and this
    // one is full — a tuple of params counts as one.
    mut tools: (ResMut<MoveDrag>, ResMut<CloneDrag>, ResMut<TargetLock>),
    // The Tiles tab's state, written by exactly one action here — `EditTile`. See `send_to_tiles`.
    mut mode: ResMut<crate::tiles::Mode>,
    mut import: ResMut<crate::tiles::ImportState>,
) {

    let (move_drag, clone_drag, target) = &mut tools;
    // One clock for every key that repeats while held — see `keys::repeating`.
    let dt = time.delta_secs();

    if keys::just_pressed(&keyboard, live.0, Action::Undo) {
        undo(&mut commands, &assets, &mut project, &mut state, &placed);
        return;
    }

    if keys::just_pressed(&keyboard, live.0, Action::Redo) {
        redo(&mut commands, &assets, &mut project, &mut state, &placed);
        return;
    }

    // **Map to Tiles, carrying the piece.** Before the branches below, because they consume the
    // window and camera singles.
    if keys::just_pressed(&keyboard, live.0, Action::EditTile) {
        send_to_tiles(window, camera, &project, &mut state, &mut mode, &mut import);
        return;
    }

    // **The delete key arms a tool; it does not delete.** Removing on the keypress meant the only
    // preview of what was about to go was the author's memory of where the cursor was. Now the key
    // turns the mode on, the red marker answers "this one", and a click or a dragged box commits.
    if keys::just_pressed(&keyboard, live.0, Action::Remove) {
        // Toggles against `Remove` specifically, not against "any tool": pressing X while the move
        // tool is armed should reach removal, not return to placing.
        state.tool = if state.tool == Tool::Remove {
            Tool::Place
        } else {
            Tool::Remove
        };
        state.status = if state.tool == Tool::Remove {
            format!(
                "removal mode: click a piece, or drag a box. {} or Esc to stop.",
                keys::binding(Action::Remove).chord
            )
        } else {
            "removal mode off".to_owned()
        };
        // Arming one tool puts down whatever the other was holding, rather than leaving a piece in
        // hand that no click can now drop.
        move_drag.held = None;
        return;
    }

    // **The move tool.** Same shape as removal: the key arms it, the click commits. Arming it also
    // clears what was armed to place — an author who means to move something is not also asking to
    // drop a copy of the brush on their first click, and `drive_place` refuses while this is live.
    if keys::just_pressed(&keyboard, live.0, Action::MoveMode) {
        state.tool = if state.tool == Tool::Move {
            Tool::Place
        } else {
            Tool::Move
        };
        state.status = if state.tool == Tool::Move {
            format!(
                "move mode: click a piece to pick it up, click again to put it down. {} or Esc to stop.",
                keys::binding(Action::MoveMode).chord
            )
        } else {
            "move mode off".to_owned()
        };
        move_drag.held = None;
        clone_drag.held = None;
        return;
    }

    // **The clone tool.** The move key's shifted sibling: drag a box to take a copy of everything
    // inside, click to stamp the set — as many times as it is held.
    if keys::just_pressed(&keyboard, live.0, Action::CloneMode) {
        state.tool = if state.tool == Tool::Clone {
            Tool::Place
        } else {
            Tool::Clone
        };
        state.status = if state.tool == Tool::Clone {
            "clone mode: drag a box to take a copy of what is inside, click to stamp it. Esc to stop."
                .to_owned()
        } else {
            "clone mode off".to_owned()
        };
        move_drag.held = None;
        clone_drag.held = None;
        return;
    }

    // **One Esc, and it undoes the most specific thing first.**
    //
    // A piece in hand, then an armed tool, then the armed piece. Each press steps back out one layer,
    // so `Esc` always means "not that" without the author having to work out which of three states
    // they are in — and pressing it twice from the move tool leaves them with a clear palette rather
    // than doing nothing the second time.
    if keys::just_pressed(&keyboard, live.0, Action::Cancel) {
        if move_drag.held.is_some() {
            move_drag.held = None;
            state.status = "put back".to_owned();
            return;
        }
        if clone_drag.holding() {
            clone_drag.held = None;
            state.status = "set put away — the originals never moved".to_owned();
            return;
        }
        if target.0.is_some() {
            target.0 = None;
            state.status = "target released".to_owned();
            return;
        }
        if state.tool != Tool::Place {
            let leaving = state.tool.label();
            state.tool = Tool::Place;
            state.status = format!("{leaving} off");
            return;
        }
        // **Clearing the selection is a real state**, not a no-op: with nothing armed the ghost goes,
        // a click places nothing, and the palette shows no highlighted row — so the cursor can be over
        // the map without a piece following it.
        if state.brush.take().is_some() {
            state.status = "selection cleared".to_owned();
        }
        return;
    }

    if keys::just_pressed(&keyboard, live.0, Action::Save) {
        match project.save() {
            Ok(()) => {
                let path = project.map_path.display().to_string();
                state.status = format!("saved {path}");
                info!("saved {path}");
            }
            // The save refuses on an invalid map rather than writing one, and says which rule.
            Err(e) => {
                state.status = format!("NOT SAVED: {e}");
                error!("{e}");
            }
        }
        return;
    }

    // **`R` and `T` turn the piece under the cursor.** The other half of aiming: `Z`/`C` set the
    // brush's facing before a piece exists, and these fix one that is already down — three chairs
    // round a table were three chairs facing the same way until this existed.
    //
    // **Held, like the aim keys.** `YAW_STEP` is 15 degrees, so squaring a piece that arrived at 240
    // was sixteen presses. `keys::repeating` fires the press at once and then every
    // `keys::REPEAT_SECS`, so tapping is unchanged and only holding is new — the same treatment
    // `AimLeft`/`AimRight` got, for the same reason, through the same function.
    for (action, step) in [
        (Action::TurnPieceLeft, -YAW_STEP),
        (Action::TurnPieceRight, YAW_STEP),
    ] {
        if keys::repeating(&keyboard, live.0, action, &mut repeat, dt)
            && !hovered_ui.iter().any(|h| h.0)
        {
            turn_under_cursor(
                &mut commands,
                &assets,
                window,
                camera,
                &mut project,
                &mut state,
                &placed,
                target.as_mut(),
                step,
            );
            return;
        }
    }

    // **`H` targets the stack** — see `cycle_target`; the verbs below act on its pick.
    if keys::just_pressed(&keyboard, live.0, Action::CycleTarget)
        && !hovered_ui.iter().any(|h| h.0)
    {
        cycle_target(window, camera, &project, &mut state, target.as_mut());
        return;
    }

    // **`Y` and `U` tip the piece under the cursor** — a quarter turn about X or Z per press.
    // Deliberately `just_pressed` where the yaw keys repeat: each axis has four states, and a held
    // key cycling them at repeat pace reads as flicker, not control.
    for (action, about_x) in [(Action::TipX, true), (Action::TipZ, false)] {
        if keys::just_pressed(&keyboard, live.0, action) && !hovered_ui.iter().any(|h| h.0) {
            tip_under_cursor(
                &mut commands,
                &assets,
                window,
                camera,
                &mut project,
                &mut state,
                &placed,
                target.as_mut(),
                about_x,
            );
            return;
        }
    }

    // **The brackets lift** — one subgrid unit per press, held like the turn keys because a piece
    // three metres up is a long tap-tap-tap otherwise.
    for (action, sign) in [(Action::LiftUp, 1.0), (Action::LiftDown, -1.0)] {
        if keys::repeating(&keyboard, live.0, action, &mut repeat, dt)
            && !hovered_ui.iter().any(|h| h.0)
        {
            lift_under_cursor(
                &mut commands,
                &assets,
                window,
                camera,
                &mut project,
                &mut state,
                &placed,
                target.as_mut(),
                sign,
            );
            return;
        }
    }

    // **O pins or unpins the piece under the cursor.** A pin is what the solver routes around.
    if keys::just_pressed(&keyboard, live.0, Action::OwnToggle) && !hovered_ui.iter().any(|h| h.0) {
        toggle_pin(window, camera, &mut project, &mut state, target.as_mut());
        return;
    }

    // **G continues the layout.** Learn the grammar from what is already placed, then fill the free
    // cells with more of it — see `emerge_core::grammar`.
    if keys::just_pressed(&keyboard, live.0, Action::GenerateDeclared) {
        generate_from(
            &mut commands,
            &assets,
            &mut project,
            &mut state,
            &placed,
            Source::Declared,
        );
    }
    if keys::just_pressed(&keyboard, live.0, Action::Generate) {
        generate(&mut commands, &assets, &mut project, &mut state, &placed);
        return;
    }

    // **F floods.** From the cell under the cursor outward, stopping at anything already placed and
    // at the map's edge — see `crate::fill`.
    if keys::just_pressed(&keyboard, live.0, Action::Fill) && !hovered_ui.iter().any(|h| h.0) {
        flood_from_cursor(
            &mut commands,
            &assets,
            window,
            camera,
            &mut project,
            &mut state,
        );
        return;
    }

    // **X puts the brush back where it started.** Turning is relative, so a piece three quarters round
    // is one press from straight in one direction and three in the other — and an author who has been
    // tapping `Z` has no reason to be keeping count. This is the only absolute among the aim keys.
    if keys::just_pressed(&keyboard, live.0, Action::AimReset) {
        state.brush_yaw = 0.0;
        state.status = "brush aimed straight again".to_owned();
        return;
    }

    // **Held, not only tapped.** Turning a brush to 240 degrees was sixteen presses of `C`; it is now
    // one held key. `keys::repeating` fires the press immediately and then every
    // `keys::REPEAT_SECS`, so tapping is unchanged and only holding is new — the comment above about
    // "an author who has been tapping `Z`" is still true, there is just less of it.
    let step = if keys::repeating(&keyboard, live.0, Action::AimRight, &mut repeat, dt) {
        YAW_STEP
    } else if keys::repeating(&keyboard, live.0, Action::AimLeft, &mut repeat, dt) {
        -YAW_STEP
    } else {
        0.0
    };
    if step != 0.0 {
        state.brush_yaw = (state.brush_yaw + step).rem_euclid(360.0);
    }
}

/// Remove the placement nearest the cursor, within a piece's own reach.
///
/// Nearest-within-a-radius rather than a picking ray: the pieces are GLB scenes with no colliders, and
/// a ray would need every one of them to be pickable — which would also make every one of them eat
/// the click that places the next piece. The radius is the brush cell, so "delete what I am pointing
/// at" means the same distance as "place one here".
fn delete_index(
    commands: &mut Commands,
    index: usize,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
) {
    if index >= project.map.placements.len() {
        state.status = "nothing here to remove".to_owned();
        return;
    }
    // **A stack goes whole.** `Placed::on` is a hard reference, so removing a host on its own left
    // its riders pointing at a placement that no longer existed — and `resolve_y` then refused the
    // whole map with *"rests on `table@3`, which does not exist"*. The editor could not draw it, the
    // file could not be reloaded, and the only thing an author had done was delete a table.
    //
    // The same set the move tool carries, from the same function, so "this piece and what it holds up"
    // means one thing in both verbs.
    let group = emerge_core::stack::group_of(&project.map, index);
    let head = project.map.placements[index].id.clone();

    // **Back to front.** Removing an earlier row shifts every later one down, so a forward pass would
    // take the wrong pieces the moment the group held more than one — the rule the box removal below
    // already follows, and now the single delete needs it too.
    let mut ordered = group.clone();
    ordered.sort_unstable();
    let mut items: Vec<(usize, Box<Placed>)> = Vec::with_capacity(ordered.len());
    for i in ordered.iter().rev() {
        let removed = project.map.placements.remove(*i);
        for (entity, marker) in placed {
            if marker.0 == removed.id {
                commands.entity(entity).despawn();
            }
        }
        items.push((*i, Box::new(removed)));
    }
    // Ascending, which is the order `Undo::RemovedMany` puts them back in — an earlier row returning
    // first shifts the later ones into place.
    items.reverse();
    project.dirty = true;
    state.status = match items.len() {
        1 => format!("removed {head}"),
        n => format!("removed {head} and {} on it", n - 1),
    };
    state.record(Undo::RemovedMany { items });
}

/// Take back the last edit.
fn undo(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
) {
    let Some(op) = state.undo.pop() else {
        state.status = "nothing to undo".to_owned();
        return;
    };
    if let Some(inverse) = apply(commands, assets, project, state, placed, op) {
        state.redo.push(inverse);
    }
}

/// Take back the last thing undone.
fn redo(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
) {
    let Some(op) = state.redo.pop() else {
        state.status = "nothing to redo".to_owned();
        return;
    };
    // **`undo.push`, not `record`.** `record` clears the redo stack, which is right for a *new* edit
    // and exactly wrong here: redoing the first of five undone steps would throw away the other four,
    // so a redo could never be repeated. Redo moves an entry between the stacks; it does not author
    // anything.
    if let Some(inverse) = apply(commands, assets, project, state, placed, op) {
        state.undo.push(inverse);
    }
}

/// **Perform one reversal, and hand back the reversal of it.**
///
/// The single body behind both stacks — see [`Undo`] on why the enum is closed under inversion. A
/// `None` return means the operation could not be applied and nothing should be pushed anywhere,
/// which is how a half-applied edit is kept out of the other stack.
fn apply(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
    op: Undo,
) -> Option<Undo> {
    let inverse = match op {
        Undo::Added { count } => {
            let keep = project.map.placements.len().saturating_sub(count);
            let taken: Vec<(usize, Box<Placed>)> = project
                .map
                .placements
                .drain(keep..)
                .enumerate()
                .map(|(n, p)| (keep + n, Box::new(p)))
                .collect();
            let gone: Vec<String> = taken.iter().map(|(_, p)| p.id.clone()).collect();
            for (entity, marker) in placed {
                if gone.contains(&marker.0) {
                    commands.entity(entity).despawn();
                }
            }
            state.status = format!("undid {} placement(s)", gone.len());
            Undo::RemovedMany { items: taken }
        }
        Undo::Moved { moved } => {
            // **The inverse is where they are NOW**, read before the restore puts them back. `moved`
            // records where they came *from*; redoing needs where they went *to*.
            let now = emerge_core::stack::Moved {
                was: moved
                    .was
                    .iter()
                    .filter_map(|(i, _, _)| {
                        project.map.placements.get(*i).map(|p| (*i, p.at, p.on.clone()))
                    })
                    .collect(),
            };
            // `restore_moved` is the recorded inverse, so the undo cannot drift from the move: it
            // replays exactly the `(at, on)` pairs the move displaced, rather than recomputing where
            // things "should" go and hoping the two agree.
            emerge_core::stack::restore_moved(&mut project.map, &moved);
            let ids: Vec<String> = moved
                .was
                .iter()
                .filter_map(|(i, _, _)| project.map.placements.get(*i).map(|p| p.id.clone()))
                .collect();
            for (entity, marker) in placed {
                if ids.contains(&marker.0) {
                    commands.entity(entity).despawn();
                }
            }
            // Restored first, drawn second — how high a piece sits is a question about the finished
            // map, the same reason `RemovedMany` below waits.
            match heights(project) {
                Ok(ys) => {
                    for (i, _, _) in &moved.was {
                        let Some(p) = project.map.placements.get(*i) else {
                            continue;
                        };
                        let (id, at, yaw, tip) = (p.id.clone(), p.at, p.yaw, p.tip);
                        let (Some(d), Some(&y)) =
                            (project.library.get(&p.descriptor).cloned(), ys.get(*i))
                        else {
                            continue;
                        };
                        if let Some(e) =
                            spawn_piece(commands, assets, &d, at, yaw, tip, project.map.origin, y)
                        {
                            commands.entity(e).insert(Placement(id));
                        }
                    }
                }
                Err(e) => {
                    state.status = format!("put back, but cannot draw it: {e}");
                    error!("{e}");
                    // `restore_moved` above ALREADY moved the rows, so the map has changed whatever
                    // happened to the drawing — skipping the shared dirty write at the tail here left
                    // a real edit looking saved.
                    project.dirty = true;
                    // Nothing goes on the other stack: the map moved but could not be drawn, and
                    // offering to redo a state the author cannot see would compound it.
                    return None;
                }
            }
            state.status = format!("put back {} placement(s)", moved.was.len());
            Undo::Moved { moved: now }
        }
        Undo::RemovedMany { items } => {
            let n = items.len();
            // Rows back first, all of them, and only THEN drawn: how high a piece sits is a question
            // about the finished map, so asking `heights` mid-restore would answer it against a map
            // that is still missing some of its own contents.
            let mut at_indices: Vec<usize> = Vec::with_capacity(n);
            for (index, p) in items {
                let at = index.min(project.map.placements.len());
                project.map.placements.insert(at, *p);
                at_indices.push(at);
            }
            let at_indices_for_inverse = at_indices.clone();
            match heights(project) {
                Ok(ys) => {
                    for at in at_indices {
                        let Some(p) = project.map.placements.get(at) else {
                            continue;
                        };
                        let (id, pat, pyaw, ptip) = (p.id.clone(), p.at, p.yaw, p.tip);
                        let (Some(d), Some(&y)) =
                            (project.library.get(&p.descriptor).cloned(), ys.get(at))
                        else {
                            continue;
                        };
                        if let Some(e) =
                            spawn_piece(commands, assets, &d, pat, pyaw, ptip, project.map.origin, y)
                        {
                            commands.entity(e).insert(Placement(id));
                        }
                    }
                    state.status = format!("restored {n} placement(s)");
                }
                Err(e) => {
                    state.status = format!("restored {n} but cannot draw them: {e}");
                    error!("{e}");
                }
            }
            Undo::RemoveAt {
                indices: at_indices_for_inverse,
            }
        }
        Undo::RemoveAt { indices } => {
            // Descending, so removing an earlier row cannot shift a later one out from under us —
            // the rule every removal in this file follows.
            let mut ordered = indices.clone();
            ordered.sort_unstable();
            let mut items: Vec<(usize, Box<Placed>)> = Vec::with_capacity(ordered.len());
            for i in ordered.iter().rev() {
                if *i >= project.map.placements.len() {
                    continue;
                }
                let removed = project.map.placements.remove(*i);
                for (entity, marker) in placed {
                    if marker.0 == removed.id {
                        commands.entity(entity).despawn();
                    }
                }
                items.push((*i, Box::new(removed)));
            }
            items.reverse();
            state.status = format!("removed {} placement(s) again", items.len());
            Undo::RemovedMany { items }
        }
        Undo::Turned { index, yaw } => {
            let Some(p) = project.map.placements.get_mut(index) else {
                return None;
            };
            let was = std::mem::replace(&mut p.yaw, yaw);
            let (id, at, tip, descriptor) = (p.id.clone(), p.at, p.tip, p.descriptor.clone());
            for (entity, marker) in placed {
                if marker.0 == id {
                    commands.entity(entity).despawn();
                }
            }
            // The identical failure the forward path (`turn_under_cursor`) reports loudly: an
            // `if let Ok` here swallowed it, leaving the piece despawned with no respawn under a
            // success message — a piece missing from screen is the one thing a status line must
            // never be cheerful about. The yaw HAS changed either way, so the inverse still stands.
            match heights(project) {
                Ok(ys) => {
                    if let (Some(d), Some(&y)) =
                        (project.library.get(&descriptor).cloned(), ys.get(index))
                    {
                        if let Some(e) =
                            spawn_piece(commands, assets, &d, at, yaw, tip, project.map.origin, y)
                        {
                            commands.entity(e).insert(Placement(id.clone()));
                        }
                    }
                    state.status = format!("{id} back to {yaw:.0} deg");
                }
                Err(e) => {
                    state.status = format!("turned {id} back but cannot draw it: {e}");
                    error!("{e}");
                }
            }
            Undo::Turned { index, yaw: was }
        }
        Undo::Lifted { index, lift } => {
            let Some(p) = project.map.placements.get_mut(index) else {
                return None;
            };
            let was = std::mem::replace(&mut p.lift, lift);
            let id = p.id.clone();
            // The whole ride: everything resting on this piece moved with the lift, so it all
            // comes back down (or up) together.
            let group = with_dependents(&project.map, index);
            match redraw_placements(commands, assets, project, placed, &group) {
                Ok(()) => {
                    state.status = if lift == 0.0 {
                        format!("{id} back on its datum")
                    } else {
                        format!("{id} back to {lift:+.2} m of lift")
                    };
                }
                Err(e) => {
                    state.status = format!("lifted {id} back but cannot draw it: {e}");
                    error!("{e}");
                }
            }
            Undo::Lifted { index, lift: was }
        }
        Undo::Tipped { index, tip } => {
            let Some(p) = project.map.placements.get_mut(index) else {
                return None;
            };
            let was = std::mem::replace(&mut p.tip, tip);
            let id = p.id.clone();
            match redraw_placements(commands, assets, project, placed, &[index]) {
                Ok(()) => {
                    state.status = if tip == (0, 0) {
                        format!("{id} upright again")
                    } else {
                        format!("{id} back to {}/4 about X, {}/4 about Z", tip.0, tip.1)
                    };
                }
                Err(e) => {
                    state.status = format!("tipped {id} back but cannot draw it: {e}");
                    error!("{e}");
                }
            }
            Undo::Tipped { index, tip: was }
        }
        Undo::Group { ops } => {
            // Applied in order; the inverse is the sub-inverses REVERSED — the composition rule, and
            // the whole reason a group can exist without a second undo mechanism. A sub-op that
            // could not apply contributes nothing; if none could, there is nothing to put on the
            // other stack.
            let mut inverses = Vec::with_capacity(ops.len());
            for op in ops {
                if let Some(inv) = apply(commands, assets, project, state, placed, op) {
                    inverses.push(inv);
                }
            }
            if inverses.is_empty() {
                return None;
            }
            inverses.reverse();
            Undo::Group { ops: inverses }
        }
        // **Stamps come off the list, and the drawn rows follow.** The entities are not despawned
        // here: `redraw_stamps` watches `map.stamps` and rebuilds the whole stamped set from it, so
        // there is one place that turns a stamp list into pictures rather than two that could
        // disagree about what is on screen.
        Undo::Stamped { count } => {
            let n = project.map.stamps.len();
            let from = n.saturating_sub(count);
            let items: Vec<(usize, Box<emerge_core::composition::Stamped>)> = project
                .map
                .stamps
                .drain(from..)
                .enumerate()
                .map(|(k, st)| (from + k, Box::new(st)))
                .collect();
            if items.is_empty() {
                return None;
            }
            state.status = format!("took back {} stamp(s)", items.len());
            Undo::UnstampedMany { items }
        }
        Undo::UnstampedMany { items } => {
            // Ascending by index, so each lands where it came from — `RemovedMany`'s rule.
            let count = items.len();
            for (at, st) in items {
                let at = at.min(project.map.stamps.len());
                project.map.stamps.insert(at, *st);
            }
            state.status = format!("put {count} stamp(s) back");
            Undo::Stamped { count }
        }
        Undo::Pinned {
            index,
            owned,
            because,
        } => {
            let Some(p) = project.map.placements.get_mut(index) else {
                return None;
            };
            let was_owned = std::mem::replace(&mut p.owned, owned);
            let was_because = std::mem::replace(&mut p.owned_because, because);
            state.status = format!(
                "{} {}",
                p.id,
                if owned { "pinned again" } else { "unpinned" }
            );
            Undo::Pinned {
                index,
                owned: was_owned,
                because: was_because,
            }
        }
    };
    project.dirty = true;
    Some(inverse)
}

/// Pin or unpin the placement nearest the cursor.
///
/// Unpinning is immediate; pinning asks for a reason first, because that is what the field is for.
fn toggle_pin(
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &mut Project,
    state: &mut EditorState,
    lock: &mut TargetLock,
) {
    let Some(index) = under_cursor_target(lock, window, camera, project) else {
        state.status = "nothing here to pin".to_owned();
        return;
    };
    let Some(p) = project.map.placements.get_mut(index) else {
        return;
    };
    if p.owned {
        let because = p.owned_because.clone();
        p.owned = false;
        p.owned_because = None;
        state.record(Undo::Pinned {
            index,
            owned: true,
            because,
        });
        project.dirty = true;
        state.status = format!("unpinned {}", p.id);
    } else {
        let id = p.id.clone();
        state.pinning = Some((index, String::new()));
        state.status = format!("why is {id} pinned? Enter to keep it, Esc to cancel");
    }
}

/// Turn the placement under the cursor by `step` degrees, and redraw it.
///
/// Rewrites the yaw in the map and respawns the entity rather than rotating the transform in place:
/// the file is the truth and the entity is a picture of it, so turning the picture without turning the
/// file is the class of bug where a save loses what you just did.
#[allow(clippy::too_many_arguments)]
fn turn_under_cursor(
    commands: &mut Commands,
    assets: &AssetServer,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
    lock: &mut TargetLock,
    step: f32,
) {
    let Some(index) = under_cursor_target(lock, window, camera, project) else {
        state.status = "nothing here to turn".to_owned();
        return;
    };
    let Some(p) = project.map.placements.get_mut(index) else {
        return;
    };
    // Recorded before it moves — the entry carries the angle to go back to.
    let was_yaw = p.yaw;
    p.yaw = (p.yaw + step).rem_euclid(360.0);
    let (id, at, yaw, tip, descriptor) = (p.id.clone(), p.at, p.yaw, p.tip, p.descriptor.clone());
    project.dirty = true;
    state.record(Undo::Turned {
        index,
        yaw: was_yaw,
    });

    for (entity, marker) in placed {
        if marker.0 == id {
            commands.entity(entity).despawn();
        }
    }
    // Redrawn from the finished map, so a piece standing on this one keeps the height it had.
    match heights(project) {
        Ok(ys) => {
            if let (Some(d), Some(&y)) = (project.library.get(&descriptor).cloned(), ys.get(index)) {
                if let Some(e) = spawn_piece(commands, assets, &d, at, yaw, tip, project.map.origin, y) {
                    commands.entity(e).insert(Placement(id.clone()));
                }
            }
        }
        Err(e) => {
            state.status = format!("turned {id} but cannot draw it: {e}");
            error!("{e}");
            return;
        }
    }
    state.status = format!("{id} now at {yaw:.0} deg");
}

/// Despawn and redraw the placements at `indices` from the finished map — the shared tail of the
/// lift and tip paths, where a change to one piece can move what rides on it. Heights are resolved
/// once, over the whole map, for the reason `spawn_range` gives.
fn redraw_placements(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &Project,
    placed: &Query<(Entity, &Placement)>,
    indices: &[usize],
) -> Result<(), String> {
    let ids: Vec<String> = indices
        .iter()
        .filter_map(|i| project.map.placements.get(*i).map(|p| p.id.clone()))
        .collect();
    for (entity, marker) in placed {
        if ids.iter().any(|id| id == &marker.0) {
            commands.entity(entity).despawn();
        }
    }
    let ys = heights(project)?;
    for &i in indices {
        let Some(p) = project.map.placements.get(i) else {
            continue;
        };
        let (Some(d), Some(&y)) = (project.library.get(&p.descriptor), ys.get(i)) else {
            continue;
        };
        if let Some(e) =
            spawn_piece(commands, assets, d, p.at, p.yaw, p.tip, project.map.origin, y)
        {
            commands.entity(e).insert(Placement(p.id.clone()));
        }
    }
    Ok(())
}

/// `root` and everything that (transitively) rests on it, as indices into the placement list —
/// what a lift must redraw, because raising the table moves the lamp, and the candle on the lamp.
fn with_dependents(map: &emerge_core::map::Map, root: usize) -> Vec<usize> {
    let mut out = vec![root];
    let mut ids: Vec<&str> = map
        .placements
        .get(root)
        .map(|p| vec![p.id.as_str()])
        .unwrap_or_default();
    let mut grew = true;
    while grew {
        grew = false;
        for (i, p) in map.placements.iter().enumerate() {
            if out.contains(&i) {
                continue;
            }
            if p.on.as_deref().is_some_and(|on| ids.contains(&on)) {
                out.push(i);
                ids.push(p.id.as_str());
                grew = true;
            }
        }
    }
    out
}

/// One subgrid unit of authored lift — the same subdivision the lattice uses, so "up one notch"
/// means the same distance everywhere in the project.
fn lift_step(project: &Project) -> f32 {
    emerge_core::grid::SNAP / project.policy.divisions.max(1) as f32
}

/// **Raise or lower the placement under the cursor by one subgrid unit.**
///
/// This writes [`emerge_core::map::Placed::lift`] — the one deliberate amendment to "the height
/// comes from the mount, never from the author". The datum still comes from `stack::resolve_y`,
/// which is why everything resting on this piece is redrawn with it: the lamp follows the table.
#[allow(clippy::too_many_arguments)]
fn lift_under_cursor(
    commands: &mut Commands,
    assets: &AssetServer,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
    lock: &mut TargetLock,
    sign: f32,
) {
    let Some(index) = under_cursor_target(lock, window, camera, project) else {
        state.status = "nothing here to lift".to_owned();
        return;
    };
    let step = lift_step(project);
    let Some(p) = project.map.placements.get_mut(index) else {
        return;
    };
    let was = p.lift;
    let mut want = p.lift + sign * step;
    // Snap the float noise out of zero so a piece brought home is exactly home and the file drops
    // the field — `Placed::lift` skips a zero on write.
    if want.abs() < step * 1e-3 {
        want = 0.0;
    }
    p.lift = want;
    let id = p.id.clone();
    project.dirty = true;
    state.record(Undo::Lifted { index, lift: was });
    let group = with_dependents(&project.map, index);
    match redraw_placements(commands, assets, project, placed, &group) {
        Ok(()) => {
            state.status = if want == 0.0 {
                format!("{id} back on its datum")
            } else {
                format!("{id} lifted {want:+.2} m")
            };
        }
        Err(e) => {
            state.status = format!("lifted {id} but cannot draw it: {e}");
            error!("{e}");
        }
    }
}

/// **Tip the placement under the cursor a quarter turn** — about X on `Y`, about Z on `U`.
///
/// Set dressing, with two refusals that are the schema's own rather than this function's taste:
/// a piece something rests on cannot tip (its guests' heights name a surface that would no longer
/// exist — `stack::host_under` skips tipped hosts for the same reason), and an unmeasured piece
/// cannot tip (`emerge_bevy::tip_seat` cannot seat bounds it does not know, so the mesh would sink
/// through the floor).
#[allow(clippy::too_many_arguments)]
fn tip_under_cursor(
    commands: &mut Commands,
    assets: &AssetServer,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
    lock: &mut TargetLock,
    about_x: bool,
) {
    let Some(index) = under_cursor_target(lock, window, camera, project) else {
        state.status = "nothing here to tip".to_owned();
        return;
    };
    let Some(p) = project.map.placements.get(index) else {
        return;
    };
    let resting = project
        .map
        .placements
        .iter()
        .filter(|q| q.on.as_deref() == Some(p.id.as_str()))
        .count();
    if resting > 0 {
        state.status = format!(
            "`{}` holds {resting} piece(s) up — a tipped surface holds nothing; move them first",
            p.id
        );
        return;
    }
    let want = if about_x {
        ((p.tip.0 + 1) % 4, p.tip.1)
    } else {
        (p.tip.0, (p.tip.1 + 1) % 4)
    };
    let Some(d) = project.library.get(&p.descriptor) else {
        return;
    };
    if emerge_core::descriptor::tipped_extents(d, want).is_none() {
        state.status = format!(
            "`{}` is unmeasured — a tip cannot be seated. Measure it on the tiles tab first.",
            p.id
        );
        return;
    }
    let Some(p) = project.map.placements.get_mut(index) else {
        return;
    };
    let was = std::mem::replace(&mut p.tip, want);
    let id = p.id.clone();
    project.dirty = true;
    state.record(Undo::Tipped { index, tip: was });
    match redraw_placements(commands, assets, project, placed, &[index]) {
        Ok(()) => {
            state.status = if want == (0, 0) {
                format!("{id} upright again")
            } else {
                format!("{id} tipped {}/4 about X, {}/4 about Z", want.0, want.1)
            };
        }
        Err(e) => {
            state.status = format!("tipped {id} but cannot draw it: {e}");
            error!("{e}");
        }
    }
}

/// **A carried piece leaves its spot.**
///
/// Picking something up should look like picking something up: the ghost follows the cursor and the
/// hole it came out of stays a hole, exactly as it does when placing a new piece. Without this the
/// original sat where it was and the ghost hovered elsewhere, so a move read as *copying* right up to
/// the moment it committed.
///
/// **The whole group goes**, not just the anchor — a table hidden with its lamp still floating in mid
/// air would be worse than not hiding anything.
///
/// Visibility only: the map row, the entity and the file are all untouched while a piece is in hand,
/// which is what makes `Esc` a matter of putting the visibility back rather than reversing a move.
/// See [`MoveDrag`].
fn hide_carried(
    drag: Res<MoveDrag>,
    project: Res<Project>,
    mut placed: Query<(&Placement, &mut Visibility)>,
) {
    let carried: Vec<String> = drag
        .held
        .as_ref()
        .and_then(|id| project.map.placements.iter().position(|p| &p.id == id))
        .map(|ix| emerge_core::stack::group_of(&project.map, ix))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| project.map.placements.get(i).map(|p| p.id.clone()))
        .collect();

    for (marker, mut vis) in &mut placed {
        let want = if carried.iter().any(|id| *id == marker.0) {
            Visibility::Hidden
        } else {
            // Nothing else in this crate manages a placement's visibility — `spawn_descriptor` gives
            // it `Inherited` and leaves it there — so restoring unconditionally cannot fight another
            // system for it. Written only on a change, or this marks the component every frame.
            Visibility::Inherited
        };
        if *vis != want {
            *vis = want;
        }
    }
}

/// **Send the piece under the cursor to the Tiles tab**, and go there.
///
/// The gap this closes: a map is where you *notice* a piece is wrong — too big, floating, facing the
/// wrong way — and until now the only route from noticing to fixing was to read its id off the status
/// block, switch tabs, and find it in a list of forty. The map already knows which piece you mean.
///
/// It sends the **descriptor**, not the placement: what the Tiles tab edits is the definition, so
/// every copy on the map moves with the edit. That is the point of editing it there rather than
/// patching one placement.
fn send_to_tiles(
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &Project,
    state: &mut EditorState,
    mode: &mut crate::tiles::Mode,
    import: &mut crate::tiles::ImportState,
) {
    let Some(index) = nearest_placement(window, camera, project) else {
        state.status = "nothing here to edit".to_owned();
        return;
    };
    let Some(id) = project.map.placements.get(index).map(|p| p.descriptor.clone()) else {
        return;
    };
    // A placement naming a descriptor the library does not have is a map/library mismatch, and
    // switching tabs to show an empty pane would report it as nothing happening.
    if project.library.get(&id).is_none() {
        state.status = format!("`{id}` is not in this library, so there is nothing to edit");
        return;
    }
    // `selected_library_id` is the discriminant `ImportState::editing` follows, so setting it is the
    // whole of "focus this piece" — the detail pane, the preview, the lattice and the fields all read
    // through that one accessor.
    import.selected_library_id = Some(id.clone());
    import.status = format!("editing `{id}`, sent from the map");
    *mode = crate::tiles::Mode::Tiles;
    state.status = format!("`{id}` — opened on the tiles tab");
}

/// **Pick a piece up, and put it down** — the whole of [`Tool::Move`].
///
/// Click-to-grab and click-to-drop rather than press-drag-release. A carried piece stays carried
/// across a camera pan, so an author can move something further than one screenful, and there is no
/// deadzone to tune: a click either grabbed something or said it did not.
#[allow(clippy::too_many_arguments)]
fn drive_move(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    // Held, the platform modifier drops the grid snap — see `map_at`.
    keyboard: Res<ButtonInput<KeyCode>>,
    assets: Res<AssetServer>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    placed: Query<(Entity, &Placement)>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
    mut drag: ResMut<MoveDrag>,
    anchor: Res<FineAnchor>,
) {
    // Read, never write, unless something happened: `state` is watched by `rebuild_palette`, and an
    // unconditional deref here rebuilds all forty-odd rows every frame. Same rule as `drive_removal`.
    if state.tool != Tool::Move {
        if drag.held.is_some() {
            drag.held = None;
        }
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // A click on a control is not a click on the world.
    if hovered_ui.iter().any(|h| h.0) {
        return;
    }
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        return;
    };

    match drag.held.clone() {
        // **Grab.** Picked unsnapped, because "which piece did I mean" is a question about where the
        // cursor actually is — snapping first would answer it about the middle of a cell.
        None => {
            let probe = project.map.to_map_space((hit.x, hit.z));
            let Some(index) = pick_at(&project, probe) else {
                state.status = "nothing here to move".to_owned();
                return;
            };
            let Some(p) = project.map.placements.get(index) else {
                return;
            };
            let id = p.id.clone();
            state.status = format!("{id} in hand — click to put it down, Esc to put it back");
            drag.held = Some(id);
        }
        // **Drop.** Snapped like a placement, and free with the modifier held, so a move lands on the
        // same grid a place would.
        Some(id) => {
            // Resolved now, not at grab: an undo or a fill between the two clicks may have moved this
            // row, and it may have removed it outright.
            let Some(index) = project.map.placements.iter().position(|p| p.id == id) else {
                drag.held = None;
                state.status = format!("`{id}` is gone — nothing was moved");
                return;
            };
            let free = keys::mod_held(&keyboard);
            let at = map_at(&project, hit, free, &anchor);
            // One `deref_mut`, then two disjoint field borrows. `ResMut`'s `Deref` cannot split them
            // for us, so `(&mut project.map, &project.library)` is a double borrow of the resource.
            let p = &mut *project;
            let moved = match emerge_core::stack::move_placement(
                &mut p.map,
                &p.library,
                index,
                at,
            ) {
                Ok(moved) => moved,
                // **Refused, and still in hand.** Dropping it anyway is the behaviour this exists to
                // avoid: the piece would land somewhere its mount does not hold and read as an
                // authoring mistake. Keeping hold of it means the next click is another try.
                Err(e) => {
                    state.status = e;
                    return;
                }
            };
            project.dirty = true;
            drag.held = None;

            // Everything that moved is redrawn from the finished map — never by nudging a transform.
            // `turn_under_cursor` states the rule; this obeys it for a whole group, because a lamp
            // whose table moved has a new `y` in the data and an entity still standing over the old
            // spot.
            let ids: Vec<String> = moved
                .was
                .iter()
                .filter_map(|(i, _, _)| project.map.placements.get(*i).map(|p| p.id.clone()))
                .collect();
            for (entity, marker) in &placed {
                if ids.iter().any(|id| id == &marker.0) {
                    commands.entity(entity).despawn();
                }
            }
            match heights(&project) {
                Ok(ys) => {
                    for (i, _, _) in &moved.was {
                        let Some(p) = project.map.placements.get(*i) else {
                            continue;
                        };
                        let (id, at, yaw, tip) = (p.id.clone(), p.at, p.yaw, p.tip);
                        let (Some(d), Some(&y)) =
                            (project.library.get(&p.descriptor).cloned(), ys.get(*i))
                        else {
                            continue;
                        };
                        if let Some(e) =
                            spawn_piece(&mut commands, &assets, &d, at, yaw, tip, project.map.origin, y)
                        {
                            commands.entity(e).insert(Placement(id));
                        }
                    }
                }
                Err(e) => {
                    state.status = format!("moved, but cannot draw it: {e}");
                    error!("{e}");
                    state.record(Undo::Moved { moved });
                    return;
                }
            }

            let carried = moved.was.len();
            state.record(Undo::Moved { moved });
            let head = ids.first().cloned().unwrap_or_default();
            let how = if free { " free" } else { "" };
            state.status = if carried > 1 {
                format!("moved {head} and {} riding on it{how}", carried - 1)
            } else {
                format!("moved {head}{how}")
            };
        }
    }
}

/// **The thing you are pointing at**, given a probe in map space.
///
/// Pure, and separated from the cursor for exactly that reason: the rule below is the whole content
/// of "which piece did I mean", and proving it through a window means aiming a synthetic mouse at a
/// 0.45 m chair — which is how the last two sessions lost an hour each to the harness rather than to
/// the code.
///
/// Two passes, and the first is the fix. It used to be distance-to-`at` alone, which reads as "the
/// nearest centre" and is wrong the moment anything stands on anything: a lamp on a table has almost
/// exactly the table's `at`, so pointing at the lamp could delete the table. That is not
/// hypothetical — it happened while authoring `break_room.map.ron`.
///
/// 1. **Anything whose footprint contains the probe**, smallest first. The piece you can see least of
///    is the one you must have been aiming at.
/// 2. Failing that, the nearest centre within **its own** reach — its own, not the brush's. The reach
///    used to come from whatever piece was armed, so how far you could grab a chair depended on what
///    you happened to be holding. What you are pointing at is not a property of what you are carrying.
///
/// Ties break on the placement id, a total order that does not depend on authoring order.
pub fn pick_at(project: &Project, probe: (f32, f32)) -> Option<usize> {
    let mut covering: Option<(usize, f32, &str)> = None;
    for (i, p) in project.map.placements.iter().enumerate() {
        let Some(d) = project.library.get(&p.descriptor) else {
            continue;
        };
        if !emerge_core::stack::covers(d, p.at, p.yaw, probe) {
            continue;
        }
        // **The same box `covers` just tested.** This area is half of the `(area, id)` total key that
        // decides which piece a click grabs, so measuring it differently from the hit test above would
        // rank a scaled piece by a size it does not have — and the smallest-footprint rule exists
        // precisely to pick the piece you can see least of.
        let area = emerge_core::descriptor::placed_footprint(d)
            .map_or(f32::INFINITY, |(w, depth)| w * depth);
        let better = match covering {
            None => true,
            Some((_, best_area, best_id)) => (area, p.id.as_str()) < (best_area, best_id),
        };
        if better {
            covering = Some((i, area, p.id.as_str()));
        }
    }
    if let Some((i, _, _)) = covering {
        return Some(i);
    }

    let mut nearest: Option<(usize, f32, &str)> = None;
    for (i, p) in project.map.placements.iter().enumerate() {
        let reach = project
            .library
            .get(&p.descriptor)
            .map(|d| crate::fill::cell_extents(d, p.yaw))
            .map_or(crate::fill::MIN_CELL, |(x, z)| x.max(z));
        let d2 = (p.at.0 - probe.0).powi(2) + (p.at.1 - probe.1).powi(2);
        if d2 > reach * reach {
            continue;
        }
        let better = match nearest {
            None => true,
            Some((_, best_d2, best_id)) => (d2, p.id.as_str()) < (best_d2, best_id),
        };
        if better {
            nearest = Some((i, d2, p.id.as_str()));
        }
    }
    nearest.map(|(i, _, _)| i)
}

/// [`pick_at`], with the probe taken from the cursor — shared by pin, delete and turn, so "the thing
/// I am pointing at" means one thing.
fn nearest_placement(
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &Project,
) -> Option<usize> {
    let (window, camera) = (window?, camera?);
    let (cam, cam_tf) = *camera;
    let hit = cursor_ground(&window, cam, cam_tf)?;
    // Both sides in map space: `at` is authored there and the cursor answers in world metres.
    pick_at(project, project.map.to_map_space((hit.x, hit.z)))
}

/// **The placement the piece-verbs act on** — the locked target while it holds, else the nearest
/// pick. One resolver for every verb, so `H`'s lock cannot mean different pieces to `R` and `]`.
///
/// The lock holds while the cursor stays on the cell it was taken on and the piece still exists;
/// otherwise it lapses silently and the nearest pick resumes — a lock that followed the cursor
/// around the map would turn every later nudge into a surprise.
fn under_cursor_target(
    lock: &mut TargetLock,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &Project,
) -> Option<usize> {
    let (window, camera) = (window?, camera?);
    let (cam, cam_tf) = *camera;
    let hit = cursor_ground(&window, cam, cam_tf)?;
    let at = project.map.to_map_space((hit.x, hit.z));
    if let Some((id, cell)) = &lock.0 {
        if (snap(at.0), snap(at.1)) == *cell {
            if let Some(i) = project.map.placements.iter().position(|p| &p.id == id) {
                return Some(i);
            }
        }
        lock.0 = None;
    }
    pick_at(project, at)
}

/// **`H`: step the target through the stack under the cursor**, bottom to top, wrapping.
///
/// Everyone whose footprint covers the cursor point is in the stack, ordered by resolved height
/// (ids break ties, so the cycle is stable across presses). The status names the pick and the
/// count, because a lock nobody can see is a surprise wearing a feature's clothes.
fn cycle_target(
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &Project,
    state: &mut EditorState,
    lock: &mut TargetLock,
) {
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        state.status = "nothing under the cursor to target".to_owned();
        return;
    };
    let at = project.map.to_map_space((hit.x, hit.z));
    let cell = (snap(at.0), snap(at.1));
    let ys = heights(project).unwrap_or_default();
    let mut stack: Vec<(usize, f32)> = project
        .map
        .placements
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            project
                .library
                .get(&p.descriptor)
                .is_some_and(|d| emerge_core::stack::covers(d, p.at, p.yaw, at))
        })
        .map(|(i, _)| (i, ys.get(i).copied().unwrap_or(0.0)))
        .collect();
    if stack.is_empty() {
        lock.0 = None;
        state.status = "nothing here to target".to_owned();
        return;
    }
    // SORT-OK: editor-side pick, total by unique placement id tiebreak.
    stack.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| project.map.placements[a.0].id.cmp(&project.map.placements[b.0].id))
    });
    // A press over the held cell steps up the stack; a press anywhere fresh starts at the bottom.
    let next = match &lock.0 {
        Some((id, held_cell)) if *held_cell == cell => stack
            .iter()
            .position(|(i, _)| &project.map.placements[*i].id == id)
            .map(|p| (p + 1) % stack.len())
            .unwrap_or(0),
        _ => 0,
    };
    let (index, _) = stack[next];
    let id = project.map.placements[index].id.clone();
    state.status = format!(
        "targeting `{id}` ({} of {} here) — turn / tip / lift act on it, {} steps up, Esc releases",
        next + 1,
        stack.len(),
        keys::binding(Action::CycleTarget).chord,
    );
    lock.0 = Some((id, cell));
}

/// The gold quad under the locked target — and the lock's undertaker: a target that stopped
/// resolving (deleted, undone) releases here rather than pointing at whatever inherits its name.
fn drive_target_marker(
    project: Res<Project>,
    mut lock: ResMut<TargetLock>,
    mut marker: Query<(&mut Transform, &mut Visibility), With<TargetTile>>,
) {
    let footprint = lock.0.as_ref().and_then(|(id, _)| {
        let p = project.map.placements.iter().find(|p| &p.id == id)?;
        let d = project.library.get(&p.descriptor)?;
        let (w, depth) = crate::fill::cell_extents(d, p.yaw);
        Some((p.at, w, depth))
    });
    if lock.0.is_some() && footprint.is_none() {
        lock.0 = None;
    }
    for (mut tf, mut vis) in &mut marker {
        match footprint {
            Some((at, w, depth)) => {
                *vis = Visibility::Visible;
                *tf = Transform::from_xyz(
                    project.map.origin.0 + at.0,
                    project.map.origin.1 + MARKER_LIFT * 2.0,
                    project.map.origin.2 + at.1,
                )
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::new(w.max(0.05), depth.max(0.05), 1.0));
            }
            None => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

/// The target marker quad — [`spawn_removal_tile`]'s recipe in the lock's gold.
fn spawn_target_tile(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        TargetTile,
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: TARGET_TINT,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// Type the reason a cell is pinned.
fn pin_reason_keys(
    mut events: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    if state.pinning.is_none() {
        // **Drain before leaving.** The reader's cursor only advances when `read()` is called, so
        // returning here leaves the keystroke that OPENED this field waiting to be read as its first
        // character next frame. Same invariant as `tiles::cell_keys`; missing it is what made the
        // first authored token come out as `xseam`.
        events.clear();
        return;
    }
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let Some((index, reason)) = state.pinning.take() else {
                    return;
                };
                let reason = reason.trim().to_owned();
                if reason.is_empty() {
                    state.status = "a pin needs a reason; nothing was pinned".to_owned();
                    return;
                }
                // Unpinned is what it was — this field only opens for a piece being pinned.
                state.record(Undo::Pinned {
                    index,
                    owned: false,
                    because: None,
                });
                let pinned_id = project.map.placements.get_mut(index).map(|p| {
                    p.owned = true;
                    p.owned_because = Some(reason.clone());
                    p.id.clone()
                });
                if let Some(id) = pinned_id {
                    project.dirty = true;
                    state.status = format!("pinned {id}: {reason}");
                }
            }
            Key::Escape => {
                state.pinning = None;
                state.status = "nothing pinned".to_owned();
            }
            Key::Backspace => {
                if let Some((_, r)) = state.pinning.as_mut() {
                    r.pop();
                }
            }
            Key::Space => {
                if let Some((_, r)) = state.pinning.as_mut() {
                    r.push(' ');
                }
            }
            Key::Character(c) => {
                if let Some((_, r)) = state.pinning.as_mut() {
                    r.push_str(c);
                }
            }
            _ => {}
        }
    }
}

/// **Continue the author's arrangement.** Learn, solve, replace the unpinned pieces.
/// **Where the rules come from.** Two sources, one solver, and neither substitutes for the other.
///
/// Karth & Smith (FDG 2019) name these as the algorithm's own two modes: rules inferred from an
/// example, and rules stated up front. `Learned` cannot answer on an empty map — there are no
/// observed pairs — and `Declared` cannot answer for a library nobody has tokened. Each says so
/// rather than quietly deferring to the other, because a generator that silently changed which
/// grammar it used would produce output the author cannot account for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    /// Learned from what is already placed. Continues the author's own corner of the room.
    Learned,
    /// Read off the kit's declared edge tokens. Works on an empty map.
    Declared,
}

fn generate(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
) {
    generate_from(commands, assets, project, state, placed, Source::Learned);
}

fn generate_from(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
    source: Source,
) {
    // One metre: the tile the kits are authored on, and coarse enough that a 32 m map is a grid the
    // solver finishes rather than 4,096 cells of half-metre noise.

    let built = match source {
        Source::Learned => emerge_core::grammar::learn(&project.map, CELL),
        Source::Declared => {
            emerge_core::grammar::declared(&project.library, project.policy.divisions, CELL)
        }
    };
    let grammar = match built {
        Ok(g) => g,
        Err(e) => {
            state.status = e;
            return;
        }
    };
    state.seed = state.seed.wrapping_add(1);
    let mut n = state.next_id;

    // **The solver has to see the stamps, or an owned group protects nothing.**
    //
    // `solve` builds its pinned set from `map.placements.iter().filter(|p| p.owned)`, and stamped
    // rows are deliberately never in `placements` — so a group an author marked owned, whose whole
    // documented meaning is *"a generator routes around the whole group"*, was invisible and the
    // collapser filled straight through it. Expanding into a scratch map is what makes the pin real:
    // `expand` already propagates `Stamped::owned` to every row it produces, so the cells come back
    // as the unary constraints they were always supposed to be.
    //
    // A scratch copy rather than the real map, because the expansion must not become authored rows —
    // that invariant is what keeps the reference model a reference.
    let mut scratch = project.map.clone();
    if !scratch.stamps.is_empty() {
        match emerge_core::composition::expand(
            &project.map,
            &project.map.stamps,
            &project.compositions.compositions,
            &project.library,
        ) {
            Ok(e) => scratch.placements.extend(e.placements),
            Err(e) => {
                state.status = format!("cannot generate around the stamps: {e}");
                return;
            }
        }
    }
    let solved = match emerge_core::grammar::solve(&scratch, &grammar, CELL, state.seed, || {
        n += 1;
        format!("gen@{n}")
    }) {
        Ok(s) => s,
        Err(e) => {
            state.status = e;
            return;
        }
    };
    state.next_id = n;

    // Everything unpinned is the sketch; the solve is the drawing. Despawn it and rebuild —
    // **keeping what was removed, with its indices**, because this is an edit like any other and the
    // history must be able to put it back. This used a bare `retain()` and recorded only the solver
    // rows, which was two defects in one line: the author's sketch was unrecoverable (Cmd+Z drained
    // the solver output and stopped, with a success message), and every index-based entry already on
    // the stack — Moved, Turned, Pinned, RemoveAt — was left pointing at rows that had shifted, so a
    // second Cmd+Z rewrote whatever now sat at those indices with the dead sketch's data.
    let removed: Vec<(usize, Box<Placed>)> = project
        .map
        .placements
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.owned)
        .map(|(i, p)| (i, Box::new(p.clone())))
        .collect();
    for (entity, marker) in placed {
        if removed.iter().any(|(_, p)| p.id == marker.0) {
            commands.entity(entity).despawn();
        }
    }
    // Descending, so removing an earlier row cannot shift a later one out from under us.
    for (i, _) in removed.iter().rev() {
        project.map.placements.remove(*i);
    }

    // **Into the map first, drawn second.** The solver lays pieces on the floor grid; how high each
    // one ends up is a question about the finished map, so the map has to be finished before it is
    // asked.
    let count = solved.placements.len();
    let first = project.map.placements.len();
    project.map.placements.extend(solved.placements);
    spawn_range(commands, assets, project, state, first);
    project.dirty = true;
    // One act, one entry: undoing a generate first strips the solver rows (the `Added`), then puts
    // the sketch back at its own indices (the `RemovedMany`) — [`Undo::Group`] applies in order and
    // inverts by reversing.
    state.record(Undo::Group {
        ops: vec![
            Undo::Added { count },
            Undo::RemovedMany { items: removed },
        ],
    });
    state.status = format!(
        "continued the layout: {count} placed around {} pinned cell(s), from {} prototype(s)",
        solved.owned_cells,
        grammar.len() - 1
    );
}

/// The `F` handler, split out so `keys` stays readable.
fn flood_from_cursor(
    commands: &mut Commands,
    assets: &AssetServer,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &mut Project,
    state: &mut EditorState,
) {
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        return;
    };
    // Said rather than silent: `F` with nothing armed used to be indistinguishable from `F` not being
    // bound, and now that the palette can be cleared it is a state an author can actually reach.
    let Some(brush) = state
        .brush
        .and_then(|ix| project.library.descriptors.get(ix))
        .cloned()
    else {
        state.status = "nothing armed to fill with — pick a piece from the palette".to_owned();
        return;
    };

    let start_id = state.next_id;
    let mut n = start_id;
    let short = short_id(&brush.id).to_owned();
    let filled = match crate::fill::flood(
        &project.map,
        &brush,
        project.map.to_map_space((hit.x, hit.z)),
        state.brush_yaw,
        || {
            n += 1;
            format!("{short}@{n}")
        },
    ) {
        Ok(f) => f,
        // A refusal is the answer, not a failure: "outside the map" and "something is already here"
        // are both things the author needs told rather than worked around.
        Err(e) => {
            state.status = e;
            return;
        }
    };
    state.next_id = n;

    let count = filled.placements.len();
    let first = project.map.placements.len();
    project.map.placements.extend(filled.placements);
    spawn_range(commands, assets, project, state, first);
    project.dirty = true;
    // **One undo entry for the whole fill.** A fill is one act to the person who performed it, and an
    // undo stack that made them press Ctrl+Z 1,408 times would be a stack that models the code rather
    // than the work.
    if count > 0 {
        state.record(Undo::Added { count });
    }
    // A cap that stopped the fill has to say so — a truncated fill looks exactly like a finished one.
    state.status = if filled.truncated {
        format!(
            "filled {count} and STOPPED at the {} cell cap — fill again to continue",
            crate::fill::MAX_CELLS
        )
    } else {
        format!("filled {count} with {}", brush.id)
    };
}

// ── the ghost ────────────────────────────────────────────────────────────────────────────────────

/// How much of its opacity a ghost keeps.
const GHOST_ALPHA: f32 = 0.45;

#[allow(clippy::too_many_arguments)]
fn drive_ghost(
    mut commands: Commands,
    assets: Res<AssetServer>,
    project: Res<Project>,
    // Mutable for one reason: the status line is where "there is no worktop here" belongs, and the
    // moment the author needs it is while the cursor is over the spot, not after the click.
    mut state: ResMut<EditorState>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    ghosts: Query<(Entity, &GhostOf), With<Ghost>>,
    mut transforms: Query<&mut Transform, With<Ghost>>,
    held: Res<MoveDrag>,
    // The ghost must snap exactly when the click will, or the preview is a lie about where the
    // piece lands — which is the one thing this whole system exists to prevent.
    keyboard: Res<ButtonInput<KeyCode>>,
    anchor: Res<FineAnchor>,
) {
    let clear = |commands: &mut Commands| {
        for (e, _) in &ghosts {
            commands.entity(e).despawn();
        }
    };

    let (Some(window), Some(camera)) = (window, camera) else {
        clear(&mut commands);
        return;
    };
    // No ghost while the cursor is over the panel: the piece is not going there, and a preview
    // hovering under the palette is a promise about a click that will never reach the world. Nor
    // while removing — two previews for two different outcomes under one cursor is a question, not
    // an answer.
    if hovered_ui.iter().any(|h| h.0) || state.tool == Tool::Remove {
        clear(&mut commands);
        return;
    }
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        clear(&mut commands);
        return;
    };

    // **What the next click will actually do**, which is not always the armed brush.
    //
    // While a piece is in hand the click puts *that* piece down, so previewing the brush would be a
    // promise about something that is not going to happen — and it is the piece the author is
    // carrying whose landing spot they need to see. With move armed and nothing picked up there is no
    // subject at all, and the honest preview of "click to pick something up" is no ghost.
    //
    // It keeps the carried piece's own yaw, not `brush_yaw`: a move changes where a thing is, and
    // silently re-aiming it would be a second edit the author did not ask for.
    let subject = match state.tool {
        Tool::Remove => None,
        // The clone tool draws its own marker — the set's bounds riding the cursor — and a brush
        // ghost beside it would be a second preview for a click that stamps, not places.
        Tool::Clone => None,
        Tool::Move => held
            .held
            .as_ref()
            .and_then(|id| project.map.placements.iter().find(|p| &p.id == id))
            .and_then(|p| {
                project
                    .library
                    .descriptors
                    .iter()
                    .position(|d| d.id == p.descriptor)
                    .map(|ix| (ix, p.yaw))
            }),
        // Nothing armed is a real answer: no ghost, because no click is going to place anything.
        Tool::Place => state.brush.map(|ix| (ix, state.brush_yaw)),
    };
    let Some((brush_ix, want_yaw)) = subject else {
        clear(&mut commands);
        return;
    };
    let Some(d) = project.library.descriptors.get(brush_ix) else {
        clear(&mut commands);
        return;
    };

    let at = map_at(&project, hit, keys::mod_held(&keyboard), &anchor);
    let yaw = emerge_bevy::draw_yaw(d, want_yaw);

    // **The ghost asks the question the drop will ask** — against a map the carried group is NOT in.
    //
    // While a piece is in hand its rows are only hidden, still present, and `move_placement`'s own
    // doc names the trap that leaves open: `host_under` does not know to skip the piece it is
    // seating, so a carried table would find its own (hidden) mug under the cursor and the ghost
    // would preview a landing the drop then computes differently. The drop probes with the group
    // removed; so must the preview, or it is a promise about something that is not going to happen —
    // the one thing this editor's previews are held to.
    let probe_map = match (state.tool, held.held.as_ref()) {
        (Tool::Move, Some(id)) => {
            let mut reduced = project.map.clone();
            if let Some(ix) = reduced.placements.iter().position(|p| &p.id == id) {
                let mut group = emerge_core::stack::group_of(&reduced, ix);
                group.sort_unstable();
                for i in group.iter().rev() {
                    reduced.placements.remove(*i);
                }
            }
            Some(reduced)
        }
        _ => None,
    };
    let probe_map = probe_map.as_ref().unwrap_or(&project.map);

    // **The ghost stands where the piece would.** A lamp dragged over a table rises onto it, so the
    // author sees the answer before committing to it rather than placing and then wondering. When
    // there is no surface under a piece that needs one there is nothing truthful to draw — showing it
    // on the floor would be a preview of something that will not happen.
    let ys = match emerge_core::stack::resolve_y(probe_map, &project.library) {
        Ok(ys) => ys,
        Err(_) => {
            clear(&mut commands);
            return;
        }
    };
    let (y, _) = match emerge_core::stack::placement_at(probe_map, &project.library, &ys, d, at) {
        Ok(found) => found,
        // **The reason, while the cursor is still there.** `docs/ui.md` §1.4: an unmet condition is an
        // instruction. A ghost that simply vanishes over bare floor reads as the editor being broken;
        // the sentence says which surface the piece wants and leaves the author holding the answer.
        Err(e) => {
            clear(&mut commands);
            // **Only when it changes.** This runs every frame the cursor sits over bare floor with a
            // surface piece armed, and `ResMut` marks the resource changed on every mutable deref —
            // `rebuild_palette` watches `resource_changed::<EditorState>`, so an unconditional write
            // here would tear down and rebuild the whole palette at frame rate.
            if state.hint != e {
                state.hint = e;
            }
            return;
        }
    };
    // The piece can go here, so there is nothing to warn about.
    if !state.hint.is_empty() {
        state.hint.clear();
    }

    let existing = ghosts.iter().find(|(_, g)| g.0 == brush_ix).map(|(e, _)| e);
    for (e, g) in &ghosts {
        if g.0 != brush_ix {
            commands.entity(e).despawn();
        }
    }

    match existing {
        Some(e) => {
            if let Ok(mut tf) = transforms.get_mut(e) {
                tf.translation = emerge_bevy::origin_of(at, project.map.origin, y);
                tf.rotation = Quat::from_rotation_y(yaw.to_radians());
            }
        }
        None => {
            if let Some(e) = spawn_piece(
                &mut commands,
                &assets,
                d,
                at,
                want_yaw,
                (0, 0),
                project.map.origin,
                y,
            ) {
                commands.entity(e).insert((Ghost, GhostOf(brush_ix)));
            }
        }
    }
}

/// Fade the ghost once its GLB has instantiated materials.
///
/// The materials come from the asset, so they are the *shared* handles every real instance uses —
/// writing alpha into them would turn every crate in the map see-through. Each descendant gets its
/// own clone, marked so the walk settles to a no-op.
fn fade_ghost(
    mut commands: Commands,
    ghosts: Query<Entity, With<Ghost>>,
    children: Query<&Children>,
    painted: Query<&MeshMaterial3d<StandardMaterial>, Without<Ghosted>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for root in &ghosts {
        let mut queue = vec![root];
        while let Some(e) = queue.pop() {
            if let Ok(handle) = painted.get(e) {
                if let Some(base) = materials.get(&handle.0) {
                    let mut faded = base.clone();
                    faded.alpha_mode = AlphaMode::Blend;
                    let a = faded.base_color.alpha() * GHOST_ALPHA;
                    faded.base_color.set_alpha(a);
                    let handle = materials.add(faded);
                    commands.entity(e).insert((
                        MeshMaterial3d(handle),
                        Ghosted,
                        bevy::light::NotShadowCaster,
                    ));
                }
            }
            if let Ok(kids) = children.get(e) {
                queue.extend(kids.iter());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emerge_core::descriptor::{Descriptor, Extent};
    use emerge_core::library::{Library, LIBRARY_VERSION};
    use emerge_core::map::{Map, Placed};

    fn piece(id: &str, w: f32, d: f32) -> Descriptor {
        Descriptor {
            id: id.to_owned(),
            mesh: Some(format!("{id}.glb")),
            extent: Extent {
                footprint: Some((w, d)),
                height: Some(1.0),
            },
            ..Descriptor::default()
        }
    }

    fn at(id: &str, descriptor: &str, at: (f32, f32)) -> Placed {
        Placed {
            id: id.to_owned(),
            descriptor: descriptor.to_owned(),
            at,
            ..Placed::default()
        }
    }

    pub(super) fn project(descriptors: Vec<Descriptor>, placements: Vec<Placed>) -> Project {
        Project {
            // A test project stamps nothing; empty is the same state as a file with none in it.
            compositions: emerge_core::composition::Compositions::default(),
            root: std::path::PathBuf::from("."),
            emerge_dir: std::path::PathBuf::from("assets/emerge"),
            library_path: std::path::PathBuf::from("assets/emerge/library.ron"),
            vocab: emerge_core::vocab::Vocabularies::default(),
            // No policy, so the measurements and the layered library are the same set — which is
            // what these tests are about. `write_library`'s own tests are the ones that pull them
            // apart.
            measured: Library {
                version: LIBRARY_VERSION,
                note: None,
                descriptors: descriptors.clone(),
            },
            library: Library {
                version: LIBRARY_VERSION,
                note: None,
                descriptors,
            },
            policy: emerge_core::policy::Policy::default(),
            masks: Vec::new(),
            map: Map {
                name: "test_map".into(),
                placements,
                ..Map::default()
            },
            map_path: std::path::PathBuf::from("test_map.map.ron"),
            dirty: false,
            triangles: Vec::new(),
        }
    }

    /// **The bug this rule was rewritten for.** A lamp on a table shares the table's `at`, so
    /// "nearest centre" could hand back the table when the cursor was squarely on the lamp — and did,
    /// while authoring `break_room.map.ron`.
    #[test]
    fn pointing_at_a_lamp_on_a_table_picks_the_lamp() {
        let p = project(
            vec![piece("table", 1.6, 0.8), piece("lamp", 0.3, 0.3)],
            vec![at("t1", "table", (0.0, 0.0)), at("l1", "lamp", (0.0, 0.0))],
        );
        assert_eq!(pick_at(&p, (0.0, 0.0)), Some(1), "the smaller thing wins");
        // Off the lamp but still on the table, the table is what you are pointing at.
        assert_eq!(pick_at(&p, (0.6, 0.0)), Some(0));
    }

    /// **Reach belongs to the target, not the brush.** Pointing at bare floor beside a piece still
    /// grabs it; how far that reaches must not depend on what happens to be armed.
    #[test]
    fn a_near_miss_still_grabs_the_piece_beside_it() {
        let p = project(
            vec![piece("crate", 1.0, 1.0)],
            vec![at("c1", "crate", (0.0, 0.0))],
        );
        assert_eq!(pick_at(&p, (0.0, 0.0)), Some(0), "dead centre");
        assert_eq!(pick_at(&p, (0.6, 0.0)), Some(0), "just outside, still its cell");
        assert_eq!(pick_at(&p, (9.0, 9.0)), None, "across the room is nothing");
    }

    /// Two identical pieces at the same distance resolve the same way every time — by id, which is a
    /// total order that does not depend on which was authored first.
    #[test]
    fn a_tie_resolves_by_id_rather_than_by_authoring_order() {
        let forwards = project(
            vec![piece("crate", 1.0, 1.0)],
            vec![at("b", "crate", (2.0, 0.0)), at("a", "crate", (-2.0, 0.0))],
        );
        let backwards = project(
            vec![piece("crate", 1.0, 1.0)],
            vec![at("a", "crate", (-2.0, 0.0)), at("b", "crate", (2.0, 0.0))],
        );
        let id = |p: &Project, i: Option<usize>| {
            i.and_then(|i| p.map.placements.get(i)).map(|q| q.id.clone())
        };
        // Equidistant from the origin, so only the id can break it — and it breaks the same way
        // whichever order the file lists them in.
        assert_eq!(
            id(&forwards, pick_at(&forwards, (0.0, 0.0))),
            id(&backwards, pick_at(&backwards, (0.0, 0.0)))
        );
    }

    /// A piece with no measured footprint covers nothing, so it cannot swallow every click.
    #[test]
    fn an_unmeasured_piece_does_not_cover_the_map() {
        let mut vague = piece("mystery", 1.0, 1.0);
        vague.extent.footprint = None;
        let p = project(vec![vague], vec![at("m1", "mystery", (0.0, 0.0))]);
        // Not covering, and its fallback reach is the minimum cell rather than infinity.
        assert_eq!(pick_at(&p, (9.0, 9.0)), None);
    }
}

/// Grid snapping, and the modifier that drops it.
#[cfg(test)]
mod snap_tests {
    use super::*;
    use emerge_core::map::Map;

    fn project_at(origin: (f32, f32, f32)) -> Project {
        let mut p = tests::project(Vec::new(), Vec::new());
        p.map = Map {
            name: "t".into(),
            origin,
            ..Map::default()
        };
        p
    }

    /// The default is unchanged: a click lands on the authoring grid, wherever inside a cell it fell.
    #[test]
    fn a_click_snaps_to_the_grid_by_default() {
        let p = project_at((0.0, 0.0, 0.0));
        assert_eq!(map_at(&p, Vec3::new(0.24, 0.0, 0.76), false, &FineAnchor::default()), (0.0, 1.0));
        assert_eq!(map_at(&p, Vec3::new(1.26, 0.0, -0.24), false, &FineAnchor::default()), (1.5, 0.0));
    }

    /// **Held, it does not.** The point comes through exactly as the cursor gave it.
    #[test]
    fn the_modifier_places_where_the_cursor_actually_is() {
        let p = project_at((0.0, 0.0, 0.0));
        let hit = Vec3::new(0.24, 0.0, 0.76);
        let free = map_at(&p, hit, true, &FineAnchor::default());
        assert!((free.0 - 0.24).abs() < 1e-6 && (free.1 - 0.76).abs() < 1e-6, "{free:?}");
        assert_ne!(free, map_at(&p, hit, false, &FineAnchor::default()), "free placement must differ from snapped");
    }

    /// Both paths still convert world space to map space, so a map that is not at the origin is not
    /// off by its own offset — the defect the conversion was introduced for.
    #[test]
    fn free_placement_still_converts_into_map_space() {
        let p = project_at((10.0, 0.0, -4.0));
        let hit = Vec3::new(12.3, 0.0, -1.7);
        assert_eq!(map_at(&p, hit, true, &FineAnchor::default()), p.map.to_map_space((hit.x, hit.z)));
        // And the snapped path lands on the grid in MAP space, not in world space.
        let (sx, sz) = map_at(&p, hit, false, &FineAnchor::default());
        assert!((sx / SNAP).fract().abs() < 1e-4 && (sz / SNAP).fract().abs() < 1e-4, "{sx}, {sz}");
    }
}
