//! **Learn a map's grammar, then continue it** — WFC over what the author already placed.
//!
//! The editor's mixed-initiative half. An author lays out a corner of a room the way they want it,
//! marks the pieces they mean to keep, and the solver fills the rest of the bounds with more of the
//! same — not more of *some* arrangement, more of *theirs*.
//!
//! # Where the rules come from
//!
//! Nowhere else. A [`Descriptor`] says what a piece is; it says nothing about what may stand beside
//! it, and inventing an adjacency schema would mean asking an author to write down a grammar before
//! they are allowed to draw one.
//!
//! So the grammar is **learned from the map**, which is what WFC has always actually been. Karth &
//! Smith 2017 make the framing precise — WFC *is* finite-domain constraint solving, and its rule set
//! is an example decomposed into local constraints. Two pieces that the author has placed side by
//! side are two pieces that may be side by side. Nothing else is permitted, which is what makes the
//! output look authored rather than shuffled.
//!
//! # Owned cells are unary constraints, not a special case
//!
//! `crate::wfc::collapse_grid` already takes a per-cell starting domain, and
//! `initial_domains_restrict_output` already asserts a pinned cell survives collapse. So "the author
//! owns this cell" is a domain of one bit, made arc-consistent before the first observe — the same
//! mechanism the dungeon's boundary rule uses. There is no branch anywhere that says *if owned*.
//!
//! This is Alvarez et al.'s lock brush (FDG 2018, `10.1145/3235765.3235810`): locked tiles subdivide
//! the space into mutable and immutable zones, and every generated suggestion preserves them. It is
//! also the one third of Smelik et al. 2010's "possible facilities" that anyone actually built — see
//! `docs/2026-08-03-emerge-mapper-plan-review.md` §4 on why that citation is an aspiration.
//!
//! # It refuses rather than approximating
//!
//! A contradiction means the learned grammar cannot tile the region the author asked for. The honest
//! response is to say so — the alternative is a room full of the one prototype that fits everywhere,
//! which looks like the tool working and is not.

use std::collections::HashMap;

use crate::composition::{Composition, Envelope, Interface};
use crate::library::Library;
use crate::placement::ir::Dir;
use crate::map::{Map, Placed};
use crate::wfc;
use crate::wfc::{E, N, S, W};

/// One thing a cell can hold: a descriptor at a yaw, or nothing.
///
/// Yaw is part of the prototype because it is part of the adjacency. A wall turned 90° meets
/// different neighbours than the same wall turned 0°, and folding the two together would learn a
/// grammar that permits corners nobody drew.
#[derive(Clone, Debug, PartialEq)]
pub enum Prototype {
    /// The cell is empty. Always index 0, and always permitted everywhere — a grammar that cannot
    /// express "nothing goes here" cannot leave a doorway.
    Empty,
    Piece { descriptor: String, yaw: f32 },
}

/// How far a piece's footprint may be from the solver's cell and still count as one tile, metres.
///
/// A tenth of a millimetre: far below anything an author expresses, far above `f32`'s error at
/// authoring scale. The same reasoning `adjacency::EDGE_EPSILON` gives for its own value.
pub const CELL_EPSILON: f32 = 1e-4;

/// The most prototypes the solver can carry — `collapse_grid` packs a domain into a `u32`.
pub const MAX_PROTOTYPES: usize = 32;

/// A learned grammar: what exists, how often, and what may sit beside what.
#[derive(Debug)]
pub struct Grammar {
    pub prototypes: Vec<Prototype>,
    /// Selection weight per prototype — how often the author used it.
    pub weights: Vec<f64>,
    /// `support[dir][p]` = the prototypes that may sit on `p`'s `dir` side (N, E, S, W).
    pub support: [Vec<u32>; 4],
}

impl Grammar {
    pub fn len(&self) -> usize {
        self.prototypes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.prototypes.is_empty()
    }
}

/// Integer cell coordinates for a world point, on a grid of `cell` metres anchored at the map's
/// minimum corner.
fn to_cell(map: &Map, cell: f32, at: (f32, f32)) -> (i64, i64) {
    let (min_x, min_z, _, _) = map.floor_rect();
    (
        ((at.0 - min_x) / cell).floor() as i64,
        ((at.1 - min_z) / cell).floor() as i64,
    )
}

/// The world centre of a cell.
fn cell_centre(map: &Map, cell: f32, c: (i64, i64)) -> (f32, f32) {
    let (min_x, min_z, _, _) = map.floor_rect();
    (
        min_x + (c.0 as f32 + 0.5) * cell,
        min_z + (c.1 as f32 + 0.5) * cell,
    )
}

/// The map's bounds in cells.
fn dimensions(map: &Map, cell: f32) -> (usize, usize) {
    (
        (map.bounds.0 / cell).round().max(1.0) as usize,
        (map.bounds.2 / cell).round().max(1.0) as usize,
    )
}

