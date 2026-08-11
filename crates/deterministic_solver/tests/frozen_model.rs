//! **The crate's contract, pinned to an exact value.**
//!
//! Every other test here asks whether an answer is *correct*. These ask whether it is **the same
//! one**, which is a different and stricter question — and the one this crate exists to answer yes to.
//!
//! # Why an exact hash rather than "solve it twice and compare"
//!
//! `the_same_problem_gives_the_same_model_every_time` in `src/lib.rs` already solves an instance
//! repeatedly and compares. That catches entropy, clock-reading and hash-map iteration order — a
//! solver that varies *within* a machine. **It cannot catch a solver that is perfectly consistent on
//! each machine and different between them**, because both sides pass it independently.
//!
//! A literal expected value can. The constants below were measured on aarch64 macOS; CI runs this same
//! file on x86_64 Linux, so the two architectures are compared by both being compared to the same
//! number. That converts the argument in `docs/research/2026-08-10-solver-choice.md` §2.4 — that pure
//! Rust has no divergence mechanism where C++ float contraction does — from reasoning into a
//! measurement, which §8 of that document listed as its largest unverified claim.
//!
//! # What a failure here means
//!
//! **Not a bug in `batsat`.** A solver is free to return any model it likes and to change which one
//! between releases; nothing about that is incorrect. It is a *goldens-moving event here*, because
//! this project generates worlds from exactly these bits and a level that quietly changed shape is
//! the least debuggable failure available.
//!
//! So a red here means: the solver's heuristics moved, or the platform does. Find out which,
//! deliberately, and then re-pin. It must never be re-pinned as a reflex — `assert_eq!` on a hash is
//! only worth having if updating it costs a thought.

use deterministic_solver::{Answer, Budget, Literal, Solver};

