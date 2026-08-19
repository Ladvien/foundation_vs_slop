//! **Tile configuration** — bringing meshes in, and saying what they are.
//!
//! The editor's second tab. `emerge_core::import` does the measuring; this is where an author reads
//! it, gives a mesh an id, decides which layer it goes on and what it is tagged as, and accepts it
//! into the library. Separate from the map tab because they are different jobs with different
//! controls, and one panel trying to hold both would be a panel that does neither well.
//!
//! # The scan is lazy and says how big it was
//!
//! A project ships far more meshes than its library defines, so the candidate list is long and the
//! scan is a second of file reading. Doing it at launch would make every session pay for a mode most
//! of them never open, so it happens on the first Tab and the panel reports what it found — a long
//! list with no count is a list nobody trusts they have seen the end of.
//!
//! **No number appears in this note on purpose.** Three module notes in this crate each stated the
//! size of the same library, and all three were wrong by the time anyone read them; the panel had
//! always computed its own. Counts belong to `emerge_core::census`, which derives them from the
//! catalog, and to nothing else.
//!
//! # Findings are shown with their fix
//!
//! A warning that does not say what to do about it is a warning that gets read once. Every
//! [`emerge_core::import::Finding`] that has an obvious remedy carries it, and the panel shows both.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton};
use emerge_core::descriptor::{Descriptor, mount_label, mount_options};
use emerge_core::import::{self, Candidate, Severity};

use crate::chrome::{
    ACCENT, CHIP_PAD, DANGER, DIM, HEADER_BG, LABEL, MIN_FIELD_H, MUTED, PANEL_BG,
    ROW_BG, ROW_HOVER, ROW_SELECTED, TEXT,
};
use crate::keys::{self, Action};
use crate::project::Project;

/// Which job the editor is doing.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Place pieces and build the level.
    #[default]
    Map,
    /// Bring meshes in and say what they are.
    Meshes,
    /// Assemble a cell-sized tile out of meshes, on the tile's own grid.
    Tiles,
    /// Preview and tune a rig's clips.
    Anim,
    /// Build reusable groups, and see what they present and what has gone stale under them.
    Compose,
}

/// **Which door the editor was entered by.**
///
/// A door is one *entity* you can work on, and it shows the panels that entity needs — no more.
/// Reported at the keyboard, 2026-08-16: *"when I select a kit and I press enter, I'm still getting
/// [map], meshes, tiles, compose, anim. we really need this to reflect just what's needed to put
/// kits together."*
///
/// # Why two and not five
///
/// The first cut was one door per panel, and building it produced the measurement that ruled that
/// out: **four shipped guides cross doors mid-flow**, `room_from_nothing.json` seven times. Meshes,
/// Tiles and Compose are not three jobs — they are one job at three levels of assembly
/// (`docs/research/2026-08-08-kitbashing-guidance.md`: *"parts -> sub-assemblies -> assemblies"*),
/// and an author moves between them constantly while making one kit. Splitting them put a process
/// boundary inside a single act.
///
/// So the boundary goes where the *entity* changes rather than where the panel does: **making
/// reusable content** on one side, **using it to build a level** on the other. Chosen at the
/// keyboard, 2026-08-16: *"Kit and Map, two doors."*
///
/// [`Door::Rigs`] is a third because it crosses with neither — no guide enters it, and a rig is a
/// character asset rather than level content. It was already its own door by an earlier decision and
/// nothing in the crossing measurement argued against it.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Door {
    /// Make reusable content: meshes → tiles → compositions.
    Kit,
    /// Build the level out of it.
    #[default]
    Map,
    /// The rig bench.
    Rigs,
}

impl Door {
    /// The doors, in the order the menu lists them.
    pub const ALL: [Door; 3] = [Door::Kit, Door::Map, Door::Rigs];

