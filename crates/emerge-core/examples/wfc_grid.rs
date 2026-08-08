//! **Wave Function Collapse over an alphabet you define.**
//!
//! `collapse_grid` is the reusable core — it knows nothing about dungeons. You hand it a prototype
//! alphabet (selection weights + an adjacency table) and a starting domain per cell, and it hands back
//! the chosen prototype index per cell, or `None` on contradiction so you can retry with another seed.
//!
//! The alphabet here is a five-step terrain ramp with one rule: neighbours may differ by at most one
//! step. That single constraint is enough to produce coherent coastlines, which is the point — WFC is
//! finite-domain constraint solving, not a texture trick (Karth & Smith 2017).
//!
//! Run: `cargo run -p emerge-core --example wfc_grid`

use emerge_core::wfc::collapse_grid;

const W: usize = 64;
const H: usize = 22;

/// Deep → shallow → sand → grass → rock.
const GLYPHS: [char; 5] = ['~', '≈', '.', '"', '^'];
const NAMES: [&str; 5] = ["deep", "shallow", "sand", "grass", "rock"];
const N: usize = GLYPHS.len();

/// Selection weights: more open water and grass than beach or peak.
const WEIGHTS: [f64; N] = [1.4, 0.7, 0.5, 1.2, 0.6];

/// `support[dir][p]` is the bitmask of prototypes allowed on the `dir` (N/E/S/W) side of `p`.
///
/// The rule is isotropic — a step of at most one in either direction — so all four directions get the
/// same table. An anisotropic alphabet (roads that only join end-to-end, say) would differ per `dir`,
/// and that is the whole extension point.
fn build_support() -> [Vec<u32>; 4] {
    let per_dir: Vec<u32> = (0..N)
        .map(|p| {
            let mut mask = 0u32;
            for q in 0..N {
                if p.abs_diff(q) <= 1 {
                    mask |= 1 << q;
                }
            }
            mask
        })
        .collect();
    [per_dir.clone(), per_dir.clone(), per_dir.clone(), per_dir]
}

fn plot(picks: &[usize], title: &str) {
    println!("\n{title}");
    for y in 0..H {
        let row: String = (0..W)
            .map(|x| GLYPHS.get(picks[y * W + x]).copied().unwrap_or('?'))
            .collect();
        println!("  {row}");
    }
}

fn histogram(picks: &[usize]) {
    let mut counts = [0usize; N];
    for &p in picks {
        if p < N {
            counts[p] += 1;
        }
    }
    println!("\n  prototype counts (weights in parentheses):");
    for i in 0..N {
        let share = counts[i] as f64 / picks.len() as f64 * 100.0;
        println!("    {} {:<9} {:>5}  {share:>5.1}%   (weight {:.1})", GLYPHS[i], NAMES[i], counts[i], WEIGHTS[i]);
    }
}

fn main() {
    let support = build_support();
    // Every cell starts fully permissive. A narrowed mask here is a unary constraint — that is how the
    // dungeon's "the border must be rock" rule is expressed, with no special case in the solver.
    let full = (1u32 << N) - 1;
    let initial = vec![full; W * H];

    let mut solved = 0;
    for seed in [1u64, 2, 3] {
        match collapse_grid(W, H, &WEIGHTS, &support, &initial, seed) {
            Some(picks) => {
                solved += 1;
                plot(&picks, &format!("seed {seed}"));
                if seed == 1 {
                    histogram(&picks);
                }
            }
            // A contradiction is a legitimate outcome, returned rather than panicked, so the caller
            // decides whether to retry, widen the alphabet, or give up.
            None => println!("\nseed {seed}: contradiction — the caller retries with another seed"),
        }
    }

    // Constrained start: force the top and bottom rows to deep water and let the solver reconcile.
    let mut banded = vec![full; W * H];
    for x in 0..W {
        banded[x] = 1 << 0;
        banded[(H - 1) * W + x] = 1 << 0;
    }
    match collapse_grid(W, H, &WEIGHTS, &support, &banded, 4) {
        Some(picks) => plot(&picks, "seed 4, with the top and bottom rows pinned to deep water"),
        None => println!("\nthe pinned-edge start was already inconsistent"),
    }

    println!("\n{solved}/3 unconstrained seeds converged.");
    println!(
        "Note what never appeared in this file: a dungeon, a room, a tile atlas. The alphabet is the\n\
         caller's — this crate only solves it."
    );
}
