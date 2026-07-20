//! Surface scatter — the fine level of the two-level placement grid. Places small props (lamp, plant,
//! TV) *on* a support surface (desk / table / drawer) by subdividing that surface's top into an inner
//! `INNER_GRID`×`INNER_GRID` lattice and dropping each prop into a free sub-cell.
//!
//! This runs AFTER the freestanding layout because it reads the *placed* support pieces: following
//! Tutenel, Smelik, Bidarra & De Kraker ("A Semantic Scene Description Language for Procedural Layout
//! Solving", AIIDE 2010), a support is a first-class *surface feature* and a prop's vertical position
//! falls out of that surface's height rather than being guessed. It is kept engine-free (no `bevy::`)
//! and seeded so it unit-tests without a GPU and reproduces under a seed, exactly like the solvers.
//!
//! It is a furnish *pass* rather than a `Solver`: a `Solver` sees only its own role-homogeneous
//! `PlacementProblem`, but scatter needs the already-placed supports as context — so the Bevy furnish
//! pass adapts them into [`SupportSurface`]s and calls [`scatter_on_surfaces`] directly.

use rand_chacha::ChaCha8Rng;

use crate::placement::ir::{CandidateIx, Placement};
use crate::rng::DetRng;

/// Inner lattice resolution: every support's top is subdivided `INNER_GRID`×`INNER_GRID` (the fine
/// "9×9 within 9×9" grid). A prop occupies a rectangular block of sub-cells sized to its footprint.
pub const INNER_GRID: usize = 9;

/// The usable top of a placed support piece: centre `(cx, cz)` in world coords, half-extents of the
/// area props may occupy (the footprint already inset so props don't overhang), `top_y` — the height
/// props rest at — and `provides`: a bitmask of the support-surface *classes* this top offers (e.g. a
/// desk provides both the generic `support` class and the `worktop` class; a bed provides only
/// `support`). A prop is only placed here when `provides & prop.requires != 0`, so a typed prop (a
/// desk lamp) never lands on the wrong surface (a bed). Following Tutenel et al. 2010, a support is a
/// *typed* surface feature, not a generic shelf.
#[derive(Clone, Copy, Debug)]
pub struct SupportSurface {
    pub cx: f32,
    pub cz: f32,
    pub half_x: f32,
    pub half_z: f32,
    pub top_y: f32,
    /// The host piece's yaw (radians about +Y). A prop rested here inherits it, so a TV on a chest of
    /// drawers faces the same way the drawers open (both fronts point into the room). Merrell et al.
    /// 2011 "align with prominent features"; Infinigen Indoors (Raistrick et al. 2024) spatial-relation
    /// constraints treat a support and the object on it as co-oriented.
    pub yaw: f32,
    /// Bitmask of surface classes this top offers (see [`crate::placement::furnish`] `surface_bits`).
    pub provides: u32,
}

/// A prop to rest on a surface: its candidate index (for the caller to map back to an asset), its
/// footprint half-extents in world units, and `requires`: the bitmask of support-surface classes it is
/// allowed to rest on. A surface is eligible for this prop iff `surface.provides & requires != 0`; a
/// `requires` of 0 (an unknown/unmatched surface token) means the prop rests on nothing and is dropped.
#[derive(Clone, Copy, Debug)]
pub struct ScatterProp {
    pub candidate: CandidateIx,
    pub half_x: f32,
    pub half_z: f32,
    /// Bitmask of surface classes this prop may rest on (see [`crate::placement::furnish`] `surface_bits`).
    pub requires: u32,
}

#[inline]
fn idx(ox: usize, oz: usize) -> usize {
    oz * INNER_GRID + ox
}

/// Is the `span_x`×`span_z` block of sub-cells with origin `(ox, oz)` entirely free?
fn block_free(occ: &[bool], ox: usize, oz: usize, span_x: usize, span_z: usize) -> bool {
    for dz in 0..span_z {
        for dx in 0..span_x {
            if occ[idx(ox + dx, oz + dz)] {
                return false;
            }
        }
    }
    true
}

/// Mark a `span_x`×`span_z` block of sub-cells occupied.
fn mark(occ: &mut [bool], ox: usize, oz: usize, span_x: usize, span_z: usize) {
    for dz in 0..span_z {
        for dx in 0..span_x {
            occ[idx(ox + dx, oz + dz)] = true;
        }
    }
}