    /// **The panels this door shows**, in hierarchy order.
    ///
    /// The Kit door shows three, and that is the point of the two-door shape: a mesh is a part, a
    /// tile is a sub-assembly, a composition is an assembly, and an author walks up and down that
    /// ladder while making one kit. What it does *not* show is the Map.
    pub fn tabs(self) -> &'static [Mode] {
        match self {
            Door::Kit => &[Mode::Meshes, Mode::Tiles, Mode::Compose],
            Door::Map => &[Mode::Map],
            Door::Rigs => &[Mode::Anim],
        }
    }

    /// Where this door opens. The first of [`Self::tabs`], which is never empty.
    pub fn opens_on(self) -> Mode {
        self.tabs().first().copied().unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Door::Kit => "KIT",
            Door::Map => "MAP",
            Door::Rigs => "RIGS",
        }
    }

    /// The `--door` spelling. Lowercase of [`Self::label`], so the flag and the title cannot drift.
    pub fn from_flag(flag: &str) -> Option<Door> {
        Door::ALL
            .into_iter()
            .find(|d| d.label().eq_ignore_ascii_case(flag))
    }

    /// **Run condition: is the Maps door open?**
    ///
    /// A condition rather than a check in `Plugin::build`, and that distinction cost a crash. The
    /// door used to be known before the app ran — it was a process — so `EditorPlugin` could read it
    /// at build time and simply not register the map-only half. With both screens in one application
    /// the door is chosen at runtime, so at build time there is none: the check read
    /// `unwrap_or_default()`, got `Map`, and registered `spawn_existing` on every door — which then
    /// panicked on the Kit door for a `Res<OpenMap>` that does not exist there.
    ///
    /// `Option<Res<Door>>`, because **every run condition is evaluated** and this one is asked in the
    /// menu too, where there is no door at all.
    pub fn map_door_is_open(door: Option<Res<Door>>) -> bool {
        door.is_some_and(|d| *d == Door::Map)
    }

    /// Does this door show that panel? The one place `Mode` and `Door` are compared.
    pub fn shows(self, mode: Mode) -> bool {
        self.tabs().contains(&mode)
    }

    /// **The door that shows this panel.** Every `Mode` belongs to exactly one, by construction of
    /// [`Self::tabs`] — so a caller can name the panel it wants and let the door follow, rather than
    /// stating both and risking a pair that disagree.
    pub fn showing(mode: Mode) -> Door {
        Door::ALL
            .into_iter()
            .find(|d| d.shows(mode))
            .unwrap_or_default()
    }
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Map => "MAP",
            Mode::Meshes => "MESHES",
            Mode::Tiles => "TILES",
            Mode::Anim => "ANIM",
            Mode::Compose => "COMPOSE",
        }
    }

    /// This tab as a key context. The census speaks in [`crate::keys::Context`] and the app speaks in
    /// `Mode`; this is the single place the two are the same thing, so a fourth tab is one arm here
    /// rather than a search for every `*mode ==` in the crate.
    pub fn context(self) -> crate::keys::Context {
        match self {
            Mode::Map => crate::keys::Context::Map,
            Mode::Meshes => crate::keys::Context::Meshes,
            Mode::Tiles => crate::keys::Context::Tiles,
            Mode::Anim => crate::keys::Context::Anim,
            Mode::Compose => crate::keys::Context::Compose,
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
    /// first version had one field, so "N mesh(es) not in the library" was replaced by "layer: on
    /// support" the moment anyone did anything, and the one number that says whether you have seen
    /// the whole list was gone for the rest of the session.
    pub summary: String,
    /// The last thing that happened, and the last thing that went wrong — see
    /// [`crate::chrome::Status`]. This tab had **no colour rule at all**: the action line was drawn
    /// in one fixed colour, so `NOT WRITTEN:` and `added \`crate\`` were byte-identical.
    pub status: crate::chrome::Status,
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
    /// **How many times the labeler has turned each mesh to right it**, keyed by mesh path.
    ///
    /// A righting re-photographs the piece and asks again, so a model that keeps saying "not
    /// upright" turns it for ever — four quarter turns is where it started, which makes the loop
    /// silent as well as endless. [`MAX_RIGHTINGS`] is the ceiling; past it the proposal is dropped
    /// with a sentence rather than retried.
    ///
    /// Session bookkeeping rather than descriptor state, so it is deliberately absent from
    /// [`Snapshot`]: undoing a turn must not hand the loop its budget back.
    pub righted: std::collections::BTreeMap<String, u8>,
    /// **The pack heading the arrows are standing on**, when they are on one rather than a mesh.
    ///
    /// The walk used to step mesh rows only, so a collapsed pack was visible and unreachable: at the
    /// top of the first open pack, `Up` had nowhere to go while 33 headings sat above it on screen,
    /// and the only way to open one was the mouse. Reported at the keyboard three times, last on
    /// 2026-08-16 — *"it goes up until the one right above it is a collapsed mesh folder, and then it
    /// just does nothing."*
    ///
    /// `None` means the highlight is on [`Self::selected`], a mesh. The two are one cursor in two
    /// states rather than two cursors, so nothing has to decide which is "really" selected.
    pub focused_pack: Option<String>,
    /// **Whether the `EXCLUDED` group at the bottom of the candidate list is open.**
    ///
    /// Its own flag rather than a reserved key in [`Self::folded_packs`], because a sentinel string
    /// there is one real pack away from a collision — and a pack called `EXCLUDED` is not a silly
    /// name for a folder of things somebody excluded. Closed by default: the group exists to get
    /// these out of the way.
    pub excluded_open: bool,
    /// **This tab's history**, most recent last. See [`Snapshot`].
    pub undo: Vec<Snapshot>,
    /// What has been undone here and can be put back. Cleared by any new edit on this tab.
    pub redo: Vec<Snapshot>,
}

/// **Everything one Tiles-tab edit can change**, taken before it changes.
///
/// The edits here are heterogeneous — a lattice cell, an id, a float, a whole mount, an entry added or
/// removed — and ten inverse operations would be ten things to keep in step with the ten forward ones.
/// A snapshot is one mechanism for all of them, and `commit_measured` already exists as the single
/// writer to restore through, so undoing re-runs exactly the validation an edit does.
///
/// Both halves, because an edit lands in one or the other: a **library** entry is written to
/// `library.ron`, while a **candidate** is `Persist::InMemory` until Accept. Snapshotting only the
/// library would leave every pre-Accept edit — id, mount, size, height, lattice — outside the history.
#[derive(Clone)]
pub struct Snapshot {
    pub measured: emerge_core::library::Library,
    pub candidates: Vec<Candidate>,
}

/// How many steps this tab remembers.
///
/// Bounded because a snapshot is a whole library plus the candidate list, and a scan can turn up
/// hundreds of candidates — an unbounded stack would grow without anything ever freeing it. Deep
/// enough that it is not a limit anyone reaches by working normally.
pub const TILE_HISTORY: usize = 64;

impl ImportState {
    /// The state as it stands, for the history.
    pub fn snapshot(&self, project: &Project) -> Snapshot {
        Snapshot {
            measured: project.measured.clone(),
            candidates: self.candidates.clone(),
        }
    }

    /// **Record an edit on this tab**, and drop anything waiting to be redone.
    ///
    /// One place, so no edit site can forget the second half — the same rule `EditorState::record`
    /// follows on the map side, and for the same reason.
    pub fn record(&mut self, before: Snapshot) {
        if self.undo.len() >= TILE_HISTORY {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }
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
pub const STAGE: Vec3 = crate::stages::TILE;

/// Where the map camera was, so leaving the tab puts it back.
///
/// Without this, coming back from a tile left the author at the origin looking at whatever happened
/// to be there — their pan and zoom silently discarded.
#[derive(Resource, Default)]
struct MapView(Option<crate::view::Rig>);

/// Move the camera to the stage on entering the tab, and back on leaving.
fn stage_camera(
    mode: Res<Mode>,
    state: Res<ImportState>,
    project: Res<Project>,
    preset: Option<Res<crate::anim_stage::BenchCamera>>,
    carousel: Res<crate::compose::StagedCarousel>,
    mut rig: ResMut<crate::view::Rig>,
    mut saved: ResMut<MapView>,
    mut staged: ResMut<StagedLift>,
) {
    // **Re-centre when the piece moves up, as well as when the tab changes.** Raising a sconce to
    // 2.4 m would otherwise push it out of a view framed on the floor, and the author's own edit would
    // look like the preview breaking.
    //
    // On a *change*, never every frame: `rig.focus` is also written by `view::drive`, so holding it
    // here would take panning away on this tab. A lift is a discrete event, exactly like a tab change.
    let want_lift = state.placed(&project).map(stage_lift).unwrap_or(0.0);
    let lift_moved = *mode == Mode::Meshes && (staged.0 - want_lift).abs() > 1e-4;
    // A preset cycle is a discrete event on the Anim tab, exactly like a lift on the Tiles tab.
    let preset_moved = *mode == Mode::Anim && preset.as_ref().is_some_and(|p| p.is_changed());
    // And a re-laid strip is one on the Compose tab. `StagedCarousel` is written only when the strip
    // actually changes — a carousel step, a group added, an envelope resized — so this is the same
    // discrete-event shape and not a per-frame write that would take panning away.
    let strip_moved = *mode == Mode::Compose && carousel.is_changed();
    if !mode.is_changed() && !lift_moved && !preset_moved && !strip_moved {
        return;
    }
    if lift_moved {
        staged.0 = want_lift;
    }
    match *mode {
        // Both tabs look at the same far stage — one shows a mesh on it, the other a tile.
        Mode::Meshes | Mode::Tiles => {
            if saved.0.is_none() {
                saved.0 = Some(crate::view::Rig {
                    focus: rig.focus,
                    height: rig.height,
                    yaw: rig.yaw,
                    goal_yaw: rig.goal_yaw,
                    elevation: rig.elevation,
                });
            }
            rig.focus = STAGE + Vec3::new(0.0, want_lift, 0.0);
            // Close enough that one grid cell fills the view — the tab is about a single tile.
            rig.height = TILE_VIEW_HEIGHT;
            // Canonical iso framing — the author may arrive from a ground-level anim preset.
            rig.elevation = crate::view::ISO_ELEVATION;
        }
        // The anim bench's stage. An arm of THIS system rather than a sibling, because two systems
        // saving/restoring one `MapView` on the same `mode.is_changed()` edge would race over it.
        Mode::Anim => {
            if saved.0.is_none() {
                saved.0 = Some(crate::view::Rig {
                    focus: rig.focus,
                    height: rig.height,
                    yaw: rig.yaw,
                    goal_yaw: rig.goal_yaw,
                    elevation: rig.elevation,
                });
            }
            let (focus_y, height, elevation, yaw_snap) =
                preset.as_deref().copied().unwrap_or_default().0.framing();
            rig.focus = crate::anim_stage::BENCH_STAGE + Vec3::Y * focus_y;
            rig.height = height;
            rig.elevation = elevation;
            if let Some(yaw) = yaw_snap {
                rig.yaw = yaw;
                rig.goal_yaw = yaw;
            }
        }
        // **Compose has its own stage, and that reverses an earlier decision on the record.**
        //
        // It used to share the Map's camera, argued this way: *"The tab is a list and a detail pane
        // over groups that land in this map, and arming one here is followed immediately by stamping
        // it there — a camera that jumped to a stage and back would make that one gesture look like
        // two places."*
        //
        // That premise stopped being true when the tab gained seating verbs. It is no longer a list
        // and a detail pane; it moves geometry, and a surface that edits what it cannot show is worse
        // than a camera jump. The worry it names is also already answered elsewhere — the Tiles tab
        // has jumped to a stage and restored on the way back for as long as it has existed, and
        // arming still switches nothing by itself.
        //
        // **The focal group is pinned to the stage origin**, so stepping the carousel slides the
        // strip past a fixed centre rather than moving the thing being edited. The camera frames the
        // whole strip, miniatures included, and re-frames only when the strip is re-laid.
        Mode::Compose => {
            if saved.0.is_none() {
                saved.0 = Some(crate::view::Rig {
                    focus: rig.focus,
                    height: rig.height,
                    yaw: rig.yaw,
                    goal_yaw: rig.goal_yaw,
                    elevation: rig.elevation,
                });
            }
            // **Focused halfway up, not on the floor.** The groups rise from the ground plane, so
            // aiming at it put a 2.4 m tile in the top half of the frame with the bottom empty —
            // seen in a captured frame, and the same correction the Tiles arm makes with `want_lift`.
            rig.focus = crate::compose::COMPOSE_STAGE + Vec3::Y * carousel.0.tallest * 0.5;
            rig.height = crate::compose::framing_height(carousel.0.extent, carousel.0.tallest)
                .min(crate::view::MAX_ZOOM);
            rig.elevation = crate::view::ISO_ELEVATION;
        }
        Mode::Map => {
            if let Some(was) = saved.0.take() {
                rig.focus = was.focus;
                rig.height = was.height;
                rig.yaw = was.yaw;
                rig.goal_yaw = was.goal_yaw;
                rig.elevation = was.elevation;
            }
        }
    }
}

/// Orthographic viewport height on the stage, metres. About three grid cells, so a 1-cell piece has
/// room around it and a 3-cell one still fits.
///
/// `pub(crate)` because it is also the Compose sheet's floor — see `compose::framing_height`. One
/// tile should read the same size whichever tab is looking at it.
pub(crate) const TILE_VIEW_HEIGHT: f32 = 4.0;

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

/// A verb that acts on the selected cell.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellVerb {
    /// Occupancy — the common one, so it is first, is a toggle rather than a field, and is what a
    /// header does before any chip has been touched.
    #[default]
    Solid,
    /// Type an edge token.
    Edge,
    /// Forget the cell entirely.
    Clear,
}

impl CellVerb {
    fn label(self) -> &'static str {
        match self {
            CellVerb::Solid => "solid",
            CellVerb::Edge => "edge",
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
    /// The token being typed, if a field is open. One facet takes tokens, so there is nothing
    /// to say about WHICH — `anchor` was the other, and it was read by nothing.
    active: Option<String>,
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

/// **`Option<Res<Project>>`, because this is a GLOBAL observer.** See [`on_cell_verb`] for the whole
/// argument; the short form is that it fires for any `Activate` anywhere in the application, and
/// `Project` belongs to a door. The entity query below is the real "is this mine" guard — it just
/// cannot run until the parameters have been validated, and in Bevy 0.19 a missing `Res<T>` panics
/// at that point rather than skipping.
fn on_note_click(
    activate: On<Activate>,
    fields: Query<&NoteField>,
    project: Option<Res<Project>>,
    mut edit: ResMut<NoteEdit>,
    mut state: ResMut<ImportState>,
) {
    if fields.get(activate.entity).is_err() {
        return;
    }
    let Some(project) = project else { return };
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
    state
        .status
        .note("describe it — Enter to keep it, Esc to leave it".to_owned());
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
                let Some((target, raw)) = edit.active.take() else {
                    return;
                };
                let text = raw.trim().to_owned();
                // The piece this field was opened on, not whatever has the focus now.
                let before = state.snapshot(&project);
                let Some((d, where_to)) = state.at_target(&target, &mut project.measured) else {
                    state
                        .status
                        .problem("the description was not kept — that tile is gone".to_owned());
                    return;
                };
                // Empty clears, the same rule the edge and anchor tokens follow — one keystroke path
                // for setting and unsetting rather than a second control for "remove".
                d.note = (!text.is_empty()).then(|| text.clone());
                state.record(before);
                let said = if text.is_empty() {
                    "description cleared".to_owned()
                } else {
                    format!("described: {text}")
                };
                state.status.say(persist(&mut project, where_to, said));
            }
            Key::Escape => {
                edit.active = None;
                state.status.note("description unchanged".to_owned());
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
/// **The lift the staged camera is currently framed on.** See `stage_camera`.
///
/// Its own resource so the re-centre fires on a *change* rather than every frame — holding
/// `rig.focus` would take panning away on this tab, which is the camera's, not this system's.
#[derive(Resource, Default)]
pub struct StagedLift(pub f32);

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

/// **Resize a piece to stand `want` metres wide**, uniformly — the SIZE field's whole arithmetic.
///
/// Returns the ratio applied; `1.0` means the typed width is the width it already stands at, in which
/// case nothing was touched — typing back the number on screen is a no-op by construction, so nothing
/// can compound. `a_width_that_is_already_set_is_a_no_op` pins that.
///
/// Pure, and split out of [`scale_keys`] for the reason the rest of this crate splits its arithmetic
/// out of its systems: the rule is the whole content of the feature, and proving it through an `App`
/// means driving a text field with a synthetic keyboard.
fn bake_width(d: &mut Descriptor, want: f32) -> Result<f32, String> {
    if !want.is_finite() || want <= 0.0 {
        return Err(format!("a piece cannot stand {want} m wide"));
    }
    let Some((w, dep)) = d.extent.footprint else {
        return Err("it has no measured footprint, so a width has nothing to resize".to_owned());
    };
    if !w.is_finite() || w <= 0.0 {
        return Err(format!(
            "it measures {w} m wide, so no resize reaches {want} m"
        ));
    }
    // The ratio against the PLACED width — which is the number on screen, so typing it back is a
    // no-op by construction and nothing compounds.
    let r = want / w;
    if (r - 1.0).abs() <= SCALE_EPS {
        return Ok(1.0);
    }

    // **Bake, exactly as `align.rotate` bakes.** `extent` records the piece as placed —
    // `src/site/visuals.rs` states the contract and `site/books` is the datum (raw mesh 0.5096 m,
    // recorded 0.306 m at scale 0.6) — so a resize rewrites the extent and *composes* the render
    // scale that maps the authored mesh onto it. A previous version stored the ratio into
    // `align.scale` over an unchanged extent, which read the field as a second multiplier; for
    // `books`, whose extent already carried its 0.6, that double-applied it and every space answer
    // shrank to 0.6x of the drawn mesh.
    d.extent.footprint = Some((w * r, dep * r));
    if let Some(h) = d.extent.height {
        d.extent.height = Some(h * r);
    }
    // The mesh-geometry corrections are proportional to the mesh, so they resize with it — the same
    // set `remeasure_rotated` rewrites for the same reason.
    if let Some((px, pz)) = d.align.pivot {
        d.align.pivot = Some((px * r, pz * r));
    }
    if let Some(y) = d.align.y_offset {
        d.align.y_offset = Some(y * r);
    }
    let s = d.align.scale.unwrap_or(1.0) * r;
    // Never an identity — see [`SCALE_EPS`]. Resizing `books` back to its authored 0.51 m composes
    // 0.6 x 1.667 = 1.0, and the honest record of that is no scale at all.
    d.align.scale = ((s - 1.0).abs() > SCALE_EPS).then_some(s);
    Ok(r)
}

fn on_scale_click(
    activate: On<Activate>,
    fields: Query<&ScaleField>,
    mut edit: ResMut<ScaleEdit>,
    mut state: ResMut<ImportState>,
    mut height: ResMut<HeightEdit>,
    mut note: ResMut<NoteEdit>,
    mut cell: ResMut<CellEdit>,
) {
    if fields.get(activate.entity).is_err() {
        return;
    }
    let Some(target) = state.target() else {
        return;
    };
    // **One field owns the keyboard.** Observers fire regardless of who is typing, and every text
    // system drains its own `MessageReader` — so two open fields would each read the same keystroke
    // stream and one Enter would commit and persist two different edits. Opening this one closes the
    // rest, uncommitted, exactly as Esc would have.
    height.active = None;
    note.active = None;
    cell.active = None;
    state.renaming = None;
    // **Starts empty**, the same call `on_size_field_click` makes and for the same measured reason:
    // seeding it with the current number meant the first digit appended to it, and it looked like it
    // had worked. The value being replaced stays on screen until Enter.
    edit.active = Some((target, String::new()));
    state.status.note("width: type the metres this piece should stand at, Enter to keep it, Esc to leave it alone"
            .to_owned());
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
                    state
                        .status
                        .note("width unchanged — nothing typed".to_owned());
                    return;
                }
                let Ok(want) = text.parse::<f32>() else {
                    state
                        .status
                        .problem(format!("`{text}` is not a number of metres"));
                    return;
                };
                // Taken before the write, which is the only moment the old value still exists.
                let before = state.snapshot(&project);
                let Some((d, where_to)) = state.at_target(&target, &mut project.measured) else {
                    state
                        .status
                        .problem("the width was not kept — that tile is gone".to_owned());
                    return;
                };
                let id = d.id.clone();
                // Refused rather than clamped — a width of zero is a piece that reserves nothing and
                // sits inside a wall with every rule reporting success.
                let r = match bake_width(d, want) {
                    Ok(r) => r,
                    Err(e) => {
                        state.status.problem(format!("`{id}`: {e}"));
                        return;
                    }
                };
                // The width it already stands at: nothing changed, so nothing to record or write.
                if r == 1.0 {
                    state
                        .status
                        .note(format!("{id} already stands {want:.2} m wide"));
                    return;
                }
                let (w, dep) = d.extent.footprint.unwrap_or((want, 0.0));
                let said = match d.align.scale {
                    Some(s) => format!("{id} — {w:.2} x {dep:.2} m (mesh scaled {s:.3}x)"),
                    None => format!("{id} — {w:.2} x {dep:.2} m, back at its authored size"),
                };
                state.record(before);
                state.status.say(persist(&mut project, where_to, said));
            }
            Key::Escape => {
                edit.active = None;
                state.status.note("width unchanged".to_owned());
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
                        let ok = c.is_ascii_digit()
                            || (c == '.' && !raw.contains('.') && !raw.is_empty());
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

/// **How far up a wall a piece hangs**, in metres. Clicking [`MountHeightField`] opens it.
///
/// `M` could put a piece *on* a wall and then not say where: [`mount_options`] is a list of literals,
/// so `1.8` — eye level, for a sign — was the only wall height reachable without hand-editing the kit's
/// `library.ron`. A picture at 1.2 m and a sconce at 2.1 m were both a text editor away.
///
/// Only offered for the two mounts that carry a height. For the rest the field is not drawn at all
/// rather than drawn dead: `emerge_core::descriptor::mount_height` is the one place that decides
/// which those are, and a disabled control is a question the panel does not need to ask.
#[derive(Resource, Default)]
pub struct HeightEdit {
    active: Option<(EditTarget, String)>,
}

impl HeightEdit {
    pub fn typing(&self) -> bool {
        self.active.is_some()
    }
}

/// The clickable height field.
#[derive(Component)]
pub struct MountHeightField;

/// The text inside it.
#[derive(Component)]
pub struct MountHeightReadout;

fn on_mount_height_click(
    activate: On<Activate>,
    fields: Query<&MountHeightField>,
    mut edit: ResMut<HeightEdit>,
    mut state: ResMut<ImportState>,
    mut width: ResMut<ScaleEdit>,
    mut note: ResMut<NoteEdit>,
    mut cell: ResMut<CellEdit>,
) {
    if fields.get(activate.entity).is_err() {
        return;
    }
    // One field owns the keyboard — see `on_scale_click`.
    width.active = None;
    note.active = None;
    cell.active = None;
    state.renaming = None;
    let Some(target) = state.target() else {
        return;
    };
    // Starts empty, the same call `on_size_field_click` and `on_scale_click` make — seeding it means
    // the first digit appends to the number already there.
    edit.active = Some((target, String::new()));
    state.status.note(
        "height: type the metres up the wall, Enter to keep it, Esc to leave it alone".to_owned(),
    );
}

/// Digits and a single point, filtered at the keystroke — the rule `size_edit_keys` states.
fn mount_height_keys(
    mut events: MessageReader<KeyboardInput>,
    mut edit: ResMut<HeightEdit>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    for event in events.read() {
        // Drained even while shut, so the click that opens this field cannot be typed into it.
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
                    state
                        .status
                        .note("height unchanged — nothing typed".to_owned());
                    return;
                }
                let Ok(want) = text.parse::<f32>() else {
                    state
                        .status
                        .problem(format!("`{text}` is not a number of metres"));
                    return;
                };
                // **Zero is legal, below the floor is not.** A wall marking at skirting height is a
                // real thing to author; a negative one is under the floor, where no wall is.
                if !want.is_finite() || want < 0.0 {
                    state
                        .status
                        .problem(format!("a wall mount cannot sit at {text} m"));
                    return;
                }
                let found =
                    state
                        .at_target(&target, &mut project.measured)
                        .and_then(|(d, where_to)| {
                            d.mount
                                .as_ref()
                                .map(|m| (d.id.clone(), m.clone(), where_to))
                        });
                let Some((id, mount, where_to)) = found else {
                    state
                        .status
                        .problem("the height was not kept — that tile is gone".to_owned());
                    return;
                };
                // Refused by name rather than silently ignored: the field is only drawn for mounts
                // that carry a height, so reaching here means the mount changed under an open field.
                let Some(next) = emerge_core::descriptor::with_mount_height(&mount, want) else {
                    state.status.problem(format!(
                        "`{id}` is not on a wall, so it has no height to set"
                    ));
                    return;
                };
                let before = state.snapshot(&project);
                let Some((d, _)) = state.at_target(&target, &mut project.measured) else {
                    state
                        .status
                        .problem("the height was not kept — that tile is gone".to_owned());
                    return;
                };
                d.mount = Some(next);
                state.record(before);
                let said = format!("{id} — {want:.2} m up the wall");
                state.status.say(persist(&mut project, where_to, said));
            }
            Key::Escape => {
                edit.active = None;
                state.status.note("height unchanged".to_owned());
            }
            Key::Backspace => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    raw.pop();
                }
            }
            Key::Character(ch) => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    for c in ch.chars() {
                        let ok = c.is_ascii_digit()
                            || (c == '.' && !raw.contains('.') && !raw.is_empty());
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

/// **Take back the last Tiles edit, or put it back.**
///
/// One body for both directions: the current state is snapshotted, the stored one is restored, and the
/// snapshot goes on the opposite stack — so undo and redo cannot drift apart the way two separate
/// implementations would.
///
/// Restored **through `commit_measured`**, the one writer, so an undo re-runs the same layering,
/// validation and atomic save an edit does. An undo that wrote the file by a second route could put
/// back a library the forward path would have refused.
fn tile_history_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    let back = keys::just_pressed(&keyboard, *live, Action::UndoTile);
    let forward = keys::just_pressed(&keyboard, *live, Action::RedoTile);
    if !back && !forward {
        return;
    }
    let verb = if back { "undo" } else { "redo" };
    let taken = if back {
        state.undo.pop()
    } else {
        state.redo.pop()
    };
    let Some(want) = taken else {
        state.status.note(format!("nothing to {verb} on this tab"));
        return;
    };

    // **The map gets a vote.** `commit_measured` validates the library against itself and never
    // against the map — that check belongs to the one forward path that can lose a descriptor,
    // `remove_tile`, which refuses while the map still places the piece. A snapshot restore is the
    // other way a descriptor can vanish (undoing an Accept, redoing a remove), and skipping the same
    // guard here rewrote `library.ron` out from under a map that still referenced it: every
    // `resolve_y` from then on refused the whole map, and the two files disagreed on disk.
    // **Every map in the project gets a vote, not just the one that used to be open.** This read
    // `project.map` — whichever map the author happened to have loaded — so restoring a snapshot
    // that dropped a piece another map placed was allowed, and that map stopped resolving with
    // nothing pointing back at the edit. See `Project::maps_that_place`.
    let mut dropped: Vec<&str> = want
        .measured
        .descriptors
        .iter()
        .map(|d| d.id.as_str())
        .collect();
    dropped.sort_unstable();
    let gone: Vec<&str> = project
        .measured
        .descriptors
        .iter()
        .map(|d| d.id.as_str())
        .filter(|id| dropped.binary_search(id).is_err())
        .collect();
    let mut missing: Vec<String> = Vec::new();
    for id in gone {
        match project.maps_that_place(id) {
            Ok(maps) if !maps.is_empty() => missing.push(format!("{id}` in `{}", maps.join("`, `"))),
            Ok(_) => {}
            // A project whose maps cannot be read cannot answer this, and guessing "nobody places
            // it" is how a guard stops guarding. Refuse and say which file.
            Err(e) => {
                if back {
                    state.undo.push(want);
                } else {
                    state.redo.push(want);
                }
                state.status.problem(format!("cannot {verb}: {e}"));
                return;
            }
        }
    }
    if !missing.is_empty() {
        let named = missing.join("`, `");
        // Back where it came from — refusing must not also cost the entry.
        if back {
            state.undo.push(want);
        } else {
            state.redo.push(want);
        }
        state.status.problem(format!(
            "cannot {verb}: `{named}` still places it — remove or undo those placements first"
        ));
        return;
    }

    let now = state.snapshot(&project);
    // The library is cloned into the commit so `want` stays whole: on failure the POPPED entry goes
    // back on its stack. A previous version pushed a snapshot of the CURRENT state instead — which
    // destroyed the real history step and replaced it with a no-op, so the next Cmd+Z reported
    // "undid the last tile edit" while changing nothing, and every deeper step was off by one.
    match commit_measured(&mut project, want.measured.clone()) {
        Ok(_) => {
            // The in-memory half moves only once the disk write has succeeded — a failed commit
            // must leave both halves exactly as they were.
            state.candidates = want.candidates;
            state.selected = state.selected.min(state.candidates.len().saturating_sub(1));
            if back {
                state.redo.push(now);
                state.status.note("undid the last tile edit".to_owned());
            } else {
                state.undo.push(now);
                state.status.note("put the tile edit back".to_owned());
            }
        }
        Err(e) => {
            if back {
                state.undo.push(want);
            } else {
                state.redo.push(want);
            }
            state.status.problem(format!("could not {verb}: {e}"));
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
            min_height: Val::Px(MIN_FIELD_H),
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
            TextFont::from_font_size(crate::chrome::text::HINT),
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
        Some(c) if c.solid && c.edge.is_some() => "%",
        Some(c) if c.solid => "#",
        Some(c) if c.edge.is_some() => "E",
        Some(_) => ".",
    }
}

/// **`Option<ResMut<Project>>`, and every global observer here needs the same.**
///
/// This fires for *any* `Activate` in the application, not only ones inside this panel — and
/// `Project` is a **door's** resource, removed by `screen::close_the_door`. On `Screen::Menu` it does
/// not exist, and in Bevy 0.19 a missing `Res<T>` **panics its system** rather than skipping
/// (`bevy_ecs/error/handler.rs:130`).
///
/// The `Query` below is the real guard — a menu row has no `CellVerb` — but parameters are validated
/// *before* a body runs, so it never got the chance. This was invisible for as long as
/// `chrome::list_row` was only ever called by editor panels; the moment the menu adopted the shared
/// row vocabulary, the first click on it took the whole application down. Found 2026-08-18 by
/// FVS-S-34a, and `tests/the_sweep_is_finished.rs` now fails on a sixth.
fn on_cell_verb(
    activate: On<Activate>,
    verbs: Query<&CellVerb>,
    mut edit: ResMut<CellEdit>,
    project: Option<ResMut<Project>>,
    mut state: ResMut<ImportState>,
) {
    let Ok(verb) = verbs.get(activate.entity) else {
        return;
    };
    let Some(mut project) = project else { return };
    apply_verb(*verb, &mut edit, &mut project, &mut state);
}

/// **Do one of the four things to the selected cell.** The chip and the key both come here.
///
/// Split out of the observer so the keyboard is not a second implementation of the same four verbs —
/// `docs/ui.md` §4.2 requires everything reachable by mouse to be reachable by keyboard, and the way
/// that requirement usually gets met is by writing it twice and letting the two drift.
fn apply_verb(verb: CellVerb, edit: &mut CellEdit, project: &mut Project, state: &mut ImportState) {
    let Some(at) = edit.at else {
        state.status.note("pick a cell first".to_owned());
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
    /// Advance this axis by `quarters` 90-degree steps, wrapping at a full turn.
    ///
    /// The count exists because the labeler can now ask for two — an asset authored upside down —
    /// and turning it twice as two separate acts would be two undo entries, two re-measures and a
    /// moment in between where the piece is on its side and something else could read it.
    fn bumped(self, r: (i32, i32, i32), quarters: u8) -> (i32, i32, i32) {
        let step = |v: i32| (v + 90 * i32::from(quarters)).rem_euclid(360);
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
/// Returns whether the piece actually turned — the labeler's apply chains a re-photograph on a
/// successful righting turn, and a refusal (the authored-cells guard, an unopenable GLB) must
/// not trigger one.
fn rotate_mesh(
    axis: RotateAxis,
    quarters: u8,
    force: bool,
    project: &mut Project,
    state: &mut ImportState,
) -> bool {
    let Some(d) = state.editing(&project.measured) else {
        state.status.note("no tile is selected".to_owned());
        return false;
    };
    let Some(mesh) = d.mesh.clone() else {
        state
            .status
            .problem(format!("`{}` has no mesh to turn", d.id));
        return false;
    };
    let authored_cells = d.subgrid.as_ref().map_or(0, |g| g.cells.len());
    if axis != RotateAxis::Y && authored_cells > 0 && !force {
        state.status.problem(format!(
            "`{}` has {authored_cells} authored cell(s). A {} turn swaps its height with a floor \
             axis and there is no mapping for that — hold Shift to turn it anyway and clear them.",
            d.id,
            axis.label()
        ));
        return false;
    }
    // **Before the extent moves.** A Y turn maps the cells onto the turned piece, and the mapping
    // reads the divisions they were authored against.
    let div_before = focused_div(state, project).ok();
    let path = project.root.join("assets").join(&mesh);
    let glb = match emerge_core::glb::Glb::open(&path) {
        Ok(glb) => glb,
        Err(why) => {
            state.status.problem(format!("{mesh}: {why}"));
            return false;
        }
    };

    // Taken before the write — the only moment the old value still exists.
    let history_before = state.snapshot(&project);
    let Some((d, where_to)) = state.editing_mut(&mut project.measured) else {
        return false;
    };
    let want = axis.bumped(d.align.rotate.unwrap_or((0, 0, 0)), quarters);
    let before = d.align.rotate;
    // A rotation of nothing is not a rotation — keep the field absent rather than storing an
    // identity nobody authored, so a descriptor that was never turned still says so.
    d.align.rotate = (want != (0, 0, 0)).then_some(want);
    if let Err(why) = emerge_core::import::remeasure_rotated(d, &glb, before) {
        d.align.rotate = before;
        state.status.problem(why);
        return false;
    }
    // The lattice, moved or dropped. Both halves of a Y turn together — the cells and the divisions —
    // because a lattice read off one without the other is the wrong shape; `rotate_div` is not called
    // here only because divisions are derived from the extent that just changed.
    let lattice = match (axis, div_before) {
        (RotateAxis::Y, Some(div)) => {
            d.subgrid = d.subgrid.take().map(|g| g.rotated(quarters, div));
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
    state.record(history_before);
    state.status.say(persist(project, where_to, said));
    true
}

/// The chip.
/// **`Option<Res<Project>>`, because this is a GLOBAL observer.** See [`on_cell_verb`] for the whole
/// argument; the short form is that it fires for any `Activate` anywhere in the application, and
/// `Project` belongs to a door. The entity query below is the real "is this mine" guard — it just
/// cannot run until the parameters have been validated, and in Bevy 0.19 a missing `Res<T>` panics
/// at that point rather than skipping.
fn on_rotate_click(
    activate: On<Activate>,
    axes: Query<&RotateAxis>,
    keyboard: Res<ButtonInput<KeyCode>>,
    project: Option<ResMut<Project>>,
    mut state: ResMut<ImportState>,
) {
    let Ok(axis) = axes.get(activate.entity) else {
        return;
    };
    let Some(mut project) = project else { return };
    rotate_mesh(*axis, 1, held_shift(&keyboard), &mut project, &mut state);
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
/// **Edge tokens read off the mesh, waiting for a yes.**
///
/// The proposal half of FVS-R-26. `emerge_core::adjacency::derive_edges` does the reading; this holds
/// the answer until an author accepts it, on the machine-proposed / human-confirmed pattern
/// `labels.rs` already runs for the vision model. Nothing here reaches `library.ron`.
///
/// **Keyed by descriptor id**, because the selection moves: a proposal about `site/wall` must not be
/// applied to whatever the author walked to afterwards. `accept_derived_edges` checks it.
#[derive(Resource, Default)]
pub struct DerivedEdges(pub Option<Derived>);

/// One staged derivation.
pub struct Derived {
    /// The descriptor the cells were read from.
    pub id: String,
    /// `(cell, token)` for every boundary cell, ascending — see `adjacency::derive_edges`.
    pub cells: Vec<((u32, u32, u32), &'static str)>,
}

impl Derived {
    /// How many of the proposals would actually change something, and how many are already that.
    ///
    /// Stated as a delta rather than a total, because `docs/ui.md` §3.2 asks for the change and
    /// because "propose 48 cells" on a piece that already carries 48 identical tokens reads as work
    /// about to happen when none is.
    pub fn delta(&self, grid: Option<&emerge_core::descriptor::Subgrid>) -> (usize, usize) {
        let mut changed = 0;
        let mut same = 0;
        for (at, token) in &self.cells {
            let now = grid.and_then(|g| g.at(*at)).and_then(|c| c.edge.as_deref());
            if now == Some(*token) {
                same += 1;
            } else {
                changed += 1;
            }
        }
        (changed, same)
    }
}

/// Returns the derivation for the caller to stage, or `None` when there was nothing to read.
///
/// **The caller decides whether to stage it, and that is load-bearing.** `autoscan_candidate` runs
/// this on every selection change; staging there would put the tab into `keys::Stance::Proposed`
/// without anybody asking, which silently changes what `Enter` means while an author is only walking
/// the list. The automatic scan is deliberately unobtrusive — its own note says so — and a proposal
/// that reassigns a key is not. Only the asked-for `B` stages.
fn scan_mesh(project: &mut Project, state: &mut ImportState) -> Option<Derived> {
    let div = match focused_div(state, project) {
        Ok(div) => div,
        Err(why) => {
            state.status.problem(why);
            return None;
        }
    };
    let Some(d) = state.editing(&project.measured) else {
        return None;
    };
    let Some(mesh) = d.mesh.clone() else {
        state
            .status
            .problem(format!("`{}` has no mesh to scan", d.id));
        return None;
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
                state.status.problem(why);
                return None;
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
            state.status.problem(format!("{mesh}: {why}"));
            return None;
        }
    };

    // Taken before the write — the only moment the old value still exists.
    let history_before = state.snapshot(&project);
    let Some((d, where_to)) = state.editing_mut(&mut project.measured) else {
        return None;
    };
    let grid = d.lattice_mut();
    for &at in &cells {
        grid.set_solid(at, div);
    }
    d.settle_lattice();
    let total = emerge_core::descriptor::Subgrid::volume(div);
    // **The same rasterisation, read a second way** — and this is what `SubCell::solid` turns out to
    // be for. Its own note records that nothing read it to decide anything (FVS-Q-9, closed *no*);
    // the boundary of a solid lattice is exactly the socket a neighbour has to match, so the scan
    // that marks the solids can propose the tokens in the same act.
    let proposal = d
        .subgrid
        .as_ref()
        .map(|g| emerge_core::adjacency::derive_edges(g, div))
        .unwrap_or_default();
    let id = d.id.clone();
    let said = format!("scanned {mesh}: {} of {total} cells solid", cells.len());
    state.record(history_before);
    state.status.say(persist(project, where_to, said));

    Some(Derived {
        id,
        cells: proposal,
    })
}

/// **Write derived tokens onto a descriptor's lattice**, returning how many cells actually moved.
///
/// One function, called twice: once on a throwaway candidate to ask the vocabulary whether the
/// result is loadable, and once on the real descriptor when it is. Two spellings of "apply the
/// proposal" would be two things to keep in step, and the whole point of the candidate is that it is
/// the *same* edit.
fn apply_edges(
    d: &mut emerge_core::descriptor::Descriptor,
    cells: &[((u32, u32, u32), &'static str)],
) -> usize {
    let grid = d.lattice_mut();
    let mut written = 0usize;
    for (at, token) in cells {
        match grid.cells.iter_mut().find(|c| c.at == *at) {
            Some(c) => {
                if c.edge.as_deref() != Some(*token) {
                    c.edge = Some((*token).to_owned());
                    written += 1;
                }
            }
            None => {
                grid.cells.push(emerge_core::descriptor::SubCell {
                    at: *at,
                    edge: Some((*token).to_owned()),
                    ..Default::default()
                });
                written += 1;
            }
        }
    }
    d.settle_lattice();
    written
}

/// **Take the derived edges, or leave them** — the commit door on FVS-R-26.
///
/// # It refuses an undeclared token by name, and that is the design rather than a gap
///
/// `edge` is a **closed** vocabulary axis. `vocab.rs` says why in as many words: an empty axis is
/// *"the honest reading of 'this project has not decided what its tiles present'"*, and the tokens
/// are matched by equality, so *"a typo does not read as a wrong token, it reads as a token that
/// matches nothing."* A derivation that quietly widened `vocab.ron` would be taking that decision on
/// the author's behalf — so this states which tokens to declare and refuses until they are.
///
/// One round-trip the first time, and none after. The alternative — writing the axis from an
/// authoring action — was considered and turned down (author's call, 2026-08-12).
fn accept_derived_edges(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
    mut derived: ResMut<DerivedEdges>,
) {
    let accept = keys::just_pressed(&keyboard, *live, Action::AcceptEdges);
    let discard = keys::just_pressed(&keyboard, *live, Action::Cancel);
    if !accept && !discard {
        return;
    }
    if discard {
        if derived.0.take().is_some() {
            state
                .status
                .note("derived edges thrown away — the lattice is as it was".to_owned());
        }
        return;
    }
    let Some(proposal) = derived.0.take() else {
        return;
    };

    // **The selection may have moved.** A proposal names the descriptor it was read from, so it can
    // never be applied to whatever the author walked to since.
    let editing = state.editing(&project.measured).map(|d| d.id.clone());
    if editing.as_deref() != Some(proposal.id.as_str()) {
        state.status.problem(format!(
            "the derived edges are for `{}`, and the focus has moved — rescan with {} to propose \
             again",
            proposal.id,
            crate::keys::chord(Action::ScanMesh),
        ));
        return;
    }

    // **The vocabulary is asked before anything is written, and it is asked of the vocabulary.**
    //
    // Not a second copy of the rule: `Vocabularies::masks` already refuses an undeclared edge token
    // by name, and its message is better than one written here would be — it names the token, prints
    // the axis as it stands, and says *"Growing a vocabulary is one row in the table — never a second
    // list."* Restating that would be the second census `keys.rs` exists to prevent, one layer down.
    //
    // What this adds is only *when*. Without it the write lands in memory and `persist` refuses at
    // save time, leaving the editor showing tokens the project cannot load — which is exactly the
    // defect `9af92aa` fixed one tab over, under the title *"the tile assembler stops writing state
    // it then refuses to save"*. So the check is run against a **candidate**, and the real descriptor
    // is only touched once the candidate passes.
    let Some(current) = state.editing(&project.measured).cloned() else {
        return;
    };
    let mut candidate = current;
    apply_edges(&mut candidate, &proposal.cells);
    if let Err(why) = project.vocab.masks(&candidate) {
        state.status.problem(format!(
            "not applied — {why} Declare it in vocab.ron and press {} again.",
            crate::keys::chord(Action::ScanMesh),
        ));
        return;
    }

    let history_before = state.snapshot(&project);
    let Some((d, where_to)) = state.editing_mut(&mut project.measured) else {
        return;
    };
    let written = apply_edges(d, &proposal.cells);
    d.settle_lattice();
    let said = format!("kept the derived edges: {written} cell(s) written");
    state.record(history_before);
    state.status.say(persist(&mut project, where_to, said));
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
    // Deliberately dropped: see `scan_mesh`'s note. An automatic scan marks solids and proposes
    // nothing, so walking the candidate list never changes what a key does.
    let _ = scan_mesh(&mut project, &mut state);
}

/// The chip.
/// **`Option<Res<Project>>`, because this is a GLOBAL observer.** See [`on_cell_verb`] for the whole
/// argument; the short form is that it fires for any `Activate` anywhere in the application, and
/// `Project` belongs to a door. The entity query below is the real "is this mine" guard — it just
/// cannot run until the parameters have been validated, and in Bevy 0.19 a missing `Res<T>` panics
/// at that point rather than skipping.
fn on_scan_mesh(
    activate: On<Activate>,
    buttons: Query<&ScanMeshButton>,
    project: Option<ResMut<Project>>,
    mut state: ResMut<ImportState>,
    mut derived: ResMut<DerivedEdges>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(mut project) = project else { return };
    // **Only the asked-for scan stages a proposal** — see `scan_mesh`'s note on why the automatic
    // one must not.
    if let Some(proposal) = scan_mesh(&mut project, &mut state) {
        let count = proposal.cells.len();
        let (changed, same) = proposal.delta(
            state
                .editing(&project.measured)
                .and_then(|d| d.subgrid.as_ref()),
        );
        *derived = DerivedEdges(Some(proposal));
        state.status.note(format!(
            "{count} boundary cell(s) read from the mesh — {changed} would change, {same} already \
             match. {} keeps them, {} throws them away",
            crate::keys::chord(crate::keys::Action::AcceptEdges),
            crate::keys::chord(crate::keys::Action::Cancel),
        ));
    }
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
            state.status.problem(why);
            return;
        }
    };
    match verb {
        CellVerb::Solid => {
            // Taken before the write — the only moment the old value still exists.
            let history_before = state.snapshot(&project);
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
            state.record(history_before);
            state.status.say(persist(project, where_to, said));
        }
        CellVerb::Clear => {
            // Taken before the write — the only moment the old value still exists.
            let history_before = state.snapshot(&project);
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
            state.record(history_before);
            state.status.say(persist(project, where_to, said));
        }
        CellVerb::Edge => {
            // Starts empty, and Enter on an empty field CLEARS the token — one keystroke path for
            // setting and unsetting, rather than a second control for "remove".
            edit.active = Some(String::new());
            edit.target = state.target();
            // **The target is captured here, not read at commit.** One cell or a whole span, the
            // same way: the cells this field was opened against are the cells it writes to. Reading
            // the live selection at `Enter` meant a click that moved the cursor mid-word redirected
            // the edit — or, once that was noticed, refused it and threw away what had been typed.
            // Neither is what the author asked for; the cell they opened the field on is.
            edit.pending = Some(cells.to_vec());
            state.status.note(if many {
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
            });
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
/// **How far off the stage floor a piece hangs**, metres — its mount's own height, or zero.
///
/// The Tiles tab used to draw every piece sitting on the stage floor, whatever its mount said, so a
/// sconce at 1.8 m and a floor crate were displayed identically and the height field's effect was
/// invisible until the piece was placed on a map. This is the one number that makes the preview show
/// what the map will show, and every part of the staged drawing reads it: the mesh, the lattice box,
/// the picker, and the footprint rectangles.
///
/// `mount_height` is the only thing that decides which mounts have a height, so this cannot disagree
/// with the field that edits it.
/// **How high a staged piece stands above the stage floor**, for every tab that stages one.
///
/// The mount's own height, then the mesh's correction on top — the order [`emerge_core::stack::datum`]
/// applies them in, which is what a placed piece's `y` is resolved from. Shared by the Meshes stage
/// and the Tiles ghost because they were 0.31 m apart: the ghost used `build::BROUGHT_IN`'s flat
/// lift, so the same piece stood at two heights depending on which tab you were looking at.
pub(crate) fn staged_lift(d: &Descriptor) -> f32 {
    stage_lift(d) + d.align.y_offset.unwrap_or(0.0)
}

fn stage_lift(d: &Descriptor) -> f32 {
    d.mount
        .as_ref()
        .and_then(emerge_core::descriptor::mount_height)
        .unwrap_or(0.0)
}

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
    let h = emerge_core::descriptor::placed_height(d)
        .unwrap_or(0.0)
        .max(0.05);
    // **Lifted with the mesh.** The picker turns a click into a cell by dividing this box by `div`,
    // so a box left on the floor under a mesh drawn 1.8 m up would hand every click the wrong cell —
    // or no cell, since the ray would miss it entirely.
    Some((
        STAGE - Vec3::new(w * 0.5, -stage_lift(d), dep * 0.5),
        Vec3::new(w, h, dep),
        div,
    ))
}

fn pick_lattice(
    mode: Res<Mode>,
    pointer: Res<crate::view::Pointer>,
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
    if *mode != Mode::Meshes || hovered_ui.iter().any(|h| h.0) {
        clear(&mut pick);
        return;
    }
    let Some(camera) = camera else {
        clear(&mut pick);
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(cursor) = pointer.0 else {
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
    state.status.note(match face {
        Some(f) => format!(
            "cell {},{},{} — {} face, the side a neighbour reads",
            at.0,
            at.1,
            at.2,
            f.label()
        ),
        None => format!("cell {},{},{} — picked from above", at.0, at.1, at.2),
    });
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
    mut derived: ResMut<DerivedEdges>,
) {
    let Ok((dx, dy, dz)) = focused_div(&state, &project) else {
        return;
    };
    // `divisions` refuses a zero, but this is a cursor and `dy - 1` on one would wrap to u32::MAX.
    if dx == 0 || dy == 0 || dz == 0 {
        return;
    }

    let pressed = |a| keys::just_pressed(&keyboard, *live, a);

    // Everything reachable by mouse is reachable by key (`docs/ui.md` §4.2), so the scan chip has
    // one too — and it goes through the same `scan_mesh` the chip calls, not a second copy.
    if pressed(Action::ScanMesh) {
        // **Only the asked-for scan stages a proposal** — see `scan_mesh`'s note on why the automatic
        // one must not.
        if let Some(proposal) = scan_mesh(&mut project, &mut state) {
            let count = proposal.cells.len();
            let (changed, same) = proposal.delta(
                state
                    .editing(&project.measured)
                    .and_then(|d| d.subgrid.as_ref()),
            );
            *derived = DerivedEdges(Some(proposal));
            state.status.note(format!(
            "{count} boundary cell(s) read from the mesh — {changed} would change, {same} already \
             match. {} keeps them, {} throws them away",
            crate::keys::chord(crate::keys::Action::AcceptEdges),
            crate::keys::chord(crate::keys::Action::Cancel),
        ));
        }
        return;
    }
    for (action, axis) in [
        (Action::RotateMeshX, RotateAxis::X),
        (Action::RotateMeshY, RotateAxis::Y),
        (Action::RotateMeshZ, RotateAxis::Z),
    ] {
        if pressed(action) {
            rotate_mesh(axis, 1, held_shift(&keyboard), &mut project, &mut state);
            return;
        }
    }

    for (action, verb) in [
        (Action::CellSolid, CellVerb::Solid),
        (Action::CellEdge, CellVerb::Edge),
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
                let Some(raw) = edit.active.take() else {
                    return;
                };
                // The cells this field was opened against — see `apply_verb_to`. A field with no
                // target was never opened by a verb, so there is nothing it could mean; that is a
                // bug rather than an author error, and it says so instead of guessing at a cell.
                let Some(targets) = edit.pending.take() else {
                    state.status.problem(format!(
                        "`{raw}` was not kept — this field was opened without a cell."
                    ));
                    return;
                };
                let Some(target) = edit.target.take() else {
                    state.status.problem(format!(
                        "`{raw}` was not kept — this field was opened without a tile."
                    ));
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
                        state.status.problem(why);
                        return;
                    }
                };
                // Taken before the write — the only moment the old lattice still exists. This was
                // the one Tiles edit invisible to the history: undoing past an unrecorded token
                // destroyed it along with whichever edit the snapshot actually belonged to.
                let history_before = state.snapshot(&project);
                let Some((d, where_to)) = state.at_target(&target, &mut project.measured) else {
                    state
                        .status
                        .problem(format!("`{raw}` was not kept — that tile is gone."));
                    return;
                };
                let mut wrote = 0usize;
                for &at in &targets {
                    let ok = d.lattice_mut().set_edge(at, div, &token);
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
                // Nothing landed means nothing to write, and nothing to remember either.
                // Nothing landed means nothing to write, and nothing to remember either.
                if wrote == 0 {
                    state.status.note(said);
                } else {
                    state.record(history_before);
                    let wrote = persist(&mut project, where_to, said);
                    state.status.say(wrote);
                }
            }
            Key::Escape => {
                edit.active = None;
                edit.pending = None;
                state.status.note("cell unchanged".to_owned());
            }
            Key::Backspace => {
                if let Some(raw) = edit.active.as_mut() {
                    raw.pop();
                }
            }
            Key::Space => {
                if let Some(raw) = edit.active.as_mut() {
                    raw.push(' ');
                }
            }
            Key::Character(ch) => {
                if let Some(raw) = edit.active.as_mut() {
                    raw.push_str(ch);
                }
            }
            _ => {}
        }
    }
}

/// Fill a row, a column or a whole layer with the verb the chips last used.
/// **`Option<Res<Project>>`, because this is a GLOBAL observer.** See [`on_cell_verb`] for the whole
/// argument; the short form is that it fires for any `Activate` anywhere in the application, and
/// `Project` belongs to a door. The entity query below is the real "is this mine" guard — it just
/// cannot run until the parameters have been validated, and in Bevy 0.19 a missing `Res<T>` panics
/// at that point rather than skipping.
fn on_fill_header(
    activate: On<Activate>,
    headers: Query<&FillHeader>,
    mut edit: ResMut<CellEdit>,
    project: Option<ResMut<Project>>,
    mut state: ResMut<ImportState>,
) {
    let Ok(header) = headers.get(activate.entity) else {
        return;
    };
    let Some(mut project) = project else { return };
    let Ok((dx, _, dz)) = focused_div(&state, &project) else {
        return;
    };
    let y = header.layer;
    let cells: Vec<(u32, u32, u32)> = match header.span {
        Span::Layer => (0..dz)
            .flat_map(|z| (0..dx).map(move |x| (x, y, z)))
            .collect(),
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
    mut cells: Query<(&CellButton, &CellLayer, &Hovered, &mut BackgroundColor)>,
    mut glyphs: Query<
        (&CellGlyph, &CellLayer, &mut Text, &mut TextColor),
        (
            Without<SelectedCellLine>,
            Without<NoteReadout>,
            Without<ScaleReadout>,
        ),
    >,
    mut lines: Query<
        (&mut Text, &mut TextColor),
        (
            With<SelectedCellLine>,
            Without<CellGlyph>,
            Without<NoteReadout>,
            Without<ScaleReadout>,
        ),
    >,
    mut notes: Query<
        (&mut Text, &mut TextColor),
        (
            With<NoteReadout>,
            Without<CellGlyph>,
            Without<SelectedCellLine>,
            Without<ScaleReadout>,
        ),
    >,
    mut widths: Query<
        (&mut Text, &mut TextColor),
        (
            With<ScaleReadout>,
            Without<CellGlyph>,
            Without<SelectedCellLine>,
            Without<NoteReadout>,
            Without<MountHeightReadout>,
        ),
    >,
    height_edit: Res<HeightEdit>,
    mut heights: Query<
        (&mut Text, &mut TextColor),
        (
            With<MountHeightReadout>,
            Without<CellGlyph>,
            Without<SelectedCellLine>,
            Without<NoteReadout>,
            Without<ScaleReadout>,
        ),
    >,
) {
    // As placed, so the cells shown are the cells that exist. See [`ImportState::placed`].
    let Some(d) = state.placed(&project) else {
        return;
    };
    // A piece with no marked cells reads as an empty lattice rather than as a missing one — the
    // grid is still drawn, it just has nothing in it.
    let empty = emerge_core::descriptor::Subgrid::default();
    let grid = d.subgrid.as_ref().unwrap_or(&empty);

    for (button, layer, hovered, mut bg) in &mut cells {
        let selected = cell_edit.at == Some((button.0, layer.0, button.1));
        // Selection beats hover; hover is `chrome::ROW_HOVER`'s signifier that the cell is a
        // click target, which a 20 px square otherwise says nothing about.
        let want = if selected {
            ROW_SELECTED
        } else if hovered.0 {
            ROW_HOVER
        } else {
            ROW_BG
        };
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
            Some(raw) => format!("{},{},{}  edge `{raw}_`", at.0, at.1, at.2),
            None => format!(
                "{},{},{}  {}",
                at.0,
                at.1,
                at.2,
                grid.at(at)
                    .map(describe_cell)
                    .unwrap_or_else(|| "open".to_owned())
            ),
        },
        None => "no cell picked".to_owned(),
    };
    for (mut text, mut colour) in &mut lines {
        if text.0 != detail {
            text.0 = detail.clone();
        }
        let tint = if cell_edit.active.is_some() {
            ACCENT
        } else {
            DIM
        };
        if colour.0 != tint {
            colour.0 = tint;
        }
    }

    let (note_text, note_tint) = crate::chrome::field_text(
        note_edit.active.as_ref().map(|(_, raw)| raw.as_str()),
        match d.note.as_deref() {
            Some(n) if !n.is_empty() => (n.to_owned(), TEXT),
            _ => ("describe it\u{2026}".to_owned(), LABEL),
        },
    );
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
    let (width_text, width_tint) = crate::chrome::field_text(
        scale_edit.active.as_ref().map(|(_, raw)| raw.as_str()),
        match state
            .editing(&project.measured)
            .and_then(emerge_core::descriptor::placed_footprint)
        {
            Some((w, _)) => (format!("{w:.2}"), TEXT),
            // LABEL, matching what `rebuild_detail` paints for the same fact — the repaint used to
            // say TEXT here, a micro-fork the shared helper closes.
            None => ("--".to_owned(), LABEL),
        },
    );
    for (mut text, mut colour) in &mut widths {
        if text.0 != width_text {
            text.0 = width_text.clone();
        }
        if colour.0 != width_tint {
            colour.0 = width_tint;
        }
    }

    // The wall-height caret, on the same argument as the two above: `rebuild_detail` only runs on
    // `resource_changed::<ImportState>`, so without this the digits would not appear until something
    // else touched the pane.
    let (height_text, height_tint) = crate::chrome::field_text(
        height_edit.active.as_ref().map(|(_, raw)| raw.as_str()),
        (
            state
                .editing(&project.measured)
                .and_then(|e| e.mount.as_ref())
                .and_then(emerge_core::descriptor::mount_height)
                .map_or_else(|| "--".to_owned(), |h| format!("{h:.2}")),
            TEXT,
        ),
    );
    for (mut text, mut colour) in &mut heights {
        if text.0 != height_text {
            text.0 = height_text.clone();
        }
        if colour.0 != height_tint {
            colour.0 = height_tint;
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
    pub fn editing<'a>(
        &'a self,
        library: &'a emerge_core::library::Library,
    ) -> Option<&'a Descriptor> {
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
            None => self
                .current()
                .map(|c| EditTarget::Candidate(c.mesh.clone())),
        }
    }

    /// **Put the highlight on this piece** — the one cursor the lists, the detail pane and the
    /// stage all read.
    ///
    /// Asked for at the keyboard, 2026-08-18: *"when I hit Shift+L and label all, it should
    /// automatically switch to what's being labeled."* A walk of hundreds photographs one piece at
    /// a time and the panel said only how many were done, so the piece the model was being asked
    /// about was the one thing on screen that nothing pointed at — Nielsen's visibility-of-system-
    /// status failure in its third form, *"the user cannot identify where he is standing in the
    /// interaction process"* (Nasir 2024, arXiv:2408.06955 §4.3.1.2, restating Nielsen 1994b).
    ///
    /// **The highlight is three facts, so this is three writes.** [`Self::selected_library_id`] is
    /// the discriminant — a candidate has to take the focus off the library list or the pane keeps
    /// showing the row that had it — and [`Self::focused_pack`] is the other half of the same
    /// cursor: a highlight standing on a pack heading is not standing on a mesh.
    ///
    /// **The fold is not cosmetic.** `keep_candidate_selection_visible` moves the selection off any
    /// row the author cannot see, so a selection set inside a folded pack is undone on the next
    /// frame. Since the first scan folds every pack this kit does not draw from, that is most of
    /// them — the follow would be silently inert exactly when a walk crosses into a new pack.
    pub(crate) fn focus_on(&mut self, target: &EditTarget) {
        match target {
            EditTarget::Library(id) => {
                self.selected_library_id = Some(id.clone());
                self.focused_pack = None;
            }
            EditTarget::Candidate(mesh) => {
                // Gone since the walk was built — a rescan, a removal. Leave the highlight where
                // the author can see it rather than moving it somewhere arbitrary.
                let Some(ix) = self.candidates.iter().position(|c| &c.mesh == mesh) else {
                    return;
                };
                // The key `packs` groups by, read the way `packs` reads it, so the fold this opens
                // is the one the row is under.
                self.folded_packs
                    .remove(mesh.rsplit_once('/').map_or(".", |(dir, _)| dir));
                self.selected = ix;
                self.selected_library_id = None;
                self.focused_pack = None;
            }
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
    // **The authoring kit's own layer**, since patches are per-kit and `Policy::apply` refuses a
    // rule that matches nothing.
    let layered = project.policy.apply(&measured)?;
    // **Then the merge, because the palette is every bound kit.** Replacing `project.library` with
    // the single kit's layer here would drop every other kit's pieces out of the palette the moment
    // a mesh was imported — the edit would look like it had deleted the rest of the project.
    let library = project.merged_with(&layered)?;
    library.validate_lattices(project.lattice.face_bands)?;
    let masks = library.resolve(&project.vocab)?;

    let path = project.library_path.clone();
    let text = measured.to_ron()?;
    emerge_core::ron_surgery::save_atomic(&path, &text)?;

    // **What the Map has to redraw, worked out by comparison.** The already-placed entities were
    // built from the shapes in `project.library`; anything whose resolved descriptor differs in the
    // library replacing it is now standing on screen in a form the project no longer describes.
    // Derived here rather than declared by the fifteen edit paths that reach this door — see
    // `Project::touched`.
    project
        .touched
        .extend(changed_ids(&project.library, &library));

    project.measured = measured;
    // The kit's own layer moves too. Without this the next `merged_with` would rebuild from the
    // layer as it was at open, and the second import in a session would undo the first.
    if let Some(k) = project
        .kits
        .iter_mut()
        .find(|k| k.dir == project.emerge_dir)
    {
        k.measured = project.measured.clone();
        k.library = layered;
    }
    project.library = library;
    project.masks = masks;
    project.remeasure_triangles();
    Ok(path)
}

/// Ids in `new` that `old` did not have, or had differently.
///
/// `Descriptor` derives `PartialEq`, so this is the whole of "did this piece change". An id that
/// only exists in `old` is a removal, and a removed descriptor has no placements left to redraw —
/// `commit_measured` refuses a removal the map still uses.
fn changed_ids(
    old: &emerge_core::library::Library,
    new: &emerge_core::library::Library,
) -> Vec<String> {
    new.descriptors
        .iter()
        .filter(|d| old.get(&d.id) != Some(*d))
        .map(|d| d.id.clone())
        .collect()
}

#[cfg(test)]
mod changed_ids_tests {
    use super::*;
    use emerge_core::descriptor::Descriptor;
    use emerge_core::library::Library;

    fn lib(ds: Vec<Descriptor>) -> Library {
        Library {
            version: 1,
            note: None,
            descriptors: ds,
        }
    }

    fn piece(id: &str, y_offset: Option<f32>) -> Descriptor {
        let mut d = Descriptor {
            id: id.to_owned(),
            ..Descriptor::default()
        };
        d.align.y_offset = y_offset;
        d
    }

    /// **A descriptor that changed is named; one that did not is not.**
    ///
    /// The whole reason this is a comparison rather than a list the edit paths append to: fifteen
    /// of them reach `commit_measured`, and the sixteenth would forget.
    #[test]
    fn only_the_pieces_that_actually_moved_are_named() {
        let before = lib(vec![piece("floor", Some(-0.06)), piece("wall", None)]);
        let after = lib(vec![piece("floor", Some(-0.12)), piece("wall", None)]);
        assert_eq!(changed_ids(&before, &after), vec!["floor".to_owned()]);
    }

    /// Nothing changed means nothing to redraw — the case that runs on every write that only
    /// touched a candidate, and the one that must not repaint the map.
    #[test]
    fn an_identical_library_names_nothing() {
        let a = lib(vec![piece("floor", Some(-0.06)), piece("wall", None)]);
        let b = lib(vec![piece("floor", Some(-0.06)), piece("wall", None)]);
        assert!(changed_ids(&a, &b).is_empty());
    }

    /// A newly accepted candidate is named. Harmless — nothing is placed under an id the map has
    /// never seen — and cheaper to allow than to special-case.
    #[test]
    fn an_added_piece_is_named_and_a_removed_one_is_not() {
        let before = lib(vec![piece("floor", None)]);
        let after = lib(vec![piece("floor", None), piece("crate", None)]);
        assert_eq!(changed_ids(&before, &after), vec!["crate".to_owned()]);
        // The other direction names nothing: a removal leaves no entry to redraw, and
        // `commit_measured` refuses one the map still places.
        assert!(changed_ids(&after, &before).is_empty());
    }
}

/// Persist a lattice edit if it landed on a library entry, and fold the outcome into the status line.
///
/// Takes the message the edit already composed, so a successful write reads as the edit rather than
/// as a file operation — and a failed one **replaces** it, because an author told "cell 1,0,2 is
/// solid" by a program that could not write the file has been told something untrue.
/// **A receipt or a refusal, and the type says which.**
///
/// This returned a bare `String` that was sometimes `NOT WRITTEN: {e}`, and all nine call sites
/// handed it to the status line as though it were a receipt — so an edit that never reached
/// `library.ron` read exactly like one that did, in the same colour, and was gone as soon as the
/// author moved the cursor. See [`crate::chrome::Status`].
fn persist(project: &mut Project, where_to: Persist, said: String) -> Result<String, String> {
    match where_to {
        Persist::InMemory => Ok(said),
        Persist::Library => match write_library(project) {
            Ok(_) => Ok(said),
            Err(e) => Err(format!("NOT WRITTEN: {e}")),
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

/// The `EXCLUDED` group's heading — clicking it opens or closes the group.
#[derive(Component)]
pub(crate) struct ExcludedHeader;

/// One tab in the strip, carrying the mode it selects. `pub(crate)` so the anim bench's stale
/// badge can find its own tab and repaint the label in place.
#[derive(Component, Clone, Copy)]
pub struct Tab(pub Mode);

/// The tab's name, so the active one can be lit without touching its key.
#[derive(Component)]
pub struct TabLabel;

/// The tab's shortcut, styled a step quieter than the name.
#[derive(Component)]
struct TabKey;

/// The tab's badge — the anim bench's stale count, in its own text so it can hold its own colour.
///
/// Its own node rather than a suffix on [`TabLabel`], because `style_tabs` owns every label's
/// `TextColor` per frame (active/inactive), so a colour written into the label is stomped on the
/// next frame. The badge carries no `TabLabel`, so the frame-owner never touches it — the same
/// division that keeps [`TabKey`] a step quieter than the name.
///
/// `DANGER`, and only the count: a persistent peripheral element wants medium intensity, not
/// maximum (Lewandowska, Dziśko & Jankowski 2022, `10.1038/s41598-022-16284-2` — high contrast is
/// burst-only, and habituation appears at low contrast only), and the word and the count are the
/// non-colour channels `docs/ui.md` §1.3 requires; the hue is the redundant one.
#[derive(Component)]
pub struct TabBadge;

/// Root of the tiles panel, shown and hidden with the mode.
#[derive(Component)]
struct TilesRoot;

/// Root of the map panel, so the mode can hide it.
#[derive(Component)]
pub struct MapRoot;

/// Root of an animation-bench panel.
#[derive(Component)]
pub struct AnimRoot;

/// Root of a compose panel.
#[derive(Component)]
pub struct ComposeRoot;

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
    fn tokens<'a>(
        self,
        v: &'a emerge_core::vocab::Vocabularies,
    ) -> &'a emerge_core::vocab::Vocabulary {
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
pub struct LibraryRow(pub String);

/// The node the candidate list is rebuilt into.
#[derive(Component)]
struct CandidateList;

/// The node the selected candidate's detail is rebuilt into.
#[derive(Component)]
struct DetailPane;

/// The candidate standing on the grid, so an author can see what they are about to accept.
#[derive(Component)]
pub struct Preview;

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
pub struct PreviewOf(pub String);

/// The persistent line saying what the scan found.
#[derive(Component)]
struct ScanSummary;

/// The transient line saying what just happened.
#[derive(Component)]
struct ActionLine;

/// The measured footprint — what the placement rules reserve.
const FOOTPRINT: Color = Color::srgb(0.35, 0.72, 0.85); // CHROME-OK: world ink — the stage's measurement gizmos are a categorical palette, not panel chrome
/// The grid cells it occupies. Where this and the footprint differ is the tiling slack.
/// The Map tab's bounds wireframe is the same warm grey — one value, one name, in chrome.
const CELLS: Color = crate::chrome::BOUNDS_LINE;
/// The volume, so a height is seen rather than only read.
const EXTENT: Color = Color::srgb(0.24, 0.42, 0.50); // CHROME-OK: world ink — see FOOTPRINT
/// The stage floor and the plumb line up to a wall-mounted piece — dimmer than anything describing
/// the piece itself, because it is the reference rather than the subject.
const GROUND: Color = Color::srgb(0.30, 0.28, 0.26); // CHROME-OK: world ink — a stage floor; NEAR ROW_SELECTED by coincidence, distinct on purpose

pub struct TilesPlugin;

impl Plugin for TilesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Door>()
            .init_resource::<Mode>()
            .init_resource::<ImportState>()
            .init_resource::<MapView>()
            .init_resource::<CellEdit>()
            .init_resource::<DerivedEdges>()
            .init_resource::<LatticePick>()
            .init_resource::<NoteEdit>()
            // Registered in the same commit as `scale_keys` and `rebuild_detail` read it — a missing
            // `Res<T>` panics its system in Bevy 0.19 rather than skipping it (`CLAUDE.md`).
            .init_resource::<ScaleEdit>()
            .init_resource::<HeightEdit>()
            .init_resource::<StagedLift>()
            .init_resource::<DemoteArm>()
            // **The Tiles tab's state**, registered here rather than in its own plugin because both
            // tabs are this file's, and a `Res<T>` a system takes must exist before the first frame —
            // a missing one panics rather than skipping (`CLAUDE.md`).
            .init_resource::<crate::build::Build>()
            .init_resource::<crate::build::TileHistory>()
            .add_systems(
                OnEnter(crate::screen::Screen::Editor),
                (spawn_tab_strip, spawn_tiles_panel, scan_on_entering_the_kits_door)
                    .after(crate::chrome::FrameSystems),
            )
            .add_systems(
                Update,
                ((
                    // **Gated by the context, not by a run condition.** `2` typed into a filter or
                    // an id used to jump the author to another tab mid-word; `keys::just_pressed`
                    // now refuses every one of these unless the Tiles tab owns the keyboard, so the
                    // `not_typing` conditions that used to say the same thing are gone rather than
                    // duplicated. `Phase::Act` puts them all ahead of the text fields below.
                    (
                        tab_shortcuts.in_set(crate::keys::Phase::Act),
                        rescan_key.in_set(crate::keys::Phase::Act),
                    ),
                    move_selection.in_set(crate::keys::Phase::Act),
                    // **The two lattice walkers, nested as one.** A system tuple caps at twenty and
                    // this pair is one idea on two tabs: Meshes walks a mesh's own lattice, Tiles
                    // walks the tile's. `Context` keeps them from firing into each other.
                    (
                        lattice_keys.in_set(crate::keys::Phase::Act),
                        // After `lattice_keys`, so the `B` that stages a derivation cannot
                        // have its proposal answered by the same frame's `Enter`.
                        accept_derived_edges
                            .in_set(crate::keys::Phase::Act)
                            .after(lattice_keys),
                        // The ordering against the tab systems is gone with them: a door is
                        // settled before the first frame, so nothing can change which panel this
                        // runs under while it runs. What it used to guard against — the tile
                        // opening on the arrival frame or the next, by Bevy's choice — cannot
                        // happen when there is no arrival.
                        crate::build::build_keys.in_set(crate::keys::Phase::Act),
                        // The tile resizes to its contents, and that has to be after whatever
                        // changed them — see `build::refit_tile`.
                        crate::build::refit_tile
                            .in_set(crate::keys::Phase::Act)
                            .after(crate::build::build_keys),
                        // **Last**, so a resize is part of the same step as the edit that caused it
                        // rather than a second thing to undo.
                        crate::build::tile_history
                            .in_set(crate::keys::Phase::Act)
                            .after(crate::build::refit_tile),
                    ),
                    autoscan_candidate.run_if(in_meshes_mode),
                    // Nested: a system tuple caps out, and these three are one feature anyway —
                    // point at the piece, click a cell, see which one is under the cursor.
                    // Nested: a system tuple caps out at twenty, and these five are one feature —
                    // the staged piece, what the cursor is on, and what a click does to it.
                    (
                        pick_lattice,
                        click_lattice.run_if(in_meshes_mode),
                        draw_pick.run_if(in_meshes_mode),
                        draw_preview_footprint.run_if(in_meshes_mode),
                        draw_subgrid.run_if(in_meshes_mode),
                    ),
                    // Nested as a pair, because a system tuple caps out at twenty and these two are
                    // one rule: a selection the filter has hidden must not stay selected, in either
                    // list. Accept and Remove both act on a selection.
                    (
                        // **Not `in_meshes_mode`.** The filter field and the library list are in
                        // the panel the Meshes and Tiles tabs share, so a filter typed while Tiles is
                        // live can hide the selected row there too — and on that tab the selection is
                        // what `Enter` drops. Gated on the panel, not on one of the two tabs in it.
                        keep_library_selection_visible.run_if(in_tiles_panel),
                        // The same argument, and it was missed here: gated `in_meshes_mode`, the
                        // list followed the arrows on one of the two tabs sharing it — reported
                        // from the keyboard 2026-08-14: "the view doesn't follow my selection, it
                        // moves off screen."
                        keep_selection_on_screen.run_if(in_tiles_panel),
                        keep_candidate_selection_visible.run_if(in_meshes_mode),
                    ),
                    cycle_mount.in_set(crate::keys::Phase::Act),
                    suggestion_keys.in_set(crate::keys::Phase::Act),
                    commit_candidate.in_set(crate::keys::Phase::Act),
                    remove_tile.in_set(crate::keys::Phase::Act),
                    focus_filter.in_set(crate::keys::Phase::Act),
                    apply_mode,
                    stage_camera,
                    // **The text fields, last.** `cell_keys` and `commit_candidate` both take
                    // `ResMut<ImportState>` and used to sit in this tuple unordered — so Bevy could
                    // run the field first, clear its own typing flag, and let the same `Enter` fall
                    // through to "add to library". Six descriptors arrived in `library.ron` that way.
                    rename_candidate.in_set(crate::keys::Phase::Text),
                    (
                        cell_keys.in_set(crate::keys::Phase::Text),
                        crate::build::naming_keys.in_set(crate::keys::Phase::Text),
                    ),
                    (style_tabs, paint_label_progress, auto_apply_batch),
                    rebuild_candidates.run_if(
                        resource_changed::<ImportState>
                            .or_else(resource_changed::<crate::filter::Filters>)
                            // The list has two tabs now, and `Build::browsing` is which one is
                            // showing -- so a tab flip has to rebuild it like any other change.
                            .or_else(resource_changed::<crate::build::Build>),
                    ),
                    // **Structure only.** The selection and the carets are repainted in place by
                    // `refresh_cells`; rebuilding the pane for them is the bounce.
                    // **And on the tile in hand.** `Mode` and `Build` are here because this one pane
                    // serves two tabs: without them the tab key changed the strip and left the mesh
                    // inspector on screen, and walking the cursor moved nothing anybody could see.
                    rebuild_detail.run_if(
                        resource_changed::<ImportState>
                            .or_else(resource_changed::<crate::labels::LabelGeneration>)
                            .or_else(resource_exists_and_changed::<Mode>)
                            .or_else(resource_changed::<crate::build::Build>),
                    ),
                    refresh_lines,
                    // **Both stages, nested as one.** A system tuple caps at twenty in 0.19, and these
                    // are one idea on two tabs: Meshes stands one mesh up, Tiles stands the tile up
                    // with the grid you steer it by. They are mutually exclusive on the tab.
                    (
                        drive_preview,
                        crate::build::drive_build_preview,
                        crate::build::draw_build_grid,
                    ),
                ),)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            // A second `add_systems` rather than a nested tuple — `add_systems` caps a tuple at 20
            // in 0.19, and nesting would imply these belong together for a reason.
            .add_systems(
                Update,
                ((
                    note_keys.in_set(crate::keys::Phase::Text),
                    scale_keys.in_set(crate::keys::Phase::Text),
                    mount_height_keys.in_set(crate::keys::Phase::Text),
                    tile_history_keys.in_set(crate::keys::Phase::Act),
                    demote_tile.in_set(crate::keys::Phase::Act),
                    exclude_pack.in_set(crate::keys::Phase::Act),
                    disarm_demote.run_if(resource_changed::<ImportState>),
                    refresh_cells,
                ),)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            .add_observer(on_cell_click)
            .add_observer(on_cell_verb)
            .add_observer(on_scan_mesh)
            .add_observer(on_rotate_click)
            .add_observer(on_fill_header)
            .add_observer(on_note_click)
            .add_observer(on_scale_click)
            .add_observer(on_mount_height_click)
            .add_observer(on_candidate_click)
            .add_observer(on_library_click)
            .add_observer(on_pack_click)
            .add_observer(on_excluded_click)
            .add_observer(on_tag_chip);
    }
}

/// **The strip of panels this door holds** — three on the Kit door, one on the others.
///
/// The strip exists because a mode reachable only by a keypress is a mode you have to be told about,
/// and an editor that has to be explained has a bug in its front page. What changed with the doors
/// is its *scope*: it lists what this door shows rather than everything the binary can do, so the
/// Kit door never offers the Map.
///
/// A one-tab door draws a one-entry strip, which reads as a title. That is deliberate rather than a
/// special case — one loop, one shape, and a door that gains a panel is one arm in [`Door::tabs`].
///
/// **`Option<Res<Door>>`, like every other `OnEnter(Editor)` spawner here** (`labels::warm_cache`,
/// `anim_cache::load_bench_cache`, `thumbs::setup`): the door is a door's resource, so a screen
/// entered without one draws no strip rather than aborting the process. `screen::open_the_door`
/// already says so on the log in the one case that reaches it.
fn spawn_tab_strip(
    mut commands: Commands,
    door: Option<Res<Door>>,
    frame: Res<crate::chrome::Frame>,
) {
    let Some(door) = door else { return };
    // **Into the frame's own band, not floating over the window.** It was absolute at
    // `left: MARGIN, top: MARGIN` with `GlobalZIndex(101)` to sit above the panels it overlapped —
    // a strip that had to out-rank the thing beneath it because both were competing for the same
    // ground. In the frame nothing overlaps, so neither number is needed.
    let strip = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(2.0),
                align_items: AlignItems::Stretch,
                ..default()
            },
            // **The editor's first accessibility, and the strip is the right place to start.**
            //
            // There was none at all — `AccessibilityNode` appeared zero times in `src/`. A screen
            // reader met this application as an unlabelled tree of boxes, and the one thing it most
            // needs to describe is which panel you are in and what the alternatives are. AccessKit
            // has exactly that shape: a `TabList` of `Tab`s.
            //
            // Stated on the strip rather than derived, because nothing derives it — Bevy populates
            // roles for its own widgets and these are not its widgets.
            bevy::a11y::AccessibilityNode::from(accesskit::Node::new(accesskit::Role::TabList)),
        ))
        .id();
    commands.entity(frame.door_strip).add_child(strip);
    commands
        .entity(strip)
        .with_children(|p| {
            for (i, &mode) in door.tabs().iter().enumerate() {
                // **The key this door gives that slot**, which is why it is read off the index and
                // not off the mode: `tab_shortcuts` walks `door.tabs()` and takes
                // `Action::tab_slot(i)`, so `1` is the door's first panel whichever panel that is.
                let chord = Action::tab_slot(i).map(crate::keys::chord);
                p.spawn((
                    // **Deliberately not a `UiButton`** — see [`click_tab`]. `Hovered` is what
                    // `style_tabs` lights it with, and it is all that was ever needed.
                    Hovered::default(),
                    Tab(mode),
                    bevy::a11y::AccessibilityNode::from(accesskit::Node::new(accesskit::Role::Tab)),
                    // The label the reader hears. `mode.label()` rather than a second string, so a
                    // panel renamed once is renamed everywhere — the same rule `chrome::key_census`
                    // keeps for chords.
                    bevy::prelude::AccessibleLabel::new(mode.label()),
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
                    //
                    // It went missing when the strip became per-door and `Mode::action` — a key per
                    // panel — stopped existing; the citation above stayed, and `style_tabs` kept
                    // querying a marker nothing spawned. A one-tab door has a slot key too, so the
                    // `Option` is only ever `None` past the fourth panel.
                    if let Some(chord) = chord {
                        tab.spawn((
                            Text::new(chord),
                            TextColor(LABEL),
                            TextFont::from_font_size(crate::chrome::text::LABEL),
                            Node {
                                margin: UiRect::right(Val::Px(7.0)),
                                ..default()
                            },
                            TabKey,
                        ));
                    }
                    tab.spawn((
                        Text::new(mode.label()),
                        TextColor(LABEL),
                        TextFont::from_font_size(crate::chrome::text::TAB),
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                        TabLabel,
                    ));
                    // Empty until the anim bench has something stale to say — see [`TabBadge`].
                    tab.spawn((
                        Text::new(String::new()),
                        TextColor(DANGER),
                        TextFont::from_font_size(crate::chrome::text::LABEL),
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                        Node {
                            margin: UiRect::left(Val::Px(7.0)),
                            ..default()
                        },
                        TabBadge,
                    ));
                })
                .observe(click_tab);
            }
        });
}

/// **The tab chips answer the pointer again, and this is the record of how.**
///
/// `on_tab_click` was deleted when the strip became per-door, and for a while the chip was a
/// `ui_widgets::Button` carrying `Hovered` that `style_tabs` lit — advertising a press it did not
/// answer. Restoring the observer regressed `the_tile_feedback_script_can_actually_be_followed`,
/// because a focused `Button` also fires `Activate` on `Enter` and the script's commit key changed
/// panel out from under the step. The note left here concluded it "needs a focus decision, not just
/// an observer", and that was one word too strong: it needed the chip to stop being a `Button`.
/// [`click_tab`] is the answer — a `Pointer<Click>` observer, no focus, no `Activate`.
/// Light the active tab. The inactive one stays legible rather than greyed to nothing — a tab you
/// cannot read is a tab you do not know is there.
fn style_tabs(
    mode: Res<Mode>,
    mut tabs: Query<(
        &Tab,
        &Hovered,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
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
            ROW_HOVER
        } else {
            ROW_BG
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want_border = if active { ACCENT } else { Color::NONE };
        // Guarded like every other write in this function. `BorderColor` is change-detected and the
        // UI extraction reads that, so an unconditional assignment dirtied the whole strip sixty
        // times a second for a colour that changes when a tab does.
        if border.top != want_border {
            *border = BorderColor::all(want_border);
        }

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

/// **`F` puts the cursor in the filter box** — the keyboard half of a control that had only a
/// mouse half.
///
/// `docs/ui.md` §4.2 is the rule this satisfies: everything reachable by mouse is reachable by
/// keyboard and vice versa. The box was the standing exception — `filter::on_click` was its only
/// writer — on the tab whose whole argument is that keystrokes are faster than reaching for the
/// mouse, and with a 45-piece library to narrow.
///
/// **Leaving is already owned by `filter::keys`**, which runs in `Phase::Text`: `Enter` blurs and
/// `Esc` blurs and clears. That ordering is also what stops this from being the `xseam` bug again —
/// while the box holds focus the context is `Typing`, so the `Enter` that leaves it cannot also
/// reach `BuildDrop` in the same frame, and by the next frame it is no longer `just_pressed`.
fn focus_filter(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut filters: ResMut<crate::filter::Filters>,
) {
    if crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::FocusFilter) {
        // The pane this tab's list is filtered by — the same one `rebuild_candidates` reads, so the
        // box that takes the keys is the box above the rows they narrow.
        filters.take_focus(crate::filter::Pane::Candidates);
    }
}

/// **The tabs this one panel serves.** Stated once, so the copy pane, the problem log and the run
/// condition that keeps the shared library selection visible cannot disagree about who they are for —
/// which is exactly how the Meshes/Tiles split left the log and the pane tagged `Meshes` while the
/// tab strip grew a second entry above them.
const TILES_PANEL_TABS: &[Mode] = &[Mode::Meshes, Mode::Tiles];

/// **Two panels, the same two the map tab has.** Controls down the left, the list down the right.
///
/// The list used to be a `max_height: 300` box a third of the way down this panel — the same shape
/// the map palette was fixed out of, and for the same reason it did not work: a `max_height` inside a
/// panel that is not pinned to the bottom of the viewport is never reached. 318 candidates scrolled
/// in a 300 px window. Now it is `chrome::scroll_list` inside a `full_height` panel, which is bounded
/// by construction.
fn spawn_tiles_panel(mut commands: Commands, frame: Res<crate::chrome::Frame>) {
    crate::chrome::panel_root(
        &mut commands,
        &frame,
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
        crate::chrome::title(p, "MESHES AND TILES");
        // **One banner per tab, both in the shared panel.** `ProblemBanner` carries the tabs it
        // speaks for and `notice.rs` shows only a matching one, so a panel serving two tabs needs
        // two — without the second, every refusal the Tiles tab's verbs write would be invisible on
        // it. The detail pane and the problem log below take the other shape, `TILES_PANEL_TABS`:
        // they are single nodes whose *contents* the live tab decides, so duplicating them would be
        // two copies to keep in step rather than one node that says which tabs it is for.

        p.spawn((
            Text::new(""),
            TextColor(DIM),
            TextFont::from_font_size(crate::chrome::text::LABEL),
            Node {
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            ScanSummary,
        ));

        // **The detail scrolls.** A candidate's block is a size, a layer, four rows of tag chips and
        // however many sentences its findings need — variable, and for a mesh with several findings
        // taller than the panel. Bounded and scrollable beats running off the bottom edge, which is
        // where the "no facing is derived" note was going. The one builder, plus this pane's own
        // gap to the summary above it.
        //
        // FOLLOW-OK: content, not a list. `build_detail` and the candidate block draw sections and
        // prose lines — there is no row here the arrows walk, so there is no selection to keep on
        // screen. The scroll exists for HEIGHT, and the pane above it (`CandidateList`) is the one
        // with a selection and has `keep_selection_on_screen`.
        crate::chrome::scroll_list(p, (DetailPane, crate::notice::CopyPane(TILES_PANEL_TABS)))
            .entry::<Node>()
            .and_modify(|mut n| n.margin.top = Val::Px(8.0));

        p.spawn((
            Text::new(""),
            TextColor(ACCENT),
            TextFont::from_font_size(crate::chrome::text::LABEL),
            Node {
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            },
            ActionLine,
        ));
        // **Last, and it must be.** `margin-top: auto` is what pins it to the bottom of
        // the panel, and an auto margin in a column absorbs the free space above it — so
        // placed any earlier it pushes every sibling after it down with it.
        crate::chrome::problem_log(p, TILES_PANEL_TABS);
    });

    // **The candidate list, in its own panel against the right edge** — the same shape, the same
    // builders and the same place on screen as the map tab's palette, so moving between the two tabs
    // does not mean learning a second layout.
    crate::chrome::panel_root(
        &mut commands,
        &frame,
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
        // **The batch, while it runs** — above the list it is filling. See `paint_label_progress`.
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(crate::chrome::GAP_TIGHT),
                margin: UiRect::bottom(Val::Px(crate::chrome::GAP_ROW)),
                display: Display::None,
                ..default()
            },
            LabelProgress,
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(""),
                TextColor(ACCENT),
                TextFont::from_font_size(crate::chrome::text::LABEL),
                LabelProgressText,
            ));
            // The bar: a dim trough with a bright fill whose WIDTH is the fraction. Two nodes,
            // because a bar is the one readout where the number is not the point — the eye reads
            // "how much is left" without reading anything.
            b.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(4.0),
                    ..default()
                },
                BackgroundColor(crate::chrome::SLOT_BG),
            ))
            .with_children(|trough| {
                trough.spawn((
                    Node {
                        width: Val::Percent(0.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(ACCENT),
                    LabelProgressBar,
                ));
            });
            b.spawn((
                Text::new(""),
                TextColor(DIM),
                TextFont::from_font_size(crate::chrome::text::HINT),
                LabelProgressNow,
            ));
        });
        crate::filter::spawn(p, crate::filter::Pane::Candidates);
        // The strip sits OUTSIDE the scroll container, so it cannot scroll away — see [`ListHeader`].
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
            ListHeader,
        ));
        crate::chrome::scroll_list(p, CandidateList);
    });
}

/// **The number keys jump straight to a panel of this door**, in strip order.
///
/// `1` is this door's first panel, not the binary's — so on the Kit door `1` is Meshes. Everything
/// reachable by mouse is reachable by keyboard (`docs/ui.md` §4.2), and the strip is what both read.
fn tab_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Res<Project>,
    door: Res<Door>,
    mut mode: ResMut<Mode>,
    mut state: ResMut<ImportState>,
) {
    for (i, &want) in door.tabs().iter().enumerate() {
        let Some(action) = Action::tab_slot(i) else {
            continue;
        };
        if keys::just_pressed(&keyboard, *live, action) {
            enter_tab(want, &project, &mut mode, &mut state);
            return;
        }
    }
}

/// **Go to a panel** — the one step, so the key and the click cannot come to mean different things.
///
/// The scan-on-arrival is the part that would drift: entering Meshes for the first time is what
/// fills the library list, and a second way in that forgot it would show an empty tab exactly once,
/// on the path nobody tested.
///
/// **It takes the `ResMut`s, not `&mut Mode`.** Passing `&mut mode` where the parameter is
/// `&mut Mode` deref-*muts* the `ResMut` at the call site, and `Mut::deref_mut` calls
/// `set_changed()` — so `Mode` was marked changed before this function could refuse, on every press
/// of the slot key you are already on and every click of the chip you are already standing on.
/// `stage_camera` reads exactly that flag, so it restored the canonical stage framing and threw
/// away the author's pan and zoom, which is the discard `MapView` exists to prevent. Measured:
/// a helper taking `&mut T` reports `is_changed()` after a no-op, an in-place compare does not.
/// Read through `Deref`, write through `DerefMut`, and only then.
fn enter_tab(
    want: Mode,
    project: &Project,
    mode: &mut ResMut<Mode>,
    state: &mut ResMut<ImportState>,
) {
    if **mode == want {
        return;
    }
    **mode = want;
    // **A receipt does not follow you to the next tab.**
    //
    // Meshes and Tiles share one `ImportState`, so its single `note` string — only ever overwritten,
    // never cleared — outlived every switch between them. See `chrome::Status::clear_note` for the
    // report that closes and the BRP measurement showing the rotation it seemed to announce was not
    // happening. Cleared here rather than in a system watching `Mode`, because this is the one door
    // both the slot key and the chip click come through, and because clearing *before* the scan
    // below means a tab that has something to say still gets to say it.
    //
    // Only the note. A problem is a state the editor is in and outlives a tab on purpose.
    state.status.clear_note();
    if want == Mode::Meshes && !state.scanned {
        scan(project, state);
    }
}

/// **The strip answers a press, and it is not a `Button`.**
///
/// The chips have looked pressable since they were written — `UiButton`, `Hovered`, a hover tint —
/// and answered nothing: `on_tab_click` was deleted when the strip became per-door, leaving an
/// affordance advertising a verb it did not have. `docs/ui.md` §4.2's parity rule says everything
/// reachable by mouse is reachable by keyboard *and the reverse*.
///
/// Restoring it as a `Button` was tried and **regressed
/// `the_tile_feedback_script_can_actually_be_followed`**: a focused `ui_widgets::Button` also fires
/// `Activate` on `Enter`, so the guide script's commit key changed panel out from under the step.
/// The note left behind said wiring it back "needs a focus decision, not just an observer" — and
/// that reading was one word too strong. It needs the chip to stop being a `Button`. A press is a
/// press; `Enter` belongs to whatever has the keyboard, and a tab strip never should.
///
/// So: no `Button`, no `Activate`, no focus — a `Pointer<Click>` observer that walks the same
/// [`enter_tab`] the key does. It bubbles, so a click on the chip's chord or label reaches the chip.
fn click_tab(
    click: On<Pointer<Click>>,
    tabs: Query<&Tab>,
    project: Option<Res<Project>>,
    mode: Option<ResMut<Mode>>,
    state: Option<ResMut<ImportState>>,
) {
    let (Ok(tab), Some(project), Some(mut mode), Some(mut state)) =
        (tabs.get(click.entity), project, mode, state)
    else {
        return;
    };
    enter_tab(tab.0, &project, &mut mode, &mut state);
}

/// **`R` rescans**, because meshes arrive while the editor is open — an importer that only sees what
/// was on disk at launch is one you have to restart to use.
fn rescan_key(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Res<Project>,
    mode: Res<Mode>,
    mut state: ResMut<ImportState>,
) {
    if *mode == Mode::Meshes && keys::just_pressed(&keyboard, *live, Action::Rescan) {
        scan(&project, &mut state);
    }
}

/// **The Kits door walks the mesh directory on the way in.**
///
/// The scan used to be triggered by *arriving* on the Meshes tab — from a click, a number key or
/// `Tab`. A door is arrived at once, before the first frame, so this is that trigger with the
/// switching removed rather than a new behaviour.
fn scan_on_entering_the_kits_door(
    project: Option<Res<Project>>,
    door: Option<Res<Door>>,
    mut state: ResMut<ImportState>,
) {
    let (Some(project), Some(door)) = (project, door) else {
        return;
    };
    if door.opens_on() == Mode::Meshes && !state.scanned {
        scan(&project, &mut state);
    }
}

/// **`Shift+R` — this pack is not what the kit is built from, or it is again.**
///
/// One key, both directions: the row already says which state it is in, and a separate restore verb
/// would be a second way to say the same thing. Written to the kit's `project.ron` through
/// `policy::rewrite_exclude`, which splices the one field and leaves the file's prose alone.
pub fn exclude_pack(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mode: Res<Mode>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    if *mode != Mode::Meshes || !keys::just_pressed(&keyboard, *live, Action::ExcludePack) {
        return;
    }
    // The pack of whatever is highlighted — the same directory `packs()` groups by.
    let Some(pack) = state
        .candidates
        .get(state.selected)
        .map(|c| c.mesh.rsplit_once('/').map_or(".", |(d, _)| d).to_owned())
    else {
        state
            .status
            .problem("nothing selected — highlight a mesh in the pack first".to_owned());
        return;
    };

    let mut exclude = project.policy.exclude.clone();
    let had = exclude.iter().any(|e| e.trim_end_matches('/') == pack);
    if had {
        exclude.retain(|e| e.trim_end_matches('/') != pack);
    } else {
        exclude.push(pack.clone());
        exclude.sort();
    }

    let path = project.emerge_dir.join(emerge_core::policy::POLICY_FILE);
    let written = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
        .and_then(|text| emerge_core::policy::rewrite_exclude(&text, &exclude))
        .and_then(|out| emerge_core::ron_surgery::save_atomic(&path, &out));
    match written {
        Err(e) => state.status.problem(format!("not excluded: {e}")),
        Ok(()) => {
            // **The file is the truth, so the resource follows it** rather than being written
            // alongside — one description of what this kit excludes, not two.
            project.policy.exclude = exclude;
            state.status.note(if had {
                format!("`{pack}` is back — it can be imported and labelled again")
            } else {
                format!("`{pack}` excluded from this kit — it will not be offered or labelled")
            });
        }
    }
}

/// **Which mesh directories the kit being authored draws on.**
///
/// A pack is "in use" when a descriptor in *this kit's* `library.ron` names a mesh inside it.
///
/// # It asked about the map until the doors split, and that question has no answer here
///
/// The first rule was **library membership**, and it was wrong by one step: the site kit and the
/// furniture kit are both in the merged library, so a dozen unrelated packs opened at once. The fix
/// was to ask what the open map placed from — right while every door had a map behind it.
///
/// The Kits door does not. Its whole subject is one kit, chosen on the way in, and there is no map
/// in the session to ask about. So the question moves to the level the door is actually about: not
/// the merged library (the rule that was rejected), and not a map (the rule that has no answer), but
/// **the authoring kit's own measurements**.
///
/// That answers the original complaint directly — a session spent on one kit opens that kit's packs
/// and folds every other — and it drops the composition expansion this used to need, along with its
/// failure path, because a kit's library cannot fail to resolve against itself.
///
/// **This is a deliberate behaviour change, not a port.** An author who imported a mesh into a kit
/// but has not placed it on any map now finds its pack open rather than folded. That is the right
/// answer on a door whose job is importing and labelling meshes.
fn packs_the_kit_draws_from(project: &Project) -> std::collections::HashSet<String> {
    project
        .measured
        .descriptors
        .iter()
        .filter_map(|d| d.mesh.as_deref())
        .map(|mesh| mesh.rsplit_once('/').map_or(".", |(dir, _)| dir).to_owned())
        .collect()
}

#[cfg(test)]
mod pack_fold_tests {
    use super::*;
    use emerge_core::descriptor::Descriptor;
    use emerge_core::library::Library;

    /// **The walk moves the highlight onto the piece it is asking about, and opens the pack that
    /// piece is in.**
    ///
    /// Asked for at the keyboard, 2026-08-18: *"when I hit Shift+L and label all, it should
    /// automatically switch to what's being labeled."*
    ///
    /// The unfold is the half worth pinning. [`keep_candidate_selection_visible`] takes the
    /// selection off any row that is not on screen, so a follow into a folded pack is undone on the
    /// next frame — and since the first scan folds every pack the kit does not draw from, the
    /// feature would look intermittent rather than broken: it would work on whichever pack happened
    /// to be open.
    #[test]
    fn a_walk_moves_the_highlight_onto_the_piece_it_is_labeling() {
        use emerge_core::import::Candidate;
        use emerge_core::policy::Policy;

        let mesh = |pack: &str, name: &str| Candidate {
            mesh: format!("{pack}/{name}.glb"),
            proposed: Descriptor::default(),
            measured: None,
            front_detail: None,
            triangles: 0,
            findings: Vec::new(),
        };
        let mut state = ImportState::default();
        state.candidates = vec![mesh("alpha", "one"), mesh("beta", "two")];
        state.folded_packs.insert("beta".to_owned());
        // Where the author left the cursor: a library row, with the candidate list's own highlight
        // parked on a pack heading.
        state.selected = 0;
        state.selected_library_id = Some("lamp_tall".to_owned());
        state.focused_pack = Some("alpha".to_owned());

        state.focus_on(&EditTarget::Candidate("beta/two.glb".to_owned()));

        assert_eq!(state.selected, 1, "the highlight is on the piece being labeled");
        assert_eq!(
            state.selected_library_id, None,
            "a candidate takes the focus off the library list, or the pane keeps showing the row \
             that had it"
        );
        assert_eq!(
            state.focused_pack, None,
            "the highlight is on a mesh, not still on a heading"
        );
        // Asked of `pack_is_open`, which is the one place allowed to answer it — see
        // `every_list_follows_its_selection::what_is_on_screen_is_decided_in_one_place`.
        assert!(
            pack_is_open(&state, &Policy::default(), "beta"),
            "the pack it is in is opened, or the next frame moves the selection back off it"
        );
        let rows = candidate_list_rows(&state, &crate::filter::Filters::default(), &Policy::default());
        assert!(
            rows.contains(&ListRow::Mesh(1)),
            "and the row is genuinely on screen: {rows:?}"
        );

        // A library row is the same cursor in its other state.
        state.focus_on(&EditTarget::Library("lamp_tall".to_owned()));
        assert_eq!(state.selected_library_id.as_deref(), Some("lamp_tall"));

        // A piece that has gone since the walk was built leaves the highlight where it is, rather
        // than moving it somewhere arbitrary.
        state.focus_on(&EditTarget::Candidate("gamma/three.glb".to_owned()));
        assert_eq!(state.selected, 1);
        assert_eq!(state.selected_library_id.as_deref(), Some("lamp_tall"));
    }

    /// A piece named `id` whose mesh lives in `pack`.
    fn piece(id: &str, pack: &str) -> Descriptor {
        Descriptor {
            id: id.to_owned(),
            mesh: Some(format!("{pack}/{id}.glb")),
            ..Descriptor::default()
        }
    }

    /// **A synthetic project — no files, no meshes, no shipped corpus.**
    ///
    /// A test that read the real `assets/` would fail the day somebody imports a kit, and importing
    /// kits is the thing this editor exists to do. Everything the rule under test reads is plain
    /// data: the authoring kit's measurements.
    ///
    /// `library` is the **merged** view and holds `wall` as well, so the test can tell "in this kit"
    /// apart from "in the library" — which is exactly the distinction the rejected first rule missed.
    fn kit_holding(ids: &[&str]) -> Project {
        let all = [piece("wall", "alpha"), piece("crate", "beta"), piece("lamp", "beta")];
        let measured = Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: None,
            descriptors: all
                .iter()
                .filter(|d| ids.contains(&d.id.as_str()))
                .cloned()
                .collect(),
        };
        let library = Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: None,
            descriptors: all.to_vec(),
        };
        Project {
            root: std::path::PathBuf::from("/nonexistent"),
            emerge_dir: std::path::PathBuf::from("/nonexistent"),
            project_dir: std::path::PathBuf::from("/nonexistent"),
            maps_dir: std::path::PathBuf::from("/nonexistent/maps"),
            kits: Vec::new(),
            namespace: "nonexistent".to_owned(),
            library_path: std::path::PathBuf::from("/nonexistent/library.ron"),
            vocab: emerge_core::vocab::Vocabularies::default(),
            compositions: emerge_core::composition::Compositions::default(),
            library,
            measured,
            policy: emerge_core::policy::Policy::default(),
            lattice: emerge_core::kits::Lattice::default(),
            masks: Vec::new(),
            // **No map anywhere in this fixture**, which is the point: the rule under test is about
            // a kit, and the Kits door has no map to consult. See `project::OpenMap`.
            touched: Vec::new(),
            triangles: Vec::new(),
        }
    }

    /// **`Up` at the top of an open pack steps onto the headings above it.**
    ///
    /// The scenario the diagnostic caught, 2026-08-16, after two failed attempts at this bug. The
    /// log said it plainly: `selected=157 rows=91 folded=33 at=Some(0) first=Some(157)` — the cursor
    /// was at the top of the visible *mesh* rows and every candidate above it lived in one of 33
    /// collapsed packs. `Up` did nothing because, walking meshes only, there was nowhere to go —
    /// while 33 headings sat on screen above the cursor, reachable by mouse alone.
    ///
    /// So the fix was not "skip the collapsed folders": there was nothing beyond them to skip TO.
    /// It was to let the cursor stand on a heading, which makes every visible row reachable and lets
    /// `Enter` open one without the mouse.
    #[test]
    fn up_from_the_first_mesh_lands_on_the_heading_above_it() {
        use emerge_core::import::Candidate;
        use emerge_core::policy::Policy;

        let mesh = |pack: &str, name: &str| Candidate {
            mesh: format!("{pack}/{name}.glb"),
            proposed: emerge_core::descriptor::Descriptor::default(),
            measured: None,
            front_detail: None,
            triangles: 0,
            findings: Vec::new(),
        };
        let mut state = ImportState::default();
        state.candidates = vec![
            mesh("alpha", "one"),   // 0 — in a FOLDED pack, like 157's neighbours above it
            mesh("beta", "two"),    // 1 — the first open pack
            mesh("beta", "three"),  // 2
        ];
        state.folded_packs.insert("alpha".to_owned());
        let filters = crate::filter::Filters::default();
        let policy = Policy::default();

        // What is on screen: `alpha`'s heading (folded, no rows), `beta`'s heading, then its meshes.
        let rows = candidate_list_rows(&state, &filters, &policy);
        assert_eq!(
            rows,
            vec![
                ListRow::Header("alpha".to_owned()),
                ListRow::Header("beta".to_owned()),
                ListRow::Mesh(1),
                ListRow::Mesh(2),
            ],
            "a folded pack contributes its heading and no meshes"
        );

        // The cursor on the first mesh of the first OPEN pack — exactly where `Up` used to die.
        state.selected = 1;
        state.focused_pack = None;
        let at = rows
            .iter()
            .position(|r| *r == ListRow::Mesh(1))
            .unwrap_or_default();

        // Up once: onto `beta`'s own heading.
        put_cursor(&mut state, &rows[at.saturating_sub(1)]);
        assert_eq!(state.focused_pack.as_deref(), Some("beta"));

        // Up again: onto the FOLDED pack's heading — which is the row that used to be unreachable.
        put_cursor(&mut state, &rows[at.saturating_sub(2)]);
        assert_eq!(
            state.focused_pack.as_deref(),
            Some("alpha"),
            "the collapsed folder is reachable, which is what makes it openable from the keyboard"
        );

        // And the mesh cursor is not left behind pointing at something else.
        assert!(
            state.focused_pack.is_some(),
            "one cursor in two states — a heading and a mesh must never be highlighted at once"
        );
    }

    /// **The arrows jump over a collapsed group and land on the next open row.**
    ///
    /// The behaviour itself, not the predicate under it. Reported at the keyboard, 2026-08-16, after
    /// the predicate alone had been fixed: *"as soon as it hits a collapsed group, you can't go
    /// anymore… I want it to jump over the collapsed groups up to the next uncollapsed selection."*
    ///
    /// The remaining half was **order**: the walk stepped candidates by index while the list drew
    /// them grouped by pack, and excluded packs are drawn last however early their indices are. So
    /// the row after the last visible one before a fold was, by index, inside the fold. Walking the
    /// drawn order removes the question — a folded pack contributes no rows, so its neighbours are
    /// adjacent and there is no skipping logic to get wrong.
    #[test]
    fn the_arrows_step_over_a_collapsed_group() {
        use emerge_core::import::Candidate;
        use emerge_core::policy::Policy;

        // `Candidate` has no `Default` — it is what a measurement produces — so the fields the walk
        // never reads are given their empty values here.
        let mesh = |pack: &str, name: &str| Candidate {
            mesh: format!("{pack}/{name}.glb"),
            proposed: emerge_core::descriptor::Descriptor::default(),
            measured: None,
            front_detail: None,
            triangles: 0,
            findings: Vec::new(),
        };
        let mut state = ImportState::default();
        state.candidates = vec![
            mesh("alpha", "one"),   // 0
            mesh("beta", "two"),    // 1  <- beta gets folded
            mesh("beta", "three"),  // 2
            mesh("gamma", "four"),  // 3
        ];
        let filters = crate::filter::Filters::default();
        let policy = Policy::default();

        // Everything open: every row is walkable, in drawn order.
        assert_eq!(candidate_rows(&state, &filters, &policy), vec![0, 1, 2, 3]);

        // Fold `beta`: its two rows leave the walk entirely, so `alpha/one` and `gamma/four` become
        // neighbours — which IS the jump. One press moves 0 -> 3.
        state.folded_packs.insert("beta".to_owned());
        let rows = candidate_rows(&state, &filters, &policy);
        assert_eq!(rows, vec![0, 3], "a folded pack contributes no rows: {rows:?}");
        let at = rows.iter().position(|&i| i == 0).unwrap_or_default();
        assert_eq!(
            rows.get(at + 1).copied(),
            Some(3),
            "down from the row above a collapsed group lands below it, not inside it"
        );
        // And back up again, the same way.
        let at = rows.iter().position(|&i| i == 3).unwrap_or_default();
        assert_eq!(rows.get(at.saturating_sub(1)).copied(), Some(0));
    }

    /// **An excluded pack is drawn last, so it is walked last** — whatever its index.
    ///
    /// The half that made the ordering bug certain rather than merely likely: `beta` here sits in
    /// the middle by index and at the end on screen, so an index-order walk would have stepped into
    /// it from `alpha` while the eye was on `gamma`.
    #[test]
    fn the_walk_takes_the_excluded_group_last_and_only_when_it_is_open() {
        use emerge_core::import::Candidate;
        use emerge_core::policy::Policy;

        // `Candidate` has no `Default` — it is what a measurement produces — so the fields the walk
        // never reads are given their empty values here.
        let mesh = |pack: &str, name: &str| Candidate {
            mesh: format!("{pack}/{name}.glb"),
            proposed: emerge_core::descriptor::Descriptor::default(),
            measured: None,
            front_detail: None,
            triangles: 0,
            findings: Vec::new(),
        };
        let mut state = ImportState::default();
        state.candidates = vec![
            mesh("alpha", "one"),   // 0
            mesh("beta", "two"),    // 1  <- excluded, drawn last
            mesh("gamma", "three"), // 2
        ];
        let filters = crate::filter::Filters::default();
        let policy = Policy {
            exclude: vec!["beta".to_owned()],
            ..Policy::default()
        };

        // Group closed: it is not on screen, so it is not walked.
        assert_eq!(candidate_rows(&state, &filters, &policy), vec![0, 2]);

        // Group open: its rows join the walk, at the END — which is where they are drawn.
        state.excluded_open = true;
        assert_eq!(
            candidate_rows(&state, &filters, &policy),
            vec![0, 2, 1],
            "drawn order, not index order — this is the whole defect"
        );
    }

    /// **A folded pack's rows are not walked**, because they are not on screen.
    ///
    /// Reported at the keyboard, 2026-08-16: *"whenever I scroll up, it doesn't skip collapsed
    /// groups."* The arrows stepped `state.selected` through every candidate the filter kept, and
    /// folding was decided somewhere else entirely — so the highlight walked into packs nobody could
    /// see. The collapsed `EXCLUDED` group made it loud, everything in it being folded by
    /// construction.
    ///
    /// Worse than cosmetic, and the same argument the filter version already carries: **`Accept`
    /// acts on the selection**, so a walk that lands somewhere invisible can import a mesh the author
    /// never looked at.
    #[test]
    fn the_walk_skips_whatever_is_folded_away() {
        use emerge_core::policy::Policy;
        let excluded = Policy {
            exclude: vec!["characters".to_owned()],
            ..Policy::default()
        };
        let mut state = ImportState::default();

        // An ordinary pack, open.
        assert!(pack_is_open(&state, &Policy::default(), "ozea_kit"));
        // Folded by the author: its rows are not drawn, so they are not walked.
        state.folded_packs.insert("ozea_kit".to_owned());
        assert!(!pack_is_open(&state, &Policy::default(), "ozea_kit"));

        // Excluded, with the group closed: not drawn either.
        let mut state = ImportState::default();
        assert!(!pack_is_open(&state, &excluded, "characters"));
        // Excluded, with the group opened: drawn, so walkable — which is what makes `Shift+R`
        // reachable, since it acts on the pack of the highlighted mesh.
        state.excluded_open = true;
        assert!(pack_is_open(&state, &excluded, "characters"));
        // ...unless it is also folded inside the group.
        state.folded_packs.insert("characters".to_owned());
        assert!(!pack_is_open(&state, &excluded, "characters"));
    }

    /// **An id that would rename the whole kit is refused at the door.**
    ///
    /// `Library::namespace` is unanimous-or-refuse: one namespaced id among a kit's flat ones changes
    /// what the kit claims to implement, and `kits::bound_library` then refuses to open the project.
    /// So a single import can make a project unopenable — which is exactly what happened on
    /// 2026-08-16, when `low_poly_furniture/shower` (named after its pack folder) landed beside 387
    /// flat ids in a kit bound as `furniture`.
    ///
    /// The rule is the same shape as every other check on that door: refuse the keystroke that
    /// causes it, and name the fix, rather than let the next launch discover it.
    #[test]
    fn an_id_from_another_namespace_cannot_join_this_kit() {
        // The kit this is about: bound as `furniture`, ids flat.
        let bound = "furniture";
        for (id, ok) in [
            ("shower", true),
            ("furniture/shower", true),
            ("low_poly_furniture/shower", false),
            ("site/wall", false),
        ] {
            let agrees = match id.split_once('/') {
                None => true,
                Some((ns, _)) => ns == bound,
            };
            assert_eq!(
                agrees, ok,
                "`{id}` joining a kit bound as `{bound}`: the check is on the namespace the id \
                 declares, and a bare name declares none"
            );
        }
    }

    /// **An excluded pack leaves the ordinary list and joins one group at the bottom.**
    ///
    /// The partition is the whole feature, so it is asserted on the data rather than on the drawn
    /// rows: `draw_pack` is called for the offered packs in place, and for the excluded ones only
    /// under the `EXCLUDED` heading and only when it is open. Chosen at the keyboard, 2026-08-16 —
    /// one group at the end rather than a muted row left in place per pack.
    ///
    /// **Never dropped**, which is the property that matters: a mesh that silently disappeared looks
    /// identical to one the scan never found, and `Shift+R` needs a row to stand on to restore it.
    #[test]
    fn an_excluded_pack_leaves_the_list_for_the_group_and_is_still_reachable() {
        use emerge_core::policy::Policy;
        let policy = Policy {
            exclude: vec!["characters".to_owned()],
            ..Policy::default()
        };
        assert!(policy.excludes("characters/rig.glb"), "the pack is excluded");
        assert!(!policy.excludes("ozea_kit/crate.glb"), "and nothing else is");

        // The partition the list draws with — offered packs keep their place, excluded ones do not
        // vanish, they move.
        let packs = ["ozea_kit", "characters", "props"];
        let (offered, excluded): (Vec<&str>, Vec<&str>) =
            packs.iter().partition(|p| !policy.excludes(p));
        assert_eq!(offered, vec!["ozea_kit", "props"], "the list keeps what the kit uses");
        assert_eq!(excluded, vec!["characters"], "and the group holds the rest");
        assert_eq!(
            offered.len() + excluded.len(),
            packs.len(),
            "every pack is somewhere — the group is a fold, not a filter"
        );
    }

    /// **Open exactly the packs this kit draws on.** Two rules were tried before this one: library
    /// membership opened every pack in the merged library, and "what the open map places from" has
    /// no answer on a door with no map. The kit's own measurements are the level the door is about.
    #[test]
    fn a_pack_counts_as_in_use_only_when_the_kit_draws_on_it() {
        let used = packs_the_kit_draws_from(&kit_holding(&["crate"]));
        assert!(used.contains("beta"), "`beta` holds this kit's piece");
        assert!(
            !used.contains("alpha"),
            "`alpha` is in the merged library but not in the kit being authored"
        );
    }

    /// A kit with nothing in it draws on no pack — the state a fresh kit opens into.
    #[test]
    fn an_empty_kit_draws_on_no_pack_at_all() {
        assert!(packs_the_kit_draws_from(&kit_holding(&[])).is_empty());
    }

    /// One pack, two pieces, counted once — the rule is about directories, not rows.
    #[test]
    fn two_pieces_from_one_pack_name_it_once() {
        let used = packs_the_kit_draws_from(&kit_holding(&["crate", "lamp"]));
        assert_eq!(used.len(), 1);
        assert!(used.contains("beta"));
    }

    /// **An imported-but-unplaced mesh has its pack open**, which the map-based rule got wrong.
    ///
    /// This is the behaviour change the move buys, stated as a test rather than left implicit: the
    /// Kits door exists to bring meshes in and label them, and folding away the pack holding the
    /// mesh you just imported is the opposite of what that door is for.
    #[test]
    fn a_mesh_in_the_kit_but_on_no_map_still_opens_its_pack() {
        let project = kit_holding(&["crate"]);
        // No map exists in this fixture at all, which is the state the Kits door runs in.
        assert!(packs_the_kit_draws_from(&project).contains("beta"));
    }
}

pub(crate) fn scan(project: &Project, state: &mut ImportState) {
    let root = project.root.join("assets");
    match import::scan(&root, &root, &project.library) {
        Ok(found) => {
            // **On the first look, only the packs THIS MAP builds from are open.**
            //
            // This keyed off library membership, which is the wrong question by one step: a kit can
            // be fully imported and have nothing to do with the map open in front of you. The site
            // kit and the furniture kit are both in `library.ron`; a session spent on `site_67` wants
            // the one it is placing from, and a dozen open packs is an alphabet to scroll past on the
            // way to the work in progress.
            //
            // Only the FIRST scan seeds this — a rescan mid-session must not stomp the folds the
            // author has since toggled by hand.
            if !state.scanned {
                let used = packs_the_kit_draws_from(project);
                state.folded_packs = packs(&found)
                    .into_iter()
                    .map(|(pack, _)| pack)
                    .filter(|pack| !used.contains(pack.as_str()))
                    .collect();
            }
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
            // **The default selection must be a row the author can SEE.** Index 0 may sit inside a
            // folded pack — which put the highlight nowhere and a mesh on the stage that no
            // visible row claimed, reported as "it doesn't load the default selection". The head
            // of the first OPEN pack is the row the eye starts on; when every pack starts folded,
            // the first one opens to provide it — a tab must never open with its selection hidden.
            state.selected = 0;
            let grouped = packs(&state.candidates);
            // **Through `pack_is_open`, so "can the author see it" is one question.** This asked only
            // about `folded_packs` and knew nothing about exclusion — so once excluded packs went
            // into their own collapsed group, a fresh scan could open with the highlight inside it,
            // which is the exact thing the paragraph above forbids.
            match grouped
                .iter()
                .find(|(pack, _)| pack_is_open(&state, &project.policy, pack))
            {
                Some((_, members)) => state.selected = members.first().copied().unwrap_or(0),
                None => {
                    // Nothing is showing. Open the first pack this kit actually uses — never an
                    // excluded one, which the author took out on purpose and which cannot be
                    // imported anyway.
                    if let Some((pack, members)) = grouped
                        .iter()
                        .find(|(pack, _)| !project.policy.excludes(pack))
                        .or_else(|| grouped.first())
                    {
                        state.folded_packs.remove(pack);
                        state.selected = members.first().copied().unwrap_or(0);
                    }
                }
            }
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
    project: Res<Project>,
    mut state: ResMut<ImportState>,
) {
    if state.renaming.is_none() {
        if keys::just_pressed(&keyboard, *live, Action::TypeId) {
            if let Some(id) = state.selected_library_id.clone() {
                state.status.problem(format!(
                    "`{id}` is in the library — renaming it would strand every placement that names it"
                ));
            } else if let Some(mesh) = state.current().map(|c| c.mesh.clone()) {
                // The target, captured now. See `ImportState::renaming`.
                state.renaming = Some(Rename {
                    mesh,
                    raw: String::new(),
                });
                state
                    .status
                    .note("type an id — Enter to keep it, Esc to leave it alone".to_owned());
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
                    state
                        .status
                        .note("an id cannot be empty; nothing was changed".to_owned());
                } else {
                    // The candidate this field was opened on, found by the mesh it names rather than
                    // by wherever the selection has since moved to.
                    // Recorded like every other candidate edit — the snapshot history carries the
                    // candidate list precisely so pre-Accept work is not outside it.
                    let history_before = state.snapshot(&project);
                    match state.candidates.iter_mut().find(|c| c.mesh == rename.mesh) {
                        Some(c) => {
                            c.proposed.id = id.clone();
                            state.record(history_before);
                            state.status.note(format!("id is `{id}`"));
                        }
                        // Only reachable if a rescan dropped the mesh mid-rename, which is a real
                        // thing `R` can do. Saying so beats renaming whatever is selected now.
                        None => state.status.problem(format!(
                            "`{id}` was not kept — `{}` is no longer in the scan.",
                            rename.mesh
                        )),
                    }
                }
            }
            Key::Escape => {
                state.renaming = None;
                state.status.note("id unchanged".to_owned());
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
    if !keys::just_pressed(&keyboard, *live, Action::CycleMount) {
        return;
    }
    let surfaces: Vec<String> = project.vocab.surfaces.names().map(str::to_owned).collect();
    let options = mount_options(&surfaces);
    // Taken before the write — the only moment the old value still exists.
    let history_before = state.snapshot(&project);
    let Some((d, where_to)) = state.editing_mut(&mut project.measured) else {
        return;
    };
    // **An authored height survives the cycle.** Every entry in `options` is a literal, so landing on
    // a wall mount used to overwrite whatever height the piece had with the list's `1.8`. Tapping `M`
    // past a wall mount and back therefore silently discarded an authored number — the same class of
    // loss as `remeasure_rotated` zeroing `y_offset`s, which is a shipped bug this project has already
    // paid for once.
    //
    // Matched on the *current* mount rather than the list's, so the carry happens only when there is
    // genuinely a height to carry: floor -> wall gets the list's default, wall -> wall keeps yours.
    let had = d
        .mount
        .as_ref()
        .and_then(emerge_core::descriptor::mount_height);
    // **Found by kind, not by value.** `Mount` compares its payload too, so once a height is
    // authorable this lookup stops matching the moment it is anything but the list's `1.8` — and a
    // miss here reads as `map_or(0, ..)`, which would silently reset the piece to `on floor`. Asking
    // "does this option become the current mount if given the current height" is the same question
    // without the payload getting in the way.
    let next = d
        .mount
        .as_ref()
        .and_then(|m| {
            options.iter().position(|o| match had {
                Some(h) => emerge_core::descriptor::with_mount_height(o, h).as_ref() == Some(m),
                None => o == m,
            })
        })
        .map_or(0, |i| (i + 1) % options.len());
    let mut want = options[next].clone();
    if let Some(h) = had {
        if let Some(kept) = emerge_core::descriptor::with_mount_height(&want, h) {
            want = kept;
        }
    }
    d.mount = Some(want);
    let said = format!("mount: {}", mount_label(d.mount.as_ref()));
    state.record(history_before);
    state.status.say(persist(&mut project, where_to, said));
}

/// **`U` applies / `Y` discards the VLM's pending proposal** — the review verbs, through the
/// ordinary edit path: snapshot, mutate at the captured target, record, persist. For a library
/// entry that persist IS `commit_measured`, so the vocabulary gate rules on the exact bytes
/// written; for a candidate the change stays in memory until the author's Enter — the existing
/// doors, no new writer. The suggestion is consumed on success and kept when the write was
/// refused, so a `NOT WRITTEN` never eats a proposal.
fn suggestion_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
    mut suggestions: ResMut<crate::labels::Suggestions>,
    mut generation: ResMut<crate::labels::LabelGeneration>,
    mut label_queue: ResMut<crate::labels::LabelQueue>,
    mut label_tasks: ResMut<crate::labels::LabelTasks>,
    mut rig: ResMut<crate::label_booth::ShotRig>,
) {
    // `Shift+Y`: the whole labeler emptied at once — proposals, batch, booth queue, in-flight.
    if keys::just_pressed(&keyboard, *live, Action::DiscardAllSuggestions) {
        state.status.note(crate::labels::clear_all_labels(
            &mut suggestions,
            &mut generation,
            &mut label_queue,
            &mut label_tasks,
            &mut rig,
        ));
        return;
    }
    let apply = keys::just_pressed(&keyboard, *live, Action::ApplySuggestion);
    let discard = keys::just_pressed(&keyboard, *live, Action::DiscardSuggestion);
    if !apply && !discard {
        return;
    }
    let Some(target) = state.target() else {
        state.status.note("nothing focused".to_owned());
        return;
    };
    let name = crate::labels::name_of(&target).to_owned();
    if discard {
        if suggestions.remove(&target).is_some() {
            generation.0 = generation.0.wrapping_add(1);
            state
                .status
                .note(format!("discarded the proposed labels for `{name}`"));
        } else {
            state.status.note("no proposed labels here".to_owned());
        }
        return;
    }
    apply_suggestion(
        target,
        &name,
        &mut project,
        &mut state,
        &mut suggestions,
        &mut generation,
        &label_tasks,
        &mut rig,
    );
}

/// **A batch confirms what it proposes, one per frame.**
///
/// The commit door still exists and still guards the single `L`: one mesh is a decision an author
/// is standing in front of. A walk of hundreds is a different act — asked for at the keyboard,
/// 2026-08-15 — and the confirmation is the decision to start it. Everything downstream is the same
/// path `U` takes, including the guards and the righting branch, so a batch cannot write something
/// a keypress would have refused.
///
/// One per frame rather than draining: `apply_suggestion` may re-photograph a piece it had to right
/// first, and a loop here would queue those shots faster than the booth can take them.
fn auto_apply_batch(
    queue: Res<crate::labels::LabelQueue>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
    mut suggestions: ResMut<crate::labels::Suggestions>,
    mut generation: ResMut<crate::labels::LabelGeneration>,
    label_tasks: Res<crate::labels::LabelTasks>,
    mut rig: ResMut<crate::label_booth::ShotRig>,
) {
    if !queue.auto_apply() || queue.paused() {
        return;
    }
    let Some(target) = suggestions.first_target() else {
        return;
    };
    let name = crate::labels::name_of(&target).to_owned();
    apply_suggestion(
        target,
        &name,
        &mut project,
        &mut state,
        &mut suggestions,
        &mut generation,
        &label_tasks,
        &mut rig,
    );
}

/// **How many times one mesh may be turned by the labeler before the loop is called a loop.**
///
/// Two, because one is the answer and the second is the correction: an odd turn taken the wrong way
/// round comes back as "still not upright" and the second attempt fixes it (see
/// [`crate::vlm::NeedsTurn::turns`] for why the direction is not asked for). A third means the model
/// and the mesh disagree about which way is up, and four quarter turns is where it started.
pub(crate) const MAX_RIGHTINGS: u8 = 2;

/// **Apply one proposal, wherever the decision came from.**
///
/// `U` is one caller; a batch running with auto-confirm is the other. Extracted rather than
/// duplicated because the interesting part is not the field copy — it is the two guards around it
/// (the piece may have gone, the mesh may have changed under the proposal) and the righting branch,
/// and a second copy of those is a second set of rules to keep in step.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_suggestion(
    target: EditTarget,
    name: &str,
    project: &mut Project,
    state: &mut ImportState,
    suggestions: &mut crate::labels::Suggestions,
    generation: &mut crate::labels::LabelGeneration,
    label_tasks: &crate::labels::LabelTasks,
    rig: &mut crate::label_booth::ShotRig,
) {
    let Some(entry) = suggestions.get(&target).cloned() else {
        state.status.note("no proposed labels here".to_owned());
        return;
    };
    // **A lying-down piece is righted FIRST, and the labels are re-asked** — the model judged a
    // sideways render, so its `front` (and the footprint it was told) describe the wrong
    // orientation; applying them would bake the error in. `U` therefore performs the quarter
    // turn through the same `rotate_mesh` the N/P keys run (its own undo entry, its own
    // authored-cells guard — a refusal keeps the suggestion and says why), discards the stale
    // suggestion, and re-photographs the upright piece for fresh labels.
    if let Some(turn) = &entry.suggestion.needs_turn {
        let axis = if turn.axis == "x" {
            RotateAxis::X
        } else {
            RotateAxis::Z
        };
        // **The turn is a write, and `rotate_mesh` writes to the FOCUSED piece** — so the focus has
        // to be the target before it runs, and it was not. This function is handed an explicit
        // target (that is the whole point of `at_target` below), while `rotate_mesh` reads
        // `ImportState::editing` because its other callers are the N/P keys, where the focus IS the
        // subject. A batch that met a lying-down mesh therefore turned whichever row the author had
        // left highlighted — and wrote the file if that row was a library entry. Same shape as the
        // bug `at_target` exists for, one function further along.
        state.focus_on(&target);
        let attempt = {
            let n = state.righted.entry(entry.mesh.clone()).or_insert(0);
            *n += 1;
            *n
        };
        // **A candidate's lattice is the scan's output, so the turn may clear it and re-derive it;
        // a library row's may be hand-authored, so it still refuses.**
        //
        // `autoscan_candidate` marks a candidate's cells the moment it is selected — and since the
        // walk now selects what it labels, EVERY candidate the labeler reaches has a lattice by the
        // time its answer comes back. `rotate_mesh` refuses an X or Z turn over authored cells, so
        // without this the righting would refuse every candidate it was asked about: the feature
        // would exist and never fire. The distinction is the one `autoscan_candidate` already draws
        // for exactly this reason — nothing a candidate holds has reached disk, and undo covers the
        // turn either way.
        let derived_lattice = matches!(target, EditTarget::Candidate(_));
        let turned = attempt <= MAX_RIGHTINGS
            && rotate_mesh(axis, turn.turns, derived_lattice, project, state);
        if turned {
            if derived_lattice {
                // **Re-derived in the frame the piece is now in.** The turn cleared the lattice, and
                // leaving it cleared would be a silent state change a moment after the scan that
                // filled it — the same argument that makes turning and re-measuring one action.
                let _ = scan_mesh(project, state);
            }
            suggestions.remove(&target);
            generation.0 = generation.0.wrapping_add(1);
            let Some(d) = state.placed_at_target(&target, &project).cloned() else {
                return;
            };
            let said = crate::labels::request_photos(target, &d, label_tasks, rig);
            state.status.note(format!(
                "righted `{name}` {} quarter turn(s) about {} — {said}",
                turn.turns,
                turn.axis.to_uppercase()
            ));
            return;
        }
        // **A righting that cannot happen drops the proposal**, and that is not tidiness: a refusal
        // used to leave the entry staged, and `auto_apply_batch` reaches for the first staged entry
        // every frame — so a piece that could not be turned was retried sixty times a second,
        // rewriting the status line, for the rest of the session. Dropping it terminates, and the
        // labels are not applied because they were judged from an orientation the piece should not
        // have been in.
        suggestions.remove(&target);
        generation.0 = generation.0.wrapping_add(1);
        let why = if attempt > MAX_RIGHTINGS {
            format!(
                "`{name}` has been righted {MAX_RIGHTINGS} times and still says it is not upright \
                 — turning it again would be a loop. Turn it by hand with N/P and press L."
            )
        } else {
            format!(
                "`{name}` could not be turned, so its labels were judged from the wrong \
                 orientation and have been dropped — the refusal above says why. Turn it by hand \
                 with N/P and press L."
            )
        };
        warn!("{why}");
        state.status.problem(why);
        return;
    }
    let history_before = state.snapshot(project);
    // **Read before the borrow.** `apply_fields` settles the derived half of `effects` and needs the
    // axis in vocabulary order to do it, and that order cannot be read out of `project` while
    // `project.measured` is mutably borrowed below. Same shape as `on_tag_chip`.
    let effects_order: Vec<String> = project.vocab.effects.names().map(str::to_owned).collect();
    let Some((d, where_to)) = state.at_target(&target, &mut project.measured) else {
        state
            .status
            .problem("not applied — that piece is gone".to_owned());
        return;
    };
    if d.mesh.as_deref() != Some(entry.mesh.as_str()) {
        state
            .status
            .problem("not applied — the mesh changed under the proposal".to_owned());
        return;
    }
    crate::labels::apply_fields(d, &entry.suggestion, &effects_order);
    state.record(history_before);
    let said = persist(
        project,
        where_to,
        format!("applied proposed labels to `{name}`"),
    );
    // A refused write keeps the proposal staged for another try. This used to ask
    // `said.starts_with("NOT WRITTEN")` — a control-flow decision made by sniffing a message for a
    // prefix the function it came from was free to reword. `persist` returns a `Result` now, so the
    // question is asked of the type.
    if said.is_ok() {
        suggestions.remove(&target);
        generation.0 = generation.0.wrapping_add(1);
    }
    state.status.say(said);
}

/// **Commit the focused tile** — add it if it is a candidate, update it if it is already a tile.
///
/// Validated first, and refused rather than repaired: a descriptor that fails the vocabulary is one
/// an author has not finished, and writing a broken entry would make the next `Library::parse` fail
/// for everyone rather than for the person who caused it.
///
/// The library is written immediately. An importer that batches its additions until some later save
/// is one where a crash loses work an author believes they did — and the file is generated from the
/// manifests today, so an unwritten addition would simply be regenerated away.
///
/// # Enter on a library entry is an update, and it used to be a refusal
///
/// It answered *"`{id}` is already in the library — pick a candidate below to add one"*, which is a
/// true sentence that lands as a false one. Every field on this pane writes through [`persist`] the
/// moment it changes, so an author who edits a tile and then reaches for save has *already* saved —
/// and was being told, at the exact moment they asked, that their piece was not going in. The verb
/// they pressed is "commit this tile"; the two destinations are the two states a tile can be in, not
/// two different verbs, so this runs the same door again and reports what it did.
///
/// Re-running it is not a no-op dressed as one. [`commit_measured`] re-applies the policy, re-checks
/// the lattices against the face bands and re-resolves the masks *before* it writes, so Enter is
/// where an author finds out that the tile they have been editing still holds together — the one
/// question the incremental writes answer piecemeal and never as a whole.
///
/// **A candidate whose id collides is still refused**, and that asymmetry is the point. Writing a
/// freshly-measured candidate over a library entry is a *replace*: the entry's tags, note, mount and
/// lattice are not in the candidate, so the write would take them out. The route to changing a tile
/// is to edit the tile.
fn commit_candidate(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
    queue: Res<crate::labels::LabelQueue>,
) {
    let accept = keys::just_pressed(&keyboard, *live, Action::Accept);
    // **Space is the heading key, and only the heading key.** It was unbound on this tab, so
    // pressing it on a collapsed pack did nothing — reported at the keyboard 2026-08-18. It joins
    // `Enter` on a heading and is deliberately inert everywhere else, which is why it is its own
    // action: a second key on `Accept` would have made Space commit a tile.
    let fold = keys::just_pressed(&keyboard, *live, Action::FoldPack);
    if !accept && !fold {
        return;
    }
    // **`Enter` acts on the row the cursor is on.** On a pack heading that means opening or closing
    // it — the arrows can stand on a heading now, and a key that did nothing there would be the
    // reason to reach for the mouse. Same rule the chooser's settings panel follows: one key, one
    // meaning, down the whole list.
    if let Some(pack) = state.focused_pack.clone() {
        // **The toggle already knows the answer**, so it is not asked again: `remove` reports
        // whether it was folded, which is exactly whether this press opened it. Re-reading
        // `folded_packs` here would be a second reader of the visibility rule, which is the thing
        // `every_list_follows_its_selection.rs` exists to stop — and it caught this.
        let opened = state.folded_packs.remove(&pack);
        if !opened {
            state.folded_packs.insert(pack.clone());
        }
        state.status.note(format!(
            "`{pack}` {}",
            if opened { "opened" } else { "closed" }
        ));
        return;
    }
    // **Off a heading, Space has nothing to say.** Everything below commits a tile, and that is
    // `Enter`'s verb alone — see the binding's own note.
    if !accept {
        return;
    }
    // **A question owns `Enter` while it is up.** `labels::answer_overwrite` reads the same key in
    // the same phase, and reading a key does not consume it — so without this the `Enter` that
    // answers "re-label everything" would also add the highlighted candidate to the library. That is
    // the `xseam` shape (`keys.rs`), which put six descriptors in `library.ron` the first time.
    if queue.ask.is_some() {
        return;
    }
    if let Some(id) = state.selected_library_id.clone() {
        match write_library(&mut project) {
            Ok(path) => {
                state.status.note(format!(
                    "`{id}` is up to date — every edit on this tab is written as you make it"
                ));
                info!("re-wrote `{id}` to {}", path.display());
            }
            Err(e) => {
                // The edits are in memory and the file is not, which is the one case where an
                // author has to be told to fix something before they leave the tab.
                state.status.problem(format!("`{id}` NOT WRITTEN: {e}"));
                error!("{e}");
            }
        }
        return;
    }
    let Some(candidate) = state.current().cloned() else {
        return;
    };
    if candidate.blocked() {
        state
            .status
            .problem("this mesh cannot be measured, so there is nothing to add".to_owned());
        return;
    }
    // **A mesh this kit excludes cannot be imported into it.**
    //
    // `Policy::exclude` meant two small things and not the one it says: the label batch skipped it
    // and the pack row drew it greyed. Nothing stopped `Enter` putting it in the library anyway, so
    // an excluded character rig could still reach a map — which is what an author excludes it to
    // prevent. Asked for at the keyboard, 2026-08-16: *"how can I mark them as not being imported in
    // the kits section so that they don't show up in the maps?"*
    //
    // Refused rather than hidden: the row stays on the list saying what it is, which is the same
    // draw-the-constraint rule the kit ticks follow (Vicente & Rasmussen, `10.1109/21.156574`). A
    // mesh that vanished would read as a scan that missed it.
    if project.policy.excludes(&candidate.mesh) {
        state.status.problem(format!(
            "`{}` is excluded from this kit, so it cannot be imported into it. Shift+R on its \
             pack puts the pack back.",
            leaf(&candidate.mesh)
        ));
        return;
    }
    let descriptor = candidate.proposed.clone();
    if descriptor.id.trim().is_empty() {
        state.status.note("give it an id first (I)".to_owned());
        return;
    }
    // **A new id has to agree with the kit it is joining.**
    //
    // `Library::namespace` is unanimous-or-refuse, so ONE namespaced id among a kit's flat ones
    // changes what the whole kit claims to be — and `kits::bound_library` then refuses to open the
    // project at all. That is not a hypothetical: on 2026-08-16 an import named `low_poly_furniture/
    // shower` after its pack folder, landed it beside 387 flat ids in a kit bound as `furniture`,
    // and the next launch could not open the project. Nothing checked, because the commit door asked
    // about duplicates and emptiness and never about the namespace.
    //
    // Refused here rather than at the next open, which is the whole point of a commit door: the
    // failure belongs to the keystroke that caused it, not to a launch tomorrow.
    if let Some((ns, _)) = descriptor.id.split_once('/')
        && ns != project.namespace
    {
        state.status.problem(format!(
            "`{}` would join this kit as `{ns}/*`, but it is bound as `{}`. Name it `{}` — or \
             `{}/{}` if you mean to qualify it.",
            descriptor.id,
            project.namespace,
            descriptor.id.rsplit('/').next().unwrap_or(&descriptor.id),
            project.namespace,
            descriptor.id.rsplit('/').next().unwrap_or(&descriptor.id),
        ));
        return;
    }
    if project.library.get(&descriptor.id).is_some() {
        // **Two routes, and they are not interchangeable.** Editing the tile above updates it;
        // accepting this candidate over it would replace it, and a candidate is what a mesh scan
        // can see — no tags, no note, no mount, no lattice — so the replace silently takes those
        // out. The refusal names the one that keeps them.
        state.status.note(format!(
            "`{}` is already in the library — select it above to edit that tile, or rename this \
             candidate (I). Accepting it here would replace the tile and take its tags and lattice \
             with it.",
            descriptor.id
        ));
        return;
    }

    // **Into `measured`** — the layer that is written and the layer an import belongs in: what a mesh
    // scan produces is a measurement, and the project's architecture is layered over it afterwards.
    //
    // A proposal, not a mutation. `commit_measured` layers the policy over it and runs the two-sided
    // surface check on the result, which is the right shape for that check: it is about the finished
    // set, so a piece that offers `worktop` makes another piece's `on worktop` legal, and checking it
    // in isolation would reject the pair that fixes each other.
    // `trial` is a clone, so `project.measured` is still the pre-edit state here.
    let before = state.snapshot(&project);
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
            state.record(before);
            state.status.note(format!(
                "added `{}` — it is in the palette now",
                descriptor.id
            ));
            info!("added `{}` to {}", descriptor.id, path.display());
        }
        Err(e) => {
            // Nothing was added and nothing was written — `commit_measured` refuses before it
            // touches the disk — so this says the one thing that is true of both.
            state.status.problem(format!("not added: {e}"));
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
    if !keys::just_pressed(&keyboard, *live, Action::RemoveTile) {
        return;
    }
    let Some(id) = state.selected_library_id.clone() else {
        state
            .status
            .note("select a library tile to remove it".to_owned());
        return;
    };
    let before = state.snapshot(&project);
    match take_out_of_library(&id, &mut project) {
        Ok(path) => {
            state.selected_library_id = None;
            state.record(before);
            state
                .status
                .note(format!("removed `{id}` from the library"));
            info!("removed `{id}` from {}", path.display());
        }
        Err(e) => state.status.problem(format!("not removed: {e}")),
    }
}

/// The shared core of Delete and Shift+Delete: take `id` out of the measured layer, through the one
/// write door.
///
/// It refuses while the open map still places the piece. An orphaned placement is not an error the
/// map can carry — it names a descriptor nothing defines, so the piece silently fails to appear and
/// the author finds out by counting crates. Saying "12 placements use this" is the answer; deleting
/// them on their behalf is not.
///
/// **Out of `measured`**, for the reason `commit_candidate` writes into it: the derived layer is
/// rebuilt from the measurements on every write, so removing a piece from it removed nothing. Ids
/// survive layering — `Policy::apply` patches entries, it does not rename them — so the piece named
/// by the palette is the piece named here. `commit_measured` re-validates, which is what catches the
/// interesting failure: removing a piece can strand another that rested on a surface only it
/// offered, and it can leave a policy patch matching nothing.
fn take_out_of_library(id: &str, project: &mut Project) -> Result<std::path::PathBuf, String> {
    // **Every map in the project, not the one that happened to be open.** See
    // `Project::maps_that_place` for what the narrow version let through.
    let used = project.maps_that_place(id)?;
    if !used.is_empty() {
        return Err(format!(
            "`{id}` is placed by {} — remove those placements first",
            used.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    // **Compositions are the second referrer of a descriptor id, and they are stricter than a map.**
    // `policy::layered_library` hard-refuses a composition whose member descriptor is missing, so
    // writing this file without the entry does not produce a map with a hole — it produces a project
    // that neither the editor nor the game can open at all. The map check above exists for exactly
    // this reason and had one list to look at when it was written; it now has two.
    let groups: Vec<&str> = project
        .compositions
        .compositions
        .iter()
        .filter(|c| {
            c.members.iter().any(|m| {
                matches!(&m.body, emerge_core::composition::Body::Descriptor { id: d, .. } if d == id)
            })
        })
        .map(|c| c.id.as_str())
        .collect();
    if !groups.is_empty() {
        // **Name the verb that DOES work.** This used to end "edit the group first", which is true
        // and unhelpful: an author reading it while pointing at `site/floor` — held by all four site
        // tiles — sees a refusal with no next step and reports the feature as broken. Twice. Sending
        // a piece back STRIPS it; editing it does not, and editing is almost always what was wanted.
        return Err(format!(
            "`{id}` is a member of {}: {}. It cannot be sent back while they hold it — the project \
             would stop opening with `compositions.ron` naming a descriptor nothing defines. \
             {} edits it in place without removing it; sending it back means redefining {} first.",
            if groups.len() == 1 {
                "the composition"
            } else {
                "the compositions"
            },
            groups.join(", "),
            crate::keys::chord(crate::keys::Action::EditTile),
            if groups.len() == 1 {
                "that composition"
            } else {
                "those compositions"
            },
        ));
    }
    let Some(at) = project.measured.descriptors.iter().position(|d| d.id == id) else {
        return Err(format!("`{id}` is not in the measured layer"));
    };
    let mut trial = project.measured.clone();
    trial.descriptors.remove(at);
    commit_measured(project, trial)
}

/// **What would stop `id` being sent back to the candidates**, and the mesh it would come back as.
///
/// Extracted because two verbs now ask it. The Tiles tab asks before demoting; the **Map** asks
/// before it deletes anything, and that ordering is the whole reason this is a function: a blocker
/// discovered *after* placements were gone would be a destructive half-act with nothing to show
/// for it.
///
/// It deliberately does **not** check the placement count. That is the one precondition the Map verb
/// exists to clear, and `take_out_of_library` still enforces it at the door for every caller.
pub(crate) fn demote_blockers(id: &str, project: &Project) -> Result<String, String> {
    // An entry with no mesh has nothing to come back as; sending it "back" would just be Delete
    // wearing a costume, so it refuses and names the honest key.
    let Some(mesh) = project
        .measured
        .descriptors
        .iter()
        .find(|d| d.id == id)
        .and_then(|d| d.mesh.clone())
    else {
        return Err(format!(
            "`{id}` has no mesh — nothing to send back ({} removes it outright)",
            crate::keys::REMOVE_NAME
        ));
    };
    // **Compositions are the second referrer of a descriptor id, and they are stricter than a map.**
    // `policy::layered_library` hard-refuses a composition whose member descriptor is missing, so a
    // library written without the entry does not give a map with a hole — it gives a project that
    // neither the editor nor the game can open.
    let groups: Vec<&str> = project
        .compositions
        .compositions
        .iter()
        .filter(|c| {
            c.members.iter().any(|m| {
                matches!(&m.body, emerge_core::composition::Body::Descriptor { id: d, .. } if d == id)
            })
        })
        .map(|c| c.id.as_str())
        .collect();
    if !groups.is_empty() {
        // **Name the verb that DOES work.** This used to end "edit the group first", which is true
        // and unhelpful: an author reading it while pointing at `site/floor` — held by all four site
        // tiles — sees a refusal with no next step and reports the feature as broken. Twice. Sending
        // a piece back STRIPS it; editing it does not, and editing is almost always what was wanted.
        return Err(format!(
            "`{id}` is a member of {}: {}. It cannot be sent back while they hold it — the project \
             would stop opening with `compositions.ron` naming a descriptor nothing defines. \
             {} edits it in place without removing it; sending it back means redefining {} first.",
            if groups.len() == 1 {
                "the composition"
            } else {
                "the compositions"
            },
            groups.join(", "),
            crate::keys::chord(crate::keys::Action::EditTile),
            if groups.len() == 1 {
                "that composition"
            } else {
                "those compositions"
            },
        ));
    }
    Ok(mesh)
}

/// Which library entry `Shift+Delete` has armed for demotion — the first press's answer, waiting
/// for the second.
#[derive(Resource, Default)]
struct DemoteArm(Option<String>);

/// **Send a library entry back to the candidate list, stripped.**
///
/// "Redo this one from scratch" is a different intent from Delete's "this was a mistake": the piece
/// leaves the library through the same door, but its GLB is still on disk, so the rescan measures it
/// fresh and it re-enters the candidate list carrying exactly what the importer can see — footprint
/// and height, no tags, no note, no mount.
///
/// Two presses, because the strip is the point and the point is destructive: the first press arms
/// with a warning naming what is lost, the second press on the same piece sends it. Moving the
/// focus disarms (`disarm_demote`) — the confirmation is for THIS piece, not whichever one is
/// focused when the key lands next. One undo step covers the whole trip, `Snapshot` holding both
/// halves by design.
fn demote_tile(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
    mut arm: ResMut<DemoteArm>,
    mut suggestions: ResMut<crate::labels::Suggestions>,
    mut generation: ResMut<crate::labels::LabelGeneration>,
) {
    if !keys::just_pressed(&keyboard, *live, Action::DemoteTile) {
        return;
    }
    let Some(id) = state.selected_library_id.clone() else {
        state
            .status
            .note("select a library tile to send it back".to_owned());
        return;
    };
    if arm.0.as_deref() != Some(id.as_str()) {
        arm.0 = Some(id.clone());
        state.status.problem(format!(
            "sending `{id}` back to the candidates loses its tags, note and mount — \
             Shift+{} again sends it",
            crate::keys::REMOVE_NAME
        ));
        return;
    }
    arm.0 = None;
    // The mesh path is the reborn candidate's name — captured before the entry is gone, through the
    // same question the Map asks before it deletes anything on this piece's behalf.
    let mesh = match demote_blockers(&id, &project) {
        Ok(mesh) => mesh,
        Err(e) => return state.status.problem(e),
    };
    let before = state.snapshot(&project);
    match take_out_of_library(&id, &mut project) {
        Ok(path) => {
            // The rescan IS the strip: `import::scan` measures the GLB fresh, so the candidate
            // re-enters with the importer's facts and none of the author's judgements.
            scan(&project, &mut state);
            if let Some(at) = state.candidates.iter().position(|c| c.mesh == mesh) {
                state.selected = at;
            }
            state.selected_library_id = None;
            // Proposed labels under either identity judged the piece as it was configured —
            // stale by definition now.
            let dropped = suggestions
                .remove(&EditTarget::Library(id.clone()))
                .is_some()
                | suggestions
                    .remove(&EditTarget::Candidate(mesh.clone()))
                    .is_some();
            if dropped {
                generation.0 = generation.0.wrapping_add(1);
            }
            state.record(before);
            state.status.note(format!(
                "sent `{id}` back to the candidates — measured fresh, stripped"
            ));
            info!("demoted `{id}` out of {}", path.display());
        }
        Err(e) => state.status.problem(format!("not sent back: {e}")),
    }
}

/// The arm is for ONE piece: focus moving off it disarms, so a `Shift+Delete` aimed at the lamp can
/// never fire the confirmation the sofa armed. Silent — writing a status here would itself change
/// `ImportState` and re-run the gate.
fn disarm_demote(state: Res<ImportState>, mut arm: ResMut<DemoteArm>) {
    let Some(armed) = arm.0.clone() else {
        return;
    };
    if state.selected_library_id.as_deref() != Some(armed.as_str()) {
        arm.0 = None;
    }
}

/// Does anything under this entity carry a drawable mesh? The watchdog's one question.
fn holds_a_mesh(
    root: Entity,
    children: &Query<&Children>,
    meshes: &Query<(), With<Mesh3d>>,
) -> bool {
    if meshes.get(root).is_ok() {
        return true;
    }
    children
        .get(root)
        .map(|kids| kids.iter().any(|k| holds_a_mesh(k, children, meshes)))
        .unwrap_or(false)
}

/// Toggle one token on one axis.
/// **`Option<ResMut<Project>>`, because this is a GLOBAL observer** — see [`on_cell_verb`].
///
/// This is the sixth, and it was found by fixing the *scanner* rather than by reading the code: the
/// rule that catches these was truncating `tiles.rs` at its first `#[cfg(test)]` module, about a
/// third of the way in, so everything past line ~1900 was reporting green over nothing. This one
/// panicked at runtime during FVS-S-34a and was side-stepped by keeping the menu off the `Activate`
/// bus; it was never actually fixed, and it stayed a live crash for any other caller.
fn on_tag_chip(
    activate: On<Activate>,
    chips: Query<&TagChip>,
    project: Option<ResMut<Project>>,
    mut state: ResMut<ImportState>,
) {
    let Ok(chip) = chips.get(activate.entity) else {
        return;
    };
    let Some(mut project) = project else { return };
    // **Both the token and the vocabulary order come out first, owned.** What follows needs a mutable
    // borrow of the same `Project`, and the sort key cannot be read from it while that is held.
    let (token, order, effects_order) = {
        let names: Vec<String> = chip
            .axis
            .tokens(&project.vocab)
            .names()
            .map(str::to_owned)
            .collect();
        // The DOES axis in vocabulary order, for the settle below. Read here for the same reason
        // everything else in this block is: the write needs `project` mutably.
        let effects: Vec<String> = project.vocab.effects.names().map(str::to_owned).collect();
        match names.get(chip.token) {
            Some(t) => (t.clone(), names, effects),
            None => return,
        }
    };
    // **Through the focus, like every other mutator.** This wrote to `candidates[selected]` while the
    // pane had already been switched to read whatever the focus points at, so clicking a tag on a
    // library tile edited an invisible candidate: the chip did not light, the descriptor did not
    // change, and nothing reached disk. The one accessor exists so this cannot happen; missing it
    // here is what it looks like when it does.
    // Taken before the write — the only moment the old value still exists.
    let history_before = state.snapshot(&project);
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
    // **A kind typed by hand implies the same effects a labelled one does.** The rule is about the
    // word, not about who wrote it — so `uses-electricity` follows a chip click exactly as it
    // follows a proposal, and stops following when the kind is clicked off again.
    if chip.axis == Axis::Kind {
        crate::labels::settle_implied_effects(d, &effects_order);
    }
    let said = format!("{} tags updated", chip.axis.label().to_lowercase());
    state.record(history_before);
    state.status.say(persist(&mut project, where_to, said));
}

// **No `Res<Build>` any more.** This system used to read `build.placing` to decide whether the arrows
// were its business; the census answers that now, so the dependency went with the guard.
fn move_selection(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    time: Res<Time>,
    mut repeat: ResMut<crate::keys::Repeat>,
    project: Res<Project>,
    // The arrows walk what is on screen, which is what the filter decides.
    filters: Res<crate::filter::Filters>,
    mut state: ResMut<ImportState>,
    // A mesh whose judgement is still a proposal is not composable — see `composable`.
    suggestions: Res<crate::labels::Suggestions>,
) {
    // **Read the keys before touching the focus.** Clearing `selected_library_id` unconditionally
    // would steal the focus back on the very next frame after a library row was clicked — the system
    // runs every frame, not only when an arrow arrives.
    // Held arrows repeat at the shared [`crate::keys::REPEAT_SECS`] cadence, like the aim keys —
    // walking a 300-candidate scan one tap at a time is not a job.
    let dt = time.delta_secs();
    // **Which pair of arrows, chosen by tab.** One `repeating` call per direction, not two OR'd
    // together: `Repeat` carries a single countdown, so asking it about two actions in one frame
    // would have the second reset the first's cadence.
    //
    // **And on the Tiles tab only while nothing is in hand** — which this no longer has to check.
    // `TileListPrev`/`TileListNext` declare `Stance::Idle`, so taking a piece with `Space` stops them
    // firing at the census rather than here. The early return that used to live here was the second
    // census `keys.rs` exists to prevent: a rule about when a key fires, written somewhere a reader
    // of the key table could not see it.
    let (prev, next) = if live.0 == crate::keys::Context::Tiles {
        (Action::TileListPrev, Action::TileListNext)
    } else {
        (Action::PrevCandidate, Action::NextCandidate)
    };
    let down = keys::repeating(&keyboard, *live, next, &mut repeat, dt);
    let up = keys::repeating(&keyboard, *live, prev, &mut repeat, dt);
    let to_library = keys::just_pressed(&keyboard, *live, Action::FocusLibrary);
    let to_candidates = keys::just_pressed(&keyboard, *live, Action::FocusCandidates);

    // Left/right choose which list the arrows walk. `selected_library_id` is already the one
    // discriminant the detail pane reads, so this sets it and everything else follows.
    if to_candidates {
        state.selected_library_id = None;
        state.status.note("candidates".to_owned());
    }
    if to_library {
        match library_ids(
            project.as_ref(),
            &filters,
            live.0 == crate::keys::Context::Tiles,
            Some(&suggestions),
        )
        .first()
        {
            Some(first) => {
                if state.selected_library_id.is_none() {
                    state.selected_library_id = Some(first.clone());
                }
                // **The heading cursor goes when the library takes over.** Leaving it set meant two
                // highlights on screen and two readers disagreeing about which was live.
                state.focused_pack = None;
                state.status.note("library".to_owned());
            }
            None => state.status.note("the library is empty".to_owned()),
        }
    }
    if !down && !up {
        return;
    }

    // **On the Tiles tab there is only one list.** A tile member must name a `library.ron`
    // descriptor; a candidate is a mesh that has been measured and not imported, so it is not a
    // legal source and walking it here would move a focus the tile author cannot spend. This is one
    // path rather than two — the tab does not *prefer* the library, it is the only list it has.
    if live.0 == crate::keys::Context::Tiles && state.selected_library_id.is_none() {
        match library_ids(
            project.as_ref(),
            &filters,
            live.0 == crate::keys::Context::Tiles,
            Some(&suggestions),
        )
        .first()
        {
            // **The press that establishes the selection lands ON the first row**, and stops there.
            //
            // It used to seed row 0 and then fall through into the walk below, which read `at = 0`
            // and stepped to row 1 — so the first `down` an author pressed on this tab skipped the
            // first piece entirely, and the only way to reach it was to press `up` afterwards. One
            // key press did two things, which is the same shape as every other list defect this
            // file has produced: a seed is not a step.
            //
            // Found on 2026-08-16 through `the_tile_feedback_script_can_actually_be_followed`,
            // which had been passing for the wrong reason and started measuring this the moment a
            // candidate's proposed id stopped colliding with a library one.
            Some(first) => {
                state.selected_library_id = Some(first.clone());
                state.status.note(format!(
                    "`{first}` selected — {} removes it",
                    keys::binding(Action::RemoveTile).chord
                ));
                return;
            }
            None => {
                state.status.problem(
                    "the library is empty — import a mesh on the Meshes tab before building"
                        .to_owned(),
                );
                return;
            }
        }
    }
    // Shift is the long stride: five rows per step, same key, same direction — a 300-candidate
    // scan at one row a step is a scroll wheel pretending to be a cursor.
    let stride = if held_shift(&keyboard) { 5 } else { 1 };

    // Walk whichever list has the focus. Two lists, one pair of keys — the alternative was a second
    // pair nobody would remember, on a tab already carrying ten rows of its twelve.
    match state.selected_library_id.clone() {
        Some(id) => {
            let ids = library_ids(
                project.as_ref(),
                &filters,
                live.0 == crate::keys::Context::Tiles,
                Some(&suggestions),
            );
            let Some(at) = ids.iter().position(|d| *d == id) else {
                return;
            };
            let want = if down {
                at + stride
            } else {
                at.saturating_sub(stride)
            };
            if let Some(next) = ids.get(want.min(ids.len().saturating_sub(1))) {
                state.selected_library_id = Some(next.clone());
                state.status.note(format!(
                    "`{next}` selected — {} removes it",
                    keys::binding(Action::RemoveTile).chord
                ));
            }
        }
        None => {
            // **Every row on screen, headings included**, so nothing visible is unreachable and a
            // collapsed pack can be walked onto and opened without the mouse. The old walk stepped
            // mesh rows only: at the top of the first open pack `Up` had nowhere to go while 33
            // headings sat above it, which is the bug reported three times.
            //
            // It also walks the DRAWN order rather than candidate indices. Inside one pack those
            // agree; at a group boundary they do not, and excluded packs are drawn last however
            // early their meshes were scanned.
            let rows = candidate_list_rows(&state, &filters, &project.policy);
            if rows.is_empty() {
                return;
            }
            let here = rows.iter().position(|r| match r {
                ListRow::Header(p) => state.focused_pack.as_deref() == Some(p.as_str()),
                ListRow::Mesh(i) => state.focused_pack.is_none() && *i == state.selected,
            });
            let at = match here {
                Some(at) => at,
                // Not on screen at all — a filter or a fold moved out from under the cursor. An
                // arrow then means "start from the top of what I can see", which is where the eye
                // already is.
                None => {
                    if let Some(row) = rows.first() {
                        put_cursor(&mut state, row);
                    }
                    return;
                }
            };
            let want = if down {
                (at + stride).min(rows.len() - 1)
            } else {
                at.saturating_sub(stride)
            };
            if let Some(row) = rows.get(want) {
                put_cursor(&mut state, row);
            }
        }
    }
}

/// Put the cursor on a row, in whichever of its two states that row calls for.
///
/// One function because the two fields are one cursor: leaving `focused_pack` set while moving to a
/// mesh would highlight a heading and a row at once, and every reader would then have to decide
/// which it believed.
fn put_cursor(state: &mut ImportState, row: &ListRow) {
    match row {
        ListRow::Header(pack) => state.focused_pack = Some(pack.clone()),
        ListRow::Mesh(ix) => {
            state.focused_pack = None;
            state.selected = *ix;
        }
    }
}

/// **The candidate rows the author can actually see**, as indices into [`ImportState::candidates`].
///
/// The sibling of [`library_ids`], filtered with the same predicate `rebuild_candidates` renders
/// with. Indices rather than mesh paths because `ImportState::selected` is an index and every other
/// reader of the focus already goes through it.
/// One row of the candidate list as it appears on screen.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum ListRow {
    /// A pack heading. Reachable whether the pack is open or closed — that is the point.
    Header(String),
    /// A mesh under an open heading, by index into `ImportState::candidates`.
    Mesh(usize),
}

/// **Every row on screen, in the order it is drawn** — headings and meshes together.
///
/// The arrows walk this, so "the next row" means the same thing to the eye and to the keyboard, and
/// nothing visible is unreachable. A collapsed pack contributes its heading and no meshes, which is
/// exactly how `Up` steps onto a folder and then past it to the one above.
pub(crate) fn candidate_list_rows(
    state: &ImportState,
    filters: &crate::filter::Filters,
    policy: &emerge_core::policy::Policy,
) -> Vec<ListRow> {
    let (offered, excluded) = visible_packs(state, filters, policy);
    let mut out: Vec<ListRow> = Vec::new();
    let push = |pack: &String, members: &Vec<usize>, out: &mut Vec<ListRow>| {
        out.push(ListRow::Header(pack.clone()));
        if pack_is_open(state, policy, pack) {
            out.extend(members.iter().copied().map(ListRow::Mesh));
        }
    };
    for (pack, members) in &offered {
        push(pack, members, &mut out);
    }
    if !excluded.is_empty() && state.excluded_open {
        for (pack, members) in &excluded {
            push(pack, members, &mut out);
        }
    }
    out
}

fn candidate_rows(
    state: &ImportState,
    filters: &crate::filter::Filters,
    policy: &emerge_core::policy::Policy,
) -> Vec<usize> {
    let (offered, excluded) = visible_packs(state, filters, policy);
    let mut out: Vec<usize> = Vec::new();
    // **A folded pack contributes nothing**, which is the whole of "the arrows jump over it": its
    // neighbours end up adjacent in this list, so one press moves from the row above a collapsed
    // group to the first row below it.
    for (pack, members) in offered {
        if pack_is_open(state, policy, &pack) {
            out.extend(members);
        }
    }
    if state.excluded_open {
        for (pack, members) in excluded {
            if pack_is_open(state, policy, &pack) {
                out.extend(members);
            }
        }
    }
    out
}

/// **The rows this panel draws, in the order it draws them** — offered packs first, then the
/// `EXCLUDED` group. Both the list and the arrows are built from this, so "the next row" means the
/// same thing to the eye and to the keyboard.
///
/// # The ordering is the point, and it is why this exists
///
/// The walk used to step `state.selected` through candidates in **index** order while the list drew
/// them **grouped by pack**. Inside one pack those agree, because a directory scan lands its meshes
/// consecutively — so it looked right, and only came apart at a group boundary, where the next index
/// belongs to a pack drawn somewhere else entirely. Excluded packs made it certain: they are drawn
/// last however early their indices are.
///
/// Reported at the keyboard, 2026-08-16: *"as soon as it hits a collapsed group, you can't go
/// anymore… I want it to jump over the collapsed groups up to the next uncollapsed selection."*
/// Which is what walking the drawn order does by construction, with no skipping logic at all — a
/// folded pack contributes no rows, so its neighbours are adjacent.
fn visible_packs(
    state: &ImportState,
    filters: &crate::filter::Filters,
    policy: &emerge_core::policy::Policy,
) -> (Vec<(String, Vec<usize>)>, Vec<(String, Vec<usize>)>) {
    let pane = crate::filter::Pane::Candidates;
    let mut offered: Vec<(String, Vec<usize>)> = Vec::new();
    let mut excluded: Vec<(String, Vec<usize>)> = Vec::new();
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
        // **Members are kept whole, folded or not.** The heading says how many it is hiding
        // (`draw_pack`), so clearing them here would make every folded pack report `0 hidden`. Who
        // skips them is the reader's business: the list draws the heading and stops, the walk in
        // `candidate_rows` takes no rows from it at all.
        if policy.excludes(&pack) {
            excluded.push((pack, members));
        } else {
            offered.push((pack, members));
        }
    }
    (offered, excluded)
}


/// **Is this pack's contents on screen?** The one rule, read by the list that draws the rows and by
/// the arrows that walk them.
///
/// It was two rules. The renderer decided folding inline while `candidate_rows` — *"the visible
/// rows"* — knew only about the filter box, so the arrows walked straight into folded packs and the
/// highlight landed on rows nobody could see. Reported at the keyboard, 2026-08-16: *"whenever I
/// scroll up, it doesn't skip collapsed groups."* The collapsed `EXCLUDED` group, added the same
/// day, made it loud — everything inside it is folded by construction.
///
/// That matters more than a cosmetic slip, and the same argument is already written one function
/// down about the filter: **`Accept` acts on the selection**, so a walk that can land somewhere
/// invisible is a walk that can import a mesh the author never saw.
fn pack_is_open(
    state: &ImportState,
    policy: &emerge_core::policy::Policy,
    pack: &str,
) -> bool {
    if policy.excludes(pack) && !state.excluded_open {
        return false;
    }
    !state.folded_packs.contains(pack)
}

/// **Keep the candidate selection on a row that is showing.**
///
/// [`keep_library_selection_visible`] for the other list, and it matters more here: filtering could
/// hide the selected candidate while it stayed selected, and Accept acts on the selection — so the
/// most natural way to find a mesh was also the way to import a different one. Same last rule as its
/// sibling: if nothing survives the filter, leave the selection alone rather than jumping it
/// somewhere arbitrary the moment a half-typed query matches nothing.
/// **Scroll the list so the selected row is on screen.**
///
/// The arrows move a selection that the list did not follow, so walking past the fold moved a
/// highlight nobody could see — and `Delete` and `Enter` both act on that selection, which makes an
/// off-screen one worse than merely awkward.
///
/// Both sections live in one scroll area (`rebuild_candidates` builds "IN LIBRARY" and "NOT YET
/// IMPORTED" into the same list), so there is one thing to scroll and one rule for it.
///
/// # Physical in, logical out
///
/// `ComputedNode` and `UiGlobalTransform` are in **physical** pixels; `ScrollPosition` is in
/// **logical** ones. `docs/2026-08-04-emerge-mapper-handoff.md` §4 records the cost of missing that
/// distinction once already. `inverse_scale_factor` is the conversion, taken from the list itself.
fn keep_selection_on_screen(
    state: Res<ImportState>,
    build: Res<crate::build::Build>,
    rows: Query<(&CandidateRow, &ComputedNode, &UiGlobalTransform)>,
    library_rows: Query<(&LibraryRow, &ComputedNode, &UiGlobalTransform)>,
    kit_rows: Query<(&KitRow, &ComputedNode, &UiGlobalTransform)>,
    headers: Query<(&PackHeader, &ComputedNode, &UiGlobalTransform)>,
    mut lists: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        (
            With<CandidateList>,
            Without<CandidateRow>,
            Without<LibraryRow>,
            Without<KitRow>,
        ),
    >,
    mut follow: Local<crate::chrome::Follow<Selected>>,
) {
    // **One frame late, on purpose.** The rows are rebuilt when the selection moves —
    // `rebuild_candidates` watches the same change — and their `ComputedNode`/`UiGlobalTransform`
    // only describe the new list after that rebuild's commands have applied and layout has run at
    // the end of the frame. Reacting on the change frame reads the PREVIOUS frame's geometry and
    // scrolls to where the row used to be.
    //
    // **Keyed on which row is selected, not on `is_changed`.** This watched `ImportState` and
    // `Build`, and both are written most frames — a status line, a preview watchdog — so the flag
    // was re-armed every frame and the scroll never happened. See `chrome::Follow`.
    if !follow.should_scroll(Some(Selected::now(&state, &build))) {
        return;
    }
    // A UI node's transform is its CENTRE, so the edges are the half-size either side.
    //
    // **Looked up through `Selected::now`, the same value this armed on.** It used to re-derive the
    // precedence here — and got it wrong the moment headings became walkable: `focused_pack` was
    // checked first while `Selected::now` ranks the library above it, so focusing the imported list
    // while a heading was still remembered scrolled to the heading and left the library selection
    // off screen. Reported at the keyboard, 2026-08-16. One decision, read twice, is the same defect
    // this file has now produced three times.
    let selected = match Selected::now(&state, &build) {
        Selected::Header(pack) => headers
            .iter()
            .find(|(h, _, _)| h.0 == pack)
            .map(|(_, n, t)| (t.translation.y, n.size.y * 0.5)),
        Selected::Kit(row) => kit_rows
            .iter()
            .find(|(r, _, _)| r.0 == row)
            .map(|(_, n, t)| (t.translation.y, n.size.y * 0.5)),
        Selected::Library(id) => library_rows
            .iter()
            .find(|(r, _, _)| r.0 == id)
            .map(|(_, n, t)| (t.translation.y, n.size.y * 0.5)),
        Selected::Candidate(ix) => rows
            .iter()
            .find(|(r, _, _)| r.0 == ix)
            .map(|(_, n, t)| (t.translation.y, n.size.y * 0.5)),
    };
    let Some((row_mid, row_half)) = selected else {
        return;
    };

    for (list, list_tf, mut scroll) in &mut lists {
        if let Some(want) = crate::chrome::scroll_to_reveal(
            (row_mid, row_half),
            (list_tf.translation.y, list.size.y * 0.5),
            scroll.0.y,
            list.inverse_scale_factor,
        ) {
            scroll.0.y = want;
        }
    }
}

fn keep_candidate_selection_visible(
    filters: Res<crate::filter::Filters>,
    project: Res<Project>,
    mut state: ResMut<ImportState>,
) {
    // **Folding is a reason to move the selection too**, not just filtering: closing the pack the
    // highlight is in leaves it on a row nobody can see, and `Accept` acts on the selection.
    if !filters.is_changed() && !state.is_changed() {
        return;
    }
    let visible = candidate_rows(&state, &filters, &project.policy);
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
/// The palette filter, for the test suite — `library_ids` itself stays `pub(crate)` so the panel
/// and the keys remain its only callers.
pub fn library_ids_for_test(
    project: &Project,
    filters: &crate::filter::Filters,
    labeled_only: bool,
    pending: Option<&crate::labels::Suggestions>,
) -> Vec<String> {
    library_ids(project, filters, labeled_only, pending)
}

pub(crate) fn library_ids(
    project: &Project,
    filters: &crate::filter::Filters,
    // Composing asks only for judged meshes; the definition bench asks for all of them.
    labeled_only: bool,
    // Proposals waiting on a human — a mesh with one is not settled yet. See `composable`.
    pending: Option<&crate::labels::Suggestions>,
) -> Vec<String> {
    let pane = crate::filter::Pane::Candidates;
    project
        .library
        .descriptors
        .iter()
        .filter(|d| filters.keeps(pane, &d.id))
        // **A tile is composed only from JUDGED meshes.** The two tabs share one list and ask
        // different questions of it: the Meshes tab is the definition bench and shows everything,
        // because an unjudged piece is precisely what it is for — and because un-labelling a piece
        // (`Shift+Delete`, "back to candidates, stripped") has to be reachable somewhere. The Tiles
        // tab composes, and a piece with no mount, no kind and no description has nothing to
        // compose *with*: its footprint is a guess and the solver cannot place what it yields.
        //
        // Asked for at the keyboard, 2026-08-15: *"unlabeled meshes shouldn't show on the tiles
        // tab."* `labels::needs_labels` is the same predicate the VLM batch picks its targets with,
        // so "what the labeler still owes you" and "what you cannot build with yet" cannot drift.
        .filter(|d| !labeled_only || composable(d, pending))
        .map(|d| d.id.clone())
        .collect()
}

/// **Settled enough to build with: labelled AND confirmed.**
///
/// Two conditions, and the second was missing. `judged_enough_to_build_with` answers *does it have a
/// name and a description*, which
/// a machine can satisfy on its own — but a suggestion the VLM has proposed and nobody has looked
/// at is a **question**, not an answer, and the whole reason the labeler stages proposals behind a
/// commit door (`U` applies, `Y` discards) is that a human decides. A mesh whose judgement is still
/// a pending proposal has no business in the palette a tile is composed from.
///
/// Asked for at the keyboard, 2026-08-15: *"before any mesh shows up there, make sure its labels
/// are completed and confirmed."*
///
/// Note what this does NOT require: provenance. Labels applied from a suggestion ARE confirmed —
/// somebody pressed `U` — and hand-authored labels were never in doubt. The only unsettled state is
/// a proposal still waiting, which is exactly what is tested.
pub(crate) fn composable(
    d: &emerge_core::descriptor::Descriptor,
    pending: Option<&crate::labels::Suggestions>,
) -> bool {
    if !crate::labels::judged_enough_to_build_with(d) {
        return false;
    }
    pending.is_none_or(|s| s.get(&EditTarget::Library(d.id.clone())).is_none())
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
    suggestions: Res<crate::labels::Suggestions>,
    project: Res<Project>,
    mut state: ResMut<ImportState>,
    // The Tiles palette hides unjudged meshes, so what counts as "still visible" differs by tab.
    mode: Res<Mode>,
) {
    if !filters.is_changed() {
        return;
    }
    let Some(id) = state.selected_library_id.clone() else {
        return;
    };
    let visible = library_ids(&project, &filters, *mode == Mode::Tiles, Some(&suggestions));
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

/// Clicking the `EXCLUDED` heading opens or closes the group — the same one-key-both-directions
/// shape a pack heading has, on the row that says which state it is in.
fn on_excluded_click(
    activate: On<Activate>,
    headers: Query<&ExcludedHeader>,
    mut state: ResMut<ImportState>,
) {
    if headers.get(activate.entity).is_err() {
        return;
    }
    state.excluded_open = !state.excluded_open;
}

fn on_library_click(
    activate: On<Activate>,
    rows: Query<&LibraryRow>,
    mut state: ResMut<ImportState>,
) {
    if let Ok(row) = rows.get(activate.entity) {
        state.selected_library_id = Some(row.0.clone());
        state
            .status
            .note(format!("`{}` selected — Del removes it", row.0));
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
    mut panels: Query<(
        &mut Node,
        Has<MapRoot>,
        Has<TilesRoot>,
        Has<AnimRoot>,
        Has<ComposeRoot>,
    )>,
) {
    if !mode.is_changed() {
        return;
    }
    for (mut node, is_map, is_tiles, is_anim, is_compose) in &mut panels {
        // **`TilesRoot` serves both tabs.** The left pane shows a mesh or a tile depending on which
        // is live (`rebuild_detail`), and the right-hand library list is needed by both — describing
        // picks a mesh to edit, building picks one to drop. Two copies of that list would be two
        // things to keep in step for no gain.
        let mine = match *mode {
            Mode::Map => is_map,
            Mode::Meshes | Mode::Tiles => is_tiles,
            Mode::Anim => is_anim,
            Mode::Compose => is_compose,
        };
        // A panel belonging to no tab is not ours to touch — the tab strip and the cost readout are
        // both unmarked and must stay visible in every mode.
        if !(is_map || is_tiles || is_anim || is_compose) {
            continue;
        }
        let want = if mine { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

/// **`Option<Res<..>>`, because `Mode` belongs to a door.** See [`crate::editor::in_map_mode`]: every
/// run condition is evaluated, so a bare `Res<Mode>` panics on the menu screen where the door — and
/// its `Mode` — have been dropped.
fn in_meshes_mode(mode: Option<Res<Mode>>) -> bool {
    mode.is_some_and(|m| *m == Mode::Meshes)
}

/// Either tab served by the shared MESHES AND TILES panel — see [`TILES_PANEL_TABS`].
fn in_tiles_panel(mode: Option<Res<Mode>>) -> bool {
    mode.is_some_and(|m| TILES_PANEL_TABS.contains(&m))
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
    mut state: ResMut<ImportState>,
    project: Res<Project>,
    previews: Query<(Entity, &PreviewOf, &Children), With<Preview>>,
    mut transforms: Query<&mut Transform>,
    child_lists: Query<&Children>,
    mesh_nodes: Query<(), With<Mesh3d>>,
    // Ground truth for "is the scene here": the asset store itself. The server's load-state
    // bookkeeping can answer "not loaded" FOREVER for a labeled sub-asset (`…#Scene0`) that is
    // sitting in memory — which kept the watchdog disarmed while the author stared at an empty
    // stage. `contains` is the same check the engine's own spawner gates on.
    worlds: Res<Assets<WorldAsset>>,
    // What this system last said about which mesh's load, so each phase is spoken once.
    mut said: Local<Option<(String, &'static str)>>,
    // The watchdog's memory: which mesh, how long it has been loaded-but-empty, how many
    // respawns it has spent, and whether the materialized instance has been settled. See the
    // HEAL block below.
    mut heal: Local<Option<(String, u32, u8, bool)>>,
) {
    let clear = |commands: &mut Commands| {
        for (e, _, _) in &previews {
            commands.entity(e).despawn();
        }
    };
    // **The Tiles tab stages the tile instead**, on its own stage — see
    // `build::drive_build_preview`. This one shows the mesh being described.
    if *mode != Mode::Meshes {
        clear(&mut commands);
        return;
    }
    // Everything the descriptor decides comes out OWNED in one block, because the status writes
    // below need the state mutably and `editing` borrows it.
    let (mesh, want, want_rot) = {
        let Some(d) = state.editing(&project.measured) else {
            clear(&mut commands);
            return;
        };
        // A blocked candidate has no trustworthy alignment, so a preview of it would be a picture
        // of a guess. The findings say why; an empty grid is the honest illustration. A library
        // entry was measured when it was accepted, so it has no such doubt.
        if state.selected_library_id.is_none() && state.current().is_some_and(|c| c.blocked()) {
            clear(&mut commands);
            return;
        }
        // The key, and the reason there is nothing to show without one: a descriptor with no mesh
        // has no preview, and leaving the previous one up would caption it with the wrong piece.
        let Some(mesh) = d.mesh.clone() else {
            clear(&mut commands);
            return;
        };
        let a = &d.align;
        // **No pivot shift, because no shipped path applies one.**
        //
        // This used to stage at `STAGE.xz - align.pivot`, on the argument that centring the
        // bounding box on the placement point *"is what makes the symmetric footprint an accurate
        // reservation"*. Measured over BRP 2026-08-18: that put the same piece 0.42 m from where
        // the Tiles tab stands it, and the Tiles tab is the one telling the truth —
        // `emerge_bevy::spawn_descriptor` places the file's origin at the placement point and
        // applies no pivot, and that is the spawner a map placement AND a tile member both go
        // through. So the correction was compensating here for something the game never does, and
        // the preview promised a position nothing would honour.
        //
        // `src/placement/furnish.rs:431` **does** apply `- rot * pivot`, and it is the one caller
        // that does. Chosen at the keyboard: the mapper authors maps and tiles, so it previews the
        // path maps and tiles take. The visible consequence is deliberate — a mesh whose origin is
        // not its bounding-box centre now sits off its own footprint rectangle here, which is
        // exactly what it will do in the game.
        //
        // **The transform a real placement applies**, which is `(scale, scale * stretch_y, scale)`
        // — see `emerge_bevy::spawn_descriptor`.
        let want = Transform::from_xyz(
            STAGE.x,
            STAGE.y + staged_lift(d),
            STAGE.z,
        )
        .with_scale(Vec3::new(
            a.scale.unwrap_or(1.0),
            a.scale.unwrap_or(1.0) * a.stretch_y.unwrap_or(1.0),
            a.scale.unwrap_or(1.0),
        ));
        (mesh, want, emerge_bevy::mesh_rotation(d))
    };

    for (e, of, _) in &previews {
        if of.0 != mesh {
            commands.entity(e).despawn();
        }
    }

    // **The stage says what the file is doing.** A large conversion takes real seconds to decode
    // and a failed texture left the stage silently empty — indistinguishable from broken, which is
    // exactly how it got reported. Each phase is spoken once per mesh, and the ready line only
    // replaces this system's OWN loading line, so nobody else's status is stomped.
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(mesh.clone()));
    use bevy::asset::{LoadState, RecursiveDependencyLoadState as Deps};
    let loading_line = format!("loading {} …", leaf(&mesh));
    let phase = match (
        assets.get_load_state(scene.id()),
        assets.get_recursive_dependency_load_state(scene.id()),
    ) {
        (Some(LoadState::Failed(e)), _) => Err(e.to_string()),
        (_, Some(Deps::Failed(e))) => Err(e.to_string()),
        _ if worlds.contains(scene.id()) => Ok(true),
        _ => Ok(false),
    };
    let ready = matches!(&phase, Ok(true));
    match phase {
        Err(e) => {
            if said.as_ref() != Some(&(mesh.clone(), "failed")) {
                *said = Some((mesh.clone(), "failed"));
                state
                    .status
                    .problem(format!("NOT DRAWN — {}: {e}", leaf(&mesh)));
                error!("preview of {mesh}: {e}");
            }
        }
        Ok(false) => {
            if said.as_ref() != Some(&(mesh.clone(), "loading")) {
                *said = Some((mesh.clone(), "loading"));
                state.status.note(loading_line.clone());
            }
        }
        Ok(true) => {
            if said.as_ref() != Some(&(mesh.clone(), "ready")) {
                let was_loading = said.as_ref() == Some(&(mesh.clone(), "loading"));
                *said = Some((mesh.clone(), "ready"));
                if was_loading && state.status.note_text() == loading_line {
                    state.status.note(format!("{} staged", leaf(&mesh)));
                }
            }
        }
    }

    // **The watchdog.** A cold-loaded scene sometimes reports loaded while nothing drawable ever
    // materializes under the stage — the state the author heals by clicking off and back, every
    // time, for every first-viewed mesh. This performs that exact gesture: loaded + staged +
    // meshless past a grace period respawns the preview, WARNS to the terminal so the engine-side
    // loss stays on the record, and gives up loudly after three attempts rather than looping.
    const HEAL_GRACE_FRAMES: u32 = 30;
    const HEAL_ATTEMPTS: u8 = 3;
    let existing = previews
        .iter()
        .find(|(_, of, _)| of.0 == mesh)
        .map(|(e, _, _)| e);
    if heal.as_ref().is_none_or(|(m, _, _, _)| *m != mesh) {
        *heal = Some((mesh.clone(), 0, 0, false));
    }
    let mut respawn_now = false;
    if let (Some(e), Some((_, frames, tries, settled))) = (existing, heal.as_mut()) {
        if holds_a_mesh(e, &child_lists, &mesh_nodes) {
            *frames = 0;
            *tries = 0;
            // **Settle the instance the moment it materializes.** The scene's entities are
            // re-parented under the stage AFTER they spawn, and transform propagation misses
            // that re-parenting: measured live, every mesh sat at GlobalTransform (0,0,0) —
            // the world origin, four kilometres from the camera aimed at the stage, culled to
            // "0 tris drawn" — while the file said staged. Touching the ROOT alone measured as
            // not enough; every Transform in the subtree is marked, so per-entity change
            // detection cannot skip any of them on the next propagation pass.
            if !*settled {
                *settled = true;
                let mut queue = vec![e];
                while let Some(node) = queue.pop() {
                    if let Ok(mut tf) = transforms.get_mut(node) {
                        tf.set_changed();
                    }
                    if let Ok(kids) = child_lists.get(node) {
                        queue.extend(kids.iter());
                    }
                }
            }
        } else {
            *settled = false;
            if ready {
                *frames += 1;
                if *frames > HEAL_GRACE_FRAMES {
                    if *tries < HEAL_ATTEMPTS {
                        *tries += 1;
                        *frames = 0;
                        warn!(
                            "staged preview of {mesh} loaded but never materialized — \
                             respawning (attempt {tries})"
                        );
                        respawn_now = true;
                    } else if *frames == HEAL_GRACE_FRAMES + 1 {
                        state.status.problem(format!(
                            "NOT DRAWN — {}: loaded, but no mesh materialized after \
                             {HEAL_ATTEMPTS} respawns",
                            leaf(&mesh)
                        ));
                    }
                }
            } else {
                *frames = 0;
            }
        }
    }
    if respawn_now {
        if let Some(e) = existing {
            commands.entity(e).despawn();
        }
        // Falls through to the spawn below: a fresh root against the now-warm asset — the
        // author's own off-and-back, performed for them.
    }

    // **Re-applied every frame, not written once at spawn.**
    //
    // This used to `return` here the moment a preview for this mesh existed — so the transform was
    // whatever the descriptor said at the instant the piece was first staged, and every later edit to
    // it changed nothing. Editing the size or the wall height moved the gizmos, which are redrawn from
    // the descriptor each frame, and left the mesh where it was: the author saw *"the box markers get
    // bigger"* and no piece move. `align.rotate` had the same hole — the mesh path does not change
    // when a piece is turned, so the rotate chips only ever took effect on a freshly staged piece.
    //
    // Written only when it differs, because a `Transform` marked changed every frame re-propagates
    // the whole hierarchy.
    if !respawn_now {
        if let Some((e, _, children)) = previews.iter().find(|(_, of, _)| of.0 == mesh) {
            if let Ok(mut tf) = transforms.get_mut(e) {
                if *tf != want {
                    *tf = want;
                }
            }
            for child in children.iter() {
                if let Ok(mut tf) = transforms.get_mut(child) {
                    if tf.rotation != want_rot {
                        tf.rotation = want_rot;
                    }
                }
            }
            return;
        }
    }

    commands
        .spawn((
            Preview,
            PreviewOf(mesh.clone()),
            want,
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
        .with_child((WorldAssetRoot(scene), Transform::from_rotation(want_rot)));
}

/// Draw the footprint the placement rules will reserve, and the grid cells it occupies.
///
/// Two rectangles, deliberately: the measured footprint, and the cells a flood fill would step on.
/// Where they differ is exactly the gap-or-overlap the findings describe in words, and a number in a
/// sentence is much easier to skip than a line that plainly does not meet its neighbour.
/// The subgrid lattice, over the staged tile.
const LATTICE: Color = Color::srgb(0.38, 0.34, 0.46); // CHROME-OK: world ink — see FOOTPRINT
/// A cell an author has said something about — solid, an edge, or an anchor.
const LATTICE_SET: Color = Color::srgb(0.62, 0.52, 0.82); // CHROME-OK: world ink — see FOOTPRINT

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
    // Lifted with the mesh — see `stage_lift`.
    let origin = STAGE - Vec3::new(w * 0.5, -stage_lift(desc), d * 0.5);

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
    let lift = stage_lift(desc);

    // **The ground, when the piece is not on it.** A lifted piece with nothing under it just looks
    // centred differently — there is no cue that it is 1.8 m up rather than that the camera moved. So
    // the floor it hangs above is drawn where it is, with a plumb line up to the piece: the gap
    // between them IS the height, which is the thing the field is for.
    if lift > 0.0 {
        gizmos.rect(
            Isometry3d::new(
                STAGE + Vec3::new(0.0, 0.002, 0.0),
                Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            ),
            Vec2::new(w, d),
            GROUND,
        );
        gizmos.line(STAGE, STAGE + Vec3::new(0.0, lift, 0.0), GROUND);
    }

    let up = Vec3::new(0.0, lift, 0.0);
    // The mesh's own footprint, at the plane it sits on.
    gizmos.rect(
        Isometry3d::new(
            STAGE + up + Vec3::new(0.0, 0.005, 0.0),
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
            STAGE + up + Vec3::new(0.0, 0.01, 0.0),
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        ),
        Vec2::new(
            cx as f32 * emerge_core::grid::SNAP,
            cz as f32 * emerge_core::grid::SNAP,
        ),
        CELLS,
    );
    // And the volume, so height is visible rather than only stated.
    //
    // **Anchored on `STAGE`**, like the two rectangles above. This read `from_xyz(0.0, ..)`, which is
    // the world origin — where the *map* is, not where the tile is staged — so the one gizmo that
    // shows a piece's height was drawn 4 km from the piece and nobody had ever seen it.
    if height > 0.0 {
        gizmos.cube(
            Transform::from_translation(STAGE + up + Vec3::new(0.0, height * 0.5, 0.0))
                .with_scale(Vec3::new(w, height, d)),
            EXTENT,
        );
    }
}

/// The two one-line readouts, plus the block above them. Cheap enough every frame, and guarded so
/// they only write on change.
///
/// The action line is the **receipt** only. It had no colour rule of its own — one fixed colour for
/// everything it was ever handed — which is why `NOT WRITTEN:` from a failed `persist` looked
/// exactly like `added \`crate\``. Refusals go to the banner now.
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
        if t.0 != state.status.note_text() {
            t.0 = state.status.note_text().to_owned();
        }
    }
}

/// Rebuild the candidate list.
///
/// **The list's two tabs**, drawn as a strip so which one is showing is visible rather than inferred.
///
/// `left`/`right` switch them, which costs no key: they were unbound on this tab while nothing was in
/// hand, and `docs/tiles_tab_contract.md` recorded exactly why — *"There is one list on this tab, so
/// there is nothing to switch between."* There are two now.
/// **Which shelf the list is showing.** Three, in the order work moves through them.
///
/// This is not new state: the editor has always had it, spread across two fields nobody drew.
/// `selected_library_id.is_none()` is *"the arrows are walking the candidates"* and
/// `Build::browsing.is_some()` is *"the kit list is up"* — and `left`/`right` have always moved
/// between them. What was missing was any way to see which one you were on, or that a third existed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    /// Measured, not imported. The `NOT YET IMPORTED` packs.
    Candidates,
    /// In `library.ron` — a mesh the editor can build with.
    Library,
    /// The tiles already composed from them.
    Kit,
}

impl Shelf {
    /// **The pipeline, and it is why `right` always means deeper.** A mesh is imported into the
    /// library and the library is composed into the kit, so the strip reads left to right in the
    /// direction work actually moves. Both tabs then agree about what `left` and `right` mean, which
    /// they did not when the strip drew a fixed pair regardless of tab.
    const ORDER: [Shelf; 3] = [Shelf::Candidates, Shelf::Library, Shelf::Kit];

    fn label(self, library: usize, candidates: usize, kit: usize) -> String {
        match self {
            Shelf::Candidates => format!("NOT IMPORTED ({candidates})"),
            Shelf::Library => format!("MESHES ({library})"),
            Shelf::Kit => format!("KIT ({kit})"),
        }
    }
}

/// A chip in the shelf strip, and which shelf it opens.
#[derive(Component, Clone, Copy)]
pub struct ShelfChip(pub Shelf);

/// **The shelves this tab has, as a strip you can read and click.**
///
/// # `NOT YET IMPORTED` was a heading at the bottom of the same list
///
/// The Meshes tab drew `IN LIBRARY (43)`, its rows, then `NOT YET IMPORTED (696)` and a dozen
/// collapsed packs — one list doing two jobs, with the second reachable only by scrolling past the
/// first. Reported at the keyboard, 2026-08-18: *"the not yet imported way at the bottom is not
/// intuitive."* It is worse than unintuitive: the two halves already have **separate walks** and
/// **separate cursors**, and `left`/`right` already switched between them. The editor knew they were
/// two shelves and drew them as one list.
///
/// So the strip shows the two shelves *this tab* has — the Meshes tab imports, the Tiles tab
/// composes — and the list below shows one of them. Every count is of what is shown, per the same
/// rule the headings kept.
fn shelf_strip(
    p: &mut ChildSpawnerCommands,
    at: Shelf,
    on_tiles: bool,
    library: usize,
    candidates: usize,
    kit: usize,
) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(10.0),
        row_gap: Val::Px(crate::chrome::GAP_TIGHT),
        align_items: AlignItems::Center,
        // **Wraps, because two counted chips and a hint do not fit `LIST_W`.** Measured in a
        // capture: `left / right` was clipped at the panel edge, which is the same class of defect
        // as the kit row that wrapped into its own value column — a fixed width and content that
        // outgrew it. The hint drops to a second line rather than being cut, and on a wider panel
        // it stays where it was.
        flex_wrap: FlexWrap::Wrap,
        ..default()
    })
    .with_children(|row| {
        // A tab shows the two shelves its own keys reach: `Meshes` imports (candidates <-> library),
        // `Tiles` composes (library <-> kit). Drawing all three everywhere would offer a chip whose
        // key does nothing on this tab, which is the dead-affordance defect the strip has had before.
        // **Taken from `ORDER`, not written out again.** A second list of the same three shelves is
        // a second place for the pipeline to be stated, and the whole point of the order is that
        // both tabs agree about it. Meshes takes the first pair, Tiles the second.
        let shown = if on_tiles {
            &Shelf::ORDER[1..3]
        } else {
            &Shelf::ORDER[0..2]
        };
        for shelf in shown {
            let active = *shelf == at;
            row.spawn((
                ShelfChip(*shelf),
                Hovered::default(),
                Node {
                    padding: crate::chrome::CHIP_PAD,
                    ..default()
                },
                BackgroundColor(if active {
                    crate::chrome::ROW_SELECTED
                } else {
                    Color::NONE
                }),
                bevy::picking::Pickable::default(),
            ))
            .with_children(|chip| {
                chip.spawn((
                    Text::new(shelf.label(library, candidates, kit)),
                    TextColor(if active { ACCENT } else { DIM }),
                    TextFont::from_font_size(crate::chrome::text::LABEL),
                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    bevy::picking::Pickable::IGNORE,
                ));
            })
            .observe(on_shelf_click);
        }
        // **The idiom, named once.** `left` and `right` walk the strip in the direction it is drawn,
        // which is the direction work moves — see [`Shelf::ORDER`].
        row.spawn((
            Text::new("left / right"),
            TextColor(crate::chrome::LABEL),
            TextFont::from_font_size(crate::chrome::text::HINT),
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
        ));
    });
}

/// **A chip goes where its key goes**, through the same two fields the keys write — so the pointer
/// and the keyboard cannot come to disagree about which shelf is up.
fn on_shelf_click(
    click: On<Pointer<Click>>,
    chips: Query<&ShelfChip>,
    project: Option<Res<Project>>,
    filters: Option<Res<crate::filter::Filters>>,
    suggestions: Option<Res<crate::labels::Suggestions>>,
    mode: Option<Res<Mode>>,
    mut state: Option<ResMut<ImportState>>,
    mut build: Option<ResMut<crate::build::Build>>,
) {
    let (Ok(chip), Some(project), Some(filters), Some(suggestions), Some(mode), Some(state), Some(build)) = (
        chips.get(click.entity),
        project,
        filters,
        suggestions,
        mode,
        state.as_mut(),
        build.as_mut(),
    ) else {
        return;
    };
    // The census counts, never a panel — `census_is_the_one_counter` forbids reading
    // `compositions.compositions.len()` here, and it is the number `Action::KitEnter` guards on.
    let kit_len =
        emerge_core::census::of_catalog(&project.library, &project.compositions.compositions)
            .compositions;
    match chip.0 {
        Shelf::Candidates => {
            build.browsing = None;
            state.selected_library_id = None;
            state.status.note("candidates".to_owned());
        }
        // **The shape `Action::FocusLibrary` already has**, arm for arm — including the refusal.
        // A chip that answered an empty library with nothing at all was the same press saying two
        // different things depending on how it was made.
        Shelf::Library => {
            build.browsing = None;
            match library_ids(&project, &filters, *mode == Mode::Tiles, Some(&suggestions)).first()
            {
                Some(first) => {
                    if state.selected_library_id.is_none() {
                        state.selected_library_id = Some(first.clone());
                    }
                    state.focused_pack = None;
                    state.status.note("library".to_owned());
                }
                None => state.status.note("the library is empty".to_owned()),
            }
        }
        // Row 0, exactly as `Action::KitEnter` does — **including its refusal**, which this arm used
        // to drop. `KitEnter` guards `kit == 0` so `browsing` is never set on an empty kit; without
        // the guard a click on `KIT (0)` walked into `compositions.get(0)`'s `None` arm, the branch
        // whose own comment calls itself "unreachable rather than unlikely".
        Shelf::Kit => {
            if kit_len == 0 {
                state
                    .status
                    .note("no tiles in the kit yet - build one and press Cmd+S".to_owned());
            } else {
                build.browsing = Some(0);
            }
        }
    }
}

/// The authored tiles, with the cursor and which one is open for editing.
///
/// Until this existed the tab could author tiles and never show them: `open_blank` was the only
/// opener, so a tile saved wrong stayed wrong and an author had no way to spot a duplicate.
fn kit_rows(p: &mut ChildSpawnerCommands, project: &Project, cursor: usize) {
    if project.compositions.compositions.is_empty() {
        p.spawn((
            Text::new("nothing authored yet — build a tile and press Cmd+S"),
            TextColor(DIM),
            TextFont::from_font_size(crate::chrome::text::LABEL),
        ));
        return;
    }
    for (i, c) in project.compositions.compositions.iter().enumerate() {
        let here = i == cursor;
        p.spawn((
            // Marked by row index so `keep_selection_on_screen` can follow the kit walk the way it
            // follows the library's — the same defect class, one list over.
            KitRow(i),
            Text::new(format!(
                "{} {}  {} member(s)",
                if here { ">" } else { " " },
                c.id,
                c.members.len()
            )),
            TextColor(if here { ACCENT } else { TEXT }),
            TextFont::from_font_size(crate::chrome::text::BODY),
        ));
    }
}

/// The batch readout's root, shown only while a walk exists.
#[derive(Component)]
struct LabelProgress;
/// `LABELING 16/778`, or `HELD AT 16/778`.
#[derive(Component)]
struct LabelProgressText;
/// The filled part of the bar; its width is the fraction done.
#[derive(Component)]
struct LabelProgressBar;
/// The subject in hand right now.
#[derive(Component)]
struct LabelProgressNow;

/// **What the batch is doing, where the work is landing.**
///
/// Asked for at the keyboard, 2026-08-15: *"I'd like some sort of progress bar to show how it's
/// working through them... each one that's being defined has showed active."* The only readout was
/// a status-line string that the next note overwrote, so a walk of several hundred meshes was
/// indistinguishable from nothing happening — which is exactly how it was reported.
fn paint_label_progress(
    queue: Res<crate::labels::LabelQueue>,
    mut roots: Query<&mut Node, (With<LabelProgress>, Without<LabelProgressBar>)>,
    mut bars: Query<&mut Node, (With<LabelProgressBar>, Without<LabelProgress>)>,
    mut heads: Query<&mut Text, (With<LabelProgressText>, Without<LabelProgressNow>)>,
    mut nows: Query<&mut Text, (With<LabelProgressNow>, Without<LabelProgressText>)>,
) {
    if !queue.is_changed() {
        return;
    }
    let running = queue.running();
    let display = if running {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut roots {
        if node.display != display {
            node.display = display;
        }
    }
    if !running {
        return;
    }
    let (done, total) = queue.progress();
    let fraction = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32 * 100.0
    };
    for mut bar in &mut bars {
        let want = Val::Percent(fraction);
        if bar.width != want {
            bar.width = want;
        }
    }
    for mut text in &mut heads {
        let want = if queue.paused() {
            format!("HELD AT {done}/{total}   Shift+L resumes")
        } else {
            format!("LABELING {done}/{total}   Shift+L holds")
        };
        if text.0 != want {
            text.0 = want;
        }
    }
    for mut text in &mut nows {
        // The active subject, which is the half a bar cannot say.
        let want = queue.current().unwrap_or("").to_owned();
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// One row of the KIT list, by index into `project.compositions.compositions` — the same list
/// `Build::browsing` indexes, which is what makes the scroll-follow able to find the cursor.
#[derive(Component)]
struct KitRow(usize);

/// **The frozen strip above the scrolling list** — `MESHES | KIT (n)` must stay put while the rows
/// scroll under it. It lived as the first child *inside* the scroll container, so it scrolled away
/// with the list; reported from the keyboard 2026-08-14. Rebuilt by `rebuild_candidates` alongside
/// the rows, because the strip's state (which list, the kit count) changes with them.
#[derive(Component)]
struct ListHeader;

/// **The un-imported shelf: what has been measured and not brought in.**
///
/// Lifted out of `rebuild_candidates` when it stopped being the bottom half of the library's list
/// and became a shelf of its own — see [`shelf_strip`]. The body is unchanged; what moved is that
/// nothing is drawn above it, so a pack no longer starts wherever the library happened to end.
fn draw_candidates(
    p: &mut ChildSpawnerCommands,
    state: &ImportState,
    filters: &crate::filter::Filters,
    project: &Project,
) {
    if state.candidates.is_empty() {
        // `Tab` is Feathers' now — `keys::Action::NextTab` was retired with it, so the old
        // "press Tab to scan" named a key that does nothing: the dead affordance this editor keeps
        // finding. Read from the census, so renaming the chord renames the sentence.
        let say = if state.scanned {
            "every mesh under assets/ is already in the library".to_owned()
        } else {
            format!("press {} to scan", crate::keys::chord(Action::Rescan))
        };
        p.spawn((
            Text::new(say),
            TextColor(DIM),
            TextFont::from_font_size(crate::chrome::text::BODY),
        ));
        return;
    }
    // **Grouped by pack.** A flat list this long is one you scroll past; grouped by where they came
    // from it is a dozen headings, and an author importing a kit wants that kit rather than
    // an alphabet.
    //
    // The directory, not `kind` — a candidate has no `kind` yet, that being the thing import
    // is FOR. The folder an artist put it in is the only categorisation that exists before
    // anyone has looked at it, and it is usually the right one.
    // **Excluded packs fall to the bottom, under one collapsed group.**
    //
    // They used to sit in place, each folded and muted. That is honest but it is still one
    // row per excluded pack scattered down a list an author is scrolling to find work in —
    // and a kit that has excluded six packs pays six rows for a fact it already knows.
    // Chosen at the keyboard, 2026-08-16: one `EXCLUDED` group at the end.
    //
    // Still *listed*, never hidden: a mesh that silently disappeared looks identical to one
    // the scan never found, and there would be no way back except editing `project.ron` by
    // hand. The group opens, its packs open, and `Shift+R` on a mesh inside restores it —
    // which is why the group has to reach all the way down to a row.
    // **The same partition the arrows walk** (`visible_packs`), so the rows on screen and
    // the rows the keyboard steps through are one list built once. They were two, and the
    // walk stepped index order while the list drew pack order — which is why the arrows
    // stopped dead at a collapsed group.
    let (offered, excluded_packs) = visible_packs(&state, &filters, &project.policy);
    for (pack, members) in &offered {
        draw_pack(p, pack, members, &state, false, !pack_is_open(&state, &project.policy, pack));
    }

    if !excluded_packs.is_empty() {
        let meshes: usize = excluded_packs.iter().map(|(_, m)| m.len()).sum();
        let packs_n = excluded_packs.len();
        p.spawn((
            UiButton,
            Hovered::default(),
            ExcludedHeader,
            Node {
                width: Val::Percent(100.0),
                padding: CHIP_PAD,
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(6.0),
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(HEADER_BG),
        ))
        .with_children(|row| {
            row.spawn((
                Node { width: Val::Px(10.0), flex_shrink: 0.0, ..default() },
                Text::new(if state.excluded_open { "v" } else { ">" }),
                TextColor(MUTED),
                TextFont::from_font_size(crate::chrome::text::LABEL),
            ));
            row.spawn((
                Node { flex_grow: 1.0, ..default() },
                Text::new("EXCLUDED"),
                TextColor(MUTED),
                TextFont::from_font_size(crate::chrome::text::LABEL),
            ));
            // Names the state and the way out of it, per `docs/ui.md` §1.4.
            row.spawn((
                Text::new(format!(
                    "{packs_n} pack(s), {meshes} mesh(es) — {} restores one",
                    keys::chord(Action::ExcludePack)
                )),
                TextColor(MUTED),
                TextFont::from_font_size(crate::chrome::text::LABEL),
            ));
        });
        if state.excluded_open {
            for (pack, members) in &excluded_packs {
                draw_pack(p, pack, members, &state, true, !pack_is_open(&state, &project.policy, pack));
            }
        }
    }
}

/// Wholesale rather than diffed: it changes on a rescan and on nothing else, and a diffing rebuild of
/// a list this long would be more code than the thing it saves.
fn rebuild_candidates(
    mut commands: Commands,
    state: Res<ImportState>,
    project: Res<Project>,
    filters: Res<crate::filter::Filters>,
    build: Res<crate::build::Build>,
    lists: Query<Entity, With<CandidateList>>,
    headers: Query<Entity, With<ListHeader>>,
    // The panel serves two tabs and they ask different questions of the same list.
    mode: Res<Mode>,
    // A proposal waiting on a human keeps its mesh out of the composing palette — see `composable`.
    suggestions: Res<crate::labels::Suggestions>,
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

    // **Two tabs on the one list, not two lists.** The kit started as a section stacked above the
    // mesh palette in the LEFT controls column, which the author called weird and was: two lists
    // competing for one panel, and the wrong panel. One list showing one of two things is the shape
    // a palette already has.
    let browsing = build.browsing;
    // Which question this panel is being asked: compose (judged only) or define (everything).
    let on_tiles = *mode == Mode::Tiles;
    // The census counts, not this panel -- `census_is_the_one_counter` forbids a panel
    // rendering `compositions.compositions.len()` itself.
    let kit = emerge_core::census::of_catalog(&project.library, &project.compositions.compositions)
        .compositions;

    // **Which shelf is up.** Not new state — see [`Shelf`]. The Tiles tab has only the library and
    // the kit, so a transient `selected_library_id: None` there means "nothing picked yet", not
    // "show me the candidates"; the walk seeds it on the first `down`.
    let at = if browsing.is_some() {
        Shelf::Kit
    } else if on_tiles || state.selected_library_id.is_some() {
        Shelf::Library
    } else {
        Shelf::Candidates
    };

    // The strip rides the header node, not the list — frozen above the scroll. See [`ListHeader`].
    for header in &headers {
        commands.entity(header).despawn_related::<Children>();
        commands.entity(header).with_children(|p| {
            shelf_strip(p, at, on_tiles, in_library, not_imported, kit);
        });
    }
    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|p| {
            if let Some(row) = browsing {
                kit_rows(p, &project, row);
                return;
            }
            // **One shelf, because the strip above says which.** These two were stacked — the
            // library's rows, then a `NOT YET IMPORTED` heading, then a dozen collapsed packs — so
            // the second shelf was reachable only by scrolling past the first, and the count that
            // told you how much was down there was itself below the fold. The headings are gone with
            // the stacking: the chip carries the count now, and saying it twice is what
            // `chrome.rs` exists to stop.
            if at == Shelf::Candidates {
                draw_candidates(p, &state, &filters, &project);
                return;
            }
            for d in project
                .library
                .descriptors
                .iter()
                .filter(|d| filters.keeps(pane, &d.id))
                // The Tiles palette composes, so it lists only judged meshes — see `library_ids`,
                // which the arrows walk by. The two must agree or the keys step onto rows the eye
                // cannot see.
                .filter(|d| !on_tiles || composable(d, Some(&suggestions)))
            {
                let selected = state.selected_library_id.as_deref() == Some(d.id.as_str());
                // **Green when it has been judged, plain when it still owes an answer.** The one
                // glance that says whether a mesh can build anything yet; on the Tiles tab every
                // row is green by construction, which is the point of the split.
                let judged = composable(d, Some(&suggestions));
                crate::chrome::list_row(p, selected, LibraryRow(d.id.clone())).with_children(
                    |row| {
                        row.spawn((
                            Text::new(d.id.clone()),
                            TextColor(if judged { crate::chrome::LABELED } else { TEXT }),
                            TextFont::from_font_size(crate::chrome::text::LABEL),
                        ));
                    },
                );
            }
        });
    }
}


/// **Which row this panel's lists have highlighted**, as one comparable value.
///
/// Three lists share one scroll area — candidates, the kit's tiles, and the library — and which of
/// them owns the highlight depends on what the author is doing. `chrome::Follow` needs a single key
/// to compare frame to frame, and this is it: the same precedence the scroll system reads, stated
/// once so the two cannot disagree about what "the selection moved" means.
#[derive(PartialEq, Clone)]
pub(crate) enum Selected {
    /// Standing on a pack heading, which the arrows can do since 2026-08-16.
    Header(String),
    /// Walking the kit's own tiles (`Build::browsing`), which outranks the rest.
    Kit(usize),
    /// A library row, focused by click or by `Cmd`+remove on the Map.
    Library(String),
    /// A candidate from the scan, by index.
    Candidate(usize),
}

impl Selected {
    fn now(state: &ImportState, build: &crate::build::Build) -> Selected {
        match (&build.browsing, &state.selected_library_id, &state.focused_pack) {
            (Some(row), _, _) => Selected::Kit(*row),
            (None, Some(id), _) => Selected::Library(id.clone()),
            // A heading outranks the mesh cursor underneath it — it is what is highlighted.
            (None, None, Some(pack)) => Selected::Header(pack.clone()),
            (None, None, None) => Selected::Candidate(state.selected),
        }
    }
}

/// **One pack heading and, unless it is folded, the meshes under it.**
///
/// Extracted so the ordinary list and the collapsed `EXCLUDED` group below it draw a pack exactly
/// the same way — two copies of this would be two ideas of what a pack row looks like, and the
/// excluded one is the copy nobody would look at.
#[allow(clippy::too_many_arguments)]
fn draw_pack(
    p: &mut ChildSpawnerCommands,
    pack: &str,
    members: &[usize],
    state: &ImportState,
    excluded: bool,
    folded: bool,
) {
    let pack = pack.to_owned();
    let members: Vec<usize> = members.to_vec();
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
                // **The heading shows the cursor**, because the arrows can stand on it now.
                    BackgroundColor(if state.focused_pack.as_deref() == Some(pack.as_str()) {
                        ROW_SELECTED
                    } else {
                        HEADER_BG
                    }),
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
                    TextFont::from_font_size(crate::chrome::text::LABEL),
                ));
                row.spawn((
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                    Text::new(pack.clone()),
                    TextColor(if excluded { MUTED } else { LABEL }),
                    TextFont::from_font_size(crate::chrome::text::LABEL),
                ));
                // **The count, and nothing else.** A folded pack used to say
                // "{n} hidden — click to open", on the argument that a bare count reads as absence
                // when 145 rows have just left the screen. Removed at the keyboard, 2026-08-18:
                // that is a whole sentence on every folded row of a list an author is scrolling,
                // and the chevron to the left of the name (`>` folded, `v` open) already carries
                // both the state and the affordance. An excluded pack keeps its words because it
                // names a state the chevron does NOT show, and the way out of it.
                row.spawn((
                    Text::new(if excluded {
                        // Names the state and the way out of it, per `docs/ui.md` §1.4.
                        format!(
                            "excluded ({}) — {} restores",
                            members.len(),
                            keys::chord(Action::ExcludePack)
                        )
                    } else {
                        format!("{}", members.len())
                    }),
                    TextColor(if excluded { MUTED } else { LABEL }),
                    TextFont::from_font_size(crate::chrome::text::LABEL),
                ));
            });
            if folded {
                return;
            }
            for ix in members {
                let Some(c) = state.candidates.get(ix) else {
                    continue;
                };
                crate::chrome::list_row(p, ix == state.selected, CandidateRow(ix))
                .with_children(|row| {
                    // The severity mark first, so a list of 300 can be skimmed for the ones that
                    // need attention rather than read. The tint comes from the one severity map,
                    // so the mark here and the rail in the detail pane cannot disagree.
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
                            Some(s) => crate::chrome::severity_style(s).0,
                            None => LABEL,
                        }),
                        TextFont::from_font_size(crate::chrome::text::BODY),
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
                        TextFont::from_font_size(crate::chrome::text::LABEL),
                    ));
                });
            }
}

