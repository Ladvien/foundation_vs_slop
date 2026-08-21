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
//! # Routing is by CONTEXT, not by focus — settled 2026-08-18
//!
//! `docs/research/2026-08-18-reusable-scroll-and-tab-widgets.md` §3.5 assumes the other model: it
//! routes keyboard paging to *"the scroll view that currently owns focus"*. **This editor does not
//! work that way and is not going to.** `just_pressed(&ButtonInput, Live, Action)` is a *pull*
//! helper each consumer polls, and `Live(Context, Stance)` is an ambient pair — there is no focused
//! widget, and `InputFocus`/`TabIndex` appear nowhere in `src/`.
//!
//! Introducing focus *as a second routing model* was the alternative and it is the one thing this
//! crate's rules forbid outright: two paths to "who gets this key", disagreeing on the day somebody
//! clicks a row and then presses a verb. `Live` is decided **once** per frame in `Phase::Sense`
//! precisely so no system sees a keyboard that changed owner mid-frame, and that guarantee is not
//! available to a model where a click can move ownership.
//!
//! So focus stays out of routing, and is used for the two things it is actually for: the **a11y
//! tree** (`Role::TabList`/`Tab` on the strip) and, when something wants it, a visible focus ring.
//! `FeathersPlugins` brings `acquire_focus` and `click_to_focus`, and they are inert here because
//! nothing carries `TabIndex` — which is the correct amount of inert, not an oversight.
//!
//! **The paging keys the design wanted are therefore not built either**, and that is the better
//! outcome rather than a consolation. Every list here is walked with the arrows, and
//! `chrome::Follow` scrolls the selection into view — so `PageUp`/`PageDown`/`Home`/`End` would be
//! four more rows against a **hard twelve-row ceiling**
//! (`no_context_carries_more_than_a_learnable_vocabulary`), buying a second way to do what the
//! arrows already do. `docs/ui.md` §3.5's own finding is that a fast path beside a slow one does not
//! work on its own; a fast path beside an *equivalent* one is worse.
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
    /// The mesh tab — bring art in, measure it, say what it is.
    Meshes,
    /// The animation bench.
    Anim,
    /// The composition tab — reusable groups, their derived interface, and what is stale.
    Compose,
    /// **The Tiles tab** — assembling a cell-sized tile out of meshes.
    ///
    /// This was `Build`, a second context on the mesh tab reached by a mode key. It is a tab of its
    /// own now (FVS-R-21): every other level of the kit hierarchy had one, and the twelve-row cap
    /// each context gets is what made two of them affordable in the first place —
    /// `no_context_carries_more_than_a_learnable_vocabulary` is not a limit to route around, since
    /// Liapis names *"too many options"* as a cause of the user fatigue this is all shaped to avoid.
    Tiles,
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
            // **A tab overlaps only itself.** Five arms rather than a `self == other` catch-all,
            // because the catch-all would also make a *new* context overlap only itself by default —
            // and the whole point of this table is that adding one costs a decision here.
            (Map, Map) | (Meshes, Meshes) | (Tiles, Tiles) | (Anim, Anim) | (Compose, Compose) => {
                true
            }
            // **The tabs are never live together**, which is what lets them reuse each other's
            // letters freely: the Tiles tab walks a lattice with `T`/`F`/`G`/`H` and so does Meshes,
            // one tab over. `the_key_space_has_no_collisions` polices that they only do so here.
            _ => false,
        }
    }
}

/// **What the author has in hand** — the second axis of who owns the keyboard.
///
/// # Why this is in the census rather than in a handler's `if`
///
/// [`Context::Typing`] was already a *phase* wearing a context's clothes: it is not a tab, it is a
/// state the editor passes through, and modelling it here is what gave the focus guard one home. The
/// second phase is holding something, and it was never modelled — so it went where an unmodelled
/// phase always goes, into a handler's own state. `build.rs` said so plainly: *"the census forbids two
/// actions on one key in one context, so the key is single and the **state** decides which job it
/// does."*
///
/// That is the second census §3.5 exists to prevent, and it cost exactly what a second census costs:
/// the arrows walk a list in three tabs and **move geometry** in a fourth, and the held-`K` overlay
/// could not say which — it rendered one row, `"list / nudge the mesh / Shift: flush it"`, whatever
/// was actually live. An author reading the key list got no answer to the only question a modal
/// grammar raises, *what do the arrows do right now*. (The overlay is badges on the controls now,
/// and the stance decides their [`Home`] as well as their text — so the arrows' badge is on the list
/// when they walk it and on the piece when they move it.)
///
/// With the phase in the table, one key carries two actions the way [`Binding::needs_shift`] already
/// lets it carry two: they are exclusive by construction, the collision test knows it, and [`rows`]
/// shows the one that is live.
///
/// **This does not make the mode safe on its own, and the corpus cannot settle whether it is.**
/// `docs/research/2026-08-10-snapping-corpus-vetting.md` records the gap — Raskin has never been
/// ingested, so there is no mode-error literature here to appeal to. The standing argument is
/// `build.rs`'s: the mode is acceptable *because it is drawn*, a ghost standing on the grid. This adds
/// a second place it is stated, which strictly improves on that; it does not replace it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    /// Nothing in hand. The arrows walk a list.
    Idle,
    /// A piece is held — taken with `Space` in the tile assembler, picked up with `B` on the map. The
    /// arrows move *it*.
    Holding,
    /// **A generated layout is waiting to be accepted or thrown away.**
    ///
    /// The third phase, and the one that pays for itself. Alvarez et al. 2018 (FDG,
    /// `10.1145/3235765.3235815`) added a two-step commit to the Evolutionary Dungeon Designer
    /// *because* apply-on-click was **"occasionally causing work loss due to accidental
    /// replacements"** — and `G` here rewrote the map on the keypress.
    ///
    /// Modelling it as a stance rather than as a flag is what made the door affordable: the four
    /// region-fills are `Idle`, so their row leaves the list while a proposal is up and the door's row
    /// takes its place. The Map context stays at twelve either way, and an author cannot start a
    /// second generate on top of an answer they have not looked at yet.
    Proposed,
    /// **The kit list is live** — the arrows walk the tiles already authored, not the meshes.
    ///
    /// The fourth phase, and it exists because the Tiles tab could make tiles and never show them.
    /// An author finished four and could not see the kit, could not reopen one to correct it, and had
    /// no way to notice they had built the same thing twice. The one that was wrong — a low wall
    /// sitting in the middle of its tile instead of flush — was only fixable by editing
    /// `compositions.ron` by hand.
    ///
    /// It is a stance rather than a flag for the same reason `Proposed` is: the arrows and `Enter`
    /// mean something different while it is on, and a key list that did not say so would be lying.
    /// **It costs no new key.** `left`/`right` were unbound on this tab at `Idle`, and
    /// `docs/tiles_tab_contract.md` recorded exactly why — *"There is one list on this tab, so there
    /// is nothing to switch between."* There are two now, so the reservation is spent.
    Browsing,
}

impl Stance {
    /// Can these two be live at the same moment? A stance is exactly one thing at a time, so this is
    /// equality — stated as a method anyway, so the collision test reads the same for both axes and a
    /// future third stance costs a decision here rather than a silent `==`.
    pub fn overlaps(self, other: Stance) -> bool {
        self == other
    }
}

/// **Where a verb's badge is drawn while [`Action::Shortcuts`] is held.**
///
/// Two answers, and the second is what the first is not. There was a third — `Subject`, on the piece
/// in the world a verb acts on — and it was removed after being looked at rather than reasoned about:
/// the Map's subject is the *armed ghost*, which follows the pointer, so the block chased the mouse
/// across the viewport and parked on top of the legend. A location that moves with the cursor also
/// teaches nothing, which is the one thing this placement existed for — ExposeHK's third goal is
/// hotkeys at the **spatial** location of the thing they act through, and a moving spot has none.
/// A piece of geometry is not a control; by the same rule as everything else, its verbs say
/// themselves in the legend.
///
/// # Why this is a field of the census rather than a table beside it
///
/// The badge overlay replaced a centred two-column list of every chord, and the reason it could is
/// that a chord drawn *on the thing it acts on* is read by looking at the thing rather than by
/// mapping a phrase back onto it. That only works if **every** binding has somewhere to be drawn:
/// one row with no home is one verb that silently vanishes from the only place it was ever
/// announced, which is the failure this whole change is about (`R` and `Shift+Delete` were bound for
/// two sessions and invisible on a collapsed row).
///
/// So it is a field, and [`Draft::at`] is the only way to make a [`Binding`] — the compiler refuses a
/// row that has not said where it lives. That is stronger than a test, and it is the reason this cost
/// a suffix on every row of the census rather than a defaulted argument: `needs_shift: None` is an honest answer
/// ("this axis does not apply to me"), and `home: None` is not.
///
/// Malacria, Bailly, Harrison, Cockburn & Gutwin 2013, *Promoting Hotkey Use through Rehearsal with
/// ExposeHK* (`10.1145/2470654.2470735`), is the measured form of the design and its third goal is
/// exactly this field: *"EHK leverages human spatial memory by ensuring that hotkeys are displayed at
/// the spatial location of the underlying visual control."*
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Home {
    /// On the control this verb acts through.
    Control(ControlId),
    /// **In the legend**, for a verb that acts on neither.
    ///
    /// `Esc` backs out; there is nothing on screen it backs out *of* in particular. Pan turns the
    /// camera, whose one honest anchor — the compass — stands down while `K` is held; the region
    /// fills paint ground that is not a widget. About a fifth of this editor's vocabulary has no
    /// subject and no control, and pretending otherwise means drawing a chord on something it does
    /// not control.
    ///
    /// So they are drawn together, in a column over empty ground, **each with its description beside
    /// its chord**. That is the one place a badge is allowed to carry prose: everywhere else the
    /// thing under the badge already says what it is, and here there is no thing.
    ///
    /// It is not a leftovers bin. It is a stable spatial home — the same corner every time, in
    /// declaration order — which is what a hand learns. ExposeHK's third goal
    /// (`10.1145/2470654.2470735`) is that a chord sits somewhere the eye can go straight to; for a
    /// verb with no control, "the legend, third row" is that somewhere.
    Legend,
}

/// **A control the census can name.**
///
/// Symbolic on purpose: `keys.rs` must not learn what a palette is, and a badge system querying
/// fourteen domain markers (`PaletteRow`, `TagChip`, `CellButton`, …) would be the second census this
/// module exists to delete. A panel attaches `crate::chrome::Control(id)` to the node it spawns, and
/// the join is by id.
///
/// **The rule that keeps this list honest:** a `ControlId` may only name a node that is on screen for
/// the *whole* of every `(Context, Stance)` in which some binding homes to it. A pane that renders
/// nothing until something is selected is not a home — its badge would vanish exactly when a new
/// author needs it. `every_home_a_live_binding_names_is_on_screen` in `tests/headless.rs` is what
/// holds that, and it runs against an empty project as well as a populated one for precisely this
/// reason.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ControlId {
    /// The strip of panel chips — `1`, `2`, `3`.
    DoorStrip,
    /// Where you are: the kit and map name in the chrome bar.
    Title,
    /// The way out, to kits and maps.
    Back,
    /// The line in the status band that says to hold the shortcuts key. Holding it puts that key's
    /// own badge on the line that told you about it, which is the rehearsal loop closing on itself.
    Hint,
    /// The tab's own text pane — the Map's status block, the Meshes/Tiles detail pane, the anim
    /// bench's slots, the Compose body. Exactly one is ever laid out, because the others' panels are
    /// `Display::None`.
    ///
    /// **For a verb whose readout is the pane itself** — `Cmd+C`, the commit door on a derivation,
    /// each tab's undo pair (what undo changes is what the pane reports), the anim bench's
    /// scrub/play/ghost, the Compose carousel. Not a bin: it was briefly the home
    /// of seven unrelated groups — `I`, `M`, `T F G H [ ]`, `Z X V`, `B N O P`, `L …` — and that was
    /// the mistake this whole overlay exists to avoid: a container is not an anchor, so eleven bare
    /// chords piled against a pane's edge with nothing under them saying what any of them did. Each
    /// of those has a row of its own, and each now names it.
    Detail,
    /// The id headline of the piece being defined — the thing `I` types.
    IdField,
    /// The `mount` row, which `M` cycles.
    Mount,
    /// The subgrid's own lattice of cells: what the cell cursor walks and what solid/edge/clear
    /// paints.
    CellGrid,
    /// The block of tag chips, and so the labels the VLM proposes into it.
    Tags,
    /// The scan button — the mesh as measured, which is what rescanning and turning act on.
    Mesh,
    /// The Map's PLACE list.
    Palette,
    /// The candidate/library/kit list the Meshes and Tiles tabs share.
    Pieces,
    /// The filter box above whichever list has one.
    Filter,
    /// The animation bench's rig list.
    Rigs,
    /// The Map's YAW row — the facing the turn cluster writes.
    Yaw,
    /// The Map's UNDER row — the piece under the cursor, which the piece-verbs act on and the row
    /// exists to name.
    Under,
    /// The TILE card in the Tiles pane — the tile being assembled, open or not: its heading is on
    /// screen in every branch, including the one that says "press N to start one".
    Tile,
    /// The MEMBERS list of the open tile — the focus the member-verbs move. On screen whenever a
    /// verb homed here is live: every one is `Stance::Holding`, and Holding means an open tile
    /// with a focused member (`editor::sense_context`).
    Members,
}

impl ControlId {
    /// **Is this control a row inside a scrolling pane?**
    ///
    /// It decides where the control's badge may go, and it is stated here because it is a fact about
    /// the editor's shape rather than about any one frame. A row inside a pane is *content*: its
    /// chord belongs at the content's own leading edge, in the panel's `MARGIN + PAD` inset, and the
    /// pane's far edge is a different part of the screen — a badge flipped out there floats in the
    /// viewport hundreds of pixels from the row it names, attached to nothing. So a chord too wide
    /// for that inset goes to the legend instead, with its description, which is what the legend is.
    ///
    /// `a_paned_control_really_is_inside_a_pane` in `tests/headless.rs` checks this against the real
    /// tree, so it cannot drift from the panels it describes.
    pub const fn in_a_pane(self) -> bool {
        match self {
            ControlId::IdField
            | ControlId::Mount
            | ControlId::CellGrid
            | ControlId::Tags
            | ControlId::Mesh
            | ControlId::Tile
            | ControlId::Members => true,
            // The lists and the window's own furniture: each is a whole node with open ground beside
            // it, so a badge sits against its edge and reads as attached.
            ControlId::DoorStrip
            | ControlId::Title
            | ControlId::Back
            | ControlId::Hint
            | ControlId::Detail
            | ControlId::Palette
            | ControlId::Pieces
            | ControlId::Filter
            | ControlId::Rigs
            // The Map's readout rows sit in the StatusBlock, a plain column with open ground
            // beside it — no fold to pin their badges to.
            | ControlId::Yaw
            | ControlId::Under => false,
        }
    }

    /// **Is this control in one of the frame's fixed-height bands?**
    ///
    /// It decides the badge's *shape*. A dock has room for a column, so every badge there is one row
    /// of a vertical list — the chord on the left, what it does on the right, the same shape as the
    /// legend. A band is twenty-six pixels of chrome and holds no column at all, so a badge there is
    /// the bare chord beside a control whose own words are already the verb: `‹ kits & maps`,
    /// `1 MESHES 2 TILES 3 COMPOSE`, the map's name, the hint line.
    ///
    /// One shape for the docks was asked for outright — *"put the z v in x labels in a vertical
    /// flexbox, and then the key legend to the right of it… I'd like to standard this across all of
    /// the UI on the left and right."* Before it, a dock could carry a bare letter on a row, a
    /// labelled column beside a list, and a keypad drawn as a cross, all at once.
    ///
    /// `a_control_in_a_band_really_is_in_one` checks this against the real tree.
    pub const fn in_a_band(self) -> bool {
        match self {
            ControlId::DoorStrip | ControlId::Title | ControlId::Back | ControlId::Hint => true,
            ControlId::Detail
            | ControlId::IdField
            | ControlId::Mount
            | ControlId::CellGrid
            | ControlId::Tags
            | ControlId::Mesh
            | ControlId::Palette
            | ControlId::Pieces
            | ControlId::Filter
            | ControlId::Rigs
            | ControlId::Yaw
            | ControlId::Under
            | ControlId::Tile
            | ControlId::Members => false,
        }
    }

    /// Every one, so a ratchet can enumerate. A `ControlId` nothing homes to is a word with no
    /// meaning, and a `ControlId` no panel attaches is a badge that never appears.
    pub const ALL: [ControlId; 18] = [
        ControlId::DoorStrip,
        ControlId::Title,
        ControlId::Back,
        ControlId::Hint,
        ControlId::Detail,
        ControlId::IdField,
        ControlId::Mount,
        ControlId::CellGrid,
        ControlId::Tags,
        ControlId::Mesh,
        ControlId::Palette,
        ControlId::Pieces,
        ControlId::Filter,
        ControlId::Rigs,
        ControlId::Yaw,
        ControlId::Under,
        ControlId::Tile,
        ControlId::Members,
    ];
}

