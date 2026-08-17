//! **Narrowing a list by typing at it.**
//!
//! The map palette is 41 rows and the tiles candidate list is 318. Grouping made the first one
//! navigable — `docs/ui.md` §3.5, fixed positions, never reordered by recency — but grouping does not
//! scale to three hundred, where the only way to reach a row is to remember which pack it came from
//! and scroll. A filter is the escape hatch that does not disturb the ordering: the rows that survive
//! stay in exactly the order they were in, so what an author learned about where things sit is still
//! true. Nothing is re-ranked, ever.
//!
//! One box per list, and the text **persists when focus leaves it** — that is why this is its own
//! resource rather than the transient buffer the name/id/size fields share. A filter you have to
//! retype every time you click a row is a filter nobody uses twice.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton};

use crate::chrome::{ACCENT, CHIP_PAD, DIM, FOCUS_BG, ROW_BG, ROW_HOVER, TEXT};

/// Which list a box narrows.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    /// The map tab's PLACE palette.
    Palette,
    /// The tiles tab's candidate list.
    Candidates,
    /// The animation bench's rig list.
    Rigs,
}

/// The filter text of each list, and which box is taking keys.
///
/// **Its own resource, and deliberately not a field on `EditorState`.** `rebuild_palette` runs on
/// `resource_changed::<EditorState>`, and every keystroke here has to rebuild the list it filters —
/// so this one is watched on purpose, where `SizeEdit` and `RemovalDrag` were split out to avoid
/// exactly that. The difference is whether the rebuild is the point.
#[derive(Resource, Default)]
pub struct Filters {
    palette: String,
    candidates: String,
    rigs: String,
    /// The box that owns the keyboard, or `None`. Read by `editor::not_typing`.
    focus: Option<Pane>,
}

impl Filters {
    /// What this list is narrowed to. Empty means everything.
    pub fn text(&self, pane: Pane) -> &str {
        match pane {
            Pane::Palette => &self.palette,
            Pane::Candidates => &self.candidates,
            Pane::Rigs => &self.rigs,
        }
    }

    /// Which box owns the keyboard, if any — the read half of [`Filters::take_focus`], so a test
    /// can assert who is typing without the field being public to every writer in the crate.
    pub fn focus_pane(&self) -> Option<Pane> {
        self.focus
    }

    /// **Put the keyboard in this box**, and start the search fresh — the same two writes
    /// `on_click` makes, so the `F` key and the mouse leave the filter in one state rather than two.
    ///
    /// A method rather than a public field: `focus` is read by `editor::not_typing` to decide
    /// whether every other key in the editor fires, and a field anything could set is a field that
    /// gets set from somewhere that has not thought about that.
    pub fn take_focus(&mut self, pane: Pane) {
        self.text_mut(pane).clear();
        self.focus = Some(pane);
    }

    /// Type one character into a box, for tests — the real path is `keys`, which reads the
    /// message stream and cannot be driven without an `App`.
    #[cfg(test)]
    pub fn push_for_test(&mut self, pane: Pane, c: char) {
        self.text_mut(pane).push(c);
    }

    fn text_mut(&mut self, pane: Pane) -> &mut String {
        match pane {
            Pane::Palette => &mut self.palette,
            Pane::Candidates => &mut self.candidates,
            Pane::Rigs => &mut self.rigs,
        }
    }

    /// Is a filter box taking keys right now?
    pub fn typing(&self) -> bool {
        self.focus.is_some()
    }

    /// **Give the keyboard back, keeping what was typed.**
    ///
    /// The text surviving is the whole point of this being its own resource (see the module note):
    /// an author who filtered to `grate`, placed one, and wants a second should not have to type it
    /// again. Only the focus goes.
    pub fn blur(&mut self) {
        self.focus = None;
    }

    /// Does this id survive the list's filter?
    ///
    /// Case-insensitive substring, not a fuzzy match: an author filtering `crt` wants the three CRTs,
    /// and a fuzzy matcher would also hand back `concrete_wall` for the same three letters. A
    /// predictable filter is one you can trust to have shown you everything.
    pub fn keeps(&self, pane: Pane, id: &str) -> bool {
        let needle = self.text(pane);
        needle.is_empty()
            || id
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
    }
}

/// The clickable box itself.
#[derive(Component, Clone, Copy)]
pub struct FilterBox(pub Pane);

/// The text inside it.
#[derive(Component, Clone, Copy)]
pub struct FilterText(pub Pane);

/// Spawn a filter box above a list.
pub fn spawn(parent: &mut ChildSpawnerCommands, pane: Pane) {
    parent
        .spawn((
            UiButton,
            Hovered::default(),
            FilterBox(pane),
            Node {
                width: Val::Percent(100.0),
                padding: CHIP_PAD,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(ROW_BG),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new("filter"),
                TextColor(DIM),
                TextFont::from_font_size(11.0),
                FilterText(pane),
            ));
        });
}

