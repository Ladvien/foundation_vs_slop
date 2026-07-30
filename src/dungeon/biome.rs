//! Surface **biome**: which art treatment a floor/wall tile is rendered with.
//!
//! Two treatments today — the Backrooms motel interior the game shipped with, and bare Foundation
//! concrete — and they exist so a run reads as *moving between zones* rather than as one continuous
//! corridor. The concrete side is also the desaturated counterweight to the Backrooms yellow: the
//! wallpaper and carpet are authored warm and saturated on purpose (see `docs/ui.md` §1.3 on why that
//! is a tension rather than a bug), and a zone with none of it is what keeps the palette honest.
//!
//! # Biome is a pure function of position and seed, and that is the whole design
//!
//! It is *not* per-region state assigned by a loop over `Dungeon::regions`. Three things fall out of
//! that choice, each of which would otherwise have been work:
//!
//! 1. **Determinism is structural.** `CLAUDE.md`'s "ECS query order decides nothing" rule exists
//!    because anything a query order can decide — an RNG draw, a budget, a last-writer-wins write —
//!    needs a stable total key. A pure function of `(seed, cell)` is decided by neither query order nor
//!    iteration order, so there is no key to get wrong and no `sort_total!` to place.
//! 2. **Rooms and corridors agree for free.** A corridor is not a `Region`, so per-region assignment
//!    would have needed a separate rule for the spaces between rooms — and any such rule can disagree
//!    with its neighbours at the doorway. Sampling the same field at every cell cannot.
//! 3. **It draws nothing from the carve RNG.** `Dungeon::generate` seeds one stream and
//!    `expand_to_fine` consumes it in site order; taking even one extra draw from it would shift every
//!    subsequent value and break the pinned layout goldens (`tests/wfc_pin.rs`). This reads a separate
//!    hash entirely, so the carve is byte-identical to before biomes existed.
//!
//! The field is value noise on a lattice of `biome_scale` tiles, smoothstep-interpolated and thresholded
//! against `biome_mix`. Value noise rather than a half-plane split because a straight dividing line
//! across the map reads as an authored border; a noise field gives interlocking zones with an irregular
//! frontier, which is what "the concrete gives way to carpet somewhere around here" should look like.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Which art treatment a tile renders with. `Copy` and one byte — it is stored per fine cell.
///
/// Serialized into the level genome, so the variant order is part of that contract: append new
/// treatments at the end rather than inserting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Biome {
    /// The shipped look: patterned wallpaper over carpet. Warm, saturated, domestic-gone-wrong.
    Backrooms,
    /// Bare facility concrete. Desaturated, institutional — the Foundation's own construction.
    Concrete,
}

