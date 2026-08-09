//! **What MAP-Elites gives you that an optimiser does not: a map, not a winner.**
//!
//! A toy 4-gene problem. Fitness is a single-peaked function, so a plain hill-climber would converge
//! on one point and forget everything it saw on the way. MAP-Elites instead keeps the fittest genome
//! found *per behaviour niche*, so what comes back is a picture of what the parameterisation can
//! actually do — including the mediocre corners, which is where the stepping stones live.
//!
//! Two things this demonstrates about the crate itself:
//!
//! 1. **There is no evaluator in here.** `map_elites_loop` is generic over the genome type and takes
//!    mutation and evaluation as closures. Everything problem-specific below is in `main`.
//! 2. **A run replays bit-for-bit from one `u64`.** The second pass re-runs from the same seed and
//!    compares coverage, QD score and best fitness — they must match exactly.
//!
//! Mouret & Clune, "Illuminating search spaces by mapping elites", arXiv:1504.04909 (2015).
//!
//! Run: `cargo run -p map_elites --example sphere_archive`

use det_rng::seeded;
use map_elites::gaussian;
use map_elites::loops::{map_elites_loop, MapElitesResult};
use map_elites::population::Population;
use map_elites::qd::BehaviorDescriptor;
use rand_chacha::ChaCha8Rng;

/// Four genes, each in `[0,1]`.
type Genome = [f32; 4];

const RES: usize = 12;
const GENERATIONS: u32 = 40;
const BATCH: u32 = 24;
/// Generations without a QD-score gain before the loop gives up.
const PATIENCE: u32 = 999;

/// The peak sits off-centre so the archive is not symmetric and the plot is readable.
const TARGET: Genome = [0.7, 0.3, 0.65, 0.25];

/// Single-peaked: 1.0 at `TARGET`, falling off with squared distance. A pure optimiser would find
/// this and report one number; the archive reports the whole landscape.
fn fitness(g: &Genome) -> f32 {
    let d2: f32 = g.iter().zip(TARGET.iter()).map(|(a, b)| (a - b) * (a - b)).sum();
    1.0 / (1.0 + 4.0 * d2)
}

/// Behaviour is *how* the genome is shaped, not how good it is — the axes are deliberately not the
/// objective. Genes 0/1 drive one axis, genes 2/3 the other.
fn descriptor(g: &Genome) -> BehaviorDescriptor {
    BehaviorDescriptor::new((g[0] + g[1]) * 0.5, (g[2] + g[3]) * 0.5)
}

fn mutate(parent: &Genome, rng: &mut ChaCha8Rng) -> Result<Genome, String> {
    let mut child = *parent;
    for gene in child.iter_mut() {
        *gene = (*gene + gaussian(rng) * 0.15).clamp(0.0, 1.0);
    }
    Ok(child)
}

/// One `search` run. Returns `(coverage, qd_score, best_fitness)` so two runs can be compared.
fn search(seed: u64, verbose: bool) -> Result<(usize, f32, f32), String> {
    let mut rng = seeded(seed);
    let mut result = MapElitesResult {
        pop: Population::<Genome>::new(RES),
        evaluations: 0,
        rejected_infeasible: 0,
        rejected_by_criterion: 0,
    };

    let authored: Genome = [0.5, 0.5, 0.5, 0.5];

    map_elites_loop(
        &mut rng,
        &mut result,
        &authored,
        GENERATIONS,
        BATCH,
        PATIENCE,
        "the authored genome failed the minimal criterion",
        mutate,
        // Cheap bounds gate before the (here, trivial) evaluation.
        |g: &Genome| g.iter().all(|v| (0.0..=1.0).contains(v)),
        // `None` here would be a minimal-criterion reject — "this run told us nothing".
        |g: &Genome| Some((descriptor(g), fitness(g))),
        |generation, r| {
            if verbose && generation % 10 == 0 {
                println!(
                    "  gen {generation:>3}  coverage {:>3}/{}  qd {:>7.3}  evals {}",
                    r.pop.archive.coverage(),
                    RES * RES,
                    r.pop.archive.qd_score(),
                    r.evaluations,
                );
            }
        },
    )?;

    let archive = &result.pop.archive;
    let best = archive.best().map(|e| e.fitness).unwrap_or(0.0);

    if verbose {
        // One pass over the archive into a dense grid, rather than a scan per cell.
        let mut grid = vec![None::<f32>; RES * RES];
        for (&(ax, ay), elite) in archive.iter() {
            if ax < RES && ay < RES {
                grid[ay * RES + ax] = Some(elite.fitness);
            }
        }

        println!("\n  archive ({RES}×{RES} niches, '·' = never reached):");
        println!("    exploration →");
        for ay in 0..RES {
            let row: String = (0..RES)
                .map(|ax| match grid[ay * RES + ax] {
                    Some(f) => {
                        const RAMP: [char; 10] = ['.', ':', '-', '=', '+', '*', '#', '%', '@', '█'];
                        RAMP[((f * 9.0).round() as usize).min(9)]
                    }
                    None => '·',
                })
                .collect();
            println!("    {row}");
        }
        println!(
            "\n  coverage {}/{}   QD score {:.3}   best fitness {:.4}   evaluations {}",
            archive.coverage(),
            RES * RES,
            archive.qd_score(),
            best,
            result.evaluations,
        );
        println!("  rejected: {} infeasible, {} by criterion", result.rejected_infeasible, result.rejected_by_criterion);
    }

    Ok((archive.coverage(), archive.qd_score(), best))
}

fn main() {
    println!("MAP-Elites over a 4-gene toy problem, {GENERATIONS} generations × {BATCH} proposals.\n");

    let first = match search(0xC0FFEE, true) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("search failed: {e}");
            std::process::exit(1);
        }
    };

    println!("\nRe-running from the same seed — every draw goes through one ChaCha8 stream and every");
    println!("archive walk is over a BTreeMap, so this must come back identical:");
    let second = match search(0xC0FFEE, false) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("search failed: {e}");
            std::process::exit(1);
        }
    };

    let identical = first.0 == second.0 && first.1.to_bits() == second.1.to_bits() && first.2.to_bits() == second.2.to_bits();
    println!(
        "  run 1: coverage {:>3}  qd {:.6}  best {:.6}\n  run 2: coverage {:>3}  qd {:.6}  best {:.6}",
        first.0, first.1, first.2, second.0, second.1, second.2,
    );

    if identical {
        println!("\n  ✔ bit-identical (compared as raw f32 bits, not within a tolerance)");
    } else {
        eprintln!("\n  ✘ runs diverged — that is a bug in the crate, not in this example");
        std::process::exit(1);
    }
}
