//! WFC golden pin — `generate` is the deterministic substrate the whole dungeon is built on, so its
//! output is frozen per seed. This is the safest exact oracle in the suite (a pure function of the seed),
//! and it is the direct proof of "same seed ⇒ same world". A hash change here is a deliberate worldgen
//! change and needs human sign-off, never an auto-update.

use foundation_vs_slop::wfc::{generate, CellKind, WfcResult};

/// The shipped shape distribution (rock, dead_end, corridor, corner, tee, cross) — mirrors the values
/// the dungeon loads from `assets/dungeon.ron` and that the in-crate wfc tests use.
const WEIGHTS: [f64; 6] = [6.0, 1.2, 2.5, 2.5, 1.2, 0.6];
const MAX_ATTEMPTS: u32 = 40;

/// FNV-1a over a canonical byte encoding of the grid. Hand-rolled (no hasher crate, and `DefaultHasher`
/// is explicitly not stable across toolchains) so the golden is byte-stable and reviewable.
fn hash_layout(r: &WfcResult) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let feed = |b: u8, h: &mut u64| {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    feed(r.width as u8, &mut h);
    feed(r.height as u8, &mut h);
    for c in &r.cells {
        feed(if matches!(c.kind, CellKind::Floor) { 1 } else { 0 }, &mut h);
        let bits = (c.open[0] as u8) | (c.open[1] as u8) << 1 | (c.open[2] as u8) << 2 | (c.open[3] as u8) << 3;
        feed(bits, &mut h);
    }
    h
}

/// (width, height, seed) corpus — a spread of sizes and seeds known to converge.
const CORPUS: &[(usize, usize, u64)] = &[
    (8, 8, 0x5C0_9191),
    (12, 12, 1),
    (12, 12, 2),
    (16, 16, 0xABCD),
    (10, 14, 777),
];

/// Golden FNV-1a hashes, one per corpus entry, in order. Frozen from the current generator.
const GOLDEN: &[u64] = &[
    9894220316481528147,
    4526058043889762125,
    7488389918954999148,
    4385647085113489389,
    10106888218712299938,
];

#[test]
fn generate_layout_is_pinned_over_seed_corpus() {
    let got: Vec<u64> = CORPUS
        .iter()
        .map(|&(w, h, seed)| {
            let r = generate(w, h, seed, MAX_ATTEMPTS, &WEIGHTS);
            assert_eq!(r.cells.len(), w * h, "cell count for {w}x{h} seed {seed}");
            hash_layout(&r)
        })
        .collect();
    assert_eq!(got.as_slice(), GOLDEN, "WFC layout hashes changed (got {got:?})");
}

#[test]
fn generate_is_reproducible_in_process() {
    // Metamorphic: the same (size, seed) yields byte-identical output on repeat — the property the whole
    // replay backbone rests on.
    for &(w, h, seed) in CORPUS {
        let a = hash_layout(&generate(w, h, seed, MAX_ATTEMPTS, &WEIGHTS));
        let b = hash_layout(&generate(w, h, seed, MAX_ATTEMPTS, &WEIGHTS));
        assert_eq!(a, b, "generate not reproducible for {w}x{h} seed {seed}");
    }
}

#[test]
fn floor_links_only_ever_join_two_floors() {
    // Reachability-adjacent invariant: a Link (corridor) never points at rock. Because the Solid
    // prototype has no open edges, a Floor with an open edge must meet a Floor whose opposite edge is
    // also open — the module's "a Link always meets a Link" guarantee, checked on real output.
    let r = generate(12, 12, 42, MAX_ATTEMPTS, &WEIGHTS);
    let at = |x: usize, y: usize| r.cells[y * r.width + x];
    // (dir index N/E/S/W, dx, dy, opposite dir index)
    let dirs = [(0usize, 0i32, -1i32, 2usize), (1, 1, 0, 3), (2, 0, 1, 0), (3, -1, 0, 1)];
    for y in 0..r.height {
        for x in 0..r.width {
            let c = at(x, y);
            for &(dir, dx, dy, opp) in &dirs {
                if !c.open[dir] {
                    continue;
                }
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                assert!(
                    nx >= 0 && ny >= 0 && (nx as usize) < r.width && (ny as usize) < r.height,
                    "cell ({x},{y}) links off-grid in dir {dir}"
                );
                let n = at(nx as usize, ny as usize);
                assert!(matches!(n.kind, CellKind::Floor), "cell ({x},{y}) links into rock");
                assert!(n.open[opp], "cell ({x},{y}) link not reciprocated by neighbour");
            }
        }
    }
}
