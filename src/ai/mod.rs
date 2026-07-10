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
pub mod faction;
pub mod field;
pub mod tuning;
pub mod utility;

use brain::{FieldHotspots, ScentNav};
use drives::{DriveDef, DriveId, DriveRegistry, DriveRule};
use faction::{Faction, FACTION_COUNT};
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
        // The brain data source. `Default` is `Authored`, so the shipped game needs no setup; the
        // headless harness inserts a `Candidate` BEFORE adding this plugin, and `init_resource` then
        // leaves it alone. One path: the resource always exists and `init_brains` matches exhaustively.
        app.init_resource::<brain::BrainSource>();
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
            // `validate_factions` runs last, after every `Startup` spawner has tagged its agents, so an
            // untagged `Drives` carrier fails the launch instead of silently never feeling fear.
            .add_systems(
                Startup,
                (init_fields, init_drives, brain::init_brains).chain(),
            )
            .add_systems(PostStartup, faction::validate_factions)
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

/// How strongly a unit fears one nearby crab. The THREAT_CRAB channel is laid at ≈ its own evaporation
/// rate, so its value at a cell tracks the local crab *count*: this gain therefore reads as "fear per
/// crab". `Flee` needs FEAR ≳ 0.28 to clear `MIN_SCORE`, so a squad holds its ground against one or two
/// crabs and breaks under four or more — a firefight, not a rout.
const FEAR_PER_CRAB: f32 = 0.08;

/// How strongly a unit fears the watcher. Near-total: standing in the aura is meant to rout the squad,
/// which is the whole point of the boss. The channel peaks near 1.0 at the boss's own cell.
const FEAR_OF_ANOMALY: f32 = 0.9;

/// How strongly a crab fears gunfire (unchanged from the original single-channel model).
const CRAB_FEAR_OF_GUNFIRE: f32 = 0.2;

/// A unit's fear sources: one entry per hostile creature type. Reduced by `max`, so a crab swarm and the
/// watcher don't sum into panic — the scarier of the two wins.
const FOUNDATION_FEAR: [(FieldId, f32); 2] = [
    (FieldId::THREAT_CRAB, FEAR_PER_CRAB),
    (FieldId::THREAT_ANOMALY, FEAR_OF_ANOMALY),
];

/// A crab's fear sources: the squad's weapons, and nothing else.
const CRAB_FEAR: [(FieldId, f32); 1] = [(FieldId::THREAT_GUN, CRAB_FEAR_OF_GUNFIRE)];

/// Build the active drive set, **keyed by faction**. This is the drive extension point — add a `DriveDef`
/// literal to the relevant faction (numeric knobs will migrate to the `ai_tuning:` config slice).
///
/// Fear is faction-relative: each side tracks only its *enemies'* threat channels. Nothing fears its own
/// emissions, which is what stops a firing unit from reading its own muzzle deposit back as terror.
fn init_drives(mut commands: Commands) {
    let mut by_faction: [Vec<DriveDef>; FACTION_COUNT] = Default::default();

    // The squad: fear the creatures. No HUNGER rule — units don't forage, and no role behaviour reads the
    // drive, so the old global rule was ramping a number nobody consulted.
    by_faction[Faction::Foundation.index()] = vec![DriveDef {
        id: DriveId::FEAR,
        rule: DriveRule::TrackMaxFields { sources: &FOUNDATION_FEAR },
    }];

    // The swarm: hunger rises steadily → pushes foraging/feeding; fear tracks the squad's gunfire.
    by_faction[Faction::Crab.index()] = vec![
        DriveDef {
            id: DriveId::HUNGER,
            rule: DriveRule::RiseOverTime { rate: 0.03 },
        },
        DriveDef {
            id: DriveId::FEAR,
            rule: DriveRule::TrackMaxFields { sources: &CRAB_FEAR },
        },
    ];

    // The watcher: afraid of nothing, hungry for nothing. Its brain (`smiley_brain`) reads no drive — it
    // is steered by distance, line-of-sight, and the SCENT field.
    by_faction[Faction::Anomaly.index()] = Vec::new();

    commands.insert_resource(DriveRegistry { by_faction });
}

