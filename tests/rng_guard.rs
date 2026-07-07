//! RNG guard — freezes the exact output of every deterministic generator the simulation and worldgen
//! draw through. A silent change to any constant (an LCG multiplier, the ChaCha seeding, a hash mix)
//! would otherwise ripple into every downstream golden at once and be hard to localise; this test trips
//! first and points straight at the primitive that moved.
//!
//! Floats are compared by their exact bit pattern (`f32::to_bits`/`f64::to_bits`) so the assertions are
//! byte-exact and platform-stable for these integer-math generators — no epsilon fuzz. See the invariant
//! in `CLAUDE.md`: RNG is seed-driven and entropy-free (`util` LCG/hashes; `rng` seeded ChaCha8), and
//! must stay bit-reproducible.
use foundation_vs_slop::rng::{seeded, DetRng};
use foundation_vs_slop::util::{hash01_u32, hash_f32, next_u32, rand01};

#[test]
fn lcg_next_u32_sequence_is_frozen() {
    // The Numerical Recipes LCG from a seed of 0 — the per-agent RNG backbone (`rand01` is `next_u32`
    // shifted into a float, so freezing the integer stream freezes the float draws too).
    let mut s = 0u32;
    let got: Vec<u32> = (0..8).map(|_| next_u32(&mut s)).collect();
    assert_eq!(
        got,
        [1013904223, 1196435762, 3519870697, 2868466484, 1649599747, 2670642822, 1476291629, 2748932008]
    );
}

#[test]
fn rand01_sequence_is_frozen() {
    let mut s = 12345u32;
    let got: Vec<u32> = (0..8).map(|_| rand01(&mut s).to_bits()).collect();
    assert_eq!(
        got,
        [1017586560, 1015516992, 1057688642, 1059227922, 1063843761, 1038504520, 1056826678, 1057775767]
    );
}

#[test]
fn hash01_u32_is_frozen() {
    // Position-independent per-spawn draw (nest-bred crabs). Keyed by an integer counter, not position.
    let got: Vec<u32> = (0..8u32).map(|i| hash01_u32(i).to_bits()).collect();
    assert_eq!(
        got,
        [1061202249, 1042172080, 1061583157, 1057476719, 1061964082, 1061554926, 1055054164, 1045107240]
    );
}

#[test]
fn hash_f32_is_frozen() {
    // PCG-style stateless draw for per-spawn effect randomness.
    let got: Vec<u32> = (0..8u32).map(|i| hash_f32(i).to_bits()).collect();
    assert_eq!(
        got,
        [1022846460, 1059634922, 1056243097, 1056841197, 1042407458, 1057018071, 1064390834, 1056755236]
    );
}

#[test]
fn chacha_raw_u64_is_frozen() {
    // The generation/solver RNG (wfc, dungeon, placement). ChaCha8 is portable, so this is stable across
    // platforms — the one generator we can rely on for cross-machine bit-identity.
    let mut rng = seeded(0xDEAD_BEEF);
    let got: Vec<u64> = (0..4).map(|_| rng.raw_u64()).collect();
    assert_eq!(
        got,
        [18375021277806890489, 10694743742067356635, 108071404945557828, 4650010346337213241]
    );
}

#[test]
fn chacha_unit_and_below_are_frozen() {
    let mut rng = seeded(0xDEAD_BEEF);
    let units: Vec<u64> = (0..4).map(|_| rng.unit().to_bits()).collect();
    assert_eq!(
        units,
        [4607147397903580561, 4603397262388082742, 4573404484962780288, 4598212645646750854]
    );

    // `below(n)` is the unbiased range sampler the WFC tie-break draws through.
    let mut rng = seeded(7);
    let below: Vec<usize> = (0..8).map(|_| rng.below(10)).collect();
    assert_eq!(below, [1, 1, 1, 1, 2, 7, 0, 7]);
    // Degenerate guard: `below(0)` returns 0 rather than panicking (documented caller-bug behaviour).
    assert_eq!(seeded(1).below(0), 0);
}
