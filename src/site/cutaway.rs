//! **The Site's knee-wall cutaway** — near walls squash so you can see into the rooms.
//!
//! # Why the hub needed its own, and why that is not a second path
//!
//! The dungeon has squashed its near walls since long before Site-67 existed
//! (`dungeon::cutaway`). The Site never did, because it is deliberately isolated from every
//! dungeon-grid subsystem — fog, the light field, mould, almond water and the cutaway are all grid
//! indexed, and the Site achieves its exemption by *distance* rather than by a `Visibility` toggle on
//! every entity at every state change (`site::mod`, `site::visuals`).
//!
//! The cost of that isolation was invisible while the hub was empty and became the dominant visual
//! defect once it was not: at two of the four yaw detents whole wings sit behind a full-height 2.4 m
//! wall, so the staff, the dressing and the containment booths shipped through July and August are
//! *conditionally invisible* depending on where the player last pressed Q.
//!
//! Reusing `dungeon::cutaway`'s `update_cutaway` outright does not work, and the reason is geometric
//! rather than architectural:
//!
//! * **`wall_pose` assumes a CENTRED mesh.** It returns an absolute `translation.y` of
//!   `WALL_HEIGHT * 0.5`, because a dungeon wall's origin is its middle. Every Site mesh is
//!   **base-origined** — `tests/ozea_asset.rs` pins that against the bytes, and `visuals::place`
//!   relies on it by adding only `y_offset`. Applying the dungeon's pose to a Site wall would drop it
//!   1.2 m through the floor.
//! * **`wall_pose` assumes a base `scale.y` of 1.** Site walls already carry `kit.y_scale(piece)`, the
//!   art correction that brings whatever the artist made to `WALL_HEIGHT`. Writing an absolute scale
//!   would silently un-stretch every wall in the hub — a defect that reads as "the kit changed".
//!
//! So the **decision** is shared and only the **pose** differs: [`crate::dungeon::cutaway::faces_camera`]
//! and the ease constants are used verbatim, and a base-origined wall's knee pose turns out to be the
//! simpler of the two — scaling shrinks it toward its own base, so `translation.y` never moves at all.
//!
//! Cosmetic by construction: `Update`, windowed-only, and it writes nothing but `Transform.scale.y` on
//! entities that carry no `Health`.

use bevy::prelude::*;

use super::layout::SiteLayout;
use crate::dungeon::cutaway::faces_camera;
use crate::dungeon::CAMERA_WALL_FRACTION;
use crate::ui::state::AppState;

/// A Site wall that squashes when its outward face turns toward the camera.
///
/// Deliberately **not** `dungeon::cutaway::CutawayWall`. That component is queried by
/// `update_cutaway` with no marker or state filter of its own, so a Site wall carrying it would be
/// re-posed by the dungeon's system — with the dungeon's centred-mesh maths — during any
/// two-live-layers visit, while an expedition is `Active`. Two systems writing one `Transform` is the
/// shape this codebase rejects; a distinct component means each system owns its own walls and neither
/// needs to know the other exists.
#[derive(Component, Debug, Clone, Copy)]
pub struct SiteKneeWall {
    /// Outward horizontal normal (±X/±Z) — points **away from the floor this wall encloses**.
    pub outward: Vec3,
    /// The `scale.y` this wall was placed with (`kit.y_scale` × `kit.scale`), which is what full
    /// height means for it. Captured at spawn rather than recomputed, so re-skinning the kit cannot
    /// leave the cutaway disagreeing with the placement about how tall a wall is.
    pub base_scale_y: f32,
}

/// The knee and full `scale.y` for a **base-origined** wall.
///
/// Both are relative to the wall's own authored scale, and `translation.y` is absent from the return
/// because it does not change: a base-origined mesh scaled about its origin shrinks toward the floor
/// it is standing on. That is the whole difference from `dungeon::cutaway::wall_pose`, which has to
/// re-seat a centred mesh as it shrinks.
pub fn knee_scale(base_scale_y: f32, near: bool) -> f32 {
    if near {
        base_scale_y * CAMERA_WALL_FRACTION
    } else {
        base_scale_y
    }
}

