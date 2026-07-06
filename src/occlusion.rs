//! Room-based camera-side wall cutaway. In the fixed 45° isometric view the walls between the camera
//! and a room's interior (the E/S faces, since the camera looks from (+X,+Z)) hide anything inside —
//! including the squad. The fix is the standard iso solution: turn off the camera-facing walls of every
//! room a squad member is in — plus, via a proximity cut, the walls around a member walking a corridor
//! (corridors belong to no region) — so the squad always reads clearly, while every other wall stays solid.
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

use crate::dungeon::{is_camera_facing_pos, Dungeon, Tile, Wall};
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

/// Distance (world units) within which a corridor-standing unit's camera-facing walls are cut. Corridor
/// cells belong to no region, so the room test can't reach them; this proximity cut keeps a unit walking
/// a corridor from being embedded in its camera-facing wall. ~2 m covers the unit's own cell plus the
/// wall just in front of it.
const CUTAWAY_RADIUS: f32 = 2.0;

pub struct OcclusionPlugin;

impl Plugin for OcclusionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, room_cutaway);
    }
}

/// Each frame, turn off (make invisible) the camera-facing (E/S) walls that would occlude a squad
/// member — the walls of any room a member occupies, plus (via a proximity cut) the walls around a
/// member standing in a corridor — and restore all other walls to opaque.
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

    // Rooms holding a squad member, plus the world positions of members standing in corridors (which
    // belong to no region, so the room test alone would leave a corridor-walker embedded in its
    // camera-facing wall — the bug the removed per-unit cutaway existed to fix).
    let mut occupied: HashSet<RegionId> = HashSet::new();
    let mut corridor_units: Vec<Vec3> = Vec::new();
    for t in &units {
        match dungeon.region_at(dungeon.world_to_cell(t.translation)) {
            Some(region) => {
                occupied.insert(region);
            }
            None => corridor_units.push(t.translation),
        }
    }

    for (tile, wall_tf, mut material) in &mut walls {
        // Only camera-facing (E/S) walls ever occlude the interior; cut one if it belongs to an occupied
        // room or sits within `CUTAWAY_RADIUS` of a corridor-standing member.
        let cut = is_camera_facing_pos(wall_tf.translation, dungeon.cell_center(tile.cell))
            && (matches!(dungeon.region_at(tile.cell), Some(r) if occupied.contains(&r))
                || corridor_units.iter().any(|u| {
                    wall_tf.translation.distance_squared(*u) < CUTAWAY_RADIUS * CUTAWAY_RADIUS
                }));

        let want = if cut { &mats.invisible } else { &mats.opaque };
        // Change-detection guard: only write when the handle differs, so Bevy doesn't re-flag every
        // wall's material as changed every frame.
        if material.0.id() != want.id() {
            material.0 = want.clone();
        }
    }
}
