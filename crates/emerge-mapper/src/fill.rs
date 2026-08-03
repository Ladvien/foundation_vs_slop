//! **Flood fill** — cover a region with the armed piece, stopping at what is already there.
//!
//! Press `F` and the cell under the cursor spreads outward: every empty neighbour inside the map's
//! bounds takes a copy of the brush, and the spread stops at anything already placed and at the map's
//! edge. It is the same algorithm as a paint bucket, and it is here for the same reason a paint
//! bucket exists — floors and walls are the bulk of a level and placing them one click at a time is
//! not authoring, it is typing.
//!
//! # A fill needs edges, which is why the map has bounds
//!
//! Without a stated extent, "fill" has no answer: the ground plane is infinite and the flood would
//! run until something stopped it, which is a cap pretending to be a rule. `Map::bounds` is the real
//! boundary, and this refuses to run outside it rather than clamping into it — clamping would place
//! a piece somewhere the author did not point.
//!
//! # The grid is the piece, per axis, and it follows the yaw
//!
//! Cells are the brush's own footprint, **X and Z separately**. That matters for anything that is not
//! square: a first version used the larger dimension for both and a fill of 1.45 × 0.5 m drawers came
//! out as neat rows with half a metre of bare floor between them — correct in the sense that nothing
//! overlapped, and obviously wrong to look at.
//!
//! The footprint is stated before yaw, so a brush turned a quarter of a turn has its axes swapped;
//! the fill asks for the extents *at the yaw it is filling with*, which is the only yaw any of these
//! pieces will have.
//!
//! # It is bounded, and it says so
//!
//! `MAX_CELLS` caps one fill. The cap exists because a 64 × 64 m map at 0.5 m cells is 16,384 GLB
//! instances and an author who meant to fill a room should not get a locked-up editor — but a silent
//! cap is worse than no cap, because a fill that stopped early looks exactly like a fill that
//! finished. When it truncates it says how many it placed and that it stopped.

use std::collections::HashSet;

use emerge_core::descriptor::Descriptor;
use emerge_core::map::{Map, Placed};

/// The most placements one fill may create. See the module note on why it is loud.
pub const MAX_CELLS: usize = 4096;

/// The smallest cell a fill will step on, metres — the authoring snap. A piece with no recorded
/// footprint gets this rather than a zero step, which would be an infinite loop wearing a fill's
/// clothes.
pub const MIN_CELL: f32 = 0.5;

/// What a fill did, so the caller can report it honestly.
pub struct Filled {
    pub placements: Vec<Placed>,
    /// True when [`MAX_CELLS`] stopped it before the region was covered.
    pub truncated: bool,
}

/// The (x, z) step this brush fills on at `yaw_deg`, rounded to the authoring snap.
///
/// A footprint is recorded before rotation, so a piece turned 90° or 270° presents its depth along X.
/// Anything in between does not tile at all — a 30°-rotated rectangle has no grid that covers the
/// plane — so those round to the nearer quarter turn, which is the yaw a fill is actually useful at.
pub fn cell_extents(d: &Descriptor, yaw_deg: f32) -> (f32, f32) {
    let (w, depth) = d.extent.footprint.unwrap_or((MIN_CELL, MIN_CELL));
    let quarter = (yaw_deg.rem_euclid(360.0) / 90.0).round() as i32 % 4;
    let (x, z) = if quarter % 2 == 1 { (depth, w) } else { (w, depth) };
    (snap_cell(x), snap_cell(z))
}

fn snap_cell(v: f32) -> f32 {
    let raw = v.max(MIN_CELL);
    (raw / MIN_CELL).round() * MIN_CELL
}

