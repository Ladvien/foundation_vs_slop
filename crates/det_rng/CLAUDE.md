# det_rng — notes for agents

One deterministic RNG for a whole simulation: a seeded ChaCha8 stream plus the unbiased integer and float draws generators need. A run reproduces from a single `u64`.

## Source of truth

The source of truth for this crate is [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) at `crates/det_rng/`. If you are reading this in a standalone `Ladvien/det_rng` checkout, that is a read-only `git subtree split` mirror — **changes made here cannot be pulled back**. Make them upstream.

## Build and test

A leaf — `rand` and `rand_chacha`, nothing else — so it builds and tests on its own: `cargo test -p det_rng`.

## The non-negotiable: these bits never move

**Treat the output of `seeded(s)` as a public API surface, not an implementation detail.** Several crates and an offline search depend on a run replaying exactly from its seed; upstream, `tests/rng_guard.rs` freezes the first draws of `raw_u64`, `unit` and `below` as literal integers, and every replay golden in that project sits downstream of them. A change here does not fail locally — it fails as a golden moving in a repository that does not obviously depend on you.

So: no swapping ChaCha8 for something faster, no "harmless" reordering of the `unit` construction, no changing which `rand` sampler `below` calls. If a change to the stream is genuinely wanted, it is a breaking change and the goldens are re-pinned deliberately, in the same commit, with the reason written down.

**`below` uses `rand`'s range sampler, not `% n`.** The modulo version is biased toward low values whenever `n` does not divide the range. It looks correct, reviews fine, and quietly skews long runs — a room type drawn a hundred thousand times comes out "slightly more of the first kind", which is a design change nobody made. Do not simplify it back.

**No entropy, ever.** Nothing may read the clock, the OS RNG, a thread id, or an address. A generator that can seed itself can produce a run nobody can reproduce, which is the failure this crate exists to prevent.

**No global generator.** There is deliberately no ambient `rng()`. A shared mutable stream makes the order of draws depend on execution order — precisely what determinism must not depend on. Callers own their generator and pass it.

## Why this crate exists at all

It was lifted out of a GPL-3.0 sibling so that a permissively-licensed quality-diversity crate could depend on the same generator without inheriting that licence. The alternative — copying the two methods locally — would have been a **second definition** of the generator every reproducibility claim in the project rests on, which is the one thing that must not happen. One definition, at the bottom of the graph, under a licence anything can depend on.

## Rules

- **No `unwrap()`**, no `expect` on caller data. `below(0)` is a caller bug and fails loudly under `debug_assertions` rather than silently returning `0`.
- **One path per feature.** No fallbacks, no legacy shims, no stub placeholders. Two ways to draw a number is two streams.
- Keep the surface at three methods. Gaussians, shuffles and weighted picks belong to whoever knows what they mean; built on `raw_u64`/`unit`, they stay visible at the call site.
- Consult Bevy documentation often. It can be found at codex_fs/offline_reference_docs/bevy-0.19-book/

## Downstream

Consumers reach this either directly or through a re-export that preserves their old path, so moving it changed no call site. If you add a consumer, depend on this crate rather than on whoever re-exports it — the re-export is compatibility, not architecture.