/// **The tile in hand** — what the Tiles tab shows where the mesh tab shows a mesh.
///
/// Everything here answers a question the author is about to act on: which mode am I in, what am I
/// building, how big is a cell, where is the cursor, and what is already in the tile. That is the
/// feedback half of Compton's grokloop, which Lai et al.'s second pillar asks to keep short — *"the
/// speed of learning depends on how short the loop is."* A tile you cannot see is a loop with the
/// feedback removed.
fn build_detail(p: &mut ChildSpawnerCommands, build: &crate::build::Build, project: &Project) {
    let line = |p: &mut ChildSpawnerCommands, text: String, colour: Color, size: f32| {
        p.spawn((
            Text::new(text),
            TextColor(colour),
            TextFont::from_font_size(size),
        ));
    };

    let Some(comp) = build.open.as_ref() else {
        crate::chrome::section(p, "TILE");
        line(
            p,
            "no tile open — press N to start one".to_owned(),
            ACCENT,
            10.0,
        );
        return;
    };
    let emerge_core::composition::Envelope::Bounded { size } = comp.envelope else {
        crate::chrome::section(p, "TILE");
        line(p, format!("`{}` claims no tile", comp.id), DANGER, 10.0);
        return;
    };

    crate::chrome::section(p, "TILE");
    line(p, comp.id.clone(), TEXT, 12.0);
    line(
        p,
        format!("{:.2} x {:.2} x {:.2} m", size.0, size.1, size.2),
        DIM,
        9.0,
    );
    // **Whether a solver can place this, said where the size is said.**
    //
    // `grammar::from_compositions` skips anything that is not one cell across, so a group that has
    // grown past a tile is hand-stamped content rather than solver content. That was a *problem*
    // raised on every size change, which the author's own log showed accumulating fifteen deep from
    // one nudge and still on screen after the tile was emptied — see `build::refit_tile`.
    //
    // It is a property, so it lives beside the property it qualifies and is true exactly while it is
    // visible. `ACCENT`, not `DANGER`: nothing is wrong — a bigger group is still what Compose
    // composes and still stamps by hand — but an author who believes they are building solver content
    // has to be able to see that they are not.
    if !crate::build::is_one_cell(size) {
        let (nx, nz) = crate::build::tiles_across(size);
        line(
            p,
            format!("{nx} x {nz} tiles — hand-stamped, too big to generate"),
            ACCENT,
            9.0,
        );
        // **And which member did it.** The line above is a consequence; on a tile with six members
        // it left the author to find the cause by opening each one. `docs/ui.md` §3.2 asks for the
        // delta, and the delta here is a named piece and a distance.
        if let Some(why) = crate::build::what_made_it_big(&comp.members, &project.library, size) {
            line(p, why, DIM, 9.0);
        }
    }

    // **The grid, and where you are on it.** The ladder is the focused piece's own span, deepened
    // in thirds by `J` — so what an author reads here is steps between centre and flush, not cells
    // of a lattice the piece can land beside.
    crate::chrome::section(p, "GRID");
    let n = project
        .lattice
        .snap_divisor
        .saturating_pow(build.depth)
        .max(1);
    line(
        p,
        match build.depth {
            0 => "centre and flush — J for thirds".to_owned(),
            _ => format!("centre to flush in {n} steps — J deepens, and wraps"),
        },
        TEXT,
        10.0,
    );
    // **Where the arrows are, which is where the focused member is** — there is no cursor beside it
    // to report. This used to print `build.at`, a derived cell index written only by the nudge and so
    // stale after every drop, removal and undo; and its writer measured signed rungs from the tile's
    // centre while this read whole cells from the tile's minimum corner, so on a nudged tile it named
    // a cell that does not exist and a corner half an envelope from the piece it claimed to describe.
    match build.open.as_ref().and_then(|c| c.members.get(build.focus)) {
        Some(m) => {
            line(
                p,
                format!("focus ({:+.3}, {:+.3}) at {:.3} m", m.at.0, m.at.1, m.lift),
                ACCENT,
                10.0,
            );
        }
        // The drop lands in the middle whatever the rung, so an empty tile can say exactly where.
        None => line(
            p,
            "empty — the next drop lands centred".to_owned(),
            ACCENT,
            10.0,
        ),
    }

    crate::chrome::section(p, "MEMBERS");
    if comp.members.is_empty() {
        line(
            p,
            "nothing yet — pick a piece in the list and press Enter".to_owned(),
            ACCENT,
            10.0,
        );
        return;
    }
    for (i, m) in comp.members.iter().enumerate() {
        let focused = i == build.focus;
        // Angle brackets for a hole, so a row that is a *place for* something never reads as a thing
        // that is there — the same shape `compose::describe_member` uses.
        let what = match &m.body {
            emerge_core::composition::Body::Descriptor { id, .. } => id.clone(),
            emerge_core::composition::Body::Composition { id } => format!("[{id}]"),
            emerge_core::composition::Body::Slot { accepts } => format!("<{accepts}>"),
        };
        let yaw = if m.yaw == 0.0 {
            String::new()
        } else {
            format!(" yaw {:.0}", m.yaw)
        };
        line(
            p,
            format!(
                "{} {}  ({:+.2}, {:+.2}) +{:.2}{yaw}",
                if focused { ">" } else { " " },
                what,
                m.at.0,
                m.at.1,
                m.lift
            ),
            if focused { ACCENT } else { TEXT },
            10.0,
        );
    }
}