/// Flood from `start` outward, returning the placements to add.
///
/// `next_id` is called for each new placement so ids stay unique across the whole map rather than
/// within this fill.
pub fn flood(
    map: &Map,
    brush: &Descriptor,
    start: (f32, f32),
    yaw: f32,
    mut next_id: impl FnMut() -> String,
) -> Result<Filled, String> {
    let (cell_x, cell_z) = cell_extents(brush, yaw);
    // From the map, never re-derived here: `floor_rect` owns the centre-on-origin convention.
    let (min_x, min_z, max_x, max_z) = map.floor_rect();

    // Integer cell coordinates, so membership in `seen` is exact. Comparing floats for "have I been
    // here" is how a flood fill revisits a cell forever.
    let to_cell = |p: (f32, f32)| -> (i64, i64) {
        (
            (p.0 / cell_x).floor() as i64,
            (p.1 / cell_z).floor() as i64,
        )
    };
    let centre_of = |c: (i64, i64)| -> (f32, f32) {
        (
            (c.0 as f32 + 0.5) * cell_x,
            (c.1 as f32 + 0.5) * cell_z,
        )
    };

    let start_cell = to_cell(start);
    let inside = |c: (i64, i64)| -> bool {
        let (x, z) = centre_of(c);
        x >= min_x && x < max_x && z >= min_z && z < max_z
    };
    if !inside(start_cell) {
        return Err(format!(
            "that point is outside the map — it runs x {min_x:.0}..{max_x:.0}, z {min_z:.0}..{max_z:.0}. \
             Grow the map or fill inside it; a fill that clamped itself back in would put pieces \
             where you did not point."
        ));
    }

    // Everything already placed blocks the flood. Keyed by cell so a piece anywhere in the cell
    // stops it, which is what "fill up to the wall" means.
    let occupied: HashSet<(i64, i64)> = map.placements.iter().map(|p| to_cell(p.at)).collect();
    if occupied.contains(&start_cell) {
        return Err("there is already something here".to_owned());
    }

    let mut seen: HashSet<(i64, i64)> = HashSet::new();
    let mut queue = vec![start_cell];
    let mut out = Vec::new();
    let mut truncated = false;
    seen.insert(start_cell);

    while let Some(c) = queue.pop() {
        if out.len() >= MAX_CELLS {
            truncated = true;
            break;
        }
        let at = centre_of(c);
        out.push(Placed {
            id: next_id(),
            descriptor: brush.id.clone(),
            at,
            yaw,
            ..Placed::default()
        });

        for step in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = (c.0 + step.0, c.1 + step.1);
            if seen.contains(&n) || occupied.contains(&n) || !inside(n) {
                continue;
            }
            seen.insert(n);
            queue.push(n);
        }
    }

    Ok(Filled {
        placements: out,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emerge_core::descriptor::Extent;

    fn brush(w: f32) -> Descriptor {
        Descriptor {
            id: "floor".into(),
            extent: Extent {
                footprint: Some((w, w)),
                height: Some(0.1),
            },
            ..Descriptor::default()
        }
    }

    fn map(bounds: (f32, f32, f32)) -> Map {
        Map {
            bounds,
            ..Map::default()
        }
    }

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("f{n}")
        }
    }

    #[test]
    fn a_fill_covers_the_map_exactly_once_per_cell() {
        let m = map((4.0, 3.0, 4.0));
        let f = flood(&m, &brush(1.0), (0.5, 0.5), 0.0, ids()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.placements.len(), 16, "4x4 m centred on the origin, at 1 m cells");
        assert!(!f.truncated);

        let mut seen = HashSet::new();
        for p in &f.placements {
            assert!(
                seen.insert((p.at.0.to_bits(), p.at.1.to_bits())),
                "cell {:?} filled twice",
                p.at
            );
        }
    }

    /// The cell is the piece. A fill with a 2 m piece over a 4 m map lays four, not sixteen.
    #[test]
    fn the_cell_is_the_brushs_footprint() {
        let m = map((4.0, 3.0, 4.0));
        let f = flood(&m, &brush(2.0), (1.0, 1.0), 0.0, ids()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.placements.len(), 4);
    }

    /// **The flood stops at what is already there** — which is what makes it useful for filling a
    /// room rather than a rectangle.
    #[test]
    fn existing_placements_dam_the_flood() {
        let mut m = map((5.0, 3.0, 1.0));
        // Centred on the origin, a 5 x 1 m strip spans x -2.5..2.5 and z -0.5..0.5, so its cell
        // centres are x = -2.5 -1.5 -0.5 0.5 1.5 on the single row z = -0.5. A wall on x = 0.5 splits
        // it three-and-one.
        m.placements.push(Placed {
            id: "wall".into(),
            descriptor: "wall".into(),
            at: (0.5, -0.5),
            ..Placed::default()
        });
        let f = flood(&m, &brush(1.0), (-2.5, -0.5), 0.0, ids()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.placements.len(), 3, "should fill only up to the wall");
        for p in &f.placements {
            assert!(p.at.0 < 0.5, "leaked past the wall to {:?}", p.at);
        }
    }

    #[test]
    fn a_fill_outside_the_map_is_refused_not_clamped() {
        let m = map((4.0, 3.0, 4.0));
        let err = flood(&m, &brush(1.0), (99.0, 99.0), 0.0, ids())
            .err()
            .unwrap_or_default();
        assert!(err.contains("outside the map"), "{err}");
    }

    #[test]
    fn starting_on_something_is_refused() {
        let mut m = map((4.0, 3.0, 4.0));
        m.placements.push(Placed {
            id: "x".into(),
            descriptor: "crate".into(),
            at: (0.5, 0.5),
            ..Placed::default()
        });
        assert!(flood(&m, &brush(1.0), (0.5, 0.5), 0.0, ids())
            .err()
            .unwrap_or_default()
            .contains("already something here"));
    }

    /// A silent cap is worse than no cap: a fill that stopped early looks exactly like one that
    /// finished, so the caller has to be able to say so.
    #[test]
    fn a_huge_region_truncates_and_says_so() {
        let m = map((200.0, 3.0, 200.0));
        let f = flood(&m, &brush(0.5), (0.25, 0.25), 0.0, ids()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.placements.len(), MAX_CELLS);
        assert!(f.truncated, "must report that it stopped short");
    }

    /// A piece with no recorded footprint still steps, rather than looping on a zero-width cell.
    /// A rectangular piece must TILE, not sit in rows with bare floor between them. The first
    /// version stepped by the larger dimension on both axes and a fill of 1.5 x 0.5 m drawers came
    /// out striped.
    #[test]
    fn a_rectangular_piece_tiles_instead_of_striping() {
        let mut d = brush(1.0);
        d.extent.footprint = Some((2.0, 0.5));
        assert_eq!(cell_extents(&d, 0.0), (2.0, 0.5));
        // Turned a quarter, it presents its depth along X.
        assert_eq!(cell_extents(&d, 90.0), (0.5, 2.0));
        assert_eq!(cell_extents(&d, 270.0), (0.5, 2.0));
        assert_eq!(cell_extents(&d, 180.0), (2.0, 0.5));

        // 4 x 4 m at 2.0 x 0.5 cells is 2 columns of 8.
        let f = flood(&map((4.0, 3.0, 4.0)), &d, (-1.0, -1.75), 0.0, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.placements.len(), 16);
    }

    #[test]
    fn a_piece_with_no_footprint_uses_the_authoring_snap() {
        let d = Descriptor {
            id: "mystery".into(),
            ..Descriptor::default()
        };
        assert_eq!(cell_extents(&d, 0.0), (MIN_CELL, MIN_CELL));
        let f = flood(&map((2.0, 3.0, 1.0)), &d, (-0.75, -0.25), 0.0, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.placements.len(), 8, "2x1 m at 0.5 m cells");
    }
}
