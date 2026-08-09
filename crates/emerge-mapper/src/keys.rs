//! **The key census** — every binding this editor has, in one table, with a test.
//!
//! `docs/ui.md` §3.5 is unambiguous about why: key allocation in the game used to live in *five*
//! hand-written prose censuses, and *"all five of which had drifted to the same wrong answer — every
//! one named `T` as taken, long after the `T` hotkey was deleted."* The remedy there was to make
//! every binding data and add `the_key_space_has_no_collisions`. This is the same move for the
//! editor, made while it has twenty bindings rather than after it has sixty.
//!
//! # Some collisions are legal, so context is part of the model
//!
//! `M` steps the mount in tile configuration and means nothing on the map; `F` floods the map and
//! means nothing in tile configuration. Two actions may share a key when their contexts can never be
//! live at once — the same rule §3.5 states, and the same reason it gives: a flat uniqueness rule
//! *"would force a worse binding"*.
//!
//! [`Context::Typing`] is the one that overlaps everything. While a name or an id is being typed the
//! keyboard belongs to that text, and every other action is suppressed — the focus guard §3.5 gives
//! one home, because `research_room::editor` found the alternative the hard way when one `Space` both
//! clicked a focused button and toggled the pause.
//!
//! # Every key is stated where it is used
//!
//! Cockburn, Gutwin, Scarr & Malacria 2014 (`10.1145/2659796`) document the intermodal-transition
//! failure: offering a fast path beside a slow one *does not work on its own*, and users plateau on
//! the slow one. So a binding carries its own label and the panel renders it next to the thing it
//! does — §4.2's rule that *"verb chips are clickable and keyed, and each chip states its key."*
//!
//! Held to ~12 per context, which Zheng et al. 2018 (`10.1145/3173574.3173823`) found is learnable to
//! recall in about thirty minutes.

use bevy::prelude::*;

/// When an action can fire. Two actions may share a key iff their contexts never overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    /// Live in every tab — the frame around the editor.
    Global,
    /// The map tab.
    Map,
    /// The tile-configuration tab.
    Tiles,
    /// The animation bench.
    Anim,
    /// The composition tab — reusable groups, their derived interface, and what is stale.
    Compose,
    /// A text field is taking raw keys. Overlaps everything, and suppresses everything.
    Typing,
}

impl Context {
    /// Can these two be live at the same moment?
    pub fn overlaps(self, other: Context) -> bool {
        use Context::*;
        match (self, other) {
            // Typing shadows every other context by construction — that is what makes it the guard.
            (Typing, _) | (_, Typing) => true,
            (Global, _) | (_, Global) => true,
            (Map, Map) | (Tiles, Tiles) | (Anim, Anim) | (Compose, Compose) => true,
            // The three tabs are never live together — which is what lets them reuse each other's
            // letters freely. `the_key_space_has_no_collisions` polices that they only do so here.
            (Map, Tiles) | (Tiles, Map) => false,
            (Map, Anim) | (Anim, Map) | (Tiles, Anim) | (Anim, Tiles) => false,
            (Compose, _) | (_, Compose) => false,
        }
    }
}

/// Everything the editor can be asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    // ── Global ───────────────────────────────────────────────────────────────
    NextTab,
    MapTab,
    TilesTab,
    AnimTab,
    ComposeTab,
    /// Walk the composition list.
    ComposePrev,
    ComposeNext,
    /// Arm the selected composition on the map, so a click stamps it.
    ComposeArm,
    /// Write down what this group's members present now, so later drift is measurable.
    ComposeRecord,
    /// Walk the selected group's members. A second cursor, under the one that walks the groups.
    ComposeMemberPrev,
    ComposeMemberNext,
    /// **Seat the selected member** — one lattice step, in the frame `Member::at` is written in.
    ///
    /// Not a cursor that a verb is then applied to, which is what the Tiles lattice keys are. Here the
    /// movement *is* the verb, so the member is seen to move; Merrell's furniture layout calls that
    /// progressively pinning a layout down, and it is why seating is per-member rather than a solve.
    SeatForward,
    SeatLeft,
    SeatBack,
    SeatRight,
    SeatDown,
    SeatUp,
    /// **Put the selected member flush against an envelope face.** The verb a wall wants: its offset
    /// comes from its own measured thickness, not from a grid step it cannot land on.
    FlushForward,
    FlushLeft,
    FlushBack,
    FlushRight,
    /// Turn the selected member. A quarter bare, 15° on Shift — see the binding for why that way round.
    TurnMemberLeft,
    TurnMemberRight,
    TurnMemberLeftFine,
    TurnMemberRightFine,
    /// Take the selected member out of the group.
    DropMember,
    /// **Start a new group** on the Compose tab — an empty bounded tile, named as it is made.
    NewGroup,
    /// **Add a piece from the library** to the selected group. Opens a picker over the library; the
    /// walk keys and `Enter` belong to it while it is open.
    AddMember,
    /// Compose's own undo pair. Map, Tiles and Anim each keep one; an editing surface without one
    /// would be the odd tab out.
    UndoCompose,
    RedoCompose,
    Save,
    Undo,
    Redo,
    /// Hold to see this tab's key list.
    Shortcuts,
    /// Open the Tiles tab on the descriptor of the piece under the cursor.
    EditTile,
    // ── Map ──────────────────────────────────────────────────────────────────
    AimLeft,
    AimRight,
    /// Turn the placement under the cursor, as opposed to the brush.
    TurnPieceLeft,
    TurnPieceRight,
    /// Cycle which piece of the stack under the cursor the piece-verbs act on — coincident
    /// pieces made "the placement under the cursor" ambiguous, and a nudge aimed at a header
    /// moved the wall under it. Each press names the next piece, bottom to top; Esc releases.
    CycleTarget,
    /// A quarter turn tipping the piece under the cursor about the map's X axis — set dressing;
    /// see `Placed::tip` for why quarter turns and why a hosting piece refuses.
    TipX,
    /// The same quarter turn about Z.
    TipZ,
    /// Raise the piece under the cursor by one subgrid unit (`grid::SNAP / divisions`) — the
    /// authored `Placed::lift`, layered over the resolved height so stacking still follows.
    LiftUp,
    /// And back down.
    LiftDown,
    Fill,
    Remove,
    /// Arm the move tool: click a piece to pick it up, click again to put it down.
    MoveMode,
    RenameMap,
    /// Put the brush back to the rotation it was authored at.
    AimReset,
    /// Leave the removal mode without removing anything.
    Cancel,
    OwnToggle,
    Generate,
    /// Fill from what the kit's tiles DECLARE, rather than from what the map already shows.
    GenerateDeclared,
    /// Step the drawn grid's spacing. A view setting, not an edit — see `editor::GridSpacing`.
    CycleGrid,
    /// Keep the set in hand as a reusable group — see `editor::composition_from_set`.
    GroupFromSet,
    PanForward,
    PanBack,
    PanLeft,
    PanRight,
    TurnViewLeft,
    TurnViewRight,
    // ── Tiles ────────────────────────────────────────────────────────────────
    PrevCandidate,
    NextCandidate,
    TypeId,
    CycleMount,
    Accept,
    Rescan,
    RemoveTile,
    /// Send a library entry back to the candidate list, stripped — it re-enters as a freshly
    /// measured candidate with no tags, note or mount. Destructive, so it takes two presses:
    /// the first arms with a warning, the second sends.
    DemoteTile,
    /// Arm the clone tool: drag a box to take a copy of every placement inside it, then click to
    /// stamp the whole set somewhere else — as many times as it is held.
    CloneMode,
    /// The Tiles tab's own history. Separate from [`Action::Undo`] because the two stacks are
    /// separate — see `ImportState::undo`.
    UndoTile,
    RedoTile,
    // ── Tiles: the lattice ───────────────────────────────────────────────────
    // The subgrid was mouse-only, against §4.2's rule that everything reachable by mouse is
    // reachable by keyboard — a rule `tiles.rs` quotes at itself and then did not follow.
    CellLeft,
    CellRight,
    CellForward,
    CellBack,
    LayerDown,
    LayerUp,
    CellSolid,
    CellEdge,
    CellAnchor,
    CellClear,
    /// Mark every cell the mesh's geometry reaches. See `tiles::scan_mesh`.
    ScanMesh,
    /// Turn the mesh a quarter turn about one axis and re-measure it. See `tiles::rotate_mesh`.
    RotateMeshX,
    RotateMeshY,
    RotateMeshZ,
    /// Point the arrows at the candidate list.
    FocusCandidates,
    /// Copy the detail pane's text — top to bottom, as it reads — into the system clipboard.
    CopyInfo,
    /// Point them at the library list.
    FocusLibrary,
    /// Photograph the focused piece and ask the VLM for labels.
    SuggestLabels,
    /// Walk every piece missing judgement fields through the labeler — or cancel a running walk.
    SuggestAll,
    /// Apply the pending suggestion to the focused piece, through the ordinary edit path.
    ApplySuggestion,
    /// Drop the pending suggestion.
    DiscardSuggestion,
    /// Drop EVERY pending suggestion, cancel the batch, and abandon in-flight requests — the
    /// one-key way out of a labeling run that went wrong.
    DiscardAllSuggestions,
    // ── Anim ─────────────────────────────────────────────────────────────────
    PrevRig,
    NextRig,
    AdoptMeasured,
    UndoBench,
    RedoBench,
    ScrubBack,
    ScrubFwd,
    PlayPause,
    CheckAllRigs,
    /// Stage a translucent second figure playing the MEASURED gait numbers over the declared one.
    ToggleGhost,
    /// The stage camera's framing: figure / feet / side / ground.
    CycleCamPreset,
}

