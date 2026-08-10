//! Expressive-range metrics over a **solved cell grid** — enclosure and opening density.
//!
//! The measurement Smith & Whitehead (`10.1145/1814256.1814260`) call *expressive range*: generate a
//! population, score each artefact on two axes, and read the 2-D histogram for bias before looking at
//! any individual output. Their own metrics (linearity, leniency) are platformer-specific; these two
//! are the substitutes argued in `docs/research/2026-08-09-composition-grammar-decisions.md` §4.1, and
//! the thresholds they are read against are pre-registered in §4.2 and §4.5 of that document.
//!
//! # The alphabet stays out of here
//!
//! Nothing in this module knows what a wall is called. Classification arrives as three closures over a
//! prototype index, the same seam [`crate::grammar::from_compositions`] takes its `agrees` through —
//! Karth & Smith's *"any arbitrary adjacency validity function can be substituted here"* applied one
//! function further out. A kit that spells its tokens differently, or a grammar over something other
//! than compositions, measures with the same code.
//!
//! # Why a doorway presents a wall
//!
//! A doorway tile is part of a room's boundary, not a hole in it. If the fill leaked through doorways,
//! then any room with a door would fail to be enclosed, enclosure would collapse toward zero for
//! exactly the rooms worth building, and opening density — *"doorway tiles per enclosed region"* —
//! would have no enclosed region left to count against. So `walls` answers true for a doorway's
//! doorway-bearing face, and `opening` marks the same tile as a puncture. Enclosure then measures
//! *whether the boundary closes*; opening density measures *how punctured it is*.

use crate::placement::ir::Dir;
use crate::wfc::{E, N, S, W};

/// Ranges per dimension in the pre-registered domain grid (§4.5 step 1).
///
/// Six, because that is the granularity Cooper's worked example of this exact measurement used
/// (`10.1609/aiide.v18i1.21944`, *Expressive Range Coverage*: *"6 ranges per dimension… of the 36
/// possible levels, 19 were found"*), and because 400 bins is noise-dominated at any sample size this
/// solver can afford.
pub const RANGES: usize = 6;

/// Total bins in the domain grid.
pub const BINS: usize = RANGES * RANGES;

/// Upper end of the opening-density axis. Values above it clamp into the top range, and a caller that
/// reports a max-bin share **must** report the clamped count separately — an unbounded tail folded into
/// one bin inflates that statistic by construction.
pub const OPENING_MAX: f32 = 4.0;

/// What one solved grid scores.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measured {
    /// Fraction of floor cells lying inside a closed wall boundary, in `[0, 1]`.
    ///
    /// **A grid with no floor cells scores 0.** Nothing is enclosed because nothing is there, and the
    /// row this feeds — *median enclosure < 0.15 means wall confetti* — reads a floorless grid as
    /// confetti, which is what it is.
    pub enclosure: f32,
    /// Doorway tiles on an enclosed region's boundary, averaged over regions.
    ///
    /// `None` when there is **no enclosed region at all**, which is undefined rather than zero: zero
    /// already means "enclosed regions, none of them with a door", and conflating the two would make
    /// the *sealed boxes* row unreadable. Pre-registered in §4.5 — such solves are excluded from the
    /// median and the histogram, and counted separately.
    pub opening_density: Option<f32>,
    /// How many enclosed regions were found. Zero iff `opening_density` is `None`.
    pub regions: usize,
}

#[inline]
fn opposite(dir: Dir) -> Dir {
    match dir {
        N => S,
        S => N,
        E => W,
        _ => E,
    }
}

/// The neighbour one step in `dir`, or `None` at the grid edge.
///
/// Directions follow [`crate::wfc`]'s own convention, taken from its propagator rather than restated:
/// `N` is `z - 1` (`wfc.rs`'s `N => (cx, cy - 1)`), and the grid is indexed `z * w + x`, matching how
/// [`crate::grammar::solve`] writes its initial domains.
#[inline]
fn step(w: usize, h: usize, x: usize, z: usize, dir: Dir) -> Option<(usize, usize)> {
    match dir {
        N => z.checked_sub(1).map(|z| (x, z)),
        E => (x + 1 < w).then_some((x + 1, z)),
        S => (z + 1 < h).then_some((x, z + 1)),
        W => x.checked_sub(1).map(|x| (x, z)),
        _ => None,
    }
}

