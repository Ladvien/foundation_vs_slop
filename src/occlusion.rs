//! Camera-side wall cutaway. In the isometric view a wall that sits *between the camera and
//! the hero* correctly depth-occludes the hero, which reads as the hero being "embedded" in
//! the wall (worst at inside corners). The fix is the standard iso solution: ghost those
//! occluding walls so the hero shows through, while every other wall stays fully solid.
//!
//! Method — this specializes the visibility/occlusion principle of Sung, "Visibility-Based
//! Fast Collision Detection of a Large Number of Moving Objects on GPU" (IEEE Access, 2023,
//! DOI 10.1109/access.2023.3277198): the depth test is what decides which objects occlude
//! which, and an object-ID buffer can read that back. We do NOT need the GPU readback here:
//! the camera is a fixed 45° orthographic rig that never rotates (see `camera`), so "toward
//! the camera" on the ground plane is the constant direction (+X, +Z). That lets us classify
//! the occluder set analytically on the CPU — zero latency, fully deterministic.
//!
//! Fog (see `fog`) owns each wall's `Visibility` (one-way Hidden→Visible reveal). To stay a
//! single, non-conflicting authority per component, the cutaway only ever writes the wall's
//! *material* (never `Visibility`): it swaps between the two shared handles in
//! [`WallMaterials`]. The two concerns are orthogonal, so neither fights the other, and a
//! not-yet-revealed wall is `Hidden` regardless of which material it holds.

use bevy::prelude::*;

use crate::dungeon::Wall;
use crate::squad::Unit;

/// The two shared wall material handles the cutaway swaps between (built in `dungeon`'s tile
/// spawn). Only these two ever exist, so per-wall fading costs a handle swap, not a new
/// material asset per wall.
#[derive(Resource)]
pub struct WallMaterials {
    pub opaque: Handle<StandardMaterial>,
    pub faded: Handle<StandardMaterial>,
}

// The bounds below are deliberately tight: fade *only* the walls actually between the camera
// and the hero (the ones it is "walking behind"). Anything wider ghosts walls the hero is not
// behind — which, where unexplored void lies beyond them, reads as the hero standing in void.

/// Below this (world units, along the camera axis) a wall is behind/level with the hero and
/// stays opaque. Slightly negative so the wall the hero is pressed right up against still fades.
const CAMERA_SIDE_EPS: f32 = -0.05;
/// How far camera-side (world units along the (+X,+Z) axis) a wall may be and still count as
/// occluding. The hero's own front (E/S) walls sit at ≈0.4 and the SE corner arm at ≈0.8; 1.4
/// also catches the one-tile-ahead wall that clips the hero's head, and no further.
const CUTAWAY_DEPTH: f32 = 1.4;
/// Screen-horizontal half-width (world units along the (+X,−Z) axis). ≈ the figurine's own
/// half-width, so only walls truly over the hero's silhouette fade — not the ones flanking it
/// along the same room edge (those sit at |lateral| ≳ 1).
const CUTAWAY_WIDTH: f32 = 0.6;

pub struct OcclusionPlugin;

impl Plugin for OcclusionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, wall_cutaway);
    }
}

/// Each frame, ghost the walls between the camera and **any** unit and restore all others.
fn wall_cutaway(
    units: Query<&Transform, With<Unit>>,
    mats: Res<WallMaterials>,
    mut walls: Query<(&Transform, &mut MeshMaterial3d<StandardMaterial>), With<Wall>>,
) {
    for (wall_transform, mut material) in &mut walls {
        // Grid X = world X, grid Y = world Z; the camera looks from (+X,+Y,+Z) toward the focus,
        // so its ground-plane "toward" axis is (+X,+Z) and screen-horizontal is (+X,−Z). A wall
        // ghosts if it occludes any squad member.
        let occludes = units.iter().any(|unit| {
            let rel = wall_transform.translation - unit.translation;
            let along = rel.x + rel.z; // > 0 ⇒ camera-side of the unit
            let lateral = rel.x - rel.z; // screen-horizontal offset
            along > CAMERA_SIDE_EPS && along < CUTAWAY_DEPTH && lateral.abs() < CUTAWAY_WIDTH
        });

        let want = if occludes { &mats.faded } else { &mats.opaque };
        // Change-detection guard: only write when the handle actually differs, so Bevy doesn't
        // re-flag every wall's material as changed every frame.
        if material.0.id() != want.id() {
            material.0 = want.clone();
        }
    }
}
