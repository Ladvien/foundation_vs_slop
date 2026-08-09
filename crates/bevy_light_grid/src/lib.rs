//! **An illuminance grid creatures can read.**
//!
//! Not a renderer. This is a CPU scalar field over cells that answers "how bright is it here, and which
//! way is brighter" — the thing a light-avoiding creature or a light-seeking plant actually needs, and
//! which a GPU lighting pass cannot tell you because its results live in a framebuffer.
//!
//! The field is two layers:
//!
//! * a **static base**, baked from a fixture list. Expensive (`O(fixtures × range²)`) but event-driven,
//!   so it runs only when a fixture changes. Walls cast shadow — a cell with no line of sight to a
//!   fixture gets nothing, which is cheap leak-suppression in the spirit of DDGI's visibility moments
//!   (Majercik et al., JCGT 2019).
//! * the **composed result**, `base + Σ moving cones`, recomputed every tick. Only the moving lights are
//!   re-added on top of the cached base, so a walking flashlight's beam sweeps live for a fraction of
//!   the bake's cost. Björk & Michelsen (FDG 2014) treat exactly that cone as a vision/deterrent field.
//!
//! Reading the gradient and descending it is **photophobic taxis** — consistent with Nakagaki et al.'s
//! Physarum photoavoidance (PRL 2007), though not their minimum-risk routing, which is a global path
//! integral rather than local descent. Climbing it instead gives the phototropic case: fungal fruiting is
//! light-gated (Zhang et al., PLoS ONE 10:e0123025, 2015).
//!
//! # Cell space, and the caller's occlusion
//!
//! Every entry point takes an `IVec2` **cell**; the crate has no idea how big a cell is. Occlusion is
//! yours too — [`LightGrid::bake`] and [`LightGrid::compose`] take a `los` closure, because "can this
//! cell see that one" is a question about your map, not about light.
//!
//! # What this crate does not do
//!
//! It registers no systems and owns no schedule. It provides the grid, the two passes, and three marker
//! components; when they run, and in what order relative to everything reading the field, is the
//! caller's business.
//!
//! # Determinism
//!
//! Both passes accumulate with a non-associative `+=`, so **`fixtures` and `cones` must arrive in a
//! stable order** — sort them by source cell before calling. The crate cannot do it for you: it sees a
//! slice, not the query that produced it. [`LightGrid::cells`] exposes the full grid so a caller can fold
//! it into a hash, including the rock-cells-stay-zero invariant.

use bevy_ecs::prelude::Component;
use bevy_math::{IVec2, Vec2, Vec3};

/// Row-major index of a cell in a `width`-wide grid: `y * width + x`.
///
/// Public because it is a layout contract: [`LightGrid::apply_mold_dim`] takes a companion grid indexed
/// this way, and a caller whose own grids disagree would silently misalign them.
#[inline]
pub fn row_major(c: IVec2, width: usize) -> usize {
    c.y as usize * width + c.x as usize
}

/// Is `c` inside a `width`×`height` grid?
#[inline]
pub fn in_grid(c: IVec2, width: usize, height: usize) -> bool {
    c.x >= 0 && c.y >= 0 && (c.x as usize) < width && (c.y as usize) < height
}

/// Flees light: steer **down** the gradient, toward the dark.
#[derive(Component)]
pub struct Photophobic;

/// Drawn to light: steer **up** the gradient. The same push with the opposite sign.
#[derive(Component)]
pub struct Photophilic;

/// Grows or orients **toward** light — a *tropism*, not steering. For things that lean and swell rather
/// than walk.
#[derive(Component)]
pub struct Phototropic;

/// One moving directional light contributed to a [`LightGrid`] each tick.
///
/// `source` is its cell, `forward` the beam direction in the grid's plane (unit length), and the rest
/// its reach, brightness and shape. Sort a batch by `source` before composing — see the determinism note
/// on the crate root.
pub struct FlashlightCone {
    pub source: IVec2,
    pub forward: Vec2,
    pub intensity: f32,
    pub range: f32,
    /// `cos` of the half-angle: the beam rim. Larger = narrower.
    pub cone_cos: f32,
    /// How far inside the rim the wedge ramps from 0 to 1. Soft, so the gradient creatures read stays
    /// smooth instead of falling off a cliff at the beam edge.
    pub edge_softness: f32,
}

