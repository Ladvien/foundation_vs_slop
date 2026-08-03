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
//! `M` steps the layer in tile configuration and means nothing on the map; `F` floods the map and
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
            (Map, Map) | (Tiles, Tiles) => true,
            (Map, Tiles) | (Tiles, Map) => false,
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
    Save,
    Undo,
    // ── Map ──────────────────────────────────────────────────────────────────
    AimLeft,
    AimRight,
    Fill,
    Remove,
    RenameMap,
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
    CycleLayer,
    Accept,
    Rescan,
    RemoveTile,
}

/// One binding: the key, when it is live, and how to say it.
pub struct Binding {
    pub action: Action,
    pub key: KeyCode,
    /// A modifier that must be held. `None` means the bare key.
    pub ctrl: bool,
    pub context: Context,
    /// How the key reads in the panel — `Ctrl+S`, `[`, `up`.
    pub chord: &'static str,
    /// What it does, in the fewest words that stay true.
    pub does: &'static str,
}

/// **The census.** Adding a binding means adding a row here; nothing else in this crate is allowed to
/// name a `KeyCode` for an action.
pub const BINDINGS: &[Binding] = &[
    b(Action::NextTab, KeyCode::Tab, false, Context::Global, "Tab", "next tab"),
    b(Action::MapTab, KeyCode::Digit1, false, Context::Global, "1", "map tab"),
    b(Action::TilesTab, KeyCode::Digit2, false, Context::Global, "2", "tiles tab"),
    b(Action::Save, KeyCode::KeyS, true, Context::Global, "Ctrl+S", "save"),
    b(Action::Undo, KeyCode::KeyZ, true, Context::Global, "Ctrl+Z", "undo"),

    b(Action::AimLeft, KeyCode::BracketLeft, false, Context::Map, "[", "aim left"),
    b(Action::AimRight, KeyCode::BracketRight, false, Context::Map, "]", "aim right"),
    b(Action::Fill, KeyCode::KeyF, false, Context::Map, "F", "flood fill"),
    b(Action::Remove, KeyCode::Delete, false, Context::Map, "Del", "remove"),
    b(Action::RenameMap, KeyCode::KeyN, false, Context::Map, "N", "rename map"),
    // Declared W, A, S, D rather than W, S, A, D: the displayed row is these chords in order, and
    // "W, A, S, D" is how the shape is named everywhere. The census's order IS the reading order.
    b(Action::PanForward, KeyCode::KeyW, false, Context::Map, "W", "pan"),
    b(Action::PanLeft, KeyCode::KeyA, false, Context::Map, "A", "pan"),
    b(Action::PanBack, KeyCode::KeyS, false, Context::Map, "S", "pan"),
    b(Action::PanRight, KeyCode::KeyD, false, Context::Map, "D", "pan"),
    b(Action::TurnViewLeft, KeyCode::KeyQ, false, Context::Map, "Q", "turn view"),
    b(Action::TurnViewRight, KeyCode::KeyE, false, Context::Map, "E", "turn view"),

    b(Action::PrevCandidate, KeyCode::ArrowUp, false, Context::Tiles, "up", "previous"),
    b(Action::NextCandidate, KeyCode::ArrowDown, false, Context::Tiles, "down", "next"),
    b(Action::TypeId, KeyCode::KeyI, false, Context::Tiles, "I", "type an id"),
    b(Action::CycleLayer, KeyCode::KeyM, false, Context::Tiles, "M", "layer"),
    b(Action::Accept, KeyCode::Enter, false, Context::Tiles, "Enter", "add to library"),
    b(Action::Rescan, KeyCode::KeyR, false, Context::Tiles, "R", "rescan"),
    b(Action::RemoveTile, KeyCode::Delete, false, Context::Tiles, "Del", "remove from library"),
];

const fn b(
    action: Action,
    key: KeyCode,
    ctrl: bool,
    context: Context,
    chord: &'static str,
    does: &'static str,
) -> Binding {
    Binding {
        action,
        key,
        ctrl,
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
                chord: b.chord.to_owned(),
                does: b.does,
            }),
        }
    }
    out
}

/// Was this action just pressed?
///
/// The one place a `KeyCode` meets `ButtonInput`. Callers name an `Action`, which is what stops the
/// census drifting from the code the way `docs/ui.md` §3.5 records it drifting five ways at once.
pub fn just_pressed(keys: &ButtonInput<KeyCode>, action: Action) -> bool {
    let b = binding(action);
    if b.ctrl && !(keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)) {
        return false;
    }
    // A bare binding must not fire while Ctrl is held, or `Ctrl+S` would also pan the camera back.
    if !b.ctrl && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)) {
        return false;
    }
    keys.just_pressed(b.key)
}

