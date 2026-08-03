//! **The engine-free placement stack** — the constraint problem and the solvers that answer it.
//!
//! Karth & Smith ("WaveFunctionCollapse is Constraint Solving in the Wild", FDG 2017) established
//! that placement *is* finite-domain constraint solving; [`ir`] is that observation made into types,
//! [`solver`] is the orchestrator that routes a constraint group to a backend that can handle it, and
//! [`solvers`] are the backends.
//!
//! The Bevy half of placement — turning a solved [`ir::Placement`] into an entity — is the game's
//! `placement::furnish`, and stays there. `ir.rs` named it as the boundary before this crate existed.

pub mod ir;
pub mod manifest;
pub mod scatter;
pub mod solver;
pub mod solvers;
pub mod surfaces;