/// **Read the author's arrangement as a grammar.**
///
/// Every placement lands in a cell; every orthogonally adjacent pair of cells teaches one rule in each
/// direction. Cells the author left empty teach rules too — that is how the solver learns it is
/// allowed to leave space, and a grammar without them fills every square metre.
pub fn learn(map: &Map, cell: f32) -> Result<Grammar, String> {
    if !(cell.is_finite() && cell > 0.0) {
        return Err(format!("grammar: cell size {cell} is not a usable step"));
    }
    let (w, h) = dimensions(map, cell);

    // Cell -> prototype index. A second placement in one cell is a stacked piece; the grammar is
    // about the floor plan, so the first one wins and the rest are simply not part of the lesson.
    let mut prototypes = vec![Prototype::Empty];
    let mut counts: Vec<f64> = vec![0.0];
    let mut grid: Vec<usize> = vec![0; w * h];
    let mut placed_at: HashMap<(i64, i64), usize> = HashMap::new();

    for p in &map.placements {
        let c = to_cell(map, cell, p.at);
        if c.0 < 0 || c.1 < 0 || c.0 as usize >= w || c.1 as usize >= h {
            continue;
        }
        if placed_at.contains_key(&c) {
            continue;
        }
        let proto = Prototype::Piece {
            descriptor: p.descriptor.clone(),
            // Quantised, so two chairs a floating-point hair apart are one prototype rather than two.
            yaw: (p.yaw / 15.0).round() * 15.0,
        };
        let ix = match prototypes.iter().position(|q| *q == proto) {
            Some(ix) => ix,
            None => {
                if prototypes.len() >= MAX_PROTOTYPES {
                    return Err(format!(
                        "grammar: this map uses more than {MAX_PROTOTYPES} distinct piece-and-yaw \
                         combinations, which is more than the solver's domain can hold. Learn from a \
                         smaller region, or place fewer kinds of thing in the example."
                    ));
                }
                prototypes.push(proto);
                counts.push(0.0);
                prototypes.len() - 1
            }
        };
        placed_at.insert(c, ix);
        grid[c.1 as usize * w + c.0 as usize] = ix;
    }

    for &ix in &grid {
        counts[ix] += 1.0;
    }

    // Adjacency, read off the example. `dir` is N, E, S, W to match `wfc`'s edge order.
    let n = prototypes.len();
    let mut support: [Vec<u32>; 4] = [vec![0; n], vec![0; n], vec![0; n], vec![0; n]];
    let steps: [(i64, i64); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
    for y in 0..h {
        for x in 0..w {
            let a = grid[y * w + x];
            for (dir, (dx, dy)) in steps.iter().enumerate() {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx < 0 || ny < 0 || nx as usize >= w || ny as usize >= h {
                    continue;
                }
                let b = grid[ny as usize * w + nx as usize];
                support[dir][a] |= 1 << b;
            }
        }
    }

    // **Empty must be able to meet empty.** A one-piece map teaches only "piece beside empty", and a
    // solver that cannot put two empties side by side then has to fill every other cell — which is
    // not what a map with one crate in it looks like.
    for dir in 0..4 {
        support[dir][0] |= 1;
    }

    Ok(Grammar {
        prototypes,
        weights: counts,
        support,
    })
}

/// What a solve produced.
pub struct Solved {
    /// New placements for the cells the solver filled. Owned and existing pieces are not repeated.
    pub placements: Vec<Placed>,
    /// Cells the solver was free to choose.
    pub free_cells: usize,
    /// Cells pinned by the author.
    pub owned_cells: usize,
    /// The collapsed grid itself: one prototype index per cell, indexed `z * width + x`.
    ///
    /// **Returned rather than rebuilt.** [`crate::range`] measures a solved grid, and reconstructing
    /// it from `placements` would be a second answer to a question this function already answered —
    /// pinned cells are not in `placements` at all, so the two could disagree about exactly the cells
    /// the author cares most about.
    pub grid: Vec<usize>,
    /// The grid's width in cells; `grid.len()` is `width * height`.
    pub width: usize,
    /// The grid's height in cells.
    pub height: usize,
}

/// **Fill the map's free cells with more of the author's arrangement.**
///
/// Owned placements pin their cell to a single prototype — a unary constraint, exactly the mechanism
/// `wfc::boundary_initial` uses — and everything else is free. Unowned existing placements are
/// *replaced*: they were the sketch, and the solve is the drawing.
pub fn solve(
    map: &Map,
    grammar: &Grammar,
    cell: f32,
    seed: u64,
    mut next_id: impl FnMut() -> String,
) -> Result<Solved, String> {
    if grammar.len() < 2 {
        return Err(
            "grammar: nothing has been placed yet, so there is no arrangement to continue. Place a \
             few pieces the way you want them first."
                .to_owned(),
        );
    }
    let (w, h) = dimensions(map, cell);
    let full: u32 = if grammar.len() == 32 {
        u32::MAX
    } else {
        (1u32 << grammar.len()) - 1
    };
    let mut initial = vec![full; w * h];
    let mut owned_cells = 0usize;

    for p in map.placements.iter().filter(|p| p.owned) {
        let c = to_cell(map, cell, p.at);
        if c.0 < 0 || c.1 < 0 || c.0 as usize >= w || c.1 as usize >= h {
            continue;
        }
        let proto = Prototype::Piece {
            descriptor: p.descriptor.clone(),
            yaw: (p.yaw / 15.0).round() * 15.0,
        };
        let Some(ix) = grammar.prototypes.iter().position(|q| *q == proto) else {
            return Err(format!(
                "grammar: owned placement `{}` is a `{}` at {}deg, which is not in the learned \
                 grammar — re-learn from a map that contains it.",
                p.id, p.descriptor, p.yaw
            ));
        };
        initial[c.1 as usize * w + c.0 as usize] = 1 << ix;
        owned_cells += 1;
    }

    let chosen = wfc::collapse_grid(w, h, &grammar.weights, &grammar.support, &initial, seed)
        .ok_or_else(|| {
            "grammar: no arrangement satisfies what you have pinned. The learned rules cannot tile \
             this region around those cells — free some of them, or extend the example so the solver \
             has more ways to join things up."
                .to_owned()
        })?;

    let mut placements = Vec::new();
    let mut free_cells = 0usize;
    for y in 0..h {
        for x in 0..w {
            let c = (x as i64, y as i64);
            let ix = chosen[y * w + x];
            let pinned = initial[y * w + x].count_ones() == 1;
            if pinned {
                continue;
            }
            free_cells += 1;
            let Prototype::Piece { descriptor, yaw } = &grammar.prototypes[ix] else {
                continue;
            };
            placements.push(Placed {
                id: next_id(),
                descriptor: descriptor.clone(),
                at: cell_centre(map, cell, c),
                yaw: *yaw,
                ..Placed::default()
            });
        }
    }

    Ok(Solved {
        placements,
        free_cells,
        owned_cells,
        grid: chosen,
        width: w,
        height: h,
    })
}


#[cfg(test)]
mod declared_tests {
    use super::*;
    use crate::descriptor::{Descriptor, Extent, SubCell, Subgrid};
    use crate::library::LIBRARY_VERSION;

    /// A 0.5 m post whose whole lattice presents `token`.
    fn post(id: &str, token: &str) -> Descriptor {
        let mut d = Descriptor {
            id: id.to_owned(),
            extent: Extent { footprint: Some((0.5, 0.5)), height: Some(0.5) },
            ..Default::default()
        };
        let div = crate::descriptor::divisions(&d, 1).expect("measured");
        let mut cells = Vec::new();
        for x in 0..div.0 {
            for y in 0..div.1 {
                for z in 0..div.2 {
                    cells.push(SubCell {
                        at: (x, y, z),
                        solid: true,
                        edge: Some(token.to_owned()),
                        anchor: None,
                    });
                }
            }
        }
        d.subgrid = Some(Subgrid { cells });
        d
    }

    fn lib(ds: Vec<Descriptor>) -> Library {
        Library { version: LIBRARY_VERSION, note: None, descriptors: ds }
    }

    /// **A library that declares nothing refuses**, and says what to do instead.
    ///
    /// The alternative — an empty grammar the solver then fills a room with — is the shape this
    /// module's own note warns about: it would look like the tool working.
    #[test]
    fn a_library_with_no_tokens_refuses_rather_than_generating_noise() {
        let plain = Descriptor {
            id: "crate_a".to_owned(),
            extent: Extent { footprint: Some((0.5, 0.5)), height: Some(0.5) },
            ..Default::default()
        };
        let err = declared(&lib(vec![plain]), 1, 0.5).expect_err("must refuse");
        assert!(err.contains("no descriptor"), "{err}");
        assert!(err.contains("Author tokens"), "{err}");
    }

    /// Tokens that agree may abut; tokens that disagree may not. Equality, not a compatibility table.
    #[test]
    fn only_matching_tokens_may_abut() {
        let g = declared(&lib(vec![post("stone", "stone"), post("glass", "glass")]), 1, 0.5)
            .expect("builds");
        let ix = |id: &str| {
            g.prototypes
                .iter()
                .position(|p| matches!(p, Prototype::Piece { descriptor, .. } if descriptor == id))
                .unwrap_or_else(|| panic!("no {id}"))
        };
        let (stone, glass) = (ix("stone"), ix("glass"));
        assert!(g.support[N][stone] & (1 << stone) != 0, "stone may meet stone");
        assert!(g.support[N][stone] & (1 << glass) == 0, "stone must not meet glass");
        // `Empty` is unconstrained in both directions, or the solver could never leave a gap.
        assert!(g.support[N][stone] & 1 != 0);
        assert!(g.support[N][0] & (1 << stone) != 0);
    }

    /// **Turning a symmetric tile produces the same tile.** Keeping both would spend two of the
    /// solver's thirty-two slots saying one thing.
    #[test]
    fn a_symmetric_tile_is_one_prototype_not_four() {
        let g = declared(&lib(vec![post("stone", "stone")]), 1, 0.5).expect("builds");
        assert_eq!(g.prototypes.len(), 2, "Empty plus one: {:?}", g.prototypes);
    }

    /// **Dedup is per descriptor.** Measured against the shipped site kit, where a global pass
    /// deleted `site/column` because it presents exactly the faces `site/wall` does — and a solver
    /// that can never place a column is not the kit the author authored.
    #[test]
    fn two_tiles_presenting_the_same_faces_are_still_two_tiles() {
        let g = declared(&lib(vec![post("wall", "stone"), post("column", "stone")]), 1, 0.5)
            .expect("builds");
        let ids: Vec<&str> = g
            .prototypes
            .iter()
            .filter_map(|p| match p {
                Prototype::Piece { descriptor, .. } => Some(descriptor.as_str()),
                Prototype::Empty => None,
            })
            .collect();
        assert!(ids.contains(&"wall") && ids.contains(&"column"), "{ids:?}");
    }

    /// **A kit whose tiles are not the grid's size refuses, and names each one with its size.**
    ///
    /// Measured on the shipped site kit, which is exactly this case at `cell = 1.0`: every tokened
    /// piece is some other size, so a solve would lay a 2.06 m doorway and a 0.22 m corner at the
    /// same 1 m spacing. Every declared adjacency would be satisfied and the geometry would
    /// interpenetrate — the shape of failure that looks like the tool working.
    #[test]
    fn tiles_that_are_not_the_grids_size_refuse_and_name_their_sizes() {
        // `post` builds 0.5 m pieces; ask for a 1 m grid.
        let err = declared(&lib(vec![post("stone", "stone")]), 1, 1.0).expect_err("must refuse");
        assert!(err.contains("wrong size"), "{err}");
        assert!(err.contains("`stone` is 0.5 x 0.5 m"), "{err}");
        assert!(err.contains("generate from the map instead"), "{err}");
    }

    /// Over the ceiling refuses **with the count**, so an author knows how far over they are.
    #[test]
    fn more_prototypes_than_the_domain_holds_refuses_with_the_count() {
        // Each post gets its own token, so nothing dedups across them and each contributes one.
        let many: Vec<Descriptor> = (0..MAX_PROTOTYPES + 2)
            .map(|i| post(&format!("p{i:03}"), &format!("t{i:03}")))
            .collect();
        let err = declared(&lib(many), 1, 0.5).expect_err("must refuse");
        assert!(err.contains(&MAX_PROTOTYPES.to_string()), "{err}");
        assert!(err.contains("narrow the kit"), "{err}");
    }

    /// The grammar feeds the SAME collapser the learned one does — one solver, two sources.
    #[test]
    fn a_declared_grammar_drives_the_shared_collapser() {
        let g = declared(&lib(vec![post("stone", "stone"), post("glass", "glass")]), 1, 0.5)
            .expect("builds");
        let n = g.prototypes.len();
        let initial = vec![(1u32 << n) - 1; 4 * 4];
        let out = crate::wfc::collapse_grid(4, 4, &g.weights, &g.support, &initial, 7)
            .unwrap_or_else(|| panic!("a fully permissive start must collapse"));
        assert_eq!(out.len(), 16);
        // Whatever it produced, every abutting pair is one the tokens allow — which is the property
        // the whole exercise is for.
        for z in 0..4usize {
            for x in 0..4usize {
                let p = out[z * 4 + x];
                if x + 1 < 4 {
                    let q = out[z * 4 + x + 1];
                    assert!(g.support[E][p] & (1 << q) != 0, "({x},{z}) east pair is not allowed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An interface presenting one token across the whole of every face.
    fn iface(token: Option<&str>) -> crate::composition::Interface {
        use crate::composition::Band;
        let band = Band {
            y: (0.0, 1.0),
            lat: (-0.5, 0.5),
            token: token.map(str::to_owned),
        };
        crate::composition::Interface {
            faces: [
                vec![band.clone()],
                vec![band.clone()],
                vec![band.clone()],
                vec![band],
            ],
            faults: Vec::new(),
        }
    }

    /// **The turns are what make a kit solvable**, measured on the shipped one.
    ///
    /// An ASSET-CONTRACT test: it reads the real site kit on purpose, because what it asserts IS a
    /// fact about what ships. FVS-R-7's "done when" is that this kit is solvable.
    ///
    /// **Before the four turns existed this failed, and the failure is the reason they do.** Every
    /// wall tile's north support was `Empty` and nothing else: a wall tile presents `wall` outward on
    /// one face, so the tile across that seam has to present `wall` back — and only the same tile
    /// turned 180 degrees does. With yaw 0 alone the kit expressed floor, empty, and no wall meeting
    /// anything. `tests/site_tiles.rs`'s `one_wall_tile_covers_four_orientations` is the authored
    /// kit's own statement of the same thing.
    #[test]
    fn the_shipped_site_kit_learns_a_grammar_and_solves() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/emerge/site");
        let Ok(lib_text) = std::fs::read_to_string(root.join("library.ron")) else {
            panic!("the shipped site kit must be readable");
        };
        let library: Library = ron::from_str(&lib_text).expect("the site library parses");
        let comps: crate::composition::Compositions =
            ron::from_str(&std::fs::read_to_string(root.join("compositions.ron")).expect("groups"))
                .expect("the site compositions parse");

        let composed =
            from_compositions(&comps.compositions, &library, 1, 1.0, crate::composition::agrees)
                .expect("the shipped kit learns");
        let Composed { grammar: g, skipped, faces } = composed;
        assert!(skipped.is_empty(), "every authored tile is one cell and bounded: {skipped:?}");
        // The interfaces come back alongside the prototypes, and `Empty` has none — which is what
        // `crate::range` reads to tell a wall face from an open one without learning the kit's tokens.
        assert_eq!(faces.len(), g.len(), "one interface slot per prototype");
        assert!(faces[0].is_none(), "Empty presents nothing");
        assert!(faces[1..].iter().all(|f| f.is_some()), "every tile prototype presents something");

        // Four authored tiles; three are asymmetric and contribute four turns each, and the floor is
        // symmetric so its four collapse to one. That count is the dedup rule working, not a
        // coincidence — assert the SHAPE rather than the number, so authoring a fifth tile does not
        // fail this for the wrong reason.
        let turns_of = |id: &str| {
            g.prototypes
                .iter()
                .filter(|p| matches!(p, Prototype::Piece { descriptor, .. } if descriptor == id))
                .count()
        };
        assert_eq!(turns_of("site/tile_floor"), 1, "a symmetric tile is one prototype, not four");
        assert_eq!(turns_of("site/tile_wall_n"), 4, "an asymmetric tile is four");

        // **The property the turns exist for**: a wall tile can have something other than `Empty`
        // beside it. Before them this was `0b1` — the empty prototype alone.
        let wall = g
            .prototypes
            .iter()
            .position(|p| matches!(p, Prototype::Piece { descriptor, yaw }
                if descriptor == "site/tile_wall_n" && *yaw == 0.0))
            .expect("the kit has a north wall tile");
        assert!(
            g.support[N][wall] & !1 != 0,
            "a wall tile whose only northern neighbour is Empty cannot make a room; support is \
             {:#b}",
            g.support[N][wall]
        );

        // **One tile, one unit of weight.** A symmetric tile dedupes to one prototype and an
        // asymmetric one keeps four, so a flat weight per prototype makes the symmetric tile a
        // quarter as likely — which is an artifact of the solver's expansion, not anything the
        // author said. Measured before this: `tile_floor` came out at 7.4% of 864 cells against
        // ~23% for each of the other three.
        let weight_of = |id: &str| -> f64 {
            g.prototypes
                .iter()
                .zip(g.weights.iter())
                .filter(|(p, _)| matches!(p, Prototype::Piece { descriptor, .. } if descriptor == id))
                .map(|(_, w)| *w)
                .sum()
        };
        let floor = weight_of("site/tile_floor");
        let wall = weight_of("site/tile_wall_n");
        assert!(
            (floor - wall).abs() < 1e-9,
            "a symmetric tile and an asymmetric one must carry the same total weight — floor \
             {floor}, wall {wall}"
        );
        assert!(
            (floor - 1.0).abs() < 1e-9,
            "and that total is one tile's worth, not one prototype's: {floor}"
        );

        // And a grid actually collapses.
        let map = crate::map::Map {
            name: "probe".into(),
            bounds: (6.0, 3.0, 6.0),
            ..crate::map::Map::default()
        };
        let mut n = 0;
        let solved = solve(&map, &g, 1.0, 42, || {
            n += 1;
            format!("g@{n}")
        })
        .expect("the shipped kit solves a 6x6 grid");
        assert_eq!(solved.free_cells, 36);
        assert!(
            solved.placements.len() > 20,
            "a solve that fills almost nothing is a grammar that cannot express a room: {} rows",
            solved.placements.len()
        );
    }

    /// **The control for FVS-R-9.** A room hand-assembled from the shipped kit's own prototypes,
    /// measured by `crate::range` and found enclosed.
    ///
    /// Without this, the measurement's headline result — *every solve scored enclosure 0* — has two
    /// explanations that look identical: the generator cannot close a boundary, or the metric cannot
    /// see one. This separates them. The kit CAN build a closed room; four corners and four walls are
    /// exactly what it has, and laying them by hand produces enclosure 1.
    ///
    /// So a zero from a solve is a statement about the solver, not about `range`.
    #[test]
    fn the_kit_can_build_a_room_the_metric_calls_enclosed() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/emerge/site");
        let library: Library =
            ron::from_str(&std::fs::read_to_string(root.join("library.ron")).expect("library"))
                .expect("the site library parses");
        let comps: crate::composition::Compositions =
            ron::from_str(&std::fs::read_to_string(root.join("compositions.ron")).expect("groups"))
                .expect("the site compositions parse");
        let Composed { grammar: g, faces, .. } =
            from_compositions(&comps.compositions, &library, 1, 1.0, crate::composition::agrees)
                .expect("the shipped kit learns");

        let ix = |id: &str, yaw: f32| -> usize {
            g.prototypes
                .iter()
                .position(|p| matches!(p, Prototype::Piece { descriptor, yaw: y }
                    if descriptor == id && (*y - yaw).abs() < 1e-6))
                .unwrap_or_else(|| panic!("the kit has {id} at {yaw} degrees"))
        };
        // Which turn walls which face is read from the kit, not assumed — see the alphabet the
        // `expressive_range` example prints.
        let (nw, ne, sw, se) = (
            ix("site/tile_corner_nw", 0.0),
            ix("site/tile_corner_nw", 270.0),
            ix("site/tile_corner_nw", 90.0),
            ix("site/tile_corner_nw", 180.0),
        );
        let (n_, e_, s_, w_) = (
            ix("site/tile_wall_n", 0.0),
            ix("site/tile_wall_n", 270.0),
            ix("site/tile_wall_n", 180.0),
            ix("site/tile_wall_n", 90.0),
        );
        let floor_ix = ix("site/tile_floor", 0.0);

        // A 5 x 5 room inside a 9 x 9 field of Empty.
        const D: usize = 9;
        let (lo, hi) = (2usize, 6usize);
        let mut grid = vec![0usize; D * D];
        for z in lo..=hi {
            for x in lo..=hi {
                let on_top = z == lo;
                let on_bottom = z == hi;
                let on_left = x == lo;
                let on_right = x == hi;
                grid[z * D + x] = match (on_top, on_bottom, on_left, on_right) {
                    (true, _, true, _) => nw,
                    (true, _, _, true) => ne,
                    (_, true, true, _) => sw,
                    (_, true, _, true) => se,
                    (true, ..) => n_,
                    (_, true, ..) => s_,
                    (_, _, true, _) => w_,
                    (_, _, _, true) => e_,
                    _ => floor_ix,
                };
            }
        }

        // **Is that room even legal?** This is the difference between "the solver is unlikely to build
        // one" and "the grammar cannot express one", and the two call for opposite fixes. Every
        // orthogonally adjacent pair in the hand-laid room must be permitted by the learned support.
        let mut illegal = Vec::new();
        for z in 0..D {
            for x in 0..D {
                let p = grid[z * D + x];
                for (dir, nx, nz) in [
                    (N, x as i64, z as i64 - 1),
                    (E, x as i64 + 1, z as i64),
                    (S, x as i64, z as i64 + 1),
                    (W, x as i64 - 1, z as i64),
                ] {
                    if nx < 0 || nz < 0 || nx as usize >= D || nz as usize >= D {
                        continue;
                    }
                    let q = grid[nz as usize * D + nx as usize];
                    if g.support[dir][p] & (1 << q) == 0 {
                        illegal.push(format!(
                            "{:?} may not sit {} of {:?}",
                            g.prototypes[q],
                            ["north", "east", "south", "west"][dir],
                            g.prototypes[p]
                        ));
                    }
                }
            }
        }
        assert!(
            illegal.is_empty(),
            "the kit's own room is not a legal arrangement under its own grammar, so no solve could \
             ever produce one: {illegal:#?}"
        );

        let f = crate::range::Faces::new(&faces, "wall", 0.5);
        let sealed = crate::range::measure(D, D, &grid, |p, d| f.wall(p, d), |p| f.floor(p), |p| {
            f.doorway(p)
        })
        .expect("a well-formed grid");
        assert_eq!(sealed.enclosure, 1.0, "every cell of a walled room is inside it");
        assert_eq!(sealed.regions, 1);
        assert_eq!(sealed.opening_density, Some(0.0), "no doors yet");

        // Swap one top-edge wall for the doorway tile: still closed, now with one opening.
        grid[lo * D + (lo + 2)] = ix("site/tile_doorway_n", 0.0);
        let with_door =
            crate::range::measure(D, D, &grid, |p, d| f.wall(p, d), |p| f.floor(p), |p| f.doorway(p))
                .expect("a well-formed grid");
        assert_eq!(with_door.enclosure, 1.0, "a doorway is part of the boundary, not a hole in it");
        assert_eq!(with_door.regions, 1);
        assert_eq!(with_door.opening_density, Some(1.0));

        // And knocking a wall out opens it, so the seal above is the walls' doing.
        grid[lo * D + (lo + 1)] = 0;
        let breached =
            crate::range::measure(D, D, &grid, |p, d| f.wall(p, d), |p| f.floor(p), |p| f.doorway(p))
                .expect("a well-formed grid");
        assert_eq!(breached.enclosure, 0.0);
        assert_eq!(breached.regions, 0);
    }

    /// A bounded one-cell composition with nothing in it — enough to carry an interface slot.
    fn empty_tile(id: &str) -> Composition {
        Composition {
            id: id.to_owned(),
            envelope: Envelope::Bounded { size: (1.0, 1.0, 1.0) },
            members: Vec::new(),
            locations: Vec::new(),
            note: None,
        }
    }

    /// **Swapping the rule changes the grammar** — driven through the learner, not just asserted of
    /// the default rule.
    ///
    /// If the face comparison were inline, as it is in `declared`, both calls below would produce
    /// the same `support` and this would fail. That is the invariant FVS-R-7 states, and it is what
    /// keeps the edge-versus-corner question (FVS-R-11) from gating anything: answering it later is
    /// passing a different argument.
    #[test]
    fn swapping_the_rule_changes_the_learned_grammar() {
        let comps = vec![empty_tile("tile_a"), empty_tile("tile_b")];
        let library = Library {
            version: crate::library::LIBRARY_VERSION,
            note: None,
            descriptors: Vec::new(),
        };

        let Composed { grammar: permissive, skipped, .. } =
            from_compositions(&comps, &library, 1, 1.0, |_, _, _| true).expect("learns");
        assert!(skipped.is_empty(), "both tiles are one cell and bounded: {skipped:?}");
        assert_eq!(permissive.len(), 3, "Empty plus the two tiles");

        let refusing = from_compositions(&comps, &library, 1, 1.0, |_, _, _| false)
            .expect("learns")
            .grammar;

        // `Empty` keeps its unconstrained role under BOTH rules — it is not routed through `agrees`,
        // because a grammar that cannot say "nothing goes here" cannot leave a doorway.
        assert_eq!(
            permissive.support[N][0], refusing.support[N][0],
            "Empty's row must not depend on the adjacency rule"
        );
        // The tiles' rows must, or the rule is not reaching the grammar at all.
        assert_ne!(
            permissive.support[N][1], refusing.support[N][1],
            "the rule is a parameter and this proves it: an inline comparison would give one answer"
        );
        assert_eq!(
            refusing.support[N][1] & 0b110,
            0,
            "a rule that refuses everything must leave no tile-to-tile support"
        );
    }

    /// **The adjacency rule is substitutable, which is the whole invariant.**
    ///
    /// FVS-R-7's stated non-negotiable: adjacency goes through `agrees`, never an inline face
    /// comparison, so the edge-versus-corner question cannot gate the grammar. Karth & Smith —
    /// *"any arbitrary adjacency validity function can be substituted here… without changing the
    /// WFC solver itself."*
    ///
    /// Proved by substituting one: the same tiles, learned twice, with a rule that refuses
    /// everything and a rule that permits everything. If the comparison were inline the two
    /// grammars would be identical, which is exactly what this asserts they are not.
    #[test]
    fn the_adjacency_rule_is_a_parameter_and_swapping_it_changes_the_grammar() {
        let wall = iface(Some("wall"));
        let floor = iface(None);

        // The shipped rule: a wall face and a nothing face disagree.
        assert!(crate::composition::agrees(&wall, &wall, N));
        assert!(crate::composition::agrees(&floor, &floor, N));
        assert!(
            !crate::composition::agrees(&wall, &floor, N),
            "`None` is a token in its own right and matches only `None`"
        );

        // A face with no bands has no seam to disagree about.
        let blank = crate::composition::Interface { faces: Default::default(), faults: Vec::new() };
        assert!(crate::composition::agrees(&blank, &wall, N));
    }

    fn map_with(bounds: (f32, f32, f32), pieces: &[(&str, f32, f32, f32, bool)]) -> Map {
        let mut m = Map {
            name: "example".into(),
            bounds,
            ..Map::default()
        };
        for (i, (d, x, z, yaw, owned)) in pieces.iter().enumerate() {
            m.placements.push(Placed {
                id: format!("p{i}"),
                descriptor: (*d).to_owned(),
                at: (*x, *z),
                yaw: *yaw,
                owned: *owned,
                owned_because: owned.then(|| "test".to_owned()),
                ..Placed::default()
            });
        }
        m
    }

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("g{n}")
        }
    }

    #[test]
    fn an_empty_map_teaches_only_emptiness() {
        let g = learn(&map_with((4.0, 3.0, 4.0), &[]), 1.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(g.prototypes, vec![Prototype::Empty]);
        // And a grammar of one thing is not something to continue.
        let err = solve(&map_with((4.0, 3.0, 4.0), &[]), &g, 1.0, 1, ids())
            .err()
            .unwrap_or_default();
        assert!(err.contains("nothing has been placed"), "{err}");
    }

    /// Yaw is part of the prototype: a wall turned 90 degrees meets different neighbours.
    #[test]
    fn the_same_piece_at_two_yaws_is_two_prototypes() {
        let m = map_with(
            (4.0, 3.0, 4.0),
            &[("wall", -1.5, -1.5, 0.0, false), ("wall", -0.5, -1.5, 90.0, false)],
        );
        let g = learn(&m, 1.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(g.len(), 3, "empty + two yaws: {:?}", g.prototypes);
    }

    /// A yaw a hair off is the same prototype — otherwise float noise multiplies the alphabet.
    #[test]
    fn yaw_is_quantised_so_float_noise_is_not_a_new_prototype() {
        let m = map_with(
            (4.0, 3.0, 4.0),
            &[("wall", -1.5, -1.5, 90.0, false), ("wall", -0.5, -1.5, 90.0001, false)],
        );
        assert_eq!(learn(&m, 1.0).unwrap_or_else(|e| panic!("{e}")).len(), 2);
    }

    /// **The point of the whole module**: what an author placed is what the solver may place, and an
    /// owned cell keeps exactly what they put there.
    #[test]
    fn an_owned_cell_survives_the_solve() {
        let m = map_with(
            (4.0, 3.0, 4.0),
            &[
                ("floor", -1.5, -1.5, 0.0, true),
                ("floor", -0.5, -1.5, 0.0, false),
                ("floor", -1.5, -0.5, 0.0, false),
            ],
        );
        let g = learn(&m, 1.0).unwrap_or_else(|e| panic!("{e}"));
        let s = solve(&m, &g, 1.0, 7, ids()).unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(s.owned_cells, 1);
        // The pinned cell is not re-emitted — it is already in the map, unchanged.
        assert!(
            !s.placements.iter().any(|p| p.at == (-1.5, -1.5)),
            "the owned cell was overwritten"
        );
        assert_eq!(s.free_cells, 15, "4x4 grid minus the one pinned cell");
    }

    /// Only what the author drew. A grammar learned from floors cannot produce a chair.
    #[test]
    fn the_solver_only_places_what_the_example_contains() {
        let m = map_with(
            (4.0, 3.0, 4.0),
            &[("floor", -1.5, -1.5, 0.0, false), ("floor", -0.5, -1.5, 0.0, false)],
        );
        let g = learn(&m, 1.0).unwrap_or_else(|e| panic!("{e}"));
        let s = solve(&m, &g, 1.0, 3, ids()).unwrap_or_else(|e| panic!("{e}"));
        for p in &s.placements {
            assert_eq!(p.descriptor, "floor", "invented a piece nobody placed");
        }
    }

    /// A map with one crate in it should not solve to a floor of crates. Empty meeting empty is what
    /// makes negative space expressible.
    #[test]
    fn empty_can_meet_empty_so_a_sparse_map_stays_sparse() {
        let m = map_with((6.0, 3.0, 6.0), &[("crate", 0.5, 0.5, 0.0, true)]);
        let g = learn(&m, 1.0).unwrap_or_else(|e| panic!("{e}"));
        let s = solve(&m, &g, 1.0, 11, ids()).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            s.placements.len() < s.free_cells,
            "every free cell was filled ({} of {}) — the grammar cannot express empty space",
            s.placements.len(),
            s.free_cells
        );
    }

    /// Same seed, same arrangement. The solver is part of an editor, and an author who presses the
    /// key twice on the same map should be able to tell whether anything changed.
    #[test]
    fn a_solve_is_reproducible_for_a_seed() {
        let m = map_with(
            (5.0, 3.0, 5.0),
            &[("floor", -1.5, -1.5, 0.0, false), ("wall", -0.5, -1.5, 90.0, false)],
        );
        let g = learn(&m, 1.0).unwrap_or_else(|e| panic!("{e}"));
        let a = solve(&m, &g, 1.0, 42, ids()).unwrap_or_else(|e| panic!("{e}"));
        let b = solve(&m, &g, 1.0, 42, ids()).unwrap_or_else(|e| panic!("{e}"));
        let key = |s: &Solved| -> Vec<(String, i64, i64)> {
            s.placements
                .iter()
                .map(|p| (p.descriptor.clone(), (p.at.0 * 100.0) as i64, (p.at.1 * 100.0) as i64))
                .collect()
        };
        assert_eq!(key(&a), key(&b));
    }

    /// An owned piece the grammar has never seen cannot be pinned to a prototype, and saying so beats
    /// dropping the pin and solving over the top of it.
    #[test]
    fn an_owned_piece_outside_the_grammar_is_refused() {
        let example = map_with((4.0, 3.0, 4.0), &[("floor", -1.5, -1.5, 0.0, false)]);
        let g = learn(&example, 1.0).unwrap_or_else(|e| panic!("{e}"));

        let other = map_with((4.0, 3.0, 4.0), &[("statue", -0.5, -0.5, 0.0, true)]);
        let err = solve(&other, &g, 1.0, 1, ids()).err().unwrap_or_default();
        assert!(err.contains("not in the learned grammar"), "{err}");
    }

    #[test]
    fn a_zero_cell_is_refused_rather_than_looping() {
        assert!(learn(&map_with((4.0, 3.0, 4.0), &[]), 0.0).is_err());
        assert!(learn(&map_with((4.0, 3.0, 4.0), &[]), f32::NAN).is_err());
    }

    /// The domain is a `u32`, so the alphabet has a hard ceiling and the error should name it rather
    /// than letting `collapse_grid`'s assert fire.
    #[test]
    fn too_many_prototypes_is_a_named_error() {
        let pieces: Vec<(String, f32, f32, f32, bool)> = (0..MAX_PROTOTYPES + 4)
            .map(|i| {
                (
                    format!("piece_{i}"),
                    -19.5 + i as f32,
                    -19.5,
                    0.0,
                    false,
                )
            })
            .collect();
        let refs: Vec<(&str, f32, f32, f32, bool)> = pieces
            .iter()
            .map(|(d, x, z, y, o)| (d.as_str(), *x, *z, *y, *o))
            .collect();
        let err = learn(&map_with((40.0, 3.0, 40.0), &refs), 1.0)
            .err()
            .unwrap_or_default();
        assert!(err.contains(&MAX_PROTOTYPES.to_string()), "{err}");
    }
}

// ---------------------------------------------------------------------------------------------
// The declared half
// ---------------------------------------------------------------------------------------------

/// **A grammar built from what tiles DECLARE, rather than from what an author has already drawn.**
///
/// [`learn`] answers *"more of what is here"* and cannot answer anything on an empty map — there are
/// no observed pairs to generalise from. This answers *"whatever the kit says may meet"*, which is
/// the other half of the same algorithm and needs no example at all.
///
/// # Two sources, one solver, and neither is a fallback for the other
///
/// Karth & Smith (*Addressing the Fundamental Tension of PCGML with Discriminative Learning*, FDG
/// 2019) name the distinction exactly. Gumin's WFC *"simply allows any tile-compatible overlapping
/// patterns to be placed adjacent to one another, even if they were never seen adjacent in the single
/// source image"* — they call this **Most General Generalization**, the inverse of learning from
/// observation. That is precisely what this function does over the token relation, and what [`learn`]
/// deliberately does not.
///
/// So the author picks a source and gets what that source can express. Neither silently substitutes
/// for the other: a library with no tokens produces no prototypes here and says so, rather than
/// quietly falling through to the learned grammar and generating something the author did not ask
/// for.
///
/// # Matching is equality, including the length of a face
///
/// `support[dir][p]` holds `q` when `p`'s `dir` face is *the same sequence* as `q`'s facing one —
/// Merrell & Manocha 2009 §4.3's rule, which [`crate::adjacency`] already applies to a finished map.
/// Two pieces whose lattices are different sizes present faces of different lengths, and those never
/// match. That is **conservative rather than permissive**: the pair is refused, not allowed, so this
/// cannot invent an adjacency the tokens do not state.
///
/// An unlabelled cell is `None`, and `None` equals only `None`. It is not a wildcard — which is what
/// lets a doorway's open middle meet only another open middle, and is why a kit with a single token

/// **A composition turned about its own anchor**, for deriving what it presents at that yaw.
///
/// Through `composition::rotate_xz` and `add_yaw` — the exact pair [`crate::composition::expand`]
/// turns a stamp by — so a tile's learned faces and the faces it actually stamps with cannot come
/// from two different conventions. `rotate_xz`'s doc names the cost of a second one: Bevy's yaw
/// turns +X toward −Z, and getting it backwards "mirrors every composition without failing
/// anything".
///
/// The envelope's x and z swap on a quarter turn, which matters for a non-square tile and is free
/// for the square ones a grid uses.
fn turn_composition(c: &Composition, yaw: f32) -> Composition {
    let mut out = c.clone();
    if let Envelope::Bounded { size } = out.envelope {
        let quarter = ((yaw / 90.0).round() as i32).rem_euclid(4);
        if quarter % 2 == 1 {
            out.envelope = Envelope::Bounded { size: (size.2, size.1, size.0) };
        }
    }
    for m in &mut out.members {
        m.at = crate::composition::rotate_xz(m.at, yaw);
        m.yaw = crate::composition::add_yaw(m.yaw, yaw);
    }
    out
}

/// **A grammar over compositions**, with adjacency behind a substitutable rule.
///
/// FVS-R-7. The sibling of [`learn`] and [`declared`]: where those read placements and descriptor
/// lattices, this reads the **composition set** — which is the unit this project made solvable, a
/// group of floor-plus-wall being a tile where the 0.1 x 1.0 m wall never could be.
///
/// # The seam is the point
///
/// `agrees` is a parameter, not a call. Karth & Smith: *"any arbitrary adjacency validity function
/// can be substituted here… without changing the WFC solver itself."* The default is
/// [`composition::agrees`]; passing another is how the edge-versus-corner question (FVS-R-11, still
/// blocked on Lagae & Dutré) gets answered later without touching this function, the solver, or the
/// prototypes.
///
/// # Only `Bounded` compositions
///
/// `composition::interface` returns `None` for an `Anchored` group — it claims no tile, so it has no
/// edge to present and a solver could not place it. Skipped by name in the report rather than
/// silently, on `declared`'s rule: a grammar quietly missing half the kit generates confidently and
/// wrongly.
///
/// # The weights are uniform, and that is a statement rather than a default
///
/// [`learn`] weights by how often the author used a piece. There is no such count here: a
/// composition set is a vocabulary, not an arrangement. Weighting by anything derivable from the set
/// itself — member count, footprint — would be inventing a frequency nobody expressed. **This is the
/// open half of FVS-R-7**, recorded on the backlog item: `site_67` yields three adjacency pairs, so
/// the map cannot supply them either.
/// A grammar over compositions, with what it refused and what each prototype presents.
///
/// # Why the interfaces come back out
///
/// A caller that wants to read a *solved grid* — [`crate::range`] measuring enclosure, an editor
/// reporting a seam fault — needs to know which faces of each prototype are walls. `Grammar` does not
/// carry that, and it cannot be re-derived from outside: the turned interface is produced by a private
/// `turn_composition`, so a second derivation would be a second rotation convention, which is exactly
/// the failure [`crate::composition::rotate_xz`]'s doc warns costs you a mirrored kit with nothing
/// going red. Returning what was already computed is one path; re-deriving it would be two.
#[derive(Debug)]
pub struct Composed {
    pub grammar: Grammar,
    /// Compositions that could not become tiles, each with the reason, by name. A grammar quietly
    /// missing half the kit generates confidently and wrongly — `declared`'s rule.
    pub skipped: Vec<String>,
    /// What each prototype presents, indexed alongside `grammar.prototypes`.
    ///
    /// Index 0 is [`Prototype::Empty`]'s, and it is `None`: empty presents nothing and may sit beside
    /// anything, which is the unconstrained role that lets the solver leave a gap.
    pub faces: Vec<Option<Interface>>,
}

pub fn from_compositions(
    compositions: &[Composition],
    library: &Library,
    // How finely the project divides a tile — `interface` reads faces off that lattice, and this is
    // the project's own number rather than one this function may choose.
    per_tile: u32,
    cell: f32,
    agrees: impl Fn(&Interface, &Interface, Dir) -> bool,
) -> Result<Composed, String> {
    if per_tile == 0 {
        return Err("composition grammar: the project divides each tile 0 ways".to_owned());
    }
    if !(cell.is_finite() && cell > 0.0) {
        return Err(format!("composition grammar: a cell of {cell} m is not a grid"));
    }

    // `Empty` is index 0 and permitted everywhere, exactly as in `learn` and `declared` — a grammar
    // that cannot say "nothing goes here" cannot leave a doorway.
    let mut prototypes = vec![Prototype::Empty];
    let mut interfaces: Vec<Option<Interface>> = vec![None];
    // `Empty` is one tile like any other: it is the grammar's way of saying "nothing goes here", and
    // weighting it above or below the authored tiles would be a claim about how holey a room should
    // be that nobody has made.
    let mut weights: Vec<f64> = vec![1.0];
    let mut skipped: Vec<String> = Vec::new();

    for c in compositions {
        let size = match c.envelope {
            Envelope::Bounded { size } => size,
            Envelope::Anchored => {
                skipped.push(format!("`{}` is anchored, so it presents no edge to match", c.id));
                continue;
            }
        };
        // **A tile grammar is a grammar over tiles of the grid's size** — `declared`'s rule, and the
        // same arithmetic: `solve` lays prototypes at `cell` centres, so a group that is not one cell
        // across is placed at a spacing with nothing to do with its extent.
        if (size.0 - cell).abs() > CELL_EPSILON || (size.2 - cell).abs() > CELL_EPSILON {
            skipped.push(format!(
                "`{}` is {:.2} x {:.2} m and the grid is {cell:.2} m, so it cannot be a tile",
                c.id, size.0, size.2
            ));
            continue;
        }
        // **Four turns per tile, and that is not an optimisation — it is what makes the kit
        // solvable at all.**
        //
        // Measured on the shipped site kit before this existed: every wall tile's north support was
        // `Empty` and nothing else. A wall tile presents `wall` outward on ONE face, so the tile
        // across that seam has to present `wall` back — and only the same tile turned 180 degrees
        // does. With yaw 0 alone the kit could express floor, empty, and no wall meeting anything.
        // `one_wall_tile_covers_four_orientations` is the authored kit's own statement of this: one
        // wall tile IS four, by rotation.
        //
        // **The turned interface is DERIVED, never rotated.** Turning the composition through
        // `rotate_xz` and `add_yaw` — the pair a stamp already turns by — and re-deriving the face
        // means there is no second rotation convention to get backwards. `rotate_xz`'s own doc names
        // that failure: a mirrored copy "mirrors every composition without failing anything".
        let mut seen_faces: Vec<[Vec<crate::composition::Band>; 4]> = Vec::new();
        let first_proto = prototypes.len();
        for quarter in 0..4u8 {
            let yaw = quarter as f32 * 90.0;
            let turned = turn_composition(c, yaw);
            let iface = match crate::composition::interface(&turned, compositions, library, per_tile)
            {
                Ok(Some(i)) => i,
                Ok(None) => {
                    if quarter == 0 {
                        skipped.push(format!("`{}` derives no interface", c.id));
                    }
                    continue;
                }
                Err(e) => return Err(format!("composition grammar: `{}`: {e}", c.id)),
            };
            // **Deduplicated within one composition only**, on `declared`'s rule and for its reason:
            // a symmetric tile presents the same four faces at 0 and at 180, and keeping both would
            // spend two of the solver's thirty-two saying one thing. Across compositions it would be
            // wrong — identical faces make two tiles interchangeable to the propagator, not the same
            // tile.
            if seen_faces.iter().any(|f| *f == iface.faces) {
                continue;
            }
            if prototypes.len() >= MAX_PROTOTYPES {
                return Err(format!(
                    "composition grammar: more than {MAX_PROTOTYPES} tiles once turned, which is \
                     what the solver packs a domain into. Narrow the set before solving."
                ));
            }
            seen_faces.push(iface.faces.clone());
            prototypes.push(Prototype::Piece { descriptor: c.id.clone(), yaw });
            interfaces.push(Some(iface));
        }
        // **One tile, one unit of weight — split across the turns it survived as.**
        //
        // Uniform per PROTOTYPE is not uniform per tile, and the difference is not a design choice:
        // a symmetric tile dedupes to one prototype while an asymmetric one keeps four, so a flat
        // 1.0 each makes the symmetric tile a quarter as likely as its neighbours. Measured on the
        // shipped kit before this: `tile_floor` is the only symmetric tile and came out at **7.4%**
        // of 864 cells against ~23% for each of the other three — open floor, the material a room is
        // mostly made of, was the rarest thing in the building.
        //
        // The dedup is right and stays; it is what keeps four tiles inside the solver's
        // thirty-two. This makes "uniform" mean uniform over the vocabulary the author wrote rather
        // than over the solver's expansion of it, which is the only reading anybody intended.
        let turns = prototypes.len() - first_proto;
        if turns > 0 {
            let share = 1.0 / turns as f64;
            for _ in 0..turns {
                weights.push(share);
            }
        }
    }

    let n = prototypes.len();
    let mut support: [Vec<u32>; 4] = [vec![0; n], vec![0; n], vec![0; n], vec![0; n]];
    for (p, pi) in interfaces.iter().enumerate() {
        for (q, qi) in interfaces.iter().enumerate() {
            for dir in [N, E, S, W] {
                // `Empty` presents nothing and may sit beside anything — the unconstrained role it
                // has in both siblings, and what lets the solver leave a gap.
                let ok = match (pi, qi) {
                    (None, _) | (_, None) => true,
                    (Some(a), Some(b)) => agrees(a, b, dir),
                };
                if ok {
                    support[dir][p] |= 1 << q;
                }
            }
        }
    }

    Ok(Composed {
        grammar: Grammar {
            prototypes,
            // Uniform per authored TILE — see the split above. `learn` counts placements; a
            // vocabulary has no count, so every tile the author wrote is equally likely.
            weights,
            support,
        },
        skipped,
        faces: interfaces,
    })
}

/// still expresses more than that token alone suggests.
///
/// # Prototypes are deduplicated by what they present
///
/// A wall whose lattice is symmetric presents the same four faces at 0° and at 180°, so those are one
/// prototype and not two. This is not an optimisation bolted on to fit the ceiling: turning a
/// symmetric tile produces the same tile, and keeping both would spend two of the solver's thirty-two
/// saying one thing. Measured on the shipped site kit: 8 tokened descriptors × 4 turns is 32
/// candidates, and 11 of them are distinct.
///
/// **Within one descriptor only.** Deduplicating across the library was tried and was wrong:
/// `site/column` presents exactly the faces `site/wall` does, so a global pass deleted the column and
/// the solver could never place one. Identical faces make two tiles interchangeable to the
/// propagator; they do not make them the same mesh.
pub fn declared(library: &Library, per_tile: u32, cell: f32) -> Result<Grammar, String> {
    if per_tile == 0 {
        return Err("declared grammar: the project divides each tile 0 ways".to_owned());
    }
    if !(cell.is_finite() && cell > 0.0) {
        return Err(format!("declared grammar: a cell of {cell} m is not a grid"));
    }
    let mut wrong_size: Vec<String> = Vec::new();
    // `Empty` is index 0 and permitted everywhere, exactly as in `learn` — a grammar that cannot say
    // "nothing goes here" cannot leave a doorway.
    let mut prototypes = vec![Prototype::Empty];
    let mut faces: Vec<[Vec<Option<String>>; 4]> = vec![Default::default()];

    for d in &library.descriptors {
        let Some(grid) = &d.subgrid else { continue };
        if !grid.cells.iter().any(|c| c.edge.is_some()) {
            continue;
        }
        // **A tile grammar is a grammar over tiles of the grid's size.**
        //
        // `solve` lays prototypes at `cell` centres, so a piece that is not one cell across is placed
        // at a spacing that has nothing to do with its extent. Measured on the shipped site kit at
        // `cell = 1.0`: `site/wall_doorway` is 0.46 x 2.06 m and would overlap its neighbour by about
        // a metre, while `site/wall_corner` at 0.22 x 0.22 m would leave three quarters of a metre of
        // gap — every declared adjacency satisfied, and geometry nobody can use.
        //
        // Merrell & Manocha's model synthesis and the Wang-tile family both assume a uniform tile;
        // this is that assumption stated instead of quietly violated. Collected and refused by name
        // below rather than skipped, because a grammar silently missing half the kit generates
        // something the author cannot account for.
        let fits = crate::descriptor::placed_footprint(d).is_some_and(|(w, dep)| {
            (w - cell).abs() <= CELL_EPSILON && (dep - cell).abs() <= CELL_EPSILON
        });
        if !fits {
            let size = crate::descriptor::placed_footprint(d)
                .map(|(w, dep)| format!("{w} x {dep} m"))
                .unwrap_or_else(|| "unmeasured".to_owned());
            wrong_size.push(format!("`{}` is {size}", d.id));
            continue;
        }
        let div = crate::descriptor::divisions(d, per_tile)?;
        // **Per descriptor, not across the library.** Deduplicating globally was measured and was
        // wrong: `site/column` presents exactly the faces `site/wall` does, so a global pass deleted
        // the column and the solver could never place one. Two tiles that present the same faces are
        // interchangeable to the *propagator* and are still two different meshes an author asked for.
        let mine = faces.len();
        for quarter in 0..4u8 {
            let turned = grid.rotated(quarter, div);
            let tdiv = crate::descriptor::rotate_div(div, quarter);
            let read = |dir: usize| -> Vec<Option<String>> {
                crate::adjacency::face(&turned, dir, tdiv)
                    .into_iter()
                    .map(|t| t.map(str::to_owned))
                    .collect()
            };
            let sig = [read(N), read(E), read(S), read(W)];
            // Same faces, same tile. Skipping here is what keeps a symmetric piece from spending two
            // of the solver's thirty-two slots saying one thing.
            if faces[mine..].iter().any(|f| *f == sig) {
                continue;
            }
            prototypes.push(Prototype::Piece {
                descriptor: d.id.clone(),
                yaw: quarter as f32 * 90.0,
            });
            faces.push(sig);
        }
    }

    if prototypes.len() == 1 {
        if !wrong_size.is_empty() {
            wrong_size.sort_unstable();
            return Err(format!(
                "declared grammar: every tokened piece is the wrong size for a {cell} m cell, so a \
                 solve would place them at a spacing unrelated to their extents — {}. A tile grammar \
                 needs tiles of the grid's size; generate from the map instead, or author a kit on \
                 the cell.",
                wrong_size.join(", ")
            ));
        }
        return Err(
            "declared grammar: no descriptor in this library carries an edge token, so there is \
             nothing to build a grammar from. Author tokens on the Tiles tab, or generate from the \
             map instead."
                .to_owned(),
        );
    }
    if prototypes.len() > MAX_PROTOTYPES {
        // Named and counted, never sampled: a generator that silently drops tiles produces output
        // that looks like the kit and is missing a third of it.
        return Err(format!(
            "declared grammar: {} prototypes, over the {MAX_PROTOTYPES} the solver's domain holds. \
             That is {} distinct turned tiles from {} tokened descriptor(s) — narrow the kit rather \
             than raising the cap, which is a `u32` the dungeon generator shares.",
            prototypes.len(),
            prototypes.len() - 1,
            library
                .descriptors
                .iter()
                .filter(|d| d
                    .subgrid
                    .as_ref()
                    .is_some_and(|g| g.cells.iter().any(|c| c.edge.is_some())))
                .count()
        ));
    }

    let n = prototypes.len();
    // Every prototype is equally likely. A declared grammar has no observations to count, and
    // inventing a weight would be a preference nobody stated.
    let weights = vec![1.0f64; n];
    let mut support: [Vec<u32>; 4] = [vec![0; n], vec![0; n], vec![0; n], vec![0; n]];
    for (p, pf) in faces.iter().enumerate() {
        for (q, qf) in faces.iter().enumerate() {
            for (dir, opposite) in [(N, S), (E, W), (S, N), (W, E)] {
                // `Empty` presents nothing and may sit beside anything — the same unconstrained role
                // it has in `learn`, and what lets the solver leave a gap.
                let ok = p == 0 || q == 0 || pf[dir] == qf[opposite];
                if ok {
                    support[dir][p] |= 1 << q;
                }
            }
        }
    }
    Ok(Grammar {
        prototypes,
        weights,
        support,
    })
}
