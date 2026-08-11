//! **The expressive range of the composition grammar, read against thresholds committed first.**
//!
//! ```text
//! cargo run -p emerge-core --example expressive_range
//! ```
//!
//! Smith & Whitehead's method (`10.1145/1814256.1814260`): generate a population, score each artefact
//! on two axes, and read the 2-D histogram for bias *before* looking at any individual output. The
//! metrics, the domain grid, the stopping rule and all four failure rows were written down in
//! `docs/research/2026-08-09-composition-grammar-decisions.md` §4 before the first solve here ran —
//! ch12's warning is the point: *"If you see five levels that are impressive, among 50 that you choose
//! to ignore or re-generate, what does that say about the qualities of the content generator?"*
//!
//! # What this prints, and what it deliberately does not
//!
//! A terminal report and a 6 x 6 ASCII heatmap. Thirty-six bins is legible as text, and
//! `docs/bevy_plugins.md` prefers a terminal example over a window because it runs anywhere and costs
//! no GPU. The raw bin counts go out as RON so a later reader can re-plot without re-solving.
//!
//! **This is not a test and must not become one.** The rows are a falsification criterion for the
//! *approach*, read once by a human; a red build is the wrong response to "the generator is biased."
//! Nothing here is a regression gate, and nothing stops the next run from ignoring the verdict.

use std::collections::BTreeMap;

use emerge_core::composition::{Compositions, Interface};
use emerge_core::constraints::{self, GridProblem};
use emerge_core::grammar::{self, Composed, Grammar, Prototype};
use emerge_core::library::Library;
use emerge_core::map::Map;
use emerge_core::range::{self, Faces, Measured, BINS, OPENING_MAX, RANGES};
use emerge_core::wfc::{E, N, S, W};

/// The region every solve runs on, in cells — `site_67`'s slab. Pre-registered in §4.5.
const REGION: usize = 12;
/// Metres per cell.
const CELL: f32 = 1.0;

/// Sample sizes for the doubling sweep, and the cap. §4.5: the stopping rule sets the run length, not
/// this list — the cap is a budget, and it is reported as one if it is hit. Smith & Whitehead run
/// 10,000 levels per graph; a WFC collapse over 144 cells is a good deal dearer than a Launchpad level.
const BLOCKS: [usize; 5] = [64, 128, 256, 512, 1024];
/// Stop when two **disjoint** blocks agree this closely. Nested blocks would halve it exactly.
const STABLE_TV: f32 = 0.05;

/// The committed floor on normalised entropy (§4.5, row 4a).
const ENTROPY_FLOOR: f32 = 0.25;
/// The committed ceiling on any one bin's share (§4.5, row 4b).
const MAX_BIN_CEILING: f32 = 0.50;
/// Row 3's gate: above this share of failed solves, nothing else is interpretable.
const NON_CONVERGENCE_GATE: f32 = 0.20;

/// How much search one constrained solve may spend before it counts as a non-convergence.
///
/// A budget rather than no limit, so one pathological seed cannot stall a 1,024-solve sweep. It makes
/// row 3 count two different events — "no arrangement exists" and "did not find one in time" — which
/// is honest for a gate whose purpose is *"nothing below is interpretable"*, and would not be if the
/// rows below distinguished them.
const CONSTRAINED_BUDGET: u64 = 200_000;

/// What produced an arrangement. **One instrument, two generators**: the rows, the bins, the stopping
/// rule and the histogram are shared, because a second copy of the measurement could disagree with
/// this one and then the comparison would be between instruments rather than between generators.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Generator {
    /// `wfc::collapse_grid` through `grammar::solve` — greedy, non-backtracking, weighted collapse.
    Wfc,
    /// `constraints::GridProblem` plus one seeded wish per cell, solved to a proven optimum.
    Constraint,
}

impl Generator {
    fn name(self) -> &'static str {
        match self {
            Generator::Wfc => "wfc",
            Generator::Constraint => "constraint",
        }
    }
}

