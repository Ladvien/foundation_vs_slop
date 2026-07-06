//! One deterministic RNG for the whole generation/solver stack: ChaCha8 (Bernstein's ChaCha, 8
//! rounds) — portable, well-tested, and reproducible under a seed regardless of platform or ECS
//! system/thread execution order. `wfc`, `dungeon`, and every placement `Solver` seed and draw
//! through this single type so a given seed always yields the same world (determinism invariant of
//! the placement grammar; see `slop/research/2026-07-05-placement-grammar-implementation.md` §4).

use rand::{Rng, RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Seed a fresh ChaCha8 stream. All generation randomness starts here. Derive per-region sub-streams
/// with `seeded(base ^ crate::placement::splitmix64(region_id))` so regions solve independently.
pub fn seeded(seed: u64) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(seed)
}

/// Ergonomic integer/float draws the generators need, as an extension trait on `ChaCha8Rng` — so the
/// codebase has exactly one RNG type rather than a bespoke PRNG struct alongside `rand`'s.
pub trait DetRng {
    /// A fresh full 64-bit draw (e.g. a sub-seed for a nested generator).
    fn raw_u64(&mut self) -> u64;
    /// Uniform integer in `[0, n)` (unbiased). Returns 0 for the degenerate `n == 0` (a caller bug)
    /// rather than panicking.
    fn below(&mut self, n: usize) -> usize;
    /// Uniform integer in the inclusive range `[lo, hi]`. Returns `lo` when `hi <= lo`.
    fn range_usize(&mut self, lo: usize, hi: usize) -> usize;
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
        // `raw_u64() % n`, which skews toward low indices whenever n does not divide 2^64. `n == 0`
        // has no valid result; every caller guarantees n > 0, but guard so a bug can't panic.
        if n == 0 {
            return 0;
        }
        self.random_range(0..n)
    }
    #[inline]
    fn range_usize(&mut self, lo: usize, hi: usize) -> usize {
        // Inclusive [lo, hi]. Guard the degenerate/inverted range so `hi - lo + 1` can't underflow
        // (usize wraps): [lo, lo] is exactly {lo}, and no caller passes hi < lo.
        if hi <= lo {
            return lo;
        }
        lo + self.below(hi - lo + 1)
    }
    #[inline]
    fn unit(&mut self) -> f64 {
        // Top 53 bits → a double in [0,1), the standard construction (matches the prior xorshift PRNG).
        (self.raw_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}
