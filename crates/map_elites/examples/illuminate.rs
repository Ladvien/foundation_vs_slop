//! **Watch an archive illuminate.**
//!
//! The point of MAP-Elites is that it does not return a winner, it returns a *map* — the best genome
//! found in every behaviour niche, including the mediocre corners. That is a picture, and a picture
//! is a poor thing to describe in prose, so this example writes one frame per generation and lets you
//! watch the search fill in.
//!
//! Two things are worth watching for:
//!
//! - **It fills outward, not uniformly.** Early elites are stepping stones: a new cell is almost
//!   always found by mutating a neighbour, so coverage spreads from what already exists. That is the
//!   whole argument for keeping mediocre solutions — they are the only route to the far corners.
//! - **Bright cells appear late and away from the centre.** A hill-climber would have gone straight
//!   at the peak and reported one number. The archive gets there too, and keeps everything else.
//!
//! **No renderer, deliberately.** This crate is engine-free because a search runs headless in a worker
//! subprocess, and `tests/engine_free.rs` fails the build if that stops being true. So the frames are
//! written with nothing but `std`: binary PPM, which every encoder reads.
//!
//! ```text
//! cargo run -p map_elites --example illuminate -- /tmp/illum
//! ffmpeg -framerate 24 -i /tmp/illum/f%04d.ppm -vf \
//!   "fps=12,scale=480:-1:flags=neighbor,split[a][b];[a]palettegen[p];[b][p]paletteuse" out.gif
//! ```
//!
//! Mouret & Clune, "Illuminating search spaces by mapping elites", arXiv:1504.04909 (2015).
//!
//! Run: `cargo run -p map_elites --example illuminate -- <output-dir>`

use std::path::Path;

use emerge_core::rng::seeded;
use map_elites::gaussian;
use map_elites::loops::{MapElitesResult, map_elites_loop};
use map_elites::population::Population;
use map_elites::qd::BehaviorDescriptor;
use rand_chacha::ChaCha8Rng;

/// Six genes, each in `[0,1]`.
type Genome = [f32; 6];

/// Archive resolution. 40x40 = 1600 niches, which is enough to look like a map rather than a chart.
const RES: usize = 40;
const GENERATIONS: u32 = 260;
const BATCH: u32 = 40;
const PATIENCE: u32 = 9_999;
const SEED: u64 = 0x5EED_1234;

/// Pixels per archive cell in the written frames.
const CELL_PX: usize = 10;
/// Height of the progress strip under the map.
const BAR_PX: usize = 14;

/// **Three peaks, not one.** A single-peaked landscape makes a pretty gradient but understates the
/// case: with several optima the archive shows you all of them at once, which is the thing an
/// optimiser structurally cannot report.
const PEAKS: [(Genome, f32); 3] = [
    ([0.75, 0.70, 0.30, 0.25, 0.5, 0.5], 1.00),
    ([0.20, 0.25, 0.80, 0.75, 0.4, 0.6], 0.85),
    ([0.55, 0.15, 0.15, 0.60, 0.7, 0.3], 0.70),
];

fn fitness(g: &Genome) -> f32 {
    PEAKS
        .iter()
        .map(|(p, h)| {
            let d2: f32 = g.iter().zip(p.iter()).map(|(a, b)| (a - b) * (a - b)).sum();
            h / (1.0 + 9.0 * d2)
        })
        .fold(0.0_f32, f32::max)
}

/// Behaviour is *how* the genome is shaped, not how good it is — the axes are deliberately not the
/// objective, or the archive would just be a fitness plot with extra steps.
fn descriptor(g: &Genome) -> BehaviorDescriptor {
    BehaviorDescriptor::new((g[0] + g[1] + g[4]) / 3.0, (g[2] + g[3] + g[5]) / 3.0)
}

fn mutate(parent: &Genome, rng: &mut ChaCha8Rng) -> Result<Genome, String> {
    let mut child = *parent;
    for gene in child.iter_mut() {
        *gene = (*gene + gaussian(rng) * 0.13).clamp(0.0, 1.0);
    }
    Ok(child)
}

/// Dark navy → teal → amber → white. Perceptually ordered so "brighter is fitter" survives being
/// quantised down to a few dozen colours by a gif encoder.
fn ramp(t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let stops: [(f32, [f32; 3]); 5] = [
        (0.00, [0.05, 0.06, 0.18]),
        (0.30, [0.10, 0.34, 0.52]),
        (0.60, [0.16, 0.68, 0.58]),
        (0.85, [0.90, 0.72, 0.24]),
        (1.00, [1.00, 0.98, 0.90]),
    ];
    let mut lo = stops[0];
    let mut hi = stops[stops.len() - 1];
    for w in stops.windows(2) {
        if t >= w[0].0 && t <= w[1].0 {
            lo = w[0];
            hi = w[1];
            break;
        }
    }
    let span = (hi.0 - lo.0).max(1.0e-6);
    let k = (t - lo.0) / span;
    let mut out = [0u8; 3];
    for i in 0..3 {
        out[i] = ((lo.1[i] + (hi.1[i] - lo.1[i]) * k) * 255.0).clamp(0.0, 255.0) as u8;
    }
    out
}

