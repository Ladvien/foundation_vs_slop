//! `ConstraintSolver` — the Hard + Global + Cardinality backend. Closes the one architectural gap
//! that falls between local WFC (adjacency only) and soft Metropolis (no exact counts): rules like
//! "exactly one door per room" (a `count(...)` over the whole region) and other global selections.
//!
//! This is a small finite-domain constraint solver in the spirit of Smith & Mateas, "Answer Set
//! Programming for Procedural Content Generation" (IEEE TCIAIG 2011), and the practical finite-domain
//! solver of *Game AI Pro 2*, Ch. 26. A candidate with discrete region-derived sites (e.g. an
//! `Anchor(Opening)` door, whose sites are the region's openings) becomes a variable; a
//! `Count { tag, count }` constraint fixes how many of those sites are chosen. Selection is seeded, so
//! the satisfying assignment is reproducible.

use rand_chacha::ChaCha8Rng;

use crate::placement::ir::{
    Candidate, Capabilities, Hardness, Host, Locality, Outcome, Placement, PlacementProblem,
    Predicate, Region, Role, SolveError,
};
use crate::placement::solver::Solver;
use crate::rng::DetRng;

pub struct ConstraintSolver;

impl Solver for ConstraintSolver {
    fn name(&self) -> &str {
        "constraint"
    }

    fn handles(&self, role: &Role) -> bool {
        matches!(role, Role::Anchor { host: Host::Opening })
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            hardness: Hardness::Hard,
            locality: Locality::Global,
            cardinality: true,
            deterministic: true,
            needs_training_data: false,
        }
    }

    /// For each candidate, choose exactly `count` of its discrete sites where a matching
    /// `Count { tag, count }` constraint applies (default = all sites if unconstrained). The chosen
    /// sites are drawn from the seeded RNG so the assignment is reproducible.
    fn solve(&self, problem: &PlacementProblem, rng: &mut ChaCha8Rng) -> Result<Outcome, SolveError> {
        let mut placed: Vec<Placement> = Vec::new();

        for (ci, cand) in problem.candidates.iter().enumerate() {
            let sites = candidate_sites(cand, problem.region);
            if sites.is_empty() {
                continue;
            }
            // The required count for this candidate: the smallest matching Count constraint, else all.
            let count = problem
                .constraints
                .iter()
                .filter_map(|c| match &c.predicate {
                    Predicate::Count { tag, count } if candidate_has_tag(cand, tag) => Some(*count),
                    _ => None,
                })
                .min()
                .unwrap_or(sites.len());
            let count = count.min(sites.len());

            for idx in choose_k(sites.len(), count, rng) {
                let cell = sites[idx];
                placed.push(Placement {
                    candidate: ci,
                    pos: [cell[0] as f32, 0.0, cell[1] as f32],
                    yaw: 0.0,
                });
            }
        }

        Ok(Outcome::Assignment(placed))
    }
}

/// The discrete placement sites a candidate can occupy within a region. Opening-anchored candidates
/// (doors) can go in any of the region's openings; other roles have no region-derived discrete domain
/// here (they are other backends' concern).
fn candidate_sites(cand: &Candidate, region: &Region) -> Vec<[i32; 2]> {
    match cand.role {
        Role::Anchor { host: Host::Opening } => region.openings.iter().map(|o| o.cell).collect(),
        _ => Vec::new(),
    }
}

/// Does a candidate carry a Count tag? Matched against its affordance tokens (the portable, kit-neutral
/// way to name "this is a door") or, as a fallback, its asset key.
fn candidate_has_tag(cand: &Candidate, tag: &str) -> bool {
    cand.affordances.iter().any(|a| a == tag) || cand.asset == tag
}

/// Choose `k` distinct indices from `0..n`, seeded. A partial Fisher–Yates: reproducible and unbiased.
fn choose_k(n: usize, k: usize, rng: &mut ChaCha8Rng) -> Vec<usize> {
    let k = k.min(n);
    let mut pool: Vec<usize> = (0..n).collect();
    for i in 0..k {
        let j = i + rng.below(n - i);
        pool.swap(i, j);
    }
    pool.truncate(k);
    pool
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::ir::{
        Constraint, Dof, Modality, Opening, PropertyBag, Rect2, Region, Scope,
    };
    use crate::rng::seeded;
    use crate::wfc::{E, N, W};

    fn region_with_openings() -> Region {
        Region {
            id: 0,
            rect: Rect2 { min: [0, 0], max: [5, 5] },
            openings: vec![
                Opening { dir: N, cell: [2, 0] },
                Opening { dir: E, cell: [4, 2] },
                Opening { dir: W, cell: [0, 2] },
            ],
            adjacency: Vec::new(),
            props: PropertyBag::default(),
        }
    }
    fn door() -> Candidate {
        Candidate {
            asset: "door".into(),
            role: Role::Anchor { host: Host::Opening },
            footprint: [0.9, 0.3],
            dof: Dof::default(),
            affordances: vec!["door".into()],
        }
    }
    fn count_door(count: usize) -> Constraint {
        Constraint {
            id: 0,
            scope: Scope::Region,
            predicate: Predicate::Count { tag: "door".into(), count },
            modality: Modality::Hard,
            guard: None,
        }
    }

    fn placements(count: usize, seed: u64) -> Vec<Placement> {
        let r = region_with_openings();
        let problem = PlacementProblem {
            region: &r,
            candidates: vec![door()].into(),
            constraints: vec![count_door(count)],
        };
        let mut rng = seeded(seed);
        match ConstraintSolver.solve(&problem, &mut rng).expect("solve") {
            Outcome::Assignment(p) => p,
            other => panic!("expected assignment, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn exactly_one_door_per_room() {
        let p = placements(1, 7);
        assert_eq!(p.len(), 1, "cardinality count=1 must place exactly one door");
        // The chosen site must be one of the region's openings.
        let r = region_with_openings();
        let cell = [p[0].pos[0] as i32, p[0].pos[2] as i32];
        assert!(r.openings.iter().any(|o| o.cell == cell));
    }

    #[test]
    fn count_two_places_two_distinct_doors() {
        let p = placements(2, 7);
        assert_eq!(p.len(), 2);
        assert_ne!(
            [p[0].pos[0] as i32, p[0].pos[2] as i32],
            [p[1].pos[0] as i32, p[1].pos[2] as i32],
            "the two doors must be at distinct openings"
        );
    }

    #[test]
    fn count_exceeding_sites_is_clamped() {
        let p = placements(9, 7); // only 3 openings exist
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn selection_is_deterministic() {
        assert_eq!(
            placements(1, 42).iter().map(|p| p.pos[0] as i32).collect::<Vec<_>>(),
            placements(1, 42).iter().map(|p| p.pos[0] as i32).collect::<Vec<_>>(),
        );
    }
}
