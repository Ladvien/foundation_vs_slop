//! Stage-5 extensibility acceptance tests — the falsifiable checks for the three axes the whole
//! architecture exists to provide (vetting §6, Stage 5). If these pass, the axes hold:
//!   1. **Backend swap** — a rule group can be handled by a different `Solver` with the grammar (the
//!      `PlacementProblem` IR) unchanged.
//!   2. **Asset swap** — repointing the manifest at an entirely different kit changes output with zero
//!      code diff (same schema, same code path).
//!   3. **Domain swap** — a `Region` can model an exterior parcel and a brand-new predicate
//!      (`aligned(a, "road")`) routes through the *same* orchestrator — a new domain adds predicates,
//!      not engines.
//!
//! This module is test-only.

use rand_chacha::ChaCha8Rng;

use super::ir::{
    Candidate, Capabilities, Constraint, Dof, Hardness, Host, Locality, Modality, Opening, Outcome,
    Placement, PlacementProblem, Predicate, PropertyBag, Rect2, Region, Role, Scope, SolveError,
};
use super::manifest::load_manifest;
use super::solver::{Orchestrator, Solver};
use super::solvers::constraint::ConstraintSolver;
use super::solvers::metropolis::{MetropolisSolver, MetropolisWeights};
use super::solvers::wfc::WfcSolver;
use crate::rng::seeded;

fn test_weights() -> MetropolisWeights {
    MetropolisWeights {
        iterations: 2500,
        temp_start: 1.0,
        temp_end: 0.02,
        translate_sigma: 0.6,
        rotate_prob: 0.35,
        w_overlap: 10.0,
        w_bounds: 25.0,
        w_wall: 1.2,
        w_min_distance: 2.0,
        w_facing: 1.5,
        w_clearance: 2.0,
    }
}

/// The production backend set (the same one `build_solvers` assembles).
fn full_orchestrator() -> Orchestrator {
    let mut o = Orchestrator::new();
    o.register(Box::new(WfcSolver));
    o.register(Box::new(MetropolisSolver::new(test_weights())));
    o.register(Box::new(ConstraintSolver));
    o
}

fn room(id: u32, w: i32, h: i32) -> Region {
    Region {
        id,
        rect: Rect2 { min: [0, 0], max: [w, h] },
        openings: vec![
            Opening { dir: 0, cell: [2, 0] },
            Opening { dir: 1, cell: [w - 1, 2] },
        ],
        adjacency: Vec::new(),
        props: PropertyBag::default(),
    }
}

