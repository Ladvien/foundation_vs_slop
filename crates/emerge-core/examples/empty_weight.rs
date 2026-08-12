//! **FVS-R-17: does `Empty`'s weight explain why nothing closes?**
//!
//! ```text
//! cargo run -p emerge-core --example empty_weight
//! ```
//!
//! `expressive_range` measured 128 solves and found **zero enclosed regions**, with `Empty` taking
//! 37.58% of cells against a 20% ask. It is the one prototype compatible with every neighbour — that
//! is deliberate, since a grammar that cannot say "nothing goes here" cannot leave a doorway — so
//! propagation never eliminates it while every wall turn is eliminated somewhere.
//!
//! This sweeps `Empty`'s weight and re-reads **the same pre-registered rows**. That is the legitimate
//! act, not tuning: §4's thresholds were committed before any solve existed, the standing "do not add
//! authored weights before FVS-R-9 runs" expired when it ran, and every column below is judged against
//! numbers nobody may change after seeing them.
//!
//! It changes nothing. `Grammar::weights` is public, so the sweep sets `weights[0]` on a grammar it
//! built itself; the shipped `from_compositions` still gives `Empty` one whole unit.

use emerge_core::composition::{Compositions, Interface};
use emerge_core::grammar::{self, Composed, Grammar};
use emerge_core::library::Library;
use emerge_core::map::Map;
use emerge_core::range::{self, Faces, Measured, BINS, OPENING_MAX, RANGES};

const REGION: usize = 12;
const CELL: f32 = 1.0;
/// Matches the run `expressive_range` stabilised at, so the columns are comparable to it.
const SOLVES: u64 = 128;
const WALL: &str = "wall";
const WALKABLE: f32 = 0.5;
/// §4.5's committed rows.
const ENTROPY_FLOOR: f32 = 0.25;
const MAX_BIN_CEILING: f32 = 0.50;
const NON_CONVERGENCE_GATE: f32 = 0.20;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/emerge/site");
    let read = |name: &str| std::fs::read_to_string(root.join(name)).map_err(|e| e.to_string());
    let library: Library = match read("library.ron").and_then(|t| ron::from_str(&t).map_err(|e| e.to_string())) {
        Ok(l) => l,
        Err(e) => return eprintln!("cannot read the site library: {e}"),
    };
    let comps: Compositions = match read("compositions.ron").and_then(|t| ron::from_str(&t).map_err(|e| e.to_string())) {
        Ok(c) => c,
        Err(e) => return eprintln!("cannot read the site compositions: {e}"),
    };
    let Composed { grammar: base, faces, .. } = match grammar::from_compositions(
        &comps.compositions,
        &library,
        1,
        CELL,
        emerge_core::composition::agrees,
    ) {
        Ok(c) => c,
        Err(e) => return eprintln!("the kit does not learn a grammar: {e}"),
    };

    let authored: f64 = base.weights.iter().skip(1).sum();
    println!("EMPTY WEIGHT SWEEP — {SOLVES} solves each, {REGION} x {REGION} cells");
    println!("{}", "=".repeat(96));
    println!(
        "the four authored tiles carry {authored:.2} between them; `Empty` ships at {:.2}",
        base.weights[0]
    );
    println!(
        "\n{:>7} {:>9} {:>9} {:>8} {:>9} {:>8} {:>8}  {}",
        "w(empty)", "empty%", "med encl", "regions", "med open", "H/lnK", "maxbin", "rows that fire"
    );
    println!("{}", "-".repeat(96));

    for w in [1.0f64, 0.75, 0.5, 0.25, 0.1, 0.05, 0.0] {
        let g = Grammar {
            prototypes: base.prototypes.clone(),
            weights: {
                let mut ws = base.weights.clone();
                ws[0] = w;
                ws
            },
            support: base.support.clone(),
        };
        let r = run(&g, &faces);

        let mut fires: Vec<&str> = Vec::new();
        if r.failed as f32 / SOLVES as f32 > NON_CONVERGENCE_GATE {
            fires.push("3(GATE)");
        }
        if r.med_enclosure.is_some_and(|m| m < 0.15) {
            fires.push("1");
        }
        if r.med_enclosure.is_some_and(|m| m > 0.95) && r.med_opening.is_some_and(|o| o < 0.5) {
            fires.push("2");
        }
        if r.entropy < ENTROPY_FLOOR {
            fires.push("4a");
        }
        if r.max_bin > MAX_BIN_CEILING {
            fires.push("4b");
        }

        println!(
            "{:>7.2} {:>8.1}% {:>9} {:>8} {:>9} {:>8.3} {:>7.1}%  {}",
            w,
            r.empty_share * 100.0,
            r.med_enclosure.map_or("—".to_owned(), |m| format!("{m:.3}")),
            r.with_regions,
            r.med_opening.map_or("—".to_owned(), |m| format!("{m:.2}")),
            r.entropy,
            r.max_bin * 100.0,
            if fires.is_empty() { "none".to_owned() } else { fires.join(" ") }
        );
        if r.failed > 0 {
            println!("{:>7} {} of {SOLVES} solves did not converge", "", r.failed);
        }
    }

    println!("\n{}", "=".repeat(96));
    println!("`regions` is how many of the {SOLVES} solves produced ANY enclosed region — the number");
    println!("`expressive_range` found to be zero. Everything else is judged against §4's rows, which");
    println!("were committed before the first solve and are not editable on the strength of this table.");
}

