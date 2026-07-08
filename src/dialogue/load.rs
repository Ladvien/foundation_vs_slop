//! Load + validate the authored dialogue RON into a [`DialogueScript`] resource.
//!
//! Mirrors the project's established config path (`config::load_game_config`): a single `std::fs`
//! read + `ron::from_str` + validation, inserted as a resource at plugin `build` time. There is no
//! Bevy `AssetLoader` and no hot-reload — one file, load-once, and a missing or malformed file is a
//! loud startup panic rather than a silent empty conversation set (one path, no fallback).

use super::model::{DialogueScript, validate_script};

/// Dialogue script path, relative to the project-root working directory (matches `config.rs`).
const DIALOGUE_PATH: &str = "assets/dialogue/script.dialogue.ron";

/// Read, parse, and validate the dialogue script. Any read/parse/validation failure is surfaced as
/// an `Err` for the caller to panic on loudly.
pub fn load_dialogue() -> Result<DialogueScript, String> {
    let text = std::fs::read_to_string(DIALOGUE_PATH)
        .map_err(|e| format!("cannot read {DIALOGUE_PATH}: {e}"))?;
    let script: DialogueScript =
        ron::from_str(&text).map_err(|e| format!("{DIALOGUE_PATH} parse error: {e}"))?;
    validate_script(&script)?;
    Ok(script)
}
