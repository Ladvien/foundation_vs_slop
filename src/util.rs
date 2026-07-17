//! Small shared numeric helpers — the project's hand-rolled RNG/hash surface, kept in one place so the
//! same generators aren't copy-pasted across modules (there is deliberately **no RNG crate**). Also the
//! home of [`nearest_planar`], the one ranking every "nearest target" scan shares.

use bevy::math::{IVec2, Vec3, Vec3Swizzles};

/// Row-major index of grid cell `c` in a `width`-wide grid (`c.y * width + c.x`). The single indexing
/// convention every fixed grid in the project shares — `FlowField`, `FogGrid`, `Stig`, `RallyField`, and
/// `Dungeon` all delegate their `index` here so the row-major layout lives in exactly one place. Assumes
/// `c` is in-bounds; gate with [`in_grid`] first when the cell may be off-grid.
#[inline]
pub fn row_major(c: IVec2, width: usize) -> usize {
    c.y as usize * width + c.x as usize
}

/// Is cell `c` inside a `width`×`height` grid (non-negative and below both extents)? The bounds check
/// paired with [`row_major`], shared by the same grids.
#[inline]
pub fn in_grid(c: IVec2, width: usize, height: usize) -> bool {
    c.x >= 0 && c.y >= 0 && (c.x as usize) < width && (c.y as usize) < height
}

/// Sort by a stable **total** key, checked (see [`sort_total_by_key_at`]). Captures the call site so a
/// violation names the exact file and line.
///
/// ```ignore
/// sort_total!(&mut shots, |s| s.member);
/// ```
#[macro_export]
macro_rules! sort_total {
    ($v:expr, $f:expr) => {
        $crate::util::sort_total_by_key_at(concat!(file!(), ":", line!()), $v, $f)
    };
}