/// Deterministic 2D integer hash → `[0, 1)`. Two rounds of the SplitMix64 finalizer, which is the
/// mixer the rest of the codebase already trusts (`laser::target_id` uses the same constants).
///
/// Negative lattice coordinates exist (the fine grid is addressed with `IVec2`, and `value_noise`
/// floors, so `x - 1` goes below zero at the origin). `as u64` on a negative `i64` is a **sign-extending
/// reinterpretation** — `-1` becomes `0xFFFF_FFFF_FFFF_FFFF`, not a small number. That is fine and
/// deliberate: the mapping `i64 → u64` is a bijection, so distinct cells stay distinct, which is the
/// only property a hash needs here. It is spelled out because the obvious misreading — "the cast wraps
/// the value into a small positive range" — would suggest negative and positive cells collide, and they
/// do not.
fn hash01(seed: u64, x: i64, y: i64) -> f32 {
    let mut z = seed
        ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // Top 24 bits → [0,1). f32 has 24 bits of mantissa, so this is exactly representable and every
    // value is distinct — taking the low bits instead would quantise visibly at this lattice size.
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// Smoothstep-interpolated value noise in `[0, 1]` at fine-grid position `cell`.
///
/// `scale` is the lattice period in tiles: how many metres of dungeon a zone spans before the field is
/// free to swing to the other biome.
fn value_noise(seed: u64, cell: IVec2, scale: f32) -> f32 {
    // A zero or negative period would divide the world to a single lattice point (or NaN); clamp rather
    // than trusting the config, which `validate_config` also checks at load.
    let scale = scale.max(1.0);
    let fx = cell.x as f32 / scale;
    let fy = cell.y as f32 / scale;
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let (ix, iy) = (x0 as i64, y0 as i64);

    let (c00, c10) = (hash01(seed, ix, iy), hash01(seed, ix + 1, iy));
    let (c01, c11) = (hash01(seed, ix, iy + 1), hash01(seed, ix + 1, iy + 1));

    // Smoothstep the interpolants so zone frontiers ease rather than creasing along lattice lines.
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    let top = c00 + (c10 - c00) * sx;
    let bottom = c01 + (c11 - c01) * sx;
    top + (bottom - top) * sy
}

/// The biome at a fine-grid cell.
///
/// `mix` is the target fraction of the map that is [`Biome::Concrete`], in `[0, 1]`. It is a threshold
/// on a roughly-uniform field, so it is approximately — not exactly — the realized fraction on any one
/// seed; `biome_share` in the tests measures the real spread. `0.0` and `1.0` are exact, and are the
/// supported way to get a single-biome level without a second code path.
pub fn biome_at(seed: u64, cell: IVec2, mix: f32, scale: f32) -> Biome {
    // Exact endpoints: `>=` on a field that can equal 0.0 would make `mix == 0.0` occasionally concrete.
    if mix <= 0.0 {
        return Biome::Backrooms;
    }
    if mix >= 1.0 {
        return Biome::Concrete;
    }
    if value_noise(seed, cell, scale) < mix {
        Biome::Concrete
    } else {
        Biome::Backrooms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Measured concrete fraction over a patch, for the distribution assertions below.
    fn biome_share(seed: u64, mix: f32, scale: f32, span: i32) -> f32 {
        let mut concrete = 0usize;
        let mut total = 0usize;
        for y in 0..span {
            for x in 0..span {
                if biome_at(seed, IVec2::new(x, y), mix, scale) == Biome::Concrete {
                    concrete += 1;
                }
                total += 1;
            }
        }
        concrete as f32 / total as f32
    }

    /// `Biome as usize` indexes the material arrays in `dungeon::render` and `FloorMaterials::pick`,
    /// and the footstep-set array in `audio`. Nothing in the type system ties those together — reorder
    /// this enum and every wall, floor and footstep silently swaps to the other biome's asset, with no
    /// compile error and no runtime complaint. This is the pin.
    #[test]
    fn the_discriminants_match_the_material_and_audio_array_order() {
        assert_eq!(Biome::Backrooms as usize, 0, "index 0 is the Backrooms asset set");
        assert_eq!(Biome::Concrete as usize, 1, "index 1 is the Concrete asset set");
    }

    #[test]
    fn the_endpoints_are_exact_single_biome_levels() {
        // The supported way to disable the second biome is the knob, not a code path — so it has to be
        // exactly single-biome, including at cells where the noise lands on 0.0.
        for x in -50..50 {
            for y in -50..50 {
                let c = IVec2::new(x, y);
                assert_eq!(biome_at(7, c, 0.0, 12.0), Biome::Backrooms, "mix 0 must be pure at {c}");
                assert_eq!(biome_at(7, c, 1.0, 12.0), Biome::Concrete, "mix 1 must be pure at {c}");
            }
        }
    }

    #[test]
    fn the_mix_knob_tracks_the_realized_share() {
        // A threshold on value noise approximates the requested fraction rather than hitting it, so this
        // pins the relationship is monotonic and roughly right — not that it is exact.
        let (lo, mid, hi) = (
            biome_share(0xABC, 0.2, 10.0, 240),
            biome_share(0xABC, 0.5, 10.0, 240),
            biome_share(0xABC, 0.8, 10.0, 240),
        );
        assert!(lo < mid && mid < hi, "share must rise with mix (got {lo:.3}, {mid:.3}, {hi:.3})");
        assert!((mid - 0.5).abs() < 0.12, "mix 0.5 should land near half the map, got {mid:.3}");
        assert!(lo < 0.4, "mix 0.2 should be clearly minority concrete, got {lo:.3}");
        assert!(hi > 0.6, "mix 0.8 should be clearly majority concrete, got {hi:.3}");
    }

    #[test]
    fn zones_are_contiguous_rather_than_per_tile_noise() {
        // The point of the whole design: a run should read as moving between zones. If neighbouring
        // tiles disagreed at anything like coin-flip rate the result would be confetti, which is what a
        // per-region random assignment or an unsmoothed hash would have produced.
        let (mut disagree, mut total) = (0usize, 0usize);
        for y in 0..200 {
            for x in 0..200 {
                let here = biome_at(99, IVec2::new(x, y), 0.5, 12.0);
                let right = biome_at(99, IVec2::new(x + 1, y), 0.5, 12.0);
                if here != right {
                    disagree += 1;
                }
                total += 1;
            }
        }
        let rate = disagree as f32 / total as f32;
        assert!(rate < 0.06, "adjacent tiles disagree {rate:.3} of the time — that is confetti, not zones");
        assert!(rate > 0.0, "no frontier at all means the field collapsed to one biome");
    }

    #[test]
    fn the_field_is_a_pure_function_of_seed_and_cell() {
        // Determinism here is structural (module docs), and this is what "structural" has to mean in
        // practice: same inputs, same answer, no hidden state between calls, and different seeds
        // actually producing different worlds.
        let cells: Vec<IVec2> = (0..40).map(|i| IVec2::new(i * 3 - 60, i * 7 - 20)).collect();
        let first: Vec<Biome> = cells.iter().map(|c| biome_at(5, *c, 0.5, 12.0)).collect();
        let again: Vec<Biome> = cells.iter().rev().map(|c| biome_at(5, *c, 0.5, 12.0)).collect();
        assert_eq!(first, again.into_iter().rev().collect::<Vec<_>>(), "evaluation order changed a result");

        let other: Vec<Biome> = cells.iter().map(|c| biome_at(6, *c, 0.5, 12.0)).collect();
        assert_ne!(first, other, "two seeds produced an identical biome map");
    }
}
