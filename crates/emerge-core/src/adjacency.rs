//! **Do the tiles the author placed agree with the tokens the author declared?**
//!
//! [`Subgrid::edge`](crate::descriptor::SubCell::edge) is what a tile presents to whatever sits
//! beside it. This module is the one thing that reads it: a comparison, [`seam`], and one caller,
//! [`faults`], which walks a finished map and reports every abutting pair whose facing tokens
//! disagree.
//!
//! # Why it reports rather than generates
//!
//! `crate::grammar` already does tile adjacency, and its module doc argues it should be the only way:
//! *"inventing an adjacency schema would mean asking an author to write down a grammar before they
//! are allowed to draw one."* That argument stands, so this does not become a second solver. It
//! answers a different question — **not** "what may go here", which the learned grammar answers from
//! the map, but "does what you drew match what you declared", which nothing could answer before.
//!
//! The two are not rivals, and Karth & Smith 2017 §"Patterns" names them as the algorithm's own two
//! modes: *"In the **simple tiled** version of the algorithm, the patterns are specified as explicit
//! tile constraint relationships. In the overlapping version, the constraints are **inferred from the
//! source image**."* `grammar.rs` is the inferred half. `edge` is the explicit half — and whether it
//! should also feed the solver's support table, rather than only checking it, is `FVS-Q-10`.
//!
//! Tokens are intent; the map is behaviour. A fault is the two disagreeing, and naming which face and
//! which tokens is the whole product. PCG Book ch11 §11.2.3 puts it plainly: a designer *"may become
//! frustrated or confused if the computer consistently acts as though it is not following the model
//! that the human designer has in her head"*. Reporting is also the only one of that chapter's three
//! conflict responses — error, arbitrary pick, or offer alternatives — that satisfies this project's
//! rule that a program fails loudly rather than repairing input behind the author's back.
//!
//! # Matching is equality
//!
//! Merrell & Manocha 2009, *Constraint-Based Model Synthesis* §4.3 — the paper WaveFunctionCollapse
//! descends from — defines adjacency as *"evaluating two states in opposite directions and checking
//! if their evaluations are identical"*. Equality of the facing descriptor, not a compatibility
//! table: a table is a second artifact that can drift from the tokens it is about.
//!
//! An unlabelled cell equals only another unlabelled cell. That is not a wildcard, and the difference
//! matters: a wildcard would make unauthored data permissive and authored data strict, which is one
//! function with two behaviours. Equality gives the useful property for free — with every lattice
//! empty, every face is all-`None`, every pair matches, and `faults` returns nothing. The feature is
//! inert until it is authored, **without a branch anywhere that makes it inert**.
//!
//! # This does not move `snapshot_hash`
//!
//! No RNG, no placement change, no write. It reads a map and returns a list.

use crate::descriptor::Subgrid;
use crate::library::Library;
use crate::map::Map;
use crate::placement::ir::Dir;
use crate::wfc::{E, N, S, W};

/// How close a yaw must be to a quarter turn to count as one, in degrees.
///
/// Yaws are authored in 15° steps (`emerge-mapper`'s `YAW_STEP`) and stored as `f32`, so a value that
/// went through a serialize/parse round trip can be a hair off. A thousandth of a degree is far below
/// anything an author can express and far above `f32`'s error at these magnitudes.
const SQUARE_EPSILON: f32 = 1e-3;

/// How far inside a cell boundary a piece's edge may land and still count as on it, as a fraction of
/// a cell.
///
/// A 3 m piece centred on the grid has edges that are a hair under or over a whole number of cells
/// depending on where it sits, and a seam that disappears because two `f32`s rounded opposite ways is
/// the kind of fault an author cannot see the cause of. A thousandth of a cell is half a millimetre
/// at the shipped size — far below anything authored, far above `f32`'s error here.
const EDGE_EPSILON: f32 = 1e-3;

/// **Which quarter turn this yaw is**, or a refusal naming the piece and the angle.
///
/// A lattice face only exists at multiples of 90°. At 45° a tile presents a corner to its neighbour
/// and there is no column of cells to compare — so this refuses rather than rounding, because
/// rounding would silently check the wrong face and report a fault the author cannot see the cause
/// of. The editor's turn step is 15°, so off-square yaws are reachable and this refusal will fire.
pub fn quarter_turns(who: &str, yaw_deg: f32) -> Result<u8, String> {
    if !yaw_deg.is_finite() {
        return Err(format!("`{who}` has a yaw of {yaw_deg}, which is not an angle"));
    }
    let from_square = yaw_deg.rem_euclid(90.0);
    let off = from_square.min(90.0 - from_square);
    if off > SQUARE_EPSILON {
        return Err(format!(
            "`{who}` sits at yaw {yaw_deg}; a lattice face is only defined at multiples of 90, so \
             its edge tokens cannot be matched against a neighbour"
        ));
    }
    Ok((yaw_deg / 90.0).round().rem_euclid(4.0) as u8)
}

/// **The edge tokens on one face, in lattice order.**
///
/// `dir` is [`crate::wfc`]'s edge index, and its world meaning is `grammar::learn`'s step table:
/// `N` is −Z, `E` is +X, `S` is +Z, `W` is −X.
///
/// The order is `y` outer, then the axis that is not `dir`'s. A cell with no `edge` contributes
/// `None`, which is a token in its own right; see the module note.
///
/// **This reads a face; it does not compare two.** [`seam`] is the comparison, and it does not go
/// through this — two pieces of different sizes share only part of a face, so reading each in full and
/// checking them element for element is the thing that was wrong. Kept because "what does this face
/// present" is a question worth being able to ask on its own.
pub fn face(g: &Subgrid, dir: Dir, div: (u32, u32, u32)) -> Vec<Option<&str>> {
    let (dx, dy, dz) = div;
    let mut out = Vec::new();
    // A degenerate lattice has no face. `Subgrid::validate` refuses one in a file; this is reached
    // from a map, so it answers rather than panicking.
    if dx == 0 || dy == 0 || dz == 0 {
        return out;
    }
    let token = |at: (u32, u32, u32)| g.at(at).and_then(|c| c.edge.as_deref());
    for y in 0..dy {
        match dir {
            E => (0..dz).for_each(|z| out.push(token((dx - 1, y, z)))),
            W => (0..dz).for_each(|z| out.push(token((0, y, z)))),
            N => (0..dx).for_each(|x| out.push(token((x, y, 0)))),
            S => (0..dx).for_each(|x| out.push(token((x, y, dz - 1)))),
            // `Dir` is a `usize` alias, so the compiler cannot prove the four are exhaustive. A
            // direction outside N/E/S/W has no face rather than a made-up one.
            _ => return Vec::new(),
        }
    }
    out
}