/// A CPU illuminance field over a fixed cell grid.
pub struct LightGrid {
    width: usize,
    height: usize,
    /// **Static baseline** — the cached fixture bake, row-major.
    base: Vec<f32>,
    /// **Final** illuminance everything reads: `base` plus the per-tick dynamic cones.
    cells: Vec<f32>,
    /// Peak cell illuminance of `cells` after the last compose, so callers can normalise to `0..1`.
    peak: f32,
    /// Row-major indices of the floor cells. Both writers gate on the floor mask, so rock cells hold
    /// exactly 0.0 forever — the per-tick peak fold and [`Self::apply_mold_dim`] scan only this list,
    /// which is bit-identical to a full-grid scan rather than an approximation. (Chilimbi, Hill & Larus,
    /// "Cache-Conscious Structure Layout", PLDI 1999 — restrict hot scans to data that can change.)
    floor_idx: Vec<usize>,
    /// The same set as a mask, for the per-cell test inside the two passes.
    floor_mask: Vec<bool>,
}

impl LightGrid {
    /// An empty field over a `width`×`height` grid whose floor cells are `floor_cells`.
    ///
    /// Both floor views are derived from the one iterator, deliberately: a mask that disagreed with the
    /// index list would light cells the peak fold never visits.
    pub fn new(width: usize, height: usize, floor_cells: impl Iterator<Item = IVec2>) -> Self {
        let n = width * height;
        let mut floor_idx = Vec::new();
        let mut floor_mask = vec![false; n];
        for c in floor_cells {
            if !in_grid(c, width, height) {
                continue;
            }
            let i = row_major(c, width);
            floor_mask[i] = true;
            floor_idx.push(i);
        }
        Self { width, height, base: vec![0.0; n], cells: vec![0.0; n], peak: 0.0, floor_idx, floor_mask }
    }

    /// Point read at a cell (query). Off-grid reads as 0.
    #[inline]
    pub fn sample_cell(&self, c: IVec2) -> f32 {
        if in_grid(c, self.width, self.height) {
            self.cells[row_major(c, self.width)]
        } else {
            0.0
        }
    }

    /// Direction of *increasing* illuminance (central differences), magnitude ≈ the local slope. Steer
    /// along `-gradient` to flee light, `+gradient` to seek it.
    #[inline]
    pub fn gradient_cell(&self, c: IVec2) -> Vec2 {
        let at = |dx: i32, dy: i32| -> f32 {
            let n = c + IVec2::new(dx, dy);
            if in_grid(n, self.width, self.height) {
                self.cells[row_major(n, self.width)]
            } else {
                0.0
            }
        };
        Vec2::new(at(1, 0) - at(-1, 0), at(0, 1) - at(0, -1))
    }

    /// Peak illuminance from the last compose (0 before the first).
    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// Recompute the static base from a fixture list — the bake.
    ///
    /// Each fixture is `(cell, intensity, range)` with `range` in cells; a floor cell within `range` that
    /// `los` says the fixture can see gains `intensity * (1 - dist/range)`. Walls cast shadow.
    ///
    /// `fixtures` must arrive in a stable order — the per-cell sum is a non-associative float add.
    pub fn bake(&mut self, fixtures: &[(IVec2, f32, f32)], los: impl Fn(IVec2, IVec2) -> bool) {
        for v in self.base.iter_mut() {
            *v = 0.0;
        }
        for &(fcell, intensity, range) in fixtures {
            if range <= 0.0 {
                continue;
            }
            let r = range.ceil() as i32;
            for dy in -r..=r {
                for dx in -r..=r {
                    let cell = fcell + IVec2::new(dx, dy);
                    if !in_grid(cell, self.width, self.height)
                        || !self.floor_mask[row_major(cell, self.width)]
                    {
                        continue;
                    }
                    let dist = ((dx * dx + dy * dy) as f32).sqrt();
                    if dist > range {
                        continue;
                    }
                    // Walls block light: only cells the fixture can "see" are lit.
                    if !los(fcell, cell) {
                        continue;
                    }
                    self.base[row_major(cell, self.width)] += intensity * (1.0 - dist / range);
                }
            }
        }
    }

