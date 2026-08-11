# deterministic_solver

> ⚠️ **Vibe Coded** — written by an AI agent working from a human's direction. It is used in a shipping game and covered by tests, but it has had no line-by-line human audit. Read it before you trust it.

A Boolean satisfiability back-end that answers the same way every run, on every machine. No clock, no threads, no entropy, and a give-up budget counted in conflicts rather than seconds.

![A dark chessboard cycling through board sizes: at 2x2 and 3x3 the squares turn dull red and the caption reads "no arrangement exists", then from 4x4 up to 12x12 amber discs appear, one per row and column with none sharing a diagonal, the grid growing finer at each step](docs/n_queens.gif)

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. Issues and PRs belong upstream.

```rust
use deterministic_solver::{Answer, Budget, Solver};

let mut s = Solver::new(3)?;
s.add_clause(&[1, 2])?;      // (x1 ∨ x2)
s.add_clause(&[-1, 3])?;     // (¬x1 ∨ x3)
s.add_clause(&[-2, -3])?;    // (¬x2 ∨ ¬x3)

match s.solve(&[], Budget::conflicts(10_000))? {
    Answer::Satisfied(m) => println!("x1={} x2={} x3={}", m.get(1), m.get(2), m.get(3)),
    Answer::Unsatisfiable { core } => println!("no arrangement exists; blamed {core:?}"),
    Answer::Exhausted => println!("gave up — at the same point it will give up next time"),
}
# Ok::<(), String>(())
```

## Why "deterministic" is in the name

Because it is the property being bought, and it is not the property most solvers optimise for.

This crate exists to decide what a procedural world generator emits. That generator's whole contract is that a seed reproduces a run — the same seed on a laptop, on a Raspberry Pi, in CI, next year. A solver that is *usually* deterministic breaks that contract in the least debuggable way available: intermittently, months later, in content rather than in a crash.

Three ways a SAT solver quietly stops being a function of its input, all of which were found in shipping crates while choosing this one:

- **It gives up on a wall clock.** A busy machine answers differently from an idle one. `splr` returns `TimeOut` off `Instant::now()`, and the timeout cannot be disabled — setting it to zero makes the first stage boundary time out immediately.
- **It gives up not at all.** Then a pathological instance has no reproducible outcome, because your only recourse is to kill it from outside. `varisat` has had an open issue about this since 2019.
- **It iterates a hash map** whose iteration order is seeded from the environment.

`Budget` is counted in **conflicts** for the first reason. There is no `Duration` anywhere in this API, and that is a deliberate omission rather than an oversight.

## What it does not do

It decides satisfiability. It does not optimise: there is no MaxSAT loop here, and `Answer` has no notion of a cost.

That is on purpose — optimisation belongs to whoever knows what the weights *mean*. What this crate provides is the three things such a loop is built from:

- **assumptions** — literals forced true for one question only;
- an **unsatisfiable core** — the subset of those assumptions that suffices for the refusal;
- **`add_var`** — so the loop can introduce counter variables mid-search, which it could not have known to ask for up front.

Guard each soft constraint with an indicator variable, assume the indicators, and price whatever the core names. That is enough to build core-guided MaxSAT (OLL/RC2) on top without this crate learning what a weight is.

## Preferences: steering the answer without constraining it

`Solver::with_preferences` gives each variable an optional value to try first. It changes *which* model comes back, never which models exist — the search backtracks out of a preference like any other decision.

This is the cheap way to get variety. Expressing "I'd like this cell to be that tile" as a soft constraint makes the solver prove it found the arrangement *closest* to the whole wish-list, which is core-guided search over hundreds of units. Measured on a 12×12 tile problem: **9,171 ms as soft constraints, 15 ms as preferences**, for a guarantee about proximity to a random draw that nobody needed.

Determinism is untouched: the preferences are part of the input, so the answer is still a function of what the solver was given.

## Budgets bound a question, not a lifetime

The conflict count resets on every `solve`, so a budget means "this question may cost this much". The alternative — counting from construction — would make the tenth question in a loop answerable only if the first nine happened to be cheap, which is not a bound anyone could predict.

## One caveat about the underlying solver, measured rather than assumed

This wraps [`batsat`](https://crates.io/crates/batsat) (MiniSat's algorithm, in Rust, one transitive dependency). **`batsat` 0.6.0 has `conflict_budget` and `propagation_budget` fields and no public way to set them** — they are initialised to `-1` and nothing in the crate ever writes them, so its documented budget is unreachable from outside.

What *is* reachable is `Callbacks::stop`, which `within_budget` consults on the same line. So this crate counts conflicts itself, through the callback interface, and stops there. The unit is therefore "learnt clauses", which tracks conflicts closely but is not promised to equal them — irrelevant to the property that matters, since it is a deterministic function of the search either way.

`SolverOpts`' four randomness knobs are pinned explicitly rather than left at their defaults. They already default to deterministic values; a default is a decision someone else can change in a patch release.

## Examples

```sh
cargo run --example graph_colouring    # colour a map, and watch it refuse an impossible one
cargo run --example soft_constraints   # assumptions + unsat cores: the shape a MaxSAT loop is built from
```

Both print to the terminal and need no GPU.

## Licence

MIT OR Apache-2.0.