/// One binding: the key, when it is live, and how to say it.
pub struct Binding {
    pub action: Action,
    pub key: KeyCode,
    /// Whether the platform command modifier ([`MOD_KEYS`]) must be held. `false` is the bare key.
    pub needs_mod: bool,
    /// **What this binding asks of Shift**, and why it is three states rather than two.
    ///
    /// `None` — Shift is not part of this binding, and holding it changes nothing. This is what every
    /// row wants by default, and making it mean *"Shift must be up"* would have broken the one place
    /// that already reads Shift: `tiles::rotate_mesh` takes `Shift` as the override that turns a piece
    /// anyway and reports how many lattice cells it cleared, and it gets there through
    /// `just_pressed(RotateMeshX)`. A strict rule would have stopped `Shift+N` firing at all, deleting
    /// a documented escape hatch as a side effect of adding redo.
    ///
    /// `Some(true)` — Shift must be held. `Some(false)` — Shift must be up.
    ///
    /// Those two exist so `Cmd+Z` and `Shift+Cmd+Z` are different bindings rather than one that fires
    /// twice: undo declares `Some(false)`, redo `Some(true)`, and the collision test below knows they
    /// can never both be satisfied.
    pub needs_shift: Option<bool>,
    pub context: Context,
    /// How the BARE key reads in the panel — `S`, `Z`, `up`. The modifier is **not** written here:
    /// [`rows`] prepends [`MOD_NAME`] when `needs_mod`, so the one panel that shows this list says
    /// `Cmd+S` on a Mac and `Ctrl+S` elsewhere without a second census to keep in step.
    pub chord: &'static str,
    /// What it does, in the fewest words that stay true.
    pub does: &'static str,
}

/// The command modifier, per platform. **Cmd on a Mac, Ctrl everywhere else** — an editor that wanted
/// Ctrl+S on macOS would be the only application on the machine that did, and `Cmd+Z` silently doing
/// nothing is exactly how a working undo stack gets reported as broken.
#[cfg(target_os = "macos")]
pub const MOD_KEYS: [KeyCode; 2] = [KeyCode::SuperLeft, KeyCode::SuperRight];
#[cfg(not(target_os = "macos"))]
pub const MOD_KEYS: [KeyCode; 2] = [KeyCode::ControlLeft, KeyCode::ControlRight];

/// Shift, either side. Not platform-dependent — unlike [`MOD_KEYS`], Shift is Shift everywhere.
pub const SHIFT_KEYS: [KeyCode; 2] = [KeyCode::ShiftLeft, KeyCode::ShiftRight];

/// What [`MOD_KEYS`] is called in writing. The panel and every status line take the name from here so
/// no string in this crate can claim a modifier the build does not read.
#[cfg(target_os = "macos")]
pub const MOD_NAME: &str = "Cmd";
#[cfg(not(target_os = "macos"))]
pub const MOD_NAME: &str = "Ctrl";

/// The delete key, per platform. A Mac keyboard's `delete` sends **Backspace**; the key winit calls
/// `Delete` is the one macOS keyboards label `fn+delete` and most do not have at all — so binding
/// `Delete` there is binding a key the author cannot press.
///
/// **Tiles only.** The map's removal moved to `X` so the whole map vocabulary — pan, turn, aim,
/// remove — sits under the left hand without reaching. Removing a tile from the library is a rarer
/// and more destructive act in a tab whose letters are nearly all spoken for, and it keeps the key
/// whose name says what it does.
#[cfg(target_os = "macos")]
pub const REMOVE_KEY: KeyCode = KeyCode::Backspace;
#[cfg(not(target_os = "macos"))]
pub const REMOVE_KEY: KeyCode = KeyCode::Delete;

/// What [`REMOVE_KEY`] is called in writing, on the same rule as [`MOD_NAME`].
#[cfg(target_os = "macos")]
pub const REMOVE_NAME: &str = "Delete";
#[cfg(not(target_os = "macos"))]
pub const REMOVE_NAME: &str = "Del";