    /// Recompose `cells = base + Σ cones`, then recompute [`Self::peak`].
    ///
    /// Runs every tick — the base is cached, so only the moving cones are re-added. Each cone is
    /// wall-occluded and radially attenuated like a fixture, and additionally gated by a soft-edged
    /// wedge around `forward`.
    ///
    /// `cones` must arrive in a stable order, for the same reason as [`Self::bake`].
    pub fn compose(&mut self, cones: &[FlashlightCone], los: impl Fn(IVec2, IVec2) -> bool) {
        self.cells.copy_from_slice(&self.base);
        for cone in cones {
            if cone.range <= 0.0 || cone.intensity <= 0.0 {
                continue;
            }
            let r = cone.range.ceil() as i32;
            for dy in -r..=r {
                for dx in -r..=r {
                    let cell = cone.source + IVec2::new(dx, dy);
                    if !in_grid(cell, self.width, self.height)
                        || !self.floor_mask[row_major(cell, self.width)]
                    {
                        continue;
                    }
                    let dist = ((dx * dx + dy * dy) as f32).sqrt();
                    if dist > cone.range {
                        continue;
                    }
                    if !los(cone.source, cell) {
                        continue;
                    }
                    // 1 at the source cell (its own footprint), else `cos θ` between the cell direction
                    // and `forward`, ramped from 0 at the rim to 1 by `edge_softness` further in.
                    let cone_factor = if dx == 0 && dy == 0 {
                        1.0
                    } else {
                        let dir = Vec2::new(dx as f32, dy as f32) / dist;
                        let c = dir.dot(cone.forward);
                        ((c - cone.cone_cos) / cone.edge_softness.max(1.0e-4)).clamp(0.0, 1.0)
                    };
                    if cone_factor <= 0.0 {
                        continue;
                    }
                    self.cells[row_major(cell, self.width)] +=
                        cone.intensity * (1.0 - dist / cone.range) * cone_factor;
                }
            }
        }
        // Peak over floor cells only: rock cells are invariantly 0.0 and every cell is >= 0, so a max
        // seeded at 0.0 cannot see them — bit-identical to the full-grid fold it replaces.
        self.peak = self.floor_idx.iter().map(|&i| self.cells[i]).fold(0.0f32, f32::max);
    }

    /// Attenuate the composed illuminance by a companion coverage grid: a cell whose `coverage` tends to
    /// 1 darkens toward `1 - dim`. Recomputes [`Self::peak`].
    ///
    /// Call it AFTER [`Self::compose`], which recopies the un-dimmed base first, so the dimming never
    /// accumulates across ticks. `coverage` must share this grid's row-major layout; a short slice reads
    /// as 0 rather than panicking.
    pub fn apply_mold_dim(&mut self, coverage: &[f32], dim: f32) {
        if dim <= 0.0 {
            return;
        }
        // Floor cells only: dimming a rock cell is `0.0 * x == 0.0` and the peak fold ignores zeros.
        let mut peak = 0.0f32;
        let Self { cells, floor_idx, .. } = self;
        for &i in floor_idx.iter() {
            let b = coverage.get(i).copied().unwrap_or(0.0);
            let cell = &mut cells[i];
            *cell *= (1.0 - dim * b).clamp(0.0, 1.0);
            peak = peak.max(*cell);
        }
        self.peak = peak;
    }

    /// The full composed grid, row-major — the deterministic accessor a caller folds into a hash.
    pub fn cells(&self) -> &[f32] {
        &self.cells
    }

