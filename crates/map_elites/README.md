# map_elites

> ⚠️ **Vibe Coded** — written by an AI agent working from a human's direction. It is used in a shipping game and covered by tests, but it has had no line-by-line human audit. Read it before you trust it.

A Quality-Diversity kernel: the MAP-Elites archive, three emitter loops (isotropic, CMA-ME, CMA-MAE), separable CMA-ES, and a POET outer loop. Engine-free, and bit-reproducible from a single `u64`.

> **This repo is a read-only mirror.** It is split out of [`Ladvien/foundation_vs_slop`](https://github.com/Ladvien/foundation_vs_slop) with `git subtree split`, history intact. It depends on a sibling crate by workspace path, so it builds *inside* that workspace, not on its own. Issues and PRs belong upstream.

## The idea

MAP-Elites doesn't optimise toward one best point. It keeps the fittest genome found **per behaviour niche**, so what you get back is a map of what your parameterised system can actually do and how — not a single winner and no memory of the search. On deceptive landscapes that also just finds better optima, because the stepping stones are kept instead of being selected away.

## What it deliberately does not include

An evaluator. Every loop is generic over the genome type `G` and takes mutation and evaluation as closures, because "how good is this genome" is the one question only the caller can answer. That is exactly what lets the crate stay free of any engine — and `tests/engine_free.rs` fails the build if a dependency ever arrives that could put a renderer, a thread pool, or an entropy source in the path.

```rust
let mut result = MapElitesResult::new(resolution);
map_elites_loop(
    &mut rng,
    &mut result,
    generations,
    |rng| random_genome(rng),          // seed
    |g, rng| mutate(g, rng),           // vary
    |g| evaluate(g),                   // -> (fitness, BehaviorDescriptor)
    |gen, archive| report(gen, archive),
);
```

## Reproducibility is the point

Every draw goes through a seeded `ChaCha8Rng`; every archive walk is over a `BTreeMap`. A run replays bit-for-bit from its seed on any platform. The parent project relies on this hard enough to have a test that fans the same search across worker subprocesses and requires the archives to match byte-for-byte.

That is also why the one Box–Muller `gaussian` lives at the crate root rather than beside any one genome: CMA-ES and every flat-vector mutator draw from it in a load-bearing order, and a second implementation would not fail a test — it would quietly make two searches irreproducible against each other.

## References

- Mouret & Clune, *Illuminating search spaces by mapping elites*, arXiv:1504.04909 (2015)
- Pugh, Soros & Clune, *Quality Diversity: A New Frontier for Evolutionary Computation*, Frontiers in Robotics and AI (2016)
- Fontaine et al., *Covariance Matrix Adaptation for the Rapid Illumination of Behavior Space*, GECCO 2020 (CMA-ME)
- Fontaine & Nikolaidis, *Differentiable Quality Diversity*, NeurIPS 2021 (CMA-MAE)
- Wang et al., *Paired Open-Ended Trailblazer (POET)*, arXiv:1901.01753 (2019)

## Examples

```sh
cargo run -p map_elites --example sphere_archive
```

A 4-gene toy problem with a single-peaked fitness and a 2-D behaviour space, printed as an ASCII archive. It then re-runs the whole search from the same seed and compares coverage, QD score and best fitness as raw `f32` bits — not within a tolerance — because reproducibility is the property the crate exists to keep.

## License

MIT OR Apache-2.0, at your option.
