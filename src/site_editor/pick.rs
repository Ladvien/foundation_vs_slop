//! **Picking a prop under the cursor — against the layout data, not the meshes.**
//!
//! # Why not mesh picking
//!
//! `bevy_picking` is in the build and would seem the obvious tool, but two facts rule it out here and
//! both are load-bearing:
//!
//! 1. **`MeshPickingSettings::require_markers` is `true`** (set by `dialogue::plugin`, with a long
//!    comment about the soft-lock that made it necessary). Nothing is pickable unless it carries
//!    `Pickable`, and Site props do not.
//! 2. **The mesh is not on the prop entity.** `site::visuals::place` spawns the GLB as a *child*
//!    (`WorldAssetRoot`), so making props pickable would mean tagging scene children as they stream
//!    in — a system whose whole job is to patch up an asset-load race.
//!
//! Against that, the layout already knows every prop's exact footprint, because
//! `layout::check_prop_placements` computes precisely these rectangles to test them for overlap. So
//! the pick is a point-in-oriented-rectangle test over data the editor is holding anyway: exact, free,
//! and immune to whether a GLB has finished loading.
//!
//! The ray itself is the repo's one ground-plane helper, `selection::cursor_ground_point` — a single
//! cursor ray against the infinite `y = 0` plane. Under the orthographic isometric camera that is
//! exact at any zoom and any of the four yaw detents.

use bevy::prelude::*;

use crate::placement::ir::Footprint;
use crate::site::kit::SiteKit;
use crate::site::layout::SiteLayout;
use crate::site::pieces::SitePiece;

/// Cursor → layout-space metres, or `None` when the cursor is off-window or there is no camera ray.
///
/// The returned pair is in the same space `PropPlacement::pos` is authored in — relative to cell
/// (0,0), before `SiteLayout::origin` is added — so it can be compared with a record directly.
pub fn cursor_layout_point(
    layout: &SiteLayout,
    window: &Window,
    camera: &Camera,
    cam_tf: &GlobalTransform,
) -> Option<(f32, f32)> {
    let world = crate::selection::cursor_ground_point(window, camera, cam_tf)?;
    Some((world.x - layout.origin.0, world.z - layout.origin.2))
}

/// The footprint a prop record occupies, in layout metres. The same construction
/// `layout::check_prop_placements` uses, so what the editor lets you grab is exactly what the rules
/// will measure.
pub fn footprint(kit: &SiteKit, piece: SitePiece, pos: (f32, f32), yaw_deg: f32) -> Footprint {
    let (fw, fd) = kit.footprint(piece);
    Footprint {
        x: pos.0,
        z: pos.1,
        yaw: yaw_deg.to_radians(),
        hw: fw * 0.5,
        hd: fd * 0.5,
    }
}

/// Is this layout-space point inside the footprint?
///
/// Uses the footprint's axis-aligned half-extents at its current yaw — `Footprint::half_extents`,
/// which swaps width and depth past a quarter turn. That is the same approximation the overlap rule
/// makes, and its doc explains why it is the right one for a game whose furniture is authored on
/// quarter turns: an oriented box would turn this into a separating-axis test for no visible gain.
fn contains(f: &Footprint, at: (f32, f32)) -> bool {
    let (hw, hd) = f.half_extents();
    (at.0 - f.x).abs() <= hw && (at.1 - f.z).abs() <= hd
}

/// Index of the prop under this layout-space point, or `None`.
///
/// Props genuinely stack — a mug rests on a table, and both footprints contain the cursor — so the
/// nearest centre wins, which picks the smaller, higher thing rather than the table under it.
pub fn prop_at(layout: &SiteLayout, kit: &SiteKit, at: (f32, f32)) -> Option<usize> {
    layout
        .props
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let f = footprint(kit, p.piece, p.pos, p.yaw);
            contains(&f, at).then(|| {
                let d = (p.pos.0 - at.0).powi(2) + (p.pos.1 - at.1).powi(2);
                (d, i)
            })
        })
        // The distance tie is not hypothetical: the shipped layout lays three FloorButton pads in a
        // row and stacks four crates, so two records really can sit at bit-identical distance from
        // one cursor point.
        //
        // SORT-OK: total comparator — squared distance, then the `props` vec index, which is unique
        // by construction. The input is a `Vec` in file order, never an ECS query.
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, i)| i)
}

/// Snap a layout-space point to the editor's placement grid.
///
/// Half a metre, not a whole one: cells are 1 m and props are authored on cell *centres*
/// (`41.5, 26.5`), so a 1 m grid would offer only the corners — the one set of positions the shipped
/// dressing never uses.
pub fn snap(at: (f32, f32)) -> (f32, f32) {
    ((at.0 * 2.0).round() * 0.5, (at.1 * 2.0).round() * 0.5)
}

/// Yaw step, degrees. Quarter turns are what the kit is authored for, but the shipped dressing also
/// uses 10°, 30° and 75° on crates to break up a stack, so the step is finer than 90°.
pub const YAW_STEP_DEG: f32 = 15.0;

/// Snap a yaw to [`YAW_STEP_DEG`] and wrap it into `[0, 360)`, the range the file is authored in.
pub fn snap_yaw(deg: f32) -> f32 {
    ((deg / YAW_STEP_DEG).round() * YAW_STEP_DEG).rem_euclid(360.0)
}
