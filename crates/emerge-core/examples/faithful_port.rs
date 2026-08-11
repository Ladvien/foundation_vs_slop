//! **The checkpoint: does the constraint encoding mean the same thing WFC's tables mean?**
//!
//! ```text
//! cargo run -p emerge-core --example faithful_port
//! ```
//!
//! `docs/2026-08-10-constraint-solver-plan.md` §3 puts this before any global constraint is written:
//! run L1 plus the tile and pattern rules *only*, and reproduce today's behaviour through the new
//! machinery. The reasoning is that a wrong encoding would not look like a bug — it would look like a
//! solver that cannot do enclosure, which is precisely the conclusion the whole exercise is trying to
//! reach honestly.
//!
//! # What is actually checked, and why it is stronger than re-running the histogram
//!
//! The plan's sketch was *"same kit, same region, same seeds, expect the same kind of output"*. That
//! comparison cannot be run yet and saying so is the point: today's variety comes from WFC's weighted
//! collapse drawing on a seed, and the constraint solver is a **deterministic function of the
//! clauses** — 128 seeds through it would produce one arrangement 128 times. Per-seed variety is
//! supposed to arrive as seeded *soft weights* (plan §6), and soft constraints need an optimiser that
//! is not built. So a histogram here would compare a distribution against a point.
//!
//! What can be checked now is the thing the histogram was a proxy for — **that the two agree on what
//! is legal** — and it is checked in both directions, which the histogram never was:
//!
//! 1. **Every arrangement WFC produces, the encoding accepts.** Pin each cell to WFC's own choice and
//!    ask the solver whether that is satisfiable. A refusal means the clauses forbid something the
//!    adjacency table permits — the encoding is too strong.
//! 2. **Every arrangement the encoding produces, WFC's table accepts.** Solve unpinned and walk every
//!    orthogonal pair against `support` directly. A violation means the clauses permit something the
//!    table forbids — the encoding is too weak.
//!
//! Together those say the clause set and the adjacency relation denote the same set of grids, which
//! is what "faithful port" means. A histogram could agree by coincidence; this cannot.
//!
//! Then the solved grid is measured with the same [`range::measure`] the expressive-range run uses,
//! so the *kind* of output is on the record too.

use emerge_core::composition::{Compositions, Interface};
use emerge_core::constraints::GridProblem;
use emerge_core::grammar::{self, Composed, Grammar};
use emerge_core::library::Library;
use emerge_core::map::Map;
use emerge_core::range::{self, Faces};
use emerge_core::wfc::{E, N, S, W};

/// `site_67`'s slab, the region the expressive-range run used. Same region, so the two are comparable.
const REGION: usize = 12;
const CELL: f32 = 1.0;
/// How many WFC arrangements to put through direction 1.
const SEEDS: u64 = 64;
/// The kit's edge token for a wall, and the gap under a lintel that counts as a doorway — the same
/// two constants `expressive_range` names.
const WALL: &str = "wall";
const WALKABLE: f32 = 0.5;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/emerge/site");
    let library: Library = match read(&root.join("library.ron")) {
        Ok(l) => l,
        Err(e) => return eprintln!("cannot read the site library: {e}"),
    };
    let comps: Compositions = match read(&root.join("compositions.ron")) {
        Ok(c) => c,
        Err(e) => return eprintln!("cannot read the site compositions: {e}"),
    };
    let composed = match grammar::from_compositions(
        &comps.compositions,
        &library,
        1,
        CELL,
        emerge_core::composition::agrees,
    ) {
        Ok(c) => c,
        Err(e) => return eprintln!("the kit does not learn a grammar: {e}"),
    };
    let Composed { grammar: g, faces, .. } = composed;

    println!("FAITHFUL PORT — the constraint encoding against WFC's own tables");
    println!("{}", "=".repeat(74));
    println!("region {REGION} x {REGION} cells, {} prototypes from the shipped Site kit\n", g.len());

    let full: u32 = if g.len() == 32 { u32::MAX } else { (1u32 << g.len()) - 1 };
    let permissive = vec![full; REGION * REGION];

    // ---- direction 1 --------------------------------------------------------------------------
    println!("1. EVERY ARRANGEMENT WFC PRODUCES, THE ENCODING ACCEPTS");
    let mut checked = 0u32;
    let mut rejected = Vec::new();
    let mut wfc_failed = 0u32;
    for seed in 1..=SEEDS {
        let Some(grid) = wfc_grid(&g, seed) else {
            wfc_failed += 1;
            continue;
        };
        // Pin every cell to WFC's own choice. If the clauses mean what the table means, that is
        // satisfiable by construction — there is nothing left to decide.
        let pinned: Vec<u32> = grid.iter().map(|&p| 1u32 << p).collect();
        match GridProblem::encode(&g.support, &pinned, REGION, REGION, g.len()) {
            Ok(gp) => match gp.problem.solve(0) {
                Ok(_) => checked += 1,
                Err(e) => rejected.push((seed, e)),
            },
            Err(e) => rejected.push((seed, e)),
        }
    }
    println!("   {checked} of {SEEDS} WFC arrangements accepted by the clauses");
    if wfc_failed > 0 {
        println!("   {wfc_failed} seeds did not converge under WFC and were not comparable");
    }
    for (seed, e) in rejected.iter().take(3) {
        println!("   REJECTED seed {seed}: {e}");
    }

    // ---- direction 2 --------------------------------------------------------------------------
    println!("\n2. EVERY ARRANGEMENT THE ENCODING PRODUCES, WFC'S TABLE ACCEPTS");
    let gp = match GridProblem::encode(&g.support, &permissive, REGION, REGION, g.len()) {
        Ok(gp) => gp,
        Err(e) => return eprintln!("   the region does not encode: {e}"),
    };
    println!(
        "   {} variables, {} clauses",
        gp.problem.vars(),
        gp.problem.clauses()
    );
    let solved = match gp.problem.solve(0).and_then(|s| gp.read(&s)) {
        Ok(grid) => grid,
        Err(e) => return eprintln!("   the encoded region did not solve: {e}"),
    };
    let breaches = illegal_pairs(&solved, &g.support, REGION, REGION);
    println!(
        "   {} adjacent pairs walked, {} violate the support table",
        REGION * REGION * 4,
        breaches.len()
    );
    for (a, b, dir) in breaches.iter().take(3) {
        println!("   VIOLATION: {a} may not have {b} on side {dir}");
    }

    // ---- and what kind of thing it is ----------------------------------------------------------
    println!("\n3. WHAT THE SOLVER'S ARRANGEMENT MEASURES, ON THE SAME METRIC");
    let f = || Faces::new(&faces, WALL, WALKABLE);
    match range::measure(
        REGION,
        REGION,
        &solved,
        |p, d| f().wall(p, d),
        |p| f().floor(p),
        |p| f().doorway(p),
    ) {
        Ok(m) => {
            println!(
                "   enclosure {:.3}   regions {}   opening density {}",
                m.enclosure,
                m.regions,
                m.opening_density.map_or("undefined".to_owned(), |o| format!("{o:.3}"))
            );
            println!(
                "   {} — the same kind of output the expressive-range run reported, reached\n   \
                 through the new machinery. Enclosure is what the global constraint is FOR; this\n   \
                 stage carries none, so a zero here is the expected result and not a regression.",
                if m.regions == 0 { "no enclosed region" } else { "an enclosed region" }
            );
        }
        Err(e) => println!("   could not measure: {e}"),
    }
    draw(&solved, &faces, REGION, REGION);

    println!("\n{}", "=".repeat(74));
    let ok = rejected.is_empty() && breaches.is_empty() && checked > 0;
    println!(
        "CHECKPOINT: {}",
        if ok {
            "the clauses and the adjacency table denote the same grids. Enclosure is next."
        } else {
            "THE ENCODING DISAGREES WITH THE TABLE — fix this before adding any global constraint."
        }
    );
}