#[cfg(test)]
mod tests {
    // Pure registry-shape checks — no App, no ECS.
    use super::*;
    use crate::ai::field::{CHANNEL_COUNT, UNIT_THREAT_CHANNELS};

    /// The channels each faction *emits*. Kept next to the assertion it feeds, so a new emitter that is
    /// wired up without extending this list makes the test lie loudly rather than quietly.
    fn emits(faction: Faction) -> &'static [FieldId] {
        match faction {
            // `laser::fire_laser` (muzzle) + `laser::update_lasers` (impact).
            Faction::Foundation => &[FieldId::THREAT_GUN],
            // `crab::deposit_crab_fields`.
            Faction::Crab => &[FieldId::THREAT_CRAB],
            // `enemy::deposit_anomaly_aura`.
            Faction::Anomaly => &[FieldId::THREAT_ANOMALY],
        }
    }

    fn fear_sources(faction: Faction) -> &'static [(FieldId, f32)] {
        match faction {
            Faction::Foundation => &FOUNDATION_FEAR,
            Faction::Crab => &CRAB_FEAR,
            Faction::Anomaly => &[],
        }
    }

    #[test]
    fn no_faction_fears_a_channel_it_emits() {
        // THE regression lock for "the squad flees from its own gunfire". A firing unit deposits
        // THREAT_GUN at its own muzzle; when FEAR tracked that channel, it saturated within a second and
        // `Flee` (the top rank for every role) preempted Overwatch, Ward, TendWounded and the rest. Since
        // firing is not gated on AI mode, the unit kept shooting while fleeing and never recovered.
        for faction in Faction::ALL {
            for &(feared, _) in fear_sources(faction) {
                assert!(
                    !emits(faction).contains(&feared),
                    "{faction:?} fears a channel it emits — it will flee from itself",
                );
            }
        }
    }

    #[test]
    fn units_fear_every_hostile_creature_channel() {
        // A creature type that emits dread nobody reads is a monster the squad walks past unafraid.
        let feared: Vec<FieldId> = fear_sources(Faction::Foundation).iter().map(|&(f, _)| f).collect();
        for hostile in [Faction::Crab, Faction::Anomaly] {
            for channel in emits(hostile) {
                assert!(feared.contains(channel), "units ignore {hostile:?}'s threat channel");
            }
        }
        assert_eq!(feared, UNIT_THREAT_CHANNELS.to_vec());
    }

    #[test]
    fn fear_sources_name_real_channels_with_positive_gains() {
        for faction in Faction::ALL {
            for &(field, gain) in fear_sources(faction) {
                assert!(gain > 0.0, "{faction:?} has a non-positive fear gain");
                assert!(field.0 < CHANNEL_COUNT, "{faction:?} fears an out-of-range channel slot");
            }
        }
    }

    #[test]
    fn a_lone_crab_does_not_rout_the_squad_but_a_pack_does() {
        // THREAT_CRAB is laid at ~its own evaporation rate, so a cell's value tracks the local crab count.
        // `Flee` needs FEAR >= ~0.28 to clear MIN_SCORE (Logistic{k:10,x0:0.5}), so the squad must hold
        // against one or two crabs and break under four. This pins the *feel*, not just the plumbing.
        let fear_from = |crabs: f32| {
            drives::track_max_target([(crabs, FEAR_PER_CRAB), (0.0, FEAR_OF_ANOMALY)])
        };
        const FLEE_ONSET: f32 = 0.28;
        assert!(fear_from(1.0) < FLEE_ONSET, "a lone crab must not rout a trained squad");
        assert!(fear_from(2.0) < FLEE_ONSET, "two crabs is a firefight, not a rout");
        assert!(fear_from(4.0) > FLEE_ONSET, "four crabs should break the line");
        // Standing in the watcher's aura is meant to be unsurvivable dread, whatever else is around.
        assert!(drives::track_max_target([(1.0, FEAR_OF_ANOMALY)]) > FLEE_ONSET);
    }
}