/// **The census.** Adding a binding means adding a row here; nothing else in this crate is allowed to
/// name a `KeyCode` for an action.
pub const BINDINGS: &[Binding] = &[
    b(Action::NextTab, KeyCode::Tab, false, Context::Global, "Tab", "next tab"),
    b(Action::MapTab, KeyCode::Digit1, false, Context::Global, "1", "map tab"),
    b(Action::TilesTab, KeyCode::Digit2, false, Context::Global, "2", "tiles tab"),
    // **The digits follow the strip, and the strip is `Mode::ALL`.** A number key that jumped to
    // the third tab while the third tab on screen was a different one would be the census
    // disagreeing with the thing it describes — which is the whole failure this file exists to
    // prevent, one layer up from key allocation.
    b(Action::ComposeTab, KeyCode::Digit3, false, Context::Global, "3", "compose tab"),
    b(Action::AnimTab, KeyCode::Digit4, false, Context::Global, "4", "animation tab"),
    // **The modified tab key: go there, and take this with you.**
    //
    // Beside `2` because it is the same destination with a subject — `Cmd+2` reads as "the Tiles tab,
    // about this piece", and an author who knows `2` has most of it already. It is a legal pair with
    // the bare `2` on the same rule `S`/`Cmd+S` and `Z`/`Cmd+Z` follow: `just_pressed` refuses a bare
    // binding while the modifier is down and a modified one while it is not.
    //
    // `Global` rather than `Map` because the Map context is at its twelve-row ceiling and this is a
    // navigation verb, which is what the rest of this block is. On the other two tabs there is simply
    // nothing under the cursor to send, and it says so.
    // **"this tile" named nothing an author could point at.** The row read `Cmd+2  edit this tile`,
    // in the Global block, which on the Map tab renders below eleven map rows inside an overlay you
    // have to hold `K` to see — so the verb was reported missing by someone looking straight at it.
    // Every other row in this census names its subject; this one said "this" and left the reader to
    // guess whether it meant the armed brush, the selected row, or what the mouse was over.
    b(Action::EditTile, KeyCode::Digit2, true, Context::Global, "2", "edit the piece under the cursor"),
    // **Held, not toggled**, and read with `keys::pressed`. The list is a thing you glance at with a
    // thumb down, not a mode you enter and have to leave — and a modal you can forget you opened is a
    // modal that eats the next keystroke.
    b(Action::Shortcuts, KeyCode::KeyK, false, Context::Global, "K", "hold for shortcuts"),
    b(Action::Save, KeyCode::KeyS, true, Context::Global, "S", "save"),
    // **The agent's read-out, and it is Global because a problem is.**
    //
    // It was `Context::Tiles`, copying that tab's detail pane. Every tab can now refuse, and a refusal
    // an agent cannot get out of the window is one that has to be retyped from a screenshot — bevy_ui
    // has no selectable text. So the verb follows the thing it is for: on any tab this copies what
    // that tab is showing, with the problem first. Leaving Tiles also gives that context back the row
    // it was over, which is what made `Esc` affordable below.
    //
    // Legal against the three bare `C` bindings (aim right, cell anchor, check all rigs) on the same
    // rule as `S`/`Cmd+S`: `just_pressed` refuses a bare binding while the modifier is down.
    b(Action::CopyInfo, KeyCode::KeyC, true, Context::Global, "C", "copy this tab's text"),
    // **One key for "not that", now on every tab** — and the problem banner is its outermost layer.
    //
    // It was `Context::Map`, where it peeled back one layer per press: a piece in hand, then the armed
    // tool, then the armed piece. A problem now sits outside all three, because a message that sticks
    // until something clears it needs something to clear it, and on the other three tabs there was no
    // key that meant "I have read that" at all.
    //
    // **Global, not four bindings.** `Action` must resolve to exactly one row
    // (`every_action_has_exactly_one_binding`), so a per-tab dismiss would be three more actions
    // spelling one idea. The map keeps its peel by handling the banner first and returning — the
    // layering the census promises, with one more layer on top.
    b(Action::Cancel, KeyCode::Escape, false, Context::Global, "Esc", "put back / stop / clear / dismiss"),
    // **Undo is the MAP's.** It was `Global`, and `keys` had lost its `in_map_mode` run condition, so
    // `Cmd+Z` on the Tiles tab silently despawned a flood fill — up to ~1,400 placements — while every
    // `MapRoot` panel was `Display::None` and nothing on screen changed. An undo you cannot see the
    // effect of is not an undo. Lattice edits go straight to `library.ron` and have no undo of their
    // own, which is exactly why the key is reached for there.
    // **One row, two chords.** A shared `does` collapses them the way `W, A, S, D` collapses, so
    // adding redo costs no row — which matters, because the Map context has none to give.
    bs(Action::Undo, KeyCode::KeyZ, true, false, Context::Map, "Z", "undo / redo"),
    bs(Action::Redo, KeyCode::KeyZ, true, true, Context::Map, "Z", "undo / redo"),

    // **Z and C turn the brush; X puts it back.** They sit under the left hand already resting on
    // WASD, which the brackets never did — and `Z` is free as a bare key precisely because the
    // modifier check above keeps `Cmd+Z` (undo) and `Z` (aim) apart rather than letting the chord
    // shadow the letter.
    // One ROW, deliberately: a shared `does` collapses the pair the way `W, A, S, D` collapses, and
    // the Map context is at its twelve-row ceiling (see the vocabulary test) with the removal mode's
    // `Esc` now in it. The label carries the direction so nothing is lost by sharing the line.
    // One row for all three aim keys — the shared `does` collapse that bought the target row
    // below inside the twelve-row ceiling.
    b(Action::AimLeft, KeyCode::KeyZ, false, Context::Map, "Z", "aim / straight again"),
    b(Action::AimRight, KeyCode::KeyC, false, Context::Map, "C", "aim / straight again"),
    b(Action::AimReset, KeyCode::KeyV, false, Context::Map, "V", "aim / straight again"),
    // **`H` picks WHICH piece of a stack the piece-verbs mean.** A floor tile, a wall and its
    // header legally share a cell (different layers), and "the placement under the cursor" cannot
    // name one of three. Each press steps up the stack, the status names the target, and the
    // verbs above act on it until the cursor leaves the cell or Esc releases.
    b(Action::CycleTarget, KeyCode::KeyH, false, Context::Map, "H", "target the stack"),
    // **A separate cluster from the aim keys, on purpose.** `Z`/`C` turn the BRUSH and must keep
    // doing only that: binding them to the selection is what made rotation feel broken before,
    // because placing selects, so the next press turned the piece just put down while the ghost —
    // the only thing on screen showing a facing — sat still. These turn what is under the cursor,
    // which is the other half nobody had.
    //
    // **One row, four chords, reading left to right on the keyboard**: `R T` yaw the piece the way
    // they always did, `Y U` tip it over — a quarter turn about X and about Z per press
    // (`Placed::tip`). Sharing a `does` collapses them the way `W, A, S, D` collapses, which is what
    // buys the lift row below inside the twelve-row ceiling.
    b(Action::TurnPieceLeft, KeyCode::KeyR, false, Context::Map, "R", "turn / tip this"),
    b(Action::TurnPieceRight, KeyCode::KeyT, false, Context::Map, "T", "turn / tip this"),
    b(Action::TipX, KeyCode::KeyY, false, Context::Map, "Y", "turn / tip this"),
    b(Action::TipZ, KeyCode::KeyU, false, Context::Map, "U", "turn / tip this"),
    // **The brackets lift.** Free in this context (the Tiles tab's layer pair is never live with
    // the Map), and vertically suggestive in a way no letter is. One subgrid unit per press, held
    // repeat for a long ride up; the authored offset is `Placed::lift`, the one amendment to
    // "height is never the author's".
    // **`J` steps the grid.** A setting rather than a verb, and the only free letter under the right
    // hand — it sits beside `H`, which is the other key about how you are reading the map rather
    // than about changing it. The kit's module is what an author counts in, and that is not the same
    // number as the snap: the site kit builds on 1 m while `grid::SNAP` is 0.5, so a grid fixed to
    // either one is wrong for somebody. This makes it theirs.
    b(Action::CycleGrid, KeyCode::KeyJ, false, Context::Map, "J", "grid: 0.5 / 1 / 2 / 4 m"),
    b(Action::LiftDown, KeyCode::BracketLeft, false, Context::Map, "[", "lift / lower this"),
    b(Action::LiftUp, KeyCode::BracketRight, false, Context::Map, "]", "lift / lower this"),
    b(Action::Fill, KeyCode::KeyF, false, Context::Map, "F", "flood fill"),
    b(Action::Remove, KeyCode::KeyX, false, Context::Map, "X", "removal mode"),
    // **`B` is the last free key under the left hand.** The cluster an author's hand already rests on
    // is `Q W E R T / A S D F G / Z X C V B`, and every other letter in it is spoken for — pan, turn
    // view, aim, aim-reset, turn-piece, fill, remove. `B` is bound in the Tiles tab too (`ScanMesh`),
    // which is legal and is exactly the case `Context` exists to model: the two tabs are never live
    // together.
    //
    // **The Map context is at its twelve-row ceiling.** There is no headroom left; the next verb
    // here has to share a `does` with a neighbour or take something else's key — which is exactly
    // what the clone tool does: the Cmd+Z shape, one key, the shifted form for the sibling verb.
    bs(Action::MoveMode, KeyCode::KeyB, false, false, Context::Map, "B", "move / clone a set / keep as a group"),
    bs(Action::CloneMode, KeyCode::KeyB, false, true, Context::Map, "B", "move / clone a set / keep as a group"),
    // **A third verb on a state that already exists.** `Shift+B` drags a box and leaves a set in
    // hand; this keeps that set as a reusable group instead of stamping it. Declared adjacent to the
    // pair above and sharing their `does`, so `rows()` collapses all three into one line — the Map
    // context is AT its twelve-row ceiling and `no_context_carries_more_than_a_learnable_vocabulary`
    // enforces it. `M` for module; it is one of four letters this context has left.
    b(Action::GroupFromSet, KeyCode::KeyM, false, Context::Map, "M", "move / clone a set / keep as a group"),
    b(Action::RenameMap, KeyCode::KeyN, false, Context::Map, "N", "rename map"),
    b(Action::OwnToggle, KeyCode::KeyO, false, Context::Map, "O", "pin / unpin"),
    // **Two sources, one row.** `rows()` collapses adjacent bindings sharing a `does` string, so the
    // Map context stays at its twelve-row ceiling while gaining a verb. The two are not a fallback
    // pair — each either produces a grammar or refuses by name.
    bs(Action::Generate, KeyCode::KeyG, false, false, Context::Map, "G", "continue the layout / Shift: from the kit's tokens"),
    bs(Action::GenerateDeclared, KeyCode::KeyG, false, true, Context::Map, "G", "continue the layout / Shift: from the kit's tokens"),

    // **The camera is Global — pan included.** This briefly moved to `Context::Map` to free
    // `W, A, S, D` for the Tiles lattice cursor, on the argument that panning off a staged tile has
    // no way back. That argument was wrong in practice: an author on any tab reaches for these keys
    // to move the view, and a tab where they silently do something else is a tab where the camera
    // feels broken. The lattice cursor moved instead — see its row.
    //
    // Declared W, A, S, D rather than W, S, A, D: the displayed row is these chords in order, and
    // "W, A, S, D" is how the shape is named everywhere. The census's order IS the reading order.
    b(Action::PanForward, KeyCode::KeyW, false, Context::Global, "W", "pan"),
    b(Action::PanLeft, KeyCode::KeyA, false, Context::Global, "A", "pan"),
    b(Action::PanBack, KeyCode::KeyS, false, Context::Global, "S", "pan"),
    b(Action::PanRight, KeyCode::KeyD, false, Context::Global, "D", "pan"),
    b(Action::TurnViewLeft, KeyCode::KeyQ, false, Context::Global, "Q", "turn view"),
    b(Action::TurnViewRight, KeyCode::KeyE, false, Context::Global, "E", "turn view"),

    // **One row for the whole arrow cluster** — up/down walk, left/right switch which list, and
    // holding Shift jumps five at a time. Four bindings sharing one `does` collapse the way
    // `W, A, S, D` collapses, which is what bought the copy row below inside the twelve-row
    // ceiling.
    b(Action::PrevCandidate, KeyCode::ArrowUp, false, Context::Tiles, "up", "walk the lists / Shift: x5"),
    b(Action::NextCandidate, KeyCode::ArrowDown, false, Context::Tiles, "down", "walk the lists / Shift: x5"),
    b(Action::FocusCandidates, KeyCode::ArrowLeft, false, Context::Tiles, "left", "walk the lists / Shift: x5"),
    b(Action::FocusLibrary, KeyCode::ArrowRight, false, Context::Tiles, "right", "walk the lists / Shift: x5"),
    b(Action::TypeId, KeyCode::KeyI, false, Context::Tiles, "I", "type an id"),
    // **"mount", not "layer".** It cycles `Descriptor::mount` — what the piece stands on — and the
    // subgrid below has its own `layer y` picker for the lattice slice. One panel said "layer" twice
    // and meant two different things.
    b(Action::CycleMount, KeyCode::KeyM, false, Context::Tiles, "M", "mount"),
    b(Action::Accept, KeyCode::Enter, false, Context::Tiles, "Enter", "add to library"),
    b(Action::Rescan, KeyCode::KeyR, false, Context::Tiles, "R", "rescan"),
    // The Cmd+Z shape again: one key, the shifted form for the reversible-but-destructive sibling.
    // Shift+Delete DEMOTES — back to the candidates, stripped — where bare Delete removes outright.
    bs(Action::RemoveTile, REMOVE_KEY, false, false, Context::Tiles, REMOVE_NAME, "remove / Shift: back to candidates"),
    bs(Action::DemoteTile, REMOVE_KEY, false, true, Context::Tiles, REMOVE_NAME, "remove / Shift: back to candidates"),
    // **This tab's own history**, on the same chords and for the reason `keys.rs`'s undo comment
    // records: an undo you cannot see the effect of is not an undo, so the map's stack is not reachable
    // from here and this one is not reachable from there. Neither is cleared by changing tabs.
    bs(Action::UndoTile, KeyCode::KeyZ, true, false, Context::Tiles, "Z", "undo / redo"),
    bs(Action::RedoTile, KeyCode::KeyZ, true, true, Context::Tiles, "Z", "undo / redo"),

    // **The lattice, by keyboard.** Two rows, which is what the twelve-row ceiling leaves once the
    // seven above and the labels row are counted — the cursor and the layer share one row (they are
    // one idea: where in the lattice), each reading its chords in order, the `W, A, S, D  pan` shape.
    //
    // **`T F G H` is an inverted T, one column left of the usual one.** The cursor cannot have
    // `W A S D` (the camera's, on every tab), nor the arrows (they walk the two lists), nor `H J K L`
    // (`K` is the shortcuts overlay). This is the nearest remaining cluster with the right *shape* —
    // T above, F left, G below, H right — and shape is what the hand remembers.
    // `Z, X, C, V` is the run under the left hand, free here because they are Map bindings and the
    // two tabs are never live together, which is the case `Context` exists to model.
    b(Action::CellForward, KeyCode::KeyT, false, Context::Tiles, "T", "cell cursor / layer"),
    b(Action::CellLeft, KeyCode::KeyF, false, Context::Tiles, "F", "cell cursor / layer"),
    b(Action::CellBack, KeyCode::KeyG, false, Context::Tiles, "G", "cell cursor / layer"),
    b(Action::CellRight, KeyCode::KeyH, false, Context::Tiles, "H", "cell cursor / layer"),
    b(Action::LayerDown, KeyCode::BracketLeft, false, Context::Tiles, "[", "cell cursor / layer"),
    b(Action::LayerUp, KeyCode::BracketRight, false, Context::Tiles, "]", "cell cursor / layer"),
    b(Action::CellSolid, KeyCode::KeyZ, false, Context::Tiles, "Z", "solid / edge / anchor / clear"),
    b(Action::CellEdge, KeyCode::KeyX, false, Context::Tiles, "X", "solid / edge / anchor / clear"),
    b(Action::CellAnchor, KeyCode::KeyC, false, Context::Tiles, "C", "solid / edge / anchor / clear"),
    b(Action::CellClear, KeyCode::KeyV, false, Context::Tiles, "V", "solid / edge / anchor / clear"),
    b(Action::ScanMesh, KeyCode::KeyB, false, Context::Tiles, "B", "from the mesh: rescan solid / turn x y z"),
    b(Action::RotateMeshX, KeyCode::KeyN, false, Context::Tiles, "N", "from the mesh: rescan solid / turn x y z"),
    b(Action::RotateMeshY, KeyCode::KeyO, false, Context::Tiles, "O", "from the mesh: rescan solid / turn x y z"),
    b(Action::RotateMeshZ, KeyCode::KeyP, false, Context::Tiles, "P", "from the mesh: rescan solid / turn x y z"),

    // **The VLM labeler's cluster** — one row, four verbs. `L` photographs the focused piece and
    // asks the model; `Shift+L` walks everything missing judgement fields (and cancels a running
    // walk); `U` applies the proposed labels through the ordinary edit path; `Y` discards them.
    // `L`/`U`/`Y` are unbound in Tiles and Global; the L pair is the Cmd+Z shape — one key, the
    // shifted form for the bigger sweep.
    bs(Action::SuggestLabels, KeyCode::KeyL, false, false, Context::Tiles, "L", "labels: suggest / all / apply / discard / clear all"),
    bs(Action::SuggestAll, KeyCode::KeyL, false, true, Context::Tiles, "L", "labels: suggest / all / apply / discard / clear all"),
    b(Action::ApplySuggestion, KeyCode::KeyU, false, Context::Tiles, "U", "labels: suggest / all / apply / discard / clear all"),
    bs(Action::DiscardSuggestion, KeyCode::KeyY, false, false, Context::Tiles, "Y", "labels: suggest / all / apply / discard / clear all"),
    bs(Action::DiscardAllSuggestions, KeyCode::KeyY, false, true, Context::Tiles, "Y", "labels: suggest / all / apply / discard / clear all"),

    // The arrows are the Tiles tab's too. Legal, and the reason the census models context at all:
    // the two tabs are never live together, so the same key means one thing in each.
    b(Action::PrevRig, KeyCode::ArrowUp, false, Context::Anim, "up", "walk the rigs / Shift: x5"),
    b(Action::NextRig, KeyCode::ArrowDown, false, Context::Anim, "down", "walk the rigs / Shift: x5"),

    // Enter is the Tiles tab's Accept too — same legal cross-context share as the arrows above.
    b(Action::AdoptMeasured, KeyCode::Enter, false, Context::Anim, "Enter", "adopt measured values into rigs.ron"),

    // ── Compose ──────────────────────────────────────────────────────────────────────────────────
    //
    // **Deliberately reusing Tiles' and Anim's letters.** The three tabs are never live at once, and
    // `Context::overlaps` says so, so `up`/`down`/`Enter` mean walk-walk-commit in all three rather
    // than being three arbitrary triples an author has to learn apart. A flat uniqueness rule would
    // force a worse binding here, which is the whole reason contexts exist.
    b(Action::ComposePrev, KeyCode::ArrowUp, false, Context::Compose, "up", "walk the groups"),
    b(Action::ComposeNext, KeyCode::ArrowDown, false, Context::Compose, "down", "walk the groups"),
    b(Action::ComposeArm, KeyCode::Enter, false, Context::Compose, "Enter", "arm this group — the map tab stamps it"),
    b(Action::ComposeRecord, KeyCode::KeyR, false, Context::Compose, "R", "record what this group's members present now"),
    // Symmetric with the pair above: `up`/`down` walk the groups, `left`/`right` walk the members of
    // the one you are on. Costs no letter, and the two cursors read as one idea.
    b(Action::ComposeMemberPrev, KeyCode::ArrowLeft, false, Context::Compose, "left", "walk this group's members"),
    b(Action::ComposeMemberNext, KeyCode::ArrowRight, false, Context::Compose, "right", "walk this group's members"),
    // **The Tiles lattice cluster, on the other lattice.** `Context` overlaps by design, so one hand
    // shape means one thing on both surfaces rather than colliding. Declared adjacent to `[` and `]`
    // and sharing their `does`, so `rows()` collapses all six into one row.
    bs(Action::SeatForward, KeyCode::KeyT, false, false, Context::Compose, "T", "seat this member / raise"),
    bs(Action::SeatLeft, KeyCode::KeyF, false, false, Context::Compose, "F", "seat this member / raise"),
    bs(Action::SeatBack, KeyCode::KeyG, false, false, Context::Compose, "G", "seat this member / raise"),
    bs(Action::SeatRight, KeyCode::KeyH, false, false, Context::Compose, "H", "seat this member / raise"),
    b(Action::SeatDown, KeyCode::BracketLeft, false, Context::Compose, "[", "seat this member / raise"),
    b(Action::SeatUp, KeyCode::BracketRight, false, Context::Compose, "]", "seat this member / raise"),
    // **Flush to a face, which is the verb a wall actually wants.**
    //
    // Step 4 measured why: `site/wall` is 0.1 m thick, so seating it flush inside a 1 m tile puts it
    // at 0.45 — not a multiple of `grid::SNAP`, and so unreachable by the lattice keys above however
    // many times you press them. A uniform grid is the right primitive for furniture and the wrong
    // one for architecture; this is the relative split value to those absolute ones (Muller et al.,
    // CGA Shape, 10.1145/1179352.1141931), and Tutenel's "snapping to the nearest valid location".
    //
    // Its own row rather than the seat row's: ten chords collapsed into one line stops being a row
    // and starts being a paragraph.
    bs(Action::FlushForward, KeyCode::KeyT, false, true, Context::Compose, "T", "flush to that face"),
    bs(Action::FlushLeft, KeyCode::KeyF, false, true, Context::Compose, "F", "flush to that face"),
    bs(Action::FlushBack, KeyCode::KeyG, false, true, Context::Compose, "G", "flush to that face"),
    bs(Action::FlushRight, KeyCode::KeyH, false, true, Context::Compose, "H", "flush to that face"),
    // **A quarter bare, 15° on Shift — and that order is the argument, not a preference.**
    //
    // A group is a tile. `adjacency::quarter_turns` refuses a yaw that is not a multiple of 90 and
    // names the piece, so a member carrying edge tokens turned 45° makes the whole group's interface
    // underivable. It only bites a tokened member — `interface` skips the rest — so 15° stays
    // reachable for a chair drawn up to a table, behind a modifier where it cannot be hit by accident.
    //
    // `Y`/`U` rather than the Map's `R`/`T`, which are both spoken for here — `R` records and `T`
    // seats. They keep the whole surface under one hand (`T F G H` seat, `Y U` turn, `[ ]` raise), and
    // the Map's own row for `Y`/`U` is "turn / tip this", so the family is the same one.
    //
    // **Not `,` and `.`, and the test is why.** `rows()` joins a collapsed row's chords with `", "`,
    // so a chord that *is* a comma comes out as `, , .` and cannot be read back —
    // `collapsing_rows_loses_nothing` failed on exactly that, naming the vanished chord.
    bs(Action::TurnMemberLeft, KeyCode::KeyY, false, false, Context::Compose, "Y", "turn a quarter / Shift: 15"),
    bs(Action::TurnMemberRight, KeyCode::KeyU, false, false, Context::Compose, "U", "turn a quarter / Shift: 15"),
    bs(Action::TurnMemberLeftFine, KeyCode::KeyY, false, true, Context::Compose, "Y", "turn a quarter / Shift: 15"),
    bs(Action::TurnMemberRightFine, KeyCode::KeyU, false, true, Context::Compose, "U", "turn a quarter / Shift: 15"),
    b(Action::DropMember, REMOVE_KEY, false, Context::Compose, REMOVE_NAME, "drop this member"),
    // **The two verbs this tab was missing.** It could refine a group and not make one, so every
    // group had to be captured on the Map first — which is a fine way to work and a bad only way.
    b(Action::NewGroup, KeyCode::KeyN, false, Context::Compose, "N", "new group"),
    b(Action::AddMember, KeyCode::KeyM, false, Context::Compose, "M", "add a piece from the library"),
    // One row, two chords — the Map, Tiles and Anim pairs again.
    bs(Action::UndoCompose, KeyCode::KeyZ, true, false, Context::Compose, "Z", "undo / redo"),
    bs(Action::RedoCompose, KeyCode::KeyZ, true, true, Context::Compose, "Z", "undo / redo"),
    // One row, two chords, same as the Map and Tiles undo pairs.
    bs(Action::UndoBench, KeyCode::KeyZ, true, false, Context::Anim, "Z", "undo / redo the last write"),
    bs(Action::RedoBench, KeyCode::KeyZ, true, true, Context::Anim, "Z", "undo / redo the last write"),

    // The staged figure's phase scrub. Left/Right are held keys (`pressed`, like panning), Shift
    // slows the sweep — read in the handler, the `rotate_mesh` precedent, so `needs_shift` stays
    // `None` and the escape hatch keeps working.
    b(Action::ScrubBack, KeyCode::ArrowLeft, false, Context::Anim, "left", "scrub phase (Shift: fine)"),
    b(Action::ScrubFwd, KeyCode::ArrowRight, false, Context::Anim, "right", "scrub phase (Shift: fine)"),
    b(Action::PlayPause, KeyCode::Space, false, Context::Anim, "Space", "play / scrub"),
    b(Action::CheckAllRigs, KeyCode::KeyC, false, Context::Anim, "C", "check all rigs"),
    // G is the Map tab's Generate and a Tiles cell-cursor key — the same legal cross-context
    // share as the arrows above. V is the Map tab's AimReset and a Tiles lattice key.
    b(Action::ToggleGhost, KeyCode::KeyG, false, Context::Anim, "G", "ghost: measured over declared"),
    b(Action::CycleCamPreset, KeyCode::KeyV, false, Context::Anim, "V", "view: figure / feet / side / ground"),
];

