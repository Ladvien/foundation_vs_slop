# bevy_fracture_modes — the non-negotiables

Read this before editing anything under `crates/bevy_fracture_modes/`. This directory **is** the root of a public mirror ([`Ladvien/bevy_fracture_modes`](https://github.com/Ladvien/bevy_fracture_modes)), split out of `Ladvien/foundation_vs_slop` with `git subtree split`. Changes flow **one way**: monorepo → mirror. Nothing is ever edited on the far side and nothing is pulled back.

## Every numerical routine is a fixed schedule

The eigen-initialisation is cyclic Jacobi for a fixed number of sweeps; the solver is a fixed number of ADMM steps; the Cholesky has no pivoting. None of them has a convergence test, and that is the design, not an omission: a convergence test is a branch on floating-point data, and a branch is where two machines part company. `a_bake_is_a_pure_function_of_its_inputs` asserts bit equality between two bakes. If you need more accuracy, raise `iterations` or `eigen_sweeps` — never add an early exit.

## The modes are scalar, and the direction comes back at impact

The paper's modes are vector fields. On a cell graph with one translation per cell, `E_D` cannot distinguish directions, so a vector mode is a scalar mode times a direction and the direction factors out of the gluing norm. Do not "upgrade" the unknowns to three per cell: it triples the work to recover a degeneracy, and it moves every golden.

## No linear-algebra dependency

`src/linalg.rs` exists so the solver's every operation is readable and fixed. `tests/leaf.rs` refuses `nalgebra`, `faer`, `ndarray` and `rand` by name.

## The plugin registers no systems

A bake needs a `CellGraph`, and only the crate that owns a decomposition knows when it is complete. The plugin adds two resources and stops. A system that polled for graphs would be a schedule this crate does not own.

## Verify

```sh
cargo test                  # unit + tests/leaf.rs + doctests
cargo build --release
cargo build --examples
```
