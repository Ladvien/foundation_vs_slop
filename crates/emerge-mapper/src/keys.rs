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
            (Map, Map) | (Tiles, Tiles) | (Anim, Anim) => true,
            // The three tabs are never live together — which is what lets them reuse each other's
            // letters freely. `the_key_space_has_no_collisions` polices that they only do so here.
            (Map, Tiles) | (Tiles, Map) => false,
            (Map, Anim) | (Anim, Map) | (Tiles, Anim) | (Anim, Tiles) => false,
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
    Save,
    Undo,
    /// Hold to see this tab's key list.
    Shortcuts,
    // ── Map ──────────────────────────────────────────────────────────────────
    AimLeft,
    AimRight,
    /// Turn the placement under the cursor, as opposed to the brush.
    TurnPieceLeft,
    TurnPieceRight,
    Fill,
    Remove,
    RenameMap,
    /// Put the brush back to the rotation it was authored at.
    AimReset,
    /// Leave the removal mode without removing anything.
    Cancel,
    OwnToggle,
    Generate,
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
    /// Point the arrows at the candidate list.
    FocusCandidates,
    /// Point them at the library list.
    FocusLibrary,
    // ── Anim ─────────────────────────────────────────────────────────────────
    PrevRig,
    NextRig,
}

/// One binding: the key, when it is live, and how to say it.
pub struct Binding {
    pub action: Action,
    pub key: KeyCode,
    /// Whether the platform command modifier ([`MOD_KEYS`]) must be held. `false` is the bare key.
    pub needs_mod: bool,
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

/// What [`MOD_KEYS`] is called in writing. The panel and every status line take the name from here so
/// no string in this crate can claim a modifier the build does not read.
#[cfg(target_os = "macos")]
pub const MOD_NAME: &str = "Cmd";
#[cfg(not(target_os = "macos"))]
pub const MOD_NAME: &str = "Ctrl";

/// The delete key, per platform. A Mac keyboard's `delete` sends **Backspace**; the key winit calls
/// `Delete` is the one macOS keyboards label `fn+delete` and most do not have at all — so binding
/// `Delete` there is binding a key the author cannot press.
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
    b(Action::AnimTab, KeyCode::Digit3, false, Context::Global, "3", "animation tab"),
    // **Held, not toggled**, and read with `keys::pressed`. The list is a thing you glance at with a
    // thumb down, not a mode you enter and have to leave — and a modal you can forget you opened is a
    // modal that eats the next keystroke.
    b(Action::Shortcuts, KeyCode::KeyK, false, Context::Global, "K", "hold for shortcuts"),
    b(Action::Save, KeyCode::KeyS, true, Context::Global, "S", "save"),
    // **Undo is the MAP's.** It was `Global`, and `keys` had lost its `in_map_mode` run condition, so
    // `Cmd+Z` on the Tiles tab silently despawned a flood fill — up to ~1,400 placements — while every
    // `MapRoot` panel was `Display::None` and nothing on screen changed. An undo you cannot see the
    // effect of is not an undo. Lattice edits go straight to `library.ron` and have no undo of their
    // own, which is exactly why the key is reached for there.
    b(Action::Undo, KeyCode::KeyZ, true, Context::Map, "Z", "undo"),

