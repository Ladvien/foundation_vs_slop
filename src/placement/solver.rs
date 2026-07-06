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
    SolveError,
};

/// A placement backend. `solve` takes the region's RNG sub-stream so results are reproducible under a
/// seed regardless of the order regions/threads run in (determinism invariant, §4). It returns a
/// `Result` and degrades to `Outcome::Partial` on trouble — it must never panic.
pub trait Solver: Send + Sync {
    /// Stable, human-readable name (diagnostics + the backend-swap acceptance test).
    fn name(&self) -> &str;
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

/// Registry + router. Backends are tried in registration order; the first that covers a group wins,
/// so registration order encodes preference (specific/cheap before general/expensive).
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

    /// The first registered backend that covers `req`, or `None` (caller surfaces `Unsupported`).
    pub fn route(&self, req: &Requirement) -> Option<&dyn Solver> {
        self.solvers
            .iter()
            .find(|s| covers(&s.capabilities(), req))
            .map(|b| b.as_ref())
    }

    /// Compile-group → route → solve. Groups whose capabilities no backend covers yield
    /// `SolveError::Unsupported` rather than a silent skip.
    pub fn solve_group(
        &self,
        problem: &PlacementProblem,
        rng: &mut ChaCha8Rng,
    ) -> Result<Outcome, SolveError> {
        let req = Requirement::of(&problem.constraints);
        match self.route(&req) {
            Some(solver) => solver.solve(problem, rng),
            None => Err(SolveError::Unsupported),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::ir::{Constraint, Modality, Predicate, Scope};

    /// A backend that advertises a fixed capability profile — enough to exercise routing.
    struct Mock(Capabilities, &'static str);
    impl Solver for Mock {
        fn name(&self) -> &str {
            self.1
        }
        fn capabilities(&self) -> Capabilities {
            self.0
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
    fn con(id: u32, predicate: Predicate, modality: Modality) -> Constraint {
        Constraint {
            id,
            scope: Scope::Region,
            predicate,
            modality,
            guard: None,
        }
    }

    #[test]
    fn local_hard_group_routes_to_local_hard_backend() {
        let mut o = Orchestrator::new();
        o.register(Box::new(Mock(cap(Hardness::Hard, Locality::Local, false), "wfc")));
        let req = Requirement::of(&[con(0, Predicate::AgainstWall, Modality::Hard)]);
        assert_eq!(o.route(&req).map(|s| s.name()), Some("wfc"));
    }

    #[test]
    fn cardinality_group_needs_a_cardinality_backend() {
        let mut o = Orchestrator::new();
        o.register(Box::new(Mock(cap(Hardness::Hard, Locality::Local, false), "wfc")));
        let req = Requirement::of(&[con(
            0,
            Predicate::Count {
                tag: "door".into(),
                count: 1,
            },
            Modality::Hard,
        )]);
        // The local WFC backend cannot cover a global cardinality count.
        assert!(o.route(&req).is_none());
        o.register(Box::new(Mock(
            cap(Hardness::Hard, Locality::Global, true),
            "constraint",
        )));
        assert_eq!(o.route(&req).map(|s| s.name()), Some("constraint"));
    }

    #[test]
    fn relational_soft_group_is_not_covered_by_local_hard() {
        let mut o = Orchestrator::new();
        o.register(Box::new(Mock(cap(Hardness::Hard, Locality::Local, false), "wfc")));
        let req = Requirement::of(&[con(0, Predicate::MinDistance(1.0), Modality::Soft(1.0))]);
        assert!(o.route(&req).is_none());
        // A soft/relational backend (Metropolis, Stage 3) covers it.
        o.register(Box::new(Mock(
            cap(Hardness::Soft, Locality::Relational, false),
            "metropolis",
        )));
        assert_eq!(o.route(&req).map(|s| s.name()), Some("metropolis"));
    }

    #[test]
    fn mixed_hard_and_soft_needs_a_both_backend() {
        let req = Requirement::of(&[
            con(0, Predicate::AgainstWall, Modality::Hard),
            con(1, Predicate::MinDistance(1.0), Modality::Soft(1.0)),
        ]);
        assert!(matches!(req.hardness, Hardness::Both));
        let mut o = Orchestrator::new();
        o.register(Box::new(Mock(cap(Hardness::Hard, Locality::Relational, false), "hard-only")));
        assert!(o.route(&req).is_none());
        o.register(Box::new(Mock(cap(Hardness::Both, Locality::Relational, false), "both")));
        assert_eq!(o.route(&req).map(|s| s.name()), Some("both"));
    }
}
