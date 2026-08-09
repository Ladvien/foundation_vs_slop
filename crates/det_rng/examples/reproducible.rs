//! **The two claims this crate makes, checked in front of you.**
//!
//! First: a run reproduces from its seed. Two independent streams from one `u64` are compared draw
//! for draw, as raw bits — not within a tolerance, because "close enough" is not the property.
//!
//! Second: `below(n)` is uniform. Note what this does NOT claim — from a 64-bit source, `% n` is
//! biased by about `n / 2^64`, which is unmeasurable, and the counts below will not distinguish the
//! two. `below` uses a rejecting range sampler because that is correct for any source width, not
//! because you could catch the difference here. Overstating that would be the easy thing to write.
//!
//! Run: `cargo run -p det_rng --example reproducible`

use det_rng::{DetRng, seeded};

const SEED: u64 = 0xD06F_00D_1234_5678;
const ROLLS: usize = 100_000;
const SIDES: usize = 6;

fn main() {
    // ---- 1. Same seed, same stream ------------------------------------------------------------
    let mut a = seeded(SEED);
    let mut b = seeded(SEED);
    let mut differing = 0;
    for _ in 0..10_000 {
        // Mix the three draw kinds, so a divergence in any one of them shows up.
        if a.raw_u64() != b.raw_u64() {
            differing += 1;
        }
        if a.below(97) != b.below(97) {
            differing += 1;
        }
        if a.unit().to_bits() != b.unit().to_bits() {
            differing += 1;
        }
    }
    println!();
    println!("  two streams from seed {SEED:#018x}, 30,000 draws each");
    println!("  differing draws: {differing}");
    assert_eq!(differing, 0, "det_rng is not deterministic, which is the one thing it is for");

    // A different seed must NOT agree, or the first check proves nothing.
    let mut c = seeded(SEED ^ 1);
    let mut d = seeded(SEED);
    let same = (0..1_000).filter(|_| c.raw_u64() == d.raw_u64()).count();
    println!("  a neighbouring seed agrees on {same} of 1,000 draws (want: about 0)");

    // ---- 2. `below` is uniform ----------------------------------------------------------------
    let mut rng = seeded(SEED);
    let mut counts = [0usize; SIDES];
    for _ in 0..ROLLS {
        counts[rng.below(SIDES)] += 1;
    }
    let expected = ROLLS as f64 / SIDES as f64;
    let worst = counts
        .iter()
        .map(|&c| ((c as f64 - expected) / expected * 100.0).abs())
        .fold(0.0_f64, f64::max);

    println!();
    println!("  {ROLLS} rolls of a {SIDES}-sided die, expected {expected:.0} per face");
    println!(
        "  {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}   worst deviation {worst:.2}%",
        counts[0], counts[1], counts[2], counts[3], counts[4], counts[5]
    );
    println!();
    println!("  A percent or so of sampling noise at this count is expected and fine. `% 6` would look");
    println!("  the same, because from a 64-bit draw its bias is about 6/2^64 — far below the noise.");
    println!("  `below` is the rejecting sampler regardless, so it stays correct if the source ever");
    println!("  narrows. That is the claim; the stronger one would not be true.");
    println!();
}
