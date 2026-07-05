//! Small shared numeric helpers — the project's hand-rolled RNG/hash surface, kept in one place so the
//! same generators aren't copy-pasted across modules (there is deliberately **no RNG crate**).

use bevy::math::Vec3;

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

/// Stateless deterministic hash of a world position → `[0, 1)` (classic sine hash). Gives each entity
/// a stable, varied per-spawn value (e.g. a crab's climb/angle bias) without carrying RNG state.
#[inline]
pub fn hash01(v: Vec3) -> f32 {
    let n = (v.x * 12.9898 + v.z * 78.233 + v.y * 37.719).sin() * 43758.547;
    n - n.floor()
}
