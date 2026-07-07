//! Small shared numeric helpers — the project's hand-rolled RNG/hash surface, kept in one place so the
//! same generators aren't copy-pasted across modules (there is deliberately **no RNG crate**). Also the
//! home of [`nearest_planar`], the one ranking every "nearest target" scan shares.

use bevy::math::{Vec3, Vec3Swizzles};

/// Nearest candidate to `origin` by planar (XZ) distance. Generic over a per-candidate payload (entity,
/// forward vector, or `()`), so every "nearest target" scan across the AI shares ONE ranking + tie-break
/// instead of hand-rolling the loop five times (a targeting-policy change — LOS, ignore near-dead prey,
/// tie-break by health — is then a single edit here). Takes an `IntoIterator<Item = (T, Vec3)>` rather
/// than a `&Query`, so it serves both live queries and a precomputed `Vec` (and needs no ECS imports);
/// each caller `.map`s its candidates to `(payload, position)`. Strict `<` keeps the FIRST candidate on a
/// tie, matching every scan it replaced.
pub fn nearest_planar<T>(
    origin: Vec3,
    candidates: impl IntoIterator<Item = (T, Vec3)>,
) -> Option<(T, Vec3, f32)> {
    let o = origin.xz();
    let mut best: Option<(T, Vec3, f32)> = None;
    for (payload, pos) in candidates {
        let d = (pos.xz() - o).length();
        // `.as_ref()` is load-bearing: `T` may be non-`Copy`, so we must not move `best` out to read `bd`.
        if best.as_ref().is_none_or(|(_, _, bd)| d < *bd) {
            best = Some((payload, pos, d));
        }
    }
    best
}

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

/// Deterministic hash → f32 in `[0, 1)` from a `u32` (PCG-style output mix). The canonical stateless
/// draw for per-spawn effect randomness that must not depend on a RNG *resource* — matching the
/// shaders' texture-free noise philosophy, reproducible per seed. Callers mix their own key into `x`
/// (a spawn counter, a fragment index, a salt). Distinct from [`hash01_u32`], which is a different
/// avalanche kept for position-independent nest-spawn draws.
#[inline]
pub fn hash_f32(x: u32) -> f32 {
    let mut h = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    h = ((h >> ((h >> 28).wrapping_add(4))) ^ h).wrapping_mul(277_803_737);
    h = (h >> 22) ^ h;
    (h as f32) / (u32::MAX as f32)
}
