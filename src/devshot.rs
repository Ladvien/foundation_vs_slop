//! Dev-only screenshot helper. Bevy renders a PNG straight from the GPU (no OS screen-capture,
//! no permissions), triggered by a sentinel file so it can be driven headlessly from a shell:
//! `touch screenshot.request` → the next frame writes `screenshot.png` in the working directory.
//! Remove this module (and its plugin registration) for release builds.

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