/// The kit's edge token for a wall. Named here rather than in `emerge_core::range`, which never learns
/// a kit's vocabulary — the same seam `agrees` is passed through.
const WALL: &str = "wall";
/// A gap under a lintel this tall or more is a doorway; less is a plinth, not something to walk under.
const WALKABLE: f32 = 0.5;

fn main() {
    // `cargo run --example expressive_range -- 7` draws seed 7's grid instead of running the sweep.
    // **Reading the rows comes first** — ch12's whole argument is that looking at output before the
    // criterion is how a generator gets graded on its five best artefacts — so this is a second entry
    // point rather than something the report prints alongside the verdict.
    // `-- 7` still draws seed 7 from the default generator, as it always has. `-- constraint`
    // sweeps the other one, and `-- constraint 7` draws from it.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (source, rest): (Generator, &[String]) = match args.first().map(String::as_str) {
        Some("constraint") => (Generator::Constraint, &args[1..]),
        Some("wfc") => (Generator::Wfc, &args[1..]),
        _ => (Generator::Wfc, &args[..]),
    };
    let show: Option<u64> = rest.first().and_then(|a| a.parse().ok());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/emerge/site");
    let library: Library = match std::fs::read_to_string(root.join("library.ron"))
        .map_err(|e| e.to_string())
        .and_then(|t| ron::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(l) => l,
        Err(e) => return eprintln!("cannot read the site library: {e}"),
    };
    let comps: Compositions = match std::fs::read_to_string(root.join("compositions.ron"))
        .map_err(|e| e.to_string())
        .and_then(|t| ron::from_str(&t).map_err(|e| e.to_string()))
    {
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
    let Composed { grammar: g, skipped, faces } = composed;

    println!("EXPRESSIVE RANGE — composition grammar over the shipped Site kit");
    println!("generator: {}", source.name());
    println!("{}", "=".repeat(78));
    println!(
        "region {REGION} x {REGION} cells   grid {RANGES} x {RANGES} = {BINS} bins   \
         enclosure [0,1] x opening [0,{OPENING_MAX}]"
    );
    println!("{} prototypes from {} compositions", g.len(), comps.compositions.len());
    for s in &skipped {
        println!("  skipped: {s}");
    }
    print_alphabet(&g, &faces);

    if let Some(seed) = show {
        draw(&g, &faces, seed, source);
        return;
    }

    // ---- the sweep ------------------------------------------------------------------------------
    let mut cache: BTreeMap<u64, Option<(Measured, Vec<u32>)>> = BTreeMap::new();
    let mut chosen: Option<(usize, f32)> = None;
    println!("\nSTOPPING RULE — disjoint blocks, stop at TV <= {STABLE_TV}");
    for n in BLOCKS {
        let a = histogram(&mut cache, &g, &faces, 1..=n as u64, source);
        let b = histogram(&mut cache, &g, &faces, n as u64 + 1..=2 * n as u64, source);
        let tv = range::total_variation(&a.bins, &b.bins);
        // **§4.6: two blocks agreeing is convergence only if one of them holds a sample.** The total
        // variation between two EMPTY histograms is 0 by definition, so before this the sweep stopped
        // at its first block having measured nothing and reported "stable". Pre-registered 2026-08-11,
        // before the enclosure run — FVS-R-18.
        let measured = a.total() + b.total() > 0;
        let stable = measured && tv <= STABLE_TV;
        println!(
            "  n = {n:>5} vs {n:>5}   TV = {tv:.4}{}",
            if stable {
                "   <- stable"
            } else if !measured {
                "   (both blocks empty — not convergence)"
            } else {
                ""
            }
        );
        if stable {
            chosen = Some((n, tv));
            break;
        }
    }
    let (n, tv) = match chosen {
        Some(c) => c,
        None => {
            let last = BLOCKS[BLOCKS.len() - 1];
            println!("  never stabilised inside the budget — reporting at the cap");
            (last, f32::NAN)
        }
    };
    let run = histogram(&mut cache, &g, &faces, 1..=2 * n as u64, source);
    let solves = 2 * n;

    // ---- the report -----------------------------------------------------------------------------
    let failed = run.failed as f32 / solves as f32;
    println!("\n{}", "=".repeat(78));
    println!("RUN — {solves} solves (blocks of {n}, TV {tv:.4})");
    println!("  did not converge      {:>5} / {solves}  ({:.1}%)", run.failed, failed * 100.0);
    println!("  no enclosed region    {:>5}          (excluded from the histogram)", run.no_region);
    println!("  clamped above opening {OPENING_MAX:<3}{:>5}          (row 4b must not fire on this)", run.clamped);
    println!("  in the histogram      {:>5}", run.total());
    if run.total() == 0 {
        println!(
            "\n  NOTE: the histogram is EMPTY, so the stopping rule was satisfied by two blank blocks\n  \
             agreeing rather than by a converged shape. TV between two empty histograms is 0 by\n  \
             definition. The run length above is not evidence of stability — it is evidence that\n  \
             nothing reached the plane at all, and no larger run would change that."
        );
    }

    println!("\nROW 3 — THE GATE");
    let gate_fired = failed > NON_CONVERGENCE_GATE;
    println!(
        "  > {:.0}% of solves fail            {}   ({:.1}%)",
        NON_CONVERGENCE_GATE * 100.0,
        verdict(gate_fired),
        failed * 100.0
    );
    if gate_fired {
        println!("\n  *** The alphabet is over-constrained. Add tiles before judging the approach —");
        println!("  *** rows 1, 2, 4a and 4b below are NOT interpretable.");
    }

    let med_enc = median(&run.enclosures);
    let med_open = median(&run.openings);
    let entropy = range::normalised_entropy(&run.bins);
    let top = range::max_bin_share(&run.bins);

    println!("\nROWS 1, 2, 4a, 4b{}", if gate_fired { "  (gated — read nothing into these)" } else { "" });
    println!(
        "  median enclosure < 0.15         {}   ({:.3})",
        verdict(med_enc.is_some_and(|m| m < 0.15)),
        med_enc.unwrap_or(f32::NAN)
    );
    println!(
        "  median enclosure > 0.95 and     {}   ({:.3} / {:.3})",
        verdict(med_enc.is_some_and(|m| m > 0.95) && med_open.is_some_and(|o| o < 0.5)),
        med_enc.unwrap_or(f32::NAN),
        med_open.unwrap_or(f32::NAN)
    );
    println!("    opening density < 0.5");
    // **§4.6: a concentration statistic over zero samples is undefined, so it is not evaluated.**
    // Before this, entropy read 0 and FIRED while max-bin share read 0 and PASSED — two verdicts about
    // how mass is distributed, with no mass. `n/a` is not a pass.
    let empty = run.total() == 0;
    if empty {
        println!("  H / ln {BINS} < {ENTROPY_FLOOR}                n/a     (the histogram is empty)");
        println!("  max bin share > {:.0}%             n/a", MAX_BIN_CEILING * 100.0);
    } else {
        println!(
            "  H / ln {BINS} < {ENTROPY_FLOOR}                {}   ({entropy:.3})",
            verdict(entropy < ENTROPY_FLOOR)
        );
        println!(
            "  max bin share > {:.0}%             {}   ({:.1}%)",
            MAX_BIN_CEILING * 100.0,
            verdict(top > MAX_BIN_CEILING),
            top * 100.0
        );
    }
    println!(
        "\n  bins occupied {} of {BINS} — a LOWER BOUND on reachability, measured by sampling and",
        run.bins.iter().filter(|&&c| c > 0).count()
    );
    println!("  not by constraint: this solver cannot be asked whether it can reach a bin (§4.3).");

    heatmap(&run.bins);
    dump(&run, source);

    println!("\n{}", "=".repeat(78));
    let any = med_enc.is_some_and(|m| m < 0.15)
        || (med_enc.is_some_and(|m| m > 0.95) && med_open.is_some_and(|o| o < 0.5))
        || (!empty && entropy < ENTROPY_FLOOR)
        || (!empty && top > MAX_BIN_CEILING)
        || gate_fired;
    println!("VERDICT: {}", if any { "the approach FAILS at least one committed row" } else { "no committed row fires" });
    // §4.6's own outcome, reported beside the rows rather than inferred from them.
    if empty {
        println!(
            "OUTCOME: the histogram is EMPTY — the generator did not reach the plane. That is the\n\
             finding, not a set of statistics about nothing (§4.6, pre-registered 2026-08-11)."
        );
    }

    // Printed after the verdict on purpose: it explains a result rather than contributing to one.
    println!("\nCELL CENSUS — what won the cells, and what the weights asked for");
    let placed: u32 = run.cells.iter().sum();
    let weight_total: f64 = g.weights.iter().sum();
    for (p, count) in run.cells.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        let name = match &g.prototypes[p] {
            Prototype::Empty => "(empty)".to_owned(),
            Prototype::Piece { descriptor, yaw } => format!("{descriptor} @ {yaw:.0}"),
            Prototype::Composed { composition, yaw } => format!("{composition} @ {yaw:.0}"),
        };
        println!(
            "  {:>6.2}% actual   {:>6.2}% by weight   {name}",
            *count as f64 / placed.max(1) as f64 * 100.0,
            g.weights.get(p).copied().unwrap_or(0.0) / weight_total * 100.0
        );
    }
}