/// Everything the editor can be asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Jump to this door's first, second or third panel. **Three, because the widest door holds
    /// three** — Kit is Meshes/Tiles/Compose. A fourth would be a fourth row here and a fourth in
    /// every context's list, which is what the twelve-row cap is for.
    TabSlot1,
    TabSlot2,
    TabSlot3,
    // ── Global ───────────────────────────────────────────────────────────────
    /// Walk the composition list.
    ComposePrev,
    ComposeNext,
    /// Arm the selected composition on the map, so a click stamps it.
    ComposeArm,
    /// Walk the selected group's members. A second cursor, under the one that walks the groups.
    ComposeMemberPrev,
    ComposeMemberNext,
    /// Step the Compose carousel — the previous or next composition becomes the focal one. Its own
    /// keys rather than the arrows, which belong to whichever of the three lists has focus.
    CarouselPrev,
    CarouselNext,
    Save,
    /// **Back to the chooser** — leave this map and pick another.
    ///
    /// `Cmd+O` because "open something else" is what every editor binds it to, and this screen has
    /// no other opening verb to confuse it with. It is a **process** boundary, not a tab: the editor
    /// was launched by the chooser and exits back to it, which is why the action lives here and the
    /// mechanism lives in `main.rs`.
    MainMenu,
    Undo,
    Redo,
    /// **Show every refusal this session has raised**, in a panel that is not on screen until asked
    /// for. See `chrome::Journal`.
    ShowErrors,
    /// Hold to label the interface: every live key, drawn on the thing it acts on. See
    /// `crate::badges`, and [`Home`] for how a verb knows where that is.
    Shortcuts,
    /// Open the Tiles tab on the descriptor of the piece under the cursor.
    EditTile,
    // ── Map ──────────────────────────────────────────────────────────────────
    /// **Turn the thing being steered** — the ghost while a brush or a composition is armed, the
    /// placement under the cursor when the cursor is clear.
    ///
    /// One cluster, not two. `Z`/`C` used to aim the brush while these turned what was underneath,
    /// and the split was reported from the keyboard 2026-08-14: *"it felt weird to have a mesh
    /// selected and use R and T to rotate not my ghosted selection, but the mesh underneath it."*
    /// The subject now follows what is armed, the same rule `editor::cursor_is_clear` gives the
    /// click.
    TurnPieceLeft,
    TurnPieceRight,
    /// Cycle which piece of the stack under the cursor the piece-verbs act on — coincident
    /// pieces made "the placement under the cursor" ambiguous, and a nudge aimed at a header
    /// moved the wall under it. Each press names the next piece, bottom to top; Esc releases.
    CycleTarget,
    /// **Walk the palette — choose what to place, from the keyboard.**
    ///
    /// This did not exist. `EditorState::brush` had exactly one writer, the `on_row_click` mouse
    /// observer, so on the tab the code itself calls *"the job"* the most frequent act in the editor
    /// could only be done with the pointer. The author's brief was the opposite: *"this should be
    /// done by the keyboard, as key strokes are faster."*
    PalettePrev,
    PaletteNext,
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
    /// Put the steered thing back to the rotation and tip it was authored at — same subject as
    /// [`Action::TurnPieceLeft`].
    Straighten,
    /// Leave the removal mode without removing anything.
    Cancel,
    OwnToggle,
    Generate,
    /// Fill from what the kit's tiles DECLARE, rather than from what the map already shows.
    GenerateDeclared,
    /// Fill from the kit's COMPOSITIONS — whole tiles, laid as stamps rather than as placements.
    GenerateComposed,
    /// **Step the rung the map places on** — and therefore the grid it draws, because they are the
    /// same number. See `editor::Rung`, whose note records the two-mechanism arrangement this
    /// replaced.
    CycleGrid,
    /// **Keep the edge tokens read off the mesh.** The other half of the door is `Cancel`.
    /// See `tiles::DerivedEdges` and `emerge_core::adjacency::derive_edges`.
    AcceptEdges,
    /// **Keep the generated layout.** The other half of the door is `Cancel`, which throws it away.
    /// See `editor::Proposal`.
    AcceptProposal,
    /// Keep the set in hand as a reusable group — see `editor::composition_from_set`.
    GroupFromSet,
    /// **Show the kit** — the tiles already authored — and let the arrows walk it.
    KitEnter,
    KitPrev,
    KitNext,
    /// **Reopen the selected tile for editing.** The verb the tab never had: every tile was a new
    /// blank one, so a tile saved wrong stayed wrong.
    KitOpen,
    /// **Leave the kit for the mesh list** — `left`, the way back out of a column browser.
    KitLeave,
    /// **Put the cursor in the filter box.** The box was reachable only by mouse, on a tab whose
    /// whole argument is that keystrokes are faster.
    FocusFilter,
    /// **Put the cursor in the tag box** — the same verb, one panel over, on the block that holds the
    /// project's whole vocabulary.
    ///
    /// Its own action rather than a second context for [`Action::FocusFilter`], because the two boxes
    /// are on screen **at the same moment**: the candidate list down the right, the tag block in the
    /// detail pane on the left. One key cannot mean both without asking which one the author meant,
    /// and a key that guesses is the thing this census exists to prevent.
    FocusTagFilter,
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
    FoldPack,
    Rescan,
    /// **Send a library mesh back to `NOT IMPORTED`**, stripped, and land the cursor on it there.
    ///
    /// One verb since 2026-08-20. `DemoteTile` sat beside it on the shifted chord and named the same
    /// act — the candidate list *is* "meshes on disk not in the library", so taking an entry out
    /// always sends its mesh back; only the rescan and the cursor differed. See `tiles::remove_tile`.
    RemoveTile,
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
    // ── Build (the Tiles tab's other half) ───────────────────────────────────
    /// **Walk the library list on the Tiles tab, with nothing in hand.**
    ///
    /// Two actions rather than reusing [`Action::BuildForward`] for both jobs, which is what this tab
    /// did until the census learned about [`Stance`]. The old arrangement could not be written down:
    /// one binding, two behaviours, and a key list that had to describe both at once — it read
    /// *"list / nudge the mesh / Shift: flush it"* whatever was actually happening.
    TileListPrev,
    TileListNext,
    BuildForward,
    /// The other two of the four. Added when the author said the arrows were not intuitive: two of
    /// four directions moved the piece and the other two walked the member list, so an isometric view
    /// offered NE/SW and nothing else. All four move it now, and `,`/`.` walk the members.
    BuildLeft,
    BuildRight,
    BuildBack,
    /// **Step to the previous / next member of the tile.**
    ///
    /// The missing primitive, reported 2026-08-12: *"once I place mesh down, and I place the second
    /// mesh down, how do I switch between two meshes to edit its placement?"* You could not.
    /// `Build::focus` is what `R`, `Delete`, the arrows and the flush all act on, and it is drawn in
    /// amber — and nothing an author could press moved it. A drop set it, removal and undo clamped
    /// it, and that was every writer there was.
    ///
    /// Left/right because **Compose already binds exactly this verb to them**, one level up: walking
    /// the members of the group in hand. Same key, same job, one level down — and it fills the arrow
    /// pair that did nothing on this tab.
    MemberPrev,
    MemberNext,
    BuildDown,
    BuildUp,
    BuildDrop,
    BuildSlot,
    /// **Take the piece in hand**, so the arrows steer the tile instead of the library list.
    /// `Esc` puts it back. See [`crate::build::Build::placing`].
    BuildArm,
    /// Step the tile assembler's own history back, and forward. See [`crate::build::TileHistory`].
    UndoBuild,
    RedoBuild,
    /// Put the focused mesh flush against that side of the tile. See [`crate::build::aligned`].
    AlignForward,
    AlignBack,
    AlignLeft,
    AlignRight,
    BuildNew,
    BuildDropMember,
    /// **Empty the tile** — the shifted form of removing one member, on the `RemoveTile`/`DemoteTile`
    /// precedent. One undo step, so it is recoverable.
    ClearTile,
    BuildTurn,
    BuildRung,
    CellEdge,
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
    /// **Exclude the highlighted pack from this kit, or put it back.** One key, one concept — a
    /// separate restore verb would be a second way to say the same thing, and the row already says
    /// which state it is in.
    ExcludePack,
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

impl Action {
    /// **The number key for the nth panel of a door**, or `None` past the third.
    ///
    /// [`crate::tiles::Door::tabs`] is what indexes this, so the digits and the strip cannot
    /// disagree about which panel is second.
    pub fn tab_slot(i: usize) -> Option<Action> {
        match i {
            0 => Some(Action::TabSlot1),
            1 => Some(Action::TabSlot2),
            2 => Some(Action::TabSlot3),
            _ => None,
        }
    }

    /// **Can this verb fire on a door showing `panels` panels?**
    ///
    /// Only the slot keys can answer no, and only because they are `Context::Global` while the thing
    /// they act on is per-door: `tiles::tab_shortcuts` walks `Door::tabs()`, so on the Map and Rigs
    /// doors — one panel each — `2` and `3` correctly do nothing. The badge overlay did not know
    /// that and drew `1, 2, 3` against a strip holding one chip, which is the overlay claiming a key
    /// the editor refuses. A verb announced and not honoured is worse than one nobody mentions.
    ///
    /// **Derived from [`Self::tab_slot`], not from a second table.** That mapping is what indexes the
    /// strip and what the key handler walks, so asking it here is asking the same question rather
    /// than answering it twice.
    pub fn fires_on_a_door_of(self, panels: usize) -> bool {
        match self {
            Action::TabSlot1 | Action::TabSlot2 | Action::TabSlot3 => {
                (0..panels).filter_map(Action::tab_slot).any(|a| a == self)
            }
            _ => true,
        }
    }
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
    /// **Which [`Stance`] this binding wants**, on exactly the rule [`Binding::needs_shift`] follows.
    ///
    /// `None` — the phase is not part of this binding, and it fires whether or not something is in
    /// hand. That is what almost every row wants, and it is the default so that adding the axis
    /// changed no existing row's behaviour.
    ///
    /// `Some(s)` — fires only in stance `s`. Two rows on one key asking for different stances can
    /// never both be satisfied, which is what lets the arrows walk a list *and* move a piece without
    /// the handler deciding.
    pub needs_stance: Option<Stance>,
    pub context: Context,
    /// How the BARE key reads in the panel — `S`, `Z`, `up`. The modifier is **not** written here:
    /// [`rows`] prepends [`MOD_NAME`] when `needs_mod`, so the one panel that shows this list says
    /// `Cmd+S` on a Mac and `Ctrl+S` elsewhere without a second census to keep in step.
    pub chord: &'static str,
    /// What it does, in the fewest words that stay true.
    pub does: &'static str,
    /// **Where this verb's badge is drawn.** See [`Home`], and [`Draft::at`] for why there is no
    /// default.
    pub home: Home,
}

/// **A binding that has not said where it lives yet.**
///
/// The four constructors below return one of these, and [`Draft::at`] is the only way out — so a row
/// cannot reach [`BINDINGS`] without a [`Home`], and *"nothing falls off the badge overlay"* is the
/// compiler's answer rather than a test's.
pub struct Draft {
    action: Action,
    key: KeyCode,
    needs_mod: bool,
    needs_shift: Option<bool>,
    needs_stance: Option<Stance>,
    context: Context,
    chord: &'static str,
    does: &'static str,
}

