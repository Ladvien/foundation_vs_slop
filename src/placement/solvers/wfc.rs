//! `WfcSolver` — the Hard + Local placement backend. Wraps the engine-free grid Wave Function
//! Collapse primitive (`crate::wfc::collapse_grid`) so `Role::Tiled` candidates fill a region's tile
//! grid under local adjacency, degrading to `Outcome::Partial` (never a panic) if the grid can't be
//! collapsed within its restart budget.
//!
//! Basis: Gumin 2016 (WFC); Merrell & Manocha 2011 (model synthesis — the richer constraint
//! taxonomy this generalizes toward); Karth & Smith 2017 (WFC *is* finite-domain constraint solving,
//! which is why it slots behind the same `Solver` trait as every other backend).

use rand_chacha::ChaCha8Rng;

use crate::placement::ir::{
    Capabilities, Hardness, Locality, Outcome, Placement, PlacementProblem, Role, SolveError,
};
use crate::placement::solver::Solver;
use crate::rng::DetRng;
use crate::wfc::collapse_grid;

/// Bounded collapse restarts before the solver gives up and returns `Partial`. WFC is greedy and can
/// hit a contradiction (deciding a non-failing tiling is NP-hard — Merrell 2011); a handful of
/// reseeded attempts clears the vast majority, and the rest degrade gracefully rather than looping.
const MAX_RESTARTS: u32 = 12;

pub struct WfcSolver;

impl Solver for WfcSolver {
    fn name(&self) -> &str {
        "wfc"
    }

    fn handles(&self, role: &Role) -> bool {
        matches!(role, Role::Tiled)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            hardness: Hardness::Hard,
            locality: Locality::Local,
            cardinality: false,
            deterministic: true,
            needs_training_data: false,
        }
    }

    /// Tile the region's grid with the problem's `Role::Tiled` candidates. Coordinates in the returned
    /// `Placement`s are region tile-grid cells (integer cell centres as `f32`); the Bevy furnish pass
    /// maps them to world space via the `Dungeon`. Non-`Tiled` candidates are ignored here — they are
    /// another backend's job, and the orchestrator only routes a group to this solver when its
    /// capability profile is Hard + Local.
    fn solve(&self, problem: &PlacementProblem, rng: &mut ChaCha8Rng) -> Result<Outcome, SolveError> {
        // The tileable candidates become prototypes 1..=k; prototype 0 is "empty cell" (place nothing),
        // so the collapse can leave gaps rather than being forced to fill every cell.
        let tiled: Vec<usize> = problem
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c.role, Role::Tiled))
            .map(|(i, _)| i)
            .collect();
        if tiled.is_empty() {
            return Ok(Outcome::Assignment(Vec::new()));
        }

        let region = problem.region;
        let w = region.rect.width().max(0) as usize;
        let h = region.rect.height().max(0) as usize;
        if w == 0 || h == 0 {
            return Ok(Outcome::Assignment(Vec::new()));
        }

        // Alphabet weights: empty + one per tiled candidate. Empty is weighted so a tile and "no tile"
        // are equally likely per cell, giving a sparse, non-saturated fill. When adjacency predicates
        // are compiled into `support` (Stage 4+), this same alphabet gains real local constraints;
        // until then every prototype is compatible beside every other, so the collapse is a weighted
        // fill that can never contradict.
        let n = tiled.len() + 1;
        let mut weights = vec![1.0_f64; n];
        weights[0] = 3.0 * tiled.len() as f64; // bias toward "empty" → a sparse scatter (~25% filled)
        let full: u32 = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        let support: [Vec<u32>; 4] = [
            vec![full; n],
            vec![full; n],
            vec![full; n],
            vec![full; n],
        ];

        // Reseed each restart from the region's RNG sub-stream so retries are distinct yet reproducible.
        for _ in 0..MAX_RESTARTS {
            let seed = rng.raw_u64();
            if let Some(picks) = collapse_grid(w, h, &weights, &support, seed) {
                let placed = picks
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, &proto)| {
                        if proto == 0 {
                            return None; // empty cell
                        }
                        let candidate = tiled[proto - 1];
                        let cx = region.rect.min[0] + (idx % w) as i32;
                        let cy = region.rect.min[1] + (idx / w) as i32;
                        Some(Placement {
                            candidate,
                            pos: [cx as f32, 0.0, cy as f32],
                            yaw: 0.0,
                        })
                    })
                    .collect();
                return Ok(Outcome::Assignment(placed));
            }
        }

        // Every restart contradicted: hand back nothing placed plus the hard constraints we could not
        // satisfy, so the caller sees an honest partial rather than a silent empty success.
        let unsatisfied = problem.constraints.iter().map(|c| c.id).collect();
        Ok(Outcome::Partial {
            placed: Vec::new(),
            unsatisfied,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::ir::{Candidate, Dof, PropertyBag, Rect2, Region};
    use crate::rng::seeded;

    fn region() -> Region {
        Region {
            id: 0,
            rect: Rect2 {
                min: [3, 5],
                max: [7, 9],
            },
            openings: Vec::new(),
            adjacency: Vec::new(),
            props: PropertyBag::default(),
        }
    }
    fn tiled(asset: &str) -> Candidate {
        Candidate {
            asset: asset.to_string(),
            role: Role::Tiled,
            footprint: [1.0, 1.0],
            dof: Dof::default(),
            affordances: Vec::new(),
        }
    }

    #[test]
    fn placements_stay_inside_the_region_rect() {
        let r = region();
        let problem = PlacementProblem {
            region: &r,
            candidates: vec![tiled("a"), tiled("b")].into(),
            constraints: Vec::new(),
        };
        let mut rng = seeded(7);
        let Ok(Outcome::Assignment(ps)) = WfcSolver.solve(&problem, &mut rng) else {
            panic!("expected an assignment from an all-compatible tiling");
        };
        assert!(!ps.is_empty(), "a weighted fill should place at least one tile");
        for p in &ps {
            assert!(
                r.rect.contains([p.pos[0] as i32, p.pos[2] as i32]),
                "placement {:?} escaped region {:?}",
                p.pos,
                r.rect
            );
            assert!(p.candidate < 2, "candidate index out of range");
        }
    }

    #[test]
    fn no_tiled_candidates_yields_empty_assignment() {
        let r = region();
        let mut freestanding = tiled("a");
        freestanding.role = Role::Freestanding;
        let problem = PlacementProblem {
            region: &r,
            candidates: vec![freestanding].into(),
            constraints: Vec::new(),
        };
        let mut rng = seeded(7);
        let out = WfcSolver.solve(&problem, &mut rng).expect("solve");
        assert!(matches!(out, Outcome::Assignment(ref p) if p.is_empty()));
    }

    #[test]
    fn deterministic_under_a_seed() {
        let r = region();
        let problem = PlacementProblem {
            region: &r,
            candidates: vec![tiled("a")].into(),
            constraints: Vec::new(),
        };
        let run = || {
            let mut rng = seeded(99);
            match WfcSolver.solve(&problem, &mut rng).expect("solve") {
                Outcome::Assignment(p) => p.iter().map(|p| (p.pos[0] as i32, p.pos[2] as i32)).collect::<Vec<_>>(),
                _ => panic!("expected assignment"),
            }
        };
        assert_eq!(run(), run(), "same seed must reproduce the same tiling");
    }
}