/// Whether the seam between two adjacent cells is blocked by a wall on either side.
#[inline]
fn blocked<F: Fn(usize, Dir) -> bool>(grid: &[usize], here: usize, there: usize, dir: Dir, walls: &F) -> bool {
    walls(grid[here], dir) || walls(grid[there], opposite(dir))
}

/// Flood-fill inward from every border cell, across seams no wall blocks.
///
/// Cells the fill reaches are outside; cells it does not are enclosed. Every border cell is a seed —
/// there is no cell beyond the region to start from, so the region's own edge is where outside begins.
fn reached_from_border<F: Fn(usize, Dir) -> bool>(
    w: usize,
    h: usize,
    grid: &[usize],
    walls: &F,
) -> Vec<bool> {
    let mut seen = vec![false; w * h];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for z in 0..h {
        for x in 0..w {
            if x == 0 || z == 0 || x + 1 == w || z + 1 == h {
                let i = z * w + x;
                if !seen[i] {
                    seen[i] = true;
                    stack.push((x, z));
                }
            }
        }
    }
    while let Some((x, z)) = stack.pop() {
        let i = z * w + x;
        for dir in [N, E, S, W] {
            let Some((nx, nz)) = step(w, h, x, z, dir) else {
                continue;
            };
            let j = nz * w + nx;
            if seen[j] || blocked(grid, i, j, dir, walls) {
                continue;
            }
            seen[j] = true;
            stack.push((nx, nz));
        }
    }
    seen
}

/// **Score one solved grid on both axes.**
///
/// One function rather than two because both rest on the same flood fill; computing it twice would be
/// two paths to one answer, and they could disagree.
///
/// `grid` holds one prototype index per cell, indexed `z * w + x`. The three closures classify a
/// prototype: `walls(p, dir)` is whether `p` presents a wall on that face, `floor(p)` whether it is
/// floor a room could be made of, and `opening(p)` whether it is a doorway.
///
/// # Errors
///
/// When `grid.len()` is not `w * h`, or either dimension is zero. That is a malformed call rather than
/// a degenerate solve, so it is named rather than scored.
pub fn measure<Wf, Ff, Of>(
    w: usize,
    h: usize,
    grid: &[usize],
    walls: Wf,
    floor: Ff,
    opening: Of,
) -> Result<Measured, String>
where
    Wf: Fn(usize, Dir) -> bool,
    Ff: Fn(usize) -> bool,
    Of: Fn(usize) -> bool,
{
    if w == 0 || h == 0 {
        return Err(format!("range: a {w} x {h} grid has no cells to measure"));
    }
    if grid.len() != w * h {
        return Err(format!(
            "range: a {w} x {h} grid holds {} cells, but {} were given",
            w * h,
            grid.len()
        ));
    }

    let outside = reached_from_border(w, h, grid, &walls);

    let total_floor = (0..w * h).filter(|&i| floor(grid[i])).count();
    let enclosed_floor = (0..w * h).filter(|&i| floor(grid[i]) && !outside[i]).count();
    let enclosure = if total_floor == 0 {
        0.0
    } else {
        enclosed_floor as f32 / total_floor as f32
    };

    // A region is a connected run of **enclosed floor** cells, joined across seams no wall blocks —
    // the interiors, so two sealed rooms sharing a wall stay two rooms. Grouping every unreached cell
    // instead would merge them through their touching wall rings.
    let mut region_of: Vec<Option<usize>> = vec![None; w * h];
    let mut regions = 0usize;
    let mut doors_per_region: Vec<usize> = Vec::new();
    for z0 in 0..h {
        for x0 in 0..w {
            let start = z0 * w + x0;
            if outside[start] || !floor(grid[start]) || region_of[start].is_some() {
                continue;
            }
            let id = regions;
            regions += 1;
            let mut doors: Vec<usize> = Vec::new();
            let mut stack = vec![(x0, z0)];
            region_of[start] = Some(id);
            while let Some((x, z)) = stack.pop() {
                let i = z * w + x;
                for dir in [N, E, S, W] {
                    let Some((nx, nz)) = step(w, h, x, z, dir) else {
                        continue;
                    };
                    let j = nz * w + nx;
                    // A doorway on this region's boundary is counted whether or not the seam is open —
                    // it is the puncture in the wall, and the wall is what makes it a boundary.
                    if opening(grid[j]) && !doors.contains(&j) {
                        doors.push(j);
                    }
                    if blocked(grid, i, j, dir, &walls) || outside[j] || !floor(grid[j]) {
                        continue;
                    }
                    if region_of[j].is_none() {
                        region_of[j] = Some(id);
                        stack.push((nx, nz));
                    }
                }
            }
            doors_per_region.push(doors.len());
        }
    }

    let opening_density = if regions == 0 {
        None
    } else {
        Some(doors_per_region.iter().sum::<usize>() as f32 / regions as f32)
    };

    Ok(Measured { enclosure, opening_density, regions })
}