fn verdict(fired: bool) -> &'static str {
    if fired {
        "FIRES"
    } else {
        "pass "
    }
}

/// One run's tally.
struct Run {
    bins: Vec<u32>,
    /// How many cells each prototype won, over the whole run — the diagnostic that explains a verdict.
    cells: Vec<u32>,
    enclosures: Vec<f32>,
    openings: Vec<f32>,
    failed: usize,
    no_region: usize,
    clamped: usize,
}

impl Run {
    fn total(&self) -> u32 {
        self.bins.iter().sum()
    }
}

/// Solve every seed in the range and bin the results, reusing anything already solved.
fn histogram(
    cache: &mut BTreeMap<u64, Option<(Measured, Vec<u32>)>>,
    g: &Grammar,
    faces: &[Option<Interface>],
    seeds: std::ops::RangeInclusive<u64>,
    source: Generator,
) -> Run {
    let mut run = Run {
        bins: vec![0; BINS],
        cells: vec![0; g.len()],
        enclosures: Vec::new(),
        openings: Vec::new(),
        failed: 0,
        no_region: 0,
        clamped: 0,
    };
    for seed in seeds {
        let scored = match cache.get(&seed) {
            Some(s) => s.clone(),
            None => {
                let s = solve_and_score(g, faces, seed, source);
                cache.insert(seed, s.clone());
                s
            }
        };
        let Some((m, cells)) = scored else {
            run.failed += 1;
            continue;
        };
        for (p, c) in cells.iter().enumerate() {
            if let Some(slot) = run.cells.get_mut(p) {
                *slot += c;
            }
        }
        run.enclosures.push(m.enclosure);
        let Some(open) = m.opening_density else {
            run.no_region += 1;
            continue;
        };
        if open > OPENING_MAX {
            run.clamped += 1;
        }
        run.openings.push(open);
        let (ex, ox) = range::bin(m.enclosure, open);
        run.bins[ex * RANGES + ox] += 1;
    }
    run
}