/// Place each prop on a support surface using the inner lattice. Deterministic under `rng`; a prop that
/// fits nowhere (too big for any surface, or every surface full) is dropped — so the returned list has
/// at most `props.len()` entries. Each returned [`Placement`] has `pos.y` set to the chosen surface's
/// `top_y`, so the prop rests on the surface rather than the floor.
pub fn scatter_on_surfaces(
    surfaces: &[SupportSurface],
    props: &[ScatterProp],
    rng: &mut ChaCha8Rng,
) -> Vec<Placement> {
    let mut out = Vec::new();
    if surfaces.is_empty() {
        return out;
    }
    // Per-surface occupancy over the inner lattice.
    let mut occ: Vec<Vec<bool>> = vec![vec![false; INNER_GRID * INNER_GRID]; surfaces.len()];

    for prop in props {
        // Rotate the surface scan by a random start so props spread across surfaces deterministically.
        let start = rng.below(surfaces.len());
        for s_off in 0..surfaces.len() {
            let si = (start + s_off) % surfaces.len();
            let s = surfaces[si];
            // Surface-class gate: this prop may only rest on a support whose provided classes include
            // one the prop requires (a desk lamp → worktop, never a bed). A prop with no matching
            // surface simply isn't placed — one path, no fallback onto the wrong furniture.
            if s.provides & prop.requires == 0 {
                continue;
            }
            let cell_w = (2.0 * s.half_x) / INNER_GRID as f32;
            let cell_d = (2.0 * s.half_z) / INNER_GRID as f32;
            if cell_w <= 0.0 || cell_d <= 0.0 {
                continue;
            }
            // How many sub-cells the prop's footprint needs (at least one).
            let span_x = ((prop.half_x * 2.0) / cell_w).ceil().max(1.0) as usize;
            let span_z = ((prop.half_z * 2.0) / cell_d).ceil().max(1.0) as usize;
            if span_x > INNER_GRID || span_z > INNER_GRID {
                continue; // prop is larger than this whole surface — try the next one
            }
            let max_ox = INNER_GRID - span_x;
            let max_oz = INNER_GRID - span_z;
            // Scan every legal block origin once, starting from a random offset so placement varies.
            let ox0 = rng.below(max_ox + 1);
            let oz0 = rng.below(max_oz + 1);
            let mut done = false;
            for j in 0..=max_oz {
                for i in 0..=max_ox {
                    let ox = (ox0 + i) % (max_ox + 1);
                    let oz = (oz0 + j) % (max_oz + 1);
                    if block_free(&occ[si], ox, oz, span_x, span_z) {
                        mark(&mut occ[si], ox, oz, span_x, span_z);
                        // Centre of the occupied block, in surface-local then world coords.
                        let bx = ox as f32 + span_x as f32 * 0.5;
                        let bz = oz as f32 + span_z as f32 * 0.5;
                        let x = s.cx - s.half_x + bx * cell_w;
                        let z = s.cz - s.half_z + bz * cell_d;
                        out.push(Placement {
                            candidate: prop.candidate,
                            pos: [x, s.top_y, z],
                            // Inherit the support's facing so the prop faces the same way its host does
                            // (a TV on a drawer faces into the room, matching the drawers' front).
                            yaw: s.yaw,
                        });
                        done = true;
                        break;
                    }
                }
                if done {
                    break;
                }
            }
            if done {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::seeded;

    // Test surfaces provide every class and test props require class bit 0, so the class gate is a
    // no-op here (`!0 & 1 != 0`) — the placement-geometry tests below are unaffected by the gate. The
    // gate itself is exercised by `a_prop_only_rests_on_a_matching_surface_class`.
    fn surface(cx: f32, cz: f32, half_x: f32, half_z: f32, top_y: f32) -> SupportSurface {
        SupportSurface {
            cx,
            cz,
            half_x,
            half_z,
            top_y,
            yaw: 0.0,
            provides: !0,
        }
    }
    fn prop(candidate: usize, half_x: f32, half_z: f32) -> ScatterProp {
        ScatterProp {
            candidate,
            half_x,
            half_z,
            requires: 1,
        }
    }

    #[test]
    fn props_rest_on_the_surface_within_its_footprint() {
        // A 1.2×0.6 desk top at height 0.74, three small props. Each must land ON the surface (y=0.74),
        // inside its footprint, and not share a sub-cell with another prop.
        let s = surface(5.0, 3.0, 0.6, 0.3, 0.74);
        let props = [prop(0, 0.16, 0.16), prop(1, 0.09, 0.09), prop(2, 0.1, 0.1)];
        let mut rng = seeded(4);
        let placed = scatter_on_surfaces(&[s], &props, &mut rng);
        assert_eq!(placed.len(), 3, "all three small props fit on the desk");
        for p in &placed {
            assert!(
                (p.pos[1] - 0.74).abs() < 1e-6,
                "prop must rest at the surface top, not the floor"
            );
            assert!(
                (p.pos[0] - s.cx).abs() <= s.half_x + 1e-4,
                "prop x within surface footprint"
            );
            assert!(
                (p.pos[2] - s.cz).abs() <= s.half_z + 1e-4,
                "prop z within surface footprint"
            );
        }
        // Distinct positions (different sub-cells).
        for a in 0..placed.len() {
            for b in (a + 1)..placed.len() {
                let same = (placed[a].pos[0] - placed[b].pos[0]).abs() < 1e-6
                    && (placed[a].pos[2] - placed[b].pos[2]).abs() < 1e-6;
                assert!(!same, "two props landed on the same sub-cell");
            }
        }
    }

    #[test]
    fn a_prop_too_big_for_any_surface_is_dropped() {
        let s = surface(0.0, 0.0, 0.25, 0.25, 0.5); // tiny 0.5×0.5 surface
        let props = [prop(0, 0.9, 0.9)]; // 1.8×1.8 prop — cannot fit
        let mut rng = seeded(1);
        let placed = scatter_on_surfaces(&[s], &props, &mut rng);
        assert!(
            placed.is_empty(),
            "an over-sized prop is dropped, never forced on"
        );
    }

    #[test]
    fn a_prop_only_rests_on_a_matching_surface_class() {
        // Two surfaces: a worktop (provides bit 0b01|0b10) and a bed (provides bit 0b01 only). A desk
        // lamp requires the worktop class (0b10). It must land on the worktop, never the bed — even
        // though the bed has ample free area. This is the fix for the "desk lamp floating on a bed" bug.
        const SUPPORT: u32 = 0b01;
        const WORKTOP: u32 = 0b10;
        let worktop = SupportSurface {
            cx: 0.0,
            cz: 0.0,
            half_x: 0.6,
            half_z: 0.3,
            top_y: 0.74,
            yaw: 0.0,
            provides: SUPPORT | WORKTOP,
        };
        let bed = SupportSurface {
            cx: 10.0,
            cz: 10.0,
            half_x: 0.86,
            half_z: 1.11,
            top_y: 0.55,
            yaw: 0.0,
            provides: SUPPORT,
        };
        let lamp = ScatterProp {
            candidate: 0,
            half_x: 0.16,
            half_z: 0.16,
            requires: WORKTOP,
        };
        // Try many seeds: the lamp must NEVER land on the bed regardless of the random surface scan.
        for seed in 0..32u64 {
            let mut rng = seeded(seed);
            let placed = scatter_on_surfaces(&[worktop, bed], &[lamp], &mut rng);
            assert_eq!(placed.len(), 1, "lamp fits on the worktop");
            assert!(
                (placed[0].pos[1] - worktop.top_y).abs() < 1e-6,
                "lamp rested at the worktop height, not the bed"
            );
            assert!(
                (placed[0].pos[0] - worktop.cx).abs() <= worktop.half_x + 1e-4
                    && (placed[0].pos[2] - worktop.cz).abs() <= worktop.half_z + 1e-4,
                "lamp landed inside the worktop footprint (never near the bed at (10,10))"
            );
        }
        // With ONLY a bed available, the worktop-only lamp is dropped, not forced onto the bed.
        let mut rng = seeded(1);
        assert!(
            scatter_on_surfaces(&[bed], &[lamp], &mut rng).is_empty(),
            "no worktop -> no lamp (never falls back onto a bed)"
        );
    }

    #[test]
    fn no_surfaces_places_nothing() {
        let props = [prop(0, 0.1, 0.1)];
        let mut rng = seeded(1);
        assert!(scatter_on_surfaces(&[], &props, &mut rng).is_empty());
    }

    #[test]
    fn deterministic_under_a_seed() {
        let s = surface(2.0, 2.0, 0.6, 0.3, 0.74);
        let props = [prop(0, 0.16, 0.16), prop(1, 0.1, 0.1)];
        let run = || {
            let mut rng = seeded(9);
            scatter_on_surfaces(&[s], &props, &mut rng)
                .iter()
                .map(|p| (p.pos[0].to_bits(), p.pos[1].to_bits(), p.pos[2].to_bits()))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run(), "same seed must reproduce the same scatter");
    }
}
