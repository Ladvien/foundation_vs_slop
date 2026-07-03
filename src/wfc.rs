//! Engine-free Wave Function Collapse generator for a coarse dungeon room graph.
//!
//! Each grid cell is a *room slot* that collapses to a "connection prototype": a room
//! with a Link or Wall on each of its four edges (N, E, S, W), or empty rock (all Wall).
//! A Link means a corridor crosses to that neighbour. Two neighbours are compatible only
//! when they agree on their shared edge — a Link must meet a Link, a Wall a Wall — so
//! corridors always join two rooms and never dead-end into rock. `dungeon` then expands
//! each slot into an actual room + corridors. The generator is deterministic for a given
//! seed and has no Bevy dependency.

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
fn build_prototypes() -> Vec<Prototype> {
    // (base link pattern [N,E,S,W], kind, weight-per-rotation)
    let bases: &[([bool; 4], CellKind, f64)] = &[
        ([false, false, false, false], CellKind::Solid, 6.0), // empty rock
        ([true, false, false, false], CellKind::Floor, 1.2),  // dead-end room (1 link)
        ([true, false, true, false], CellKind::Floor, 2.5),   // corridor-through (2, opposite)
        ([true, true, false, false], CellKind::Floor, 2.5),   // corner room (2, adjacent)
        ([true, true, true, false], CellKind::Floor, 1.2),    // tee (3 links)
        ([true, true, true, true], CellKind::Floor, 0.6),     // cross (4 links)
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

/// Small deterministic xorshift64 PRNG — keeps generation reproducible with zero deps.
/// Shared with `dungeon` (room sizing) so there is one PRNG implementation.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        // Force a non-zero state (xorshift is stuck at zero).
        Rng((seed ^ 0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
    /// Inclusive integer in `[lo, hi]`.
    pub(crate) fn range_usize(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }
}

/// Generate a dungeon grid. Retries on contradiction with an offset seed; panics
/// loudly if the (permissive) alphabet still fails to converge — there is one path,
/// and an unconvergeable generation is a bug to surface, not to paper over.
pub fn generate(width: usize, height: usize, seed: u64, max_attempts: u32) -> WfcResult {
    let protos = build_prototypes();
    assert!(protos.len() <= 32, "prototype set must fit in a u32 mask");
    let support = build_support(&protos);
    let full: u32 = if protos.len() == 32 {
        u32::MAX
    } else {
        (1u32 << protos.len()) - 1
    };

    for attempt in 0..max_attempts {
        if let Some(cells) = try_collapse(
            width,
            height,
            &protos,
            &support,
            full,
            seed.wrapping_add(attempt as u64),
        ) {
            return WfcResult {
                width,
                height,
                cells,
            };
        }
    }
    panic!("WFC failed to converge after {max_attempts} attempts (seed {seed})");
}

/// One collapse attempt. Returns `None` on contradiction so the caller can retry.
// The `b`/`dir` loops index by bit position / direction, which double as offset math.
#[allow(clippy::needless_range_loop)]
fn try_collapse(
    width: usize,
    height: usize,
    protos: &[Prototype],
    support: &[Vec<u32>; 4],
    full: u32,
    seed: u64,
) -> Option<Vec<CellData>> {
    let n = protos.len();
    let mut rng = Rng::new(seed);
    let mut cells = vec![full; width * height]; // per-cell mask of still-allowed prototypes

    loop {
        // Observe: find the undecided cell with the fewest remaining options.
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
        if ties.is_empty() {
            break; // everything collapsed
        }

        // Collapse the chosen cell to a single prototype, weighted by prototype weight.
        let chosen = ties[rng.below(ties.len())];
        let mask = cells[chosen];
        let total: f64 = (0..n)
            .filter(|&b| mask & (1 << b) != 0)
            .map(|b| protos[b].weight)
            .sum();
        let mut r = rng.next_f64() * total;
        let mut pick = usize::MAX;
        for b in 0..n {
            if mask & (1 << b) != 0 {
                r -= protos[b].weight;
                if r <= 0.0 {
                    pick = b;
                    break;
                }
            }
        }
        // Floating-point slack: fall through to the last allowed option.
        if pick == usize::MAX {
            pick = (0..n).rev().find(|&b| mask & (1 << b) != 0).unwrap();
        }
        cells[chosen] = 1 << pick;

        // Propagate the consequences to neighbours until the wave is consistent.
        let mut stack = vec![chosen];
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
                        return None; // contradiction
                    }
                    cells[ni] = new_mask;
                    stack.push(ni);
                }
            }
        }
    }

    let result = cells
        .iter()
        .map(|&mask| {
            let b = mask.trailing_zeros() as usize;
            CellData {
                kind: protos[b].kind,
                open: protos[b].open,
            }
        })
        .collect();
    Some(result)
}