    // **Z and C turn the brush; X puts it back.** They sit under the left hand already resting on
    // WASD, which the brackets never did — and `Z` is free as a bare key precisely because the
    // modifier check above keeps `Cmd+Z` (undo) and `Z` (aim) apart rather than letting the chord
    // shadow the letter.
    // One ROW, deliberately: a shared `does` collapses the pair the way `W, A, S, D` collapses, and
    // the Map context is at its twelve-row ceiling (see the vocabulary test) with the removal mode's
    // `Esc` now in it. The label carries the direction so nothing is lost by sharing the line.
    b(Action::AimLeft, KeyCode::KeyZ, false, Context::Map, "Z", "aim left / right"),
    b(Action::AimRight, KeyCode::KeyC, false, Context::Map, "C", "aim left / right"),
    b(Action::AimReset, KeyCode::KeyX, false, Context::Map, "X", "aim straight again"),
    // **A separate pair, on purpose.** `[`/`]` turn the BRUSH and must keep doing only that: binding
    // them to the selection is what made rotation feel broken before, because placing selects, so the
    // next `]` turned the piece just put down while the ghost — the only thing on screen showing a
    // facing — sat still. These turn what is under the cursor, which is the other half nobody had.
    // Distinct `does` strings so the pair does NOT collapse into one row: the collapsed chord is
    // comma-joined, and a chord that IS a comma cannot survive that — which is why this is R/T rather
    // than the `<`/`>` a rotate usually wants.
    b(Action::TurnPieceLeft, KeyCode::KeyR, false, Context::Map, "R", "turn this left"),
    b(Action::TurnPieceRight, KeyCode::KeyT, false, Context::Map, "T", "turn this right"),
    b(Action::Fill, KeyCode::KeyF, false, Context::Map, "F", "flood fill"),
    b(Action::Remove, REMOVE_KEY, false, Context::Map, REMOVE_NAME, "removal mode"),
    b(Action::Cancel, KeyCode::Escape, false, Context::Map, "Esc", "stop removing"),
    b(Action::RenameMap, KeyCode::KeyN, false, Context::Map, "N", "rename map"),
    b(Action::OwnToggle, KeyCode::KeyO, false, Context::Map, "O", "pin / unpin"),
    b(Action::Generate, KeyCode::KeyG, false, Context::Map, "G", "continue the layout"),

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

    b(Action::PrevCandidate, KeyCode::ArrowUp, false, Context::Tiles, "up", "previous"),
    b(Action::NextCandidate, KeyCode::ArrowDown, false, Context::Tiles, "down", "next"),
    // **Left and right switch which list the arrows walk.** Up/Down already meant "move in a list";
    // the tab has two of them and only one was reachable, so an author could edit a candidate's
    // lattice by keyboard but had to reach for the mouse to edit a library tile's. One row, and no
    // new idea to learn — the arrow cluster keeps meaning "move around the lists".
    b(Action::FocusCandidates, KeyCode::ArrowLeft, false, Context::Tiles, "left", "which list"),
    b(Action::FocusLibrary, KeyCode::ArrowRight, false, Context::Tiles, "right", "which list"),
    b(Action::TypeId, KeyCode::KeyI, false, Context::Tiles, "I", "type an id"),
    // **"mount", not "layer".** It cycles `Descriptor::mount` — what the piece stands on — and the
    // subgrid below has its own `layer y` picker for the lattice slice. One panel said "layer" twice
    // and meant two different things.
    b(Action::CycleMount, KeyCode::KeyM, false, Context::Tiles, "M", "mount"),
    b(Action::Accept, KeyCode::Enter, false, Context::Tiles, "Enter", "add to library"),
    b(Action::Rescan, KeyCode::KeyR, false, Context::Tiles, "R", "rescan"),
    b(Action::RemoveTile, REMOVE_KEY, false, Context::Tiles, REMOVE_NAME, "remove from library"),

    // **The lattice, by keyboard.** Three rows, which is what the twelve-row ceiling leaves once the
    // seven above are counted — so each group shares one `does` and reads its chords in order, the
    // same shape as `W, A, S, D  pan`.
    //
    // **`T F G H` is an inverted T, one column left of the usual one.** The cursor cannot have
    // `W A S D` (the camera's, on every tab), nor the arrows (they walk the two lists), nor `H J K L`
    // (`K` is the shortcuts overlay). This is the nearest remaining cluster with the right *shape* —
    // T above, F left, G below, H right — and shape is what the hand remembers.
    // `Z, X, C, V` is the run under the left hand, free here because they are Map bindings and the
    // two tabs are never live together, which is the case `Context` exists to model.
    b(Action::CellForward, KeyCode::KeyT, false, Context::Tiles, "T", "move the cell cursor"),
    b(Action::CellLeft, KeyCode::KeyF, false, Context::Tiles, "F", "move the cell cursor"),
    b(Action::CellBack, KeyCode::KeyG, false, Context::Tiles, "G", "move the cell cursor"),
    b(Action::CellRight, KeyCode::KeyH, false, Context::Tiles, "H", "move the cell cursor"),
    b(Action::LayerDown, KeyCode::BracketLeft, false, Context::Tiles, "[", "layer down / up"),
    b(Action::LayerUp, KeyCode::BracketRight, false, Context::Tiles, "]", "layer down / up"),
    b(Action::CellSolid, KeyCode::KeyZ, false, Context::Tiles, "Z", "solid / edge / anchor / clear"),
    b(Action::CellEdge, KeyCode::KeyX, false, Context::Tiles, "X", "solid / edge / anchor / clear"),
    b(Action::CellAnchor, KeyCode::KeyC, false, Context::Tiles, "C", "solid / edge / anchor / clear"),
    b(Action::CellClear, KeyCode::KeyV, false, Context::Tiles, "V", "solid / edge / anchor / clear"),

