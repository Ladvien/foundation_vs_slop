# deterministic_solver — notes for agents

A Boolean satisfiability back-end: clauses in, bits out, incrementally, under assumptions.

## Source of truth

The source of truth for this crate is [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) at `crates/deterministic_solver/`. If you are reading this in a standalone `Ladvien/deterministic_solver` checkout, that is a read-only `git subtree split` mirror — **changes made here cannot be pulled back**. Make them upstream.

## Build and test

A leaf — `batsat`, and nothing else — so it builds and tests on its own: `cargo test -p deterministic_solver`.

## The non-negotiable: the answer is a function of the input, and nothing else

Not "usually deterministic". Not "deterministic on one machine". The same variables, the same clauses in the same order and the same assumptions must produce the same model on a Raspberry Pi, on an M5, in CI, and next year.

That is the entire reason the crate exists, so **anything that could make an answer depend on something other than the clauses is forbidden here**:

- **No wall clock.** No `Instant`, no `SystemTime`, no `Duration` — not in the API, not in the implementation. A solver that abandons a search on elapsed time answers differently on a busy machine, which turns a generated world into a function of what else was running. `Budget` counts **conflicts** for this reason, and adding a time-based sibling would defeat it.
- **No threads.** Two workers racing to a conflict clause order the clause database by scheduling.
- **No entropy, and no `std` hash map iteration.** `SolverOpts`' four randomness knobs are pinned explicitly in `pinned_opts()` even though `batsat` already defaults them to deterministic values — a default is a decision someone else can change in a patch release.

`tests/leaf.rs` is the dependency ratchet: `ALLOWED_DEPS` is `["batsat"]`. Widening it should cost an argument, because a dependency here is inherited by every caller and each one is another chance to consult a clock.

## Rules

- **No `unwrap()`**, no `expect` on caller-supplied data. A malformed literal is an error to name, not a panic — `0` and an out-of-range variable are both a caller bug, and absorbing them into a wrong answer is worse than refusing.
- **One path per feature.** No fallbacks. `Answer::Exhausted` is a real third outcome and must never be collapsed into "unsatisfiable": *"we did not find one"* is not *"there is not one"*, and a generator that treats them alike will quietly emit a degraded world.
- Leave academic paper references in comments where a paper informed the code.

## Things already paid for

- **`Lit::new`'s `sign` is `true` for the POSITIVE literal** — the opposite of MiniSat's convention, in a same-named parameter on a same-shaped API. Verified in `batsat-0.6.0/src/clause.rs`: `value_lit(v) = value_var(v.var()) ^ !v.sign()`. `the_model_reads_a_literal_the_same_way_the_solver_does` is the test that would catch getting it backwards.
- **`batsat` 0.6.0's own conflict budget is unreachable.** `conflict_budget` and `propagation_budget` are private, initialised to `-1`, and nothing in the crate ever writes them. The budget here goes through `Callbacks::stop`, which `within_budget` consults on the same line. If a future `batsat` exposes a setter, moving to it is fine — but re-measure, because the unit would change from "learnt clauses" to "conflicts".
- **The budget resets per `solve`.** A lifetime-cumulative count would make the tenth question in an optimisation loop answerable only if the first nine were cheap.
- **Every variable is allocated at construction**, so a model is full-width even for a variable in no clause. A short model would read as `false` by accident rather than by decision.

## What belongs here, and what does not

This crate decides. It does not optimise, and it must not learn to: there is no MaxSAT loop here and `Answer` has no cost, because weights mean something only to the caller that chose them.

What it owes such a loop is the two primitives one is built from — **assumptions** and an **unsatisfiable core** — and both are on `solve`. A caller guards each soft constraint with an indicator, assumes the indicators, and relaxes whatever the core names.

It also knows nothing about grids, tiles, prototypes or games. Literals are plain DIMACS `i32` precisely so that a caller can swap this crate out; a bespoke literal type would couple the caller to the thing it was supposed to be able to replace.