/// **A piece's world extent**, and the lattice divisions that span it.
///
/// The two are quoted together because neither means anything alone: a cell index is only a position
/// once you know which span it divides.
#[derive(Clone, Copy, Debug)]
pub struct Placed3 {
    /// World `(min, max)` per axis, after yaw.
    pub x: (f32, f32),
    pub y: (f32, f32),
    pub z: (f32, f32),
    /// Divisions per axis, matching those world spans — so already turned, if the piece is.
    pub div: (u32, u32, u32),
}

/// Which cell of `n` divisions over `[lo, hi]` contains `v`.
///
/// Clamped rather than refused: callers sample the centre of a step inside the overlap, so a value
/// lands outside only through floating point, and the honest answer there is the edge cell.
fn index(v: f32, (lo, hi): (f32, f32), n: u32) -> u32 {
    if n == 0 || hi <= lo {
        return 0;
    }
    (((v - lo) / (hi - lo) * n as f32) as i64).clamp(0, n as i64 - 1) as u32
}

/// **The tokens either side of a seam presents to the other, over the part they actually share.**
///
/// # Why not whole faces
///
/// Comparing whole faces element for element — which this did until 2026-08-05 — is only meaningful
/// when the two pieces are
/// the same size. The shipped kits say otherwise in two independent places:
///
/// * A 2.40 m wall meets a **2.00 m doorway** with a 0.40 m header above it. Whole faces are five rows
///   against four, so the seam was refused even when every token on it read `wall` — and the header,
///   which supplies the fifth row, was never compared to the doorway at all because it shares its
///   `(x, z)` cells rather than abutting them.
/// * `site_greybox`'s **`wall_corner` is twice as wide** as its own wall. Not an architectural stack,
///   just two pieces of one family being different widths.
///
/// Both are the same defect: equality of whole faces asks "are these pieces the same shape" when the
/// question is "do they agree where they touch". This answers the second.
///
/// # How
///
/// The seam is a rectangle: the **lateral** overlap along the axis the seam runs on, and the
/// **vertical** overlap of the two pieces' world heights. It is sampled at the centre of each subunit
/// step and each sample is mapped into both pieces' own lattices, so pieces divided differently — or
/// offset from each other — are still read at the same physical places, in the same order.
///
/// `None` when they share no rectangle: no lateral overlap, or one piece entirely above the other.
/// That is not a seam and there is nothing to compare.
pub fn seam<'a>(
    a: &'a Subgrid,
    a_at: Placed3,
    b: &'a Subgrid,
    b_at: Placed3,
    dir: Dir,
    subunit: f32,
) -> Option<(Vec<Option<&'a str>>, Vec<Option<&'a str>>)> {
    if !(subunit.is_finite() && subunit > 0.0) {
        return None;
    }
    // The vertical overlap is shared by every seam direction.
    let y = (a_at.y.0.max(b_at.y.0), a_at.y.1.min(b_at.y.1));
    if y.1 <= y.0 {
        return None;
    }
    // The lateral axis is whichever one the seam runs along, and the face index on the other is
    // fixed by the direction: entering from the East reads `a`'s last x column and `b`'s first.
    let lateral_is_z = dir == E || dir == W;
    let (a_lat, b_lat) = if lateral_is_z {
        (a_at.z, b_at.z)
    } else {
        (a_at.x, b_at.x)
    };
    let lat = (a_lat.0.max(b_lat.0), a_lat.1.min(b_lat.1));
    if lat.1 <= lat.0 {
        return None;
    }

    let last = |n: u32| n.saturating_sub(1);
    let (a_face, b_face) = match dir {
        E => (last(a_at.div.0), 0),
        W => (0, last(b_at.div.0)),
        N => (0, last(b_at.div.2)),
        S => (last(a_at.div.2), 0),
        _ => return None,
    };

    // Steps, not divisions: the two pieces may divide this stretch differently, so the sampling rate
    // is the project's own subunit and both are read at the same physical places.
    let steps = |span: (f32, f32)| (((span.1 - span.0) / subunit).round() as u32).max(1);
    let (n_lat, n_y) = (steps(lat), steps(y));

    let mut left = Vec::with_capacity((n_lat * n_y) as usize);
    let mut right = Vec::with_capacity((n_lat * n_y) as usize);
    for iy in 0..n_y {
        let wy = y.0 + (iy as f32 + 0.5) * (y.1 - y.0) / n_y as f32;
        for il in 0..n_lat {
            let wl = lat.0 + (il as f32 + 0.5) * (lat.1 - lat.0) / n_lat as f32;
            let cell = |at: Placed3, face: u32| -> (u32, u32, u32) {
                let ay = index(wy, at.y, at.div.1);
                if lateral_is_z {
                    (face, ay, index(wl, at.z, at.div.2))
                } else {
                    (index(wl, at.x, at.div.0), ay, face)
                }
            };
            left.push(a.at(cell(a_at, a_face)).and_then(|c| c.edge.as_deref()));
            right.push(b.at(cell(b_at, b_face)).and_then(|c| c.edge.as_deref()));
        }
    }
    Some((left, right))
}