    /// Grid dimensions, `(width, height)`.
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}

/// The steering push a light-response creature feels at `cell`: `signed_gain · ∇illuminance`, in the
/// grid plane (returned as world XZ).
///
/// A photophobic creature passes a negative gain (descends toward the dark), a photophilic one a
/// positive gain. Zero where the field is flat — deep dark, or the middle of a uniform pool — so a
/// creature far from any gradient is simply unbiased rather than pushed somewhere arbitrary.
pub fn light_push_at(field: &LightGrid, cell: IVec2, signed_gain: f32) -> Vec3 {
    if signed_gain == 0.0 {
        return Vec3::ZERO;
    }
    let g = field.gradient_cell(cell);
    Vec3::new(g.x, 0.0, g.y) * signed_gain
}

/// The next scale for a [`Phototropic`] body easing toward its light-scaled target size
/// `base·(1 + bonus·light01)`, approached from `current` by at most `max_step` this tick.
///
/// Rate-limited so the change stays sub-perceptual. `light01` is illuminance normalised to the field
/// peak: 0 (dark) means the target is just `base`; 1 (brightest) means the full bonus.
pub fn phototropic_scale(base: f32, current: f32, light01: f32, bonus: f32, max_step: f32) -> f32 {
    let target = base * (1.0 + bonus * light01.clamp(0.0, 1.0));
    (current + (target - current).clamp(-max_step, max_step)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_floor(w: usize, h: usize) -> impl Iterator<Item = IVec2> {
        (0..h as i32).flat_map(move |y| (0..w as i32).map(move |x| IVec2::new(x, y)))
    }

    /// Everything sees everything — the "no walls" occlusion closure.
    fn open(_: IVec2, _: IVec2) -> bool {
        true
    }

    #[test]
    fn a_fixture_falls_off_linearly_to_its_range() {
        let mut g = LightGrid::new(7, 1, all_floor(7, 1));
        g.bake(&[(IVec2::new(3, 0), 1.0, 3.0)], open);
        g.compose(&[], open);
        assert_eq!(g.sample_cell(IVec2::new(3, 0)), 1.0, "at the fixture: 1 - 0/3");
        assert_eq!(g.sample_cell(IVec2::new(4, 0)), 1.0 - 1.0 / 3.0);
        assert_eq!(g.sample_cell(IVec2::new(6, 0)), 0.0, "exactly at range contributes nothing");
        assert_eq!(g.peak(), 1.0);
    }

    #[test]
    fn walls_cast_shadow() {
        // An occluder that blocks anything crossing x = 3.
        let blocked = |a: IVec2, b: IVec2| !(a.x < 3 && b.x > 3) && !(a.x > 3 && b.x < 3);
        let mut g = LightGrid::new(7, 1, all_floor(7, 1));
        g.bake(&[(IVec2::new(0, 0), 1.0, 10.0)], blocked);
        g.compose(&[], open);
        assert!(g.sample_cell(IVec2::new(2, 0)) > 0.0, "near side is lit");
        assert_eq!(g.sample_cell(IVec2::new(5, 0)), 0.0, "far side is shadowed");
    }

    #[test]
    fn a_cone_lights_ahead_and_not_behind() {
        let mut g = LightGrid::new(9, 9, all_floor(9, 9));
        let cone = FlashlightCone {
            source: IVec2::new(4, 4),
            forward: Vec2::new(1.0, 0.0),
            intensity: 1.0,
            range: 4.0,
            cone_cos: 0.5,
            edge_softness: 0.3,
        };
        g.compose(&[cone], open);
        assert!(g.sample_cell(IVec2::new(6, 4)) > 0.0, "ahead of the beam is lit");
        assert_eq!(g.sample_cell(IVec2::new(2, 4)), 0.0, "behind it is not");
        assert!(g.sample_cell(IVec2::new(4, 4)) > 0.0, "the source cell lights its own footprint");
    }

    #[test]
    fn the_gradient_points_toward_the_light() {
        let mut g = LightGrid::new(9, 1, all_floor(9, 1));
        g.bake(&[(IVec2::new(8, 0), 1.0, 9.0)], open);
        g.compose(&[], open);
        let grad = g.gradient_cell(IVec2::new(4, 0));
        assert!(grad.x > 0.0, "brighter toward +x, so the gradient is too: {grad}");
        // And the taxis helper turns that into opposite pushes for the two dispositions.
        assert!(light_push_at(&g, IVec2::new(4, 0), 1.0).x > 0.0, "photophilic climbs");
        assert!(light_push_at(&g, IVec2::new(4, 0), -1.0).x < 0.0, "photophobic descends");
        assert_eq!(light_push_at(&g, IVec2::new(4, 0), 0.0), Vec3::ZERO, "no gain, no push");
    }

    #[test]
    fn dimming_darkens_and_recomputes_the_peak() {
        let mut g = LightGrid::new(4, 1, all_floor(4, 1));
        g.bake(&[(IVec2::new(0, 0), 1.0, 4.0)], open);
        g.compose(&[], open);
        let before = g.peak();
        let coverage = vec![1.0, 0.0, 0.0, 0.0];
        g.apply_mold_dim(&coverage, 0.75);
        assert_eq!(g.sample_cell(IVec2::ZERO), before * 0.25, "a fully covered cell keeps 1 - dim");
        assert!(g.peak() < before, "the peak follows the dimming down");
    }

    #[test]
    fn dimming_is_a_no_op_at_zero_and_survives_a_short_slice() {
        let mut g = LightGrid::new(4, 1, all_floor(4, 1));
        g.bake(&[(IVec2::new(0, 0), 1.0, 4.0)], open);
        g.compose(&[], open);
        let lit = g.sample_cell(IVec2::ZERO);
        g.apply_mold_dim(&[1.0], 0.0);
        assert_eq!(g.sample_cell(IVec2::ZERO), lit, "dim = 0 returns early");
        // A slice shorter than the grid reads as 0 coverage rather than panicking.
        g.apply_mold_dim(&[], 1.0);
        assert_eq!(g.sample_cell(IVec2::ZERO), lit);
    }

    #[test]
    fn rock_cells_stay_zero_so_the_full_grid_is_foldable() {
        // Only the middle cell is floor; the bake must not write its neighbours even in range.
        let mut g = LightGrid::new(3, 1, [IVec2::new(1, 0)].into_iter());
        g.bake(&[(IVec2::new(1, 0), 1.0, 3.0)], open);
        g.compose(&[], open);
        assert_eq!(g.cells(), &[0.0, 1.0, 0.0]);
    }

    #[test]
    fn phototropic_scale_eases_toward_target_and_is_rate_limited() {
        // light01 = 1 with bonus 1 doubles the target, but a small step only moves a little.
        assert_eq!(phototropic_scale(1.0, 1.0, 1.0, 1.0, 0.1), 1.1);
        // Given room, it lands exactly on target.
        assert_eq!(phototropic_scale(1.0, 1.0, 1.0, 1.0, 10.0), 2.0);
        // Dark means the target is just `base`, and it shrinks back toward it.
        assert_eq!(phototropic_scale(1.0, 2.0, 0.0, 1.0, 10.0), 1.0);
        // Never negative.
        assert_eq!(phototropic_scale(1.0, 0.0, 0.0, 1.0, 10.0), 1.0);
        // light01 is clamped, so an out-of-range input cannot inflate the target.
        assert_eq!(phototropic_scale(1.0, 1.0, 9.0, 1.0, 10.0), 2.0);
    }

    #[test]
    fn off_grid_reads_are_zero_rather_than_a_panic() {
        let g = LightGrid::new(2, 2, all_floor(2, 2));
        assert_eq!(g.sample_cell(IVec2::new(-1, 0)), 0.0);
        assert_eq!(g.sample_cell(IVec2::new(0, 5)), 0.0);
    }
}
