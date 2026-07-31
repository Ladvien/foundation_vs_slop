//! View-relative knee-wall cutaway: near walls squash to knee height so the camera can see into
//! rooms, and follow the Q/E rotation. Cosmetic — `Update` only, nothing hashed.
//! Split out of the former single-file `dungeon.rs` (3,447 lines) — a **pure move**, no logic
//! changed, so the replay goldens are untouched (FVS-N-1). `use super::*` at the top of each submodule
//! inherits the parent's imports, which is what keeps the move mechanical and reviewable: the diff is
//! whole items relocated, not hundreds of rewritten `use` lines.

use super::*;

/// The knee-wall cutaway: near walls squash so the camera can see into rooms, and follow the Q/E
/// rotation.
///
/// **Cosmetic, and separable on purpose.** It re-poses render geometry from the camera direction and
/// touches no pinned sim state, so it is `Update`-only and could be swapped for a different
/// see-into-rooms treatment (transparency, roof-fade) without the generator knowing. Its public
/// interface is the `CutawayWall` / `CutawayMounted` components other spawners attach.
pub struct CutawayPlugin;

impl Plugin for CutawayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_cutaway.distributive_run_if(in_state(crate::session::RunState::Active)));
    }
}

// ── View-relative knee-wall cutaway ──────────────────────────────────────────────────────────────
// The knee-wall cutaway used to be baked once at spawn for the fixed (+X,+Z) iso view. With Q/E map
// rotation the camera can look from any of four corners, so *which* walls occlude a room changes with
// the view. These components + `update_cutaway` make the squash follow the camera: it is a purely
// *visual* effect — collision, navigation (`surface_nav`), and prop/nest/splat placement stay baked to
// the canonical orientation, so the camera never changes gameplay (it stays deterministic at every
// angle; only the rendered geometry re-poses).

/// Ease rate for the cutaway height/scale lerp — matched to the camera's own rotation smoothing so a
/// wall grows or shrinks over the same turn. Frame-rate independent via `1 − exp(−k·dt)`.
const CUTAWAY_SMOOTHING: f32 = 9.0;

/// Once a wall/decoration is this close to its target pose it is snapped exactly onto it, and from
/// then on `update_cutaway` never touches its `Transform` again until the camera turns. Without the
/// snap the exponential ease approaches but never *reaches* the target, so every wall tile in the
/// dungeon (a 192×192 map's worth) was written — and marked `Changed<Transform>` — every frame
/// forever, fanning out into transform propagation and render re-extraction for geometry that had
/// visually settled seconds ago (FVS perf pass, 2026-07-31). Well under a pixel at any zoom.
const CUTAWAY_SNAP: f32 = 2e-3;

/// How a spawned tile participates in the cutaway: floors don't; walls squash to knee height on the
/// near edge; wall-mounted lintels hide on the near edge; a corner post squashes like a wall but
/// carries its own (diagonal) outward normal since it sits at a tile-corner vertex, not on an edge.
/// Passed to the tile spawner so the tag and the initial (yaw=0) pose are set in one place.
#[derive(Clone, Copy)]
pub(crate) enum Cutaway {
    None,
    Wall,
    Mounted,
    Post(Vec3),
}

/// A wall that participates in the view-relative cutaway. `outward` is its outward-facing horizontal
/// normal (±X/±Z). The wall is squashed to `CAMERA_WALL_FRACTION` whenever that normal faces the
/// camera (its inner face then occludes the room). Full walls and corner arms both stand 0→`WALL_HEIGHT`.
#[derive(Component)]
pub struct CutawayWall {
    pub outward: Vec3,
}

/// A decoration mounted on a wall face (doorway lintel; wall-hung prop). `outward` is the host wall's
/// outward normal; the item is scaled to zero — hidden — while that wall is a near knee wall, so it
/// never floats in the cutaway gap above the squashed wall. `base_scale` is its shown scale. Hiding
/// rides `scale`, not `Visibility`, so it composes with the fog reveal (which owns `Visibility`).
#[derive(Component)]
pub struct CutawayMounted {
    pub outward: Vec3,
    pub base_scale: Vec3,
}