/// Click to focus. Clicking the box that is already focused clears it, which is the fastest way back
/// to the whole list and needs no second control.
pub fn on_click(activate: On<Activate>, boxes: Query<&FilterBox>, mut filters: ResMut<Filters>) {
    let Ok(b) = boxes.get(activate.entity) else {
        return;
    };
    // **A click always starts a fresh search, with the cursor in the box.** Clicking a focused box
    // used to clear it *and* blur it, so narrowing a list twice meant click, type, click, click, type
    // — the second click looked like it had done nothing. Enter and Escape still both leave, so there
    // is no way to get stuck in a box you did not mean to open.
    filters.text_mut(b.0).clear();
    filters.focus = Some(b.0);
}

/// Keystrokes into the focused box.
///
/// Ungated by `not_typing` — this system *is* the typing, so a guard against typing would stop it
/// from ever running. Same shape as `rename_keys` and `size_edit_keys`: read the buffered events,
/// ignore releases, match the logical key.
pub fn keys(mut events: MessageReader<KeyboardInput>, mut filters: ResMut<Filters>) {
    let Some(pane) = filters.focus else {
        return;
    };
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            // Enter and Escape both stop typing; Escape also throws the filter away, so there is one
            // key that always gets you back to the whole list.
            Key::Enter => filters.focus = None,
            Key::Escape => {
                filters.text_mut(pane).clear();
                filters.focus = None;
            }
            Key::Backspace => {
                filters.text_mut(pane).pop();
            }
            Key::Character(s) => {
                // No spaces: ids are snake_case and a space can only ever match nothing, so accepting
                // one would be accepting a keystroke that empties the list.
                let s = s.clone();
                if s.chars().all(|c| !c.is_whitespace()) {
                    filters.text_mut(pane).push_str(&s);
                }
            }
            _ => {}
        }
    }
}

/// Show what is typed, with a caret while focused and the placeholder when empty.
pub fn refresh(
    filters: Res<Filters>,
    mut texts: Query<(&FilterText, &mut Text, &mut TextColor)>,
    mut boxes: Query<(&FilterBox, &Hovered, &mut BackgroundColor)>,
) {
    // The fill answers to the pointer as well as to focus, so it repaints every frame — hover
    // cannot wait for `Filters` to change. The `!=` guard keeps the steady state a comparison.
    // Focus beats hover; hover is `chrome::ROW_HOVER`'s signifier that the box takes a click.
    for (which, hovered, mut bg) in &mut boxes {
        let want = if filters.focus == Some(which.0) {
            FOCUS_BG
        } else if hovered.0 {
            ROW_HOVER
        } else {
            ROW_BG
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    // The text, unlike the fill, only changes when the filter does.
    if !filters.is_changed() {
        return;
    }
    for (which, mut text, mut colour) in &mut texts {
        let focused = filters.focus == Some(which.0);
        let raw = filters.text(which.0);
        let (want, want_colour) = match (focused, raw.is_empty()) {
            (true, _) => (format!("{raw}_"), ACCENT),
            (false, true) => ("filter".to_owned(), DIM),
            (false, false) => (raw.to_owned(), TEXT),
        };
        if text.0 != want {
            text.0 = want;
        }
        if colour.0 != want_colour {
            colour.0 = want_colour;
        }
    }
}

/// **A click in the world takes the keyboard back from a filter box.**
///
/// Enter and Escape were the only ways out, and nothing said so. Meanwhile `editor::place_on_click`
/// is gated on `not_typing`, which reads [`Filters::typing`] — so filtering the palette to find a
/// piece and then clicking to place it did **nothing at all, silently**. That is the failure this
/// repo's rules exist to prevent, and it was reachable by the most natural way to find one piece
/// among a hundred and eighty.
///
/// # Why `Phase::Sense`, and why the same click still places
///
/// [`crate::keys::Phase`] exists to decide who owns the keyboard once, before anything reads a key.
/// Running here — ahead of `editor::sense_context`, which computes `Live` from this very flag —
/// means the focus is already gone by the time `place_on_click` evaluates `not_typing` in
/// `Phase::Act`. So one click blurs and places, rather than the first click being eaten as a
/// dismissal and the author clicking again.
///
/// A click on a widget is left alone. It might be this box, another filter, or a palette row, and
/// none of those is "the author is done here" — `on_filter_click` already owns the first case.
pub fn blur_on_world_click(
    mouse: Res<ButtonInput<MouseButton>>,
    hovered: Query<&bevy::picking::hover::Hovered>,
    mut filters: ResMut<Filters>,
) {
    if !mouse.just_pressed(MouseButton::Left) || !filters.typing() {
        return;
    }
    if hovered.iter().any(|h| h.0) {
        return;
    }
    filters.blur();
}

#[cfg(test)]
mod blur_tests {
    use super::*;

    /// Blurring gives back the keyboard and keeps the search — the two halves of the module note.
    #[test]
    fn blurring_keeps_the_text_and_drops_the_focus() {
        let mut f = Filters::default();
        f.focus = Some(Pane::Palette);
        *f.text_mut(Pane::Palette) = "grate".to_owned();

        f.blur();

        assert!(!f.typing(), "the keyboard goes back to the editor");
        assert_eq!(
            f.text(Pane::Palette),
            "grate",
            "the search survives, so placing a second one needs no retyping"
        );
    }
}
