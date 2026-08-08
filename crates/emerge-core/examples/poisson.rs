//! **Poisson-disk sites → Delaunay → a degree-capped graph.**
//!
//! The three-step pipeline behind a graph-topology dungeon, run standalone. Bridson's algorithm
//! scatters points no closer than `radius` apart, an incremental Bowyer–Watson triangulation connects
//! them, and the degree cap trims the longest edges until no node is over-connected.
//!
//! All `f64`, so the in-circle predicate is bit-reproducible, and every random choice flows from the
//! caller's `DetRng` — the same seed lays out the same graph anywhere.
//!
//! Bridson, "Fast Poisson Disk Sampling in Arbitrary Dimensions", SIGGRAPH 2007.
//!
//! Run: `cargo run -p emerge-core --example poisson`

use emerge_core::geom::{delaunay_edges, poisson_disk, prune_to_max_degree, Point};
use emerge_core::rng::seeded;

const W: f64 = 72.0;
const H: f64 = 24.0;
const RADIUS: f64 = 6.0;
/// Candidate attempts per active sample — Bridson's `k`.
const K: usize = 30;
const MAX_DEGREE: usize = 3;

fn plot(points: &[Point], title: &str) {
    println!("\n{title}");
    let mut canvas = vec![vec![' '; W as usize]; H as usize];
    for (i, p) in points.iter().enumerate() {
        let (x, y) = (p[0] as usize, p[1] as usize);
        if y < canvas.len() && x < canvas[0].len() {
            // Label the first 36 sites so edges below can be read off the plot.
            canvas[y][x] = char::from_digit(i as u32 % 36, 36).unwrap_or('*');
        }
    }
    for row in canvas {
        println!("  {}", row.into_iter().collect::<String>());
    }
}

/// Smallest gap between any two sites — must be ≥ `RADIUS` for the sampler to have done its job.
fn min_separation(points: &[Point]) -> f64 {
    let mut best = f64::INFINITY;
    for (i, a) in points.iter().enumerate() {
        for b in points.iter().skip(i + 1) {
            let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
            best = best.min(d);
        }
    }
    best
}

fn degrees(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut d = vec![0usize; n];
    for &(a, b) in edges {
        d[a] += 1;
        d[b] += 1;
    }
    d
}

fn main() {
    let mut rng = seeded(0x5EED);
    let points = poisson_disk(W, H, RADIUS, K, &mut rng);
    plot(&points, &format!("{} sites, minimum spacing {RADIUS}", points.len()));
    println!(
        "  closest pair: {:.3} (must be ≥ {RADIUS})",
        if points.len() > 1 { min_separation(&points) } else { f64::NAN }
    );

    let edges = delaunay_edges(&points);
    let d = degrees(points.len(), &edges);
    println!("\nDelaunay triangulation: {} edges", edges.len());
    println!(
        "  degree min {} / max {} / mean {:.2}",
        d.iter().min().copied().unwrap_or(0),
        d.iter().max().copied().unwrap_or(0),
        d.iter().sum::<usize>() as f64 / d.len().max(1) as f64,
    );

    let pruned = prune_to_max_degree(&points, &edges, MAX_DEGREE);
    let dp = degrees(points.len(), &pruned);
    println!("\nPruned to degree ≤ {MAX_DEGREE}: {} edges ({} removed)", pruned.len(), edges.len() - pruned.len());
    println!(
        "  degree min {} / max {} / mean {:.2}",
        dp.iter().min().copied().unwrap_or(0),
        dp.iter().max().copied().unwrap_or(0),
        dp.iter().sum::<usize>() as f64 / dp.len().max(1) as f64,
    );

    println!("\n  first 12 surviving edges (site indices, matching the plot labels):");
    for chunk in pruned.iter().take(12).collect::<Vec<_>>().chunks(6) {
        let line: Vec<String> = chunk.iter().map(|(a, b)| format!("{a}–{b}")).collect();
        println!("    {}", line.join("   "));
    }

    // Same seed, same layout. This is the property the whole generation stack is built on.
    let mut rng2 = seeded(0x5EED);
    let again = poisson_disk(W, H, RADIUS, K, &mut rng2);
    let identical = again.len() == points.len()
        && again.iter().zip(points.iter()).all(|(a, b)| a[0].to_bits() == b[0].to_bits() && a[1].to_bits() == b[1].to_bits());
    println!(
        "\n{}",
        if identical {
            "✔ re-running from seed 0x5EED reproduced the point set bit-for-bit"
        } else {
            "✘ the same seed produced a different layout — that would be a bug in the crate"
        }
    );
    if !identical {
        std::process::exit(1);
    }
}
