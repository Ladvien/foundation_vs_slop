//! **Do the tiles the author placed agree with the tokens the author declared?**
//!
//! [`Subgrid::edge`](crate::descriptor::SubCell::edge) is what a tile presents to whatever sits
//! beside it. This module is the one thing that reads it: a predicate, [`may_abut`], and one caller,
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
/// The order is `y` outer, then the axis that is not `dir`'s — so two facing columns are read in the
/// same spatial order and can be compared element by element. A cell with no `edge` contributes
/// `None`, which is a token in its own right; see the module note.
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

/// The direction facing back the other way.
fn opposite(dir: Dir) -> Dir {
    match dir {
        N => S,
        E => W,
        S => N,
        W => E,
        other => other,
    }
}

/// **May `b` sit `dir` of `a`?** True when the facing columns agree, element for element.
///
/// Equality, per Merrell & Manocha 2009 §4.3 — see the module note. Faces of different lengths never
/// agree, which is how two tiles divided differently refuse each other rather than matching on a
/// prefix.
///
/// **That refusal still matters now that divisions are derived.** Two pieces of the same size get
/// the same divisions and reach the element-wise comparison, which is the whole point of deriving
/// them — but a 0.5 m crate beside a 2.4 m wall genuinely presents a shorter face, and reporting
/// those as compatible would be inventing an agreement that the geometry does not support.
pub fn may_abut(a: &Subgrid, b: &Subgrid, dir: Dir, a_div: (u32, u32, u32), b_div: (u32, u32, u32)) -> bool {
    face(a, dir, a_div) == face(b, opposite(dir), b_div)
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
        yaw: f32,
        grid: Subgrid,
        /// The lattice's divisions, derived from this placement's own patched extent — so a
        /// placement that overrides its size is compared on the lattice it actually stands on.
        div: (u32, u32, u32),
        /// Whether this tile says anything about its edges at all.
        declares: bool,
    }
    let mut placed: Vec<Tile> = Vec::new();
    for p in &map.placements {
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
        let Ok(div) = crate::descriptor::divisions(&authored.extent, divisions, &p.descriptor) else {
            continue;
        };
        let grid = authored.subgrid.clone().unwrap_or_default();
        // The footprint as it stands, which a quarter turn swaps.
        let (fw, fd) = authored.extent.footprint.unwrap_or((cell, cell));
        let (fw, fd) = match quarter_turns(&p.id, p.yaw) {
            Ok(q) if q % 2 == 1 => (fd, fw),
            _ => (fw, fd),
        };
        placed.push(Tile {
            id: p.id.as_str(),
            span: to_span(p.at, (fw, fd)),
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
                if may_abut(a_grid, b_grid, dir, *a_div, *b_div) {
                    continue;
                }
                let a_face = face(a_grid, dir, *a_div);
                let b_face = face(b_grid, opposite(dir), *b_div);
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

    const ONE: (u32, u32, u32) = (1, 1, 1);

    /// **The inert property, and it must hold without a branch.** Every shipped descriptor has an
    /// empty lattice, so every face is all-`None`, so everything matches everything. If this ever
    /// fails, someone has made `None` mean "wildcard" or "no answer" instead of "a token".
    #[test]
    fn an_unauthored_lattice_agrees_with_everything() {
        let empty = Subgrid::default();
        for dir in [N, E, S, W] {
            assert!(
                may_abut(&empty, &empty, dir, ONE, ONE),
                "two unauthored lattices must abut in {}",
                dir_name(dir)
            );
        }
    }

    /// The whole point: two faces that say different things do not match.
    #[test]
    fn facing_tokens_must_agree() {
        // A 1x1x1 lattice: one cell, so each face is one token.
        let wall = grid(&[((0, 0, 0), "wall")]);
        let door = grid(&[((0, 0, 0), "door")]);
        assert!(may_abut(&wall, &wall, E, ONE, ONE));
        assert!(!may_abut(&wall, &door, E, ONE, ONE), "`wall` must not meet `door`");
        assert!(
            !may_abut(&wall, &Subgrid::default(), E, ONE, ONE),
            "an authored token must not meet an unlabelled cell — that would be a wildcard"
        );
    }

    /// Faces of different lengths cannot agree, rather than agreeing on a prefix.
    ///
    /// Divisions are derived from size now, so this is the differently-**sized** case: a piece one
    /// cell deep beside one three cells deep. Their faces are genuinely different shapes and saying
    /// otherwise would invent an agreement the geometry does not support.
    #[test]
    fn lattices_divided_differently_refuse_each_other() {
        let empty = Subgrid::default();
        assert!(!may_abut(&empty, &empty, E, (1, 1, 3), (1, 1, 2)));
    }

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
        // Turned a quarter, the wall is 0.5 wide and 3 deep, running from z = 0 to z = 3.
        let map = map_with(vec![
            placed("w1", "wall", (0.25, 1.5), 90.0),
            // Beside its long side, two metres along — only reachable if the turn was accounted for.
            placed("g1", "gap", (0.75, 2.0), 0.0),
        ]);
        let found = faults(&map, &lib, crate::grid::SNAP, ONE_PER_TILE);
        assert_eq!(found.len(), 1, "the turned wall's long face: {found:#?}");
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
