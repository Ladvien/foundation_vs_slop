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

/// `drive.request` — whitespace-separated verbs applied through the same resources the key
/// handlers write, so a script can reproduce an author's exact steps: `tiles`, `map`, `anim`,
/// `down`, `up`. The windowed sibling of the headless probes, for the bugs only pixels can show.
const DRIVE: &str = "drive.request";

pub struct DevShotPlugin;

impl Plugin for DevShotPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (watch_sentinel, watch_drive));
    }
}

#[allow(clippy::too_many_arguments)]
fn watch_drive(
    mut mode: ResMut<crate::tiles::Mode>,
    mut state: ResMut<crate::tiles::ImportState>,
    project: Res<crate::project::Project>,
    previews: Query<Entity, With<crate::tiles::Preview>>,
    names: Query<&Name>,
    children: Query<&Children>,
    meshes: Query<&Mesh3d>,
    vis: Query<(
        Option<&Visibility>,
        Option<&bevy::camera::visibility::InheritedVisibility>,
        Option<&bevy::camera::visibility::ViewVisibility>,
    )>,
    transforms: Query<&GlobalTransform>,
) {
    let Ok(text) = fs::read_to_string(DRIVE) else {
        return;
    };
    let _ = fs::remove_file(DRIVE);
    for verb in text.split_whitespace() {
        match verb {
            "tiles" => {
                *mode = crate::tiles::Mode::Tiles;
                // The same first-entry scan every real entry path performs.
                if !state.scanned {
                    crate::tiles::scan(&project, &mut state);
                }
            }
            "map" => *mode = crate::tiles::Mode::Map,
            "anim" => *mode = crate::tiles::Mode::Anim,
            "down" | "up" => {
                let delta: isize = if verb == "down" { 1 } else { -1 };
                let n = state.candidates.len() as isize;
                if n > 0 {
                    let want = state.selected as isize + delta;
                    state.selected = want.rem_euclid(n) as usize;
                    state.selected_library_id = None;
                }
            }
            // The whole staged-preview hierarchy, one line per entity: what exists, where it
            // stands, and which visibility gate says no.
            "dump" => {
                fn walk(
                    e: Entity,
                    depth: usize,
                    names: &Query<&Name>,
                    children: &Query<&Children>,
                    meshes: &Query<&Mesh3d>,
                    vis: &Query<(
                        Option<&Visibility>,
                        Option<&bevy::camera::visibility::InheritedVisibility>,
                        Option<&bevy::camera::visibility::ViewVisibility>,
                    )>,
                    transforms: &Query<&GlobalTransform>,
                ) {
                    let name = names.get(e).map(|n| n.as_str().to_owned()).unwrap_or_default();
                    let mesh = meshes.get(e).is_ok();
                    let (v, iv, vv) = vis.get(e).unwrap_or((None, None, None));
                    let at = transforms
                        .get(e)
                        .map(|t| t.translation())
                        .unwrap_or(Vec3::NAN);
                    info!(
                        "dump: {:indent$}{e} `{name}` mesh={mesh} vis={v:?} inherited={:?} view={:?} at=({:.1}, {:.1}, {:.1})",
                        "",
                        iv.map(|x| x.get()),
                        vv.map(|x| x.get()),
                        at.x,
                        at.y,
                        at.z,
                        indent = depth * 2,
                    );
                    if let Ok(kids) = children.get(e) {
                        for k in kids.iter() {
                            walk(k, depth + 1, names, children, meshes, vis, transforms);
                        }
                    }
                }
                for root in &previews {
                    walk(root, 0, &names, &children, &meshes, &vis, &transforms);
                }
            }
            other => warn!("drive.request: unknown verb `{other}`"),
        }
    }
    info!("drive.request applied: {}", text.trim());
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