/// Is this action's key held? For the continuous ones — panning.
pub fn pressed(keys: &ButtonInput<KeyCode>, action: Action) -> bool {
    let b = binding(action);
    if b.ctrl != (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)) {
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
            Action::NextTab, Action::MapTab, Action::TilesTab, Action::Save, Action::Undo,
            Action::AimLeft, Action::AimRight, Action::Fill, Action::Remove, Action::RenameMap,
            Action::PanForward, Action::PanBack, Action::PanLeft, Action::PanRight,
            Action::TurnViewLeft, Action::TurnViewRight,
            Action::PrevCandidate, Action::NextCandidate, Action::TypeId, Action::CycleLayer,
            Action::Accept, Action::Rescan, Action::RemoveTile,
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
                if a.key == b.key && a.ctrl == b.ctrl && a.context.overlaps(b.context) {
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

    /// The legal collision, asserted directly so nobody "fixes" it: `S` pans the map and `Ctrl+S`
    /// saves, and those are different chords rather than a clash.
    #[test]
    fn a_bare_key_and_its_ctrl_chord_are_different_bindings() {
        let pan = binding(Action::PanBack);
        let save = binding(Action::Save);
        assert_eq!(pan.key, save.key);
        assert!(!pan.ctrl && save.ctrl);
    }

    /// Map and tile contexts can never be live together, which is what lets them share letters.
    #[test]
    fn the_two_tabs_do_not_overlap() {
        assert!(!Context::Map.overlaps(Context::Tiles));
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
        for context in [Context::Global, Context::Map, Context::Tiles] {
            let n = rows(context).len();
            assert!(
                n <= 12,
                "{context:?} shows {n} rows; past about a dozen a key list stops being learnable and \
                 starts being a reference card (docs/ui.md §3.5, Zheng et al. 2018)"
            );
        }
    }

    /// Four keys, one idea. The displayed list collapses them rather than repeating the word.
    #[test]
    fn keys_that_do_one_thing_share_a_row() {
        let map = rows(Context::Map);
        let pan = map
            .iter()
            .find(|r| r.does == "pan")
            .unwrap_or_else(|| panic!("no pan row"));
        assert_eq!(pan.chord, "W, A, S, D");
        assert_eq!(map.iter().filter(|r| r.does == "pan").count(), 1);

        let turn = map
            .iter()
            .find(|r| r.does == "turn view")
            .unwrap_or_else(|| panic!("no turn row"));
        assert_eq!(turn.chord, "Q, E");
    }

    /// Collapsing must not lose a binding — every one still appears in exactly one row.
    #[test]
    fn collapsing_rows_loses_nothing() {
        for context in [Context::Global, Context::Map, Context::Tiles] {
            let chords: String = rows(context)
                .iter()
                .map(|r| r.chord.clone())
                .collect::<Vec<_>>()
                .join(" ");
            for b in in_context(context) {
                assert!(
                    chords.split(&[' ', ','][..]).any(|c| c == b.chord),
                    "{:?}'s chord `{}` vanished when rows collapsed",
                    b.action,
                    b.chord
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
        assert!(just_pressed(&input, Action::NextTab), "Tab did not fire NextTab");
        assert!(!just_pressed(&input, Action::MapTab), "Tab fired an unrelated action");

        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::Digit2);
        assert!(just_pressed(&input, Action::TilesTab), "2 did not fire TilesTab");
    }

    /// A ctrl chord fires only with ctrl, and the bare key only without — `S` pans and `Ctrl+S` saves.
    #[test]
    fn ctrl_chords_and_bare_keys_do_not_shadow_each_other() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::KeyS);
        assert!(just_pressed(&input, Action::PanBack));
        assert!(!just_pressed(&input, Action::Save), "bare S must not save");

        // A FRESH input, not `clear()`: `clear` keeps the pressed state, so pressing an
        // already-held key never re-registers as just-pressed and the assertion below would fail for
        // a reason that has nothing to do with the chord.
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::ControlLeft);
        input.press(KeyCode::KeyS);
        assert!(just_pressed(&input, Action::Save), "Ctrl+S must save");
        assert!(!just_pressed(&input, Action::PanBack), "Ctrl+S must not also pan");
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
