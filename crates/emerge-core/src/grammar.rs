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

use crate::map::{Map, Placed};
use crate::wfc;

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

/// The most prototypes the solver can carry — `collapse_grid` packs a domain into a `u32`.
pub const MAX_PROTOTYPES: usize = 32;

/// A learned grammar: what exists, how often, and what may sit beside what.
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