/// One solve, scored. `None` is a solve that did not converge — row 3's countable event.
fn solve_and_score(
    g: &Grammar,
    faces: &[Option<Interface>],
    seed: u64,
    source: Generator,
) -> Option<(Measured, Vec<u32>)> {
    let grid = arrange(g, seed, source)?;

    let mut cells = vec![0u32; g.len()];
    for &p in &grid {
        if let Some(slot) = cells.get_mut(p) {
            *slot += 1;
        }
    }
    let m = range::measure(
        REGION,
        REGION,
        &grid,
        |p, d| Faces::new(faces, WALL, WALKABLE).wall(p, d),
        |p| Faces::new(faces, WALL, WALKABLE).floor(p),
        |p| Faces::new(faces, WALL, WALKABLE).doorway(p),
    )
    .ok()?;
    Some((m, cells))
}

/// **The only place the two generators differ.** Everything above and below this function is shared,
/// so a difference in the report is a difference in the generator rather than in how it was measured.
///
/// `None` is a solve that did not converge — row 3's countable event. For the constraint path that
/// covers both a proven refusal and a budget exhaustion; see [`CONSTRAINED_BUDGET`].
fn arrange(g: &Grammar, seed: u64, source: Generator) -> Option<Vec<usize>> {
    match source {
        Generator::Wfc => {
            let map = Map {
                name: "expressive-range".into(),
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
        Generator::Constraint => {
            let full: u32 = if g.len() == 32 { u32::MAX } else { (1u32 << g.len()) - 1 };
            let mut gp =
                GridProblem::encode(&g.support, &vec![full; REGION * REGION], REGION, REGION, g.len())
                    .ok()?;
            // One wish per cell, drawn from the author's own weights — the whole of the variety, and
            // the only thing the seed touches. Weight 1, so the objective is the number of cells the
            // rules forced away from what the seed asked for.
            constraints::preference_rules(&mut gp.problem, &gp.place, &g.weights, seed).ok()?;
            gp.problem.solve(CONSTRAINED_BUDGET).ok().and_then(|s| gp.read(&s).ok())
        }
    }
}

/// Draw one solve: a wall glyph per cell, then which cells the border fill could not reach.
///
/// The glyph is a box-drawing character whose strokes point at the faces that present a wall, so a
/// closed room reads as a closed rectangle and a stray wall reads as a stub.
fn draw(g: &Grammar, faces: &[Option<Interface>], seed: u64, source: Generator) {
    let Some(grid) = arrange(g, seed, source) else {
        return println!("\nseed {seed} did not converge under {}", source.name());
    };
    let (w, h) = (REGION, REGION);

    const GLYPH: [char; 16] = [
        '·', '\u{2575}', '\u{2576}', '\u{2514}', '\u{2577}', '\u{2502}', '\u{250c}', '\u{251c}',
        '\u{2574}', '\u{2518}', '\u{2500}', '\u{2534}', '\u{2510}', '\u{2524}', '\u{252c}', '\u{253c}',
    ];
    println!("\nSEED {seed} — walls (strokes point at walled faces, `d` marks a doorway, ` ` is Empty)");
    for z in 0..h {
        print!("  ");
        for x in 0..w {
            let p = grid[z * w + x];
            if p == 0 {
                print!("  ");
                continue;
            }
            let mask = [N, E, S, W]
                .iter()
                .enumerate()
                .fold(0usize, |m, (bit, &d)| m | usize::from(Faces::new(faces, WALL, WALKABLE).wall(p, d)) << bit);
            print!("{}{}", GLYPH[mask], if Faces::new(faces, WALL, WALKABLE).doorway(p) { 'd' } else { ' ' });
        }
        println!();
    }

    let scored = range::measure(
        w,
        h,
        &grid,
        |p, d| Faces::new(faces, WALL, WALKABLE).wall(p, d),
        |p| Faces::new(faces, WALL, WALKABLE).floor(p),
        |p| Faces::new(faces, WALL, WALKABLE).doorway(p),
    );
    match scored {
        Ok(m) => println!(
            "\n  enclosure {:.3}   regions {}   opening density {}",
            m.enclosure,
            m.regions,
            m.opening_density.map_or("undefined".to_owned(), |o| format!("{o:.3}"))
        ),
        Err(e) => println!("\n  {e}"),
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

fn print_alphabet(g: &Grammar, faces: &[Option<Interface>]) {
    println!("\nALPHABET — what each prototype presents, as `range` reads it");
    for (p, proto) in g.prototypes.iter().enumerate() {
        let name = match proto {
            Prototype::Empty => "(empty)".to_owned(),
            Prototype::Piece { descriptor, yaw } => format!("{descriptor} @ {yaw:.0}"),
            Prototype::Composed { composition, yaw } => format!("{composition} @ {yaw:.0}"),
        };
        let sides: String = [(N, 'N'), (E, 'E'), (S, 'S'), (W, 'W')]
            .iter()
            .map(|&(d, c)| if Faces::new(faces, WALL, WALKABLE).wall(p, d) { c } else { '.' })
            .collect();
        println!(
            "  {p:>2}  walls {sides}  {}  w={:.3}  {name}",
            if Faces::new(faces, WALL, WALKABLE).doorway(p) { "door" } else { "    " },
            g.weights.get(p).copied().unwrap_or(0.0)
        );
    }
}

/// Enclosure runs up the rows, opening density across the columns — the orientation ch12's own figures
/// use, so a reader who knows those can read this.
fn heatmap(bins: &[u32]) {
    let total: u32 = bins.iter().sum();
    println!("\nHEATMAP — rows: enclosure (high at top).  columns: opening density");
    if total == 0 {
        println!("  (nothing in the histogram)");
        return;
    }
    let width = bins.iter().map(|c| c.to_string().len()).max().unwrap_or(1).max(4);
    for ex in (0..RANGES).rev() {
        let lo = ex as f32 / RANGES as f32;
        print!("  enc {lo:.2}-{:.2} |", lo + 1.0 / RANGES as f32);
        for ox in 0..RANGES {
            print!(" {:>width$}", bins[ex * RANGES + ox], width = width);
        }
        println!();
    }
    print!("  {:<14}|", "");
    for ox in 0..RANGES {
        print!(" {:>width$}", format!("{:.1}", ox as f32 * OPENING_MAX / RANGES as f32), width = width);
    }
    println!("   <- opening density, lower edge");
}

fn dump(run: &Run, source: Generator) {
    // **A separate file per generator.** The WFC counts are committed evidence for a verdict that has
    // already been written down; a constraint run overwriting them would quietly replace the thing
    // the verdict was read from.
    let name = match source {
        Generator::Wfc => "2026-08-10-expressive-range.bins.ron".to_owned(),
        Generator::Constraint => "2026-08-10-expressive-range.constraint.bins.ron".to_owned(),
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/research")
        .join(name);
    let rows: Vec<String> = (0..RANGES)
        .map(|ex| {
            let cols: Vec<String> =
                (0..RANGES).map(|ox| run.bins[ex * RANGES + ox].to_string()).collect();
            format!("        [{}],", cols.join(", "))
        })
        .collect();
    let text = format!(
        "// Raw bin counts, so a later reader can re-plot without re-solving.\n\
         // Rows are enclosure ranges (index 0 = [0, 1/6)), columns opening-density ranges.\n\
         (\n    region: {REGION},\n    solves_in_histogram: {},\n    did_not_converge: {},\n    \
         no_enclosed_region: {},\n    clamped_above_opening_max: {},\n    bins: [\n{}\n    ],\n)\n",
        run.total(),
        run.failed,
        run.no_region,
        run.clamped,
        rows.join("\n")
    );
    match std::fs::write(&path, text) {
        Ok(()) => println!("\nraw counts -> {}", path.display()),
        Err(e) => eprintln!("\ncould not write the counts: {e}"),
    }
}