fn tiled(asset: &str) -> Candidate {
    Candidate {
        asset: asset.into(),
        role: Role::Tiled,
        footprint: [1.0, 1.0],
        dof: Dof::default(),
        affordances: Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------------
// Axis 1 — backend swap: same grammar, a different Solver for the rule group.
// ---------------------------------------------------------------------------------------------------

/// An alternate Hard+Local backend that ignores WFC entirely and fills every tile — a recognizably
/// different implementation of the same capability profile.
struct FillEverythingSolver;
impl Solver for FillEverythingSolver {
    fn name(&self) -> &str {
        "fill-everything"
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
    fn solve(&self, problem: &PlacementProblem, _rng: &mut ChaCha8Rng) -> Result<Outcome, SolveError> {
        let r = problem.region.rect;
        let mut placed = Vec::new();
        for y in r.min[1]..r.max[1] {
            for x in r.min[0]..r.max[0] {
                placed.push(Placement { candidate: 0, pos: [x as f32, 0.0, y as f32], yaw: 0.0 });
            }
        }
        Ok(Outcome::Assignment(placed))
    }
}

#[test]
fn axis1_backend_swap_same_grammar() {
    // One grammar instance (the PlacementProblem) — reused verbatim against two backend registries.
    let region = room(0, 4, 4);
    let make_problem = || PlacementProblem {
        region: &region,
        candidates: vec![tiled("prop")],
        constraints: Vec::new(),
    };

    // Registry A: the default WFC backend (sparse scatter).
    let mut a = Orchestrator::new();
    a.register(Box::new(WfcSolver));
    let out_a = a.solve_group(&make_problem(), &mut seeded(1)).expect("A solves");

    // Registry B: swap in the alternate backend — grammar (the problem) is byte-for-byte identical.
    let mut b = Orchestrator::new();
    b.register(Box::new(FillEverythingSolver));
    let out_b = b.solve_group(&make_problem(), &mut seeded(1)).expect("B solves");

    let count = |o: &Outcome| match o {
        Outcome::Assignment(p) => p.len(),
        _ => 0,
    };
    // The alternate backend fills all 16 cells; WFC scatters far fewer. Same grammar, different engine.
    assert_eq!(count(&out_b), 16, "alternate backend fills every tile");
    assert!(count(&out_a) < 16, "WFC scatter is sparse");
    assert_ne!(count(&out_a), count(&out_b), "swapping the backend changed the output");
}

// ---------------------------------------------------------------------------------------------------
// Axis 2 — asset swap: repoint the manifest at a different kit, zero code diff.
// ---------------------------------------------------------------------------------------------------

#[test]
fn axis2_asset_swap_manifest_only() {
    // Both manifests are consumed by the identical code path (`load_manifest`); the only thing that
    // differs is which RON file we name.
    let furniture = load_manifest("assets/placement/furniture.ron").expect("kit A parses");
    let kenney = load_manifest("assets/placement/furniture_kenney.ron").expect("kit B parses");

    // Different kits → different GLB paths, same schema and roles. Both kits carry a wall-anchored
    // light (an `Anchor(Wall)`) pointing at their own asset, so the swap is manifest-only.
    let a_wall = furniture
        .by_role(|r| matches!(r, Role::Anchor { host: Host::Wall }))
        .first()
        .copied()
        .expect("kit A has a wall anchor");
    let b_wall = kenney
        .by_role(|r| matches!(r, Role::Anchor { host: Host::Wall }))
        .first()
        .copied()
        .expect("kit B has a wall anchor");
    assert_ne!(a_wall.glb, b_wall.glb, "the two kits point at different assets");
    assert!(matches!(a_wall.role, Role::Anchor { host: Host::Wall }));
    assert!(matches!(b_wall.role, Role::Anchor { host: Host::Wall }));

    // Both kits yield candidates that the SAME orchestrator can route — no code changed between them.
    let orch = full_orchestrator();
    for manifest in [&furniture, &kenney] {
        let tiled_items: Vec<Candidate> = manifest
            .by_role(|r| matches!(r, Role::Tiled))
            .iter()
            .map(|it| Candidate {
                asset: it.key.clone(),
                role: it.role.clone(),
                footprint: [it.footprint.0, it.footprint.1],
                dof: Dof::default(),
                affordances: it.affordances.clone(),
            })
            .collect();
        assert!(!tiled_items.is_empty(), "each kit has at least one tiled prop");
        let region = room(7, 4, 4);
        let problem = PlacementProblem { region: &region, candidates: tiled_items, constraints: Vec::new() };
        let out = orch.solve_group(&problem, &mut seeded(5)).expect("routes + solves");
        assert!(matches!(out, Outcome::Assignment(_)));
    }
}

// ---------------------------------------------------------------------------------------------------
// Axis 3 — domain swap: a Region as an exterior parcel + a new `aligned(a, road)` predicate, routed
// through the SAME orchestrator. No new engine.
// ---------------------------------------------------------------------------------------------------

#[test]
fn axis3_domain_swap_new_predicate_same_orchestrator() {
    // A parcel is just a bounded container with different property tags — the same `Region` type.
    let mut parcel = Region {
        id: 100,
        rect: Rect2 { min: [0, 0], max: [10, 8] },
        openings: Vec::new(),
        adjacency: Vec::new(),
        props: PropertyBag { tags: vec!["parcel".into(), "exterior".into()] },
    };
    parcel.props.tags.push("road_south".into());

    // "Buildings" on the parcel, each asked to align to the road — a predicate interiors never use.
    let buildings = vec![
        Candidate { asset: "house_a".into(), role: Role::Freestanding, footprint: [2.0, 2.0], dof: Dof { rotate_quarter: true, ..Default::default() }, affordances: vec![] },
        Candidate { asset: "house_b".into(), role: Role::Freestanding, footprint: [2.5, 1.5], dof: Dof { rotate_quarter: true, ..Default::default() }, affordances: vec![] },
    ];
    let constraints = vec![
        Constraint { id: 0, scope: Scope::Object(0), predicate: Predicate::Aligned("road".into()), modality: Modality::Soft(2.0), guard: None },
        Constraint { id: 1, scope: Scope::Object(1), predicate: Predicate::Aligned("road".into()), modality: Modality::Soft(2.0), guard: None },
    ];

    // The SAME orchestrator that furnishes interiors handles the parcel — new predicate, no new engine.
    let orch = full_orchestrator();
    let problem = PlacementProblem { region: &parcel, candidates: buildings.clone(), constraints };
    let out = orch.solve_group(&problem, &mut seeded(9)).expect("parcel routes + solves");

    let placed = match out {
        Outcome::Ranked(mut v) => v.remove(0).1,
        Outcome::Assignment(p) => p,
        Outcome::Partial { placed, .. } => placed,
    };
    assert_eq!(placed.len(), 2, "both buildings placed");
    // The alignment predicate did its job: each building's yaw is (near) parallel to the road axis,
    // i.e. |sin(yaw)| ≈ 0 (yaw ∈ {0, π}).
    for p in &placed {
        assert!(p.yaw.sin().abs() < 0.2, "building yaw {} not aligned to the road axis", p.yaw);
    }
}
