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

use crate::chrome::{ACCENT, DIM, ROW_BG, SLOT_BG, TEXT};

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

    /// Does this id survive the list's filter?
    ///
    /// Case-insensitive substring, not a fuzzy match: an author filtering `crt` wants the three CRTs,
    /// and a fuzzy matcher would also hand back `concrete_wall` for the same three letters. A
    /// predictable filter is one you can trust to have shown you everything.
    pub fn keeps(&self, pane: Pane, id: &str) -> bool {
        let needle = self.text(pane);
        needle.is_empty() || id.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
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
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
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
pub fn on_click(
    activate: On<Activate>,
    boxes: Query<&FilterBox>,
    mut filters: ResMut<Filters>,
) {
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
    mut boxes: Query<(&FilterBox, &mut BackgroundColor)>,
) {
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
    for (which, mut bg) in &mut boxes {
        let want = if filters.focus == Some(which.0) {
            SLOT_BG
        } else {
            ROW_BG
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}
