//! **Colour a map so no two neighbours share a colour — then ask for one colour too few.**
//!
//! ```text
//! cargo run --example graph_colouring
//! ```
//!
//! Graph colouring is the shortest honest demonstration of what a SAT solver is for: the constraints
//! are trivial to state, the search is not, and the interesting half of the output is the *refusal*.
//! Australia's mainland states are the textbook instance (Russell & Norvig, *AIA* ch. 6) because they
//! are famously 3-colourable and just as famously not 2-colourable, so one graph shows both answers.
//!
//! Terminal output, no GPU — `docs/bevy_plugins.md`'s preference, and a solver has nothing to draw.

use deterministic_solver::{Answer, Budget, Literal, Solver};

/// The mainland states, in a fixed order so the variable numbering is stable.
const REGIONS: [&str; 6] = ["WA", "NT", "SA", "Q", "NSW", "V"];

/// Which regions share a border.
const BORDERS: [(usize, usize); 9] = [
    (0, 1), // WA-NT
    (0, 2), // WA-SA
    (1, 2), // NT-SA
    (1, 3), // NT-Q
    (2, 3), // SA-Q
    (2, 4), // SA-NSW
    (2, 5), // SA-V
    (3, 4), // Q-NSW
    (4, 5), // NSW-V
];

const COLOUR_NAMES: [&str; 3] = ["red", "green", "blue"];

fn main() {
    for colours in [3usize, 2] {
        println!("\n{} colours", colours);
        println!("{}", "-".repeat(40));
        match colour(colours) {
            Ok(Some(assignment)) => {
                for (r, c) in REGIONS.iter().zip(&assignment) {
                    println!("  {r:<4} {}", COLOUR_NAMES.get(*c).copied().unwrap_or("?"));
                }
            }
            Ok(None) => println!("  no colouring exists — refused, rather than approximated"),
            Err(e) => println!("  {e}"),
        }
    }
}

/// Variable numbering: region `r` takes colour `c` is `r * colours + c + 1`.
///
/// One-based because that is DIMACS, where `0` is the clause terminator and so cannot name a
/// variable — the reason [`Literal`] rejects it.
fn var(r: usize, c: usize, colours: usize) -> Literal {
    (r * colours + c + 1) as Literal
}

fn colour(colours: usize) -> Result<Option<Vec<usize>>, String> {
    let mut s = Solver::new(REGIONS.len() * colours)?;

    // Every region takes at least one colour...
    for r in 0..REGIONS.len() {
        let any: Vec<Literal> = (0..colours).map(|c| var(r, c, colours)).collect();
        s.add_clause(&any)?;
        // ...and no more than one. Pairwise: at this size the quadratic clause count is smaller than
        // the auxiliary variables a counter encoding would allocate.
        for c in 0..colours {
            for d in (c + 1)..colours {
                s.add_clause(&[-var(r, c, colours), -var(r, d, colours)])?;
            }
        }
    }

    // No two neighbours share a colour.
    for &(a, b) in &BORDERS {
        for c in 0..colours {
            s.add_clause(&[-var(a, c, colours), -var(b, c, colours)])?;
        }
    }

    match s.solve(&[], Budget::conflicts(100_000))? {
        Answer::Satisfied(m) => {
            let mut out = Vec::with_capacity(REGIONS.len());
            for r in 0..REGIONS.len() {
                match (0..colours).find(|&c| m.get(var(r, c, colours).unsigned_abs())) {
                    Some(c) => out.push(c),
                    // Cannot happen — the at-least-one clause forbids it — but reading a missing
                    // colour as 0 would print a plausible map that the solver never chose.
                    None => return Err(format!("the model left {} uncoloured", REGIONS[r])),
                }
            }
            Ok(Some(out))
        }
        Answer::Unsatisfiable { .. } => Ok(None),
        Answer::Exhausted => Err("gave up inside the conflict budget".to_owned()),
    }
}