impl Draft {
    /// Say where this verb's badge goes, and become a [`Binding`].
    pub const fn at(self, home: Home) -> Binding {
        Binding {
            action: self.action,
            key: self.key,
            needs_mod: self.needs_mod,
            needs_shift: self.needs_shift,
            needs_stance: self.needs_stance,
            context: self.context,
            chord: self.chord,
            does: self.does,
            home,
        }
    }
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

/// Alt/Option, either side — **free placement while placing**, and nothing else.
///
/// It has its own modifier so that [`shift_held`] can mean exactly one thing. Free placement used to
/// live on [`MOD_KEYS`], which made Shift read as "one rung finer" from bare and "back onto a
/// lattice" from the modifier — a modifier whose meaning inverts depending on another modifier does
/// not get learned.
pub const ALT_KEYS: [KeyCode; 2] = [KeyCode::AltLeft, KeyCode::AltRight];

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
    // **One row for three keys**, because jumping to a panel is one idea — the same collapse
    // `T F G H` gets. Three separate rows put `Context::Global` over the twelve-row ceiling
    // `no_context_carries_more_than_a_learnable_vocabulary` enforces, and a Global row costs every
    // context.
    //
    // **The digits follow the strip, and the strip is `Door::tabs`** — so `1` means this door's
    // first panel rather than the binary's. A number key that jumped somewhere the strip does not
    // show would be the census disagreeing with the thing it describes.
    b(
        Action::TabSlot1,
        KeyCode::Digit1,
        false,
        Context::Global,
        "1",
        "panel 1 / 2 / 3 of this door",
    )
    .at(Home::Control(ControlId::DoorStrip)),
    b(
        Action::TabSlot2,
        KeyCode::Digit2,
        false,
        Context::Global,
        "2",
        "panel 1 / 2 / 3 of this door",
    )
    .at(Home::Control(ControlId::DoorStrip)),
    b(
        Action::TabSlot3,
        KeyCode::Digit3,
        false,
        Context::Global,
        "3",
        "panel 1 / 2 / 3 of this door",
    )
    .at(Home::Control(ControlId::DoorStrip)),
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
    // **`Cmd`+remove opens the piece under the cursor for editing.** It was `Cmd`+the tab key, on the
    // argument that one key could carry both "the Tiles tab" and "the Tiles tab, about this piece".
    // That was tidy and it was not what an author reached for: asked what should happen when they
    // press a chord on a piece they got wrong, the answer was "open it for editing", and the chord
    // they reached for was the remove key with the command modifier — "get this out of my way and
    // let me fix it". The bare remove key is unbound on the Map, so this pairs with nothing and
    // collides with nothing.
    // **One question, with an ordered answer**: the piece under the cursor, and failing that the
    // PLACE selection. It reached only pieces standing on the map, so a piece selected in PLACE —
    // which the author is looking straight at — answered "nothing here to edit". The first fix keyed
    // on whether the pointer was over the interface, which made the answer depend on where the mouse
    // happened to be resting; ordering it does not.
    b(
        Action::EditTile,
        REMOVE_KEY,
        true,
        Context::Global,
        REMOVE_NAME,
        "define this piece",
    )
    .at(Home::Legend),
    // **Held, not toggled**, and read with `keys::pressed`. The list is a thing you glance at with a
    // thumb down, not a mode you enter and have to leave — and a modal you can forget you opened is a
    // modal that eats the next keystroke.
    b(
        Action::Shortcuts,
        KeyCode::KeyK,
        false,
        Context::Global,
        "K",
        "hold: every key lands on what it does",
    )
    .at(Home::Control(ControlId::Hint)),
    b(
        Action::Save,
        KeyCode::KeyS,
        true,
        Context::Global,
        "S",
        "save",
    )
    .at(Home::Control(ControlId::Title)),
    b(
        Action::MainMenu,
        KeyCode::KeyO,
        true,
        Context::Global,
        "O",
        "back to the menu",
    )
    .at(Home::Control(ControlId::Back)),
    // **`Cmd+E` for the errors, and the legend is where it lives** — the panel it opens is not on
    // screen to carry a badge, which is exactly the case `Home::Legend` is for.
    //
    // Legal against the bare `E` (turn view) on the rule `S`/`Cmd+S` and `Z`/`Cmd+Z` already follow:
    // `just_pressed` refuses a bare binding while the modifier is down and a modified one while it
    // is not. The mnemonic is the word, not the neighbour.
    b(
        Action::ShowErrors,
        KeyCode::KeyE,
        true,
        Context::Global,
        "E",
        "every error this session",
    )
    .at(Home::Legend),
    // **The agent's read-out, and it is Global because a problem is.**
    //
    // It was `Context::Meshes`, copying that tab's detail pane. Every tab can now refuse, and a refusal
    // an agent cannot get out of the window is one that has to be retyped from a screenshot — bevy_ui
    // has no selectable text. So the verb follows the thing it is for: on any tab this copies what
    // that tab is showing, with the problem first. Leaving Tiles also gives that context back the row
    // it was over, which is what made `Esc` affordable below.
    //
    // Legal against the three bare `C` bindings (aim right, cell anchor, check all rigs) on the same
    // rule as `S`/`Cmd+S`: `just_pressed` refuses a bare binding while the modifier is down.
    b(
        Action::CopyInfo,
        KeyCode::KeyC,
        true,
        Context::Global,
        "C",
        "copy this tab's text",
    )
    .at(Home::Control(ControlId::Detail)),
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
    b(
        Action::Cancel,
        KeyCode::Escape,
        false,
        Context::Global,
        "Esc",
        "back out",
    )
    .at(Home::Legend),
    // **Undo is the MAP's.** It was `Global`, and `keys` had lost its `in_map_mode` run condition, so
    // `Cmd+Z` on the Tiles tab silently despawned a flood fill — up to ~1,400 placements — while every
    // `MapRoot` panel was `Display::None` and nothing on screen changed. An undo you cannot see the
    // effect of is not an undo. Lattice edits go straight to `library.ron` and have no undo of their
    // own, which is exactly why the key is reached for there.
    // **One row, two chords.** A shared `does` collapses them the way `W, A, S, D` collapses, so
    // adding redo costs no row — which matters, because the Map context has none to give.
    bs(
        Action::Undo,
        KeyCode::KeyZ,
        true,
        false,
        Context::Map,
        "Z",
        "undo / redo",
    )
    .at(Home::Control(ControlId::Detail)),
    bs(
        Action::Redo,
        KeyCode::KeyZ,
        true,
        true,
        Context::Map,
        "Z",
        "undo / redo",
    )
    .at(Home::Control(ControlId::Detail)),
    // **Z and C turn the brush; X puts it back.** They sit under the left hand already resting on
    // WASD, which the brackets never did — and `Z` is free as a bare key precisely because the
    // modifier check above keeps `Cmd+Z` (undo) and `Z` (aim) apart rather than letting the chord
    // shadow the letter.
    // One ROW, deliberately: a shared `does` collapses the pair the way `W, A, S, D` collapses, and
    // the Map context is at its twelve-row ceiling (see the vocabulary test) with the removal mode's
    // `Esc` now in it. The label carries the direction so nothing is lost by sharing the line.
    // **`H` picks WHICH piece of a stack the piece-verbs mean.** A floor tile, a wall and its
    // header legally share a cell (different layers), and "the placement under the cursor" cannot
    // name one of three. Each press steps up the stack, the status names the target, and the
    // verbs above act on it until the cursor leaves the cell or Esc releases.
    b(
        Action::CycleTarget,
        KeyCode::KeyH,
        false,
        Context::Map,
        "H",
        "target the stack",
    )
    .at(Home::Control(ControlId::Under)),
    // **The arrows, on the tab that had none.** `Stance::Idle` because a piece already in hand is
    // being placed, not chosen — and the same two keys are then free to mean something else, which is
    // what the axis is for. Indifferent to Shift on purpose: `Shift` is the five-row stride here, the
    // same as every other list in the editor.
    bp(
        Action::PalettePrev,
        KeyCode::ArrowUp,
        false,
        Stance::Idle,
        Context::Map,
        "up",
        "walk the palette / Shift: x5",
    )
    .at(Home::Control(ControlId::Palette)),
    bp(
        Action::PaletteNext,
        KeyCode::ArrowDown,
        false,
        Stance::Idle,
        Context::Map,
        "down",
        "walk the palette / Shift: x5",
    )
    .at(Home::Control(ControlId::Palette)),
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
    b(
        Action::TurnPieceLeft,
        KeyCode::KeyR,
        false,
        Context::Map,
        "R",
        "turn L / turn R / tip x / tip z / straight",
    )
    .at(Home::Control(ControlId::Yaw)),
    b(
        Action::TurnPieceRight,
        KeyCode::KeyT,
        false,
        Context::Map,
        "T",
        "turn L / turn R / tip x / tip z / straight",
    )
    .at(Home::Control(ControlId::Yaw)),
    b(
        Action::TipX,
        KeyCode::KeyY,
        false,
        Context::Map,
        "Y",
        "turn L / turn R / tip x / tip z / straight",
    )
    .at(Home::Control(ControlId::Yaw)),
    b(
        Action::TipZ,
        KeyCode::KeyU,
        false,
        Context::Map,
        "U",
        "turn L / turn R / tip x / tip z / straight",
    )
    .at(Home::Control(ControlId::Yaw)),
    b(
        Action::Straighten,
        KeyCode::KeyV,
        false,
        Context::Map,
        "V",
        "turn L / turn R / tip x / tip z / straight",
    )
    .at(Home::Control(ControlId::Yaw)),
    // **The brackets lift.** Free in this context (the Tiles tab's layer pair is never live with
    // the Map), and vertically suggestive in a way no letter is. One subgrid unit per press, held
    // repeat for a long ride up; the authored offset is `Placed::lift`, the one amendment to
    // "height is never the author's".
    // **`J` steps the rung — the same job it does on the Tiles tab.** The only free letter under the
    // right hand, beside `H`, which is the other key about how you are reading the map.
    //
    // It used to step a *drawn* grid through `[0.5, 1, 2, 4]` m while the lattice a piece landed on
    // was a held modifier — one key, two tabs, two meanings, and the Map's meaning was the one that
    // decided nothing. Two of its four steps were lines no piece could ever sit on. FVS-R-19.
    //
    // The description names metres rather than rung names because the pitch is what an author is
    // deciding between, and it depends on the project's `snap_divisor` — so the row says the shape
    // and the status line, written by `cycle_grid`, says the number.
    b(
        Action::CycleGrid,
        KeyCode::KeyJ,
        false,
        Context::Map,
        "J",
        "grid rung: tile / fine / finer",
    )
    .at(Home::Legend),
    b(
        Action::LiftDown,
        KeyCode::BracketLeft,
        false,
        Context::Map,
        "[",
        "lift / lower this",
    )
    .at(Home::Control(ControlId::Under)),
    b(
        Action::LiftUp,
        KeyCode::BracketRight,
        false,
        Context::Map,
        "]",
        "lift / lower this",
    )
    .at(Home::Control(ControlId::Under)),
    // **The delete key arms a tool; it does not delete.** Removing on the keypress meant the only
    // preview of what was about to go was the author's memory of where the cursor was.
    b(
        Action::Remove,
        KeyCode::KeyX,
        false,
        Context::Map,
        "X",
        "removal mode",
    )
    .at(Home::Control(ControlId::Under)),
    // **`B` is the last free key under the left hand.** The cluster an author's hand already rests on
    // is `Q W E R T / A S D F G / Z X C V B`, and every other letter in it is spoken for — pan, turn
    // view, aim, aim-reset, turn-piece, fill, remove. `B` is bound in the Tiles tab too (`ScanMesh`),
    // which is legal and is exactly the case `Context` exists to model: the two tabs are never live
    // together.
    //
    // **The Map context is at its twelve-row ceiling.** There is no headroom left; the next verb
    // here has to share a `does` with a neighbour or take something else's key — which is exactly
    // what the clone tool does: the Cmd+Z shape, one key, the shifted form for the sibling verb.
    bs(
        Action::MoveMode,
        KeyCode::KeyB,
        false,
        false,
        Context::Map,
        "B",
        "move / Shift: clone / M: keep as a composition",
    )
    .at(Home::Legend),
    bs(
        Action::CloneMode,
        KeyCode::KeyB,
        false,
        true,
        Context::Map,
        "B",
        "move / Shift: clone / M: keep as a composition",
    )
    .at(Home::Legend),
    // **A third verb on a state that already exists.** `Shift+B` drags a box and leaves a set in
    // hand; this keeps that set as a reusable group instead of stamping it. Declared adjacent to the
    // pair above and sharing their `does`, so `rows()` collapses all three into one line — the Map
    // context is AT its twelve-row ceiling and `no_context_carries_more_than_a_learnable_vocabulary`
    // enforces it. `M` for module; it is one of four letters this context has left.
    b(
        Action::GroupFromSet,
        KeyCode::KeyM,
        false,
        Context::Map,
        "M",
        "move / Shift: clone / M: keep as a composition",
    )
    .at(Home::Legend),
    b(
        Action::RenameMap,
        KeyCode::KeyN,
        false,
        Context::Map,
        "N",
        "rename map",
    )
    .at(Home::Control(ControlId::Title)),
    b(
        Action::OwnToggle,
        KeyCode::KeyO,
        false,
        Context::Map,
        "O",
        "pin / unpin",
    )
    .at(Home::Control(ControlId::Under)),
    // **Four ways to cover a region, on one row, each named by its own chord.**
    //
    // `F` was a row of its own reading "flood fill" and the three `G`s shared a row reading
    // *"continue the layout: from the map, the kit's tokens, or its compositions"* — three chords,
    // three sources, and **no way to tell which chord took which source**. That is the collapse rule
    // being used to buy row count with vagueness rather than to state one idea, and it is exactly
    // what `no_context_carries_more_than_a_learnable_vocabulary` was measuring instead of key count.
    //
    // These four *are* one idea at the altitude that matters — take a region, fill it — and they
    // differ only in where the content comes from: the armed brush, or a grammar learned from the
    // map, declared by the kit's tokens, or composed from its tiles. Stated in chord order so a
    // reader can pair them off, which the old row could not support at any length.
    //
    // They must stay adjacent and keep the same string: `rows()` collapses by `does`, and splitting
    // them buys rows the Map context has not got.
    // **All four are `Stance::Idle`**, so the row leaves the list while a proposal is waiting — see
    // [`Stance::Proposed`]. That is not decoration: it is what stops a second generate landing on top
    // of an answer nobody has looked at, and it is what buys the door's row inside the twelve.
    bp(
        Action::Fill,
        KeyCode::KeyF,
        false,
        Stance::Idle,
        Context::Map,
        "F",
        "fill: brush / learned / declared / composed",
    )
    .at(Home::Legend),
    bsp(
        Action::Generate,
        KeyCode::KeyG,
        false,
        false,
        Stance::Idle,
        Context::Map,
        "G",
        "fill: brush / learned / declared / composed",
    )
    .at(Home::Legend),
    bsp(
        Action::GenerateDeclared,
        KeyCode::KeyG,
        false,
        true,
        Stance::Idle,
        Context::Map,
        "G",
        "fill: brush / learned / declared / composed",
    )
    .at(Home::Legend),
    bsp(
        Action::GenerateComposed,
        KeyCode::KeyG,
        true,
        false,
        Stance::Idle,
        Context::Map,
        "G",
        "fill: brush / learned / declared / composed",
    )
    .at(Home::Legend),
    // **The commit door.** `Enter` is free on this tab and taken on every other one, which is the
    // case `Context` exists for. `Esc` is the Global cancel and discards it — stated in the row,
    // because a door with only one visible half reads as a trap.
    bp(
        Action::AcceptProposal,
        KeyCode::Enter,
        false,
        Stance::Proposed,
        Context::Map,
        "Enter",
        "keep this layout / Esc throws it away",
    )
    .at(Home::Legend),
    // **The camera is Global — pan included.** This briefly moved to `Context::Map` to free
    // `W, A, S, D` for the Tiles lattice cursor, on the argument that panning off a staged tile has
    // no way back. That argument was wrong in practice: an author on any tab reaches for these keys
    // to move the view, and a tab where they silently do something else is a tab where the camera
    // feels broken. The lattice cursor moved instead — see its row.
    //
    // Declared W, A, S, D rather than W, S, A, D: the displayed row is these chords in order, and
    // "W, A, S, D" is how the shape is named everywhere. The census's order IS the reading order.
    b(
        Action::PanForward,
        KeyCode::KeyW,
        false,
        Context::Global,
        "W",
        "pan",
    )
    .at(Home::Legend),
    b(
        Action::PanLeft,
        KeyCode::KeyA,
        false,
        Context::Global,
        "A",
        "pan",
    )
    .at(Home::Legend),
    b(
        Action::PanBack,
        KeyCode::KeyS,
        false,
        Context::Global,
        "S",
        "pan",
    )
    .at(Home::Legend),
    b(
        Action::PanRight,
        KeyCode::KeyD,
        false,
        Context::Global,
        "D",
        "pan",
    )
    .at(Home::Legend),
    b(
        Action::TurnViewLeft,
        KeyCode::KeyQ,
        false,
        Context::Global,
        "Q",
        "turn view",
    )
    .at(Home::Legend),
    b(
        Action::TurnViewRight,
        KeyCode::KeyE,
        false,
        Context::Global,
        "E",
        "turn view",
    )
    .at(Home::Legend),
    // **One row for the whole arrow cluster** — up/down walk, left/right switch which list, and
    // holding Shift jumps five at a time. Four bindings sharing one `does` collapse the way
    // `W, A, S, D` collapses, which is what bought the copy row below inside the twelve-row
    // ceiling.
    b(
        Action::PrevCandidate,
        KeyCode::ArrowUp,
        false,
        Context::Meshes,
        "up",
        "walk the lists / Shift: x5",
    )
    .at(Home::Control(ControlId::Pieces)),
    b(
        Action::NextCandidate,
        KeyCode::ArrowDown,
        false,
        Context::Meshes,
        "down",
        "walk the lists / Shift: x5",
    )
    .at(Home::Control(ControlId::Pieces)),
    b(
        Action::FocusCandidates,
        KeyCode::ArrowLeft,
        false,
        Context::Meshes,
        "left",
        "walk the lists / Shift: x5",
    )
    .at(Home::Control(ControlId::Pieces)),
    b(
        Action::FocusLibrary,
        KeyCode::ArrowRight,
        false,
        Context::Meshes,
        "right",
        "walk the lists / Shift: x5",
    )
    .at(Home::Control(ControlId::Pieces)),
    b(
        Action::TypeId,
        KeyCode::KeyI,
        false,
        Context::Meshes,
        "I",
        "type an id",
    )
    .at(Home::Control(ControlId::IdField)),
    // **"mount", not "layer".** It cycles `Descriptor::mount` — what the piece stands on — and the
    // subgrid below has its own `layer y` picker for the lattice slice. One panel said "layer" twice
    // and meant two different things.
    b(
        Action::CycleMount,
        KeyCode::KeyM,
        false,
        Context::Meshes,
        "M",
        "mount",
    )
    .at(Home::Control(ControlId::Mount)),
    // **One verb, two states of a tile.** It read "add to library", which named half of what it
    // does and made the other half look like a refusal: Enter on a piece already in the library
    // answered "already in the library", to an author who had just edited it.
    // **`Stance::Idle`, so `Enter` can be the derivation's door while one is staged.** Adding a tile
    // and answering a proposal about the tile you are looking at are two answers to one key, and the
    // proposal is the one that has to be resolved first — it describes the very lattice an Accept
    // would write.
    bp(
        Action::Accept,
        KeyCode::Enter,
        false,
        Stance::Idle,
        Context::Meshes,
        "Enter",
        "add / update this tile",
    )
    .at(Home::Control(ControlId::Pieces)),
    // **Space opens and closes a heading, and does nothing anywhere else.**
    //
    // Asked for at the keyboard, 2026-08-18: Space on a collapsed pack did nothing, because it is
    // bound in Tiles (take the piece) and Anim (play) but was never bound here. `Enter` already
    // toggled a heading — the row the cursor is on decides what committing means — so this is a
    // second key for that one meaning, not a second meaning.
    //
    // It is worth a row because a folded list is walked with the arrows and opened with the thumb,
    // and reaching for `Enter` mid-scroll is the reach for the mouse that `commit_candidate`'s own
    // note is about. Deliberately inert off a heading: Space must never commit a tile, which is why
    // this is its own action rather than a second key on `Accept`.
    bp(
        Action::FoldPack,
        KeyCode::Space,
        false,
        Stance::Idle,
        Context::Meshes,
        "Space",
        "open / close the pack",
    )
    .at(Home::Control(ControlId::Pieces)),
    bp(
        Action::AcceptEdges,
        KeyCode::Enter,
        false,
        Stance::Proposed,
        Context::Meshes,
        "Enter",
        "keep the derived edges / Esc throws them away",
    )
    .at(Home::Control(ControlId::Detail)),
    // **One row, one idea: what this list offers.** `R` looks at the folders again; `Shift+R` says
    // a folder is not what this kit is built from. The shifted form for the wider act, the same
    // shape `L`/`Shift+L` uses one row down — and sharing the row is what keeps the Meshes tab
    // inside its twelve, which `no_context_carries_more_than_a_learnable_vocabulary` polices.
    bs(
        Action::Rescan,
        KeyCode::KeyR,
        false,
        false,
        Context::Meshes,
        "R",
        "rescan / exclude",
    )
    .at(Home::Control(ControlId::Pieces)),
    bs(
        Action::ExcludePack,
        KeyCode::KeyR,
        false,
        true,
        Context::Meshes,
        // **The BARE key**, like every other row. [`chord_text`] prepends `Shift+` when
        // `needs_shift` is `Some(true)`, so writing it here too rendered `Shift+Shift+R` — invisible
        // for as long as the key list was a table nobody could read, and glaring the moment the
        // chord went on the control it belongs to.
        "R",
        "rescan / exclude",
    )
    .at(Home::Control(ControlId::Pieces)),
    // The Cmd+Z shape again: one key, the shifted form for the reversible-but-destructive sibling.
    // Shift+Delete DEMOTES — back to the candidates, stripped — where bare Delete removes outright.
    bs(
        Action::RemoveTile,
        REMOVE_KEY,
        false,
        false,
        Context::Meshes,
        REMOVE_NAME,
        "send back to NOT IMPORTED",
    )
    .at(Home::Control(ControlId::Pieces)),
    // **This tab's own history**, on the same chords and for the reason `keys.rs`'s undo comment
    // records: an undo you cannot see the effect of is not an undo, so the map's stack is not reachable
    // from here and this one is not reachable from there. Neither is cleared by changing tabs.
    bs(
        Action::UndoTile,
        KeyCode::KeyZ,
        true,
        false,
        Context::Meshes,
        "Z",
        "undo / redo",
    )
    .at(Home::Control(ControlId::Detail)),
    bs(
        Action::RedoTile,
        KeyCode::KeyZ,
        true,
        true,
        Context::Meshes,
        "Z",
        "undo / redo",
    )
    .at(Home::Control(ControlId::Detail)),
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
    b(
        Action::CellForward,
        KeyCode::KeyT,
        false,
        Context::Meshes,
        "T",
        "cell forward",
    )
    .at(Home::Control(ControlId::CellGrid)),
    b(
        Action::CellLeft,
        KeyCode::KeyF,
        false,
        Context::Meshes,
        "F",
        "cell left",
    )
    .at(Home::Control(ControlId::CellGrid)),
    b(
        Action::CellBack,
        KeyCode::KeyG,
        false,
        Context::Meshes,
        "G",
        "cell back",
    )
    .at(Home::Control(ControlId::CellGrid)),
    b(
        Action::CellRight,
        KeyCode::KeyH,
        false,
        Context::Meshes,
        "H",
        "cell right",
    )
    .at(Home::Control(ControlId::CellGrid)),
    b(
        Action::LayerDown,
        KeyCode::BracketLeft,
        false,
        Context::Meshes,
        "[",
        "previous layer",
    )
    .at(Home::Control(ControlId::CellGrid)),
    b(
        Action::LayerUp,
        KeyCode::BracketRight,
        false,
        Context::Meshes,
        "]",
        "next layer",
    )
    .at(Home::Control(ControlId::CellGrid)),
    b(
        Action::CellSolid,
        KeyCode::KeyZ,
        false,
        Context::Meshes,
        "Z",
        "solid this cell",
    )
    .at(Home::Control(ControlId::Mesh)),
    b(
        Action::CellEdge,
        KeyCode::KeyX,
        false,
        Context::Meshes,
        "X",
        "edge this cell",
    )
    .at(Home::Control(ControlId::Mesh)),
    b(
        Action::CellClear,
        KeyCode::KeyV,
        false,
        Context::Meshes,
        "V",
        "clear this cell",
    )
    .at(Home::Control(ControlId::Mesh)),
    // ── BUILD: assembling a tile ─────────────────────────────────────────────────────────────
    //
    // The cursor keeps `T F G H` and `[ ]` — the same inverted T walking the same kind of lattice one
    // tab over, so the hand does not relearn a shape it already has. It is legal because two tabs
    // are never live together, which is the case `Context` exists to model.
    //
    // **There is no door key.** It was `C`, flipping a mode on the mesh tab; the tab strip is the
    // door now, and a strip is a mode nobody can forget — Raskin's condition, met by construction
    // rather than by an indicator (FVS-R-21).
    // **The arrows, and they do two jobs — which is now stated here rather than inside a handler.**
    //
    // `T F G H` held these before and the author's verdict on them was "gross". The replacement put
    // both jobs on one key and let `Build::placing` pick between them, because the census had no way
    // to say "these two can never both fire". It has one now — [`Stance`] — so the split is written
    // down: with nothing in hand the arrows walk the library, with a piece in hand they move *it*.
    // `Space` is the door, the ghost shows which side you are on, and the key list now shows it too.
    //
    // **What that bought, concretely.** One row saying `"list / nudge the mesh / Shift: flush it"` —
    // eight bindings, three ideas, and no way for a reader to tell which was live — becomes three
    // rows of which at most two are ever shown at once. Each is true when it is on screen.
    //
    // **The list walk is `bp`, not `bsp`.** It is *indifferent* to Shift on purpose: `Shift`+arrow is
    // the five-row stride the Meshes tab already uses, and the same shared system serves both tabs.
    // The tile-moving pair is `bsp` because bare and shifted are genuinely different verbs there.
    // **The kit, and it costs no new key.** `left`/`right` were the tab's one unbound pair at
    // `Idle`, reserved by the contract against there being a second list; this is that list.
    bp(
        Action::KitEnter,
        KeyCode::ArrowRight,
        false,
        Stance::Idle,
        Context::Tiles,
        "right",
        "show the kit / Esc goes back",
    )
    .at(Home::Control(ControlId::Pieces)),
    bp(
        Action::KitPrev,
        KeyCode::ArrowUp,
        false,
        Stance::Browsing,
        Context::Tiles,
        "up",
        "walk the kit",
    )
    .at(Home::Control(ControlId::Pieces)),
    bp(
        Action::KitNext,
        KeyCode::ArrowDown,
        false,
        Stance::Browsing,
        Context::Tiles,
        "down",
        "walk the kit",
    )
    .at(Home::Control(ControlId::Pieces)),
    // **`right` descends**: into the kit from the mesh list, then into the tile from the kit. `Esc`
    // backs out of either. A column browser's idiom, and it is what keeps `Enter` meaning one thing:
    // `BuildDrop` is bound across every stance, so an `Enter` here would be one key with two
    // meanings — the collision the census forbids, and the reason the stance axis exists at all.
    bp(
        Action::KitOpen,
        KeyCode::ArrowRight,
        false,
        Stance::Browsing,
        Context::Tiles,
        "right",
        "reopen this tile / left goes back",
    )
    .at(Home::Control(ControlId::Pieces)),
    // **`left` ascends, because a column browser is symmetric.** The strip promised this and the
    // binding did not exist: the hint read *"right reopens / left back"* over an unbound key, and the
    // first fix was to reword the hint to name `Esc` instead. That was backwards — reported at the
    // keyboard, 2026-08-15: *"I would expect left to move back to meshes."* `Esc` still backs out
    // (it backs out of everything), so this adds the direction the idiom implies rather than a
    // second way to do something new. Adjacent to `KitOpen` and sharing its `does`, so it costs no
    // row.
    bp(
        Action::KitLeave,
        KeyCode::ArrowLeft,
        false,
        Stance::Browsing,
        Context::Tiles,
        "left",
        "reopen this tile / left goes back",
    )
    .at(Home::Control(ControlId::Pieces)),
    // **`F` finds.** The filter box had one writer — a mouse click — on the tab that argues
    // keystrokes are faster, so narrowing a 45-piece library meant leaving the keyboard. `F` is free
    // in this context (`Fill` is the Map's and the lattice cursor's `F` is the Meshes tab's, and the
    // two are never live together, which is the case `Context` exists to model). Asked for at the
    // keyboard, 2026-08-15. `Enter` and `Esc` both leave the box, which `filter::keys` already owned.
    b(
        Action::FocusFilter,
        KeyCode::KeyF,
        false,
        Context::Tiles,
        "F",
        "filter the list",
    )
    .at(Home::Control(ControlId::Filter)),
    bp(
        Action::TileListPrev,
        KeyCode::ArrowUp,
        false,
        Stance::Idle,
        Context::Tiles,
        "up",
        "walk the library / Shift: x5",
    )
    .at(Home::Control(ControlId::Pieces)),
    bp(
        Action::TileListNext,
        KeyCode::ArrowDown,
        false,
        Stance::Idle,
        Context::Tiles,
        "down",
        "walk the library / Shift: x5",
    )
    .at(Home::Control(ControlId::Pieces)),
    bsp(
        Action::BuildForward,
        KeyCode::ArrowUp,
        false,
        false,
        Stance::Holding,
        Context::Tiles,
        "up",
        "move the piece",
    )
    .at(Home::Control(ControlId::Members)),
    bsp(
        Action::BuildBack,
        KeyCode::ArrowDown,
        false,
        false,
        Stance::Holding,
        Context::Tiles,
        "down",
        "move the piece",
    )
    .at(Home::Control(ControlId::Members)),
    // **Left/right walk the members, and that is a trade made deliberately.** They used to nudge on
    // the X axis; the cost of taking them is that sideways is reached by turning the view (`Q`/`E`
    // step quarter detents and `step_in_view` maps the arrows through the yaw) or by `Shift`+arrow
    // to the edge. The gain is that the focus can be moved at all — see [`Action::MemberPrev`].
    // **All four arrows move the piece**, mapped through the camera yaw by `step_in_view` -- so on
    // this isometric view `up` is north-east and `left` is north-west, which is what the screen
    // shows. Two of them used to walk the member list instead, which meant the arrows offered half
    // the directions and the missing half did something unrelated.
    bsp(
        Action::BuildLeft,
        KeyCode::ArrowLeft,
        false,
        false,
        Stance::Holding,
        Context::Tiles,
        "left",
        "move the piece",
    )
    .at(Home::Control(ControlId::Members)),
    bsp(
        Action::BuildRight,
        KeyCode::ArrowRight,
        false,
        false,
        Stance::Holding,
        Context::Tiles,
        "right",
        "move the piece",
    )
    .at(Home::Control(ControlId::Members)),
    // The member walk moved here to free them. A prev/next pair, no modifier, under the fingers.
    bsp(
        Action::MemberPrev,
        KeyCode::Comma,
        false,
        false,
        Stance::Holding,
        Context::Tiles,
        ",",
        "step to the previous / next member",
    )
    .at(Home::Control(ControlId::Members)),
    bsp(
        Action::MemberNext,
        KeyCode::Period,
        false,
        false,
        Stance::Holding,
        Context::Tiles,
        ".",
        "step to the previous / next member",
    )
    .at(Home::Control(ControlId::Members)),
    // **Flush is its own verb, not a finer rung.** The author's word for it was *"left aligned"*,
    // and it is what a wall needs: a 0.1 m panel sits flush at -0.45 in a 1 m tile, and no rung of
    // any divisor lands on -0.45 — the position is a function of the piece's own width.
    bsp(
        Action::AlignForward,
        KeyCode::ArrowUp,
        false,
        true,
        Stance::Holding,
        Context::Tiles,
        "up",
        "flush it to that side",
    )
    .at(Home::Control(ControlId::Members)),
    bsp(
        Action::AlignBack,
        KeyCode::ArrowDown,
        false,
        true,
        Stance::Holding,
        Context::Tiles,
        "down",
        "flush it to that side",
    )
    .at(Home::Control(ControlId::Members)),
    bsp(
        Action::AlignLeft,
        KeyCode::ArrowLeft,
        false,
        true,
        Stance::Holding,
        Context::Tiles,
        "left",
        "flush it to that side",
    )
    .at(Home::Control(ControlId::Members)),
    bsp(
        Action::AlignRight,
        KeyCode::ArrowRight,
        false,
        true,
        Stance::Holding,
        Context::Tiles,
        "right",
        "flush it to that side",
    )
    .at(Home::Control(ControlId::Members)),
    b(
        Action::BuildArm,
        KeyCode::Space,
        false,
        Context::Tiles,
        "Space",
        "take the piece / Esc puts it back",
    )
    .at(Home::Control(ControlId::Pieces)),
    b(
        Action::BuildDown,
        KeyCode::BracketLeft,
        false,
        Context::Tiles,
        "[",
        "layer",
    )
    .at(Home::Legend),
    b(
        Action::BuildUp,
        KeyCode::BracketRight,
        false,
        Context::Tiles,
        "]",
        "layer",
    )
    .at(Home::Legend),
    // **`J` cycles the rung, latched.** The same key the Map cycles its drawn grid with, and the same
    // argument: Bier's snap-dragging latches every one of its modal commands, and StickyLines'
    // designers avoid held modifiers because menus and modifiers *"make them lose focus"*. Safe to
    // latch because the drawn grid shows which rung is live.
    b(
        Action::BuildRung,
        KeyCode::KeyJ,
        false,
        Context::Tiles,
        "J",
        "grid deeper by thirds, wrapping",
    )
    .at(Home::Legend),
    // **Both stated with `bs`.** A bare `b` is *indifferent* to Shift by design, so it would swallow
    // the shifted chord rather than sit beside it — the same pair `RemoveTile`/`DemoteTile` makes.
    // A hole rather than a piece is the rarer of the two, so it takes the modifier.
    bs(
        Action::BuildDrop,
        KeyCode::Enter,
        false,
        false,
        Context::Tiles,
        "Enter",
        "drop the piece / Shift: a slot",
    )
    .at(Home::Control(ControlId::Pieces)),
    bs(
        Action::BuildSlot,
        KeyCode::Enter,
        false,
        true,
        Context::Tiles,
        "Enter",
        "drop the piece / Shift: a slot",
    )
    .at(Home::Control(ControlId::Pieces)),
    b(
        Action::BuildTurn,
        KeyCode::KeyR,
        false,
        Context::Tiles,
        "R",
        "turn / remove this / Shift: empty the tile",
    )
    .at(Home::Control(ControlId::Tile)),
    // **`bs`, both of them.** A bare binding is indifferent to Shift and would swallow the shifted
    // chord — the collision the census exists to catch, and the precedent `RemoveTile`/`DemoteTile`
    // set on the Meshes tab.
    bs(
        Action::BuildDropMember,
        REMOVE_KEY,
        false,
        false,
        Context::Tiles,
        REMOVE_NAME,
        "turn / remove this / Shift: empty the tile",
    )
    .at(Home::Control(ControlId::Tile)),
    bs(
        Action::ClearTile,
        REMOVE_KEY,
        false,
        true,
        Context::Tiles,
        REMOVE_NAME,
        "turn / remove this / Shift: empty the tile",
    )
    .at(Home::Control(ControlId::Tile)),
    // **No save key here, on purpose.** `Cmd+S` is Global and already means *save what is open*; a
    // second one in this context would collide with it, and the collision is the census pointing out
    // that they are the same verb. The handler asks which mode is live.
    b(
        Action::BuildNew,
        KeyCode::KeyN,
        false,
        Context::Tiles,
        "N",
        "name a new tile",
    )
    .at(Home::Control(ControlId::Tile)),
    // **Its own stack, like every other tab's.** `UndoTile` is the *mesh* tab's, over library edits;
    // this one is over the tile in hand. Two tabs editing different files through one stack would
    // make "undo" mean whichever thing was touched last, which is the shape `Action::UndoTile`'s own
    // note already argues against.
    bs(
        Action::UndoBuild,
        KeyCode::KeyZ,
        true,
        false,
        Context::Tiles,
        "Z",
        "undo / redo",
    )
    .at(Home::Control(ControlId::Detail)),
    bs(
        Action::RedoBuild,
        KeyCode::KeyZ,
        true,
        true,
        Context::Tiles,
        "Z",
        "undo / redo",
    )
    .at(Home::Control(ControlId::Detail)),
    b(
        Action::ScanMesh,
        KeyCode::KeyB,
        false,
        Context::Meshes,
        "B",
        "rescan solid / turn mesh x / y / z",
    )
    .at(Home::Control(ControlId::Mesh)),
    b(
        Action::RotateMeshX,
        KeyCode::KeyN,
        false,
        Context::Meshes,
        "N",
        "rescan solid / turn mesh x / y / z",
    )
    .at(Home::Control(ControlId::Mesh)),
    b(
        Action::RotateMeshY,
        KeyCode::KeyO,
        false,
        Context::Meshes,
        "O",
        "rescan solid / turn mesh x / y / z",
    )
    .at(Home::Control(ControlId::Mesh)),
    b(
        Action::RotateMeshZ,
        KeyCode::KeyP,
        false,
        Context::Meshes,
        "P",
        "rescan solid / turn mesh x / y / z",
    )
    .at(Home::Control(ControlId::Mesh)),
    // **The VLM labeler's cluster** — one row, three verbs. `L` photographs the focused piece and
    // asks the model; `Shift+L` walks everything missing judgement fields (and cancels a running
    // walk); `Shift+Y` abandons the batch. A label applies on arrival, so there is no per-label
    // confirm verb to bind. `L` is unbound in Tiles and Global; the L pair is the Cmd+Z shape — one
    // key, the shifted form for the bigger sweep.
    bs(
        Action::SuggestLabels,
        KeyCode::KeyL,
        false,
        false,
        Context::Meshes,
        "L",
        "suggest / all / abandon",
    )
    .at(Home::Control(ControlId::Tags)),
    bs(
        Action::SuggestAll,
        KeyCode::KeyL,
        false,
        true,
        Context::Meshes,
        "L",
        "suggest / all / abandon",
    )
    .at(Home::Control(ControlId::Tags)),

