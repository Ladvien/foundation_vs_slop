//! **A frame, on demand.** `touch screenshot.request` → the next frame writes `screenshot.png` in
//! the working directory.
//!
//! Straight from the GPU via Bevy, not an OS screen-capture: no compositor permissions, no window
//! manager involved, and it works while the editor is driven by a script rather than a person. That
//! matters more here than it does in the game, because the way this editor gets checked is
//! `scripts/vinput.py` pressing keys and `scripts/framestats.py` measuring what came out — three of
//! the Site editor's bugs were invisible to a green test suite and visible only in a measured frame.
//!
//! Same shape as the game's `src/devshot.rs`, deliberately, so one capture rig drives both.

use std::fs;
use std::path::Path;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

const REQUEST: &str = "screenshot.request";
const OUTPUT: &str = "screenshot.png";

pub struct DevShotPlugin;

impl Plugin for DevShotPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, watch_sentinel);
    }
}

fn watch_sentinel(mut commands: Commands) {
    if !Path::new(REQUEST).exists() {
        return;
    }
    let _ = fs::remove_file(REQUEST);
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(OUTPUT));
    info!("devshot: wrote {OUTPUT}");
}
