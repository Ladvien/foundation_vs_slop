//! **N queens on an N×N board, none attacking another — and a refusal where there is nothing to find.**
//!
//! ```text
//! cargo run --example n_queens          # boards for n = 2..=12
//! cargo run --example n_queens 20       # one board, any size
//! ```
//!
//! The canonical satisfiability demonstration, and the one that shows both halves of [`Answer`] in a
//! single run: `n = 2` and `n = 3` have **no** solution, and a solver's job there is to say so rather
//! than to return the closest thing it found. That distinction is the whole reason this crate refuses
//! instead of approximating — a generator handed a near-miss cannot tell it from a hit.
//!
//! The constraints are all "at most one of these", which is worth seeing written out: it is the same
//! shape as *"exactly one prototype per cell"* in a tile-based world generator, which is what this
//! solver is here to back.
//!
//! Terminal output, no GPU.

use deterministic_solver::{Answer, Budget, Literal, Solver};

fn main() {
    let sizes: Vec<usize> = match std::env::args().nth(1).and_then(|a| a.parse().ok()) {
        Some(n) => vec![n],
        None => (2..=12).collect(),
    };

    println!("N-QUEENS — no two queens share a row, column or diagonal");
    println!("{}", "=".repeat(56));

    for n in sizes {
        match queens(n) {
            Ok(Some(board)) => {
                println!("\nn = {n}");
                for row in &board {
                    let cells: Vec<&str> =
                        row.iter().map(|&q| if q { "\u{265b}" } else { "\u{00b7}" }).collect();
                    println!("  {}", cells.join(" "));
                }
            }
            Ok(None) => println!("\nn = {n}   no arrangement exists"),
            Err(e) => println!("\nn = {n}   {e}"),
        }
    }
}

/// A queen stands on row `r`, column `c`. One-based, because `0` cannot be a [`Literal`].
fn var(r: usize, c: usize, n: usize) -> Literal {
    (r * n + c + 1) as Literal
}

/// No two of `lits` are true at once, stated pairwise.
///
/// Quadratic in `lits.len()`, and the right choice at this size: a counter encoding would allocate
/// auxiliary variables to save clauses, and below roughly twenty literals the clauses are cheaper
/// than the variables. It also keeps every variable in the problem meaning something, which matters
/// when reading a model back.
fn at_most_one(s: &mut Solver, lits: &[Literal]) -> Result<(), String> {
    for (i, &a) in lits.iter().enumerate() {
        for &b in &lits[i + 1..] {
            s.add_clause(&[-a, -b])?;
        }
    }
    Ok(())
}

fn queens(n: usize) -> Result<Option<Vec<Vec<bool>>>, String> {
    if n == 0 {
        return Ok(Some(Vec::new()));
    }
    let mut s = Solver::new(n * n)?;

    // Exactly one queen per row: at least one...
    for r in 0..n {
        let row: Vec<Literal> = (0..n).map(|c| var(r, c, n)).collect();
        s.add_clause(&row)?;
        at_most_one(&mut s, &row)?; // ...and at most one.
    }
    // At most one per column. "At least" is implied: n queens in n rows, one per column at most.
    for c in 0..n {
        let col: Vec<Literal> = (0..n).map(|r| var(r, c, n)).collect();
        at_most_one(&mut s, &col)?;
    }
    // At most one per diagonal, both directions. Indexed by the two quantities constant along a
    // diagonal: `r - c` for one family (offset to stay non-negative) and `r + c` for the other.
    for d in 0..(2 * n - 1) {
        let down: Vec<Literal> = (0..n)
            .filter_map(|r| (d + r).checked_sub(n - 1).filter(|c| *c < n).map(|c| var(r, c, n)))
            .collect();
        at_most_one(&mut s, &down)?;
        let up: Vec<Literal> =
            (0..n).filter_map(|r| d.checked_sub(r).filter(|c| *c < n).map(|c| var(r, c, n))).collect();
        at_most_one(&mut s, &up)?;
    }

    match s.solve(&[], Budget::conflicts(2_000_000))? {
        Answer::Satisfied(m) => Ok(Some(
            (0..n)
                .map(|r| (0..n).map(|c| m.get(var(r, c, n).unsigned_abs())).collect())
                .collect(),
        )),
        Answer::Unsatisfiable { .. } => Ok(None),
        Answer::Exhausted => Err("gave up inside the conflict budget".to_owned()),
    }
}
