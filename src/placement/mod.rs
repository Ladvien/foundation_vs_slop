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

// The `_tests.rs` NAME is load-bearing (the `mycelia/fruit_tests.rs` idiom): `tests/panic_budget.rs`
// walks files independently, so this parent-module cfg gate is invisible to it — the filename is
// what tells the scanner the file's panics are test expectations, not shipped crashes.
#[cfg(test)]
#[path = "acceptance_tests.rs"]
mod acceptance;
pub mod anomalies;
pub mod furnish;
// Re-exported from `forge-core` at their old paths — see `lib.rs`. `furnish` and `anomalies` stay
// here because they are the Bevy boundary and the SCP content respectively.
pub use forge_core::placement::{ir, manifest, scatter, solver, solvers};

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
pub(crate) fn build_solvers(metropolis_weights: MetropolisWeights) -> Orchestrator {
    let mut orch = Orchestrator::new();
    orch.register(Box::new(WfcSolver));
    // The wall slab is the game's fact, not the solver's — see `MetropolisSolver::wall_inset`.
    orch.register(Box::new(MetropolisSolver::new(
        metropolis_weights,
        crate::dungeon::WALL_THICKNESS,
    )));
    orch.register(Box::new(ConstraintSolver));
    orch
}

/// Refresh this run's dialled `placement:` knobs from `GameConfig` (`RunBuild::Config`, FVS-H-8).
///
/// `director::pick_next_challenge` writes `gc.placement.metropolis` and `gc.placement.density` from the
/// sampled level cell; both were snapshotted at plugin build and never re-read, so a dialled layout
/// reached nothing. The solver registry is rebuilt through [`build_solvers`] rather than reaching into
/// `MetropolisSolver` — one construction path, so a future backend cannot be wired into the startup
/// registry and forgotten here.
///
/// `Manifest` (the furniture catalogue) is deliberately absent: the director does not dial
/// `placement.furniture`, and refreshing a slice nothing writes would be a path with no writer.
fn resnapshot_placement_config(
    gc: Res<crate::config::GameConfig>,
    mut solvers: ResMut<PlacementSolvers>,
    mut density: ResMut<furnish::Density>,
) {
    *solvers = PlacementSolvers(build_solvers(gc.placement.metropolis.clone()));
    *density = furnish::Density(gc.placement.density);
}

/// Tags a placed furniture entity with the region it belongs to — read by `furnish::furniture_room_visibility`
/// to reveal furniture once the squad has entered its room (and to keep it revealed thereafter).
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
        let density = app
            .world()
            .resource::<crate::config::GameConfig>()
            .placement
            .density;
        app.insert_resource(PlacementSolvers(build_solvers(weights)));
        app.insert_resource(furnish::Manifest(catalogue));
        app.insert_resource(furnish::Density(density));

        // Runs at Startup after `DungeonPlugin` inserts the `Dungeon` resource (in its own `build`).
        app.add_systems(OnEnter(crate::session::RunState::Active), furnish::furnish_regions.in_set(crate::session::RunBuild::Populate));
        // Where every anomaly goes, decided ONCE for the whole level so separation is cross-species.
        // `RunBuild::Grids` is after `World` (the `Dungeon` exists) and before `Populate` (nothing has
        // spawned), so each species' spawner reads the table with no per-spawner ordering edge — see
        // `anomalies` for the corner-clustering bug this replaces.
        app.init_resource::<anomalies::AnomalySites>();
        app.add_systems(
            OnEnter(crate::session::RunState::Active),
            anomalies::build_anomaly_sites.in_set(crate::session::RunBuild::Grids),
        );
        // Refresh the dialled half of the `placement:` slice from `GameConfig` before `furnish_regions`
        // reads it (FVS-H-8). `RunBuild::Config` is chained ahead of `Populate`, so no extra edge.
        app.add_systems(
            OnEnter(crate::session::RunState::Active),
            resnapshot_placement_config.in_set(crate::session::RunBuild::Config),
        );
        // Reveal each room's furniture the first time a squad unit walks into it, and keep it revealed
        // thereafter (remembered, per-room — see `furniture_room_visibility`).
        app.init_resource::<furnish::RevealedRooms>();
        app.add_systems(Update, furnish::furniture_room_visibility.distributive_run_if(in_state(crate::session::RunState::Active)));
    }
}