/// One disagreement between two placed tiles.
#[derive(Clone, Debug, PartialEq)]
pub struct Fault {
    /// The placement whose face is named by [`Self::dir`].
    pub a: String,
    /// The placement on `a`'s `dir` side.
    pub b: String,
    /// N, E, S or W, as [`crate::wfc`] numbers them.
    pub dir: Dir,
    /// What `a` presents on that face.
    pub a_face: Vec<Option<String>>,
    /// What `b` presents back.
    pub b_face: Vec<Option<String>>,
    /// The whole thing in a sentence, for a status line.
    pub message: String,
}

/// N, E, S, W as an author reads them.
fn dir_name(dir: Dir) -> &'static str {
    match dir {
        N => "N",
        E => "E",
        S => "S",
        W => "W",
        _ => "?",
    }
}

/// **Every abutting pair whose edge tokens disagree.**
///
/// `cell` is the grid step the map is read on — the same one `grammar::learn` is given, so the
/// validator and the generator agree about which placements are neighbours. Two placements are
/// neighbours when their cells are orthogonally adjacent.
///
/// # What it reads
///
/// The **authored** `Descriptor`, patched by the placement. `Placed::patch` replaces a lattice
/// wholesale rather than merging it (`Descriptor::patched`), so a placement that carries one is
/// standing on a different lattice than the library entry says — reading the library entry would
/// validate a tile that is not on the map. `Resolved` is no use here either: it has no `subgrid`.
///
/// # Order
///
/// Sorted on `(a, b, dir)`, which is a **total** key because `Map::validate` requires placement ids
/// to be unique — the project's determinism rule, and the reason this cannot depend on the order
/// `map.placements` happens to be in.
///
/// # Refusals are faults too
///
/// A yaw that is not a quarter turn produces a fault carrying that sentence rather than an `Err` for
/// the whole map. One unturnable piece must not blind the author to the other twelve, and the message
/// says exactly which piece and which angle.
pub fn faults(map: &Map, library: &Library, cell: f32, divisions: u32) -> Vec<Fault> {
    let mut out: Vec<Fault> = Vec::new();
    if !(cell.is_finite() && cell > 0.0) {
        return out;
    }

    // **Nothing declared, nothing to check.** The module note calls this feature inert until it is
    // authored; this is where that stops being a property of the answer and becomes a property of
    // the cost. Until an edge token exists anywhere, the whole pass is one scan of the library
    // instead of a quadratic walk of the map that clones a descriptor per placement — and the editor
    // reruns it on every placement, so a 1,400-piece flood fill was paying for it repeatedly.
    //
    // Patches are checked too: `Placed::patch` may carry a lattice the library never had.
    let declared = |g: &Option<Subgrid>| {
        g.as_ref()
            .is_some_and(|g| g.cells.iter().any(|c| c.edge.is_some()))
    };
    let any_declared = library.descriptors.iter().any(|d| declared(&d.subgrid))
        || map
            .placements
            .iter()
            .any(|p| p.patch.as_ref().is_some_and(|d| declared(&d.subgrid)));
    if !any_declared {
        return out;
    }
    let (min_x, min_z, _, _) = map.floor_rect();
    // A placement's `at` is its centre, so its extent runs half a footprint either way. Rounded
    // rather than floored/ceiled: a piece authored on the grid should occupy exactly the cells it
    // covers, and floating point leaves a 3 m span a hair under or over six cells depending on where
    // it sits. `EDGE_EPSILON` absorbs that; anything larger would merge genuinely separate pieces.
    let to_span = |at: (f32, f32), (w, d): (f32, f32)| {
        // The near edge rounds up into its cell and the far edge rounds down out of the next one, so
        // a piece whose edge lands exactly on a boundary occupies the cells it covers and not the
        // one it merely touches. Nudged in opposite directions on purpose: a single shared epsilon
        // cancels itself out and makes every piece a cell too wide, which reads as everything
        // abutting everything.
        let near = |v: f32| ((v / cell) + EDGE_EPSILON).floor() as i64;
        let far = |v: f32| ((v / cell) - EDGE_EPSILON).ceil() as i64;
        (
            near(at.0 - w * 0.5 - min_x),
            far(at.0 + w * 0.5 - min_x),
            near(at.1 - d * 0.5 - min_z),
            far(at.1 + d * 0.5 - min_z),
        )
    };

    // A piece whose descriptor is missing from the library is the map's problem, not this check's;
    // `Map::validate` is where that is caught.
    struct Tile<'a> {
        id: &'a str,
        /// The half-open cell rectangle this piece occupies, `[x0, x1) x [z0, z1)`.
        ///
        /// **Its footprint, not its centre.** Pairing by the cell a placement's `at` falls in
        /// assumed every piece was one cell across: two 3 m walls side by side have centres six
        /// cells apart, so they were never compared, while two small props whose centres landed in
        /// neighbouring cells were compared whether or not they touched. Both halves of that were
        /// wrong, and the first is the one that matters — a wall seam is the thing edge tokens exist
        /// to check.
        span: (i64, i64, i64, i64),
        /// The same piece in world metres, which is what the seam comparison reads. The cell rect
        /// above answers "do these touch"; this answers "where, exactly".
        x: (f32, f32),
        y: (f32, f32),
        z: (f32, f32),
        yaw: f32,
        grid: Subgrid,
        /// The lattice's divisions, derived from this placement's own patched extent — so a
        /// placement that overrides its size is compared on the lattice it actually stands on.
        div: (u32, u32, u32),
        /// Whether this tile says anything about its edges at all.
        declares: bool,
    }
    // **Every placement's base Y.** The seam comparison needs a vertical overlap, and a piece's Y is
    // not in its `Placed`: it comes from its `mount` through `stack::datum`, which for anything
    // resting on a surface needs that surface resolved first. `resolve_y` is the one function that
    // does it, so it is the one used here.
    //
    // A map whose heights cannot be resolved is `Map::validate`'s problem and `placement_at`'s to
    // report — the same call this function already makes about a descriptor missing from the library.
    // Reporting it twice under a name that hides its cause helps nobody.
    let Ok(ys) = crate::stack::resolve_y(map, library) else {
        return out;
    };

    let mut placed: Vec<Tile> = Vec::new();
    for (i, p) in map.placements.iter().enumerate() {
        let Some(base) = library.get(&p.descriptor) else {
            continue;
        };
        // Borrowed unless a patch forces a merge. The descriptor carries several string vectors and
        // this runs per placement, per recheck; the lattice below is sparse and cheap to clone.
        let patched;
        let authored: &crate::descriptor::Descriptor = match &p.patch {
            Some(patch) => {
                patched = base.patched_with(patch);
                &patched
            }
            None => base,
        };
        // A piece with no derivable lattice has no face to check. That is not a fault of the map —
        // a missing footprint is `Descriptor::resolve`'s to report, and reporting it here too would
        // put the same problem in front of the author twice under a name that hides its cause.
        let Ok(div) = crate::descriptor::divisions(authored, divisions) else {
            continue;
        };
        let grid = authored.subgrid.clone().unwrap_or_default();
        // The footprint as it stands, which a quarter turn swaps.
        let (fw, fd) = authored.extent.footprint.unwrap_or((cell, cell));
        let (fw, fd) = match quarter_turns(&p.id, p.yaw) {
            Ok(q) if q % 2 == 1 => (fd, fw),
            _ => (fw, fd),
        };
        let base = ys.get(i).copied().unwrap_or(map.origin.1);
        // The height as it stands, which is the same rule `divisions` uses — see
        // `descriptor::divisions` on why the stretch belongs in it.
        let h = authored.extent.height.unwrap_or(0.0) * authored.align.stretch_y.unwrap_or(1.0);
        placed.push(Tile {
            id: p.id.as_str(),
            span: to_span(p.at, (fw, fd)),
            x: (p.at.0 - fw * 0.5, p.at.0 + fw * 0.5),
            y: (base, base + h),
            z: (p.at.1 - fd * 0.5, p.at.1 + fd * 0.5),
            yaw: p.yaw,
            div,
            declares: grid.cells.iter().any(|c| c.edge.is_some()),
            grid,
        });
    }

    // **A pair where neither tile declares an edge is not checked at all.**
    //
    // This is the scope of the question, not a shortcut: the check asks whether what was drawn
    // matches what was *declared*, so where nothing is declared there is nothing to match. It is also
    // what keeps the feature quiet on a map that has never used it — `break_room` has an armchair at
    // yaw 240, and squaring that yaw is only anybody's problem once the armchair has a token on it.
    // Which side of `a` does `b` sit on, if they share a face at all?
    //
    // Two rectangles abut when one's far edge is the other's near edge AND their spans overlap on
    // the other axis — touching at a corner is not a seam, because no column of cells faces another.
    // The world meaning of the directions is `crate::wfc`'s: N is -Z, E is +X, S is +Z, W is -X.
    let side = |a: &Tile, b: &Tile| -> Option<Dir> {
        let (ax0, ax1, az0, az1) = a.span;
        let (bx0, bx1, bz0, bz1) = b.span;
        let over_x = ax0 < bx1 && bx0 < ax1;
        let over_z = az0 < bz1 && bz0 < az1;
        if ax1 == bx0 && over_z {
            Some(E)
        } else if bx1 == ax0 && over_z {
            Some(W)
        } else if az1 == bz0 && over_x {
            Some(S)
        } else if bz1 == az0 && over_x {
            Some(N)
        } else {
            None
        }
    };

    let mut yaw_reported: Vec<&str> = Vec::new();
    for a in &placed {
        for b in &placed {
            {
                let Some(dir) = side(a, b) else {
                    continue;
                };
                if !a.declares && !b.declares {
                    continue;
                }
                // **One seam, one fault.** Every adjacent pair is walked twice — once from each side —
                // and reporting both made a single disagreement read as "2 faults", which is the
                // panel telling the author to look for a second problem that is not there. `a < b`
                // picks one of the two orderings; it is total because ids are unique, so which one is
                // not left to the order `map.placements` happens to be in.
                if a.id >= b.id {
                    continue;
                }
                // Both yaws have to square before there is a face to read. Reported once per piece,
                // because the same tilted piece has up to four neighbours and four copies of one
                // sentence is not four pieces of information.
                let mut turned = Vec::new();
                for t in [a, b] {
                    match quarter_turns(t.id, t.yaw) {
                        // Both halves of the turn, together: the cells move and the divisions swap,
                        // and a face read off one without the other is a face of the wrong length.
                        Ok(q) => turned.push((
                            t.grid.rotated(q, t.div),
                            crate::descriptor::rotate_div(t.div, q),
                        )),
                        Err(message) => {
                            if !yaw_reported.contains(&t.id) {
                                yaw_reported.push(t.id);
                                out.push(Fault {
                                    a: t.id.to_owned(),
                                    b: String::new(),
                                    dir: N,
                                    a_face: Vec::new(),
                                    b_face: Vec::new(),
                                    message,
                                });
                            }
                        }
                    }
                }
                let [(a_grid, a_div), (b_grid, b_div)] = turned.as_slice() else {
                    continue;
                };
                // **The part they share, not the whole face.** See [`seam`] for why, and for the two
                // places in the shipped kits that forced the change.
                let a_at = Placed3 { x: a.x, y: a.y, z: a.z, div: *a_div };
                let b_at = Placed3 { x: b.x, y: b.y, z: b.z, div: *b_div };
                let Some((a_face, b_face)) =
                    seam(a_grid, a_at, b_grid, b_at, dir, cell / divisions.max(1) as f32)
                else {
                    // No rectangle in common: the cell rects touch but the pieces do not overlap in Y,
                    // so there is no seam here to be right or wrong about.
                    continue;
                };
                if a_face == b_face {
                    continue;
                }
                out.push(Fault {
                    a: a.id.to_owned(),
                    b: b.id.to_owned(),
                    dir,
                    a_face: a_face.iter().map(|t| t.map(str::to_owned)).collect(),
                    b_face: b_face.iter().map(|t| t.map(str::to_owned)).collect(),
                    message: format!(
                        "`{}` face {} is {} but `{}` presents {}",
                        a.id,
                        dir_name(dir),
                        say(&a_face),
                        b.id,
                        say(&b_face)
                    ),
                });
            }
        }
    }

    // A total key: ids are unique (`Map::validate`), so no two faults tie on all three.
    out.sort_by(|l, r| (&l.a, &l.b, l.dir).cmp(&(&r.a, &r.b, r.dir)));
    out
}

