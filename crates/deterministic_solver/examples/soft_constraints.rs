//! **Assumptions and unsatisfiable cores — the two pieces a MaxSAT loop is built from.**
//!
//! ```text
//! cargo run --example soft_constraints
//! ```
//!
//! This crate decides; it does not optimise. But *"satisfy as many of these as possible"* is what
//! anyone actually wants from a constraint solver, so this example builds that loop from the outside
//! to show the two primitives doing the work.
//!
//! The trick is a guard variable. A soft constraint's clauses each carry `¬guard`, so setting the
//! guard false relaxes the constraint and setting it true enforces it. Then **assume every guard**:
//! if that is satisfiable, everything holds at once; if not, the **unsatisfiable core** names guards
//! that cannot hold together — and relaxing one of them is a step toward the best available answer.
//!
//! This is the shape of RC2 (Ignatiev, Morgado & Marques-Silva, *RC2: an Efficient MaxSAT Solver*,
//! JSAT 2019), minus the cardinality constraints that make it find a proven optimum. What it finds
//! here is a maximal satisfiable set, not a maximum one — stated plainly rather than overclaimed.
//!
//! The scenario: seat five guests at a table, where some pairs want to sit together and one guest
//! refuses to sit with another. Not every wish can be granted.

use deterministic_solver::{Answer, Budget, Literal, Solver};

const GUESTS: [&str; 5] = ["Ada", "Bo", "Cy", "Di", "Eli"];
/// Two seats. `seat(g)` true means guest `g` sits on the left.
const SEATS: [&str; 2] = ["left", "right"];

/// Pairs who wish to share a side, and how much they mind not getting it.
const WISHES: [(usize, usize, u32); 4] = [(0, 1, 5), (1, 2, 4), (2, 3, 3), (3, 4, 2)];
/// A pair who must NOT share a side. Hard — it is not up for negotiation.
const FEUD: (usize, usize) = (0, 4);

fn main() {
    match run() {
        Ok(()) => {}
        Err(e) => eprintln!("{e}"),
    }
}

fn run() -> Result<(), String> {
    // Variables 1..=5 are the guests' seats. Variables 6..=9 are one guard per wish.
    let guard = |w: usize| (GUESTS.len() + 1 + w) as Literal;
    let seat = |g: usize| (g + 1) as Literal;
    let mut s = Solver::new(GUESTS.len() + WISHES.len())?;

    // Hard: the feuding pair sit on opposite sides. Both directions, so neither can share with the
    // other whichever side each takes.
    s.add_clause(&[seat(FEUD.0), seat(FEUD.1)])?;
    s.add_clause(&[-seat(FEUD.0), -seat(FEUD.1)])?;

    // Soft: each wish says its pair share a side, guarded so it can be given up.
    for (w, &(a, b, _)) in WISHES.iter().enumerate() {
        s.add_clause(&[-guard(w), seat(a), -seat(b)])?;
        s.add_clause(&[-guard(w), -seat(a), seat(b)])?;
    }

    println!("SEATING — {} guests, {} wishes, 1 feud\n", GUESTS.len(), WISHES.len());

    // Start by assuming every wish is granted, then give up whatever the core blames until the rest
    // fit together. Each round costs one solve and the solver keeps everything it has learnt.
    let mut assumed: Vec<Literal> = (0..WISHES.len()).map(guard).collect();
    let mut relaxed: Vec<usize> = Vec::new();

    let model = loop {
        match s.solve(&assumed, Budget::conflicts(100_000))? {
            Answer::Satisfied(m) => break m,
            Answer::Exhausted => return Err("gave up inside the conflict budget".to_owned()),
            Answer::Unsatisfiable { core } => {
                // No assumptions left to blame means the HARD constraints are the problem, and no
                // amount of relaxing wishes will help. Refuse rather than loop forever.
                let Some(&blamed) = core.first() else {
                    return Err("the seating is impossible before any wish is considered".to_owned());
                };
                let Some(w) = (0..WISHES.len()).find(|&w| guard(w) == blamed) else {
                    return Err(format!("the core named {blamed}, which is not a wish"));
                };
                println!("  cannot grant every wish — giving up {} + {}", GUESTS[WISHES[w].0], GUESTS[WISHES[w].1]);
                assumed.retain(|&l| l != blamed);
                relaxed.push(w);
            }
        }
    };

    println!("\nSEATS");
    for (g, name) in GUESTS.iter().enumerate() {
        let side = if model.get(seat(g).unsigned_abs()) { SEATS[0] } else { SEATS[1] };
        println!("  {name:<4} {side}");
    }

    let cost: u32 = relaxed.iter().filter_map(|&w| WISHES.get(w).map(|x| x.2)).sum();
    println!("\n  wishes granted   {} of {}", WISHES.len() - relaxed.len(), WISHES.len());
    println!("  weight given up  {cost}");
    println!(
        "\n  This is a MAXIMAL satisfiable set, not a maximum one: the loop gives up whatever the\n  \
         core happens to name first, and never reconsiders. Finding a proven optimum needs a\n  \
         cardinality constraint over the guards — which is the caller's job, not this crate's."
    );
    Ok(())
}
