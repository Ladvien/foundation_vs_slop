//! Engine-free Wave Function Collapse generator for a coarse dungeon room graph.
//!
//! Each grid cell is a *room slot* that collapses to a "connection prototype": a room
//! with a Link or Wall on each of its four edges (N, E, S, W), or empty rock (all Wall).
//! A Link means a corridor crosses to that neighbour. Two neighbours are compatible only
//! when they agree on their shared edge — a Link must meet a Link, a Wall a Wall — so
//! corridors always join two rooms and never dead-end into rock. `dungeon` then expands
//! each slot into an actual room + corridors. The generator is deterministic for a given
//! seed and has no Bevy dependency.

use crate::rng::{seeded, DetRng};

/// Direction indices into every `[_; 4]` edge array: North, East, South, West.
pub const N: usize = 0;
pub const E: usize = 1;
pub const S: usize = 2;
pub const W: usize = 3;

#[inline]
fn opposite(dir: usize) -> usize {
    match dir {
        N => S,
        S => N,
        E => W,
        W => E,
        _ => unreachable!(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellKind {
    /// A room slot (expands into a room).
    Floor,
    /// Empty rock (the negative space between rooms).
    Solid,
}

/// The collapsed result for one room slot.
#[derive(Clone, Copy, Debug)]
pub struct CellData {
    pub kind: CellKind,
    /// Link (corridor crosses this edge) vs wall, indexed by [`N`]/[`E`]/[`S`]/[`W`].
    /// `true` = a corridor connects to that neighbour.
    pub open: [bool; 4],
}

/// A fully collapsed grid, row-major (`cells[y * width + x]`).
pub struct WfcResult {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<CellData>,
}

#[derive(Clone, Copy, Debug)]
struct Prototype {
    kind: CellKind,
    open: [bool; 4],
    weight: f64,
}

/// Rotate an edge pattern 90° clockwise: the socket now on edge `j` came from `j-1`.
fn rotate_cw(open: [bool; 4]) -> [bool; 4] {
    [open[3], open[0], open[1], open[2]]
}

/// The room-slot alphabet: empty rock, plus a room for every distinct rotation of
/// dead-end / corridor / corner / tee / cross connection patterns (an edge's `true`
/// means a corridor links to that neighbour). Weights bias toward 1–2 link rooms plus
/// a healthy share of rock, giving distinct rooms and corridors with negative space.
fn build_prototypes(base: &[f64; 6]) -> Vec<Prototype> {
    // (base link pattern [N,E,S,W], kind, weight-per-rotation). Weights come from
    // `DungeonConfig.wfc_weights` (order: rock, dead_end, corridor, corner, tee, cross) so the
    // dungeon's shape distribution is data-driven via `assets/dungeon.ron`, not hardcoded here.
    let bases: &[([bool; 4], CellKind, f64)] = &[
        ([false, false, false, false], CellKind::Solid, base[0]), // empty rock
        ([true, false, false, false], CellKind::Floor, base[1]),  // dead-end room (1 link)
        ([true, false, true, false], CellKind::Floor, base[2]),   // corridor-through (2, opposite)
        ([true, true, false, false], CellKind::Floor, base[3]),   // corner room (2, adjacent)
        ([true, true, true, false], CellKind::Floor, base[4]),    // tee (3 links)
        ([true, true, true, true], CellKind::Floor, base[5]),     // cross (4 links)
    ];

    let mut protos: Vec<Prototype> = Vec::new();
    for &(open, kind, weight) in bases {
        let mut cur = open;
        for _ in 0..4 {
            if !protos.iter().any(|p| p.open == cur && p.kind == kind) {
                protos.push(Prototype { kind, open: cur, weight });
            }
            cur = rotate_cw(cur);
        }
    }
    protos
}

/// `support[dir][a]` = bitmask of prototypes that may sit on the `dir` side of
/// prototype `a` (i.e. whose opposite edge agrees with `a`'s `dir` edge).
fn build_support(protos: &[Prototype]) -> [Vec<u32>; 4] {
    let n = protos.len();
    let mut support = [vec![0u32; n], vec![0u32; n], vec![0u32; n], vec![0u32; n]];
    for (dir, table) in support.iter_mut().enumerate() {
        for (a, proto_a) in protos.iter().enumerate() {
            let mut mask = 0u32;
            for (b, proto_b) in protos.iter().enumerate() {
                if proto_a.open[dir] == proto_b.open[opposite(dir)] {
                    mask |= 1 << b;
                }
            }
            table[a] = mask;
        }
    }
    support
}

/// Per-cell initial domain (a CSP *unary* constraint; Karth & Smith 2017). Every boundary cell forbids
/// any prototype whose off-grid edge is a Link, so a corridor can never point into the void — restoring
/// the module invariant (a Link always meets a Link) at the map edge. Local propagation alone cannot
/// enforce this: it skips off-grid neighbours (there is nothing there to disagree with). Rock (all-Wall)
/// always survives, so no boundary cell can be emptied — this constraint can never itself contradict.
fn boundary_initial(protos: &[Prototype], width: usize, height: usize) -> Vec<u32> {
    let n = protos.len();
    let full: u32 = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
    let mut initial = vec![full; width * height];
    for y in 0..height {
        for x in 0..width {
            // Which of this cell's four edges point off the grid (indexed N/E/S/W)?
            let off = [y == 0, x + 1 == width, y + 1 == height, x == 0];
            let mut mask = full;
            for (b, proto) in protos.iter().enumerate() {
                if (0..4).any(|dir| off[dir] && proto.open[dir]) {
                    mask &= !(1u32 << b);
                }
            }
            initial[y * width + x] = mask;
        }
    }
    initial
}

/// Generate a dungeon grid. Retries on contradiction with an offset seed; panics
/// loudly if the (permissive) alphabet still fails to converge — there is one path,
/// and an unconvergeable generation is a bug to surface, not to paper over. (This is the *substrate*
/// pass; the placement-grammar furniture pass degrades to `Outcome::Partial` instead — see
/// `crate::placement::solvers::wfc`.) `base_weights` is the shape distribution (rock, dead_end,
/// corridor, corner, tee, cross) from `assets/dungeon.ron`.
pub fn generate(
    width: usize,
    height: usize,
    seed: u64,
    max_attempts: u32,
    base_weights: &[f64; 6],
) -> WfcResult {
    let protos = build_prototypes(base_weights);
    let weights: Vec<f64> = protos.iter().map(|p| p.weight).collect();
    let support = build_support(&protos);
    let initial = boundary_initial(&protos, width, height);

    for attempt in 0..max_attempts {
        if let Some(picks) = collapse_grid(
            width,
            height,
            &weights,
            &support,
            &initial,
            seed.wrapping_add(attempt as u64),
        ) {
            let cells = picks
                .iter()
                .map(|&b| CellData {
                    kind: protos[b].kind,
                    open: protos[b].open,
                })
                .collect();
            return WfcResult {
                width,
                height,
                cells,
            };
        }
    }
    panic!("WFC failed to converge after {max_attempts} attempts (seed {seed})");
}

/// Observe step (min-entropy / MRV): scan the domains and return the undecided cells with the fewest
/// remaining options. `None` = a contradiction (some domain has emptied); `Some(empty)` = every cell is
/// collapsed (done); `Some(ties)` = the lowest-entropy cells to choose among. Draws no RNG, and knows
/// nothing about topology or alphabet size — shared by `collapse_grid` and `collapse_graph` so both
/// observe through one code path. Note the raw `count_ones` comparison biases toward smaller-alphabet
/// cells; that is a deliberate quality choice and must stay unchanged (the grid golden depends on it).
fn observe_min_entropy(cells: &[u32]) -> Option<Vec<usize>> {
    let mut best_count = u32::MAX;
    let mut ties: Vec<usize> = Vec::new();
    for (i, &mask) in cells.iter().enumerate() {
        let count = mask.count_ones();
        if count == 0 {
            return None; // contradiction
        }
        if count > 1 {
            if count < best_count {
                best_count = count;
                ties.clear();
                ties.push(i);
            } else if count == best_count {
                ties.push(i);
            }
        }
    }
    Some(ties)
}

/// Collapse one cell's domain `mask` to a single prototype, chosen weighted by `weights[b]` over the set
/// bits, drawing `rng.unit()` exactly once. Returns the chosen bit index, or `None` in the (FP-slack,
/// otherwise unreachable) case that every option was pruned — a contradiction the caller retries.
/// Shared by both collapsers; keeping the single `unit()` draw here preserves the RNG draw order (the
/// caller draws the tie-break `below` first, then this).
fn collapse_one(mask: u32, weights: &[f64], rng: &mut impl DetRng) -> Option<usize> {
    let n = weights.len();
    let total: f64 = (0..n).filter(|&b| mask & (1 << b) != 0).map(|b| weights[b]).sum();
    let mut r = rng.unit() * total;
    let mut pick = usize::MAX;
    for b in 0..n {
        if mask & (1 << b) != 0 {
            r -= weights[b];
            if r <= 0.0 {
                pick = b;
                break;
            }
        }
    }
    // Floating-point slack: fall through to the last allowed option. `?` returns None (a contradiction
    // the caller retries) in the unreachable case that every option was pruned.
    if pick == usize::MAX {
        pick = (0..n).rev().find(|&b| mask & (1 << b) != 0)?;
    }
    Some(pick)
}

/// Generic grid Wave Function Collapse over an arbitrary prototype alphabet, returning the chosen
/// prototype **index** per cell (row-major), or `None` on contradiction so the caller can retry.
///
/// This is the reusable core (Karth & Smith 2017: WFC *is* finite-domain constraint solving). The
/// dungeon room-graph builder above and the placement `WfcSolver` both drive it — the only difference
/// is the alphabet: `weights[p]` is prototype `p`'s selection weight, and `support[dir][p]` is the
/// bitmask of prototypes that may legally sit on the `dir` (N/E/S/W) side of `p`. `initial[c]` is cell
/// `c`'s starting domain (all bits set = fully permissive; a narrowed mask is a unary constraint, made
/// arc-consistent before the first observe — e.g. the boundary rule from `boundary_initial`).
// The `b`/`dir` loops index by bit position / direction, which double as offset math.
#[allow(clippy::needless_range_loop)]
pub fn collapse_grid(
    width: usize,
    height: usize,
    weights: &[f64],
    support: &[Vec<u32>; 4],
    initial: &[u32],
    seed: u64,
) -> Option<Vec<usize>> {
    let n = weights.len();
    assert!(n <= 32, "prototype set must fit in a u32 mask");
    assert_eq!(initial.len(), width * height, "initial domain size mismatch");
    let mut rng = seeded(seed);
    // Start from the caller's per-cell domain (all-`full` = fully permissive; a narrowed `initial` is a
    // unary constraint, e.g. the boundary rule). Make it arc-consistent before observing so a restricted
    // start behaves exactly like a mid-run restriction. No-op when every cell is full (empty worklist).
    let mut cells = initial.to_vec();
    let seed_stack: Vec<usize> = (0..cells.len())
        .filter(|&i| (cells[i].count_ones() as usize) < n)
        .collect();
    if !propagate(&mut cells, width, height, support, seed_stack) {
        return None; // the initial domains are already inconsistent
    }

    loop {
        // Observe the min-entropy cell(s), then collapse one — shared with `collapse_graph`. The RNG
        // draw order is load-bearing: the tie-break `below` here, then the single `unit()` inside
        // `collapse_one`. `?` on either step surfaces a contradiction as `None` for the caller to retry.
        let ties = observe_min_entropy(&cells)?;
        if ties.is_empty() {
            break; // everything collapsed
        }
        let chosen = ties[rng.below(ties.len())];
        let pick = collapse_one(cells[chosen], weights, &mut rng)?;
        cells[chosen] = 1 << pick;

        // Propagate the collapse to a fixed point; an emptied neighbour domain is a contradiction.
        if !propagate(&mut cells, width, height, support, vec![chosen]) {
            return None;
        }
    }

    let result = cells
        .iter()
        .map(|&mask| mask.trailing_zeros() as usize)
        .collect();
    Some(result)
}

/// Arc-consistency propagation from a worklist of just-narrowed cells until the wave is consistent.
/// Returns `false` on contradiction (a neighbour's domain emptied). Shared by the initial-domain pass
/// and every post-collapse step, so both narrow through one code path (Karth & Smith 2017).
#[allow(clippy::needless_range_loop)]
fn propagate(
    cells: &mut [u32],
    width: usize,
    height: usize,
    support: &[Vec<u32>; 4],
    mut stack: Vec<usize>,
) -> bool {
    while let Some(ci) = stack.pop() {
        let cx = (ci % width) as i32;
        let cy = (ci / width) as i32;
        let cmask = cells[ci];
        for dir in 0..4 {
            let (nx, ny) = match dir {
                N => (cx, cy - 1),
                E => (cx + 1, cy),
                S => (cx, cy + 1),
                W => (cx - 1, cy),
                _ => unreachable!(),
            };
            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                continue;
            }
            let ni = ny as usize * width + nx as usize;

            // Which prototypes can still legally sit on this neighbour?
            let mut allowed = 0u32;
            let mut m = cmask;
            while m != 0 {
                let a = m.trailing_zeros() as usize;
                m &= m - 1;
                allowed |= support[dir][a];
            }
            let new_mask = cells[ni] & allowed;
            if new_mask != cells[ni] {
                if new_mask == 0 {
                    return false; // contradiction
                }
                cells[ni] = new_mask;
                stack.push(ni);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_compatible_alphabet_always_collapses() {
        let n = 3;
        let full = (1u32 << n) - 1;
        let support = [vec![full; n], vec![full; n], vec![full; n], vec![full; n]];
        let weights = vec![1.0, 1.0, 1.0];
        let picks =
            collapse_grid(4, 4, &weights, &support, &[full; 16], 42).expect("all-compatible must collapse");
        assert_eq!(picks.len(), 16);
        assert!(picks.iter().all(|&p| p < n));
    }

    #[test]
    fn forbidding_every_neighbour_contradicts() {
        // Support that allows no prototype beside any other → any multi-cell grid contradicts.
        let n = 2;
        let full = (1u32 << n) - 1;
        let support = [vec![0u32; n], vec![0u32; n], vec![0u32; n], vec![0u32; n]];
        let weights = vec![1.0, 1.0];
        assert!(collapse_grid(2, 1, &weights, &support, &[full; 2], 1).is_none());
    }

    #[test]
    fn collapse_is_deterministic_for_a_seed() {
        let n = 4;
        let full = (1u32 << n) - 1;
        let support = [vec![full; n], vec![full; n], vec![full; n], vec![full; n]];
        let weights = vec![1.0, 2.0, 0.5, 1.5];
        let initial = [full; 25];
        let a = collapse_grid(5, 5, &weights, &support, &initial, 123);
        let b = collapse_grid(5, 5, &weights, &support, &initial, 123);
        assert_eq!(a, b);
    }

    // Phase 3 Step-0 golden: locks the EXACT observe→below→collapse→unit draw sequence so the Step-2
    // shared-helper extraction (`observe_min_entropy`/`collapse_one`) stays byte-identical for BOTH
    // `collapse_grid` callers (the dungeon coarse grid and the furniture solver). All-`full` support
    // isolates the collapse draw path — the exact piece the refactor moves. Captured pre-refactor.
    const GOLDEN_COLLAPSE_GRID: [usize; 25] = [
        3, 1, 0, 1, 1, 2, 1, 2, 1, 2, 1, 3, 0, 3, 0, 2, 1, 1, 0, 1, 2, 1, 3, 1, 2,
    ];

    #[test]
    fn golden_collapse_grid_draw_order_is_stable() {
        let n = 4;
        let full = (1u32 << n) - 1;
        let support = [vec![full; n], vec![full; n], vec![full; n], vec![full; n]];
        let weights = vec![1.0, 2.0, 0.5, 1.5];
        let initial = [full; 25];
        let picks = collapse_grid(5, 5, &weights, &support, &initial, 123).expect("collapses");
        println!("GOLDEN_COLLAPSE_GRID = {picks:?}");
        assert_eq!(picks.as_slice(), &GOLDEN_COLLAPSE_GRID, "collapse_grid draw order changed");
    }

    #[test]
    fn room_graph_still_generates_floors() {
        let r = generate(9, 9, 0x5C0_9191, 20, &[6.0, 1.2, 2.5, 2.5, 1.2, 0.6]);
        assert_eq!(r.cells.len(), 81);
        assert!(r.cells.iter().any(|c| matches!(c.kind, CellKind::Floor)));
    }

    #[test]
    fn boundary_links_never_point_off_grid() {
        // The initial-domain rule (`boundary_initial`) forbids any off-grid Link, so no corridor ever
        // dead-ends into the void — the module's stated invariant, now enforced at the map edge.
        let r = generate(6, 6, 0x5C0_9191, 20, &[6.0, 1.2, 2.5, 2.5, 1.2, 0.6]);
        for y in 0..r.height {
            for x in 0..r.width {
                let c = r.cells[y * r.width + x];
                assert!(!(y == 0 && c.open[N]), "cell ({x},{y}) links N off-grid");
                assert!(!(x + 1 == r.width && c.open[E]), "cell ({x},{y}) links E off-grid");
                assert!(!(y + 1 == r.height && c.open[S]), "cell ({x},{y}) links S off-grid");
                assert!(!(x == 0 && c.open[W]), "cell ({x},{y}) links W off-grid");
            }
        }
    }

    #[test]
    fn initial_domains_restrict_output() {
        // A narrowed initial domain is honored: pinning cell 0 to prototype 2 must yield it there.
        let n = 3;
        let full = (1u32 << n) - 1;
        let support = [vec![full; n], vec![full; n], vec![full; n], vec![full; n]];
        let weights = vec![1.0, 1.0, 1.0];
        let mut initial = [full; 9];
        initial[0] = 1 << 2;
        let picks = collapse_grid(3, 3, &weights, &support, &initial, 7).expect("must collapse");
        assert_eq!(picks[0], 2, "initial domain must pin cell 0 to prototype 2");
    }
}
