# bevy_viscera — notes for agents

XPBD Cosserat-style strands with a tearing mesenteric membrane: guts that spill, fall with weight, coil on the floor, stay tethered, and tear loose.

## Source of truth

The source of truth for this crate is [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) at `crates/bevy_viscera/`. If you are reading this in a standalone `Ladvien/bevy_viscera` checkout, that is a read-only `git subtree split` mirror — **changes made here cannot be pulled back**. Make them upstream.

## Build and test

A leaf on `bevy` alone: `cargo test -p bevy_viscera`, then `cargo run -p bevy_viscera --example rod_determinism` for the proof that actually matters. `cargo check -p bevy_viscera --all-targets` covers the windowed example too.

## The non-negotiable: the answer must not depend on how hard the machine was working

This crate's product is not viscera. It is *reproducible* viscera, and every rule below exists to keep one number — `Strand::digest()` — a function of the inputs and nothing else.

- **The substep count, the iteration count and the constraint sequence are fixed.** Four substeps, eight passes, stretch → bend → mesentery → floor, ascending index order throughout. They are named constants (`DEFAULT_SUBSTEPS`, `DEFAULT_ITERATIONS`) and they are not tuned per call: a solver that spent more iterations on a nearby strand than on a distant one would give a different answer for the same input.
- **There is no early-out.** Every iteration walks the whole sequence whether or not the residual is already zero. Constraints are *skipped* only by data — torn, degenerate, or slack — never by convergence, so the pass count is identical on every run. If you find yourself adding `if residual < eps { break }`, you are deleting the crate's reason to exist.
- **Nothing reads a clock.** `FIXED_DT` is a constant. Bevy's `Time<Fixed>` defaults to 64 Hz, which means the shipped plugin runs 60 Hz physics on a 64 Hz tick unless the app says `Time::<Fixed>::from_hz(60.0)`; that is a documented caveat, not a bug to fix by reading `Time`. Reading it would make the digest a function of a runtime setting.
- **Nothing draws from an RNG.** `spill` derives every per-strand quantity from its `seed` through `src/hash.rs`. Both hashes there are frozen; changing either moves every digest the crate has ever printed.
- **Tearing is monotone.** A `torn` flag is set and never cleared, in `Strand` and in `Mesentery` alike. That is what makes a tear a state change rather than a threshold the sim can chatter across. The mesentery's `torn` is parallel to its `anchors` and the solver's `canonicalise` sorts them *together*; a change that reorders one without the other migrates a tear to a different link, which is the same bug as clearing it.

## Rules

- **Bevy 0.19 is pinned.** Read the vendored `bevy-0.19.0` source, not bevy.org — that documents `main` and has been wrong for this pin more than once. Three traps already paid for: a missing `Res<T>` **panics** its system rather than skipping it, `Resource` is a subtrait of `Component` so you cannot derive both, and `add_plugins` tuples cap at 15.
- **No `unwrap()`**, no `expect`, no panicking index. Every index in `src/solver.rs` is proved in range by the single `let n = …min()…` line at the top of `solve_one`; keep it that way rather than adding a guard per access.
- **One path per feature.** No fallbacks, no legacy shims, no stub placeholders. Degenerate input is *clamped and reported*, which is one path with a guard, not two paths.
- **The crate never spawns.** `tube_mesh` returns a `Mesh`. Choosing the material, the handle, the parent and the schedule slot is the caller's job, and a crate that chose them would be unusable in any game that wanted different ones.

## Where the boundary falls

Four things belong to the caller, and each is a thing this crate deliberately cannot do:

- **Rendering.** A `Mesh` comes back; nothing is spawned, no material is chosen, no asset arena is written.
- **The wound.** Where guts come from, when, and how many, is gameplay. `spill` takes a point, a direction, a count and a seed.
- **Collision beyond the floor.** Strands do not see each other and do not see the world. Self-collision on a coiling rope needs a broadphase the caller already owns.
- **Blood.** `tests/leaf.rs` allows exactly one dependency, `bevy`. A rope solver has no business knowing about rheology, and widening that list should cost a deliberate edit rather than a passing build.

## Interpretations recorded at the time

Two numbers in the contract needed a reading, and both are written down at the constant rather than buried in behaviour:

- **A mesenteric link is a pin — rest length zero — and its strain is `|node − anchor| / rest_len`**, because `Mesentery` carries no length of its own and the strand's segment rest length is the only length scale in the data. The other reading, a link whose rest length is one segment, was tried and is measurably wrong: it leaves the link slack for 35 mm and then gives it 12 mm of working range, which a node already at terminal velocity crosses inside one substep, so every tether tore, always, and the flag stopped meaning anything.
- **`COMPLIANCE_MESENTERY` is a crate constant, not a `ViscSettings` dial**, and its value is derived rather than eyeballed — the derivation is in its doc comment. A compliant XPBD constraint settles at `C = (1 + α̃)·g·Δt²·N` for `N` nodes of hanging weight, so the compliance is what decides the capacity, and both neighbourhoods of the shipped `6e-5` are dead: far softer and everything tears, far stiffer and nothing ever does.