    bs(
        Action::DiscardAllSuggestions,
        KeyCode::KeyY,
        false,
        true,
        Context::Meshes,
        "Y",
        "suggest / all / abandon",
    )
    .at(Home::Control(ControlId::Tags)),
    // **`/` narrows the tag block, and it is this context's only mouse-shaped hole.**
    //
    // The block draws the project's whole vocabulary — 55 chips on the shipped kit, of which a piece
    // holds three to six — and every one of them was a mouse target and nothing else: `on_tag_chip`
    // was their only writer. `docs/ui.md` §4.2 is the rule that makes that a defect rather than a
    // preference: everything reachable by mouse is reachable by keyboard and vice versa.
    //
    // **A row this context pays for, deliberately.** `no_context_carries_more_than_a_learnable_
    // vocabulary` is the ceiling and it is not one to route around — but the alternative was leaving
    // the largest control in the pane keyboard-unreachable, which is a worse answer than one more
    // chord. `/` because it is what "search here" has meant for forty years, and because Meshes has
    // no symbol keys at all: `[`/`]` walk layers and nothing else in this context is punctuation.
    //
    // Homed on the block rather than on the box: `chrome::Control(ControlId::Tags)` is the column,
    // the box is its first row, and `filter::spawn` deliberately brings no control of its own for
    // exactly this reason — two `Control`s in one subtree is the badge system with two answers.
    b(
        Action::FocusTagFilter,
        KeyCode::Slash,
        false,
        Context::Meshes,
        "/",
        "filter the tags / Enter takes the one match",
    )
    .at(Home::Control(ControlId::Tags)),
    // The arrows are the Tiles tab's too. Legal, and the reason the census models context at all:
    // the two tabs are never live together, so the same key means one thing in each.
    b(
        Action::PrevRig,
        KeyCode::ArrowUp,
        false,
        Context::Anim,
        "up",
        "walk the rigs / Shift: x5",
    )
    .at(Home::Control(ControlId::Rigs)),
    b(
        Action::NextRig,
        KeyCode::ArrowDown,
        false,
        Context::Anim,
        "down",
        "walk the rigs / Shift: x5",
    )
    .at(Home::Control(ControlId::Rigs)),
    // Enter is the Tiles tab's Accept too — same legal cross-context share as the arrows above.
    b(
        Action::AdoptMeasured,
        KeyCode::Enter,
        false,
        Context::Anim,
        "Enter",
        "adopt measured values into rigs.ron",
    )
    .at(Home::Control(ControlId::Detail)),
    // ── Compose ──────────────────────────────────────────────────────────────────────────────────
    //
    // **Deliberately reusing Tiles' and Anim's letters.** The three tabs are never live at once, and
    // `Context::overlaps` says so, so `up`/`down`/`Enter` mean walk-walk-commit in all three rather
    // than being three arbitrary triples an author has to learn apart. A flat uniqueness rule would
    // force a worse binding here, which is the whole reason contexts exist.
    b(
        Action::ComposePrev,
        KeyCode::ArrowUp,
        false,
        Context::Compose,
        "up",
        "walk the focused list",
    )
    .at(Home::Control(ControlId::Detail)),
    b(
        Action::ComposeNext,
        KeyCode::ArrowDown,
        false,
        Context::Compose,
        "down",
        "walk the focused list",
    )
    .at(Home::Control(ControlId::Detail)),
    b(
        Action::ComposeArm,
        KeyCode::Enter,
        false,
        Context::Compose,
        "Enter",
        "arm this composition for the Map",
    )
    .at(Home::Control(ControlId::Detail)),
    // Symmetric with the pair above: `up`/`down` walk the groups, `left`/`right` walk the members of
    // the one you are on. Costs no letter, and the two cursors read as one idea.
    b(
        Action::ComposeMemberPrev,
        KeyCode::ArrowLeft,
        false,
        Context::Compose,
        "left",
        "which list the arrows walk",
    )
    .at(Home::Control(ControlId::Detail)),
    b(
        Action::ComposeMemberNext,
        KeyCode::ArrowRight,
        false,
        Context::Compose,
        "right",
        "which list the arrows walk",
    )
    .at(Home::Control(ControlId::Detail)),
    // **The Tiles lattice cluster, on the other lattice.** `Context` overlaps by design, so one hand
    // shape means one thing on both surfaces rather than colliding. Declared adjacent to `[` and `]`
    // and sharing their `does`, so `rows()` collapses all six into one row.
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
    // **A quarter bare, 15° on Shift — and that order is the argument, not a preference.**
    //
    // A group is a tile. `adjacency::quarter_turns` refuses a yaw that is not a multiple of 90 and
    // names the piece, so a member carrying edge tokens turned 45° makes the whole group's interface
    // underivable. It only bites a tokened member — `interface` skips the rest — so 15° stays
    // reachable for a chair drawn up to a table, behind a modifier where it cannot be hit by accident.
    //
    // `Y`/`U` rather than the Map's `R`/`T`, which are both spoken for here — `R` records and `T`
    // seats. They keep the whole surface under one hand (`T F G H` seat, `Y U` turn, `[ ]` raise), and
    // the Map's own row for `Y`/`U` is "turn left / turn right / tip x / tip z", so the family is the same one.
    //
    // **Not `,` and `.`, and the test is why.** `rows()` joins a collapsed row's chords with `", "`,
    // so a chord that *is* a comma comes out as `, , .` and cannot be read back —
    // `collapsing_rows_loses_nothing` failed on exactly that, naming the vanished chord.
    // **Not `,` and `.`, for the second time, and the test caught it both times.**
    //
    // `rows()` joins a collapsed row's chords with `", "`, so a comma chord is unreadable the moment
    // it shares a row with anything — including its own pair. `,`/`.` were tried for turn and printed
    // `, , .`; tried again here and did it again. A comma cannot be a chord in this editor while the
    // separator is a comma, and that is a property of the census, not of this row.
    // **The two verbs this tab was missing.** It could refine a group and not make one, so every
    // group had to be captured on the Map first — which is a fine way to work and a bad only way.
    // **The carousel, and this tab's TWELFTH row — the last one it has.** The focal composition stands
    // full size with its neighbours either side as miniatures; these step the strip.
    //
    // Its own pair rather than the arrows, because the arrows belong to whichever of the three lists
    // has focus: stepping to the next group while editing a member would otherwise cost
    // `left left up right right`.
    //
    // `O`/`P` are adjacent and free — `Context::Compose` overlaps nothing but `Global` (see
    // `Context::overlaps`), so the Map's own `O` is no obstacle. After this row,
    // `no_context_carries_more_than_a_learnable_vocabulary` is AT its ceiling for this context: a new
    // Compose verb now costs a merge or a removal, not an addition.
    b(
        Action::CarouselPrev,
        KeyCode::KeyO,
        false,
        Context::Compose,
        "O",
        "previous / next composition",
    )
    .at(Home::Control(ControlId::Detail)),
    b(
        Action::CarouselNext,
        KeyCode::KeyP,
        false,
        Context::Compose,
        "P",
        "previous / next composition",
    )
    .at(Home::Control(ControlId::Detail)),
    // One row, two chords — the Map, Tiles and Anim pairs again.
    // One row, two chords, same as the Map and Tiles undo pairs.
    bs(
        Action::UndoBench,
        KeyCode::KeyZ,
        true,
        false,
        Context::Anim,
        "Z",
        "undo / redo the last write",
    )
    .at(Home::Control(ControlId::Detail)),
    bs(
        Action::RedoBench,
        KeyCode::KeyZ,
        true,
        true,
        Context::Anim,
        "Z",
        "undo / redo the last write",
    )
    .at(Home::Control(ControlId::Detail)),
    // The staged figure's phase scrub. Left/Right are held keys (`pressed`, like panning), Shift
    // slows the sweep — read in the handler, the `rotate_mesh` precedent, so `needs_shift` stays
    // `None` and the escape hatch keeps working.
    b(
        Action::ScrubBack,
        KeyCode::ArrowLeft,
        false,
        Context::Anim,
        "left",
        "scrub phase (Shift: fine)",
    )
    .at(Home::Control(ControlId::Detail)),
    b(
        Action::ScrubFwd,
        KeyCode::ArrowRight,
        false,
        Context::Anim,
        "right",
        "scrub phase (Shift: fine)",
    )
    .at(Home::Control(ControlId::Detail)),
    b(
        Action::PlayPause,
        KeyCode::Space,
        false,
        Context::Anim,
        "Space",
        "play / scrub",
    )
    .at(Home::Control(ControlId::Detail)),
    b(
        Action::CheckAllRigs,
        KeyCode::KeyC,
        false,
        Context::Anim,
        "C",
        "check all rigs",
    )
    .at(Home::Control(ControlId::Rigs)),
    // G is the Map tab's Generate and a Tiles cell-cursor key — the same legal cross-context
    // share as the arrows above. V is the Map tab's Straighten and a Tiles lattice key.
    b(
        Action::ToggleGhost,
        KeyCode::KeyG,
        false,
        Context::Anim,
        "G",
        "ghost: measured over declared",
    )
    .at(Home::Control(ControlId::Detail)),
    b(
        Action::CycleCamPreset,
        KeyCode::KeyV,
        false,
        Context::Anim,
        "V",
        "view: figure / feet / side / ground",
    )
    .at(Home::Legend),
];