/// A face, in words. `-` is an unlabelled cell — which is a token, not an absence.
fn say(face: &[Option<&str>]) -> String {
    if face.is_empty() {
        return "no face".to_owned();
    }
    let parts: Vec<&str> = face.iter().map(|t| t.unwrap_or("-")).collect();
    format!("[{}]", parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{Descriptor, SubCell};
    use crate::map::{Map, Placed};

    fn grid(cells: &[((u32, u32, u32), &str)]) -> Subgrid {
        Subgrid {
            cells: cells
                .iter()
                .map(|(at, token)| SubCell {
                    at: *at,
                    edge: Some((*token).to_owned()),
                    ..SubCell::default()
                })
                .collect(),
        }
    }

    /// **The inert property, and it must hold without a branch.** Every shipped descriptor has an
    /// empty lattice, so every face is all-`None`, so everything matches everything. If this ever


    /// Faces of different lengths cannot agree, rather than agreeing on a prefix.
    ///
    /// Divisions are derived from size now, so this is the differently-**sized** case: a piece one
    /// cell deep beside one three cells deep. Their faces are genuinely different shapes and saying

    /// Four quarter turns put every cell back where it started.
    #[test]
    fn rotating_four_times_is_the_identity() {
        let div = (3, 2, 4);
        let g = grid(&[((0, 0, 0), "a"), ((2, 1, 3), "b"), ((1, 0, 2), "c")]);
        let mut turned = g.clone();
        let mut turned_div = div;
        for _ in 0..4 {
            turned = turned.rotated(1, turned_div);
            turned_div = crate::descriptor::rotate_div(turned_div, 1);
        }
        assert_eq!(turned_div, div);
        let mut want = g.cells.clone();
        let mut got = turned.cells.clone();
        want.sort_by_key(|c| c.at);
        got.sort_by_key(|c| c.at);
        assert_eq!(got, want);
    }

    /// **A quarter turn carries a face round with it.** The +X face becomes the −Z face, because a
    /// positive yaw turns +X toward −Z — the project's one forward convention.
    #[test]
    fn a_quarter_turn_moves_the_east_face_to_north() {
        let div = (2, 1, 3);
        let g = grid(&[((1, 0, 0), "seam"), ((1, 0, 1), "seam"), ((1, 0, 2), "seam")]);
        let east = face(&g, E, div);
        assert_eq!(east, vec![Some("seam"), Some("seam"), Some("seam")]);
        let turned = g.rotated(1, div);
        let turned_div = crate::descriptor::rotate_div(div, 1);
        assert_eq!(turned_div, (3, 1, 2), "a quarter turn swaps the x and z divisions");
        assert_eq!(face(&turned, N, turned_div), east, "the east face is now the north face");
    }

    /// Off-square yaws are refused by name rather than rounded to the nearest face.
    #[test]
    fn a_yaw_that_is_not_a_quarter_turn_is_refused_with_the_angle() {
        assert_eq!(quarter_turns("a", 0.0), Ok(0));
        assert_eq!(quarter_turns("a", 90.0), Ok(1));
        assert_eq!(quarter_turns("a", 270.0), Ok(3));
        assert_eq!(quarter_turns("a", -90.0), Ok(3));
        let e = quarter_turns("chair_a", 15.0).unwrap_err();
        assert!(e.contains("chair_a") && e.contains("15"), "{e}");
    }

    fn map_with(placements: Vec<Placed>) -> Map {
        Map {
            placements,
            ..Map::default()
        }
    }

    /// Every entry is one authoring cell on a side, so at `divisions: 1` its lattice is 1x1x1 and
    /// each face is a single token — the smallest shape that still has faces to compare.
    fn library_with(entries: &[(&str, Subgrid)]) -> Library {
        Library {
            descriptors: entries
                .iter()
                .map(|(id, g)| Descriptor {
                    id: (*id).to_owned(),
                    extent: crate::descriptor::Extent {
                        footprint: Some((crate::grid::SNAP, crate::grid::SNAP)),
                        height: Some(crate::grid::SNAP),
                    },
                    subgrid: Some(g.clone()),
                    ..Descriptor::default()
                })
                .collect(),
            ..Library::default()
        }
    }

    /// The shipped divisions-per-tile, so a test lattice is the size its `extent` says.
    const ONE_PER_TILE: u32 = 1;

    fn placed(id: &str, descriptor: &str, at: (f32, f32), yaw: f32) -> Placed {
        Placed {
            id: id.to_owned(),
            descriptor: descriptor.to_owned(),
            at,
            yaw,
            ..Placed::default()
        }
    }

    /// A library whose entries are `w` x `d` metres on the floor, so a test can place pieces that
    /// are bigger than one cell — which is what the centre-cell pairing could not handle.
    fn sized_library(entries: &[(&str, (f32, f32), Subgrid)]) -> Library {
        Library {
            descriptors: entries
                .iter()
                .map(|(id, fp, g)| Descriptor {
                    id: (*id).to_owned(),
                    extent: crate::descriptor::Extent {
                        footprint: Some(*fp),
                        height: Some(crate::grid::SNAP),
                    },
                    subgrid: Some(g.clone()),
                    ..Descriptor::default()
                })
                .collect(),
            ..Library::default()
        }
    }

    /// **A map nobody has authored tokens for costs one scan of the library.**
    ///
    /// The editor rechecks on every placement, so the quadratic walk was being paid repeatedly for
    /// an answer that is empty by construction until an edge token exists. Pinned by behaviour
    /// rather than by timing: a map far too large to walk quadratically still answers instantly, and
    /// would take visible seconds if the early-out were removed.
    #[test]
    fn an_unauthored_map_short_circuits_however_large_it_is() {
        let lib = sized_library(&[("crate_a", (0.5, 0.5), Subgrid::default())]);
        let many: Vec<Placed> = (0..4000)
            .map(|i| {
                placed(
                    &format!("p{i}"),
                    "crate_a",
                    ((i % 64) as f32 * 0.5, (i / 64) as f32 * 0.5),
                    0.0,
                )
            })
            .collect();
        assert_eq!(
            faults(&map_with(many), &lib, crate::grid::SNAP, ONE_PER_TILE),
            Vec::new()
        );
    }

    /// And the early-out cannot hide a fault: one authored token anywhere turns the check back on.
    #[test]
    fn a_single_declared_token_re_enables_the_whole_check() {
        let lib = sized_library(&[
            ("wall", (0.5, 0.5), grid(&[((0, 0, 0), "stone")])),
            ("gap", (0.5, 0.5), Subgrid::default()),
        ]);
        let map = map_with(vec![
            placed("g1", "gap", (0.25, 0.25), 0.0),
            placed("w1", "wall", (0.75, 0.25), 0.0),
        ]);
        assert_eq!(faults(&map, &lib, crate::grid::SNAP, ONE_PER_TILE).len(), 1);
    }

    /// **The defect this pairing was rewritten for.** Two 3 m walls side by side share a seam, but
    /// their centres are six cells apart — so pairing by the cell a placement's `at` fell in never
    /// compared them, and a wall seam is the thing edge tokens exist to check.
    #[test]
    fn two_wide_pieces_side_by_side_are_compared() {
        let lib = sized_library(&[
            // The seam under test is the wall's EAST face, which is its last x cell.
            ("wall", (3.0, 0.5), grid(&[((5, 0, 0), "stone")])),
            ("gap", (3.0, 0.5), Subgrid::default()),
        ]);
        // Centres 3 m apart on a 0.5 m grid: touching, six cells between the centres.
        let map = map_with(vec![
            placed("w1", "wall", (1.5, 0.25), 0.0),
            placed("g1", "gap", (4.5, 0.25), 0.0),
        ]);
        let found = faults(&map, &lib, crate::grid::SNAP, ONE_PER_TILE);
        assert_eq!(found.len(), 1, "the seam between two touching walls: {found:#?}");
        assert_eq!((found[0].a.as_str(), found[0].b.as_str()), ("g1", "w1"));
    }

    /// And the other half: pieces whose centre cells are neighbours but which do not touch are no
    /// longer compared. A gap of a whole cell is a gap.
    #[test]
    fn pieces_with_a_gap_between_them_are_not_compared() {
        let lib = sized_library(&[
            ("wall", (0.5, 0.5), grid(&[((0, 0, 0), "stone")])),
            ("gap", (0.5, 0.5), Subgrid::default()),
        ]);
        let map = map_with(vec![
            placed("w1", "wall", (0.25, 0.25), 0.0),
            // One empty cell between them.
            placed("g1", "gap", (1.25, 0.25), 0.0),
        ]);
        assert_eq!(faults(&map, &lib, crate::grid::SNAP, ONE_PER_TILE), Vec::new());
    }

    /// **A corner is not a seam.** Two pieces meeting only at a point present no column of cells to
    /// each other, so there is nothing to compare and nothing to report.
    #[test]
    fn pieces_touching_only_at_a_corner_are_not_compared() {
        let lib = sized_library(&[
            ("wall", (0.5, 0.5), grid(&[((0, 0, 0), "stone")])),
            ("gap", (0.5, 0.5), Subgrid::default()),
        ]);
        let map = map_with(vec![
            placed("w1", "wall", (0.25, 0.25), 0.0),
            placed("g1", "gap", (0.75, 0.75), 0.0),
        ]);
        assert_eq!(faults(&map, &lib, crate::grid::SNAP, ONE_PER_TILE), Vec::new());
    }

    /// A turned piece abuts on its turned footprint — a 3 m wall at 90 degrees runs along Z, and the
    /// neighbour it touches is the one beside it *then*, not the one beside it before the turn.
    #[test]
    fn a_turned_piece_abuts_on_the_footprint_it_actually_has() {
        let lib = sized_library(&[
            ("wall", (3.0, 0.5), grid(&[((0, 0, 0), "stone")])),
            ("gap", (0.5, 0.5), Subgrid::default()),
        ]);
        // Turned a quarter, the wall is 0.5 wide and 3 deep, running from z = 0 to z = 3. The token was
        // authored at x = 0 and `rotated` carries `(x, y, z)` to `(z, y, dx - 1 - x)`, so it lands at
        // z = 5 of 6 — the LAST half-metre of the run. That is where a neighbour has to be to meet it.
        let map = map_with(vec![
            placed("w1", "wall", (0.25, 1.5), 90.0),
            // Beside its long side. Reachable at all only if the turn was accounted for in the
            // pairing, and a fault only if the token moved with it.
            placed("g1", "gap", (0.75, 2.75), 0.0),
        ]);
        let found = faults(&map, &lib, crate::grid::SNAP, ONE_PER_TILE);
        assert_eq!(found.len(), 1, "the turned wall's long face: {found:#?}");

        // **The same neighbour at the other end of the run is not a fault**, because the token is not
        // there. Under whole-face equality it was one — six cells against one, refused on length
        // alone — and that is exactly the difference `seam` makes: it asks what is *at* the seam, not
        // how big the two faces are.
        let elsewhere = map_with(vec![
            placed("w1", "wall", (0.25, 1.5), 90.0),
            placed("g1", "gap", (0.75, 0.25), 0.0),
        ]);
        assert_eq!(faults(&elsewhere, &lib, crate::grid::SNAP, ONE_PER_TILE), Vec::new());
    }

    /// A map of unauthored tiles reports nothing — the inert property, end to end.
    #[test]
    fn a_map_of_unauthored_tiles_has_no_faults() {
        let lib = library_with(&[("crate_a", Subgrid::default())]);
        let map = map_with(vec![
            placed("one", "crate_a", (0.5, 0.5), 0.0),
            placed("two", "crate_a", (1.5, 0.5), 0.0),
        ]);
        assert_eq!(faults(&map, &lib, 1.0, ONE_PER_TILE), Vec::new());
    }

    /// One mismatched pair is reported once per direction it is wrong in — here `one`'s E face and
    /// `two`'s W face, which is one ordered pair each way.
    #[test]
    fn a_mismatched_pair_is_reported_and_names_both_faces() {
        let lib = library_with(&[
            ("wall", grid(&[((0, 0, 0), "stone")])),
            ("gap", Subgrid::default()),
        ]);
        let map = map_with(vec![
            placed("w1", "wall", (0.5, 0.5), 0.0),
            placed("g1", "gap", (1.5, 0.5), 0.0),
        ]);
        let found = faults(&map, &lib, 1.0, ONE_PER_TILE);
        assert_eq!(found.len(), 1, "one seam is one fault, not two: {found:#?}");
        assert!(found[0].message.contains("stone"), "{}", found[0].message);
        // `g1` sorts before `w1`, so that is the ordering reported — and the sentence still names
        // both pieces and both faces, so nothing is lost by picking one.
        assert_eq!((found[0].a.as_str(), found[0].b.as_str()), ("g1", "w1"));
    }

    /// **Order is stable and does not depend on placement order.** The determinism rule: the key is
    /// total, so shuffling the input cannot reorder the output.
    #[test]
    fn fault_order_does_not_depend_on_placement_order() {
        let lib = library_with(&[
            ("wall", grid(&[((0, 0, 0), "stone")])),
            ("gap", Subgrid::default()),
        ]);
        let forward = faults(
            &map_with(vec![
                placed("aaa", "wall", (0.5, 0.5), 0.0),
                placed("bbb", "gap", (1.5, 0.5), 0.0),
            ]),
            &lib,
            1.0,
            ONE_PER_TILE,
        );
        let backward = faults(
            &map_with(vec![
                placed("bbb", "gap", (1.5, 0.5), 0.0),
                placed("aaa", "wall", (0.5, 0.5), 0.0),
            ]),
            &lib,
            1.0,
            ONE_PER_TILE,
        );
        assert_eq!(forward, backward);
    }

    /// An off-square placement becomes a fault that names it, and does not stop the rest of the map
    /// being checked. Reported once, not once per neighbour.
    #[test]
    fn an_off_square_yaw_is_a_fault_not_a_dead_check() {
        let lib = library_with(&[("wall", grid(&[((0, 0, 0), "stone")]))]);
        let map = map_with(vec![
            placed("tilted", "wall", (0.5, 0.5), 15.0),
            placed("square", "wall", (1.5, 0.5), 0.0),
        ]);
        let found = faults(&map, &lib, 1.0, ONE_PER_TILE);
        assert_eq!(
            found.iter().filter(|f| f.a == "tilted" && f.b.is_empty()).count(),
            1,
            "one sentence per tilted piece, not one per neighbour: {found:#?}"
        );
        assert!(found.iter().any(|f| f.message.contains("tilted")));
    }

    /// **A tile that declares nothing is not asked to square its yaw.** `break_room` ships an armchair
    /// at yaw 240 and no edge tokens anywhere; before this rule the editor opened reporting a fault
    /// about a feature nobody had used yet.
    #[test]
    fn an_undeclared_tile_at_an_odd_yaw_is_not_a_fault() {
        let lib = library_with(&[("armchair", Subgrid::default()), ("table", Subgrid::default())]);
        let map = map_with(vec![
            placed("chair", "armchair", (0.5, 0.5), 240.0),
            placed("table", "table", (1.5, 0.5), 0.0),
        ]);
        assert_eq!(faults(&map, &lib, 1.0, ONE_PER_TILE), Vec::new());
    }

    /// But once its neighbour declares one, the odd yaw is exactly what stops the check, and it says
    /// so.
    #[test]
    fn an_odd_yaw_matters_as_soon_as_a_neighbour_declares_an_edge() {
        let lib = library_with(&[
            ("armchair", Subgrid::default()),
            ("wall", grid(&[((0, 0, 0), "stone")])),
        ]);
        let map = map_with(vec![
            placed("chair", "armchair", (0.5, 0.5), 240.0),
            placed("w1", "wall", (1.5, 0.5), 0.0),
        ]);
        let found = faults(&map, &lib, 1.0, ONE_PER_TILE);
        assert!(
            found.iter().any(|f| f.a == "chair" && f.message.contains("240")),
            "{found:#?}"
        );
    }
}

/// **The seam rule, and the two cases in the shipped kits that decided it.**
#[cfg(test)]
mod seam_tests {
    use super::*;
    use crate::descriptor::{Descriptor, Extent, SubCell};
    use crate::map::Placed;

    /// `wall` on every cell of a lattice, which is how the kits are authored.
    fn all_wall(div: (u32, u32, u32)) -> Subgrid {
        let (dx, dy, dz) = div;
        Subgrid {
            cells: (0..dx)
                .flat_map(|x| (0..dy).flat_map(move |y| (0..dz).map(move |z| (x, y, z))))
                .map(|at| SubCell {
                    at,
                    edge: Some("wall".into()),
                    ..SubCell::default()
                })
                .collect(),
        }
    }

    fn piece(id: &str, w: f32, h: f32, d: f32, div: (u32, u32, u32)) -> Descriptor {
        Descriptor {
            id: id.into(),
            extent: Extent {
                footprint: Some((w, d)),
                height: Some(h),
            },
            mount: Some(crate::descriptor::Mount::OnFloor),
            subgrid: Some(all_wall(div)),
            ..Descriptor::default()
        }
    }

    fn map_of(ps: Vec<Placed>) -> Map {
        Map {
            name: "t".into(),
            placements: ps,
            ..Map::default()
        }
    }

    fn at(id: &str, d: &str, x: f32, z: f32) -> Placed {
        Placed {
            id: id.into(),
            descriptor: d.into(),
            at: (x, z),
            yaw: 0.0,
            ..Placed::default()
        }
    }

    /// A wall's run-face against a doorway's, the shipped shapes: 2.40 m against 2.01 m, so five rows
    /// against four. **Whole-face equality refused this**; the overlap agrees, because every cell on
    /// the part they share says `wall` on both sides.
    #[test]
    fn a_wall_and_a_shorter_doorway_agree_over_the_rows_they_share() {
        let lib = Library {
            descriptors: vec![
                piece("wall", 0.1, 2.40, 1.0, (1, 5, 2)),
                piece("door", 0.1, 2.01, 1.0, (1, 4, 2)),
            ],
            ..Library::default()
        };
        let map = map_of(vec![at("w1", "wall", 0.25, 0.5), at("d1", "door", 0.25, 1.5)]);
        let found = faults(&map, &lib, crate::grid::SNAP, 1);
        assert!(found.is_empty(), "{found:#?}");
    }

    /// And a header, which only exists between 2.00 m and 2.40 m, agrees with the wall over exactly
    /// that band. **This pair was never compared at all** under the old pairing: a header sitting above
    /// a doorway shares its `(x, z)` cells, so `side` saw overlap rather than abutment. Here it is
    /// beside the wall, which is the seam it really has.
    #[test]
    fn a_header_agrees_with_the_wall_over_the_band_it_occupies() {
        let mut header = piece("header", 0.1, 0.40, 1.0, (1, 1, 2));
        // A header hangs at wall height rather than standing on the floor.
        header.mount = Some(crate::descriptor::Mount::OnWall { height: 2.0 });
        let lib = Library {
            descriptors: vec![piece("wall", 0.1, 2.40, 1.0, (1, 5, 2)), header],
            ..Library::default()
        };
        let map = map_of(vec![at("w1", "wall", 0.25, 0.5), at("h1", "header", 0.25, 1.5)]);
        let found = faults(&map, &lib, crate::grid::SNAP, 1);
        assert!(found.is_empty(), "{found:#?}");
    }

    /// **A disagreement inside the overlap is still a fault.** The whole point is to compare *less*,
    /// not to compare nothing — a permissive rule that reported nothing would be worse than the strict
    /// one it replaced.
    #[test]
    fn a_different_token_on_the_shared_rows_is_still_reported() {
        let mut door = piece("door", 0.1, 2.01, 1.0, (1, 4, 2));
        door.subgrid = Some(Subgrid {
            cells: (0..4)
                .flat_map(|y| [(0, y, 0), (0, y, 1)])
                .map(|at| SubCell {
                    at,
                    edge: Some("glass".into()),
                    ..SubCell::default()
                })
                .collect(),
        });
        let lib = Library {
            descriptors: vec![piece("wall", 0.1, 2.40, 1.0, (1, 5, 2)), door],
            ..Library::default()
        };
        let map = map_of(vec![at("w1", "wall", 0.25, 0.5), at("d1", "door", 0.25, 1.5)]);
        let found = faults(&map, &lib, crate::grid::SNAP, 1);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("glass"), "{}", found[0].message);
    }

    /// The greybox case: two pieces of one family at different **widths**, meeting along a run. Not an
    /// architectural stack — just a corner that is two cells wide against a wall that is one — and the
    /// overlap is the metre they actually share.
    #[test]
    fn a_wider_corner_agrees_with_its_wall_over_the_width_they_share() {
        let lib = Library {
            descriptors: vec![
                piece("wall", 0.5, 2.40, 1.0, (1, 5, 2)),
                piece("corner", 1.0, 2.40, 1.0, (2, 5, 2)),
            ],
            ..Library::default()
        };
        // Wall spans x [0, 0.5]; corner spans x [0, 1.0]. They meet on z, overlapping on the wall's
        // half-metre of width.
        let map = map_of(vec![at("w1", "wall", 0.25, 0.5), at("c1", "corner", 0.5, 1.5)]);
        let found = faults(&map, &lib, crate::grid::SNAP, 1);
        assert!(found.is_empty(), "{found:#?}");
    }

    /// **No vertical overlap is not a seam.** A piece floating entirely above another shares a cell
    /// footprint and no surface, so there is nothing to be right or wrong about — and reporting one
    /// would be inventing a relationship the geometry does not have.
    #[test]
    fn pieces_that_do_not_overlap_in_height_are_not_a_seam() {
        let mut high = piece("high", 0.1, 0.40, 1.0, (1, 1, 2));
        high.mount = Some(crate::descriptor::Mount::OnWall { height: 3.0 });
        high.subgrid = Some(Subgrid {
            cells: vec![SubCell {
                at: (0, 0, 0),
                edge: Some("glass".into()),
                ..SubCell::default()
            }],
        });
        let lib = Library {
            descriptors: vec![piece("wall", 0.1, 2.40, 1.0, (1, 5, 2)), high],
            ..Library::default()
        };
        // Beside the wall on the floor plan, but 3.0 m up where the wall stops at 2.40.
        let map = map_of(vec![at("w1", "wall", 0.25, 0.5), at("h1", "high", 0.25, 1.5)]);
        assert_eq!(faults(&map, &lib, crate::grid::SNAP, 1), Vec::new());
    }
}
