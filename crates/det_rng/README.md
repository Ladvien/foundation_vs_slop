# det_rng

> ⚠️ **Vibe Coded** — written by an AI agent working from a human's direction. It is used in a shipping game and covered by tests, but it has had no line-by-line human audit. Read it before you trust it.

One deterministic RNG for a whole simulation: a seeded ChaCha8 stream, plus the unbiased integer and float draws generators actually need. A run reproduces from a single `u64`, on any platform, at any thread count.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. Issues and PRs belong upstream.

## Why this is a crate and not four lines in yours

Because "four lines in yours" is how you end up with two of them.

The moment a second module needs a random integer, the temptation is to write a small PRNG next to it — and now the reproducibility claim covering your whole simulation depends on two generators staying identical forever. That is not a hypothetical: this crate exists because a quality-diversity search and a world generator in the same project were about to do exactly that, and the search's entire value proposition is that an archive replays bit-for-bit from its seed.

So: one type, one place, and any consumer that needs a draw takes this.

```rust
use det_rng::{DetRng, seeded};

let mut rng = seeded(0xD06F_00D);
let roll = rng.below(6);        // unbiased, in [0, 6)
let t = rng.unit();             // [0, 1)
let sub = rng.raw_u64();        // a seed for a nested generator

// Same seed, same everything — the property the crate exists for.
let mut again = seeded(0xD06F_00D);
assert_eq!(again.below(6), roll);
```

## `below` is not `% n`

`raw_u64() % n` is biased toward low values whenever `n` does not divide the source range — and to be honest about the size of that: from a 64-bit draw the bias is on the order of `n / 2^64`, which you will never measure. At a hundred thousand rolls of a six-sided die it is far below the sampling noise, so the example does not pretend to show it.

The reason to use the range sampler anyway is that it is correct *by construction, for any source*. The modulo version is only fine because of a property of the generator behind it, so the moment someone draws from something narrower — a 32-bit value, a byte, a cached partial draw — the reasoning has to be redone, and it will not be. `below` removes the question.

`below(0)` is a caller bug — there is no valid result — so it fails loudly under `debug_assertions` rather than quietly returning `0`.

## What it deliberately does not do

**No entropy.** Nothing here reads the clock, the OS RNG, or a thread id. A generator that can seed itself is a generator that can produce a run you cannot reproduce, and the point is the opposite.

**No global.** There is no ambient `rng()` to reach for. Draws come from a generator you own and pass, because a shared mutable stream makes the order of draws depend on execution order, which is the exact thing determinism needs to not depend on.

**No distributions beyond these three.** Gaussians, shuffles and weighted picks are all fine, they just belong to whoever knows what they mean — build them on `raw_u64`/`unit` and keep the shape of the stream visible at the call site.

## Determinism, and what it is worth

ChaCha8 is a specified algorithm, so `seeded(s)` produces the same bytes on any platform and any thread count. What that buys you is not "randomness quality" — it is that a bug reproduces, a replay replays, and a search archive can be compared for equality rather than for approximate agreement.

The upstream project freezes the first draws of every generator in a test and treats a change to those bits as a breaking change, which is the discipline this is meant for. If you adopt it, do the same: the value is in the bits not moving.

## Examples

```sh
cargo run -p det_rng --example reproducible
```

Prints two runs from one seed and diffs them draw for draw, checks that a neighbouring seed does *not* agree, and rolls a hundred thousand dice to show `below` is uniform. Terminal only — no window, no GPU.

## License

MIT OR Apache-2.0, at your option.
