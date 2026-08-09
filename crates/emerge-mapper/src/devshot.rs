//! **Driving the editor from a script.** `drive.request` — whitespace-separated verbs applied through
//! the same resources the key handlers write, so a capture script can reproduce an author's exact
//! steps: `tiles`, `map`, `anim`, `compose`, `arm`, `stamp`, `down`, `up`.
//!
//! This exists because of how the editor actually gets checked: `scripts/vinput.py` pressing keys and
//! `scripts/framestats.py` measuring what came out. Three of the Site editor's bugs were invisible to
//! a green test suite and visible only in a measured frame — so the windowed sibling of the headless
//! probes has to be reachable without a person at the keyboard.
//!
//! **The capture half is `bevy_devshot`.** This file used to carry a byte-for-byte copy of the game's
//! `watch_sentinel`, with a comment promising the two stayed the same shape — the kind of promise that
//! goes stale the first time one side is edited. Both now register the one crate; what is left here is
//! the part that is genuinely the editor's, its verbs.

use std::fs;

use bevy::prelude::*;

/// `drive.request` — whitespace-separated verbs applied through the same resources the key
/// handlers write, so a script can reproduce an author's exact steps: `tiles`, `map`, `anim`,
/// `down`, `up`. The windowed sibling of the headless probes, for the bugs only pixels can show.
const DRIVE: &str = "drive.request";

/// The editor's `drive.request` verbs. Capture is `bevy_devshot::DevShotPlugin`, added beside this
/// one in `harness.rs` — two plugins because they are two jobs, and only one of them is ours.
pub struct DrivePlugin;

impl Plugin for DrivePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, watch_drive);
    }
}

#[allow(clippy::too_many_arguments)]
fn watch_drive(
    mut mode: ResMut<crate::tiles::Mode>,
    mut state: ResMut<crate::tiles::ImportState>,
    mut project: ResMut<crate::project::Project>,
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
    mut compose: ResMut<crate::compose::ComposeState>,
    mut editor: ResMut<crate::editor::EditorState>,
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
            "compose" => *mode = crate::tiles::Mode::Compose,
            // **Arm and stamp go through the same calls the keyboard and the click make**, so a
            // captured frame is evidence about the real path rather than about a test-only one.
            // The same call the key handler makes, toggle included — never a second copy of it.
            "arm" => {
                crate::compose::toggle_arm(&mut compose, &project);
                info!("drive.request: arm — {}", compose.status.line());
            }
            "stamp" => {
                crate::editor::stamp_here_for_test(
                    &mut project,
                    &mut editor,
                    &mut compose,
                    (0.0, 0.0),
                );
                info!("drive.request: stamp — {}", editor.status.line());
            }
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
