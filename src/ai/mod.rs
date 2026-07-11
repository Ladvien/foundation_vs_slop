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

use crate::audio_tuning::AudioTuning;
use crate::sim::SimTuning;

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
        // Simulation-dynamics knobs (combat, economy, deposit strengths, fear, boss). Read once here and
        // inserted as a global resource so every FixedUpdate consumer (crab/laser/enemy/nest) and the
        // `init_drives` Startup system reads `Res<SimTuning>` — the same one-path config seam as `AiTuning`.
        let sim = app.world().resource::<crate::config::GameConfig>().sim;
        // The `audio:` slice — the acoustic-stimulus propagation/salience/perception knobs. Read once
        // here (same one-path config seam as `AiTuning`/`SimTuning`) and inserted so `init_fields` can
        // compose the acoustic channel defs and `init_drives` can read the per-faction din-fear gains.
        let audio = app.world().resource::<crate::config::GameConfig>().audio;
        app.insert_resource(tuning)
            .insert_resource(sim)
            .insert_resource(audio)
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
fn init_fields(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    tuning: Res<AiTuning>,
    audio: Res<AudioTuning>,
) {
    // Channels 0..6 come from `ai_tuning.fields`; the acoustic channels (`NOISE_SQUAD`/`NOISE_SWARM`)
    // come from the `audio:` slice, composed here so the audio search evolves their propagation
    // independently of `AiTuning`. `channel_defs()` already sized the array to `CHANNEL_COUNT` with the
    // acoustic slots defaulted; overwrite them with the tuned defs (asserted non-default in tests).
    let mut defs = tuning.fields.channel_defs();
    defs[FieldId::NOISE_SQUAD.0] = audio.stimulus.noise_squad.into();
    defs[FieldId::NOISE_SWARM.0] = audio.stimulus.noise_swarm.into();
    commands.insert_resource(Stig::new(&dungeon, defs));
    commands.insert_resource(RallyField::new(&dungeon, tuning.rally.into()));
}

/// Which threat channels each faction *fears*, one entry per hostile emitter. Identity (which channel)
/// stays in code — a unit fears crabs (`THREAT_CRAB`) and the watcher (`THREAT_ANOMALY`); a crab fears
/// gunfire (`THREAT_GUN`). The *gains* now come from the `sim:` config slice (`SimTuning::fear`), so
/// `init_drives` pairs each channel with its configured gain. Reduced by `max`, so a crab swarm and the
/// watcher don't sum into panic — the scarier of the two wins. Nothing fears a channel it emits (the
/// regression lock in the tests below).
const FOUNDATION_FEAR_CHANNELS: [FieldId; 2] = [FieldId::THREAT_CRAB, FieldId::THREAT_ANOMALY];
const CRAB_FEAR_CHANNELS: [FieldId; 1] = [FieldId::THREAT_GUN];