/// **Sort by a key that must be a TOTAL order — and prove it, don't assert it in a comment.**
///
/// Use this for every sort whose *order* is load-bearing: a greedy/stateful loop, a `take(n)` budget, a
/// shared RNG draw, a shared counter, a clamped accumulate, a last-writer-wins write. If two elements can
/// produce the same key, `sort_unstable` resolves them by the input order — which in this codebase is ECS
/// query order, which is **not stable across `App` instances** (GLB scene-child instantiation + entity-id
/// reuse permute it). That is the single most common determinism bug here.
///
/// It exists because prose did not work. Three separate sites — `almond_water_effect`, `enemy::smiley_defense`,
/// and the ORCA neighbour sort — carried comments *asserting* a total order, and all three were partial:
/// crabs `clamp_to_patch`-ed against a wall hold BIT-IDENTICAL coordinates, so a position-only key ties
/// (measured: 6 fully-tied pairs at one tick on held-in world `0xA11CE`). Each site documented the exact
/// trap it then fell into. A comment cannot fail; this can.
///
/// Under `debug_assertions` or `test-harness` it **panics naming the call site and the duplicated key** the
/// moment a tie occurs — so the harness suite is also a hunting tool for undiscovered instances. Release
/// builds of the game pay nothing (the check compiles out entirely).
///
/// **When ties are legitimate, do NOT reach for this** — use [`sort_value_canonical`] and say why. The two
/// are different contracts, not a fast path and a safe path.
///
/// Prefer the [`sort_total!`](crate::sort_total) macro, which fills in `site` from `file!()`/`line!()`.
#[inline]
pub fn sort_total_by_key_at<T, K, F>(site: &'static str, v: &mut [T], mut f: F)
where
    K: Ord + std::fmt::Debug,
    F: FnMut(&T) -> K,
{
    v.sort_unstable_by_key(&mut f);
    #[cfg(any(debug_assertions, feature = "test-harness"))]
    {
        for w in v.windows(2) {
            let (a, b) = (f(&w[0]), f(&w[1]));
            assert!(
                a != b,
                "NON-TOTAL SORT KEY at {site}: two elements share the key {a:?}, so `sort_unstable` breaks \
                 the tie by INPUT order — i.e. by ECS query order, which is not stable across `App` \
                 instances. Whatever this ordering decides (a shared draw, a `take(n)` budget, a clamped \
                 accumulate, a lethal pick) is therefore not reproducible. Add a stable tiebreak: \
                 `SquadMember` (units), `CrabSeed` (crabs), `GibKey` (chunks), `CyanideSmell::id` (any \
                 `Biological`), or a monotonic spawn seq. A raw `Entity` will NOT do — recycled ids are the \
                 instability being guarded against. If the tie is genuinely harmless because tied elements \
                 are INTERCHANGEABLE, use `util::sort_value_canonical` instead and say why. \
                 See docs/rl/2026-07-16-search-rollout-nondeterminism.md"
            );
        }
    }
    let _ = site;
}

/// Sort by a key where **ties are legitimate because tied elements are interchangeable** — i.e. permuting
/// them cannot change any observable result.
///
/// The canonical example is [`crate::ai::field::sort_deposits`]: two deposits with the same position and
/// amount contribute the same term to the same non-associative sum, so their order genuinely cannot matter.
/// Same for the crab separation buckets, which hold bare positions.
///
/// This is NOT a relaxed [`sort_total_by_key_at`] — it is a different claim, and the claim is on you. Ask:
/// *if I swapped two tied elements, could anything downstream tell?* If they carry identity, state, or a
/// payload (an `Entity`, a seed, a health value, a mode), the answer is yes and you need
/// [`sort_total_by_key_at`]. `almond_water_effect` looked like this case and was not: its tied drinkers
/// differed in `anosmic`, mode, and carry phase.
#[inline]
pub fn sort_value_canonical<T, K, F>(v: &mut [T], f: F)
where
    K: Ord,
    F: FnMut(&T) -> K,
{
    v.sort_unstable_by_key(f);
}

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
    // DETERMINISM: rank by `(planar distance, position bits)`, not a plain `d < bd` that keeps whichever
    // candidate the iterator yielded first. Entity query order is NOT stable across two same-seed runs
    // (GLB scene-child instantiation + entity-id reuse permute it), so an exact distance tie broken by
    // iteration order flips WHICH target is chosen — and that flip cascades into the physics-free replay
    // hash (`deterministic_core_is_bit_identical`). The position fallback (a stable geometric key) makes
    // the pick depend only on geometry. `d.to_bits()` is monotonic in `d` for finite non-negative d.
    let o = origin.xz();
    let mut best: Option<(T, Vec3, f32)> = None;
    for (payload, pos) in candidates {
        let d = (pos.xz() - o).length();
        // `.as_ref()` is load-bearing: `T` may be non-`Copy`, so we must not move `best` out to read it.
        let take = match best.as_ref() {
            None => true,
            Some((_, bpos, bd)) => {
                (d.to_bits(), pos.x.to_bits(), pos.y.to_bits(), pos.z.to_bits())
                    < (bd.to_bits(), bpos.x.to_bits(), bpos.y.to_bits(), bpos.z.to_bits())
            }
        };
        if take {
            best = Some((payload, pos, d));
        }
    }
    best
}

/// GLSL-style `smoothstep` (Hermite ramp), clamped to `[0, 1]`. When `edge0 > edge1` the ramp is
/// reversed, so `smoothstep(FAR, NEAR, d)` rises from 0 at `d = FAR` to 1 at `d = NEAR` — a shared
/// proximity curve (the smiley's grin in `enemy`, the audio threat scalar in `audio`).
#[inline]
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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

/// Pure gaze/facing test: is `target` within the `look_cos` cone of `forward` as seen from `pos`? Planar
/// (XZ); the caller adds range + line-of-sight. Shared perception primitive: the smiley watcher uses it
/// to know when a unit is *looking directly at it* (`enemy.rs`), and the crab swarm uses the negation to
/// pounce only from a prey's blind side (`crab.rs`) — one cone test, so "what counts as facing" is a
/// single edit (Rabin, "Vision Zones", GameAIPro2 Ch.4: perception keys off the agent's actual view
/// direction). A target on top of `pos` is treated as faced.
pub(crate) fn unit_is_facing(pos: Vec3, forward: Vec3, target: Vec3, look_cos: f32) -> bool {
    let bearing = (target - pos).with_y(0.0).normalize_or_zero();
    if bearing == Vec3::ZERO {
        return true; // on top of it — treat as faced
    }
    let fwd = forward.with_y(0.0).normalize_or(Vec3::NEG_Z);
    bearing.dot(fwd) >= look_cos
}