    // The arrows are the Tiles tab's too. Legal, and the reason the census models context at all:
    // the two tabs are never live together, so the same key means one thing in each.
    b(Action::PrevRig, KeyCode::ArrowUp, false, Context::Anim, "up", "previous rig"),
    b(Action::NextRig, KeyCode::ArrowDown, false, Context::Anim, "down", "next rig"),
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
        context,
        chord,
        does,
    }
}

/// The binding for an action.
pub fn binding(action: Action) -> &'static Binding {
    // Every `Action` has a row; the test below is what keeps that true, so this cannot be reached
    // with a missing one in a build that passes.
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
    if b.needs_mod {
        format!("{MOD_NAME}+{}", b.chord)
    } else {
        b.chord.to_owned()
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
                last.chord.push_str(", ");
                last.chord.push_str(b.chord);
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
fn mod_held(keys: &ButtonInput<KeyCode>) -> bool {
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
    if b.needs_mod != mod_held(keys) {
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
    if b.needs_mod != mod_held(keys) {
        return false;
    }
    keys.pressed(b.key)
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
            Action::Save, Action::Undo, Action::Shortcuts,
            Action::AimLeft, Action::AimRight, Action::AimReset, Action::Cancel,
            Action::Fill, Action::Remove, Action::RenameMap,
            Action::OwnToggle, Action::Generate,
            Action::PanForward, Action::PanBack, Action::PanLeft, Action::PanRight,
            Action::TurnViewLeft, Action::TurnViewRight,
            Action::PrevCandidate, Action::NextCandidate, Action::TypeId, Action::CycleMount,
            Action::Accept, Action::Rescan, Action::RemoveTile,
            Action::CellLeft, Action::CellRight, Action::CellForward, Action::CellBack,
            Action::LayerDown, Action::LayerUp,
            Action::CellSolid, Action::CellEdge, Action::CellAnchor, Action::CellClear,
            Action::FocusCandidates, Action::FocusLibrary,
            Action::PrevRig, Action::NextRig,
            Action::TurnPieceLeft, Action::TurnPieceRight,
        ];
        assert_eq!(
            actions.len(),
            BINDINGS.len(),
            "the action list and the binding table disagree — one of them gained a row alone"
        );
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
                if a.key == b.key && a.needs_mod == b.needs_mod && a.context.overlaps(b.context) {
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
        for context in [Context::Global, Context::Map, Context::Tiles, Context::Anim] {
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

        let aim = map
            .iter()
            .find(|r| r.does == "aim left / right")
            .unwrap_or_else(|| panic!("no aim row"));
        assert_eq!(aim.chord, "Z, C");

        // The lattice cursor is its own cluster, and must not be the camera's — an author reaches
        // for `W A S D` to move the view on every tab.
        let cursor = rows(Context::Tiles)
            .into_iter()
            .find(|r| r.does == "move the cell cursor")
            .unwrap_or_else(|| panic!("no cursor row"));
        assert_eq!(cursor.chord, "T, F, G, H");
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
        for context in [Context::Global, Context::Map, Context::Tiles, Context::Anim] {
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
}