/// Which way does this wall panel face away from the room it encloses?
///
/// `visuals::wall_panels` derives panels as the *boundary* between a floor cell and a wall cell, so
/// the normal is not a property of the panel's own position — it is "which side of me can somebody
/// stand on". Derived, never authored, like every other Site geometry fact.
///
/// `None` when neither side is walkable (a wall inside a wall — it encloses nothing) or when **both**
/// are (a free-standing divider with floor on either side, which occludes whichever room the camera
/// is outside of, so squashing it by one normal would be wrong half the time). Leaving those at full
/// height is the honest answer rather than picking a side.
pub fn panel_outward(l: &SiteLayout, (x, z, along_x): (i32, i32, bool)) -> Option<Vec3> {
    let (near_cell, far_cell, axis) = if along_x {
        // An X-running panel sits on the boundary at z; the cells either side are z-1 and z.
        (IVec2::new(x, z - 1), IVec2::new(x, z), Vec3::Z)
    } else {
        (IVec2::new(x - 1, z), IVec2::new(x, z), Vec3::X)
    };
    match (l.is_walkable(near_cell), l.is_walkable(far_cell)) {
        // Room on the low side: the wall faces away from it, up-axis.
        (true, false) => Some(axis),
        (false, true) => Some(-axis),
        _ => None,
    }
}

/// The outward normal for a **corner cap**, summed from the panels that meet at its lattice point.
///
/// A corner sits at a vertex rather than on an edge, so it has no single enclosing floor cell — the
/// dungeon carries the same idea as `Cutaway::Post(Vec3)` with its own diagonal normal. Summing the
/// incident panels' normals gives that diagonal for free, and gives `None` for a straight run (two
/// opposed normals cancelling), which is correct: a cap on a straight run has no corner to face.
///
/// Squashing the cap matters more than it sounds. A full-height post left standing at the end of a
/// knee wall does not read as "a post" — it reads as the cutaway being broken.
///
/// Not normalised: [`faces_camera`] only takes the sign of a dot product, so the length is irrelevant
/// and normalising would be arithmetic nobody reads.
pub fn corner_outward(
    l: &SiteLayout,
    panels: &std::collections::BTreeSet<(i32, i32, bool)>,
    v: (i32, i32),
) -> Option<Vec3> {
    let (vx, vz) = v;
    // The four panels that can touch this vertex: two running in Z (above and below it) and two
    // running in X (either side of it).
    let incident = [
        (vx, vz, false),
        (vx, vz - 1, false),
        (vx, vz, true),
        (vx - 1, vz, true),
    ];
    let sum = incident
        .iter()
        .filter(|p| panels.contains(*p))
        .filter_map(|p| panel_outward(l, *p))
        .fold(Vec3::ZERO, |acc, n| acc + n);
    (sum != Vec3::ZERO).then_some(sum)
}

/// Ease every Site knee wall toward the pose implied by the current camera direction.
///
/// Structure lifted from `dungeon::cutaway::update_cutaway` including its two performance rules,
/// which are not optional here either:
///
/// * **Read through the immutable deref first.** `Mut` raises `Changed` on a *mutable* deref, so a
///   settled wall has to decide to skip before touching the transform. The Site has 300-odd walls; the
///   dungeon learned this at 192×192 (FVS perf pass, 2026-07-31).
/// * **Snap at the end.** An exponential ease approaches but never reaches its target, so without the
///   snap every wall is rewritten and re-extracted forever after the camera has visually settled.
pub fn squash_near_walls(
    time: Res<Time<bevy::time::Real>>,
    view: Res<crate::camera::CameraView>,
    mut walls: Query<(&SiteKneeWall, &mut Transform)>,
) {
    if view.to_camera == Vec3::ZERO {
        return; // not yet seeded by the camera (first-frame ordering) — leave the placed pose.
    }
    let ease = 1.0 - (-crate::dungeon::cutaway::CUTAWAY_SMOOTHING * time.delta_secs()).exp();
    for (wall, mut tf) in &mut walls {
        let target = knee_scale(wall.base_scale_y, faces_camera(wall.outward, view.to_camera));
        let cur = tf.scale.y;
        if cur == target {
            continue; // settled — leave no Changed<Transform> behind
        }
        if (target - cur).abs() < crate::dungeon::cutaway::CUTAWAY_SNAP {
            tf.scale.y = target; // final write: land exactly so the next frame skips
        } else {
            tf.scale.y = cur + (target - cur) * ease;
        }
    }
}