/// Write the archive as one binary PPM frame. `std` only — see the module note.
fn write_frame(dir: &Path, index: u32, pop: &Population<Genome>, filled: f32) -> Result<(), String> {
    let w = RES * CELL_PX;
    let h = RES * CELL_PX + BAR_PX;
    let mut px = vec![0u8; w * h * 3];

    // Empty niches read as "not visited yet", so they are darker than the dimmest elite.
    for p in px.chunks_exact_mut(3) {
        p.copy_from_slice(&[10, 11, 18]);
    }

    let best = pop.archive.best().map(|e| e.fitness).unwrap_or(1.0).max(1.0e-6);
    for ((cx, cy), elite) in pop.archive.iter() {
        let colour = ramp(elite.fitness / best);
        for oy in 0..CELL_PX {
            for ox in 0..CELL_PX {
                let x = cx * CELL_PX + ox;
                // Row 0 at the bottom, so the map is oriented the way a plot of the two axes is.
                let y = (RES - 1 - cy) * CELL_PX + oy;
                if x < w && y < RES * CELL_PX {
                    let i = (y * w + x) * 3;
                    px[i..i + 3].copy_from_slice(&colour);
                }
            }
        }
    }

    // A coverage strip under the map, so the gif shows progress even while the map is still sparse.
    let lit = ((filled.clamp(0.0, 1.0) * w as f32) as usize).min(w);
    for y in RES * CELL_PX + 3..h - 3 {
        for x in 0..w {
            let i = (y * w + x) * 3;
            let c: [u8; 3] = if x < lit { [230, 184, 61] } else { [26, 28, 38] };
            px[i..i + 3].copy_from_slice(&c);
        }
    }

    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.extend_from_slice(&px);
    let path = dir.join(format!("f{index:04}.ppm"));
    std::fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "illumination".to_string());
    let dir = Path::new(&dir);
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("could not create `{}`: {e}", dir.display());
        std::process::exit(1);
    }

    let mut rng = seeded(SEED);
    let mut result = MapElitesResult {
        pop: Population::<Genome>::new(RES),
        evaluations: 0,
        rejected_infeasible: 0,
        rejected_by_criterion: 0,
    };
    let authored: Genome = [0.5; 6];
    let cells = (RES * RES) as f32;
    let mut frames = 0u32;
    let mut write_err: Option<String> = None;

    let outcome = map_elites_loop(
        &mut rng,
        &mut result,
        &authored,
        GENERATIONS,
        BATCH,
        PATIENCE,
        "the authored genome failed the minimal criterion",
        mutate,
        |g: &Genome| g.iter().all(|v| (0.0..=1.0).contains(v)),
        |g: &Genome| Some((descriptor(g), fitness(g))),
        |generation, r| {
            // One frame per generation. The callback is also where a real search would checkpoint.
            if write_err.is_none() {
                let filled = r.pop.archive.coverage() as f32 / cells;
                if let Err(e) = write_frame(dir, frames, &r.pop, filled) {
                    write_err = Some(e);
                }
                frames += 1;
            }
            if generation % 40 == 0 {
                println!(
                    "  gen {generation:>3}  coverage {:>4}/{}  qd {:>8.3}  evals {}",
                    r.pop.archive.coverage(),
                    RES * RES,
                    r.pop.archive.qd_score(),
                    r.evaluations,
                );
            }
        },
    );

    if let Some(e) = write_err {
        eprintln!("failed writing a frame: {e}");
        std::process::exit(1);
    }
    if let Err(e) = outcome {
        eprintln!("search failed: {e}");
        std::process::exit(1);
    }

    let a = &result.pop.archive;
    println!();
    println!("  wrote {frames} frames to {}", dir.display());
    println!(
        "  final: coverage {}/{}  qd {:.3}  best {:.4}",
        a.coverage(),
        RES * RES,
        a.qd_score(),
        a.best().map(|e| e.fitness).unwrap_or(0.0)
    );
    println!();
    println!("  ffmpeg -framerate 24 -i {}/f%04d.ppm \\", dir.display());
    println!("    -vf \"fps=12,scale=480:-1:flags=neighbor,split[a][b];[a]palettegen[p];[b][p]paletteuse\" \\");
    println!("    illuminate.gif");
}