/// FNV-1a over the assignment.
///
/// **Written out rather than taken from `std::collections::hash_map::DefaultHasher`**, whose
/// documentation says outright that its output is not guaranteed stable across Rust releases. A
/// frozen value computed with an unfrozen hash is not frozen — it would go red on a toolchain bump
/// and say "the solver changed", which is exactly the wrong diagnosis to hand someone.
fn fnv1a(bits: &[bool]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bits {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A deterministic stream, so the "random" instance below is the same one on every machine.
///
/// The multiplier and increment are PCG's; only the low-quality raw output is used, which is all a
/// fixture generator needs. Nothing here draws from the environment.
struct Stream(u64);

impl Stream {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        // Xorshift the state on the way out so consecutive draws do not share low-bit structure.
        let x = self.0;
        (x ^ (x >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }
}

/// N queens, as clauses. Structured, and with 14,200 solutions at n = 12 — so a solver choosing
/// differently has plenty of room to show it.
fn queens_instance(n: usize) -> Result<(Solver, Vec<Vec<Literal>>), String> {
    let var = |r: usize, c: usize| (r * n + c + 1) as Literal;
    let mut s = Solver::new(n * n)?;
    let mut clauses: Vec<Vec<Literal>> = Vec::new();
    let add = |s: &mut Solver, c: Vec<Literal>, clauses: &mut Vec<Vec<Literal>>| -> Result<(), String> {
        s.add_clause(&c)?;
        clauses.push(c);
        Ok(())
    };
    for r in 0..n {
        let row: Vec<Literal> = (0..n).map(|c| var(r, c)).collect();
        add(&mut s, row.clone(), &mut clauses)?;
        for (i, &a) in row.iter().enumerate() {
            for &b in &row[i + 1..] {
                add(&mut s, vec![-a, -b], &mut clauses)?;
            }
        }
    }
    for c in 0..n {
        let col: Vec<Literal> = (0..n).map(|r| var(r, c)).collect();
        for (i, &a) in col.iter().enumerate() {
            for &b in &col[i + 1..] {
                add(&mut s, vec![-a, -b], &mut clauses)?;
            }
        }
    }
    for d in 0..(2 * n - 1) {
        let down: Vec<Literal> = (0..n)
            .filter_map(|r| (d + r).checked_sub(n - 1).filter(|c| *c < n).map(|c| var(r, c)))
            .collect();
        for (i, &a) in down.iter().enumerate() {
            for &b in &down[i + 1..] {
                add(&mut s, vec![-a, -b], &mut clauses)?;
            }
        }
        let up: Vec<Literal> =
            (0..n).filter_map(|r| d.checked_sub(r).filter(|c| *c < n).map(|c| var(r, c))).collect();
        for (i, &a) in up.iter().enumerate() {
            for &b in &up[i + 1..] {
                add(&mut s, vec![-a, -b], &mut clauses)?;
            }
        }
    }
    Ok((s, clauses))
}

/// A random 3-SAT instance at clause/variable ratio 4.0 — just under the satisfiability threshold
/// (~4.27), so it is satisfiable but only after real search.
///
/// **This is the fixture that would actually notice a heuristic change.** N-queens is solved with
/// little backtracking; an instance near the phase transition spends thousands of conflicts, so which
/// model comes out is a function of the whole search — restart policy, clause deletion, activity
/// decay. If any of that moved, this hash moves.
fn random_3sat(vars: usize, clauses: usize, seed: u64) -> Result<(Solver, Vec<Vec<Literal>>), String> {
    let mut rng = Stream(seed);
    let mut s = Solver::new(vars)?;
    let mut out: Vec<Vec<Literal>> = Vec::with_capacity(clauses);
    for _ in 0..clauses {
        let mut clause: Vec<Literal> = Vec::with_capacity(3);
        while clause.len() < 3 {
            let v = (rng.below(vars as u64) + 1) as Literal;
            if clause.iter().any(|l| l.abs() == v) {
                continue;
            }
            clause.push(if rng.below(2) == 0 { v } else { -v });
        }
        s.add_clause(&clause)?;
        out.push(clause);
    }
    Ok((s, out))
}

fn satisfies(bits: &[bool], clauses: &[Vec<Literal>]) -> bool {
    let holds = |l: Literal| {
        bits.get(l.unsigned_abs() as usize - 1).copied().unwrap_or(false) == (l > 0)
    };
    clauses.iter().all(|c| c.iter().copied().any(holds))
}

/// The model `batsat` returns for 12-queens, hashed. Measured on aarch64 macOS 26.4.1, Rust stable,
/// `batsat 0.6.0`.
const FROZEN_QUEENS_12: u64 = 0x3988_f9fa_171f_e585;
/// The model for the near-threshold 3-SAT instance. Same provenance.
const FROZEN_3SAT: u64 = 0x3774_9da4_0b33_c695;

#[test]
fn the_model_for_twelve_queens_is_frozen() {
    let (mut s, clauses) = queens_instance(12).expect("the fixture builds");
    let answer = s.solve(&[], Budget::UNLIMITED).expect("a well-formed solve");
    let Answer::Satisfied(m) = answer else {
        panic!("12 queens is satisfiable; got {answer:?}");
    };
    // Validity first, and independently of the hash — so a hash re-pinned carelessly still cannot
    // freeze in a wrong answer.
    assert!(satisfies(m.values(), &clauses), "the frozen model must actually satisfy the instance");
    let h = fnv1a(m.values());
    println!("FROZEN_QUEENS_12 = 0x{h:016x}");
    assert_eq!(
        h, FROZEN_QUEENS_12,
        "\nthe model for 12-queens moved.\n\
         This is NOT necessarily a bug: a solver may return any model it likes, and may change which \
         one between releases.\n\
         It IS a goldens-moving event for anything downstream that generates content from these bits.\n\
         Find out WHICH changed — `batsat`'s version, the toolchain, or the platform — before re-pinning."
    );
}

#[test]
fn the_model_for_a_near_threshold_3sat_instance_is_frozen() {
    let (mut s, clauses) = random_3sat(220, 880, 0x5EED_1234_ABCD_0001).expect("the fixture builds");
    let answer = s.solve(&[], Budget::UNLIMITED).expect("a well-formed solve");
    let Answer::Satisfied(m) = answer else {
        panic!("this instance was chosen to be satisfiable; got {answer:?}");
    };
    assert!(satisfies(m.values(), &clauses), "the frozen model must actually satisfy the instance");
    let h = fnv1a(m.values());
    println!("FROZEN_3SAT = 0x{h:016x}");
    assert_eq!(
        h, FROZEN_3SAT,
        "\nthe model for the near-threshold 3-SAT instance moved.\n\
         This fixture spends real search, so it is the sensitive one: restart policy, clause deletion \
         and activity decay all feed it.\n\
         See the note on the test above before re-pinning."
    );
}

#[test]
fn the_fixtures_are_reproducible_within_a_process() {
    // Guards the fixtures themselves rather than the solver: if `Stream` or the queens builder ever
    // became order-dependent, the frozen hashes above would be pinning a moving target and the
    // failure would look like a solver regression.
    let a = random_3sat(60, 240, 7).expect("builds").1;
    let b = random_3sat(60, 240, 7).expect("builds").1;
    assert_eq!(a, b, "the instance generator must be a function of its seed");
    let q1 = queens_instance(6).expect("builds").1;
    let q2 = queens_instance(6).expect("builds").1;
    assert_eq!(q1, q2, "the queens encoding must be emitted in one order");
}
