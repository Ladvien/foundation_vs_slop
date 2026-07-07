//! Placement grammar — the extensible furniture/prop placement system.
//!
//! Architecture (see `slop/research/2026-07-05-placement-grammar-implementation.md`):
//! a grammar compiles to an engine-free [`ir::PlacementProblem`]; an [`solver::Orchestrator`] routes
//! each constraint group to a pluggable [`solver::Solver`] backend; the [`furnish`] Bevy pass consumes
//! the resulting [`ir::Outcome`] and spawns entities. Stages land incrementally:
//!   - Stage 0: the IR + `Solver` trait, `Region`s carried on `Dungeon`.
//!   - Stage 1: the first backend ([`solvers::wfc::WfcSolver`], Hard + Local) + the orchestrator.
//!   - Stage 2 (here): the affordance [`manifest`] + FBX→GLB assets; a deterministic anchor pass
//!     (ceiling lights, doors) plus the WFC-routed tiled scatter now spawn real GLB furniture.
//!   - Stage 3+: the Metropolis solver arranges `Freestanding` furniture; more backends follow.
//!
//! Determinism (§4): one seeded `ChaCha8Rng` stream split into per-region sub-streams (via
//! [`splitmix64`]) so regions solve independently and reproducibly regardless of ECS/thread order.

#[cfg(test)]
mod acceptance;
pub mod furnish;
pub mod ir;
pub mod manifest;
pub mod solver;
pub mod solvers;

use bevy::prelude::*;

use solver::Orchestrator;
use solvers::constraint::ConstraintSolver;
use solvers::metropolis::{MetropolisSolver, MetropolisWeights};
use solvers::wfc::WfcSolver;

/// Base seed for placement RNG. Per-region sub-seeds derive from `PLACEMENT_SEED ^ splitmix64(id)`.
const PLACEMENT_SEED: u64 = 0x0050_1ACE;

/// Mix an integer into a 64-bit seed (SplitMix64 finalizer, Steele et al. 2014). Used to derive a
/// per-region sub-seed so each region gets an independent, reproducible RNG stream that does not
/// depend on iteration or thread order.
pub fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The registered solver backends, as a Bevy resource. `Orchestrator` itself is engine-free; this
/// newtype gives it a home in the ECS world so the furnish pass (and later stages) route through it.
#[derive(Resource)]
pub struct PlacementSolvers(pub Orchestrator);

/// Build the backend registry. Registration order encodes preference (first cover wins). The three
/// backends have disjoint capability profiles, so a constraint group routes to exactly the right one:
/// WFC = Hard+Local (tiled scatter), Metropolis = Soft+Relational (freestanding layout),
/// ConstraintSolver = Hard+Global+Cardinality (counts / global rules like one-door-per-room).
fn build_solvers(metropolis_weights: MetropolisWeights) -> Orchestrator {
    let mut orch = Orchestrator::new();
    orch.register(Box::new(WfcSolver));
    orch.register(Box::new(MetropolisSolver::new(metropolis_weights)));
    orch.register(Box::new(ConstraintSolver));
    orch
}

/// Tags a placed furniture entity with the region it belongs to — read by `furnish::furniture_room_visibility`
/// to show furniture only in the room the squad currently occupies.
#[derive(Component)]
pub struct PlacedIn(pub ir::RegionId);

pub struct PlacementPlugin;

impl Plugin for PlacementPlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback. The `placement:` slice (furniture manifest + layout
        // weights) comes from the unified `assets/config/config.ron`, loaded + validated once by
        // `ConfigPlugin` (registered first): a missing/malformed file already failed loudly there.
        let (weights, catalogue) = {
            let cfg = app.world().resource::<crate::config::GameConfig>();
            (cfg.placement.metropolis.clone(), cfg.placement.furniture.clone())
        };
        app.insert_resource(PlacementSolvers(build_solvers(weights)));
        app.insert_resource(furnish::Manifest(catalogue));

        // Runs at Startup after `DungeonPlugin` inserts the `Dungeon` resource (in its own `build`).
        app.add_systems(Startup, furnish::furnish_regions);
        // Reveal each room's furniture the first time the squad gains line of sight into it, and keep
        // it revealed thereafter (remembered, per-room — see `furniture_room_visibility`).
        app.init_resource::<furnish::RevealedRooms>();
        app.add_systems(Update, furnish::furniture_room_visibility);
    }
}