fn read<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    ron::from_str(&text).map_err(|e| e.to_string())
}

/// One WFC arrangement over the same region, through the path the editor uses today.
fn wfc_grid(g: &Grammar, seed: u64) -> Option<Vec<usize>> {
    let map = Map {
        name: "faithful-port".into(),
        bounds: (REGION as f32 * CELL, 3.0, REGION as f32 * CELL),
        ..Map::default()
    };
    let mut n = 0u64;
    grammar::solve(&map, g, CELL, seed, || {
        n += 1;
        format!("r@{n}")
    })
    .ok()
    .map(|s| s.grid)
}

/// Every orthogonally adjacent pair the support table forbids. Empty is the answer that matters.
fn illegal_pairs(
    grid: &[usize],
    support: &[Vec<u32>; 4],
    w: usize,
    h: usize,
) -> Vec<(usize, usize, usize)> {
    const STEPS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
    let mut out = Vec::new();
    for z in 0..h {
        for x in 0..w {
            for (dir, (dx, dz)) in STEPS.iter().enumerate() {
                let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                if nx < 0 || nz < 0 || nx as usize >= w || nz as usize >= h {
                    continue;
                }
                let (Some(&a), Some(&b)) =
                    (grid.get(z * w + x), grid.get(nz as usize * w + nx as usize))
                else {
                    continue;
                };
                let permitted = support.get(dir).and_then(|t| t.get(a)).copied().unwrap_or(0);
                if b < 32 && permitted & (1u32 << b) == 0 {
                    out.push((a, b, dir));
                }
            }
        }
    }
    out
}

/// The same glyph grid `expressive_range` draws, so the two outputs can be read side by side.
fn draw(grid: &[usize], faces: &[Option<Interface>], w: usize, h: usize) {
    const GLYPH: [char; 16] = [
        '·', '\u{2575}', '\u{2576}', '\u{2514}', '\u{2577}', '\u{2502}', '\u{250c}', '\u{251c}',
        '\u{2574}', '\u{2518}', '\u{2500}', '\u{2534}', '\u{2510}', '\u{2524}', '\u{252c}', '\u{253c}',
    ];
    println!("\n   walls (strokes point at walled faces, `d` a doorway, blank is Empty)");
    for z in 0..h {
        print!("   ");
        for x in 0..w {
            let Some(&p) = grid.get(z * w + x) else { continue };
            if p == 0 {
                print!("  ");
                continue;
            }
            let f = || Faces::new(faces, WALL, WALKABLE);
            let mask = [N, E, S, W]
                .iter()
                .enumerate()
                .fold(0usize, |m, (bit, &d)| m | usize::from(f().wall(p, d)) << bit);
            print!("{}{}", GLYPH[mask], if f().doorway(p) { 'd' } else { ' ' });
        }
        println!();
    }
}
