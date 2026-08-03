//! Concrete `Solver` backends. Each is an impl of `super::solver::Solver` consuming the same
//! engine-free IR, so the orchestrator can route to whichever one covers a constraint group's
//! capabilities. Backends land per stage:
//!   - `wfc` (Stage 1): Hard + Local — Wave Function Collapse over a region's tile grid.
//!   - `metropolis` (Stage 3): Soft + Relational — Metropolis–Hastings furniture layout.
//!   - `constraint` (Stage 4): Hard + Global + Cardinality — finite-domain counts / global rules.

pub mod constraint;
pub mod metropolis;
pub mod wfc;