/// Rebuild the detail for whichever candidate is selected.
fn rebuild_detail(
    mut commands: Commands,
    state: Res<ImportState>,
    cell_edit: Res<CellEdit>,
    note_edit: Res<NoteEdit>,
    scale_edit: Res<ScaleEdit>,
    height_edit: Res<HeightEdit>,
    suggestions: Option<Res<crate::labels::Suggestions>>,
    project: Res<Project>,
    mode: Res<Mode>,
    build: Res<crate::build::Build>,
    panes: Query<Entity, With<DetailPane>>,
) {
    for pane in &panes {
        commands.entity(pane).despawn_related::<Children>();
        commands.entity(pane).with_children(|p| {
            // **The Tiles tab draws the tile, not the mesh.** One pane serves both tabs: they show
            // different subjects but the same shape of thing, and two panes would be two layouts to
            // keep in step. Which subject is the tab's to say.
            if *mode == Mode::Tiles {
                build_detail(p, &build, &project);
                return;
            }
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
            let (Some(d), Some(placed)) =
                (state.editing(&project.measured), state.placed(&project))
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
            // The VLM's pending proposal for this piece, if one exists AND still describes this
            // mesh — a re-import under the same id must not wear another mesh's labels.
            let proposal = state.target().and_then(|t| {
                let entry = suggestions.as_ref()?.get(&t)?;
                (d.mesh.as_deref() == Some(entry.mesh.as_str())).then(|| entry.clone())
            });

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
                TextFont::from_font_size(crate::chrome::text::TAB),
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
            let (note_text, note_tint) = crate::chrome::field_text(
                note_edit.active.as_ref().map(|(_, raw)| raw.as_str()),
                match d.note.as_deref() {
                    Some(n) if !n.is_empty() => (n.to_owned(), TEXT),
                    _ => ("describe it\u{2026}".to_owned(), LABEL),
                },
            );
            crate::chrome::text_field(
                p,
                Val::Auto,
                NoteField,
                10.0,
                (note_text, note_tint),
                NoteReadout,
            )
            .entry::<Node>()
            .and_modify(|mut n| n.margin.bottom = Val::Px(crate::chrome::GAP_ROW));

            // **The proposal header** — machine-proposed, human-unconfirmed, and it says so with
            // its provenance and its verbs. Everything below in `SUGGEST` is part of this question.
            if let Some(entry) = &proposal {
                let s = &entry.suggestion;
                let p_ = &entry.provenance;
                let attempts = if p_.attempts > 1 {
                    format!(", {} attempts", p_.attempts)
                } else {
                    String::new()
                };
                p.spawn((
                    Text::new(format!(
                        "PROPOSED by {} ({}{attempts}, confidence {:?}) - U apply, Y discard",
                        p_.model, p_.date, s.confidence
                    )),
                    TextColor(crate::chrome::SUGGEST),
                    TextFont::from_font_size(crate::chrome::text::LABEL),
                ));
                // The model's identification — the reasoning its answers hang off, and the line a
                // reviewer sanity-checks first.
                p.spawn((
                    Text::new(s.what.clone()),
                    TextColor(TEXT),
                    TextFont::from_font_size(crate::chrome::text::LABEL),
                ));
                // The proposed note as a ghost line — never in the editable buffer.
                if let Some(note) = &s.note {
                    if d.note.as_deref() != Some(note.as_str()) {
                        p.spawn((
                            Text::new(format!("proposed: {note}")),
                            TextColor(crate::chrome::SUGGEST),
                            TextFont::from_font_size(crate::chrome::text::HINT),
                        ));
                    }
                }
                // The proposed front face, when it differs from what stands.
                if let Some(front) = s.front {
                    if d.align.front != Some(front) {
                        p.spawn((
                            Text::new(format!(
                                "front {} -> proposed: {front:?}",
                                match d.align.front {
                                    Some(f) => format!("{f:?}"),
                                    None => "unset".to_owned(),
                                }
                            )),
                            TextColor(crate::chrome::SUGGEST),
                            TextFont::from_font_size(crate::chrome::text::HINT),
                        ));
                    }
                }
                // A righting turn changes what U does: it turns the piece (the same re-measure
                // the N/P keys run) and re-asks the model, because labels judged from a sideways
                // render describe the wrong orientation.
                if let Some(turn) = &s.needs_turn {
                    let key = if turn.axis == "x" { "N" } else { "P" };
                    p.spawn((
                        Text::new(format!(
                            "appears to lie down - U rights it about {} and re-asks the model \
                             (hand turn: key {key}): {}",
                            turn.axis.to_uppercase(),
                            turn.why
                        )),
                        TextColor(crate::chrome::SUGGEST),
                        TextFont::from_font_size(crate::chrome::text::HINT),
                    ));
                }
                // Vocabulary the model wanted and could not have — a human's decision, elsewhere.
                // Still SUGGEST, not DIM: the header's contract is that everything in this block
                // is part of the machine-proposed question, and this line is no less a proposal
                // for pointing at a different door.
                for t in &s.token_proposals {
                    p.spawn((
                        Text::new(format!(
                            "wants `{}` on {} ({}) - needs a human vocab edit; see \
                             slop/llm/vocab_proposals.ron",
                            t.token, t.axis, t.why
                        )),
                        TextColor(crate::chrome::SUGGEST),
                        TextFont::from_font_size(crate::chrome::text::HINT),
                    ));
                }
            }

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
                        crate::chrome::row_label(row, 48.0, label);
                        crate::chrome::row_value(row, value, TEXT, ());
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
            // Not `placed` — that name is already the layered *descriptor* in this scope, and
            // shadowing it here would have handed the lattice code below a footprint tuple.
            let placed_fp = emerge_core::descriptor::placed_footprint(d);
            let (width_text, width_tint) = crate::chrome::field_text(
                scale_edit.active.as_ref().map(|(_, raw)| raw.as_str()),
                match placed_fp {
                    Some((w, _)) => (format!("{w:.2}"), TEXT),
                    None => ("--".to_owned(), LABEL),
                },
            );
            // What the number means, spelled out. The multiplier shown is `align.scale` — the render
            // factor mapping the authored mesh onto this extent — because a resize BAKES: the extent
            // is rewritten and the scale composes (`bake_width`), so the extent itself is always the
            // placed truth and the scale is the only derived fact worth surfacing.
            let width_note = match placed_fp {
                Some((w, dep)) => match d.align.scale {
                    Some(s) => format!("  {w:.2} x {dep:.2} m — mesh scaled {s:.3}x"),
                    None => format!("  {w:.2} x {dep:.2} m"),
                },
                None => "  no measured footprint to size".to_owned(),
            };
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                margin: UiRect::bottom(Val::Px(crate::chrome::GAP_ROW)),
                ..default()
            })
            .with_children(|row| {
                crate::chrome::text_field(
                    row,
                    Val::Px(62.0),
                    ScaleField,
                    11.0,
                    (width_text, width_tint),
                    ScaleReadout,
                );
                row.spawn((
                    Text::new(width_note),
                    TextColor(LABEL),
                    TextFont::from_font_size(crate::chrome::text::LABEL),
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
                // **"mount", not "layer".** The subgrid below has its own `layer y` picker, and
                // one panel saying "layer" twice about two different things is the confusion the
                // key census already fixed on its side (`Action::CycleMount`).
                crate::chrome::row_label(row, 48.0, "mount");
                crate::chrome::row_value(
                    row,
                    mount_label(d.mount.as_ref()),
                    if d.mount.is_some() { TEXT } else { ACCENT },
                    (),
                );
                // The proposed mount rides the same row, when it differs — one line, one fact.
                if let Some(m) = proposal.as_ref().and_then(|e| e.suggestion.mount.as_ref()) {
                    if d.mount.as_ref() != Some(m) {
                        row.spawn((
                            Text::new(format!("  -> proposed: {}", mount_label(Some(m)))),
                            TextColor(crate::chrome::SUGGEST),
                            TextFont::from_font_size(crate::chrome::text::LABEL),
                        ));
                    }
                }
            });

            // **How far up, for the two mounts that have an up.**
            //
            // Drawn only when the mount carries a height — `mount_height` is the one place that
            // decides which those are, so this cannot drift from the schema. A dead field beside
            // `on floor` would be the panel asking a question with no answer.
            if let Some(now) = d.mount.as_ref().and_then(emerge_core::descriptor::mount_height) {
                let (height_text, height_tint) = crate::chrome::field_text(
                    height_edit.active.as_ref().map(|(_, raw)| raw.as_str()),
                    (format!("{now:.2}"), TEXT),
                );
                p.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    margin: UiRect::top(Val::Px(crate::chrome::GAP_TIGHT)),
                    ..default()
                })
                .with_children(|row| {
                    crate::chrome::row_label(row, 48.0, "height");
                    crate::chrome::text_field(
                        row,
                        Val::Px(62.0),
                        MountHeightField,
                        11.0,
                        (height_text, height_tint),
                        MountHeightReadout,
                    );
                    row.spawn((
                        Text::new("  m up the wall, from the map floor"),
                        TextColor(LABEL),
                        TextFont::from_font_size(crate::chrome::text::LABEL),
                    ));
                });
            }

            // A piece whose size is not measured yet has no derivable lattice, and the honest thing
            // is to say which piece and why rather than draw an empty grid that looks authored.
            let div = match project.divisions_of(placed) {
                Ok(div) => div,
                Err(why) => {
                    crate::chrome::section(p, "SUBGRID");
                    p.spawn((
                        Text::new(why),
                        TextColor(ACCENT),
                        TextFont::from_font_size(crate::chrome::text::HINT),
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
            let subunit_mm = emerge_core::grid::SNAP / project.lattice.face_bands as f32 * 1000.0;
            p.spawn((
                Text::new(format!("{dx} x {dy} x {dz} cells of {subunit_mm:.0} mm")),
                TextColor(TEXT),
                TextFont::from_font_size(crate::chrome::text::BODY),
                DivReadout,
            ));
            p.spawn((
                Text::new(format!(
                    // **`from the map`, not `from project.ron`.** The setting moved on 2026-08-16,
                    // and a row telling an author to go and edit a file that no longer holds the
                    // number is worse than a row saying nothing.
                    "{marked} of {} marked — {} division(s) per {:.1} m tile, from the map",
                    emerge_core::descriptor::Subgrid::volume(div),
                    project.lattice.face_bands,
                    emerge_core::grid::SNAP,
                )),
                TextColor(DIM),
                TextFont::from_font_size(crate::chrome::text::HINT),
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
                                TextFont::from_font_size(crate::chrome::text::HINT),
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
                                header_button(
                                    row,
                                    FillHeader {
                                        layer: y,
                                        span: Span::Layer,
                                    },
                                    "*",
                                );
                                for x in 0..dx {
                                    header_button(
                                        row,
                                        FillHeader {
                                            layer: y,
                                            span: Span::Column(x),
                                        },
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
                                        FillHeader {
                                            layer: y,
                                            span: Span::Row(z),
                                        },
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
                                                min_height: Val::Px(MIN_FIELD_H),
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
                                        .with_children(
                                            |b| {
                                                b.spawn((
                                                    Text::new(cell_glyph(cell).to_owned()),
                                                    // A marked cell is brighter than an empty one —
                                                    // luminance, not hue, per `docs/ui.md` §1.3.
                                                    TextColor(if cell.is_some() {
                                                        ACCENT
                                                    } else {
                                                        LABEL
                                                    }),
                                                    TextFont::from_font_size(crate::chrome::text::BODY),
                                                    CellGlyph(x, z),
                                                    CellLayer(y),
                                                ));
                                            },
                                        );
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
                    Some(raw) => format!("{},{},{}  edge `{raw}_`", at.0, at.1, at.2),
                    None => format!(
                        "{},{},{}  {}",
                        at.0,
                        at.1,
                        at.2,
                        grid.at(at)
                            .map(describe_cell)
                            .unwrap_or_else(|| "open".to_owned())
                    ),
                },
                None => "no cell picked".to_owned(),
            };
            p.spawn((
                Text::new(detail),
                TextColor(if cell_edit.active.is_some() {
                    ACCENT
                } else {
                    DIM
                }),
                TextFont::from_font_size(crate::chrome::text::HINT),
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
                for verb in [CellVerb::Solid, CellVerb::Edge, CellVerb::Clear] {
                    let on = matches!(
                        (verb, cell_edit.at.and_then(|a| grid.at(a))),
                        (CellVerb::Solid, Some(c)) if c.solid
                    );
                    crate::chrome::chip(
                        chips,
                        verb,
                        verb.label(),
                        10.0,
                        if verb == CellVerb::Clear { DANGER } else { TEXT },
                        if on { ROW_SELECTED } else { ROW_BG },
                        Color::NONE,
                    );
                }

                // **Turning the mesh sits beside the lattice, because it reshapes it.** A quarter
                // turn about X or Z swaps the piece's height with a floor axis, so the grid above
                // changes shape — putting these anywhere else would hide the cause of that.
                for axis in [RotateAxis::X, RotateAxis::Y, RotateAxis::Z] {
                    crate::chrome::chip(chips, axis, axis.label(), 10.0, LABEL, ROW_BG, Color::NONE)
                        .entry::<Node>()
                        .and_modify(|mut n| n.margin.left = Val::Px(crate::chrome::GAP_ROW));
                }

                // **Occupancy from the mesh, on its own chip.** Nobody hand-marks a lattice this
                // size, so the cells have to come off the geometry — but this is a button and never
                // runs on import, because it overwrites hand-authored cells and an author who tuned
                // a lattice must not lose it to re-importing.
                crate::chrome::chip(
                    chips,
                    ScanMeshButton,
                    "rescan mesh",
                    10.0,
                    ACCENT,
                    ROW_BG,
                    Color::NONE,
                )
                .entry::<Node>()
                .and_modify(|mut n| n.margin.left = Val::Px(crate::chrome::GAP_ROW));
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
                // The model's proposed set for this axis — the chips' third state.
                let proposed: Vec<String> = proposal
                    .as_ref()
                    .map(|e| match axis {
                        Axis::Kind => e.suggestion.kind.clone(),
                        Axis::Effects => e.suggestion.effects.clone(),
                        Axis::Look => e.suggestion.look.clone(),
                        Axis::Surfaces => e.suggestion.offers_surfaces.clone(),
                    })
                    .unwrap_or_default();
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
                        // Proposed-but-not-held: ghost-lit in SUGGEST with a hairline border —
                        // visibly a question, not a selection. A token both held and proposed is
                        // simply held; agreement is not news.
                        let ghost = !on && proposed.iter().any(|t| t == name);
                        crate::chrome::chip(
                            chips,
                            TagChip { axis, token: ix },
                            name,
                            10.0,
                            if on {
                                TEXT
                            } else if ghost {
                                crate::chrome::SUGGEST
                            } else {
                                LABEL
                            },
                            if on { ROW_SELECTED } else { ROW_BG },
                            if ghost { crate::chrome::SUGGEST } else { Color::NONE },
                        );
                    }
                });
            }

            // **Rooms and group** — the first UI these placement fields have ever had. Read-only:
            // apply writes them, and hand-editing free text stays a `library.ron` edit, as today.
            {
                let p_rooms = proposal
                    .as_ref()
                    .map(|e| e.suggestion.rooms.clone())
                    .unwrap_or_default();
                let p_group = proposal.as_ref().and_then(|e| e.suggestion.group.clone());
                let show_rooms = !d.placement.rooms.is_empty() || !p_rooms.is_empty();
                let show_group = d.placement.group.is_some() || p_group.is_some();
                if show_rooms || show_group {
                    crate::chrome::section(p, "PLACEMENT");
                }
                let mut line = |label: &str, now: String, prop: Option<String>| {
                    p.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        ..default()
                    })
                    .with_children(|row| {
                        crate::chrome::row_label(row, 48.0, label);
                        // 11 px like every other value beside a label — this pane rendered 10/10
                        // until the 2026-08-17 type-role decision unified the pair at 10/11.
                        crate::chrome::row_value(
                            row,
                            if now.is_empty() { "-".to_owned() } else { now },
                            TEXT,
                            (),
                        );
                        if let Some(prop) = prop {
                            row.spawn((
                                Text::new(format!("  -> proposed: {prop}")),
                                TextColor(crate::chrome::SUGGEST),
                                TextFont::from_font_size(crate::chrome::text::LABEL),
                            ));
                        }
                    });
                };
                if show_rooms {
                    let now = d.placement.rooms.join(", ");
                    let prop = (!p_rooms.is_empty() && p_rooms != d.placement.rooms)
                        .then(|| p_rooms.join(", "));
                    line("rooms", now, prop);
                }
                if show_group {
                    let now = d.placement.group.clone().unwrap_or_default();
                    let prop = p_group.filter(|g| Some(g) != d.placement.group.as_ref());
                    line("group", now, prop);
                }
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
                    TextFont::from_font_size(crate::chrome::text::HINT),
                    Node {
                        margin: UiRect::bottom(Val::Px(crate::chrome::GAP_ROW)),
                        ..default()
                    },
                ));
                for f in findings {
                    let (tint, word) = crate::chrome::severity_style(f.severity);
                    crate::chrome::severity_rail(p, tint, ()).with_children(|block| {
                        block.spawn((
                            Text::new(word),
                            TextColor(tint),
                            TextFont::from_font_size(crate::chrome::text::HINT),
                        ));
                        block.spawn((
                            Text::new(f.message.clone()),
                            TextColor(TEXT),
                            TextFont::from_font_size(crate::chrome::text::LABEL),
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
                                TextFont::from_font_size(crate::chrome::text::LABEL),
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
/// **What `M` does to an authored wall height.** See `cycle_mount`.
#[cfg(test)]
mod mount_cycle_tests {
    use emerge_core::descriptor::{
        DecalHost, Mount, mount_height, mount_options, with_mount_height,
    };

    /// The lookup `cycle_mount` performs, extracted so the rule can be tested without an `App`. It
    /// must find the current mount **by kind**, ignoring the height payload.
    fn position_of(options: &[Mount], current: &Mount) -> Option<usize> {
        let had = mount_height(current);
        options.iter().position(|o| match had {
            Some(h) => with_mount_height(o, h).as_ref() == Some(current),
            None => o == current,
        })
    }

    /// **The reset this guards against.** `Mount` compares its payload, so a piece authored at 1.2 m
    /// is not equal to the list's `OnWall { height: 1.8 }` — and a miss in `cycle_mount` falls through
    /// to `map_or(0, ..)`, which is `OnFloor`. Before the height was authorable every wall mount was
    /// 1.8 and this could not happen; the moment it is authorable, one tap of `M` would have taken a
    /// picture off the wall and put it on the ground.
    #[test]
    fn a_piece_at_an_authored_height_is_still_found_in_the_list() {
        let options = mount_options(&["worktop".to_owned()]);
        for current in [
            Mount::OnWall { height: 1.2 },
            Mount::OnWall { height: 0.0 },
            Mount::Decal {
                on: DecalHost::Wall { height: 2.35 },
            },
        ] {
            let at = position_of(&options, &current)
                .unwrap_or_else(|| panic!("{current:?} is not in the offered list"));
            assert!(
                mount_height(&options[at]).is_some(),
                "{current:?} matched {:?}, which is not a wall mount",
                options[at]
            );
        }
    }

    /// Naive equality is what fails, which is worth pinning so nobody simplifies the lookup back.
    #[test]
    fn plain_equality_is_what_does_not_work() {
        let options = mount_options(&[]);
        let authored = Mount::OnWall { height: 1.2 };
        assert!(
            !options.iter().any(|o| *o == authored),
            "if this ever passes, the offered list gained a 1.2 m entry and this test is stale"
        );
    }

    /// A mount with no height is still found the plain way — the carry must not disturb the common
    /// case, which is every piece that stands on the floor.
    #[test]
    fn a_mount_with_no_height_is_unaffected() {
        let options = mount_options(&["worktop".to_owned()]);
        for current in [
            Mount::OnFloor,
            Mount::OnCeiling,
            Mount::OnSurface {
                class: "worktop".into(),
            },
        ] {
            let at = position_of(&options, &current)
                .unwrap_or_else(|| panic!("{current:?} is not in the offered list"));
            assert_eq!(options[at], current);
        }
    }
}

/// **The `SIZE (m)` field's arithmetic.** See [`bake_width`].
#[cfg(test)]
mod scale_field_tests {
    use super::*;

    fn piece(w: f32, dep: f32, h: f32, scale: Option<f32>) -> Descriptor {
        Descriptor {
            id: "p".into(),
            extent: emerge_core::descriptor::Extent {
                footprint: Some((w, dep)),
                height: Some(h),
            },
            align: emerge_core::descriptor::Align {
                scale,
                ..Default::default()
            },
            ..Descriptor::default()
        }
    }

    /// **The trap this function exists to avoid.** Typing back the width already on screen must
    /// change nothing. The bake makes that true by construction: the ratio is against the placed
    /// width — the number the field shows — so the shown value maps to a ratio of exactly 1.
    #[test]
    fn a_width_that_is_already_set_is_a_no_op() {
        let mut d = piece(1.0, 0.5, 2.0, None);
        let r = bake_width(&mut d, 0.6).unwrap_or_else(|e| panic!("{e}"));
        assert!((r - 0.6).abs() < 1e-6, "{r}");
        let after = d.clone();

        // The field now shows 0.60. Committing that again must change nothing at all.
        let r = bake_width(&mut d, 0.6).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r, 1.0);
        assert_eq!(
            d, after,
            "committing the shown width must be a byte-level no-op"
        );
    }

    /// **The resize is uniform and it is a bake**: every extent axis moves by the ratio, and the
    /// render scale composes so the drawn mesh still matches the recorded extent.
    #[test]
    fn resizing_rewrites_the_extent_and_composes_the_render_scale() {
        let mut d = piece(1.0, 0.5, 2.0, None);
        d.align.pivot = Some((0.1, -0.2));
        d.align.y_offset = Some(0.06);
        bake_width(&mut d, 0.5).unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(
            d.extent.footprint,
            Some((0.5, 0.25)),
            "both axes, one ratio"
        );
        assert_eq!(d.extent.height, Some(1.0));
        // The mesh-geometry corrections are proportional to the mesh, so they resize with it.
        assert_eq!(d.align.pivot, Some((0.05, -0.1)));
        assert_eq!(d.align.y_offset, Some(0.03));
        let s = d
            .align
            .scale
            .unwrap_or_else(|| panic!("a resized piece carries its render scale"));
        assert!((s - 0.5).abs() < 1e-6, "{s}");
    }

    /// Unity is stored as absence. Resizing `books` back to its authored size composes
    /// 0.6 x 1.667 = 1.0, and the honest record of that is no scale at all — the rule `rotate_mesh`
    /// follows when it refuses to write an identity rotation.
    #[test]
    fn returning_to_the_authored_size_clears_the_scale() {
        // The shipped datum: books stands 0.306 m wide at scale 0.6 over a 0.5096 m mesh.
        let mut d = piece(0.306, 0.106, 0.178, Some(0.6));
        bake_width(&mut d, 0.51).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            d.align.scale, None,
            "0.6 composed with 0.51/0.306 is unity, and unity is stored as absence"
        );
        let (w, _) = d.extent.footprint.unwrap_or_default();
        assert!((w - 0.51).abs() < 1e-5, "{w}");
    }

    /// A width that cannot be a size is refused by name, not clamped — and a refusal touches
    /// nothing. A zero footprint overlaps nothing, so every placement rule would report success
    /// while the piece sat inside a wall.
    #[test]
    fn a_width_that_is_not_a_size_is_refused_and_changes_nothing() {
        let mut d = piece(1.0, 0.5, 2.0, None);
        let before = d.clone();
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                bake_width(&mut d, bad).is_err(),
                "{bad} must be refused, not stored"
            );
            assert_eq!(d, before, "{bad} must not have touched the piece");
        }
        // And an unmeasurable mesh cannot be resized at all.
        let mut vague = piece(1.0, 0.5, 2.0, None);
        vague.extent.footprint = None;
        assert!(bake_width(&mut vague, 0.6).is_err());
    }

    /// **The shipped scaled piece keeps its space answers.** `site/books`' extent is already the
    /// post-scale value every placement rule reads; the field must show it, and resizing must move
    /// it, without the render scale ever multiplying into a space answer.
    #[test]
    fn the_one_shipped_scaled_piece_reads_as_placed() {
        let d = piece(0.306, 0.106, 0.178, Some(0.6));
        assert_eq!(
            emerge_core::descriptor::placed_footprint(&d),
            Some((0.306, 0.106)),
            "the reservation is the extent — the scale is a render instruction, not a multiplier"
        );
    }
}

#[cfg(test)]
mod righting_tests {
    use super::*;

    /// **A turn count steps ninety degrees at a time, and two of them is upside down.**
    ///
    /// The count is what the labeler now sends (`vlm::NeedsTurn::turns`), and this is the whole of
    /// what it means: `bumped` is the only place `align.rotate` moves, so a wrong step here is a
    /// mesh stored at an orientation it does not have — the invariant `rotate_mesh` exists to hold.
    #[test]
    fn a_turn_count_steps_ninety_degrees_at_a_time() {
        assert_eq!(RotateAxis::X.bumped((0, 0, 0), 1), (90, 0, 0));
        assert_eq!(
            RotateAxis::X.bumped((0, 0, 0), 2),
            (180, 0, 0),
            "two quarter turns is the answer for a piece standing on its head"
        );
        assert_eq!(RotateAxis::Z.bumped((0, 0, 0), 3), (0, 0, 270));

        // It accumulates on what the descriptor already carries, and wraps at a full turn.
        assert_eq!(RotateAxis::X.bumped((270, 0, 0), 2), (90, 0, 0));
        assert_eq!(
            RotateAxis::X.bumped((90, 0, 0), 3),
            (0, 0, 0),
            "four quarters is where it started, which is why the gate refuses 4"
        );

        // And it never touches an axis it was not asked about.
        assert_eq!(RotateAxis::Y.bumped((90, 0, 270), 2), (90, 180, 270));
    }

    /// **The ceiling is two, and the second attempt is the one that has to be allowed.**
    ///
    /// An odd turn taken the wrong way round comes back as "still not upright" — the direction is
    /// not asked for (`vlm::NeedsTurn::turns`) — so a ceiling of one would leave every 3-turn piece
    /// unrighted and blame the model.
    #[test]
    fn the_righting_ceiling_leaves_room_for_the_correction() {
        assert_eq!(MAX_RIGHTINGS, 2);
    }
}

#[cfg(test)]
mod write_library_tests {
    use super::*;
    use emerge_core::descriptor::{Align, Descriptor, Extent};
    use emerge_core::library::{LIBRARY_VERSION, Library};
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
            // A test project stamps nothing; empty is the same state as a file with none in it.
            compositions: emerge_core::composition::Compositions::default(),
            root: dir.to_path_buf(),
            emerge_dir: dir.to_path_buf(),
            project_dir: dir.to_path_buf(),
            maps_dir: dir.join("maps"),
            // **One kit, itself.** Not empty: `merged_with` rebuilds the palette from these layers,
            // so a Project holding none would merge to nothing and the first import in a test would
            // look like it had deleted the library.
            kits: vec![emerge_core::kits::KitLayer {
                dir: dir.to_path_buf(),
                namespace: "test".to_owned(),
                measured: measured.clone(),
                library: library.clone(),
                policy: policy.clone(),
            }],
            lattice: emerge_core::kits::Lattice::default(),
            // `wall()`'s id carries no namespace, so the directory is what a tile is named after.
            namespace: dir
                .file_name()
                .map_or_else(|| "emerge".to_owned(), |n| n.to_string_lossy().into_owned()),
            library_path: dir.join("library.ron"),
            vocab: emerge_core::vocab::Vocabularies::default(),
            measured,
            library,
            policy,
            masks: Vec::new(),
            touched: Vec::new(),
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

        project.measured.descriptors[0]
            .lattice_mut()
            .set_solid((0, 0, 0), (6, 2, 1))
            .unwrap_or_else(|| panic!("in range"));
        write_library(&mut project).unwrap_or_else(|e| panic!("{e}"));

        let written =
            std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));
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
        assert!(
            project.library.get("crate_b").is_some(),
            "the derived palette"
        );
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
        let before =
            std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));

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
        assert!(
            project.library.get("wall").is_some(),
            "nor out of the derived layer"
        );
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

        let written =
            std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));
        let back = Library::parse(&written).unwrap_or_else(|e| panic!("{e}"));
        let reopened = project
            .policy
            .apply(&back)
            .unwrap_or_else(|e| panic!("{e}"));
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
        let before =
            std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));

        // The only `wall` is gone, so the rule about walls matches nothing.
        project.measured.descriptors.clear();
        let err = write_library(&mut project).err().unwrap_or_default();
        assert!(err.contains("matches no descriptor"), "{err}");

        let after =
            std::fs::read_to_string(dir.join("library.ron")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            before, after,
            "a refused write must not have touched the file"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