const fn b(
    action: Action,
    key: KeyCode,
    needs_mod: bool,
    context: Context,
    chord: &'static str,
    does: &'static str,
) -> Binding {
    Binding {
        action,
        key,
        needs_mod,
        // Indifferent to Shift — see [`Binding::needs_shift`] for why that is the default.
        needs_shift: None,
        context,
        chord,
        does,
    }
}

/// A row that cares about Shift. `shift` is `true` for "must be held", `false` for "must be up".
const fn bs(
    action: Action,
    key: KeyCode,
    needs_mod: bool,
    shift: bool,
    context: Context,
    chord: &'static str,
    does: &'static str,
) -> Binding {
    Binding {
        action,
        key,
        needs_mod,
        needs_shift: Some(shift),
        context,
        chord,
        does,
    }
}

/// The binding for an action.
///
/// # The last resort is a real row, so only a test can tell you it was reached
///
/// There is no infallible way to write this: `Action` has no derive that enumerates it, so the lookup
/// can miss, and returning `Option` would push a "what do I show instead" decision onto every label
/// in the editor. It therefore returns `BINDINGS[0]` — Tab — and **that is indistinguishable from a
/// correct answer by looking at the chord or the description**, which is exactly how a missing row
/// went unnoticed: `every_action_resolves_to_its_own_binding_at_runtime` asserted that the returned
/// row had a non-empty chord and description, and `BINDINGS[0]` has both.
///
/// The only field that gives it away is `action`, so both guards now compare that: the test above
/// per action, and `every_action_has_exactly_one_binding` below over the whole table.
pub fn binding(action: Action) -> &'static Binding {
    BINDINGS
        .iter()
        .find(|b| b.action == action)
        .unwrap_or(&BINDINGS[0])
}

