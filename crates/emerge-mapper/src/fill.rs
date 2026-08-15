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

/// **The ground a box fill will cover**, from the two anchors a drag runs between.
///
/// A drag is recorded as two *placement anchors* — where a piece's centre lands — and each piece
/// laid between them covers its own [`cell_extents`] around that centre. So the region is the
/// anchors grown by half a cell on every side, and a rectangle drawn anchor-to-anchor is half a
/// cell short all round: on a cell-sized brush its corners sit at the **centres** of the end cells
/// rather than on the cells' corners.
///
/// Reported from the keyboard, 2026-08-14: *"the yellowish orange selection... falls in the center
/// of each tile. I would expect it to fall on the corners of a tile."* The removal tool had already
/// written the right answer for the single-piece case — it outlines `cell_extents` around the piece
/// under the cursor — so this is that same rule, applied to the case that was drawing anchors.
///
/// Returns `(x0, z0, x1, z1)`, min corner first.
pub fn covered_rect(from: (f32, f32), at: (f32, f32), extents: (f32, f32)) -> (f32, f32, f32, f32) {
    let (hx, hz) = (extents.0 * 0.5, extents.1 * 0.5);
    (
        from.0.min(at.0) - hx,
        from.1.min(at.1) - hz,
        from.0.max(at.0) + hx,
        from.1.max(at.1) + hz,
    )
}

/// The (x, z) step this brush fills on at `yaw_deg`, tipped by `tip`, rounded to the authoring snap.
///
/// A footprint is recorded before rotation, so a piece turned 90° or 270° presents its depth along X.
/// Anything in between does not tile at all — a 30°-rotated rectangle has no grid that covers the
/// plane — so those round to the nearer quarter turn, which is the yaw a fill is actually useful at.
///
/// # The tip is the same question asked about the other two axes
///
/// [`emerge_core::map::Placed::tip`]'s own doc names this function as where the axis-swap lesson is
/// recorded — and for a long time it recorded only half of it, swapping X against Z for yaw and
/// ignoring the tip entirely. A tipped piece lies down: its height becomes one of its floor
/// dimensions, so a 2.40 m wall tipped a quarter turn covers 2.40 m of floor and a lattice built
/// from its standing footprint puts every row inside the last. Both turns are quarter turns for the
/// same reason, so both belong in one answer.
pub fn cell_extents(d: &Descriptor, yaw_deg: f32, tip: (u8, u8)) -> (f32, f32) {
    // **The placed footprint** — `stack::covers` reserves that box, so a fill stepping on the measured
    // one would lay a scaled piece down at a pitch its own reservation disagrees with.
    let (w, depth) =
        emerge_core::descriptor::tipped_footprint(d, tip).unwrap_or((MIN_CELL, MIN_CELL));
    let quarter = (yaw_deg.rem_euclid(360.0) / 90.0).round() as i32 % 4;
    let (x, z) = if quarter % 2 == 1 {
        (depth, w)
    } else {
        (w, depth)
    };
    (snap_cell(x), snap_cell(z))
}

fn snap_cell(v: f32) -> f32 {
    let raw = v.max(MIN_CELL);
    (raw / MIN_CELL).round() * MIN_CELL
}