/// Which bin of the pre-registered domain grid a score falls in, as `(enclosure, opening)` indices.
///
/// The domain is fixed before any solve is looked at — enclosure `[0, 1]`, opening density
/// `[0, OPENING_MAX]` — so that the histogram's shape is a property of the generator rather than of
/// the data's own spread. Values at or above the top of either axis clamp into the last range.
pub fn bin(enclosure: f32, opening: f32) -> (usize, usize) {
    let clamp = |v: f32, hi: f32| -> usize {
        if !v.is_finite() || v <= 0.0 {
            return 0;
        }
        let ix = (v / hi * RANGES as f32).floor();
        (ix.max(0.0) as usize).min(RANGES - 1)
    };
    (clamp(enclosure, 1.0), clamp(opening, OPENING_MAX))
}

/// The largest share any single bin holds, in `[0, 1]`. Zero for an empty histogram.
///
/// §4.5's row 4b: *no single bin holds a majority of all solves.* Dimensionless, sample-size
/// invariant, and independent of the bin count — which is what lets its threshold be argued rather
/// than calibrated against a baseline this solver cannot measure.
pub fn max_bin_share(counts: &[u32]) -> f32 {
    let total: u64 = counts.iter().map(|&c| c as u64).sum();
    if total == 0 {
        return 0.0;
    }
    let top = counts.iter().copied().max().unwrap_or(0) as f64;
    (top / total as f64) as f32
}

/// Shannon entropy of the bin distribution over `ln(counts.len())`, in `[0, 1]`.
///
/// §4.5's row 4a. One occupied bin scores 0; a uniform spread over every bin scores 1. Zero for an
/// empty histogram, and zero for a single-bin one, which are the same reading: no spread at all.
pub fn normalised_entropy(counts: &[u32]) -> f32 {
    let total: u64 = counts.iter().map(|&c| c as u64).sum();
    if total == 0 || counts.len() < 2 {
        return 0.0;
    }
    let total = total as f64;
    let h: f64 = counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / total;
            -p * p.ln()
        })
        .sum();
    (h / (counts.len() as f64).ln()) as f32
}