/// A wall's outward horizontal normal (±X/±Z), derived from its offset off the cell centre. Straight
/// walls sit ~0.4 along one axis; corner arms likewise, each dominant on a single axis — so the larger
/// component names the edge. The single classifier for both [`CutawayWall`] tagging and initial squash.
pub fn wall_outward(wall_pos: Vec3, cell_center: Vec3) -> Vec3 {
    let dx = wall_pos.x - cell_center.x;
    let dz = wall_pos.z - cell_center.z;
    if dx.abs() >= dz.abs() {
        Vec3::new(dx.signum(), 0.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, dz.signum())
    }
}

/// True when an outward-facing wall normal points toward the camera — its inner face occludes the room,
/// so the wall should be a knee wall. At the four 90° detents exactly the two adjacent edges qualify.
pub(crate) fn faces_camera(outward: Vec3, to_camera: Vec3) -> bool {
    outward.dot(to_camera) > 0.0
}

/// `(scale.y, translation.y)` for a wall standing 0→`WALL_HEIGHT`: knee-high and reseated on the floor
/// when near the camera, full height and centred otherwise.
pub(crate) fn wall_pose(near: bool) -> (f32, f32) {
    if near {
        (CAMERA_WALL_FRACTION, WALL_HEIGHT * CAMERA_WALL_FRACTION * 0.5)
    } else {
        (1.0, WALL_HEIGHT * 0.5)
    }
}

/// Ease every cutaway wall's height and every wall-mounted decoration's scale toward the pose implied
/// by the current camera direction, so the knee-wall cutaway rotates with the Q/E view. Visual only —
/// see the module comment above; nothing here touches nav, placement, or the fog's `Visibility`.
pub(crate) fn update_cutaway(
    time: Res<Time<bevy::time::Real>>,
    view: Res<crate::camera::CameraView>,
    mut walls: Query<(&CutawayWall, &mut Transform), Without<CutawayMounted>>,
    mut mounted: Query<(&CutawayMounted, &mut Transform), Without<CutawayWall>>,
) {
    if view.to_camera == Vec3::ZERO {
        return; // not yet seeded by the camera (first frame ordering) — leave the baked pose.
    }
    let ease = 1.0 - (-CUTAWAY_SMOOTHING * time.delta_secs()).exp();
    for (wall, mut tf) in &mut walls {
        let (scale_y, y) = wall_pose(faces_camera(wall.outward, view.to_camera));
        // Read through Deref (immutable) first: `Mut` only raises `Changed` on a *mutable* deref,
        // so a settled wall must decide to skip before touching the transform at all.
        let (cur_scale, cur_y) = (tf.scale.y, tf.translation.y);
        if cur_scale == scale_y && cur_y == y {
            continue; // settled — leave no Changed<Transform> behind
        }
        if (scale_y - cur_scale).abs() < CUTAWAY_SNAP && (y - cur_y).abs() < CUTAWAY_SNAP {
            tf.scale.y = scale_y; // final write: land exactly on target so next frame skips
            tf.translation.y = y;
        } else {
            tf.scale.y = cur_scale + (scale_y - cur_scale) * ease;
            tf.translation.y = cur_y + (y - cur_y) * ease;
        }
    }
    for (deco, mut tf) in &mut mounted {
        let target = if faces_camera(deco.outward, view.to_camera) {
            Vec3::ZERO
        } else {
            deco.base_scale
        };
        let cur = tf.scale;
        if cur == target {
            continue;
        }
        if (target - cur).abs().max_element() < CUTAWAY_SNAP {
            tf.scale = target;
        } else {
            tf.scale = cur + (target - cur) * ease;
        }
    }
}

// Coarse WFC operates on room slots; each expands to a `block`×`block` patch of fine tiles. At 1 tile =
// 1 m the sizes read at real, Backrooms-like human scale under 2.4 m (8 ft) ceilings. When `block` is
// vastly larger than the rooms each floats in deep negative space — the liminal look — so the
// generation shape is data-driven via the `dungeon:` slice of `assets/config/config.ron` (see
// `DungeonConfig`), not hardcoded here.
