# emerge-core — notes for agents

The engine-free half of world building: the asset descriptor and map schemas, a constraint IR with three
solver backends, WFC, Poisson/Delaunay geometry, a seeded ChaCha8 RNG, comment-preserving RON surgery, a
hand-rolled glTF reader, and animation-clip measurement.

## Source of truth

The source of truth for this crate is [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop)
at `crates/emerge-core/`. If you are reading this in a standalone `Ladvien/emerge-core` checkout, that is
a read-only `git subtree split` mirror — **changes made here cannot be pulled back**. Make them upstream.

## Build and test

A leaf — `serde`, `serde_json`, `ron`, `rand`, `rand_chacha` — so it builds and tests on its own:
`cargo test -p emerge-core`.

## The non-negotiable: engine-free is enforced, not intended

`tests/engine_free.rs` is the ratchet. Allowed: `serde`, `serde_json`, `ron`, `rand`, `rand_chacha`.
Forbidden as substrings, so `bevy_math` is caught as readily as `bevy`: `bevy`, `avian`, `wgpu`,
`winit`. A second test scans the sources as a backstop for the case where someone adds the dependency
*and* edits the allowed list to match.

Why it is enforced rather than trusted: **three consumers share this crate** — a game, an offline search
that fans out across worker subprocesses, and a standalone editor — and none of them should have to
agree on a renderer in order to share a schema. That was a *comment* for months, and a comment cannot
fail a build. Widening the dependency list should cost an argument and a deliberate edit there.

**`DetRng` is the workspace's one generator.** `map_elites` depends on this crate solely for it, rather
than copying two methods, because a local copy would be a second definition of the generator every
reproducibility claim rests on. Changing a draw here moves goldens everywhere.

**The glTF reader is hand-rolled on purpose** (`glb.rs` reads the JSON chunk, data only). Pulling a glTF
crate would add "a second, differently-behaved reader of an asset the engine already parses its own
way".

**RON surgery preserves comments** because the files it edits are hand-authored and reviewed. A writer
that reformats them is not a faster version of this — it is a different, lossy tool.

No Bevy dependency exists here, and none may be added.

## Rules

- **No `unwrap()`**, no `expect` on parsed data. Every input to this crate is a file somebody wrote by
  hand; malformed input is an error to report, not a panic.
- **One path per feature.** No fallbacks, no legacy shims, no stub placeholders — do not write a
  degraded result on a solver failure, and do not silently default a missing field.
- Leave academic paper references in comments where a paper informed the code.

## In the monorepo

The game re-exports this at its old paths — `crate::{geom, rng, wfc}` (`src/lib.rs:169`),
`placement::{ir, manifest, scatter, solver, solvers}` (`src/placement/mod.rs:26`),
`site_editor::source_map`'s RON helpers (`src/site_editor/source_map.rs:44`) — so the extraction moved
no caller. Root `CLAUDE.md` and `TESTING.md` carry the project-wide rules; neither is part of this
mirror.