/// The chord for an action, for putting next to the control that does it.
pub fn chord(action: Action) -> &'static str {
    binding(action).chord
}

/// Everything live in one context, in declaration order — never sorted, never reordered by use.
///
/// Samp 2011, via `docs/ui.md` §3.5: a menu's cost is paid at first sight, so **fix item positions
/// permanently and never reorder by recency**. A key list that rearranges itself is one nobody can
/// build a memory of.
pub fn in_context(context: Context) -> impl Iterator<Item = &'static Binding> {
    BINDINGS.iter().filter(move |b| b.context == context)
}

/// How a binding READS: the bare key, or the platform modifier joined to it.
///
/// **The only place a chord becomes text.** The panel, the verb chips and the tests all come through
/// here, so `Cmd+S` cannot appear in one and `Ctrl+S` in another — the drift `docs/ui.md` §3.5 records
/// is exactly this, a second place that renders the same fact.
pub fn chord_text(b: &Binding) -> String {
    let base = if b.needs_mod {
        format!("{MOD_NAME}+{}", b.chord)
    } else {
        b.chord.to_owned()
    };
    // Only a *required* Shift is written. `Some(false)` is a rule about what must not be held, which
    // is not something a key list should ask a reader to carry.
    if b.needs_shift == Some(true) {
        format!("Shift+{base}")
    } else {
        base
    }
}

/// Is Shift down?
pub fn shift_held(keys: &ButtonInput<KeyCode>) -> bool {
    SHIFT_KEYS.iter().any(|k| keys.pressed(*k))
}

/// Does this binding's Shift requirement hold right now? `None` is always satisfied.
fn shift_ok(b: &Binding, keys: &ButtonInput<KeyCode>) -> bool {
    match b.needs_shift {
        None => true,
        Some(want) => want == shift_held(keys),
    }
}

/// One displayed row: the chords, and what they do.
pub struct Row {
    pub chord: String,
    pub does: &'static str,
}

/// The key list as it should be READ, which is not one row per binding.
///
/// `W`, `S`, `A` and `D` are four bindings and one idea. Listing them separately produced four
/// consecutive rows all saying "pan" — sixteen rows where twelve carry the same information, and
/// `docs/ui.md` §1.2 names over-informing as the failure mode. Adjacent bindings sharing a `does`
/// collapse into one row with their chords joined.
///
/// **Adjacent**, not grouped-by-label: order is declaration order and stays that way. Samp 2011, via
/// §3.5 — fix item positions permanently and never reorder, because a list that rearranges itself is
/// one nobody can build a memory of.
pub fn rows(context: Context) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for b in in_context(context) {
        match out.last_mut() {
            Some(last) if last.does == b.does => {
                // Comma-separated: `W, A, S, D` reads as a set of keys, `W A S D` reads as a sequence
                // to press in order.
                //
                // **The RENDERED chord**, not the bare field. This pushed `b.chord` — fine while
                // every collapsed row was bare letters, and wrong the moment two rows share a `does`
                // and differ by a modifier: `Cmd+Z` and `Shift+Cmd+Z` collapsed to `Cmd+Z, Z`, which
                // names a key that does not do that. `chord_text` is the one place a chord becomes
                // text, and this was the one place that went around it.
                last.chord.push_str(", ");
                last.chord.push_str(&chord_text(b));
            }
            _ => out.push(Row {
                chord: chord_text(b),
                does: b.does,
            }),
        }
    }
    out
}

