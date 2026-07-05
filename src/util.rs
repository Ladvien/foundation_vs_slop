//! Small shared numeric helpers — the project's hand-rolled RNG/hash surface, kept in one place so the
//! same generators aren't copy-pasted across modules (there is deliberately **no RNG crate**).

/// Advance a linear congruential generator (Numerical Recipes constants) and return the new state.
#[inline]
pub fn next_u32(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *state
}

/// Cheap LCG → float in `[0, 1)`. Full-period from any seed, including a `Local<u32>` default of 0.
/// This is the project's canonical per-agent RNG (drives wander headings, aim scatter, decision
/// tie-breaks); seed lives in a component field or a system `Local<u32>`.
#[inline]
pub fn rand01(state: &mut u32) -> f32 {
    (next_u32(state) >> 8) as f32 / (1u32 << 24) as f32
}

/// Stateless integer avalanche hash of a `u32` seed → `[0, 1)` (Wang-style mix). Use this for per-spawn
/// randomization that must NOT be keyed on a spawn *position*: nest-bred crabs all seat on the one
/// delivery cell, so a position hash would make every sibling identical — an integer spawn counter fed
/// here gives each newborn an independent draw. Distinct `salt`s decorrelate multiple draws per crab.
#[inline]
pub fn hash01_u32(seed: u32) -> f32 {
    let mut h = seed;
    h = (h ^ 61) ^ (h >> 16);
    h = h.wrapping_add(h << 3);
    h ^= h >> 4;
    h = h.wrapping_mul(0x27d4_eb2d);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}
