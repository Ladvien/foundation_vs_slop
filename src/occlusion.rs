//! Room-based camera-side wall cutaway. In the fixed 45° isometric view the walls between the camera
//! and a room's interior (the E/S faces, since the camera looks from (+X,+Z)) hide anything inside —
//! including the squad. The fix is the standard iso solution: for every room a squad member is in,
//! turn off its two camera-facing walls so the squad reads clearly, while every other wall stays solid.
//!
//! Because the camera never rotates (see `camera`), "toward the camera" on the ground plane is the
//! constant direction (+X,+Z); a wall on its cell's E or S edge sits on that side, so the occluder set
//! is classified analytically on the CPU — no GPU readback, fully deterministic.
//!
//! Fog (see `fog`) owns each wall's `Visibility` (one-way Hidden→Visible reveal). To keep a single,
//! non-conflicting authority per component, the cutaway only ever writes the wall's *material* (never
//! `Visibility`): it swaps between the two shared handles in [`WallMaterials`]. The two concerns are
//! orthogonal, so a not-yet-revealed wall stays `Hidden` regardless of which material it holds.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::dungeon::{Dungeon, Tile, Wall};
use crate::placement::ir::RegionId;
use crate::squad::Unit;

/// The two shared wall material handles the cutaway swaps between (built in `dungeon`'s tile spawn).
/// Only these two ever exist, so per-wall cutaway costs a handle swap, not a new material asset.
#[derive(Resource)]
pub struct WallMaterials {
    pub opaque: Handle<StandardMaterial>,
    /// Fully transparent — the camera-facing walls of an occupied room switch to this to reveal the squad.
    pub invisible: Handle<StandardMaterial>,
}

/// A wall counts as camera-facing when its position sits at least this far toward +X or +Z of its
/// cell centre (its E or S edge). Straight edges sit at ≈0.4 and corner arms similarly, so 0.1 cleanly
/// separates the near (E/S) faces from the far (N/W) ones.
const CAMERA_FACING_EPS: f32 = 0.1;

pub struct OcclusionPlugin;

impl Plugin for OcclusionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, room_cutaway);
    }
}

/// Each frame, turn off (make invisible) the E/S walls of every room a squad member occupies, and
/// restore all other walls to opaque.
fn room_cutaway(
    units: Query<&Transform, With<Unit>>,
    dungeon: Res<Dungeon>,
    mats: Res<WallMaterials>,
    mut walls: Query<(&Tile, &Transform, &mut MeshMaterial3d<StandardMaterial>), With<Wall>>,
) {
    // In knee-wall mode the camera-facing walls are permanently short, so there's nothing to cut away.
    if crate::dungeon::SHORT_CAMERA_WALLS {
        return;
    }

    // Rooms currently holding a squad member.
    let occupied: HashSet<RegionId> = units
        .iter()
        .filter_map(|t| dungeon.region_at(dungeon.world_to_cell(t.translation)))
        .collect();

    for (tile, wall_tf, mut material) in &mut walls {
        // Cut the wall only if it belongs to an occupied room and faces the camera (E or S edge).
        let cut = match dungeon.region_at(tile.cell) {
            Some(region) if occupied.contains(&region) => {
                let center = dungeon.cell_center(tile.cell);
                (wall_tf.translation.x - center.x) > CAMERA_FACING_EPS
                    || (wall_tf.translation.z - center.z) > CAMERA_FACING_EPS
            }
            _ => false,
        };

        let want = if cut { &mats.invisible } else { &mats.opaque };
        // Change-detection guard: only write when the handle differs, so Bevy doesn't re-flag every
        // wall's material as changed every frame.
        if material.0.id() != want.id() {
            material.0 = want.clone();
        }
    }
}