/// Total-variation distance between two histograms, in `[0, 1]`.
///
/// The stopping rule of §4.5: run **disjoint** blocks and compare. Nested samples would not do —
/// with `H₂ₙ = (Hₙ + H′)/2`, `TV(Hₙ, H₂ₙ) = ½·TV(Hₙ, H′)` exactly, so a nested comparison reads half
/// the distance it appears to and stops twice as early.
///
/// Returns `1.0` if either side is empty and the other is not, and `0.0` if both are.
pub fn total_variation(a: &[u32], b: &[u32]) -> f32 {
    let ta: u64 = a.iter().map(|&c| c as u64).sum();
    let tb: u64 = b.iter().map(|&c| c as u64).sum();
    if ta == 0 && tb == 0 {
        return 0.0;
    }
    if ta == 0 || tb == 0 {
        return 1.0;
    }
    let n = a.len().max(b.len());
    let sum: f64 = (0..n)
        .map(|i| {
            let pa = *a.get(i).unwrap_or(&0) as f64 / ta as f64;
            let pb = *b.get(i).unwrap_or(&0) as f64 / tb as f64;
            (pa - pb).abs()
        })
        .sum();
    (0.5 * sum) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    // A hand-built alphabet, so the tests state what they mean rather than importing a kit.
    //
    //   0  open        — floor, no walls on any face
    //   1  solid       — no floor, walls on all four faces
    //   2  doorway     — no floor, walls on all four faces, and a puncture
    //   3  void        — no floor, no walls: a hole a fill walks straight through
    const OPEN: usize = 0;
    const SOLID: usize = 1;
    const DOOR: usize = 2;
    const VOID: usize = 3;

    /// Prototypes at or above this index wall exactly the faces in their low four bits.
    ///
    /// **The shipped kit is one-sided** — `tile_wall_n` presents `wall` on a single face, and a room is
    /// closed by four of those plus four corners. A test alphabet of all-four-faces tiles never varies
    /// `dir`, so it cannot tell a correct `opposite` from a broken one; that gap survived a mutation
    /// run before this existed.
    const MASKED: usize = 16;

    fn masked(dirs: &[Dir]) -> usize {
        MASKED + dirs.iter().fold(0usize, |m, &d| m | 1 << d)
    }

    fn walls(p: usize, d: Dir) -> bool {
        if p >= MASKED {
            return (p - MASKED) >> d & 1 == 1;
        }
        p == SOLID || p == DOOR
    }
    fn floor(p: usize) -> bool {
        p == OPEN
    }
    fn opening(p: usize) -> bool {
        p == DOOR
    }

    fn score(w: usize, h: usize, g: &[usize]) -> Measured {
        match measure(w, h, g, walls, floor, opening) {
            Ok(m) => m,
            Err(e) => panic!("{e}"),
        }
    }

    /// A 5x5 with a ring of solid around one floor cell. Everything outside the ring is void, so the
    /// fill runs right up to the wall and stops.
    fn sealed_room() -> Vec<usize> {
        let mut g = vec![VOID; 25];
        for z in 1..4 {
            for x in 1..4 {
                g[z * 5 + x] = SOLID;
            }
        }
        g[2 * 5 + 2] = OPEN;
        g
    }

    #[test]
    fn a_sealed_room_encloses_its_floor_and_has_no_doors() {
        let m = score(5, 5, &sealed_room());
        assert_eq!(m.enclosure, 1.0, "the one floor cell is inside a closed boundary");
        assert_eq!(m.regions, 1);
        assert_eq!(m.opening_density, Some(0.0), "sealed is zero doors, not undefined");
    }

    #[test]
    fn a_door_in_the_ring_is_counted_against_the_region_it_opens() {
        let mut g = sealed_room();
        g[1 * 5 + 2] = DOOR; // directly north of the floor cell
        let m = score(5, 5, &g);
        assert_eq!(m.enclosure, 1.0, "a door is part of the boundary, so the room still closes");
        assert_eq!(m.regions, 1);
        assert_eq!(m.opening_density, Some(1.0));
    }

    #[test]
    fn a_gap_in_the_ring_lets_the_fill_in_and_nothing_is_enclosed() {
        let mut g = sealed_room();
        g[1 * 5 + 2] = VOID; // a hole, not a door: no walls at all
        let m = score(5, 5, &g);
        assert_eq!(m.enclosure, 0.0);
        assert_eq!(m.regions, 0);
        assert_eq!(m.opening_density, None, "no enclosed region is undefined, not zero");
    }

    #[test]
    fn open_floor_everywhere_encloses_nothing() {
        let m = score(4, 4, &vec![OPEN; 16]);
        assert_eq!(m.enclosure, 0.0);
        assert_eq!(m.regions, 0);
        assert_eq!(m.opening_density, None);
    }

    #[test]
    fn a_grid_with_no_floor_scores_zero_rather_than_dividing_by_zero() {
        let m = score(4, 4, &vec![SOLID; 16]);
        assert_eq!(m.enclosure, 0.0);
        assert_eq!(m.regions, 0, "solid is not floor, so there is no room in there");
        assert_eq!(m.opening_density, None);
    }

    #[test]
    fn two_rooms_sharing_a_wall_stay_two_regions() {
        // 7x5: two 1-cell rooms at x=2 and x=4, walled, sharing the solid column at x=3.
        let mut g = vec![VOID; 35];
        for z in 1..4 {
            for x in 1..6 {
                g[z * 7 + x] = SOLID;
            }
        }
        g[2 * 7 + 2] = OPEN;
        g[2 * 7 + 4] = OPEN;
        let m = score(7, 5, &g);
        assert_eq!(m.enclosure, 1.0);
        assert_eq!(m.regions, 2, "grouping every unreached cell would merge them through the ring");
        assert_eq!(m.opening_density, Some(0.0));
    }

    #[test]
    fn half_the_floor_enclosed_is_half() {
        let mut g = sealed_room();
        g[0] = OPEN; // a floor cell out in the open, on the border
        let m = score(5, 5, &g);
        assert_eq!(m.enclosure, 0.5, "one of two floor cells is inside");
        assert_eq!(m.regions, 1);
    }

    /// The shape the real kit actually makes: a ring of **one-sided** wall tiles, each presenting a
    /// wall only on the face pointing out of the room, plus corners presenting two.
    ///
    /// This is the case that exercises the far side of every seam — the fill is stopped by the
    /// *neighbour's* opposing face rather than by the face it is leaving — so it is the only test here
    /// that can tell a correct direction inverse from a broken one.
    #[test]
    fn a_ring_of_one_sided_walls_seals_from_every_side() {
        let mut g = vec![VOID; 25];
        // Outward faces, for the eight cells around the centre of a 5x5.
        g[1 * 5 + 1] = masked(&[N, W]);
        g[1 * 5 + 2] = masked(&[N]);
        g[1 * 5 + 3] = masked(&[N, E]);
        g[2 * 5 + 1] = masked(&[W]);
        g[2 * 5 + 3] = masked(&[E]);
        g[3 * 5 + 1] = masked(&[W, S]);
        g[3 * 5 + 2] = masked(&[S]);
        g[3 * 5 + 3] = masked(&[E, S]);
        g[2 * 5 + 2] = OPEN;

        let m = score(5, 5, &g);
        assert_eq!(m.enclosure, 1.0, "every approach is stopped by the far face of the seam");
        assert_eq!(m.regions, 1);
        assert_eq!(m.opening_density, Some(0.0));

        // Knock the south wall's only face off and the fill walks in from below — proving the seal
        // above rests on that one face and not on the tile being solid.
        let mut leaky = g.clone();
        leaky[3 * 5 + 2] = masked(&[]);
        assert_eq!(score(5, 5, &leaky).enclosure, 0.0);
    }

    /// The same seal built the other way round: four one-sided tiles each walling the face that points
    /// *at* the room. The fill now reaches every wall tile freely and is stopped on the way out of it,
    /// by the near side of the seam rather than the far side.
    ///
    /// Both orientations occur in a real kit — which face of `tile_wall_n` points into the room is a
    /// matter of where the author turned it — and a seam test that only ever checks one of them passes
    /// with half the rule deleted.
    #[test]
    fn a_ring_of_inward_facing_walls_seals_from_the_near_side() {
        let mut g = vec![VOID; 25];
        g[1 * 5 + 2] = masked(&[S]); // north of the room, walling south
        g[3 * 5 + 2] = masked(&[N]);
        g[2 * 5 + 1] = masked(&[E]);
        g[2 * 5 + 3] = masked(&[W]);
        g[2 * 5 + 2] = OPEN;

        let m = score(5, 5, &g);
        assert_eq!(m.enclosure, 1.0, "each wall stops the fill as it leaves, not as it arrives");
        assert_eq!(m.regions, 1);

        let mut leaky = g.clone();
        leaky[1 * 5 + 2] = masked(&[]);
        assert_eq!(score(5, 5, &leaky).enclosure, 0.0, "one face is the whole seal");
    }

    /// Outside is not necessarily one place. A wall running border to border splits it in two, and
    /// **every** border cell has to be a seed — starting from one corner would mark the far side as
    /// enclosed and report a room where there is only the other half of the outdoors.
    #[test]
    fn outside_can_be_two_places_and_both_are_outside() {
        let mut g = vec![OPEN; 25];
        for z in 0..5 {
            g[z * 5 + 2] = SOLID; // a stripe from the north border to the south one
        }
        let m = score(5, 5, &g);
        assert_eq!(m.enclosure, 0.0, "both halves reach a border of their own");
        assert_eq!(m.regions, 0);

        // A dead-end corridor reaching in from **one** edge, touching no other. It is outdoors, and
        // only a seed on that edge says so — so all four sides must seed, and each is checked, because
        // omitting either pair leaves the other pair's corridors looking like rooms.
        for (edge, cells) in [
            ("west", [(0, 2), (1, 2), (2, 2)]),
            ("east", [(4, 2), (3, 2), (2, 2)]),
            ("north", [(2, 0), (2, 1), (2, 2)]),
            ("south", [(2, 4), (2, 3), (2, 2)]),
        ] {
            let mut pocket = vec![SOLID; 25];
            for (x, z) in cells {
                pocket[z * 5 + x] = OPEN;
            }
            let m = score(5, 5, &pocket);
            assert_eq!(m.enclosure, 0.0, "a corridor open to the {edge} is not a room");
            assert_eq!(m.regions, 0, "a corridor open to the {edge} is not a room");
        }
    }

    #[test]
    fn a_malformed_grid_is_named_rather_than_scored() {
        let e = measure(3, 3, &[OPEN; 4], walls, floor, opening).unwrap_err();
        assert!(e.contains("9 cells"), "{e}");
        assert!(e.contains('4'), "{e}");
        assert!(measure(0, 3, &[], walls, floor, opening).is_err());
    }

    #[test]
    fn bins_span_the_pre_registered_domain_and_clamp_above_it() {
        assert_eq!(bin(0.0, 0.0), (0, 0));
        assert_eq!(bin(1.0, OPENING_MAX), (RANGES - 1, RANGES - 1), "the top edge is the top bin");
        assert_eq!(bin(2.0, 99.0), (RANGES - 1, RANGES - 1), "above the domain clamps in");
        assert_eq!(bin(0.5, 2.0), (3, 3));
        assert_eq!(bin(0.16, 0.7), (0, 1), "a sixth of each axis is one range");
        assert_eq!(bin(-1.0, -1.0), (0, 0));
        assert_eq!(bin(f32::NAN, f32::NAN), (0, 0));
    }

    #[test]
    fn max_bin_share_is_the_largest_share() {
        assert_eq!(max_bin_share(&[]), 0.0);
        assert_eq!(max_bin_share(&[0, 0]), 0.0);
        assert_eq!(max_bin_share(&[10]), 1.0);
        assert_eq!(max_bin_share(&[3, 1]), 0.75);
        assert_eq!(max_bin_share(&[1, 1, 1, 1]), 0.25);
    }

    #[test]
    fn entropy_is_zero_on_one_bin_and_one_on_a_uniform_spread() {
        assert_eq!(normalised_entropy(&[0; 36]), 0.0);
        let mut one = [0u32; 36];
        one[7] = 200;
        assert_eq!(normalised_entropy(&one), 0.0, "everything in one bin has no spread");
        assert!((normalised_entropy(&[1; 36]) - 1.0).abs() < 1e-6);
    }

    /// The exact arithmetic §4.5 sets the floor by: because `36 = 6²`, `ln6/ln36 = ½`, so the two-bin
    /// and three-bin uniform values are symmetric about ¼ — 0.25 is the maximum-margin point, and a
    /// floor anywhere else is nearer one canonical degenerate case than the other.
    #[test]
    fn the_committed_floor_sits_exactly_between_two_and_three_uniform_bins() {
        let uniform_over = |k: usize| -> f32 {
            let mut c = [0u32; 36];
            for slot in c.iter_mut().take(k) {
                *slot = 10;
            }
            normalised_entropy(&c)
        };
        let two = uniform_over(2);
        let three = uniform_over(3);
        assert!(two < 0.25 && three > 0.25, "0.25 separates them: {two} / {three}");
        assert!(
            ((0.25 - two) - (three - 0.25)).abs() < 1e-6,
            "equidistant: {two} .. 0.25 .. {three}"
        );
        assert!((two + three - 0.5).abs() < 1e-6, "they sum to a half because ln6/ln36 = 1/2");
    }

    #[test]
    fn total_variation_is_zero_on_identical_shapes_and_one_on_disjoint_ones() {
        assert_eq!(total_variation(&[1, 1], &[5, 5]), 0.0, "same shape, different size");
        assert_eq!(total_variation(&[1, 0], &[0, 1]), 1.0);
        assert_eq!(total_variation(&[], &[]), 0.0);
        assert_eq!(total_variation(&[1], &[0]), 1.0, "one side empty");
        assert!((total_variation(&[3, 1], &[1, 1]) - 0.25).abs() < 1e-6);
    }

    /// Nested blocks halve the distance exactly, which is why §4.5 compares disjoint ones.
    #[test]
    fn a_nested_block_reads_half_the_distance_of_a_disjoint_one() {
        let a = [8u32, 0];
        let fresh = [0u32, 8];
        let nested = [a[0] + fresh[0], a[1] + fresh[1]];
        let disjoint = total_variation(&a, &fresh);
        let against_nested = total_variation(&a, &nested);
        assert!((disjoint - 1.0).abs() < 1e-6);
        assert!((against_nested - 0.5 * disjoint).abs() < 1e-6);
    }
}