/// **Every field stated.** The three constructors below are all this one with defaults filled in, so a
/// new axis is added here once rather than in each of them — which is what let `needs_stance` arrive
/// without touching a single existing row.
///
/// **[`Home`] is the one axis that could not arrive that way**, and the difference is worth stating:
/// a default is only honest when "not stated" is a real answer. `needs_stance: None` means *this
/// binding does not care what is in hand*; there is no corresponding `home: None`, because a verb
/// with nowhere to be drawn is a verb that vanishes from the only surface that announces it. So the
/// constructors stop one field short and [`Draft::at`] finishes the row.
#[allow(clippy::too_many_arguments)]
const fn full(
    action: Action,
    key: KeyCode,
    needs_mod: bool,
    needs_shift: Option<bool>,
    needs_stance: Option<Stance>,
    context: Context,
    chord: &'static str,
    does: &'static str,
) -> Draft {
    Draft {
        action,
        key,
        needs_mod,
        needs_shift,
        needs_stance,
        context,
        chord,
        does,
    }
}

const fn b(
    action: Action,
    key: KeyCode,
    needs_mod: bool,
    context: Context,
    chord: &'static str,
    does: &'static str,
) -> Draft {
    // Indifferent to Shift and to what is in hand — see [`Binding::needs_shift`] and
    // [`Binding::needs_stance`] for why those are the defaults.
    full(action, key, needs_mod, None, None, context, chord, does)
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
) -> Draft {
    full(
        action,
        key,
        needs_mod,
        Some(shift),
        None,
        context,
        chord,
        does,
    )
}

/// A row that cares what is in hand, and not about Shift.
const fn bp(
    action: Action,
    key: KeyCode,
    needs_mod: bool,
    stance: Stance,
    context: Context,
    chord: &'static str,
    does: &'static str,
) -> Draft {
    full(
        action,
        key,
        needs_mod,
        None,
        Some(stance),
        context,
        chord,
        does,
    )
}

/// A row that cares about both — `Shift`+arrow while holding a piece is the case that needs it.
#[allow(clippy::too_many_arguments)]
const fn bsp(
    action: Action,
    key: KeyCode,
    needs_mod: bool,
    shift: bool,
    stance: Stance,
    context: Context,
    chord: &'static str,
    does: &'static str,
) -> Draft {
    full(
        action,
        key,
        needs_mod,
        Some(shift),
        Some(stance),
        context,
        chord,
        does,
    )
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

/// **The chord for an action, rendered** — for putting next to the control that does it.
///
/// Goes through [`chord_text`], which is the one place a chord becomes text. It used to return the
/// bare `chord` field, which **silently drops the modifier**: every message about `Cmd+2` came out
/// saying "2 edits it in place", and an author reading that reasonably concluded the tool was
/// broken. Two live callers were wrong that way and neither could be spotted by reading them.
///
/// The deleted `rows()` carried the same note for the same reason — it pushed `b.chord` and
/// collapsed `Cmd+Z` and `Shift+Cmd+Z` into "Cmd+Z, Z", naming a key that does not do that. The
/// badge rows render through [`chord_column`], the same single path. One renderer, no exceptions.
pub fn chord(action: Action) -> String {
    chord_text(binding(action))
}

/// Everything live in one context, in declaration order — never sorted, never reordered by use.
///
/// Samp 2011, via `docs/ui.md` §3.5: a menu's cost is paid at first sight, so **fix item positions
/// permanently and never reorder by recency**. A key list that rearranges itself is one nobody can
/// build a memory of.
/// **What is live in one context, in one [`Stance`]** — declaration order, never sorted.
///
/// A row that declares no stance is live in both, so an `Idle` list and a `Holding` list share
/// everything except the handful of keys the phase actually changes. That is the property that makes
/// the two lists readable as one vocabulary with a variant, rather than two vocabularies.
pub fn in_context(context: Context, stance: Stance) -> impl Iterator<Item = &'static Binding> {
    BINDINGS
        .iter()
        .filter(move |b| b.context == context && stance_ok(b, stance))
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
pub fn alt_held(keys: &ButtonInput<KeyCode>) -> bool {
    ALT_KEYS.iter().any(|k| keys.pressed(*k))
}

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

/// **One badge: the chords that share a home, what they do, and which actions they are.**
///
/// The rendered form of a [`Home`]. Carries its actions rather than its chord text so the thing that
/// draws it can ask [`pressed`] whether one of them is firing right now — a chord is a *rendering*,
/// and re-parsing `Shift+Cmd+Z` from a string would be a second parser for a question the census
/// already answers.
#[derive(Debug)]
pub struct Badge {
    pub home: Home,
    pub chord: String,
    pub does: &'static str,
    pub actions: Vec<Action>,
}

impl Badge {
    /// **The same badge with the chords this door cannot honour taken out of it**, or `None` if that
    /// is all of them.
    ///
    /// One badge can be several chords, and the slot keys are exactly that case: `1`, `2` and `3`
    /// share a `does` and collapse into one row. On a door showing a single panel, `1` still fires
    /// and the other two do not — so the badge is trimmed rather than kept or dropped whole, and its
    /// column is re-rendered through [`chord_column`], the same function that wrote it.
    ///
    /// See [`Action::fires_on_a_door_of`] for why the census cannot answer this on its own.
    pub fn on_a_door_of(mut self, panels: usize) -> Option<Badge> {
        let before = self.actions.len();
        self.actions.retain(|a| a.fires_on_a_door_of(panels));
        if self.actions.is_empty() {
            return None;
        }
        if self.actions.len() != before {
            self.chord = chord_column(&self.actions);
        }
        Some(self)
    }
}

/// **The key list as it should be SEEN** — [`rows`], split along its bindings' homes.
///
/// Exactly [`rows`]'s collapse with one more condition: two adjacent bindings join only if they agree
/// about *where* as well as about *what*. So `R, T, Y, U, V` stays one badge on the piece it turns,
/// and a row whose bindings disagreed would split rather than pick one of them — which is the honest
/// answer and not a special case.
///
/// Declaration order, never sorted, for the reason [`rows`] gives: Samp 2011 via `docs/ui.md` §3.5, a
/// list that rearranges itself is one nobody can build a memory of. That matters more here than it did
/// there, because a badge's *position* is half of what is being learned — ExposeHK's third goal
/// (`10.1145/2470654.2470735`) is that hotkeys sit at the spatial location of the control they act
/// through, and Schramm, Gutwin & Cockburn 2016 measured spatial-memory shortcuts 700 ms faster than
/// visually-guided ones.
pub fn badges(context: Context, stance: Stance) -> Vec<Badge> {
    let mut out: Vec<Badge> = Vec::new();
    for b in in_context(context, stance) {
        match out.last_mut() {
            Some(last) if last.does == b.does && last.home == b.home => last.actions.push(b.action),
            _ => out.push(Badge {
                home: b.home,
                chord: String::new(),
                does: b.does,
                actions: vec![b.action],
            }),
        }
    }
    // **The column is rendered once, from the actions, after the collapse.** It used to be built
    // during it, appending as each binding joined — which meant a badge that later *lost* a chord
    // had no way to say so but to re-render it somewhere else, and two renderers of one column is
    // how `Cmd+` goes missing from exactly one of them. See [`Badge::on_a_door_of`].
    for b in &mut out {
        b.chord = chord_column(&b.actions);
    }
    out
}

/// **How a badge's chords read**, and the only place that decides.
fn chord_column(actions: &[Action]) -> String {
    actions
        .iter()
        .map(|a| chord(*a))
        .collect::<Vec<_>>()
        .join(", ")
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
            .add_systems(Update,
                (crate::editor::sense_context.in_set(Phase::Sense))
                    .run_if(in_state(crate::screen::Screen::Editor)),
            );
    }
}

/// Who owns the keyboard this frame, and what is in hand. Written once, in [`Phase::Sense`], and read
/// everywhere else.
///
/// **A tuple struct with the context still at `.0`** so the six places that compare it to a `Context`
/// directly (`live.0 == Context::Tiles`) read the way they always did. The dispatch functions take the
/// whole thing, because a binding that declares a [`Stance`] cannot be judged from the context alone.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Live(pub Context, pub Stance);

impl Default for Live {
    fn default() -> Self {
        // The tab the editor opens on — `tiles::Mode::default()` is `Map`. Stated rather than
        // derived, because `Context`'s own first variant is `Global`, which is not a tab.
        Live(Context::Map, Stance::Idle)
    }
}