/// **The lattice a fill steps on — the one a click lands on.**
///
/// Cell `k` sits at `k * step + phase`, and `phase` is half the brush's turned footprint, because
/// that is exactly what [`emerge_core::grid::snap_corner`] leaves behind: it puts a piece's minimum
/// corner on a multiple of the pitch, so its centre is that multiple plus half its span. Inverting it
/// is `round`, not `floor`, for the same reason `snap_corner` rounds — the cell you are in is the one
/// whose landing you are nearest.
///
/// # This used to be the brush's own size, and that made a third lattice
///
/// [`cell_extents`] rounds a footprint up to [`MIN_CELL`] and steps by it, which gave a fill a pitch
/// of its own. Measured on `site/wall`, footprint `0.1 x 1.0`: a fill clamped the thin axis up to a
/// 0.5 m step and laid pieces at `k*0.5 + 0.25`; a single click landed the same wall at `k*1.0 +
/// 0.05`; and the ghost — before it was fixed — previewed a third place again. **Three answers to
/// "where does this piece go", chosen by which gesture you used.**
///
/// [`cell_extents`] stays, and is not this: five markers draw a piece's *occupied box* with it, which
/// is a question about the piece rather than about the grid.
///
/// # An unmeasured brush centre-snaps, exactly as a click does
///
/// `brush_span` answers `(0, 0)` for a piece with no footprint, so `phase` is zero and cells fall on
/// the bare pitch — the same honest answer `map_at` gives when nothing knows how big the thing is.
///
/// # A fill steps by what the piece OCCUPIES, in whole pitches
///
/// Stepping by one pitch would be wrong in the other direction: a 2 m piece on a 1 m rung would lay
/// copies a metre apart, each one buried half inside the last. So the step is
/// `pitch * ceil(span / pitch)` — the whole number of rungs the piece actually takes up, never zero.
///
/// Every position this produces is a position a click can also land on, because a multiple of
/// `n * pitch` is a multiple of `pitch`. That is the property being bought: the fill is a *subset* of
/// the click's lattice, not a second one. A 1 m tile on the tile rung steps 1 m and abuts, exactly as
/// it always did; a 0.1 m wall steps 1 m and lands at `k + 0.05`, which is where clicking it lands;
/// a 0.5 m bench steps 1 m and leaves a gap, which is *also* what clicking it twice does — and
/// dropping a rung closes the gap, which is what the rungs are for.
struct Lattice {
    step: (f32, f32),
    phase: (f32, f32),
}

impl Lattice {
    /// Refuses a pitch that cannot index, rather than clamping to one that can. A zero or negative
    /// step is a flood fill that never terminates, and inventing a step here would put pieces at a
    /// spacing the caller did not ask for — the failure mode [`MIN_CELL`] exists to prevent for
    /// footprints, made loud instead of silent because a rung is not a measurement.
    fn new(d: &Descriptor, yaw_deg: f32, tip: (u8, u8), pitch: f32) -> Result<Self, String> {
        if !(pitch.is_finite() && pitch > 0.0) {
            return Err(format!(
                "a fill needs a positive step and the lattice offered {pitch}; nothing was placed"
            ));
        }
        let (w, depth) = crate::editor::brush_span(d, yaw_deg, tip);
        // `max(1.0)` covers both an unmeasured piece — `brush_span` answers `(0, 0)`, so it occupies
        // one rung, the same centre-snapping a click gives it — and a piece finer than the rung.
        let rungs = |span: f32| (span / pitch).ceil().max(1.0) * pitch;
        Ok(Lattice {
            step: (rungs(w), rungs(depth)),
            phase: (w * 0.5, depth * 0.5),
        })
    }

    /// Which cell a world point belongs to. Integer, so `seen` membership is exact — comparing floats
    /// for "have I been here" is how a flood fill revisits a cell forever.
    fn cell(&self, p: (f32, f32)) -> (i64, i64) {
        (
            ((p.0 - self.phase.0) / self.step.0).round() as i64,
            ((p.1 - self.phase.1) / self.step.1).round() as i64,
        )
    }

