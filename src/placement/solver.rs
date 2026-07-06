//! The pluggable-backend seam (vetting §2, extensibility axis #1): a `Solver` trait every backend
//! implements, plus an orchestrator that routes each compiled constraint group to the first
//! registered backend whose `Capabilities` cover the group's needs.
//!
//! This is deliberately engine-free. WFC, Metropolis, and the cardinality solver all become impls of
//! one trait consuming one IR (`PlacementProblem`), so a learned/LLM backend is a drop-in later.
#![allow(dead_code)] // Orchestrator wiring is exercised from Stage 1 onward.

use rand_chacha::ChaCha8Rng;

use super::ir::{
    Capabilities, Constraint, Hardness, Locality, Modality, Outcome, Predicate, PlacementProblem,
    Role, SolveError,
};

/// A placement backend. `solve` takes the region's RNG sub-stream so results are reproducible under a
/// seed regardless of the order regions/threads run in (determinism invariant, §4). It returns a
/// `Result` and degrades to `Outcome::Partial` on trouble — it must never panic.
pub trait Solver: Send + Sync {
    /// Stable, human-readable name (diagnostics + the backend-swap acceptance test).
    fn name(&self) -> &str;
    /// The candidate `Role` this backend places — the routing key. Every `PlacementProblem` the furnish
    /// pass builds is role-homogeneous (the catalogue is partitioned `by_role` first) and each backend
    /// already dispatches on role internally, so a group's role names its backend 1:1. Roles partition
    /// the backends, so routing is unambiguous and registration-order-independent.
    fn handles(&self, role: &Role) -> bool;
    /// Constraint shapes this backend can satisfy. NOT a routing key (role is) — `solve_group` uses it
    /// only as a post-route guard that the role-selected backend can honor the group's constraints,
    /// failing loud on a mismatch instead of letting a backend silently ignore a predicate it can't
    /// model.
    fn capabilities(&self) -> Capabilities;
    fn solve(&self, problem: &PlacementProblem, rng: &mut ChaCha8Rng) -> Result<Outcome, SolveError>;
}

/// The capability profile a constraint group demands — computed from the compiled constraints, then
/// matched against each backend's `Capabilities`.
#[derive(Clone, Copy, Debug)]
pub struct Requirement {
    pub hardness: Hardness,
    pub locality: Locality,
    pub cardinality: bool,
}

impl Requirement {
    /// Derive what a group of constraints needs: the strongest hardness (any hard term ⇒ Hard, any
    /// soft term ⇒ at least Soft, a mix ⇒ Both), the widest locality any predicate implies, and
    /// whether any predicate is a cardinality count.
    pub fn of(constraints: &[Constraint]) -> Self {
        let mut has_hard = false;
        let mut has_soft = false;
        let mut locality = Locality::Local;
        let mut cardinality = false;

        for c in constraints {
            match c.modality {
                Modality::Hard => has_hard = true,
                Modality::Soft(_) => has_soft = true,
            }
            locality = locality.max(predicate_locality(&c.predicate));
            if let Predicate::Count { .. } = c.predicate {
                cardinality = true;
            }
        }

        let hardness = match (has_hard, has_soft) {
            (true, true) => Hardness::Both,
            (false, true) => Hardness::Soft,
            // Pure-hard, or an empty group (nothing to violate) — treat as hard.
            _ => Hardness::Hard,
        };
        Requirement {
            hardness,
            locality,
            cardinality,
        }
    }
}

/// The reach a single predicate implies.
fn predicate_locality(p: &Predicate) -> Locality {
    match p {
        // Purely local: satisfiable looking only at a cell and its immediate neighbours.
        Predicate::Clearance(_) | Predicate::AgainstWall => Locality::Local,
        // Relational: couples two objects across the region.
        Predicate::Facing(_) | Predicate::MinDistance(_) | Predicate::Aligned(_) => Locality::Relational,
        // Global: reasons over the whole region at once.
        Predicate::Count { .. } => Locality::Global,
        // Unknown custom predicates are conservatively treated as global (route to the most capable).
        Predicate::Custom(_) => Locality::Global,
    }
}