/// **Sense who owns the keyboard, let the fields have it, then dispatch** — in that order, once a
/// frame.
///
/// Two measured defects shaped this, and they pull in opposite directions.
///
/// **The one that wrote to disk.** `tiles::cell_keys` and `tiles::commit_candidate` both took
/// `ResMut<ImportState>` in one *unordered* tuple, so Bevy was free to run the text field first. It
/// called `edit.active.take()`, the `not_typing` run condition re-evaluated to **true** in the same
/// frame, `Enter` was still `just_pressed`, and finishing an edge token *also* imported the candidate
/// into `assets/emerge/library.ron`. Six descriptors arrived there that way, and
/// `docs/2026-08-04-emerge-mapper-handoff.md` blamed stray automation. The fix is [`Live`]: decided
/// **once**, in [`Phase::Sense`], so no system can observe a keyboard that changed owner halfway
/// through its own frame. It is what makes `Enter` safe regardless of the order below.
///
/// **The one that typed a letter nobody meant.** With the fields running *after* the dispatchers, the
/// `X` that opened the edge token field was still in that frame's `KeyboardInput` stream when the
/// now-open field read it — the first authored token in this repo came out as `xseam`. So the fields
/// go **first**: on the frame a key opens one, the field is still shut when it drains the stream, and
/// the keystroke that opened it is discarded rather than entered. Which is also what
/// [`Context::Typing`] means in words — the text has the keyboard before anything else does.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Decide who owns the keyboard.
    Sense,
    /// Feed raw keystrokes to whichever field owns them.
    Text,
    /// Dispatch census actions.
    Act,
}

/// Installs [`Live`] and orders the three phases. **Added first**, because a missing `Res<T>` panics
/// its system in Bevy 0.19 rather than skipping it (`CLAUDE.md`), and `Live` is read from three
/// plugins — so no one of them can be its owner.
pub struct KeysPlugin;

impl Plugin for KeysPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Live>()
            .init_resource::<Repeat>()
            // `chain()` makes the sets run in the order listed
            // (`bevy-0.19.0/examples/ecs/ecs_guide.rs:330`).
            .configure_sets(Update, (Phase::Sense, Phase::Text, Phase::Act).chain())
            .add_systems(Update, crate::editor::sense_context.in_set(Phase::Sense));
    }
}

/// Who owns the keyboard this frame. Written once, in [`Phase::Sense`], and read everywhere else.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Live(pub Context);

impl Default for Live {
    fn default() -> Self {
        // The tab the editor opens on — `tiles::Mode::default()` is `Map`. Stated rather than
        // derived, because `Context`'s own first variant is `Global`, which is not a tab.
        Live(Context::Map)
    }
}

/// Which context owns the keyboard: the live tab, unless a field is taking raw keys.
pub fn live(tab: Context, typing: bool) -> Context {
    if typing {
        Context::Typing
    } else {
        tab
    }
}

/// May an action bound to `want` fire, given who owns the keyboard?
///
/// **[`Context::overlaps`] alone is the wrong predicate for dispatch.** Its `(Typing, _) => true` arm
/// says Typing is live *alongside* everything — which is what makes it the right answer for the
/// collision test, and the exact inverse of what dispatch needs. Used naively, every binding in the
/// census would fire *while* you type. So the suppression is stated once, here, and `overlaps` keeps
/// doing only the tab-exclusion half it was written for.
pub fn fires_in(want: Context, live: Context) -> bool {
    live != Context::Typing && want.overlaps(live)
}

/// Is the command modifier down?
///
/// **One modifier per platform, not two that both work.** A build accepting Ctrl *and* Cmd would give
/// every chord two spellings, and the panel could only name one of them — so the key list would be
/// wrong for whichever half of the users pressed the other. This is the same one-path rule the rest of
/// the project holds to: the modifier IS `MOD_KEYS`, and [`MOD_NAME`] is what it is called.
pub fn mod_held(keys: &ButtonInput<KeyCode>) -> bool {
    MOD_KEYS.iter().any(|k| keys.pressed(*k))
}

/// Was this action just pressed, in a context where it is allowed to fire?
///
/// The one place a `KeyCode` meets `ButtonInput`. Callers name an `Action` and pass the live context,
/// which is what stops the census drifting from the code the way `docs/ui.md` §3.5 records it drifting
/// five ways at once.
///
/// **The context is a parameter rather than a run condition** so that there is exactly one gate. A
/// system gated from outside still has to be gated correctly by every future caller; a function that
/// cannot answer without being told who owns the keyboard cannot be called wrongly. The five
/// `if *mode != Mode::Tiles` early returns this replaced were that second census.
pub fn just_pressed(keys: &ButtonInput<KeyCode>, live: Context, action: Action) -> bool {
    let b = binding(action);
    if !fires_in(b.context, live) {
        return false;
    }
    // A bare binding must not fire while the modifier is held, or `Cmd+S` would also pan the camera
    // back — and `Cmd+Z` would turn the brush as well as undo, now that `Z` aims.
    if b.needs_mod != mod_held(keys) || !shift_ok(b, keys) {
        return false;
    }
    keys.just_pressed(b.key)
}

/// Is this action's key held? For the continuous ones — panning.
pub fn pressed(keys: &ButtonInput<KeyCode>, live: Context, action: Action) -> bool {
    let b = binding(action);
    if !fires_in(b.context, live) {
        return false;
    }
    if b.needs_mod != mod_held(keys) || !shift_ok(b, keys) {
        return false;
    }
    keys.pressed(b.key)
}

/// **How long a held key waits before firing again**, and then between repeats.
///
/// Between the two things a repeat can get wrong. Faster and a tap starts becoming two steps,
/// because a deliberate tap is rarely under about 120 ms; slower and holding is not worth doing.
/// At the aim keys' [`crate::editor::YAW_STEP`] this sweeps a full turn in about 3.5 seconds.
pub const REPEAT_SECS: f32 = 0.150;

/// Per-action countdown to the next repeat, for [`repeating`].
///
/// A `Vec` rather than a map because [`Action`] is `Eq` but not `Hash`, and because the list only
/// ever holds the keys actually down — at most a couple.
#[derive(Resource, Default)]
pub struct Repeat(Vec<(Action, f32)>);