struct Row {
    empty_share: f64,
    med_enclosure: Option<f32>,
    med_opening: Option<f32>,
    with_regions: usize,
    failed: usize,
    entropy: f32,
    max_bin: f32,
}

fn run(g: &Grammar, faces: &[Option<Interface>]) -> Row {
    let f = Faces::new(faces, WALL, WALKABLE);
    let mut bins = vec![0u32; BINS];
    let (mut enclosures, mut openings) = (Vec::new(), Vec::new());
    let (mut failed, mut with_regions) = (0usize, 0usize);
    let (mut empty_cells, mut total_cells) = (0u64, 0u64);

    for seed in 1..=SOLVES {
        let map = Map {
            name: "empty-weight".into(),
            bounds: (REGION as f32 * CELL, 3.0, REGION as f32 * CELL),
            ..Map::default()
        };
        let mut n = 0u64;
        let Ok(solved) = grammar::solve(&map, g, CELL, seed, || {
            n += 1;
            format!("w@{n}")
        }) else {
            failed += 1;
            continue;
        };
        for &p in &solved.grid {
            total_cells += 1;
            if p == 0 {
                empty_cells += 1;
            }
        }
        let Ok(m) = range::measure(
            solved.width,
            solved.height,
            &solved.grid,
            |p, d| f.wall(p, d),
            |p| f.floor(p),
            |p| f.doorway(p),
        ) else {
            failed += 1;
            continue;
        };
        let Measured { enclosure, opening_density, regions } = m;
        enclosures.push(enclosure);
        if regions > 0 {
            with_regions += 1;
        }
        let Some(open) = opening_density else { continue };
        openings.push(open);
        let (ex, ox) = range::bin(enclosure, open);
        bins[ex * RANGES + ox] += 1;
    }

    Row {
        empty_share: if total_cells == 0 { 0.0 } else { empty_cells as f64 / total_cells as f64 },
        med_enclosure: median(&enclosures),
        med_opening: median(&openings),
        with_regions,
        failed,
        entropy: range::normalised_entropy(&bins),
        max_bin: range::max_bin_share(&bins),
    }
}

fn median(xs: &[f32]) -> Option<f32> {
    if xs.is_empty() {
        return None;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(v[v.len() / 2])
}

const _: () = {
    let _ = OPENING_MAX;
};
