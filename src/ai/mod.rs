//! Emergent-AI systemic layer — a **layered hybrid** so complex behaviour emerges from interacting
//! systems rather than top-down scripts:
//!   1. **Movement** — context steering (Fray/Jack, "Context Steering", Game AI Pro 2 Ch.18): small,
//!      stateless, decoupled behaviours write interest/danger maps that merge cleanly.
//!   2. **Decision** — utility (Dill, "Dual-Utility Reasoning", Game AI Pro 2 Ch.3): score competing
//!      behaviours at runtime over the creature's **drives**; rank buckets + weighted-random.
//!   3. **Coordination substrate** — stigmergy (Holland & Melhuish 1999; Tang 2021 ACO) via shared
//!      influence fields (Lewis Ch.29; Mark Ch.30), the layer that actually produces emergent stories.
//!
//! Everything is a small composable unit over shared fields (the modularity principle of Colledanchise
//! & Ögren, "Behavior Trees: An Introduction", 2017): channels, drives, steering behaviours, and
//! decision behaviours are all data literals extended by adding one const/registry entry. Numeric
//! knobs live in the `ai_tuning:` slice of `assets/config/config.ron`; structure lives in code.

use bevy::prelude::*;

use crate::dungeon::Dungeon;

pub mod brain;
pub mod diag;
pub mod drives;
pub mod field;
pub mod tuning;
pub mod utility;

use brain::{FieldHotspots, ScentNav};
use drives::{DriveDef, DriveId, DriveRegistry, DriveRule};
use field::{FieldId, RallyDeposits, RallyField, Stig, StigDeposits};
use tuning::AiTuning;

/// Ordering of the AI pipeline within `Update`, so downstream creature decision systems (in other
/// plugins) can `.after(AiSet::Think)`. Runs: deposits → field update → drives → think.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiSet {
    /// Drain queued field writes into the grid.
    Deposits,
    /// Evaporate + diffuse the fields.
    FieldUpdate,
    /// Update per-agent drives (needs).
    Drives,
    /// Run utility decisions, choosing each agent's active behaviour.
    Think,
}

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback. The `ai_tuning:` slice comes from the unified
        // `assets/config/config.ron`, loaded + validated once by `ConfigPlugin` (registered first).
        let tuning = app.world().resource::<crate::config::GameConfig>().ai_tuning;
        app.insert_resource(tuning)
            .init_resource::<StigDeposits>()
            .init_resource::<RallyDeposits>()
            .init_resource::<FieldHotspots>()
            .init_resource::<ScentNav>()
            // The AI pipeline is PINNED simulation: it runs on `FixedUpdate` so it advances by a fixed
            // timestep independent of frame rate (the repeatability precondition — the harness and the
            // live game then share one fixed sub-step). Every creature-decision/movement system in the
            // other plugins that orders against these sets is likewise on `FixedUpdate`.
            .configure_sets(
                FixedUpdate,
                (
                    AiSet::Deposits,
                    AiSet::FieldUpdate,
                    AiSet::Drives,
                    AiSet::Think,
                )
                    .chain(),
            )
            // Tuning is already inserted (from GameConfig) above; allocate the fields + build the drive
            // registry + brains from it.
            .add_systems(
                Startup,
                (init_fields, init_drives, brain::init_brains).chain(),
            )
            // Pinned AI simulation on `FixedUpdate`.
            .add_systems(
                FixedUpdate,
                (
                    field::drain_deposits.in_set(AiSet::Deposits),
                    field::drain_rally_deposits.in_set(AiSet::Deposits),
                    field::evaporate_diffuse.in_set(AiSet::FieldUpdate),
                    field::evaporate_rally.in_set(AiSet::FieldUpdate),
                    brain::update_hotspots.in_set(AiSet::FieldUpdate),
                    brain::rebuild_scent_nav
                        .in_set(AiSet::FieldUpdate)
                        .after(brain::update_hotspots),
                    drives::update_drives.in_set(AiSet::Drives),
                    // `think` reads the LOS grid (`seen_by_squad`), so it must run after `update_los`
                    // writes it this tick (see `fog::LosWritten`), not race it in the multithreaded build.
                    brain::think.in_set(AiSet::Think).after(crate::fog::LosWritten),
                ),
            )
            // Diagnostics are cosmetic logging — they read the fields but never feed the pinned hash, so
            // they stay on `Update` (variable dt is fine).
            .add_systems(
                Update,
                (
                    diag::log_fields,
                    diag::log_drives,
                    diag::log_boss,
                    diag::log_crab_modes,
                    diag::log_crew,
                ),
            );
    }
}

/// Allocate the stigmergy grids sized to the dungeon, with per-channel behaviour from tuning, plus the
/// vectorial rally pheromone map (Tang et al. 2019) with its own decay/accumulate tuning.
fn init_fields(mut commands: Commands, dungeon: Res<Dungeon>, tuning: Res<AiTuning>) {
    commands.insert_resource(Stig::new(&dungeon, tuning.fields.channel_defs()));
    commands.insert_resource(RallyField::new(&dungeon, tuning.rally.into()));
}

/// Build the active drive set. **This is the drive extension point** — add a `DriveDef` literal here
/// (numeric knobs will migrate to the `ai_tuning:` config slice). Every agent with a `Drives` component gets these.
fn init_drives(mut commands: Commands) {
    commands.insert_resource(DriveRegistry {
        defs: vec![
            // Hunger rises steadily → pushes foraging/feeding.
            DriveDef {
                id: DriveId::HUNGER,
                rule: DriveRule::RiseOverTime { rate: 0.03 },
            },
            // Fear tracks the THREAT field (gunfire, boss aura) → pushes flight.
            DriveDef {
                id: DriveId::FEAR,
                rule: DriveRule::TrackField {
                    field: FieldId::THREAT,
                    gain: 0.2,
                },
            },
        ],
    });
}