/// Build the active drive set, **keyed by faction**. This is the drive extension point — add a `DriveDef`
/// literal to the relevant faction. Numeric knobs (fear gains, the hunger rate) come from the `sim:`
/// config slice (`SimTuning`); channel identity stays in code.
///
/// Fear is faction-relative: each side tracks only its *enemies'* threat channels. Nothing fears its own
/// emissions, which is what stops a firing unit from reading its own muzzle deposit back as terror.
fn init_drives(mut commands: Commands, sim: Res<SimTuning>, audio: Res<AudioTuning>) {
    let mut by_faction: [Vec<DriveDef>; FACTION_COUNT] = Default::default();

    // The squad: fear the creatures (menace channels, gains from `sim.fear`) with the audible din of the
    // swarm (`NOISE_SWARM`, gain from the `audio:` slice) ADDED on top. Creature threats `max`-reduce (two
    // mild dangers ≠ panic); the din is additive because the menace field saturates (~24) and would
    // otherwise drown any din term in the max — so the sound of the swarm dying could never register. Din
    // gain ships at 0 (dormant), so this is exactly the old creature-only fear at the shipped config.
    by_faction[Faction::Foundation.index()] = vec![DriveDef {
        id: DriveId::FEAR,
        rule: DriveRule::TrackMaxPlusDin {
            threats: vec![
                (FOUNDATION_FEAR_CHANNELS[0], sim.fear.per_crab),
                (FOUNDATION_FEAR_CHANNELS[1], sim.fear.of_anomaly),
            ],
            din: vec![(FieldId::NOISE_SWARM, audio.perception.unit_fear_of_din)],
        },
    }];

    // The swarm: hunger rises steadily → pushes foraging/feeding; fear tracks the squad's gunfire
    // (`THREAT_GUN`, from `sim.fear`) with the squad's audible din (`NOISE_SQUAD`, from the `audio:` slice)
    // ADDED on top (same reason as the squad: an additive nudge the search can climb, not a max-shadowed
    // term). NOISE_SQUAD is *also* what the swarm may be drawn toward (the investigate behaviour) —
    // whether the sound of a firefight scatters or attracts the swarm is the emergent, RL-tuned sign.
    by_faction[Faction::Crab.index()] = vec![
        DriveDef {
            id: DriveId::HUNGER,
            rule: DriveRule::RiseOverTime { rate: sim.breeding.hunger_rate },
        },
        DriveDef {
            id: DriveId::FEAR,
            rule: DriveRule::TrackMaxPlusDin {
                threats: vec![(CRAB_FEAR_CHANNELS[0], sim.fear.crab_of_gunfire)],
                din: vec![(FieldId::NOISE_SQUAD, audio.perception.crab_fear_of_din)],
            },
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

    /// The acoustic din channels — a distinct category from creature menace (they are NOT in
    /// `UNIT_THREAT_CHANNELS`, so psi-vision ignores them). Used to split the invariants below.
    const ACOUSTIC_CHANNELS: [FieldId; 2] = [FieldId::NOISE_SQUAD, FieldId::NOISE_SWARM];
    fn is_acoustic(f: FieldId) -> bool {
        ACOUSTIC_CHANNELS.contains(&f)
    }

    /// The channels each faction *emits*, creature-menace and acoustic din alike. Kept next to the
    /// assertion it feeds, so a new emitter that is wired up without extending this list makes the test
    /// lie loudly rather than quietly.
    fn emits(faction: Faction) -> &'static [FieldId] {
        match faction {
            // `laser::fire_laser` (muzzle) + `laser::update_lasers` (impact); NOISE_SQUAD at the same
            // sites + `squad`'s unit-death.
            Faction::Foundation => &[FieldId::THREAT_GUN, FieldId::NOISE_SQUAD],
            // `crab::deposit_crab_fields`; NOISE_SWARM at the crab-death site.
            Faction::Crab => &[FieldId::THREAT_CRAB, FieldId::NOISE_SWARM],
            // `enemy::deposit_anomaly_aura`.
            Faction::Anomaly => &[FieldId::THREAT_ANOMALY],
        }
    }

    fn fear_sources(faction: Faction) -> Vec<(FieldId, f32)> {
        // Gains come from the shipped config defaults (the same values `init_drives` reads at runtime);
        // creature-menace gains from `sim.fear`, acoustic-din gains from the `audio:` slice. Channel
        // identity comes from the code-owned channel lists. The acoustic source is listed LAST so the
        // creature-only prefix equals `UNIT_THREAT_CHANNELS`.
        let fear = SimTuning::default().fear;
        let perc = AudioTuning::default().perception;
        match faction {
            Faction::Foundation => vec![
                (FOUNDATION_FEAR_CHANNELS[0], fear.per_crab),
                (FOUNDATION_FEAR_CHANNELS[1], fear.of_anomaly),
                (FieldId::NOISE_SWARM, perc.unit_fear_of_din),
            ],
            Faction::Crab => vec![
                (CRAB_FEAR_CHANNELS[0], fear.crab_of_gunfire),
                (FieldId::NOISE_SQUAD, perc.crab_fear_of_din),
            ],
            Faction::Anomaly => vec![],
        }
    }

    #[test]
    fn no_faction_fears_a_channel_it_emits() {
        // THE regression lock for "the squad flees from its own gunfire". A firing unit deposits
        // THREAT_GUN at its own muzzle; when FEAR tracked that channel, it saturated within a second and
        // `Flee` (the top rank for every role) preempted Overwatch, Ward, TendWounded and the rest. Since
        // firing is not gated on AI mode, the unit kept shooting while fleeing and never recovered.
        for faction in Faction::ALL {
            for &(feared, _) in fear_sources(faction).iter() {
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
        // Creature-menace channels only — acoustic din is a separate category (checked below), and the
        // creature prefix must equal `UNIT_THREAT_CHANNELS` (what psi-vision renders).
        let feared: Vec<FieldId> = fear_sources(Faction::Foundation)
            .iter()
            .map(|&(f, _)| f)
            .filter(|&f| !is_acoustic(f))
            .collect();
        for hostile in [Faction::Crab, Faction::Anomaly] {
            for channel in emits(hostile).iter().filter(|&&f| !is_acoustic(f)) {
                assert!(feared.contains(channel), "units ignore {hostile:?}'s threat channel");
            }
        }
        assert_eq!(feared, UNIT_THREAT_CHANNELS.to_vec());
    }

    #[test]
    fn each_faction_is_wired_to_the_other_factions_din() {
        // The acoustic dual of the creature-channel lock: a din channel nobody reads is a dead channel
        // (and would leave the audio search with a knob that moves nothing). Crabs must be WIRED to the
        // squad's din; units to the swarm's din — present in the fear def, ready to react once the gain is
        // raised (it ships at 0, dormant). `no_faction_fears_a_channel_it_emits` above already proves
        // neither is wired to its OWN din.
        let hears = |faction| -> Vec<FieldId> {
            fear_sources(faction).iter().map(|&(f, _)| f).filter(|&f| is_acoustic(f)).collect()
        };
        assert!(hears(Faction::Crab).contains(&FieldId::NOISE_SQUAD), "crabs ignore the squad's din");
        assert!(hears(Faction::Foundation).contains(&FieldId::NOISE_SWARM), "units ignore the swarm's din");
    }

    #[test]
    fn fear_sources_name_real_channels_with_positive_gains() {
        for faction in Faction::ALL {
            for &(field, gain) in fear_sources(faction).iter() {
                assert!(field.0 < CHANNEL_COUNT, "{faction:?} fears an out-of-range channel slot");
                // Creature-menace gains must be positive (a 0 gain = a monster nobody fears). Acoustic-din
                // gains ship at 0 ON PURPOSE — the channel is wired but dormant, so the shipped sim is the
                // creature-only-fear game and the audio search is what turns the din up. So din is allowed
                // to be 0 but never negative (a negative gain would make din SOOTHE, which the additive
                // `TrackMaxPlusDin` does not model).
                if is_acoustic(field) {
                    assert!(gain >= 0.0, "{faction:?} has a negative din gain");
                } else {
                    assert!(gain > 0.0, "{faction:?} has a non-positive creature-fear gain");
                }
            }
        }
    }

    #[test]
    fn a_lone_crab_does_not_rout_the_squad_but_a_pack_does() {
        // THREAT_CRAB is laid at ~its own evaporation rate, so a cell's value tracks the local crab count.
        // `Flee` needs FEAR >= ~0.28 to clear MIN_SCORE (Logistic{k:10,x0:0.5}), so the squad must hold
        // against one or two crabs and break under four. This pins the *feel*, not just the plumbing.
        let fear = SimTuning::default().fear;
        let fear_from = |crabs: f32| {
            drives::track_max_target([(crabs, fear.per_crab), (0.0, fear.of_anomaly)])
        };
        const FLEE_ONSET: f32 = 0.28;
        assert!(fear_from(1.0) < FLEE_ONSET, "a lone crab must not rout a trained squad");
        assert!(fear_from(2.0) < FLEE_ONSET, "two crabs is a firefight, not a rout");
        assert!(fear_from(4.0) > FLEE_ONSET, "four crabs should break the line");
        // Standing in the watcher's aura is meant to be unsurvivable dread, whatever else is around.
        assert!(drives::track_max_target([(1.0, fear.of_anomaly)]) > FLEE_ONSET);
    }
}