/// Who owns the keyboard: the live tab, unless a field is taking raw keys — and what is in hand.
///
/// **Typing clears the stance to [`Stance::Idle`].** Not because the piece is put down — it is not —
/// but because `Typing` suppresses every action anyway ([`fires_in`]), and leaving a live `Holding`
/// beside it would be a second state that decides nothing while looking like it decides something.
pub fn live(tab: Context, typing: bool, stance: Stance) -> Live {
    if typing {
        Live(Context::Typing, Stance::Idle)
    } else {
        Live(tab, stance)
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

/// Does this binding's [`Stance`] requirement hold right now? `None` is always satisfied — the same
/// shape as [`shift_ok`], and for the same reason: an axis a row does not mention must not gate it.
fn stance_ok(b: &Binding, live: Stance) -> bool {
    match b.needs_stance {
        None => true,
        Some(want) => want.overlaps(live),
    }
}

/// Every gate a binding must pass except the key itself, in one place so the three entry points below
/// cannot drift from each other.
fn allowed(b: &Binding, keys: &ButtonInput<KeyCode>, live: Live) -> bool {
    fires_in(b.context, live.0)
        && stance_ok(b, live.1)
        // A bare binding must not fire while the modifier is held, or `Cmd+S` would also pan the
        // camera back — and `Cmd+Z` would turn the brush as well as undo, now that `Z` aims.
        && b.needs_mod == mod_held(keys)
        && shift_ok(b, keys)
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
/// `if *mode != Mode::Meshes` early returns this replaced were that second census.
pub fn just_pressed(keys: &ButtonInput<KeyCode>, live: Live, action: Action) -> bool {
    let b = binding(action);
    allowed(b, keys, live) && keys.just_pressed(b.key)
}

/// Is this action's key held? For the continuous ones — panning.
pub fn pressed(keys: &ButtonInput<KeyCode>, live: Live, action: Action) -> bool {
    let b = binding(action);
    allowed(b, keys, live) && keys.pressed(b.key)
}

/// **How long a held key waits before firing again**, and then between repeats.
///
/// **One number for the whole application**, chosen at the keyboard 2026-08-18 — walking a
/// 300-candidate list was too slow at the old 0.150 s, and the answer to "which surface" was
/// "all of them". So every repeating key moved together rather than the lists growing a constant
/// of their own: two cadences would be two answers to how fast a held key goes, and an author who
/// learned the lists would have been wrong about the turn keys.
///
/// **Walked back a third on 2026-08-18**, the same day it was halved: 0.150 → 0.075 → 0.100. The
/// halving was right about the direction and overshot, which is what tuning a cadence by feel looks
/// like — 75 ms is about seven rows a second and the selection outran the eye reading it. This is
/// still a third faster than the original.
///
/// At [`crate::editor::YAW_STEP`] this sweeps a full turn in about 2.4 s.
///
/// **The known cost, stated rather than discovered.** The first repeat lands one interval after the
/// press, so a deliberate tap much over 100 ms fires *twice* — the hazard the original 0.150 was
/// picked to avoid, since a tap is rarely under about 120 ms. The margin is thinner than it was and
/// this is the value to suspect if tapping starts double-stepping. The fix then is not a slower
/// number: it is the split every OS keyboard makes, a long delay before the first repeat and a
/// short interval after it. That is one constant and one line in [`countdown`] away, and
/// deliberately not taken yet — nobody has reported a double step.
pub const REPEAT_SECS: f32 = 0.100;

/// **The fastest a held key will ever go**, reached after [`REPEAT_RAMP_SECS`] of holding.
///
/// A single number could not serve both jobs a held arrow has. Stepping two rows to read them wants
/// about 100 ms; crossing 300 candidates to reach the one you are thinking of wants a number that
/// would be unreadable if you got it immediately — which is exactly what 0.075 felt like, and why
/// it was walked back the same day it landed. An acceleration curve is not a compromise between
/// the two, it is both: the key starts at [`REPEAT_SECS`] and only becomes a traversal tool once
/// you have held it long enough to have meant it.
///
/// This is the shape every OS keyboard and every DCC scrubber uses, for the same reason.
pub const REPEAT_FAST_SECS: f32 = 0.030;

/// **How long a key must be held before it reaches [`REPEAT_FAST_SECS`].**
///
/// Long enough that a two-or-three-row nudge never sees any of the ramp — at 100 ms a step, three
/// quarters of a second is seven rows, and by then an author walking a list rather than reading one
/// has made that obvious. All three of these numbers are by feel and meant to be tuned; they are
/// named and separate so that tuning one does not mean rediscovering what the others were for.
pub const REPEAT_RAMP_SECS: f32 = 0.75;

/// **The gap before the next repeat, given how long the key has already been down.**
///
/// Linear from [`REPEAT_SECS`] to [`REPEAT_FAST_SECS`] across [`REPEAT_RAMP_SECS`], then flat.
/// Linear rather than eased on purpose: an author tuning this reads two endpoints and a duration
/// and can predict the middle, which an easing curve takes away for a difference nobody has asked
/// for.
pub fn interval_after(held: f32) -> f32 {
    let t = (held / REPEAT_RAMP_SECS).clamp(0.0, 1.0);
    REPEAT_SECS + (REPEAT_FAST_SECS - REPEAT_SECS) * t
}

/// **What a countdown belongs to.**
///
/// Two, because the editor and the menu name a key differently and neither should be made to speak
/// the other's vocabulary. Inside the editor a repeat belongs to an [`Action`], which is what
/// carries the context and stance rules that decide whether the key is even live. The chooser runs
/// before any of that exists — it reads raw [`KeyCode`], deliberately, being the screen you arrive
/// at — so its repeats are keyed by the key.
///
/// One enum rather than two resources: the countdown arithmetic is written once, in [`countdown`],
/// and a second store would be a second place for the cadence to drift.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepeatId {
    /// A binding in the editor's census.
    Action(Action),
    /// A raw key, for a screen that has no census — see [`repeating_key`].
    Key(KeyCode),
}

/// One key that is currently down: what it is, how long until its next repeat, and how long it has
/// been held — the last of which is what [`interval_after`] reads to accelerate.
struct Countdown {
    id: RepeatId,
    left: f32,
    held: f32,
}

/// Per-key countdown to the next repeat, for [`repeating`] and [`repeating_key`].
///
/// A `Vec` rather than a map because [`Action`] is `Eq` but not `Hash`, and because the list only
/// ever holds the keys actually down — at most a couple.
#[derive(Resource, Default)]
pub struct Repeat(Vec<Countdown>);

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
    live: Live,
    action: Action,
    repeat: &mut Repeat,
    dt: f32,
) -> bool {
    countdown(
        repeat,
        RepeatId::Action(action),
        pressed(keys, live, action),
        just_pressed(keys, live, action),
        dt,
    )
}

/// **The same cadence, for a screen with no key census.**
///
/// The chooser reads raw [`KeyCode`] on purpose — it is the door, and the editor's contexts and
/// stances do not exist yet there. This gives its arrows the repeat every list inside the editor
/// has, off the one [`REPEAT_SECS`], so "hold to walk a long list" means the same thing on both
/// sides of the door.
pub fn repeating_key(
    keys: &ButtonInput<KeyCode>,
    key: KeyCode,
    repeat: &mut Repeat,
    dt: f32,
) -> bool {
    countdown(
        repeat,
        RepeatId::Key(key),
        keys.pressed(key),
        keys.just_pressed(key),
        dt,
    )
}

/// **The countdown itself, written once.**
///
/// `held` and `fresh` are the caller's answer to "is this down" and "did it arrive this frame",
/// because that is the only part the editor and the menu disagree about.
fn countdown(repeat: &mut Repeat, id: RepeatId, down: bool, fresh: bool, dt: f32) -> bool {
    if !down {
        // **Releasing forgets the ramp**, which is what makes acceleration safe to have at all: a
        // key let go and pressed again starts slow, so tapping never inherits the speed of the
        // hold before it.
        repeat.0.retain(|c| c.id != id);
        return false;
    }
    if fresh {
        repeat.0.retain(|c| c.id != id);
        repeat.0.push(Countdown {
            id,
            left: REPEAT_SECS,
            held: 0.0,
        });
        return true;
    }
    let Some(c) = repeat.0.iter_mut().find(|c| c.id == id) else {
        return false;
    };
    c.held += dt;
    c.left -= dt;
    if c.left <= 0.0 {
        // Add rather than reset, so a long frame does not silently swallow the overshoot and drift
        // the cadence slower than it says it is.
        c.left += interval_after(c.held);
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
            Action::TabSlot1,
            Action::TabSlot2,
            Action::TabSlot3,
            Action::ComposePrev,
            Action::ComposeNext,
            Action::ComposeArm,
            Action::ComposeMemberPrev,
            Action::ComposeMemberNext,
            Action::CarouselPrev,
            Action::CarouselNext,
            Action::Save,
            Action::MainMenu,
            Action::Undo,
            Action::Redo,
            Action::Shortcuts,
            Action::EditTile,
            Action::Straighten,
            Action::Cancel,
            Action::Fill,
            Action::Remove,
            Action::MoveMode,
            Action::CloneMode,
            Action::RenameMap,
            Action::OwnToggle,
            Action::Generate,
            Action::GenerateDeclared,
            Action::GenerateComposed,
            Action::CycleGrid,
            Action::GroupFromSet,
            Action::PanForward,
            Action::PanBack,
            Action::PanLeft,
            Action::PanRight,
            Action::TurnViewLeft,
            Action::TurnViewRight,
            Action::PrevCandidate,
            Action::NextCandidate,
            Action::TypeId,
            Action::CycleMount,
            Action::Accept,
            Action::FoldPack,
            Action::AcceptEdges,
            Action::Rescan,
            Action::RemoveTile,
            Action::UndoTile,
            Action::RedoTile,
            Action::CellLeft,
            Action::CellRight,
            Action::CellForward,
            Action::CellBack,
            Action::LayerDown,
            Action::LayerUp,
            Action::CellSolid,
            Action::CellEdge,
            Action::CellClear,
            Action::ScanMesh,
            Action::RotateMeshX,
            Action::RotateMeshY,
            Action::RotateMeshZ,
            Action::FocusCandidates,
            Action::FocusLibrary,
            Action::CopyInfo,
            Action::SuggestLabels,
            Action::SuggestAll,
            Action::ExcludePack,
            Action::DiscardAllSuggestions,
            Action::PrevRig,
            Action::NextRig,
            Action::AdoptMeasured,
            Action::UndoBench,
            Action::RedoBench,
            Action::ScrubBack,
            Action::ScrubFwd,
            Action::PlayPause,
            Action::CheckAllRigs,
            Action::ToggleGhost,
            Action::CycleCamPreset,
            Action::TurnPieceLeft,
            Action::TurnPieceRight,
            Action::TipX,
            Action::TipZ,
            Action::LiftUp,
            Action::LiftDown,
            Action::CycleTarget,
            Action::PalettePrev,
            Action::PaletteNext,
            Action::AcceptProposal,
            // The Tiles tab's verbs. `Build*` rather than `Tile*` because they name the act, not the
            // tab — dropping and turning are building whatever the strip calls the place it happens.
            Action::TileListPrev,
            Action::TileListNext,
            Action::BuildForward,
            Action::BuildBack,
            Action::BuildLeft,
            Action::BuildRight,
            Action::MemberPrev,
            Action::MemberNext,
            Action::BuildDown,
            Action::BuildUp,
            Action::BuildRung,
            Action::BuildDrop,
            Action::BuildSlot,
            Action::BuildArm,
            Action::UndoBuild,
            Action::RedoBuild,
            Action::AlignForward,
            Action::AlignBack,
            Action::AlignLeft,
            Action::AlignRight,
            Action::BuildTurn,
            Action::BuildDropMember,
            Action::ClearTile,
            Action::BuildNew,
            Action::KitEnter,
            Action::KitPrev,
            Action::KitNext,
            Action::KitOpen,
            Action::KitLeave,
            Action::FocusFilter,
            Action::FocusTagFilter,
            Action::ShowErrors,
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
                binding(b.action).action,
                b.action,
                "`{}` ({}) is in the table but `binding` does not return it",
                b.chord,
                b.does
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
                // **And the same for the stance**, on the identical rule: two rows asking for
                // different things to be in hand can never both fire, which is what lets the arrows
                // walk a list and move a piece on one key without a handler deciding between them.
                let stance_exclusive = matches!(
                    (a.needs_stance, b.needs_stance),
                    (Some(x), Some(y)) if !x.overlaps(y)
                );
                if a.key == b.key
                    && a.needs_mod == b.needs_mod
                    && !shift_exclusive
                    && !stance_exclusive
                    && a.context.overlaps(b.context)
                {
                    clashes.push(format!(
                        "{:?} ({:?}, {:?}) and {:?} ({:?}, {:?}) both take `{}`",
                        a.action,
                        a.context,
                        a.needs_stance,
                        b.action,
                        b.context,
                        b.needs_stance,
                        a.chord
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

    /// **One key, two jobs, and only one of them fires** — the property the whole [`Stance`] axis
    /// exists to give, asserted on the pair that motivated it.
    ///
    /// Before this, `ArrowUp` on the Tiles tab was a single action whose meaning `build::Build::placing`
    /// decided inside a handler. A test could not reach that decision without building an `App`, so
    /// the only thing standing between "walk the list" and "move the piece" was an `if` nobody could
    /// see from the key table.
    #[test]
    fn the_arrows_walk_a_list_or_move_a_piece_but_never_both() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::ArrowUp);

        let idle = Live(Context::Tiles, Stance::Idle);
        assert!(
            just_pressed(&input, idle, Action::TileListPrev),
            "with nothing in hand, up walks the library"
        );
        assert!(
            !just_pressed(&input, idle, Action::BuildForward),
            "with nothing in hand, up must not move a piece there is none of"
        );

        let holding = Live(Context::Tiles, Stance::Holding);
        assert!(
            just_pressed(&input, holding, Action::BuildForward),
            "with a piece in hand, up moves it"
        );
        assert!(
            !just_pressed(&input, holding, Action::TileListPrev),
            "with a piece in hand, up must not also walk the library out from under it"
        );
    }

    /// **The key list answers `what do the arrows do right now`**, which is the only question a modal
    /// grammar raises and the one the old single row could not answer.
    ///
    /// Asserted on the rendered rows rather than the bindings, because the rows are what an author
    /// reads — and the failure being fixed was a row that stayed true by being vague.
    #[test]
    fn the_tiles_key_list_changes_when_a_piece_is_picked_up() {
        let idle = badges(Context::Tiles, Stance::Idle);
        let holding = badges(Context::Tiles, Stance::Holding);

        let says = |rs: &[Badge], want: &str| rs.iter().any(|r| r.does.contains(want));

        assert!(says(&idle, "library"), "idle rows: {idle:?}");
        assert!(
            !says(&idle, "move the piece"),
            "the idle list must not offer a verb that cannot fire: {idle:?}"
        );

        assert!(
            says(&holding, "move the piece"),
            "holding rows: {holding:?}"
        );
        assert!(says(&holding, "flush"), "holding rows: {holding:?}");
        assert!(
            !says(&holding, "library"),
            "the holding list must not offer a verb that cannot fire: {holding:?}"
        );

        // **The whole arrow cluster moves the piece, and the member walk is elsewhere.**
        //
        // This asserted the opposite until 2026-08-13: up/down moved and left/right walked the
        // members. The author's report was that the arrows were not intuitive, and they were right in
        // a way the split made invisible — `step_in_view` maps a wish through the camera yaw, so on
        // an isometric view the arrows point at the four diagonals the screen shows, and offering two
        // of them while the other two did something unrelated is exactly what "not intuitive" means.
        //
        // The walk still has to exist: `Build::focus` is drawn and acted on by five verbs, and was
        // unreachable before it (reported 2026-08-12: *"how do I switch between two meshes to edit
        // its placement?"*). It is `,` and `.` now. Both halves are checked together, because an edit
        // that quietly took the arrows back would either strand the focus again or halve the
        // directions again, and the pair is what says which.
        let moving = holding
            .iter()
            .find(|r| r.does == "move the piece")
            .unwrap_or_else(|| panic!("no row for moving the piece: {holding:?}"));
        assert_eq!(
            moving.chord.split(", ").count(),
            4,
            "all four arrows move the piece, and it reads `{}`",
            moving.chord
        );
        let walking = holding
            .iter()
            .find(|r| r.does.contains("member"))
            .unwrap_or_else(|| panic!("no row walks the members: {holding:?}"));
        assert_eq!(
            walking.chord.split(", ").count(),
            2,
            "a prev/next pair walks the members, and it reads `{}`",
            walking.chord
        );
        assert!(
            !walking.chord.contains("left") && !walking.chord.contains("right"),
            "the walk must not take an arrow back: `{}`",
            walking.chord
        );
    }

    /// The legal collisions, asserted directly so nobody "fixes" them: `S` pans the map and the
    /// modified `S` saves; `Z` marks a lattice cell solid on the Meshes tab and the modified `Z`
    /// undoes. Different chords rather than clashes — and the second pair is what makes `Z` safe to
    /// bind at all. (It read `AimLeft` until the aim keys retired into the turn cluster, 2026-08-14;
    /// the rule it guards is the same.)
    #[test]
    fn a_bare_key_and_its_modified_chord_are_different_bindings() {
        for (bare, modified) in [
            (Action::PanBack, Action::Save),
            (Action::CellSolid, Action::Undo),
        ] {
            let bare = binding(bare);
            let modified = binding(modified);
            assert_eq!(bare.key, modified.key);
            assert!(!bare.needs_mod && modified.needs_mod);
        }
    }


    /// **The composition verb names its own key**, because it was reported missing twice while on
    /// screen.
    ///
    /// `Context::Map` is at the row ceiling the test below enforces, so `B`, `Shift+B` and `M` share
    /// one `does` and collapse into a single line. That line read `move / clone a set / keep as a
    /// composition` — three chords and three phrases paired only by position, one of which (`M`) has
    /// no relationship to the other two. An author with the overlay open asked twice how to keep a
    /// selection as a composition.
    ///
    /// **Deliberately about this row rather than a general rule**, and that is the finding. Two
    /// attempts at a lint over every collapsed row both flagged prose that is fine: `Cmd+Z,
    /// Shift+Cmd+Z / undo / redo` and `[, ] / lift / lower` pair by a convention older than this
    /// editor, and `Z, X, C, V / solid / edge / anchor / clear` is a contiguous run read as a keypad,
    /// like `W, A, S, D`. A lint that forces `Shift:` into those makes the panel worse to satisfy
    /// itself. What was actually wrong here is one unrelated key hiding in a family row.
    #[test]
    fn the_composition_verb_names_its_key_in_the_map_census() {
        let row = badges(Context::Map, Stance::Idle)
            .into_iter()
            .find(|r| r.does.contains("composition"))
            .unwrap_or_else(|| panic!("the Map census no longer offers a composition verb at all"));
        let chord = chord_text(binding(Action::GroupFromSet));
        assert!(
            row.chord.split(", ").any(|c| c == chord),
            "the row's chord column must list `{chord}`, and it reads `{}`",
            row.chord
        );
        assert!(
            row.does.contains(&format!("{chord}:")),
            "`{chord}` is one of {} chords on this row, so the phrase has to name it — it reads `{}`",
            row.chord.split(", ").count(),
            row.does
        );
    }

    /// **Every context, so a tab cannot escape the ceiling by not being listed.**
    ///
    /// `Context::Tiles` was absent, exactly as `Context::Compose` had been before it — and the note
    /// below records that lesson being learned once already. A tab missing from the test that polices
    /// the tabs is a tab the policing does not reach, and the Tiles tab is the one with an author's
    /// "those keys are gross" against it.
    ///
    /// **Both stances, because both are things a reader looks at.** A list that fits only while
    /// nothing is in hand is not a list that fits.
    pub const POLICED: [Context; 6] = [
        Context::Global,
        Context::Map,
        Context::Meshes,
        Context::Tiles,
        Context::Anim,
        // **Compose was missing from both of these lists**, so the fourth tab's rows were
        // neither counted against the ceiling nor checked for surviving the collapse. A tab
        // absent from the test that polices the tabs is a tab the policing does not reach.
        Context::Compose,
    ];

    /// **Retired, and this is the record of why.**
    ///
    /// It capped a context at ~12 *rows* — Zheng et al. 2018 on a vocabulary of about a dozen being
    /// learnable in three ten-minute sessions — because a centred table of every chord was what an
    /// author read. That table is gone. `rows()` renders nowhere; `badges()` is what is drawn, and it
    /// is drawn **on the controls**, where ExposeHK (`10.1145/2470654.2470735`) measured 72 at once
    /// with the lowest workload of its conditions. Capping the drawn count is the wrong axis.
    ///
    /// What the ceiling actually protected is still guarded, in the two places it is now real:
    /// [`no_home_carries_more_than_it_can_show`] caps what has to be read **at one anchor** and pins
    /// the legend — the one place that *is* still a list — and [`no_context_gains_a_key`] ratchets the
    /// vocabulary itself, which is what a hand learns.
    ///
    /// It is left here as a comment rather than deleted because the number came from a paper, and a
    /// ratchet that vanishes without a reason gets reinvented at the wrong value.
    #[test]
    fn the_row_ceiling_has_no_rendering_left_to_guard() {
        assert!(
            crate::badges::RENDERS_NO_ROWS,
            "if something renders `keys::rows` again, this ceiling has to come back with it"
        );
    }

    /// **A collapsed row must name what each of its chords does.**
    ///
    /// # The row cap was measuring the wrong thing, and this is the other half of it
    ///
    /// `rows()` collapses adjacent bindings that share a `does`, and the test above counts the
    /// result. So the cheapest way to stay under the ceiling was never to remove a key — it was to
    /// give more keys the *same, vaguer* description, and that is what happened. `T F G H [ ]` read
    /// `"cell cursor / layer"`: six keys, two phrases, and nothing pairing them. `G / Shift+G /
    /// Cmd+G` read `"continue the layout: from the map, the kit's tokens, or its compositions"` —
    /// three chords, three sources, no mapping. Both rows were *true*. Neither could teach the tab.
    ///
    /// Goodhart, in a key table: the guard was satisfied while the thing it guarded got worse.
    ///
    /// # Why this rule and not the two that were tried
    ///
    /// `the_composition_verb_names_its_key_in_the_map_census` records two earlier attempts at a
    /// general lint, both abandoned because they flagged prose that is fine. This one does not flag
    /// any of the three cases named there — `Cmd+Z, Shift+Cmd+Z / undo / redo`, `[, ] / lift / lower`
    /// and the `Z, X, C, V` keypad run all have as many phrases as chords already.
    ///
    /// The rule is only about **slash-separated** descriptions, because a slash is what this table
    /// uses to mean "and these are the parts". A single phrase covering several chords is a family
    /// row — `W, A, S, D / pan` — and is left alone.
    ///
    /// # Two things it deliberately does not catch, so nobody reads it as more than it is
    ///
    /// **The annotation form is exempt.** `"walk the lists / Shift: x5"` is four arrows and one idea
    /// with a note about a modifier, not four ideas; `"move / Shift: clone / M: keep as a
    /// composition"` names its chords outright. Both put a `chord:` prefix on the trailing phrase,
    /// and that prefix *is* the pairing — so a phrase after the first containing a colon exempts the
    /// row. Without this the rule flags exactly the prose the two abandoned attempts flagged.
    ///
    /// **A single phrase over many chords is not checked at all**, and one such row was genuinely
    /// bad: `G / Shift+G / Cmd+G` reading *"continue the layout: from the map, the kit's tokens, or
    /// its compositions"* is one phrase, so it would pass. It was fixed by hand rather than by rule,
    /// because no mechanical test can tell `W, A, S, D / pan` from that — the difference is whether
    /// the chords do the same thing, which only a reader knows. This catches the arithmetic case;
    /// the judgement case stays a judgement.
    #[test]
    fn a_collapsed_row_names_each_of_its_chords() {
        let mut vague = Vec::new();
        for (context, stance) in POLICED
            .into_iter()
            .flat_map(|c| [(c, Stance::Idle), (c, Stance::Holding)])
        {
            for row in badges(context, stance) {
                let chords = row.chord.split(", ").count();
                let phrases: Vec<&str> = row.does.split(" / ").collect();
                // The annotation form names its own chord; see the note above.
                if phrases.iter().skip(1).any(|p| p.contains(':')) {
                    continue;
                }
                let phrases = phrases.len();
                if chords > 1 && phrases > 1 && chords != phrases {
                    vague.push(format!(
                        "{context:?}/{stance:?}: `{}` has {chords} chords but {phrases} phrases — \
                         `{}`",
                        row.chord, row.does
                    ));
                }
            }
        }
        assert!(
            vague.is_empty(),
            "{} row(s) collapse more chords than they explain. Either name each chord, or give the \
             row one phrase that honestly covers all of them:\n  {}",
            vague.len(),
            vague.join("\n  ")
        );
    }

    /// **Every stance a context can be in.** The row tests police two because those are the two a
    /// *reader* compares; the badge tests police four, because a badge is not read — it is looked at,
    /// and an author holding the shortcut key mid-generate is looking at `Proposed`.
    const STANCES: [Stance; 4] = [
        Stance::Idle,
        Stance::Holding,
        Stance::Proposed,
        Stance::Browsing,
    ];

    /// **Nothing falls off the badge overlay.**
    ///
    /// The arithmetic half of the guarantee [`Draft::at`] makes structurally: the compiler says every
    /// binding named a home, and this says [`badges`] then hands every one of them to something that
    /// will draw it — exactly once, never twice. Without it a slip in the split would drop a verb
    /// silently, which is the failure the whole overlay exists to end.
    #[test]
    fn every_live_binding_gets_exactly_one_badge() {
        for (context, stance) in POLICED.into_iter().flat_map(|c| STANCES.map(|s| (c, s))) {
            let mut drawn: Vec<Action> = badges(context, stance)
                .into_iter()
                .flat_map(|b| b.actions)
                .collect();
            let mut live: Vec<Action> = in_context(context, stance).map(|b| b.action).collect();
            // Compared as ordered lists, not as sets: `badges` promises declaration order, and a
            // reordering would move every badge on screen at once.
            assert_eq!(
                format!("{drawn:?}"),
                format!("{live:?}"),
                "{context:?}/{stance:?}: the badges do not cover exactly what is live"
            );
            drawn.sort_by_key(|a| format!("{a:?}"));
            live.sort_by_key(|a| format!("{a:?}"));
            drawn.dedup();
            assert_eq!(drawn.len(), live.len(), "{context:?}/{stance:?}: a badge repeats an action");
        }
    }

    /// **A badge is one place**, so the thing that draws it never has to choose between two.
    ///
    /// Guards the one extra condition [`badges`] adds to [`rows`]'s collapse. If it were dropped, a
    /// row whose bindings disagreed would silently take the first one's home and the rest would be
    /// drawn somewhere they do not act.
    #[test]
    fn a_badge_never_joins_two_homes() {
        for (context, stance) in POLICED.into_iter().flat_map(|c| STANCES.map(|s| (c, s))) {
            for badge in badges(context, stance) {
                for action in &badge.actions {
                    assert_eq!(
                        binding(*action).home,
                        badge.home,
                        "{context:?}/{stance:?}: `{}` puts {action:?} at {:?}, but its binding says \
                         {:?}",
                        badge.chord,
                        badge.home,
                        binding(*action).home
                    );
                }
            }
        }
    }

    /// **A cluster is a label, not a table** — pinned, going down only.
    ///
    /// Badges sharing a home stack at that home, and past a handful the stack stops being something
    /// you glance at and becomes the centred list this overlay replaced. The number is **measured, not
    /// chosen** — the same honesty [`no_context_gains_a_key`] states: there is no citable threshold
    /// for badges-per-anchor, and inventing one would be the mistake of measuring rows and calling it
    /// learnability. Lowering it is free; raising it belongs in a commit message.
    ///
    /// The count that *is* sourced is the total on screen, and it is not this one: ExposeHK
    /// (`10.1145/2470654.2470735`) posted **72 badges at once** and measured 99% hotkey use with the
    /// lowest workload of its conditions, because a reader locates the control they already half-know
    /// and reads the one badge on it rather than searching all of them. This caps what has to be read
    /// *in one place*, which is the number that argument does not cover.
    ///
    /// **Fifteen was what the Meshes tab owed**, and it is a number to bring *down*: every row in it
    /// is a verb with no control to sit on, so the way to shrink the legend is to give those verbs
    /// somewhere to be — not to hide them.
    ///
    /// **Sixteen since 2026-08-19, and here is the commit message this doc asks for.** `Cmd+E` opens
    /// the session journal (`chrome::Journal`), a panel that is not on screen until it is pressed —
    /// which is precisely the case `Home::Legend` exists for, and the one home it cannot be given
    /// instead is a control that does not exist yet. It arrived in the same change that deleted four
    /// per-panel problem logs, so the *screen* got quieter by more than this row costs.
    ///
    /// **Eight since 2026-08-21, and the commit message again.** The legend was drained onto
    /// readouts: the Map's piece-verbs moved to the rows that display what they change (`YAW`,
    /// `UNDER`), the tile-verbs to the TILE card and the MEMBERS list, take/drop to the piece
    /// list, save to the document's name in the chrome bar, and every tab's undo pair to its own
    /// text pane. What remains is the set with no honest control: `Esc`, the journal, `Cmd+Delete`,
    /// the camera (whose anchor — the compass — stands down while `K` is held), the tool-armers
    /// whose subject is a future click, the generate row, and the grid rungs whose readout does
    /// not exist yet. Map's worst stance is the new eight.
    #[test]
    fn no_home_carries_more_than_it_can_show() {
        const CAP: usize = 8;
        const LEGEND: usize = 8;
        let mut over = Vec::new();
        for (context, stance) in POLICED.into_iter().flat_map(|c| STANCES.map(|s| (c, s))) {
            // The frame's badges are on screen at the same time as the tab's, so they are counted
            // together — a home is a place on the window, not a place in a context.
            let mut live = badges(context, stance);
            if context != Context::Global {
                live.extend(badges(Context::Global, stance));
            }
            for home in ControlId::ALL.map(Home::Control) {
                let n = live.iter().filter(|b| b.home == home).count();
                if n > CAP {
                    over.push(format!("{context:?}/{stance:?} {home:?}: {n}"));
                }
            }
            // **The legend is exempt from the cap and pinned instead**, because it is the one place
            // that is *supposed* to be a list: a verb with no control and no subject has nowhere
            // better to be, and capping it would only push one back onto something it does not act
            // on. Ratchet rather than ceiling, the shape `no_context_gains_a_key` uses — measured,
            // lowerable for free, raising it a decision that belongs in a commit message.
            let legend = live.iter().filter(|b| b.home == Home::Legend).count();
            if legend > LEGEND {
                over.push(format!(
                    "{context:?}/{stance:?} legend: {legend} (pinned at {LEGEND})"
                ));
            }
        }
        assert!(
            over.is_empty(),
            "these anchors carry more badges than a glance can take ({CAP}). Move a verb, or admit \
             the anchor is two anchors:\n  {}",
            over.join("\n  ")
        );
    }

    /// **A `ControlId` nothing homes to is a word with no meaning.**
    ///
    /// The other half is `tests/every_key_has_a_home.rs`, which checks that every id is *attached* to
    /// a node by some panel. Between them: the census cannot name a control nobody marks, and no
    /// panel marks a control the census never asks for.
    #[test]
    fn every_control_id_is_named_by_a_binding() {
        for id in ControlId::ALL {
            assert!(
                BINDINGS.iter().any(|b| b.home == Home::Control(id)),
                "{id:?} is in the census's vocabulary and no binding lives there"
            );
        }
    }

    /// **The live key vocabulary can shrink and cannot grow.**
    ///
    /// The ceiling above counts *rows*, which is what a reader sees — and that is the right thing to
    /// measure for a cheat sheet. It is the wrong thing to measure for a **hand**: the fingers learn
    /// keys, not rows, and four arrows on one row are still four keys.
    ///
    /// The gap between those numbers is where this editor grew. At the time this ratchet was written
    /// the Meshes tab showed 10 rows and bound 30 keys, and its author had to hold `K` to use it.
    ///
    /// So this pins the count per context, going down only. It is deliberately **not** a cap with a
    /// principled number attached — there isn't one to cite, and inventing one would be the same
    /// mistake as measuring rows and calling it learnability. It is a ratchet: adding a key to a tab
    /// costs a deliberate edit here, and removing one is free.
    #[test]
    fn no_context_gains_a_key() {
        // (context, keys) — measured, not chosen. Lower these when a key goes; raising one is a
        // decision, and it belongs in a commit message.
        let pinned = [
            // 17 -> 18: `MainMenu`. `Cmd+O` leaves this map for the chooser, and it is Global
            // because "I am on the wrong map" is true on every tab — the state an author was in
            // when they opened the wrong kit three times in one afternoon, with no way back but
            // quitting the process. Costs a row it shares with `Save`.
            // 18 -> 16: measured 2026-08-21, not derived. The assertion is one-directional, so this
            // pin sat stale-high while the six per-panel tab keys (`MapTab` … `NextTab`) collapsed
            // into the three door-relative slots (`1`/`2`/`3`, "the kit stops being the project")
            // and `ShowErrors` arrived. Two keys of headroom nobody granted, closed on the day it
            // was noticed.
            (Context::Global, 16),
            // 25 -> 26: `AcceptProposal`, the generate commit door. Bought deliberately, and it
            // costs no *row* — the four region-fills are `Stance::Idle` and this is
            // `Stance::Proposed`, so the two never share a list. See [`Stance::Proposed`].
            // 26 -> 24: measured 2026-08-21. The 26 was an increment stated on a count that was
            // never re-measured — with `AcceptProposal` in, the table holds 24 `Context::Map` rows.
            // A one-directional assert cannot catch a stale-high pin, so this one is corrected by
            // counting rather than by arithmetic on its predecessor.
            (Context::Map, 24),
            // 31 -> 32: `ExcludePack`. The importer scans every `.glb` under `assets/`, which is
            // right for finding art and wrong for offering it — a labelling batch spent its tenth
            // call of 778 describing `characters/cipher_field`, a character rig that could not be
            // a tile under any circumstances. `Shift+R` says a folder is not what this kit is
            // built from; `Policy::exclude` remembers it.
            //
            // **Costs no row**: it shares `Rescan`'s, which is the same idea one step wider —
            // what this list offers. That sharing is what keeps this tab inside its twelve.
            //
            // 30 -> 31: `AcceptEdges`, the door on the geometric socket derivation (FVS-R-26).
            // Costs no row — `Accept` is `Stance::Idle` and this is `Stance::Proposed`, so the
            // two never share a list.
            //
            // 32 -> 33: `FoldPack`. Space on a collapsed pack heading did nothing on this tab —
            // it is bound in Tiles and Anim and was never bound here — so a list walked with the
            // arrows had to be opened with `Enter`, which is the reach this tab keeps trying to
            // remove. **No new key to learn**: Space already means "the obvious thing to this row"
            // on two other tabs, and it is inert off a heading by construction rather than by
            // prose, since making it a second key on `Accept` would have let Space commit a tile.
            // 32 -> 31: `DemoteTile` retired on 2026-08-20 into `RemoveTile`, which now does the
            // whole trip — out of the library, rescanned, and the cursor left on the reborn row.
            // 34 -> 32: `ApplySuggestion` and `DiscardSuggestion` retired on 2026-08-20 — a label
            // no longer waits to be confirmed, so `U` and `Y` named a state that cannot occur. The
            // number goes DOWN here, which the assertion below does not police (it catches growth);
            // stated anyway, because a count nobody updated is a count nobody can trust.
            // 33 -> 34: `FocusTagFilter`. The tag block draws the project's whole vocabulary — 55
            // chips on the shipped kit — and every one of them was a mouse target and nothing else:
            // `tiles::on_tag_chip` was their only writer, on the tab whose argument is that
            // keystrokes are faster. `/` is a **new key to learn**, which the two rows above this one
            // were each able to avoid; it is charged knowingly, because the alternative was leaving
            // the largest control in the pane keyboard-unreachable. `docs/ui.md` §4.2.
            (Context::Meshes, 31),
            // 21 -> 22: `ClearTile`. `MemberPrev`/`MemberNext` replace the X nudge rather than
            // adding to it, so the walk costs nothing here.
            // 22 -> 26: the KIT list (FVS: the tab could author tiles and never show them).
            // `right` opens it, `up`/`down` walk it, `right` again reopens the selected tile -- four
            // 26 -> 28: `,` and `.` for the member walk, freeing `left`/`right` so that all four
            // arrows move the piece. The author's report was that two of four directions moved it
            // and the other two did something unrelated, which on an isometric view is exactly what
            // it looks like.
            //
            // bindings, and **no new key to learn**: every one is an arrow this tab already used,
            // kept out of each other's lists by the stance. `Esc` backs out, which the tab already
            // promised ("Esc always returns to Choosing").
            // 29 -> 30: `FocusFilter`. The filter box was mouse-only; see its binding.
            // 28 -> 29: `KitLeave`. `left` at `Stance::Browsing` was unbound while the KIT strip
            // told authors it went back — the census cannot see prose, so the lie survived until
            // somebody pressed it. Costs no row: it shares `KitOpen`'s.
            (Context::Tiles, 30),
            (Context::Anim, 11),
            (Context::Compose, 7),
        ];
        let mut grown = Vec::new();
        for (context, was) in pinned {
            let now = BINDINGS.iter().filter(|b| b.context == context).count();
            if now > was {
                grown.push(format!("{context:?}: {was} -> {now}"));
            }
        }
        assert!(
            grown.is_empty(),
            "a context gained keys. The row ceiling will not catch this — a row can hold six keys \
             — so state the new number here on purpose:\n  {}",
            grown.join("\n  ")
        );
    }

    /// **A rendered chord carries its modifier**, or a message names a key that does something else.
    ///
    /// `chord` returned the bare field, so `Cmd+2` printed as "2" — and the refusal that told an
    /// author to press it read as nonsense. Asserted against a binding that HAS a modifier, because
    /// that is the only case the old version got wrong.
    #[test]
    fn a_rendered_chord_includes_the_modifier() {
        let send = binding(Action::EditTile);
        assert!(send.needs_mod, "this test is about a modified binding");
        let text = chord(Action::EditTile);
        assert!(
            text.contains(MOD_NAME),
            "a chord an author is told to press has to name the modifier: got `{text}`"
        );
        assert!(
            text.ends_with(send.chord),
            "and still end in the key itself: got `{text}`"
        );
    }

    /// Four keys, one idea. The displayed list collapses them rather than repeating the word.
    ///
    /// The camera's two collapsed rows are Global, so they read the same on every tab.
    #[test]
    fn keys_that_do_one_thing_share_a_row() {
        let map = badges(Context::Map, Stance::Idle);
        let global = badges(Context::Global, Stance::Idle);
        let pan = global
            .iter()
            .find(|r| r.does == "pan")
            .unwrap_or_else(|| panic!("no pan row"));
        assert_eq!(pan.chord, "W, A, S, D");
        assert_eq!(global.iter().filter(|r| r.does == "pan").count(), 1);

        let turn = badges(Context::Global, Stance::Idle)
            .into_iter()
            .find(|r| r.does == "turn view")
            .unwrap_or_else(|| panic!("no turn row"));
        assert_eq!(turn.chord, "Q, E");

        // **The whole rotate cluster on one row.** It was two rows and two subjects — `Z, C, V`
        // aimed the brush while `R, T, Y, U` turned what was under the cursor — until the split was
        // reported from the keyboard (2026-08-14). One row now, because it is one idea: turn the
        // thing you are steering. Five chords, five phrases, which
        // `a_collapsed_row_names_each_of_its_chords` is what enforces.
        let turn_piece = map
            .iter()
            .find(|r| r.does == "turn L / turn R / tip x / tip z / straight")
            .unwrap_or_else(|| panic!("no turn row"));
        assert_eq!(turn_piece.chord, "R, T, Y, U, V");
        assert!(
            !map.iter().any(|r| r.does.starts_with("aim ")),
            "the aim row retired into the turn cluster; two rows would be two subjects again"
        );

        // **The lattice cursor is six chords and six things, and it says so.**
        //
        // They were one collapsed row — `T, F, G, H, [, ]  cell fwd / left / back / right / layer
        // down / up` — six chords paired to six phrases by counting, which is exactly what got
        // reported: *"what does t f g h open bracket, closed bracket, z x v even do."* They were
        // collapsed to fit a twelve-row ceiling on a centred table, and that table is gone; nothing
        // renders `rows` any more. One chord, one thing.
        for (action, does) in [
            (Action::CellForward, "cell forward"),
            (Action::CellLeft, "cell left"),
            (Action::CellBack, "cell back"),
            (Action::CellRight, "cell right"),
            (Action::LayerDown, "previous layer"),
            (Action::LayerUp, "next layer"),
        ] {
            let one = badges(Context::Meshes, Stance::Idle)
                .into_iter()
                .find(|r| r.actions == vec![action])
                .unwrap_or_else(|| panic!("{action:?} shares a badge with something else"));
            assert_eq!(one.does, does);
        }

        // And the three cell verbs are three chips, so each is its own badge on the button its key
        // replaces clicking.
        for (action, does) in [
            (Action::CellSolid, "solid this cell"),
            (Action::CellEdge, "edge this cell"),
            (Action::CellClear, "clear this cell"),
        ] {
            let one = badges(Context::Meshes, Stance::Idle)
                .into_iter()
                .find(|r| r.actions == vec![action])
                .unwrap_or_else(|| panic!("{action:?} shares a badge with something else"));
            assert_eq!(one.does, does);
        }

        // **The labeler's three verbs read as one row**, the shifted forms rendered as such. It was
        // five until 2026-08-20, when `U` and `Y` retired: a label applies on arrival, so "apply
        // this one" and "discard this one" named a state that no longer occurs. What is left is ask
        // about this piece, ask about everything, and abandon a walk in progress.
        let labels = badges(Context::Meshes, Stance::Idle)
            .into_iter()
            .find(|r| r.does == "suggest / all / abandon")
            .unwrap_or_else(|| panic!("no labels row"));
        assert_eq!(labels.chord, "L, Shift+L, Shift+Y");
    }

    /// **The overlay key is held, not tapped.** `pressed` must answer for it while it is down —
    /// `just_pressed` is true for one frame, which would make the list flicker rather than show.
    #[test]
    fn the_shortcuts_key_reads_as_held() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(binding(Action::Shortcuts).key);
        for tab in [Context::Map, Context::Meshes, Context::Anim] {
            assert!(
                pressed(&input, Live(tab, Stance::Idle), Action::Shortcuts),
                "the shortcuts overlay must be reachable from {tab:?}"
            );
        }
        assert!(
            !pressed(
                &input,
                Live(Context::Typing, Stance::Idle),
                Action::Shortcuts
            ),
            "and must not open while a field is taking keys"
        );
    }

    /// Collapsing must not lose a binding — every one still appears in exactly one row.
    #[test]
    fn collapsing_rows_loses_nothing() {
        for context in [
            Context::Global,
            Context::Map,
            Context::Meshes,
            Context::Anim,
            // **Compose was missing from both of these lists**, so the fourth tab's rows were
            // neither counted against the ceiling nor checked for surviving the collapse. A tab
            // absent from the test that polices the tabs is a tab the policing does not reach.
            Context::Compose,
        ] {
            let chords: String = badges(context, Stance::Idle)
                .iter()
                .map(|r| r.chord.clone())
                .collect::<Vec<_>>()
                .join(" ");
            for b in in_context(context, Stance::Idle) {
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
    ///
    /// It used to press `Tab` and the digits, because those were the tab keys. There are no tab keys
    /// — a door shows one thing and is chosen on the way in — so it presses two surviving `Global`
    /// bindings instead. What it is checking has not changed: that a pressed key reaches its own
    /// action and no other.
    #[test]
    fn pressing_a_bound_key_fires_its_action() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::KeyK);
        assert!(
            just_pressed(&input, Live(Context::Map, Stance::Idle), Action::Shortcuts),
            "K did not fire Shortcuts"
        );
        assert!(
            !just_pressed(&input, Live(Context::Map, Stance::Idle), Action::Cancel),
            "K fired an unrelated action"
        );

        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::Escape);
        assert!(
            just_pressed(&input, Live(Context::Map, Stance::Idle), Action::Cancel),
            "Escape did not fire Cancel"
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
                !just_pressed(&input, Live(Context::Typing, Stance::Idle), b.action),
                "{:?} fired while a text field owned the keyboard",
                b.action
            );
            assert!(
                !pressed(&input, Live(Context::Typing, Stance::Idle), b.action),
                "{:?} read as held while a text field owned the keyboard",
                b.action
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
        assert!(just_pressed(
            &input,
            Live(Context::Map, Stance::Idle),
            Action::PanBack
        ));
        assert!(
            !just_pressed(&input, Live(Context::Map, Stance::Idle), Action::Save),
            "bare S must not save"
        );

        // A FRESH input, not `clear()`: `clear` keeps the pressed state, so pressing an
        // already-held key never re-registers as just-pressed and the assertion below would fail for
        // a reason that has nothing to do with the chord.
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(MOD_KEYS[0]);
        input.press(KeyCode::KeyS);
        assert!(
            just_pressed(&input, Live(Context::Map, Stance::Idle), Action::Save),
            "{MOD_NAME}+S must save"
        );
        assert!(
            !just_pressed(&input, Live(Context::Map, Stance::Idle), Action::PanBack),
            "{MOD_NAME}+S must not also pan"
        );
    }

    /// **The chord that made undo look broken.** `Cmd+Z` on a Mac had been checked against Ctrl, so
    /// it did nothing at all — a flood fill could not be taken back and the undo stack looked empty.
    ///
    /// Both halves of the `Z` pair are still asserted, on the Meshes tab, which is where the bare
    /// letter now lives: `Z` marks a lattice cell solid and `Cmd+Z` steps that tab's history. On the
    /// Map the bare letter is free — it aimed the brush until the aim keys retired into the turn
    /// cluster (2026-08-14) — and a modifier check that is wrong there is exactly as silent as it
    /// ever was, which is why the Map half is asserted too.
    #[test]
    fn the_platform_modifier_undoes_and_the_bare_key_does_not() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(MOD_KEYS[0]);
        input.press(KeyCode::KeyZ);
        assert!(
            just_pressed(&input, Live(Context::Map, Stance::Idle), Action::Undo),
            "{MOD_NAME}+Z must undo"
        );
        assert!(
            !just_pressed(
                &input,
                Live(Context::Meshes, Stance::Idle),
                Action::CellSolid
            ),
            "{MOD_NAME}+Z must not also mark a cell solid"
        );

        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::KeyZ);
        assert!(
            just_pressed(
                &input,
                Live(Context::Meshes, Stance::Idle),
                Action::CellSolid
            ),
            "bare Z must mark the lattice cell solid"
        );
        assert!(
            !just_pressed(&input, Live(Context::Map, Stance::Idle), Action::Undo),
            "bare Z must not undo"
        );
    }

    /// **Three sources on one key, and none of them shadows another.**
    ///
    /// `G` is bound three times — bare, Shift, and the platform modifier — and the dispatcher runs all
    /// three checks in a row with no `return` between them. That is only safe because `just_pressed`
    /// requires the modifier state to match *exactly*, so this is the assertion the dispatcher's lack
    /// of ordering rests on. Written when the third arm landed; the pairwise tests above cover other
    /// keys but say nothing about a triple.
    #[test]
    fn the_three_generate_sources_do_not_shadow_each_other() {
        let cases = [
            (vec![], Action::Generate, "bare G"),
            (vec![SHIFT_KEYS[0]], Action::GenerateDeclared, "Shift+G"),
            (vec![MOD_KEYS[0]], Action::GenerateComposed, "modified G"),
        ];
        for (mods, wanted, name) in cases {
            // A fresh input each time: `clear()` keeps the pressed state, so a key already held never
            // registers as just-pressed again.
            let mut input = ButtonInput::<KeyCode>::default();
            for m in &mods {
                input.press(*m);
            }
            input.press(KeyCode::KeyG);
            for other in [
                Action::Generate,
                Action::GenerateDeclared,
                Action::GenerateComposed,
            ] {
                let fired = just_pressed(&input, Live(Context::Map, Stance::Idle), other);
                assert_eq!(
                    fired,
                    other == wanted,
                    "{name} must fire {wanted:?} and nothing else, but {other:?} answered {fired}"
                );
            }
        }
    }

    /// **The four region-fills share one row, and the row pairs each chord with its source.**
    ///
    /// The collapse is what keeps the Map context inside the ceiling
    /// `no_context_carries_more_than_a_learnable_vocabulary` enforces — they collapse only while they
    /// are adjacent and carry the same `does`, so this pins both.
    ///
    /// **The second assertion is the one that was missing.** The row used to read *"continue the
    /// layout: from the map, the kit's tokens, or its compositions"* against three chords, and this
    /// test was satisfied: three chords, one row, all present. What it could not see is that a reader
    /// had no way to tell which chord took which source — the count was right and the row taught
    /// nothing. Counting the slash-separated phrases against the chords is what makes that a failure.
    #[test]
    fn the_region_fills_collapse_into_one_row_that_names_each_source() {
        let rows = badges(Context::Map, Stance::Idle);
        let fills: Vec<&Badge> = rows
            .iter()
            .filter(|r| r.chord.split(", ").any(|c| c.ends_with('G')))
            .collect();
        assert_eq!(fills.len(), 1, "four bindings, one row: {fills:?}");
        let chords: Vec<&str> = fills[0].chord.split(", ").collect();
        assert_eq!(chords.len(), 4, "all four chords are on it: {:?}", fills[0]);
        assert!(chords.iter().any(|c| *c == "F"), "{chords:?}");
        assert!(chords.iter().any(|c| *c == "G"), "{chords:?}");
        assert!(chords.iter().any(|c| c.starts_with("Shift")), "{chords:?}");
        assert!(chords.iter().any(|c| c.starts_with(MOD_NAME)), "{chords:?}");

        // One phrase per chord, so the pairing is readable off the row.
        let phrases = fills[0].does.split(" / ").count();
        assert_eq!(
            phrases,
            chords.len(),
            "a collapsed row must name what each of its chords does — `{}` has {} chords and {} \
             phrases",
            fills[0].does,
            chords.len(),
            phrases
        );
    }

    /// Every binding can be rendered next to the thing it does, which is the whole point of carrying
    /// the label in the census — see the module note on Cockburn et al. 2014.
    #[test]
    fn every_binding_states_itself() {
        for b in BINDINGS {
            assert!(!b.chord.is_empty(), "{:?} has no chord label", b.action);
            assert!(
                !b.does.is_empty(),
                "{:?} does not say what it does",
                b.action
            );
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
        input.press(binding(Action::TurnPieceRight).key);

        assert!(
            repeating(
                &input,
                Live(Context::Map, Stance::Idle),
                Action::TurnPieceRight,
                &mut repeat,
                0.0
            ),
            "the press itself must fire without waiting on a timer"
        );

        // Held, but `just_pressed` no longer reports it — this is what a second frame looks like.
        input.clear();
        input.press(binding(Action::TurnPieceRight).key);
        input.clear_just_pressed(binding(Action::TurnPieceRight).key);
        // Just under the interval, in ten equal steps: nothing more may fire.
        let step = REPEAT_SECS / 10.0;
        let mut fired = 0;
        for _ in 0..9 {
            if repeating(
                &input,
                Live(Context::Map, Stance::Idle),
                Action::TurnPieceRight,
                &mut repeat,
                step,
            ) {
                fired += 1;
            }
        }
        assert_eq!(
            fired, 0,
            "9/10 of the {REPEAT_SECS} s interval must not fire"
        );

        // Crossing it fires exactly once.
        assert!(repeating(
            &input,
            Live(Context::Map, Stance::Idle),
            Action::TurnPieceRight,
            &mut repeat,
            step * 2.0
        ));
        assert!(!repeating(
            &input,
            Live(Context::Map, Stance::Idle),
            Action::TurnPieceRight,
            &mut repeat,
            0.0
        ));
    }

    /// **The ramp runs from [`REPEAT_SECS`] to [`REPEAT_FAST_SECS`] and never turns back.**
    ///
    /// The behavioural test below can only ever measure the curve through sixty frames of
    /// integration; this reads it directly, so "acceleration is switched on" is pinned exactly
    /// rather than statistically.
    #[test]
    fn the_ramp_only_ever_speeds_up() {
        assert!(
            REPEAT_FAST_SECS < REPEAT_SECS,
            "the floor must be faster than the opening interval, or there is no ramp"
        );
        // Approximate, because the lerp lands a float ULP off its endpoint (0.030000001 vs 0.03)
        // and pinning that would be pinning f32 rounding rather than the curve.
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(
            near(interval_after(0.0), REPEAT_SECS),
            "a fresh hold starts at the slow end, not {}",
            interval_after(0.0)
        );
        assert!(
            near(interval_after(REPEAT_RAMP_SECS), REPEAT_FAST_SECS),
            "the ramp must actually reach the floor, not {}",
            interval_after(REPEAT_RAMP_SECS)
        );
        assert!(
            near(interval_after(REPEAT_RAMP_SECS * 10.0), REPEAT_FAST_SECS),
            "past the ramp it is flat, never faster, not {}",
            interval_after(REPEAT_RAMP_SECS * 10.0)
        );
        // Monotone, and strictly so inside the ramp.
        let mut prev = interval_after(0.0);
        for i in 1..=100 {
            let now = interval_after(REPEAT_RAMP_SECS * i as f32 / 100.0);
            assert!(now <= prev, "the interval grew at step {i}: {prev} -> {now}");
            prev = now;
        }
    }

    /// **A held key speeds up, and a released one forgets it had.**
    ///
    /// Two properties rather than a rate, because the rate is now a function of time and pinning a
    /// count would pin the tuning — the three constants are explicitly by-feel. What must not
    /// change without somebody meaning it is that holding accelerates, and that letting go resets,
    /// which is the whole reason acceleration is safe to have on keys that also nudge one step.
    #[test]
    fn holding_accelerates_and_releasing_forgets() {
        let key = binding(Action::TurnPieceLeft).key;
        let live = Live(Context::Map, Stance::Idle);
        let mut input = ButtonInput::<KeyCode>::default();
        let mut repeat = Repeat::default();

        // Count fires over one second of holding, split into two half-seconds.
        input.press(key);
        assert!(
            repeating(&input, live, Action::TurnPieceLeft, &mut repeat, 0.0),
            "the press itself always fires"
        );
        input.clear_just_pressed(key);
        let mut half = [0usize; 2];
        for frame in 0..60 {
            if repeating(&input, live, Action::TurnPieceLeft, &mut repeat, 1.0 / 60.0) {
                half[usize::from(frame >= 30)] += 1;
            }
        }
        // **Twice, not merely more.** `half[1] > half[0]` was the first version of this line and it
        // was vacuous: with acceleration switched off entirely the halves come out 4 and 5, because
        // the first interval is spent before any repeat lands. Measured with the ramp on it is 6 and
        // 14, so the doubling is what actually distinguishes the two — found by turning
        // REPEAT_FAST_SECS up to REPEAT_SECS and watching the test stay green.
        assert!(
            half[1] >= half[0] * 2,
            "holding must accelerate: {} fires in the first half-second, {} in the second — a flat \
             cadence gives roughly equal halves",
            half[0],
            half[1]
        );

        // The floor is a floor — a second half-second cannot exceed what REPEAT_FAST_SECS allows.
        let ceiling = (0.5 / REPEAT_FAST_SECS).ceil() as usize + 1;
        assert!(
            half[1] <= ceiling,
            "{} fires in half a second is past the {REPEAT_FAST_SECS}s floor (max {ceiling})",
            half[1]
        );

        // **Release, press again: back to the slow end.** Without this, a fast hold would leave the
        // next tap running at traversal speed.
        input.release(key);
        repeating(&input, live, Action::TurnPieceLeft, &mut repeat, 1.0 / 60.0);
        input.clear_just_released(key);
        input.press(key);
        assert!(repeating(&input, live, Action::TurnPieceLeft, &mut repeat, 0.0));
        input.clear_just_pressed(key);
        let mut after = 0usize;
        for _ in 0..30 {
            if repeating(&input, live, Action::TurnPieceLeft, &mut repeat, 1.0 / 60.0) {
                after += 1;
            }
        }
        assert!(
            after <= half[0] + 1,
            "a fresh press fired {after} times in half a second — it inherited the previous \
             hold's ramp instead of starting at REPEAT_SECS ({} from cold)",
            half[0]
        );
    }

    /// Releasing forgets the countdown, so the next tap is immediate rather than owing the remainder
    /// of an interval nobody is waiting through.
    #[test]
    fn releasing_resets_the_countdown() {
        let mut input = ButtonInput::<KeyCode>::default();
        let mut repeat = Repeat::default();
        let key = binding(Action::TurnPieceRight).key;

        input.press(key);
        assert!(repeating(
            &input,
            Live(Context::Map, Stance::Idle),
            Action::TurnPieceRight,
            &mut repeat,
            0.0
        ));
        input.clear_just_pressed(key);
        // Part of the way to the next repeat, then let go.
        repeating(
            &input,
            Live(Context::Map, Stance::Idle),
            Action::TurnPieceRight,
            &mut repeat,
            REPEAT_SECS * 0.8,
        );
        input.release(key);
        assert!(!repeating(
            &input,
            Live(Context::Map, Stance::Idle),
            Action::TurnPieceRight,
            &mut repeat,
            0.0
        ));

        input.clear();
        input.press(key);
        assert!(
            repeating(
                &input,
                Live(Context::Map, Stance::Idle),
                Action::TurnPieceRight,
                &mut repeat,
                0.0
            ),
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
        let key = binding(Action::TurnPieceRight).key;

        // Held down while the Tiles tab owns the keyboard: nothing accrues.
        input.press(key);
        input.clear_just_pressed(key);
        assert!(!repeating(
            &input,
            Live(Context::Meshes, Stance::Idle),
            Action::TurnPieceRight,
            &mut repeat,
            5.0
        ));

        // Now the Map tab is live and the key is still down, but was never pressed here.
        assert!(
            !repeating(
                &input,
                Live(Context::Map, Stance::Idle),
                Action::TurnPieceRight,
                &mut repeat,
                5.0
            ),
            "a key that was already down must not start repeating on a context change"
        );
    }
}