    /// Where a piece in that cell lands — a point [`emerge_core::grid::snap_corner`] would also answer.
    fn centre(&self, c: (i64, i64)) -> (f32, f32) {
        (
            c.0 as f32 * self.step.0 + self.phase.0,
            c.1 as f32 * self.step.1 + self.phase.1,
        )
    }
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
    tip: (u8, u8),
    pitch: f32,
    mut next_id: impl FnMut() -> String,
) -> Result<Filled, String> {
    // **A fill covers open floor, so a piece that mounts on something cannot be filled with.**
    // `stack::placement_at` already refuses such a piece cell by cell — that is the rule the click
    // path obeys — but the fill never asked, so it wrote one placement per cell that could never rest
    // anywhere and was never drawn. A measured run left **4,089 invisible lamps** in the map, all of
    // them saveable, none of them on screen: the file and the screen disagreeing, which is the exact
    // failure the one-path rule exists to prevent. Refused here, in the same shape as the other two
    // refusals, so there is one answer to "may this piece go here" rather than two.
    //
    // Through [`crate::editor::mount_class`], which asks about **every** mount that needs a host.
    // This guard used to name `stack::needs_surface` directly and so knew about exactly one of them:
    // the day `Mount::OnFace` arrived, a wall-mounted sconce became flood-fillable and the 4,089
    // lamps were back under a different mount kind.
    if let Some(class) = crate::editor::mount_class(brush) {
        return Err(format!(
            "`{}` mounts to a `{class}`, so it cannot be flood filled — a fill covers open \
             floor and none of these cells offers one. Place it on its host instead.",
            brush.id
        ));
    }

    // **The click's lattice, not one of its own.** See [`Lattice`] for the three-answer defect this
    // replaced.
    let lat = Lattice::new(brush, yaw, tip, pitch)?;
    // From the map, never re-derived here: `floor_rect` owns the centre-on-origin convention.
    let (min_x, min_z, max_x, max_z) = map.floor_rect();

    let to_cell = |p: (f32, f32)| -> (i64, i64) { lat.cell(p) };
    let centre_of = |c: (i64, i64)| -> (f32, f32) { lat.centre(c) };

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
            tip,
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
    tip: (u8, u8),
    pitch: f32,
    mut next_id: impl FnMut() -> String,
) -> Result<Filled, String> {
    // The same refusal `flood` makes, through the same [`crate::editor::mount_class`], and for the
    // same measured reason: filling with a hosted piece once wrote 4,089 invisible lamps into a map,
    // all saveable, none drawable.
    if let Some(class) = crate::editor::mount_class(brush) {
        return Err(format!(
            "`{}` mounts to a `{class}`, so it cannot be box filled — a fill covers open floor \
             and none of these cells offers one. Place it on its host instead.",
            brush.id
        ));
    }

    // **The click's lattice**, the same one [`flood`] steps on — see [`Lattice`].
    let lat = Lattice::new(brush, yaw, tip, pitch)?;
    let (min_x, min_z, max_x, max_z) = map.floor_rect();
    let (x0, z0) = (corners.0.0.min(corners.1.0), corners.0.1.min(corners.1.1));
    let (x1, z1) = (corners.0.0.max(corners.1.0), corners.0.1.max(corners.1.1));

    let to_cell = |p: (f32, f32)| -> (i64, i64) { lat.cell(p) };
    let centre_of = |c: (i64, i64)| -> (f32, f32) { lat.centre(c) };

    // Only this brush's own cells, so a re-drag is idempotent. Everything else is filled under.
    let mine: std::collections::HashSet<(i64, i64)> = map
        .placements
        .iter()
        .filter(|p| p.descriptor == brush.id)
        .map(|p| to_cell(p.at))
        .collect();

    // **Generous by one cell each way, deliberately.** The old range leaned on `floor` including every
    // cell a corner merely clips; the lattice rounds, so a corner can round *inward* and drop the
    // edge row. Widening costs a discarded iteration and the real filter is the centre-inside test
    // below — which is what keeps "the box drawn IS the box committed" true either way.
    let (cx0, cz0) = to_cell((x0, z0));
    let (cx1, cz1) = to_cell((x1, z1));
    let (cx0, cz0) = (cx0 - 1, cz0 - 1);
    let (cx1, cz1) = (cx1 + 1, cz1 + 1);
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
                tip,
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

    /// **The fill outline lands on cell corners, not cell centres.**
    ///
    /// The drag box was drawn anchor-to-anchor while the fill covers each anchor's whole cell, so
    /// the preview was half a cell short on every side — reported from the keyboard, 2026-08-14,
    /// as the box falling *"in the center of each tile"*. A one-cell brush dragged from the middle
    /// of one cell to the middle of the cell two over covers three whole cells, corner to corner.
    #[test]
    fn a_fill_box_outlines_whole_cells() {
        // Anchors at cell centres on a 1 m grid: (0.5, 0.5) to (2.5, 0.5).
        let (x0, z0, x1, z1) = covered_rect((0.5, 0.5), (2.5, 0.5), (1.0, 1.0));
        assert_eq!(
            (x0, z0, x1, z1),
            (0.0, 0.0, 3.0, 1.0),
            "three cells, corner to corner"
        );

        // Direction cannot matter: a drag right-to-left covers the same ground.
        assert_eq!(
            covered_rect((2.5, 0.5), (0.5, 0.5), (1.0, 1.0)),
            (0.0, 0.0, 3.0, 1.0)
        );

        // A drag that never leaves one cell still outlines that whole cell, never a zero-area box.
        assert_eq!(
            covered_rect((0.5, 0.5), (0.5, 0.5), (1.0, 1.0)),
            (0.0, 0.0, 1.0, 1.0)
        );

        // A non-square brush grows by its own half-extents on each axis, not by a cell.
        assert_eq!(
            covered_rect((1.0, 1.0), (1.0, 1.0), (2.0, 0.5)),
            (0.0, 0.75, 2.0, 1.25)
        );
    }
    use emerge_core::descriptor::Extent;
    /// The tile rung, which is what every fill in these tests is driven at — the rung an author is on
    /// unless they are holding a modifier, and the only one a cell-sized piece may use.
    use emerge_core::grid::TILE;

    /// A brush that has not been tipped — the state every test below but the tipping ones is about.
    /// Spelled rather than defaulted so a test that *means* upright and one that merely forgot to
    /// say are different text.
    const UPRIGHT: (u8, u8) = (0, 0);

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

    /// A brush whose three dimensions are all different, so a tip that permutes the wrong pair is
    /// visible instead of coincidentally right. `brush` is a square 0.1 m slab and would hide it.
    fn slab(w: f32, depth: f32, height: f32) -> Descriptor {
        Descriptor {
            id: "wall".into(),
            extent: Extent {
                footprint: Some((w, depth)),
                height: Some(height),
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
        let e = flood(&m, &lamp, (0.5, 0.5), 0.0, UPRIGHT, TILE, ids())
            .err()
            .unwrap_or_else(|| panic!("a fill with a surface piece must be refused, not written"));
        assert!(
            e.contains("worktop"),
            "the refusal must name the surface it wants: {e}"
        );
    }

    /// A dragged box lays exactly the cells it covers, once each.
    #[test]
    fn a_box_fills_the_rectangle_it_was_dragged_over() {
        let m = map((10.0, 3.0, 10.0));
        let f = box_fill(
            &m,
            &brush(1.0),
            ((0.2, 0.2), (2.8, 1.8)),
            0.0,
            UPRIGHT,
            TILE,
            ids(),
        )
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
        let f = box_fill(
            &m,
            &brush(1.0),
            ((0.8, 0.2), (2.2, 1.8)),
            0.0,
            UPRIGHT,
            TILE,
            ids(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let ats: Vec<_> = f.placements.iter().map(|p| p.at).collect();
        assert_eq!(ats, vec![(1.5, 0.5), (1.5, 1.5)], "{ats:?}");
    }

    /// **Corner order does not matter.** An author dragging bottom-right to top-left gets the same
    /// box as one dragging the other way, and the ids come out in the same order either way.
    #[test]
    fn a_box_is_the_same_box_dragged_from_any_corner() {
        let m = map((10.0, 3.0, 10.0));
        let a = box_fill(
            &m,
            &brush(1.0),
            ((0.2, 0.2), (2.8, 1.8)),
            0.0,
            UPRIGHT,
            TILE,
            ids(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let b = box_fill(
            &m,
            &brush(1.0),
            ((2.8, 1.8), (0.2, 0.2)),
            0.0,
            UPRIGHT,
            TILE,
            ids(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let ats = |f: &Filled| f.placements.iter().map(|p| p.at).collect::<Vec<_>>();
        assert_eq!(ats(&a), ats(&b));
        let names = |f: &Filled| {
            f.placements
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&a),
            names(&b),
            "ids must not depend on the drag direction"
        );
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
        let f = box_fill(
            &m,
            &brush(1.0),
            ((0.2, 0.2), (2.8, 1.8)),
            0.0,
            UPRIGHT,
            TILE,
            ids(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            f.placements.len(),
            6,
            "every cell in the box, the crate's included"
        );
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
        let first = box_fill(&m, &b, ((0.2, 0.2), (2.8, 1.8)), 0.0, UPRIGHT, TILE, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        m.placements.extend(first.placements);

        let again = box_fill(&m, &b, ((0.2, 0.2), (2.8, 1.8)), 0.0, UPRIGHT, TILE, ids());
        assert!(
            again.is_err(),
            "a second identical drag has nothing left to lay"
        );

        // But a DIFFERENT piece still fills the same cells — it is the descriptor that repeats, not
        // the cell that is taken.
        let mut other = brush(1.0);
        other.id = "rug".into();
        let over = box_fill(
            &m,
            &other,
            ((0.2, 0.2), (2.8, 1.8)),
            0.0,
            UPRIGHT,
            TILE,
            ids(),
        )
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
        let e = box_fill(
            &m,
            &lamp,
            ((0.2, 0.2), (1.8, 1.8)),
            0.0,
            UPRIGHT,
            TILE,
            ids(),
        )
        .err()
        .unwrap_or_else(|| panic!("must be refused, not written"));
        assert!(e.contains("worktop"), "{e}");
    }

    /// A box drawn entirely off the map says so rather than silently placing nothing.
    #[test]
    fn a_box_outside_the_map_is_refused_by_name() {
        let m = map((4.0, 3.0, 4.0));
        let e = box_fill(
            &m,
            &brush(1.0),
            ((50.0, 50.0), (60.0, 60.0)),
            0.0,
            UPRIGHT,
            TILE,
            ids(),
        )
        .err()
        .unwrap_or_else(|| panic!("must be refused"));
        assert!(e.contains("outside the map"), "{e}");
    }

    #[test]
    fn a_fill_covers_the_map_exactly_once_per_cell() {
        let m = map((4.0, 3.0, 4.0));
        let f = flood(&m, &brush(1.0), (0.5, 0.5), 0.0, UPRIGHT, TILE, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            f.placements.len(),
            16,
            "4x4 m centred on the origin, at 1 m cells"
        );
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
        let f = flood(&m, &brush(2.0), (1.0, 1.0), 0.0, UPRIGHT, TILE, ids())
            .unwrap_or_else(|e| panic!("{e}"));
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
        let f = flood(&m, &brush(1.0), (-2.5, -0.5), 0.0, UPRIGHT, TILE, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.placements.len(), 3, "should fill only up to the wall");
        for p in &f.placements {
            assert!(p.at.0 < 0.5, "leaked past the wall to {:?}", p.at);
        }
    }

    #[test]
    fn a_fill_outside_the_map_is_refused_not_clamped() {
        let m = map((4.0, 3.0, 4.0));
        let err = flood(&m, &brush(1.0), (99.0, 99.0), 0.0, UPRIGHT, TILE, ids())
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
        assert!(
            flood(&m, &brush(1.0), (0.5, 0.5), 0.0, UPRIGHT, TILE, ids())
                .err()
                .unwrap_or_default()
                .contains("already something here")
        );
    }

    /// A silent cap is worse than no cap: a fill that stopped early looks exactly like one that
    /// finished, so the caller has to be able to say so.
    #[test]
    fn a_huge_region_truncates_and_says_so() {
        let m = map((200.0, 3.0, 200.0));
        let f = flood(&m, &brush(0.5), (0.25, 0.25), 0.0, UPRIGHT, TILE, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.placements.len(), MAX_CELLS);
        assert!(f.truncated, "must report that it stopped short");
    }

    /// **The axes step independently**, which is the striping guard: a first version used the larger
    /// dimension on both, and a fill of 1.5 x 0.5 m drawers came out in rows with bare floor between
    /// them.
    ///
    /// The numbers moved when the fill joined the click's lattice, and the reason is worth stating
    /// rather than re-fitting. A 2.0 x 0.5 piece on the tile rung steps **2.0 along X** — two whole
    /// rungs, abutting exactly as before — and **1.0 along Z**, because half a metre is finer than
    /// the rung and a fill may not land where a click cannot. So Z gains a 0.5 m gap, which is
    /// precisely what clicking the same piece twice has always produced. It is the rung's doing, not
    /// the fill's, and dropping a rung closes it.
    #[test]
    fn a_rectangular_piece_tiles_instead_of_striping() {
        let mut d = brush(1.0);
        d.extent.footprint = Some((2.0, 0.5));
        // `cell_extents` is the marker-drawing helper, not the fill's lattice — it still answers the
        // piece's own occupied box, and still swaps axes on a quarter turn.
        assert_eq!(cell_extents(&d, 0.0, UPRIGHT), (2.0, 0.5));
        assert_eq!(cell_extents(&d, 90.0, UPRIGHT), (0.5, 2.0));
        assert_eq!(cell_extents(&d, 270.0, UPRIGHT), (0.5, 2.0));
        assert_eq!(cell_extents(&d, 180.0, UPRIGHT), (2.0, 0.5));

        // 4 x 4 m: X steps 2.0 so two columns fit; Z steps 1.0 so four rows do.
        let f = flood(
            &map((4.0, 3.0, 4.0)),
            &d,
            (-1.0, -1.75),
            0.0,
            UPRIGHT,
            TILE,
            ids(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.placements.len(), 8);
        let xs: HashSet<u32> = f.placements.iter().map(|p| p.at.0.to_bits()).collect();
        let zs: HashSet<u32> = f.placements.iter().map(|p| p.at.1.to_bits()).collect();
        assert_eq!(
            (xs.len(), zs.len()),
            (2, 4),
            "two columns of four, not one stripe"
        );

        // **And the gap is the rung, not the fill.** On a kit that divides by halves the middle rung
        // is 0.5, the piece is exactly one rung deep, and Z abuts — eight rows in the same map.
        let f = flood(
            &map((4.0, 3.0, 4.0)),
            &d,
            (-1.0, -1.75),
            0.0,
            UPRIGHT,
            0.5,
            ids(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let zs: HashSet<u32> = f.placements.iter().map(|p| p.at.1.to_bits()).collect();
        assert_eq!(zs.len(), 8, "at a 0.5 m rung a 0.5 m piece abuts");
    }

    /// **An unmeasured piece still steps**, rather than looping forever on a zero-width cell — which
    /// is the whole reason this test exists.
    ///
    /// It no longer steps at [`MIN_CELL`]. `brush_span` answers `(0, 0)` for a piece nothing has
    /// measured, so the fill centre-snaps it on the bare rung — the same honest answer `map_at` gives
    /// a click on the same piece. Inventing a 0.5 m box for something nobody has measured was the old
    /// behaviour, and it put a fill on a pitch no click could reach.
    #[test]
    fn a_piece_with_no_footprint_still_steps_and_lands_where_a_click_would() {
        let d = Descriptor {
            id: "mystery".into(),
            ..Descriptor::default()
        };
        // The marker helper still falls back to a drawable box; that is a different question.
        assert_eq!(cell_extents(&d, 0.0, UPRIGHT), (MIN_CELL, MIN_CELL));

        let f = flood(
            &map((2.0, 3.0, 1.0)),
            &d,
            (-0.75, -0.25),
            0.0,
            UPRIGHT,
            TILE,
            ids(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        // A 2 x 1 m map on the tile rung: x centres −1 and 0 are inside, z centre 0 is.
        assert_eq!(f.placements.len(), 2, "{:?}", f.placements);
        for p in &f.placements {
            assert_eq!(p.at.1, 0.0);
            assert!(
                p.at.0 == -1.0 || p.at.0 == 0.0,
                "{:?} is off the rung",
                p.at
            );
        }
    }

    /// **A rung that cannot index is refused, not repaired.** A zero step is a flood that never
    /// terminates; clamping it to something workable would place pieces at a spacing nobody asked
    /// for, which is the silent-substitute failure the one-path rule exists to prevent.
    #[test]
    fn a_degenerate_rung_is_refused_by_name() {
        let m = map((4.0, 3.0, 4.0));
        for bad in [0.0, -1.0, f32::NAN] {
            let e = flood(&m, &brush(1.0), (0.5, 0.5), 0.0, UPRIGHT, bad, ids())
                .err()
                .unwrap_or_else(|| panic!("a {bad} step must be refused, not filled"));
            assert!(e.contains("positive step"), "{e}");
        }
    }

    /// **A fill lays the brush the way the author is holding it** — tip included.
    ///
    /// Both fills built their `Placed` with `..Placed::default()` after naming `yaw`, so `tip` came
    /// out `(0, 0)` however the brush was tipped. The single-click path has always passed
    /// `state.brush_tip` (`editor.rs`, beside the `stack::blocking` call that asks about the tipped
    /// piece), so the same brush on the same cell landed **tipped by click and upright by drag** —
    /// one piece, one spot, two answers chosen by the gesture, which is the disagreement
    /// `CLAUDE.md`'s one-path rule exists to prevent and which `box_fill`'s own doc comment already
    /// claims not to have.
    ///
    /// Recorded in `docs/2026-08-15-blank-slate-handoff.md` §6 as `box_fill` only. It was both.
    #[test]
    fn a_fill_lays_the_brush_tipped_the_way_the_author_tipped_it() {
        let m = map((4.0, 3.0, 4.0));
        let laid = slab(1.0, 0.5, 2.0);
        let tip = (1, 0);

        let boxed = box_fill(&m, &laid, ((0.0, 0.0), (4.0, 4.0)), 0.0, tip, TILE, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            !boxed.placements.is_empty(),
            "the box must fill something to be evidence"
        );
        for p in &boxed.placements {
            assert_eq!(p.tip, tip, "`{}` came out of a box fill standing up", p.id);
        }

        let flooded =
            flood(&m, &laid, (0.5, 1.0), 0.0, tip, TILE, ids()).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            !flooded.placements.is_empty(),
            "the flood must fill something to be evidence"
        );
        for p in &flooded.placements {
            assert_eq!(
                p.tip, tip,
                "`{}` came out of a flood fill standing up",
                p.id
            );
        }
    }

    /// **A tipped brush fills on the footprint it actually presents**, not the one it had standing.
    ///
    /// The half a `Placed::tip` assertion cannot see. A tipped piece lies down, so its height
    /// becomes one of its floor dimensions — and the lattice was still built from
    /// `placed_footprint`, which is the standing box. The rows then come out at the upright pitch
    /// and each one is laid partly **inside** the last: a fill that looks right in the file, wrong
    /// on screen, and whose overlap only shows up as triangles.
    ///
    /// `Placed::tip`'s own doc names `fill::cell_extents` as where the axis-swap lesson is recorded.
    /// It recorded half of it — the yaw half.
    #[test]
    fn a_tipped_brush_fills_on_the_footprint_it_actually_presents() {
        // 2.0 wide, 0.5 deep, 1.0 tall — three different numbers, so a wrong permutation shows.
        let d = slab(2.0, 0.5, 1.0);

        assert_eq!(
            cell_extents(&d, 0.0, UPRIGHT),
            (2.0, 0.5),
            "standing: its own footprint"
        );
        // Tipped about X, the depth and the height trade places: 0.5 deep becomes 1.0 deep.
        assert_eq!(cell_extents(&d, 0.0, (1, 0)), (2.0, 1.0));
        // Tipped about Z, the width and the height trade: 2.0 wide becomes 1.0 wide.
        assert_eq!(cell_extents(&d, 0.0, (0, 1)), (1.0, 0.5));
        // A half turn changes which face is down and not the box — the same rule `tipped_extents`
        // states, checked here because a fill would otherwise re-derive it.
        assert_eq!(cell_extents(&d, 0.0, (2, 0)), (2.0, 0.5));
        // And the two turns compose: yaw still swaps X against Z after the tip has been applied.
        assert_eq!(cell_extents(&d, 90.0, (1, 0)), (1.0, 2.0));

        // **An upright piece never consults a height it does not have.** Routing the untipped case
        // through `tipped_extents` would make this unmeasured and centre-snap every heightless
        // floor tile in the kit — a regression with no error message.
        let heightless = Descriptor {
            extent: Extent {
                footprint: Some((2.0, 0.5)),
                height: None,
            },
            ..slab(2.0, 0.5, 1.0)
        };
        assert_eq!(
            cell_extents(&heightless, 0.0, UPRIGHT),
            (2.0, 0.5),
            "a piece standing up covers its footprint whatever its height"
        );

        // **The lattice moves with it**, which is the part that decides where rows land. A 1.0 x 0.5
        // brush 2.0 m tall steps one tile in Z standing; tipped about X it is 2.0 m deep and steps
        // two, so the same drag over the same map lays half as many rows — none of them overlapping.
        let m = map((4.0, 3.0, 4.0));
        let laid = slab(1.0, 0.5, 2.0);
        // Map space is centred on the origin — `Map::floor_rect` is `(-hx, -hz, hx, hz)` — so the
        // whole floor of a 4 x 4 map is this box and not one anchored at zero.
        let whole_map = ((-2.0, -2.0), (2.0, 2.0));
        let standing = box_fill(&m, &laid, whole_map, 0.0, UPRIGHT, TILE, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        let tipped = box_fill(&m, &laid, whole_map, 0.0, (1, 0), TILE, ids())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            standing.placements.len(),
            16,
            "four rows of four at the upright pitch"
        );
        assert_eq!(
            tipped.placements.len(),
            8,
            "tipped it is 2 m deep, so two rows of four — at the standing pitch it would lay four \
             rows, each buried half inside the one before it"
        );

        // Rows two metres apart, not one: the spacing is the claim, the count is its shadow.
        let rows = |f: &Filled| {
            let mut zs: Vec<f32> = f.placements.iter().map(|p| p.at.1).collect();
            zs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            zs.dedup();
            zs
        };
        assert_eq!(
            rows(&standing),
            vec![-1.75, -0.75, 0.25, 1.25],
            "the upright 0.5 m pitch"
        );
        assert_eq!(
            rows(&tipped),
            vec![-1.0, 1.0],
            "one whole tipped depth between rows"
        );
    }
}
