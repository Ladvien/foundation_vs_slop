//! `KeyCode` ↔ human name, the one table that makes a binding both **savable** and **showable**.
//!
//! Bindings persist as names (`"Ctrl+P"`) rather than as `KeyCode` discriminants, so
//! `user_settings.ron` stays human-editable and the save format does not depend on the layout of a
//! Bevy enum. The same names are what the controls screen prints, which is why one table serves
//! both — a key that cannot be written to disk is a key that cannot be shown to the player, and
//! `super::every_default_binding_can_be_written_to_disk` catches both failures at once.
//!
//! This is deliberately **not** every `KeyCode` Bevy defines. It is the set a player can be offered,
//! which excludes the modifiers themselves (they are `Mods`, not keys) and the long tail of media
//! and IME keys. Adding a key here is what makes it bindable.

use bevy::prelude::KeyCode;

/// The bindable key space. Order is display order for any future "press a key" picker.
const KEY_NAMES: &[(KeyCode, &str)] = &[
    (KeyCode::KeyA, "A"),
    (KeyCode::KeyB, "B"),
    (KeyCode::KeyC, "C"),
    (KeyCode::KeyD, "D"),
    (KeyCode::KeyE, "E"),
    (KeyCode::KeyF, "F"),
    (KeyCode::KeyG, "G"),
    (KeyCode::KeyH, "H"),
    (KeyCode::KeyI, "I"),
    (KeyCode::KeyJ, "J"),
    (KeyCode::KeyK, "K"),
    (KeyCode::KeyL, "L"),
    (KeyCode::KeyM, "M"),
    (KeyCode::KeyN, "N"),
    (KeyCode::KeyO, "O"),
    (KeyCode::KeyP, "P"),
    (KeyCode::KeyQ, "Q"),
    (KeyCode::KeyR, "R"),
    (KeyCode::KeyS, "S"),
    (KeyCode::KeyT, "T"),
    (KeyCode::KeyU, "U"),
    (KeyCode::KeyV, "V"),
    (KeyCode::KeyW, "W"),
    (KeyCode::KeyX, "X"),
    (KeyCode::KeyY, "Y"),
    (KeyCode::KeyZ, "Z"),
    (KeyCode::Digit0, "0"),
    (KeyCode::Digit1, "1"),
    (KeyCode::Digit2, "2"),
    (KeyCode::Digit3, "3"),
    (KeyCode::Digit4, "4"),
    (KeyCode::Digit5, "5"),
    (KeyCode::Digit6, "6"),
    (KeyCode::Digit7, "7"),
    (KeyCode::Digit8, "8"),
    (KeyCode::Digit9, "9"),
    (KeyCode::F1, "F1"),
    (KeyCode::F2, "F2"),
    (KeyCode::F3, "F3"),
    (KeyCode::F4, "F4"),
    (KeyCode::F5, "F5"),
    (KeyCode::F6, "F6"),
    (KeyCode::F7, "F7"),
    (KeyCode::F8, "F8"),
    (KeyCode::F9, "F9"),
    (KeyCode::F10, "F10"),
    (KeyCode::F11, "F11"),
    (KeyCode::F12, "F12"),
    // Arrows print as glyphs. All four are in `assets/fonts/FiraMono-Regular.ttf` (U+2190..2193);
    // `ui::theme` documents that the embedded default face is a 95-codepoint subset that would
    // tofu them, which is why every UI label goes through `FontAssets`.
    (KeyCode::ArrowUp, "\u{2191}"),
    (KeyCode::ArrowDown, "\u{2193}"),
    (KeyCode::ArrowLeft, "\u{2190}"),
    (KeyCode::ArrowRight, "\u{2192}"),
    (KeyCode::Space, "Space"),
    (KeyCode::Tab, "Tab"),
    (KeyCode::Escape, "Esc"),
    (KeyCode::Enter, "Enter"),
    (KeyCode::NumpadEnter, "NumEnter"),
    (KeyCode::Backspace, "Backspace"),
    (KeyCode::Home, "Home"),
    (KeyCode::End, "End"),
    (KeyCode::Insert, "Insert"),
    (KeyCode::Delete, "Delete"),
    (KeyCode::PageUp, "PageUp"),
    (KeyCode::PageDown, "PageDown"),
    (KeyCode::Minus, "-"),
    (KeyCode::Equal, "="),
    (KeyCode::BracketLeft, "["),
    (KeyCode::BracketRight, "]"),
    (KeyCode::Semicolon, ";"),
    (KeyCode::Quote, "'"),
    (KeyCode::Comma, ","),
    (KeyCode::Period, "."),
    (KeyCode::Slash, "/"),
    (KeyCode::Backslash, "\\"),
    (KeyCode::Backquote, "`"),
];

pub fn key_name(key: KeyCode) -> Option<&'static str> {
    KEY_NAMES.iter().find(|(k, _)| *k == key).map(|(_, n)| *n)
}

/// Case-insensitive for letters, so a hand-edited settings file saying `"w"` still loads.
pub fn key_from_name(name: &str) -> Option<KeyCode> {
    KEY_NAMES
        .iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(name))
        .map(|(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_a_bijection() {
        // A duplicate key would make `key_name` pick one arbitrarily; a duplicate NAME would make
        // `key_from_name` resolve two different keys to one, silently rebinding whichever lost.
        for (i, (k, n)) in KEY_NAMES.iter().enumerate() {
            for (k2, n2) in &KEY_NAMES[i + 1..] {
                assert_ne!(k, k2, "{k:?} appears twice");
                assert!(!n.eq_ignore_ascii_case(n2), "the name {n:?} is used twice");
            }
        }
    }

    #[test]
    fn every_entry_round_trips() {
        for (k, n) in KEY_NAMES {
            assert_eq!(key_name(*k), Some(*n));
            assert_eq!(key_from_name(n), Some(*k));
        }
    }

    #[test]
    fn a_hand_edited_lowercase_name_still_loads() {
        assert_eq!(key_from_name("w"), Some(KeyCode::KeyW));
        assert_eq!(key_from_name("esc"), Some(KeyCode::Escape));
    }

    #[test]
    fn the_modifiers_are_not_bindable_keys() {
        // Ctrl/Alt/Shift/Super are `Mods`, not keys. Letting one into the table would allow
        // binding an action to a bare modifier, which then fires on every chord that uses it.
        for k in [
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ] {
            assert_eq!(key_name(k), None, "{k:?} must not be bindable on its own");
        }
    }
}
