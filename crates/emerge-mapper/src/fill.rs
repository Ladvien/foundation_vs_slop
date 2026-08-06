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
    // **The placed footprint** — `stack::covers` reserves that box, so a fill stepping on the measured
    // one would lay a scaled piece down at a pitch its own reservation disagrees with.
    let (w, depth) =
        emerge_core::descriptor::placed_footprint(d).unwrap_or((MIN_CELL, MIN_CELL));
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
    // **A fill covers open floor, so a piece that mounts on something cannot be filled with.**
    // `stack::placement_at` already refuses such a piece cell by cell — that is the rule the click
    // path obeys — but the fill never asked, so it wrote one placement per cell that could never rest
    // anywhere and was never drawn. A measured run left **4,089 invisible lamps** in the map, all of
    // them saveable, none of them on screen: the file and the screen disagreeing, which is the exact
    // failure the one-path rule exists to prevent. Refused here, in the same shape as the other two
    // refusals, so there is one answer to "may this piece go here" rather than two.
    if let Some(class) = emerge_core::stack::needs_surface(brush) {
        return Err(format!(
            "`{}` goes on a `{class}` surface, so it cannot be flood filled — a fill covers open \
             floor and none of these cells offers one. Place it on a surface instead.",
            brush.id
        ));
    }

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