/// Does a backend's `Capabilities` cover a group's `Requirement`?
fn covers(cap: &Capabilities, req: &Requirement) -> bool {
    let hardness_ok = match (cap.hardness, req.hardness) {
        (Hardness::Both, _) => true,
        (Hardness::Hard, Hardness::Hard) => true,
        (Hardness::Soft, Hardness::Soft) => true,
        _ => false,
    };
    // A backend with wider locality can serve a narrower need (Global ≥ Relational ≥ Local).
    let locality_ok = cap.locality >= req.locality;
    let cardinality_ok = !req.cardinality || cap.cardinality;
    hardness_ok && locality_ok && cardinality_ok
}

/// Registry + router. Each backend handles a disjoint set of candidate `Role`s, so a group routes to
/// exactly the backend that handles its (homogeneous) role — independent of registration order.
#[derive(Default)]
pub struct Orchestrator {
    solvers: Vec<Box<dyn Solver>>,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, solver: Box<dyn Solver>) -> &mut Self {
        self.solvers.push(solver);
        self
    }

    /// The backend that handles `role`, or `None` (caller surfaces `Unsupported`). Roles partition the
    /// backends 1:1, so this is unambiguous regardless of registration order.
    pub fn route(&self, role: &Role) -> Option<&dyn Solver> {
        self.solvers
            .iter()
            .find(|s| s.handles(role))
            .map(|b| b.as_ref())
    }

    /// Route by candidate role → guard the constraint shape → solve. Every problem the furnish pass
    /// builds is role-homogeneous, so the first candidate's role names the backend. A role no backend
    /// handles, or a group whose constraints exceed the routed backend's capabilities, yields
    /// `SolveError::Unsupported` — a loud failure, never a silent empty placement.
    pub fn solve_group(
        &self,
        problem: &PlacementProblem,
        rng: &mut ChaCha8Rng,
    ) -> Result<Outcome, SolveError> {
        let Some(role) = problem.candidates.first().map(|c| &c.role) else {
            // No candidates ⇒ nothing to place. An empty success, not a routing failure.
            return Ok(Outcome::Assignment(Vec::new()));
        };
        let Some(solver) = self.route(role) else {
            return Err(SolveError::Unsupported);
        };
        // Post-route guard: the role picked the backend; ensure it can also satisfy the group's
        // constraint shape (e.g. reject a hard cardinality count landing on the soft freestanding
        // backend) rather than letting the backend silently drop predicates it doesn't model.
        if !covers(&solver.capabilities(), &Requirement::of(&problem.constraints)) {
            return Err(SolveError::Unsupported);
        }
        solver.solve(problem, rng)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::ir::{Candidate, Dof, Host, Opening, PropertyBag, Rect2, Region, Scope};
    use crate::rng::seeded;

    /// A backend that handles one role and advertises a fixed capability profile — enough to exercise
    /// role routing and the post-route constraint guard.
    struct Mock {
        role: Role,
        caps: Capabilities,
        label: &'static str,
    }
    impl Solver for Mock {
        fn name(&self) -> &str {
            self.label
        }
        fn handles(&self, role: &Role) -> bool {
            *role == self.role
        }
        fn capabilities(&self) -> Capabilities {
            self.caps
        }
        fn solve(&self, _: &PlacementProblem, _: &mut ChaCha8Rng) -> Result<Outcome, SolveError> {
            Ok(Outcome::Assignment(Vec::new()))
        }
    }

    fn cap(hardness: Hardness, locality: Locality, cardinality: bool) -> Capabilities {
        Capabilities {
            hardness,
            locality,
            cardinality,
            deterministic: true,
            needs_training_data: false,
        }
    }
    fn mock(role: Role, caps: Capabilities, label: &'static str) -> Box<dyn Solver> {
        Box::new(Mock { role, caps, label })
    }
    fn region_4x4() -> Region {
        Region {
            id: 0,
            rect: Rect2 { min: [0, 0], max: [4, 4] },
            openings: vec![Opening { dir: 0, cell: [2, 0] }],
            adjacency: Vec::new(),
            props: PropertyBag::default(),
        }
    }
    fn candidate(role: Role) -> Candidate {
        Candidate {
            asset: "x".into(),
            role,
            footprint: [1.0, 1.0],
            dof: Dof::default(),
            affordances: Vec::new(),
        }
    }

    #[test]
    fn routes_by_candidate_role() {
        let mut o = Orchestrator::new();
        o.register(mock(Role::Tiled, cap(Hardness::Hard, Locality::Local, false), "wfc"));
        o.register(mock(
            Role::Freestanding,
            cap(Hardness::Soft, Locality::Relational, false),
            "metropolis",
        ));
        assert_eq!(o.route(&Role::Tiled).map(|s| s.name()), Some("wfc"));
        assert_eq!(o.route(&Role::Freestanding).map(|s| s.name()), Some("metropolis"));
    }

    #[test]
    fn routing_is_registration_order_independent() {
        // The old capability routing let ConstraintSolver{Hard,Global} also cover a {Hard,Local} tiled
        // group, so WFC won only by being registered first. Role routing is unambiguous either way.
        let tiled = || mock(Role::Tiled, cap(Hardness::Hard, Locality::Local, false), "wfc");
        let cons = || {
            mock(
                Role::Anchor { host: Host::Opening },
                cap(Hardness::Hard, Locality::Global, true),
                "constraint",
            )
        };
        let mut a = Orchestrator::new();
        a.register(tiled());
        a.register(cons());
        let mut b = Orchestrator::new();
        b.register(cons());
        b.register(tiled());
        assert_eq!(a.route(&Role::Tiled).map(|s| s.name()), Some("wfc"));
        assert_eq!(b.route(&Role::Tiled).map(|s| s.name()), Some("wfc"));
    }

    #[test]
    fn unhandled_role_has_no_route() {
        let mut o = Orchestrator::new();
        o.register(mock(Role::Tiled, cap(Hardness::Hard, Locality::Local, false), "wfc"));
        assert!(o.route(&Role::Freestanding).is_none());
    }

    #[test]
    fn empty_candidates_is_empty_success() {
        let region = region_4x4();
        let o = Orchestrator::new();
        let problem = PlacementProblem {
            region: &region,
            candidates: Vec::new().into(),
            constraints: Vec::new(),
        };
        assert!(matches!(
            o.solve_group(&problem, &mut seeded(1)),
            Ok(Outcome::Assignment(ref p)) if p.is_empty()
        ));
    }

    #[test]
    fn post_route_guard_rejects_constraint_mismatch() {
        // Role routes this Freestanding group to the soft backend, but it also carries a hard
        // cardinality count the soft backend can't model — solve_group must fail loud, not silently
        // hand it to a backend that ignores the count.
        let region = region_4x4();
        let mut o = Orchestrator::new();
        o.register(mock(
            Role::Freestanding,
            cap(Hardness::Soft, Locality::Relational, false),
            "metropolis",
        ));
        let problem = PlacementProblem {
            region: &region,
            candidates: vec![candidate(Role::Freestanding)].into(),
            constraints: vec![Constraint {
                id: 0,
                scope: Scope::Region,
                predicate: Predicate::Count { tag: "door".into(), count: 1 },
                modality: Modality::Hard,
                guard: None,
            }],
        };
        assert!(matches!(
            o.solve_group(&problem, &mut seeded(1)),
            Err(SolveError::Unsupported)
        ));
    }

    #[test]
    fn role_routed_group_with_matching_constraints_solves() {
        let region = region_4x4();
        let mut o = Orchestrator::new();
        o.register(mock(
            Role::Freestanding,
            cap(Hardness::Soft, Locality::Relational, false),
            "metropolis",
        ));
        let problem = PlacementProblem {
            region: &region,
            candidates: vec![candidate(Role::Freestanding)].into(),
            constraints: vec![Constraint {
                id: 0,
                scope: Scope::Object(0),
                predicate: Predicate::AgainstWall,
                modality: Modality::Soft(1.0),
                guard: None,
            }],
        };
        assert!(matches!(o.solve_group(&problem, &mut seeded(1)), Ok(Outcome::Assignment(_))));
    }

    #[test]
    fn requirement_of_mixed_hard_and_soft_is_both() {
        // Requirement::of still drives the post-route guard, so keep its Both-derivation covered.
        let req = Requirement::of(&[
            Constraint { id: 0, scope: Scope::Region, predicate: Predicate::AgainstWall, modality: Modality::Hard, guard: None },
            Constraint { id: 1, scope: Scope::Region, predicate: Predicate::MinDistance(1.0), modality: Modality::Soft(1.0), guard: None },
        ]);
        assert!(matches!(req.hardness, Hardness::Both));
    }
}
