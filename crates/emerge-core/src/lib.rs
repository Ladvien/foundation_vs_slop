//! **emerge-core** — the engine-free half of world building.
//!
//! Constraint IR, solver backends, Wave Function Collapse, Poisson/Delaunay geometry, and the seeded
//! RNG they all draw from. Nothing here imports `bevy`, and nothing here knows what an SCP is.
//!
//! # Why this is a crate and not a module
//!
//! It always was engine-free — `placement/ir.rs` has carried the sentence *"Nothing here imports
//! `bevy::`"* since long before this crate existed, and `wfc.rs`, `geom.rs` and `scatter.rs` each say
//! the same about themselves. But a comment cannot fail a build. Now the manifest can: `emerge-core`
//! depends on `serde`, `ron`, `rand` and `rand_chacha`, so reaching for a `bevy` type or a game
//! concept from in here does not compile.
//!
//! That matters because three consumers need this code and only one of them is the game: the game
//! itself, the offline search (`bin/train`), and the standalone editor described in
//! `docs/2026-08-03-emerge-mapper-plan.md`. The editor is why the split happened now.
//!
//! # The module layout is deliberately unchanged
//!
//! `placement::ir`, `placement::solver`, `placement::solvers`, `placement::scatter` and
//! `placement::manifest` keep the paths they had inside the game, so every `crate::placement::ir::…`
//! inside these files still resolves and the move is reviewable as a pure `git mv`. The game
//! re-exports them at their old paths for the same reason — a workspace split that also rewrites a
//! thousand import lines is a split nobody can diff.
//!
//! # What stayed behind, and why
//!
//! `placement::furnish` is the Bevy boundary of the placement stack (it spawns entities);
//! `placement::mod` binds to the game's run lifecycle; `placement::anomalies` is SCP content. Those
//! are the game's, and `ir.rs:7` already named `furnish.rs` as the boundary before the split.

pub mod adjacency;
pub mod census;
pub mod clips;
pub mod composition;
pub mod constraints;
pub mod convert;
pub mod descriptor;
pub mod gait;
pub mod geom;
pub mod glb;
pub mod grammar;
pub mod grid;
pub mod import;
pub mod library;
pub mod map;
pub mod naming;
pub mod placement;
pub mod plot;
pub mod policy;
pub mod range;
pub mod rig_check;
pub mod rigs;
pub mod rigs_edit;
/// The deterministic RNG, re-exported at its original path.
///
/// It lives in the `det_rng` crate now — lifted out so a permissively-licensed sibling could depend
/// on the same generator without inheriting this crate's GPL. Copying it there instead would have
/// created a SECOND definition of the stream every reproducibility claim here rests on, which is the
/// one outcome that had to be avoided. This alias means no caller moved.
pub use det_rng as rng;
pub mod ron_surgery;
pub mod smart;
pub mod stack;
pub mod vocab;
pub mod wfc;

/// Hermite ease between two edges. Pure math, so it lives here rather than in a crate that has a
/// renderer — `emerge-anim`'s weight blending and the game's own `util` both take it from one place.
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