/// **Fires on the press, then every [`REPEAT_SECS`] for as long as the key is held.**
///
/// The press always fires immediately, so tapping behaves exactly as [`just_pressed`] did and only
/// holding is new. That ordering matters: an author who taps is not waiting on a timer, and one who
/// holds gets the first step at once and the rest at a readable pace.
///
/// A key held across a change of context does **not** resume repeating — it has no countdown, so it
/// waits for a fresh press. Otherwise switching tabs with a finger down would fire an action in a
/// context the author never pressed it in.
pub fn repeating(
    keys: &ButtonInput<KeyCode>,
    live: Context,
    action: Action,
    repeat: &mut Repeat,
    dt: f32,
) -> bool {
    if !pressed(keys, live, action) {
        repeat.0.retain(|(a, _)| *a != action);
        return false;
    }
    if just_pressed(keys, live, action) {
        repeat.0.retain(|(a, _)| *a != action);
        repeat.0.push((action, REPEAT_SECS));
        return true;
    }
    let Some((_, left)) = repeat.0.iter_mut().find(|(a, _)| *a == action) else {
        return false;
    };
    *left -= dt;
    if *left <= 0.0 {
        // Add rather than reset, so a long frame does not silently swallow the overshoot and drift
        // the cadence slower than it says it is.
        *left += REPEAT_SECS;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The census is complete.** Every `Action` has exactly one row, so `binding` cannot silently
    /// fall through to the wrong one.
    #[test]
    fn every_action_has_exactly_one_binding() {
        let actions = [
            Action::NextTab, Action::MapTab, Action::TilesTab, Action::AnimTab,
            Action::ComposeTab, Action::ComposePrev, Action::ComposeNext, Action::ComposeArm,
            Action::ComposeRecord, Action::ComposeMemberPrev, Action::ComposeMemberNext,
            Action::SeatForward, Action::SeatLeft, Action::SeatBack, Action::SeatRight,
            Action::SeatDown, Action::SeatUp,
            Action::FlushForward, Action::FlushLeft, Action::FlushBack, Action::FlushRight,
            Action::TurnMemberLeft, Action::TurnMemberRight,
            Action::TurnMemberLeftFine, Action::TurnMemberRightFine,
            Action::DropMember, Action::UndoCompose, Action::RedoCompose,
            Action::NewGroup, Action::AddMember,
            Action::Save, Action::Undo, Action::Redo, Action::Shortcuts, Action::EditTile,
            Action::AimLeft, Action::AimRight, Action::AimReset, Action::Cancel,
            Action::Fill, Action::Remove, Action::MoveMode, Action::CloneMode, Action::RenameMap,
            Action::OwnToggle, Action::Generate, Action::GenerateDeclared, Action::CycleGrid,
            Action::GroupFromSet,
            Action::PanForward, Action::PanBack, Action::PanLeft, Action::PanRight,
            Action::TurnViewLeft, Action::TurnViewRight,
            Action::PrevCandidate, Action::NextCandidate, Action::TypeId, Action::CycleMount,
            Action::Accept, Action::Rescan, Action::RemoveTile, Action::DemoteTile,
            Action::UndoTile, Action::RedoTile,
            Action::CellLeft, Action::CellRight, Action::CellForward, Action::CellBack,
            Action::LayerDown, Action::LayerUp,
            Action::CellSolid, Action::CellEdge, Action::CellAnchor, Action::CellClear,
            Action::ScanMesh,
            Action::RotateMeshX, Action::RotateMeshY, Action::RotateMeshZ,
            Action::FocusCandidates, Action::FocusLibrary, Action::CopyInfo,
            Action::SuggestLabels, Action::SuggestAll,
            Action::ApplySuggestion, Action::DiscardSuggestion, Action::DiscardAllSuggestions,
            Action::PrevRig, Action::NextRig,
            Action::AdoptMeasured, Action::UndoBench, Action::RedoBench,
            Action::ScrubBack, Action::ScrubFwd, Action::PlayPause, Action::CheckAllRigs,
            Action::ToggleGhost, Action::CycleCamPreset,
            Action::TurnPieceLeft, Action::TurnPieceRight,
            Action::TipX, Action::TipZ, Action::LiftUp, Action::LiftDown, Action::CycleTarget,
        ];
        assert_eq!(
            actions.len(),
            BINDINGS.len(),
            "the action list and the binding table disagree — one of them gained a row alone"
        );
        // **Every row is reachable through `binding`.** A duplicate row is shadowed by `find`, which
        // returns the first match — so the count check above can pass while half the table is dead.
        for b in BINDINGS {
            assert_eq!(
                binding(b.action).action, b.action,
                "`{}` ({}) is in the table but `binding` does not return it",
                b.chord, b.does
            );
        }
        for a in actions {
            assert_eq!(
                BINDINGS.iter().filter(|b| b.action == a).count(),
                1,
                "{a:?} does not have exactly one binding"
            );
        }
    }

    /// **The key space has no collisions**, in the sense `docs/ui.md` §3.5 means: two actions may
    /// share a key only when their contexts can never be live together.
    ///
    /// This is the test whose absence let five prose censuses drift to the same wrong answer.
    #[test]
    fn the_key_space_has_no_collisions() {
        let mut clashes = Vec::new();
        for (i, a) in BINDINGS.iter().enumerate() {
            for b in BINDINGS.iter().skip(i + 1) {
                // Two rows that ask opposite things of Shift can never both fire, so they are not a
                // collision — that is what makes `Cmd+Z` and `Shift+Cmd+Z` two bindings on one key.
                let shift_exclusive = matches!(
                    (a.needs_shift, b.needs_shift),
                    (Some(x), Some(y)) if x != y
                );
                if a.key == b.key
                    && a.needs_mod == b.needs_mod
                    && !shift_exclusive
                    && a.context.overlaps(b.context)
                {
                    clashes.push(format!(
                        "{:?} ({:?}) and {:?} ({:?}) both take `{}`",
                        a.action, a.context, b.action, b.context, a.chord
                    ));
                }
            }
        }
        assert!(
            clashes.is_empty(),
            "{} key collision(s):\n  {}",
            clashes.len(),
            clashes.join("\n  ")
        );
    }

    /// The legal collisions, asserted directly so nobody "fixes" them: `S` pans the map and the
    /// modified `S` saves; `Z` aims the brush left and the modified `Z` undoes. Different chords
    /// rather than clashes — and the second pair is the one that makes `Z` safe to bind at all.
    #[test]
    fn a_bare_key_and_its_modified_chord_are_different_bindings() {
        for (bare, modified) in [
            (Action::PanBack, Action::Save),
            (Action::AimLeft, Action::Undo),
        ] {
            let bare = binding(bare);
            let modified = binding(modified);
            assert_eq!(bare.key, modified.key);
            assert!(!bare.needs_mod && modified.needs_mod);
        }
    }

    /// Map and tile contexts can never be live together, which is what lets them share letters.
    #[test]
    fn the_two_tabs_do_not_overlap() {
        assert!(!Context::Map.overlaps(Context::Tiles));
        assert!(!Context::Anim.overlaps(Context::Map));
        assert!(!Context::Anim.overlaps(Context::Tiles));
        assert!(Context::Global.overlaps(Context::Anim));
        assert!(Context::Global.overlaps(Context::Map));
        assert!(Context::Global.overlaps(Context::Tiles));
        // Typing shadows everything — that is the focus guard.
        assert!(Context::Typing.overlaps(Context::Map));
        assert!(Context::Typing.overlaps(Context::Global));
    }

    /// ~12 per context, counted as ROWS — what a reader actually sees. Zheng et al. 2018 found a
    /// vocabulary of about a dozen is learnable to recall in three ten-minute sessions; past that a
    /// list stops being memorised and starts being read.
    #[test]
    fn no_context_carries_more_than_a_learnable_vocabulary() {
        for context in [
            Context::Global,
            Context::Map,
            Context::Tiles,
            Context::Anim,
            // **Compose was missing from both of these lists**, so the fourth tab's rows were
            // neither counted against the ceiling nor checked for surviving the collapse. A tab
            // absent from the test that polices the tabs is a tab the policing does not reach.
            Context::Compose,
        ] {
            let n = rows(context).len();
            assert!(
                n <= 12,
                "{context:?} shows {n} rows; past about a dozen a key list stops being learnable and \
                 starts being a reference card (docs/ui.md §3.5, Zheng et al. 2018)"
            );
        }
    }

    /// Four keys, one idea. The displayed list collapses them rather than repeating the word.
    ///
    /// The camera's two collapsed rows are Global, so they read the same on every tab.
    #[test]
    fn keys_that_do_one_thing_share_a_row() {
        let map = rows(Context::Map);
        let global = rows(Context::Global);
        let pan = global
            .iter()
            .find(|r| r.does == "pan")
            .unwrap_or_else(|| panic!("no pan row"));
        assert_eq!(pan.chord, "W, A, S, D");
        assert_eq!(global.iter().filter(|r| r.does == "pan").count(), 1);

        let turn = rows(Context::Global)
            .into_iter()
            .find(|r| r.does == "turn view")
            .unwrap_or_else(|| panic!("no turn row"));
        assert_eq!(turn.chord, "Q, E");

        // All three aim keys on one row — the merge that bought the target-lock row its slot.
        let aim = map
            .iter()
            .find(|r| r.does == "aim / straight again")
            .unwrap_or_else(|| panic!("no aim row"));
        assert_eq!(aim.chord, "Z, C, V");

        // The lattice cursor is its own cluster, and must not be the camera's — an author reaches
        // for `W A S D` to move the view on every tab. The layer keys share its row: one idea,
        // "where in the lattice", merged when the labels row needed the twelfth slot.
        let cursor = rows(Context::Tiles)
            .into_iter()
            .find(|r| r.does == "cell cursor / layer")
            .unwrap_or_else(|| panic!("no cursor row"));
        assert_eq!(cursor.chord, "T, F, G, H, [, ]");

        // The labeler's five verbs read as one row, the shifted forms rendered as such.
        let labels = rows(Context::Tiles)
            .into_iter()
            .find(|r| r.does == "labels: suggest / all / apply / discard / clear all")
            .unwrap_or_else(|| panic!("no labels row"));
        assert_eq!(labels.chord, "L, Shift+L, U, Y, Shift+Y");
    }

    /// **The overlay key is held, not tapped.** `pressed` must answer for it while it is down —
    /// `just_pressed` is true for one frame, which would make the list flicker rather than show.
    #[test]
    fn the_shortcuts_key_reads_as_held() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(binding(Action::Shortcuts).key);
        for tab in [Context::Map, Context::Tiles, Context::Anim] {
            assert!(
                pressed(&input, tab, Action::Shortcuts),
                "the shortcuts overlay must be reachable from {tab:?}"
            );
        }
        assert!(
            !pressed(&input, Context::Typing, Action::Shortcuts),
            "and must not open while a field is taking keys"
        );
    }

    /// Collapsing must not lose a binding — every one still appears in exactly one row.
    #[test]
    fn collapsing_rows_loses_nothing() {
        for context in [
            Context::Global,
            Context::Map,
            Context::Tiles,
            Context::Anim,
            // **Compose was missing from both of these lists**, so the fourth tab's rows were
            // neither counted against the ceiling nor checked for surviving the collapse. A tab
            // absent from the test that polices the tabs is a tab the policing does not reach.
            Context::Compose,
        ] {
            let chords: String = rows(context)
                .iter()
                .map(|r| r.chord.clone())
                .collect::<Vec<_>>()
                .join(" ");
            for b in in_context(context) {
                // Against the RENDERED chord, not the bare field: a modified binding reads as one
                // token (`Cmd+S`), and splitting it back apart would be this test inventing a second
                // rendering rule to disagree with `chord_text`.
                let want = chord_text(b);
                assert!(
                    chords.split(&[' ', ','][..]).any(|c| c == want),
                    "{:?}'s chord `{want}` vanished when rows collapsed",
                    b.action
                );
            }
        }
    }

    /// **The lookup actually resolves.** Everything above tests the table; this tests the function
    /// the systems call, which is where a refactor can quietly break every key at once.
    #[test]
    fn pressing_a_bound_key_fires_its_action() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::Tab);
        assert!(
            just_pressed(&input, Context::Map, Action::NextTab),
            "Tab did not fire NextTab"
        );
        assert!(
            !just_pressed(&input, Context::Map, Action::MapTab),
            "Tab fired an unrelated action"
        );

        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::Digit2);
        assert!(
            just_pressed(&input, Context::Map, Action::TilesTab),
            "2 did not fire TilesTab"
        );
    }

    /// **The bug that put six descriptors in `library.ron`.** While a field owns the keyboard, no
    /// census action may fire — including `Enter`, which is both "commit this token" and "add to
    /// library". Asserted over the WHOLE census rather than the one binding that hurt, because the
    /// next one to hurt will be a different row.
    #[test]
    fn no_action_fires_while_a_field_is_taking_keys() {
        let mut input = ButtonInput::<KeyCode>::default();
        for b in BINDINGS {
            input.press(b.key);
        }
        for k in MOD_KEYS {
            input.press(k);
        }
        for b in BINDINGS {
            assert!(
                !just_pressed(&input, Context::Typing, b.action),
                "{:?} fired while a text field owned the keyboard",
                b.action
            );
            assert!(
                !pressed(&input, Context::Typing, b.action),
                "{:?} read as held while a text field owned the keyboard",
                b.action
            );
        }
    }

    /// A tab's letters mean nothing in another tab — which is the whole reason `Context` exists, and
    /// what previously took five hand-written `if *mode != Mode::Tiles` early returns to enforce.
    #[test]
    fn a_tabs_binding_does_not_fire_from_another_tab() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::KeyF);
        assert!(
            just_pressed(&input, Context::Map, Action::Fill),
            "F must flood fill on the map tab"
        );
        assert!(
            !just_pressed(&input, Context::Tiles, Action::Fill),
            "F must do nothing on the tiles tab"
        );

        // And a Global binding fires from every tab, which is what `Global` means.
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::Tab);
        for tab in [Context::Map, Context::Tiles, Context::Anim] {
            assert!(
                just_pressed(&input, tab, Action::NextTab),
                "Tab must cycle tabs from {tab:?}"
            );
        }
    }

    /// A modified chord fires only with the modifier, and the bare key only without — `S` pans and
    /// the modified `S` saves. Driven through `MOD_KEYS` rather than a named `ControlLeft`, so the
    /// test proves the binding the BUILD reads instead of the one this file was first written for.
    #[test]
    fn modified_chords_and_bare_keys_do_not_shadow_each_other() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::KeyS);
        assert!(just_pressed(&input, Context::Map, Action::PanBack));
        assert!(
            !just_pressed(&input, Context::Map, Action::Save),
            "bare S must not save"
        );

        // A FRESH input, not `clear()`: `clear` keeps the pressed state, so pressing an
        // already-held key never re-registers as just-pressed and the assertion below would fail for
        // a reason that has nothing to do with the chord.
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(MOD_KEYS[0]);
        input.press(KeyCode::KeyS);
        assert!(
            just_pressed(&input, Context::Map, Action::Save),
            "{MOD_NAME}+S must save"
        );
        assert!(
            !just_pressed(&input, Context::Map, Action::PanBack),
            "{MOD_NAME}+S must not also pan"
        );
    }

    /// **The chord that made undo look broken.** `Cmd+Z` on a Mac had been checked against Ctrl, so
    /// it did nothing at all — a flood fill could not be taken back and the undo stack looked empty.
    /// Both halves of the `Z` pair are asserted because binding the bare letter to aim is what makes
    /// getting this wrong silent rather than loud.
    #[test]
    fn the_platform_modifier_undoes_and_the_bare_key_aims() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(MOD_KEYS[0]);
        input.press(KeyCode::KeyZ);
        assert!(
            just_pressed(&input, Context::Map, Action::Undo),
            "{MOD_NAME}+Z must undo"
        );
        assert!(
            !just_pressed(&input, Context::Map, Action::AimLeft),
            "{MOD_NAME}+Z must not also turn the brush"
        );

        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::KeyZ);
        assert!(
            just_pressed(&input, Context::Map, Action::AimLeft),
            "bare Z must aim left"
        );
        assert!(
            !just_pressed(&input, Context::Map, Action::Undo),
            "bare Z must not undo"
        );
    }

    /// Every binding can be rendered next to the thing it does, which is the whole point of carrying
    /// the label in the census — see the module note on Cockburn et al. 2014.
    #[test]
    fn every_binding_states_itself() {
        for b in BINDINGS {
            assert!(!b.chord.is_empty(), "{:?} has no chord label", b.action);
            assert!(!b.does.is_empty(), "{:?} does not say what it does", b.action);
            assert!(
                b.chord.len() <= 8,
                "{:?}'s chord `{}` is too long for a key column",
                b.action,
                b.chord
            );
        }
    }
    /// **A tap is one step, exactly as it was.** The repeat must not change the thing every author
    /// already does — a press fires immediately, and nothing else fires until the key is held past
    /// the interval.
    #[test]
    fn a_press_fires_at_once_and_then_waits() {
        let mut input = ButtonInput::<KeyCode>::default();
        let mut repeat = Repeat::default();
        input.press(binding(Action::AimRight).key);

        assert!(
            repeating(&input, Context::Map, Action::AimRight, &mut repeat, 0.0),
            "the press itself must fire without waiting on a timer"
        );

        // Held, but `just_pressed` no longer reports it — this is what a second frame looks like.
        input.clear();
        input.press(binding(Action::AimRight).key);
        input.clear_just_pressed(binding(Action::AimRight).key);
        // Just under the interval, in ten equal steps: nothing more may fire.
        let step = REPEAT_SECS / 10.0;
        let mut fired = 0;
        for _ in 0..9 {
            if repeating(&input, Context::Map, Action::AimRight, &mut repeat, step) {
                fired += 1;
            }
        }
        assert_eq!(fired, 0, "9/10 of the {REPEAT_SECS} s interval must not fire");

        // Crossing it fires exactly once.
        assert!(repeating(&input, Context::Map, Action::AimRight, &mut repeat, step * 2.0));
        assert!(!repeating(&input, Context::Map, Action::AimRight, &mut repeat, 0.0));
    }

    /// Holding for a second yields the presses the interval promises, rather than one per frame.
    #[test]
    fn holding_repeats_at_the_stated_cadence() {
        let mut input = ButtonInput::<KeyCode>::default();
        let mut repeat = Repeat::default();
        input.press(binding(Action::AimLeft).key);
        let mut fired = usize::from(repeating(&input, Context::Map, Action::AimLeft, &mut repeat, 0.0));

        input.clear_just_pressed(binding(Action::AimLeft).key);
        // One second at 60 fps.
        for _ in 0..60 {
            if repeating(&input, Context::Map, Action::AimLeft, &mut repeat, 1.0 / 60.0) {
                fired += 1;
            }
        }
        // The press, plus 1 / REPEAT_SECS more per second.
        let want = 1 + (1.0 / REPEAT_SECS) as usize;
        assert!(
            fired == want || fired == want + 1,
            "a second of holding fired {fired} times, wanted about {want}"
        );
    }

    /// Releasing forgets the countdown, so the next tap is immediate rather than owing the remainder
    /// of an interval nobody is waiting through.
    #[test]
    fn releasing_resets_the_countdown() {
        let mut input = ButtonInput::<KeyCode>::default();
        let mut repeat = Repeat::default();
        let key = binding(Action::AimRight).key;

        input.press(key);
        assert!(repeating(&input, Context::Map, Action::AimRight, &mut repeat, 0.0));
        input.clear_just_pressed(key);
        // Part of the way to the next repeat, then let go.
        repeating(&input, Context::Map, Action::AimRight, &mut repeat, REPEAT_SECS * 0.8);
        input.release(key);
        assert!(!repeating(&input, Context::Map, Action::AimRight, &mut repeat, 0.0));

        input.clear();
        input.press(key);
        assert!(
            repeating(&input, Context::Map, Action::AimRight, &mut repeat, 0.0),
            "a fresh press fires at once, not after the remainder of an interval"
        );
    }

    /// **A key held across a tab change does not resume.** Otherwise switching tabs with a finger
    /// down would fire an action in a context the author never pressed it in — the same class of
    /// defect `Phase` exists for.
    #[test]
    fn a_key_held_into_a_new_context_waits_for_a_fresh_press() {
        let mut input = ButtonInput::<KeyCode>::default();
        let mut repeat = Repeat::default();
        let key = binding(Action::AimRight).key;

        // Held down while the Tiles tab owns the keyboard: nothing accrues.
        input.press(key);
        input.clear_just_pressed(key);
        assert!(!repeating(&input, Context::Tiles, Action::AimRight, &mut repeat, 5.0));

        // Now the Map tab is live and the key is still down, but was never pressed here.
        assert!(
            !repeating(&input, Context::Map, Action::AimRight, &mut repeat, 5.0),
            "a key that was already down must not start repeating on a context change"
        );
    }
}