pub struct SiteCutawayPlugin;

impl Plugin for SiteCutawayPlugin {
    fn build(&self, app: &mut App) {
        // Gated on `AppState::Site` and NOT on `RunState`, which is the trap the dungeon's own
        // registration sets for anyone reusing it: `update_cutaway` runs only while
        // `RunState::Active`, and the hub between expeditions is `Idle`. A component attached with
        // the dungeon's run condition would animate the Site's walls during a two-live-layers visit
        // and freeze them at their placed pose the rest of the time — visibly worse than not having
        // it, because the walls would be knee-high or full height depending on unrelated state.
        app.add_systems(Update, squash_near_walls.run_if(in_state(AppState::Site)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A base-origined wall shrinks toward the floor: only the scale moves.
    ///
    /// The dungeon's `wall_pose` returns an absolute `(0.25, WALL_HEIGHT * 0.25 * 0.5)` because its
    /// walls are centred on their own middle. Applying that to a Site wall would both drop it through
    /// the deck and discard `y_scale`. This test is the difference, written down.
    #[test]
    fn a_knee_wall_keeps_its_kit_stretch_and_never_leaves_the_floor() {
        // A wall the kit stretches to 1.6x to reach `WALL_HEIGHT`.
        let base = 1.6;
        assert_eq!(knee_scale(base, false), base, "away from the camera it stands full height");
        assert_eq!(
            knee_scale(base, true),
            base * CAMERA_WALL_FRACTION,
            "squashing is RELATIVE to the kit's stretch — an absolute scale would un-stretch the wall"
        );
        // The dungeon's own pose, for contrast: absolute, and it moves the wall.
        let (dungeon_scale, dungeon_y) = crate::dungeon::cutaway::wall_pose(true);
        assert_eq!(dungeon_scale, CAMERA_WALL_FRACTION);
        assert!(
            dungeon_y > 0.0,
            "the dungeon re-seats a CENTRED mesh as it shrinks; a base-origined one needs no move"
        );
    }

    /// The normal points away from the floor, on both axes and both signs.
    #[test]
    fn a_wall_faces_away_from_the_room_it_encloses() {
        let l = SiteLayout::load().expect("the shipped layout must load");

        // Every panel the Site actually builds either gets a normal that points at a non-walkable
        // cell, or gets none at all. Nothing may face into the room it is supposed to be hiding.
        let mut with_normal = 0;
        for panel in crate::site::visuals::wall_panels(&l) {
            let Some(outward) = panel_outward(&l, panel) else {
                continue;
            };
            with_normal += 1;
            let (x, z, along_x) = panel;
            let room_side = if along_x {
                if outward.z > 0.0 { IVec2::new(x, z - 1) } else { IVec2::new(x, z) }
            } else if outward.x > 0.0 {
                IVec2::new(x - 1, z)
            } else {
                IVec2::new(x, z)
            };
            assert!(
                l.is_walkable(room_side),
                "panel {panel:?} faces {outward:?}, which puts the room at {room_side:?} — not floor"
            );
        }
        assert!(
            with_normal > 200,
            "the Site's perimeter is 262 panels; only {with_normal} got a normal, so most walls \
             would never squash and the cutaway would be cosmetic in name only"
        );
    }

    /// A wall with floor on both sides is left alone rather than guessed at.
    #[test]
    fn a_divider_with_floor_on_both_sides_gets_no_normal() {
        let l = SiteLayout::load().expect("the shipped layout must load");
        // The spine runs x[0,34) z[12,14); z=13 and z=12 are both walkable floor, so the boundary
        // between them encloses nothing and squashing it would be wrong from one side or the other.
        assert_eq!(panel_outward(&l, (16, 13, true)), None);
    }
}
