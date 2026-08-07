//! **A frame on demand.** Bevy renders a PNG straight from the GPU — no OS screen-capture, no
//! accessibility permissions, no window manager involved — triggered by a sentinel file so it can be
//! driven from a shell, a script, or an agent that has no keyboard:
//!
//! ```text
//! touch screenshot.request     # the next frame writes screenshot.png in the working directory
//! ```
//!
//! Being a *file* rather than a key binding is the point: it works over SSH, in CI, from a Makefile,
//! and from a process that is not the one drawing the window.
//!
//! # Register it behind a debug gate
//!
//! [`DevShotPlugin`] polls one `Path::exists` per frame and does nothing else, but a shipped game has
//! no reason to watch the filesystem for a screenshot request. Gate the registration:
//!
//! ```ignore
//! #[cfg(debug_assertions)]
//! app.add_plugins(bevy_devshot::DevShotPlugin);
//! ```

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
