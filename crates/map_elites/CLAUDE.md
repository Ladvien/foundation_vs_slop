# map_elites — notes for agents

A Quality-Diversity kernel: the MAP-Elites archive, three emitter loops (isotropic, CMA-ME, CMA-MAE), separable CMA-ES, and a POET outer loop. Engine-free, and bit-reproducible from a single `u64`.

## Source of truth

The source of truth for this crate is [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) at `crates/map_elites/`. If you are reading this in a standalone `Ladvien/map_elites` checkout, that is a read-only `git subtree split` mirror — **changes made here cannot be pulled back**. Make them upstream.

## Build and test

**Not a leaf.** It path-depends on the sibling `det_rng` for the seeded generator, so it builds *inside* the workspace, not on its own: `cargo test -p map_elites`.

That dependency is deliberate and should not be removed by copying the two RNG methods locally. A local copy would be a **second definition** of the generator every reproducibility claim rests on, and this crate's whole value is that a run replays exactly from its seed.

It used to depend on `emerge-core` for the same generator. That crate is GPL-3.0-only, which meant this MIT-OR-Apache crate could only be built against copyleft — a trap for anyone who adopted it on the strength of its licence. `det_rng` is the same code lifted out under a permissive licence; the stream did not change, and `tests/rng_guard.rs` upstream still freezes the same bits.

## The non-negotiables

**No evaluator. Ever.** Every loop is generic over the genome type `G` and takes mutation and evaluation as closures, because "how good is this genome" is the one question only the caller can answer. That is exactly what lets this crate stay free of any engine — and adding an evaluator would drag the whole game back in behind it.

**Bit-reproducible from one `u64`.** Every draw goes through a single seeded `ChaCha8Rng`; every archive walk is over a `BTreeMap`. Anything that could perturb that is forbidden: thread pools, entropy sources, `Instant`, `HashMap` iteration order, a float reduction whose order depends on scheduling. Adding `bevy` here would compile fine and nobody would notice until a golden moved.

**`tests/engine_free.rs` is the ratchet** — a manifest check plus a source scan, in the spirit of the parent repo's `tests/determinism_lint.rs`. Widening the dependency list should cost an argument and a deliberate edit there, not a passing `cargo build`.

No Bevy dependency exists here, and none should be added.

## Rules

- **No `unwrap()`**, no `expect` on caller data. A search that runs for forty minutes must not die on an index.
- **One path per feature.** No fallbacks, no legacy shims, no stub placeholders — a QD run with two execution paths produces an archive nobody can explain.

## In the monorepo

The game reaches this through `squad_ai`, which re-exports the modules at their old paths (`src/squad_ai/mod.rs:43+`). **Inside `squad_ai`, write `::map_elites::` with leading colons for the crate** — the bare path resolves to `squad_ai::map_elites`, which is an alias for `::map_elites::loops`. Root `CLAUDE.md` and `TESTING.md` carry the project-wide rules; neither is part of this mirror.
