#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use rand::{Rng, RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Seed a fresh ChaCha8 stream.
///
/// Derive independent sub-streams by mixing the seed rather than by cloning the generator —
/// `seeded(base ^ splitmix64(region_id))` — so two regions can be solved in either order, or in
/// parallel, and still produce the same result.
pub fn seeded(seed: u64) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(seed)
}

/// Ergonomic integer/float draws the generators need, as an extension trait on `ChaCha8Rng` — so the
/// codebase has exactly one RNG type rather than a bespoke PRNG struct alongside `rand`'s.
pub trait DetRng {
    /// A fresh full 64-bit draw (e.g. a sub-seed for a nested generator).
    fn raw_u64(&mut self) -> u64;
    /// Uniform integer in `[0, n)` (unbiased). `n == 0` is a caller bug (no valid result exists) — it
    /// fails loudly under `debug_assertions`/`test-harness`, matching the `sort_total!` discipline, and
    /// is elided in release builds (which pay nothing for the check).
    fn below(&mut self, n: usize) -> usize;
    /// Uniform float in `[0, 1)`.
    fn unit(&mut self) -> f64;
}

impl DetRng for ChaCha8Rng {
    #[inline]
    fn raw_u64(&mut self) -> u64 {
        // Disambiguate from this trait's own methods: call `rand::Rng`'s inherent draw.
        Rng::next_u64(self)
    }
    #[inline]
    fn below(&mut self, n: usize) -> usize {
        // Unbiased uniform draw in [0, n) via `rand`'s range sampler — not the modulo reduction
        // `raw_u64() % n`, which skews toward low indices whenever n does not divide 2^64.
        debug_assert!(n > 0, "DetRng::below(0): degenerate range — caller bug, not a valid zero-draw");
        self.random_range(0..n.max(1))
    }
    #[inline]
    fn unit(&mut self) -> f64 {
        // Top 53 bits → a double in [0,1), the standard construction (matches the prior xorshift PRNG).
        (self.raw_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The stream is frozen, and that is the crate's entire contract.**
    ///
    /// These are not "some values it happened to produce" — they are the API. Every consumer's
    /// reproducibility claim, and every replay golden downstream, sits on exactly these bits. If this
    /// test goes red, the change under it is a breaking change no matter how innocuous it looked.
    ///
    /// The upstream project pins the identical values in its own `tests/rng_guard.rs`; the copy here
    /// exists so the guard travels with the crate rather than living only in a repository an adopter
    /// cannot see.
    #[test]
    fn the_stream_is_frozen() {
        let mut rng = seeded(0xDEAD_BEEF);
        let raw: Vec<u64> = (0..4).map(|_| rng.raw_u64()).collect();
        assert_eq!(
            raw,
            [18375021277806890489, 10694743742067356635, 108071404945557828, 4650010346337213241],
            "raw_u64 moved"
        );

        let mut rng = seeded(0xDEAD_BEEF);
        let units: Vec<u64> = (0..4).map(|_| rng.unit().to_bits()).collect();
        assert_eq!(
            units,
            [4607147397903580561, 4603397262388082742, 4573404484962780288, 4598212645646750854],
            "unit moved (compared as bits, deliberately — a tolerance would hide the thing being pinned)"
        );

        let mut rng = seeded(7);
        let below: Vec<usize> = (0..8).map(|_| rng.below(10)).collect();
        assert_eq!(below, [1, 1, 1, 1, 2, 7, 0, 7], "below moved");
    }

    /// Two generators from one seed agree; neighbouring seeds do not. The second half matters — a
    /// generator that ignored its seed would pass the first half perfectly.
    #[test]
    fn same_seed_agrees_and_a_different_one_does_not() {
        let (mut a, mut b) = (seeded(42), seeded(42));
        for _ in 0..256 {
            assert_eq!(a.raw_u64(), b.raw_u64());
        }
        let (mut a, mut c) = (seeded(42), seeded(43));
        let agreements = (0..256).filter(|_| a.raw_u64() == c.raw_u64()).count();
        assert!(agreements < 4, "seeds 42 and 43 agreed on {agreements} of 256 draws");
    }

    /// `unit()` must stay inside `[0, 1)`. The top-53-bit construction makes 1.0 unreachable, which
    /// callers scaling by a range depend on.
    #[test]
    fn unit_stays_in_range() {
        let mut rng = seeded(0xC0FFEE);
        for _ in 0..100_000 {
            let u = rng.unit();
            assert!((0.0..1.0).contains(&u), "unit() returned {u}");
        }
    }

    /// `below(n)` must never return `n`. An off-by-one here indexes out of bounds in every consumer.
    #[test]
    fn below_never_reaches_its_bound() {
        let mut rng = seeded(0xBEEF);
        for n in [1usize, 2, 3, 7, 10, 64, 1000] {
            for _ in 0..2_000 {
                assert!(rng.below(n) < n, "below({n}) returned its own bound");
            }
        }
    }
}