/// **Lay the brush across a dragged rectangle**, at its own cell pitch.
///
/// The twin of [`flood`], and deliberately the same gesture the removal tool already uses: press,
/// drag a box, release. A red box takes everything inside it; an accent box fills it.
///
/// # What it does not share with `flood`
///
/// `flood` spreads from a point until something stops it, so "where does it end" is the map's answer,
/// and a piece already standing somewhere is a wall to stop at. Here the author drew the edges, so the
/// rectangle is the answer and there is no spreading.
///
/// # It fills UNDER what is already there
///
/// A cell holding something is not a cell that is spoken for. Laying a floor across a dressed room is
/// the ordinary case, and the placement rules do not forbid it — `place_on_click` asks
/// `stack::placement_at` whether the *mount* can be satisfied and nothing else, so a single click has
/// always been able to put a floor tile under a crate. A fill that skipped those cells was answering a
/// stricter question than the click path, which is the sort of disagreement `CLAUDE.md`'s one-path rule
/// exists to prevent: the same piece, the same spot, two answers depending on the gesture.
///
/// The one thing skipped is a cell that already holds **this same descriptor**. That is not a rule
/// about space, it is about idempotence: dragging twice over the same floor would otherwise lay a
/// second identical tile inside the first, doubling the triangles and leaving a duplicate that only
/// shows up as a cost. Re-dragging to catch a missed corner is a thing authors do.
///
/// The refusals it *does* share are the ones about the brush rather than the region: a piece that
/// mounts on a surface cannot be laid on open floor, and [`MAX_CELLS`] caps the batch. Both are
/// `flood`'s, quoted rather than re-decided, so there is one answer to "may this piece go here".
pub fn box_fill(
    map: &Map,
    brush: &Descriptor,
    corners: ((f32, f32), (f32, f32)),
    yaw: f32,
    mut next_id: impl FnMut() -> String,
) -> Result<Filled, String> {
    // The same refusal `flood` makes, and for the same measured reason: filling with a
    // surface-mounted piece once wrote 4,089 invisible lamps into a map, all saveable, none drawable.
    if let Some(class) = emerge_core::stack::needs_surface(brush) {
        return Err(format!(
            "`{}` goes on a `{class}` surface, so it cannot be box filled — a fill covers open floor \
             and none of these cells offers one. Place it on a surface instead.",
            brush.id
        ));
    }

    let (cell_x, cell_z) = cell_extents(brush, yaw);
    let (min_x, min_z, max_x, max_z) = map.floor_rect();
    let (x0, z0) = (corners.0 .0.min(corners.1 .0), corners.0 .1.min(corners.1 .1));
    let (x1, z1) = (corners.0 .0.max(corners.1 .0), corners.0 .1.max(corners.1 .1));

    let to_cell = |p: (f32, f32)| -> (i64, i64) {
        ((p.0 / cell_x).floor() as i64, (p.1 / cell_z).floor() as i64)
    };
    let centre_of = |c: (i64, i64)| -> (f32, f32) {
        ((c.0 as f32 + 0.5) * cell_x, (c.1 as f32 + 0.5) * cell_z)
    };

    // Only this brush's own cells, so a re-drag is idempotent. Everything else is filled under.
    let mine: std::collections::HashSet<(i64, i64)> = map
        .placements
        .iter()
        .filter(|p| p.descriptor == brush.id)
        .map(|p| to_cell(p.at))
        .collect();

    let (cx0, cz0) = to_cell((x0, z0));
    let (cx1, cz1) = to_cell((x1, z1));
    let mut out = Vec::new();
    let mut truncated = false;
    // **Row-major, ascending.** A total order over the rectangle, so the ids the fill mints do not
    // depend on which corner the author happened to start from.
    'rows: for cz in cz0..=cz1 {
        for cx in cx0..=cx1 {
            if out.len() >= MAX_CELLS {
                truncated = true;
                break 'rows;
            }
            let at = centre_of((cx, cz));
            // **A cell belongs to the box only if its CENTRE is inside it** — the same rule the
            // removal box applies to placements (`p.at` within the rect), so the two box gestures
            // agree about what a rectangle contains. The corner range above is generous by
            // construction: `floor(corner/cell)` includes every cell a corner merely clips, and
            // committing those laid pieces whose centres sat up to half a pitch OUTSIDE the drawn
            // outline — the accent box understating the fill by up to a full cell per axis, against
            // the "the box drawn IS the box committed" rule.
            if !(at.0 >= x0 && at.0 <= x1 && at.1 >= z0 && at.1 <= z1) {
                continue;
            }
            // Inside the map, on the same half-open test `flood` uses.
            if !(at.0 >= min_x && at.0 < max_x && at.1 >= min_z && at.1 < max_z) {
                continue;
            }
            if mine.contains(&(cx, cz)) {
                continue;
            }
            out.push(Placed {
                id: next_id(),
                descriptor: brush.id.clone(),
                at,
                yaw,
                ..Placed::default()
            });
        }
    }

    if out.is_empty() {
        return Err(
            "nothing to fill there — that box is outside the map, or every cell in it already has \
             this piece in it"
                .to_owned(),
        );
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

    /// **The 4,089 invisible lamps.** A surface-mounted piece has no floor answer, so filling with one
    /// wrote a placement per cell that `stack` would refuse and `spawn_range` could never draw — the
    /// map held thousands of rows that were saveable and invisible. The refusal is the whole fill, up
    /// front, rather than a per-cell decision the caller then ignored.
    #[test]
    fn a_surface_piece_cannot_be_filled_with() {
        use emerge_core::descriptor::Mount;
        let lamp = Descriptor {
            id: "lamp_tall".into(),
            mount: Some(Mount::OnSurface {
                class: "worktop".into(),
            }),
            ..brush(0.5)
        };
        let m = map((4.0, 3.0, 4.0));
        let e = flood(&m, &lamp, (0.5, 0.5), 0.0, ids())
            .err()
            .unwrap_or_else(|| panic!("a fill with a surface piece must be refused, not written"));
        assert!(e.contains("worktop"), "the refusal must name the surface it wants: {e}");
    }

    /// A dragged box lays exactly the cells it covers, once each.
    #[test]
    fn a_box_fills_the_rectangle_it_was_dragged_over() {
        let m = map((10.0, 3.0, 10.0));
        let f = box_fill(&m, &brush(1.0), ((0.2, 0.2), (2.8, 1.8)), 0.0, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        // Cells are 1 m and the box spans x 0.2..2.8, z 0.2..1.8 — three columns by two rows.
        assert_eq!(f.placements.len(), 6, "{:?}", f.placements);
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

    /// **The box drawn IS the box committed.** A cell is filled only when its centre lies inside the
    /// dragged rectangle — the same containment rule the removal box applies to placements. The
    /// corner arithmetic alone would include every cell a corner merely clips, laying pieces whose
    /// centres sit up to half a pitch outside the outline the author watched themselves draw.
    #[test]
    fn a_box_lays_only_cells_whose_centres_it_contains() {
        let m = map((10.0, 3.0, 10.0));
        // Clips four cells it does not contain: x centres are 0.5 (out), 1.5 (in), 2.5 (out).
        let f = box_fill(&m, &brush(1.0), ((0.8, 0.2), (2.2, 1.8)), 0.0, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        let ats: Vec<_> = f.placements.iter().map(|p| p.at).collect();
        assert_eq!(ats, vec![(1.5, 0.5), (1.5, 1.5)], "{ats:?}");
    }

    /// **Corner order does not matter.** An author dragging bottom-right to top-left gets the same
    /// box as one dragging the other way, and the ids come out in the same order either way.
    #[test]
    fn a_box_is_the_same_box_dragged_from_any_corner() {
        let m = map((10.0, 3.0, 10.0));
        let a = box_fill(&m, &brush(1.0), ((0.2, 0.2), (2.8, 1.8)), 0.0, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        let b = box_fill(&m, &brush(1.0), ((2.8, 1.8), (0.2, 0.2)), 0.0, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        let ats = |f: &Filled| f.placements.iter().map(|p| p.at).collect::<Vec<_>>();
        assert_eq!(ats(&a), ats(&b));
        let names = |f: &Filled| f.placements.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
        assert_eq!(names(&a), names(&b), "ids must not depend on the drag direction");
    }

    /// **A floor goes under the furniture.** A cell holding something else is not spoken for — the
    /// click path has always allowed it, and a fill that refused would answer a stricter question
    /// than a click on the same spot. This is where `box_fill` parts company with `flood`, which
    /// stops dead at an occupied cell because stopping is what a flood's edge means.
    #[test]
    fn a_box_fills_under_what_is_already_there() {
        let mut m = map((10.0, 3.0, 10.0));
        m.placements.push(Placed {
            id: "crate1".into(),
            descriptor: "crate".into(),
            at: (0.5, 0.5),
            ..Placed::default()
        });
        let f = box_fill(&m, &brush(1.0), ((0.2, 0.2), (2.8, 1.8)), 0.0, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.placements.len(), 6, "every cell in the box, the crate's included");
        assert!(
            f.placements.iter().any(|p| p.at == (0.5, 0.5)),
            "the floor must reach under the crate"
        );
    }

    /// **Re-dragging the same floor lays nothing twice.** Not a rule about space — a duplicate tile
    /// inside the first is invisible except as doubled triangles, and catching a missed corner with a
    /// second drag is a thing authors do.
    #[test]
    fn filling_the_same_area_twice_with_the_same_piece_adds_nothing() {
        let mut m = map((10.0, 3.0, 10.0));
        let b = brush(1.0);
        let first = box_fill(&m, &b, ((0.2, 0.2), (2.8, 1.8)), 0.0, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        m.placements.extend(first.placements);

        let again = box_fill(&m, &b, ((0.2, 0.2), (2.8, 1.8)), 0.0, ids());
        assert!(again.is_err(), "a second identical drag has nothing left to lay");

        // But a DIFFERENT piece still fills the same cells — it is the descriptor that repeats, not
        // the cell that is taken.
        let mut other = brush(1.0);
        other.id = "rug".into();
        let over = box_fill(&m, &other, ((0.2, 0.2), (2.8, 1.8)), 0.0, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(over.placements.len(), 6);
    }

    /// The same refusal `flood` makes, for the same reason — the box gesture must not be a second way
    /// to write thousands of undrawable rows.
    #[test]
    fn a_surface_piece_cannot_be_box_filled_with_either() {
        use emerge_core::descriptor::Mount;
        let lamp = Descriptor {
            id: "lamp_tall".into(),
            mount: Some(Mount::OnSurface {
                class: "worktop".into(),
            }),
            ..brush(0.5)
        };
        let m = map((4.0, 3.0, 4.0));
        let e = box_fill(&m, &lamp, ((0.2, 0.2), (1.8, 1.8)), 0.0, ids())
            .err()
            .unwrap_or_else(|| panic!("must be refused, not written"));
        assert!(e.contains("worktop"), "{e}");
    }

    /// A box drawn entirely off the map says so rather than silently placing nothing.
    #[test]
    fn a_box_outside_the_map_is_refused_by_name() {
        let m = map((4.0, 3.0, 4.0));
        let e = box_fill(&m, &brush(1.0), ((50.0, 50.0), (60.0, 60.0)), 0.0, ids())
            .err()
            .unwrap_or_else(|| panic!("must be refused"));
        assert!(e.contains("outside the map"), "{e}");
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
