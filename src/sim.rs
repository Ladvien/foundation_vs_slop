//! Hot-tunable numeric knobs for the **simulation dynamics** — combat, the swarm economy, field-deposit
//! strengths, fear gains, and the boss — deserialized from the `sim:` slice of the unified
//! `assets/config/config.ron` at startup (loaded + validated once by [`crate::config::ConfigPlugin`]).
//! Required config — one path, no fallback: a missing or malformed slice is a loud failure at the loader.
//!
//! This mirrors [`crate::ai::tuning::AiTuning`] (which owns the field *propagation* knobs — evaporate /
//! diffuse / radius). Together `AiTuning` + `SimTuning` are the full data-driven surface an offline search
//! evolves as a `WorldConfig` (see `squad_ai::world_genome`). Structure stays in code (systems, factions,
//! channels are type-safe Rust); only the *numbers* live here, so a designer — or the search — can retune
//! world dynamics and relaunch without recompiling.
//!
//! Every value in [`SimTuning::default`] is **bit-identical** to the Rust `const` it replaced; a
//! `sim_default_equals_shipped_config` test pins that the RON slice matches the default, and the
//! deterministic-core replay hash pins that promoting these consts changed no gameplay math.

use serde::{Deserialize, Serialize};

/// Fear gains — how strongly a drive tracks an enemy's threat channel. Each threat channel is laid at
/// ≈ its own evaporation rate, so a cell's value tracks the local emitter *count*, and the gain reads as
/// "fear per emitter". `Flee` needs FEAR ≳ 0.28 to clear `MIN_SCORE`, so `per_crab = 0.08` holds the squad
/// against one or two crabs and breaks it under four — a firefight, not a rout. `of_anomaly` is near-total:
/// standing in the watcher's aura is meant to rout the squad. Nothing may fear a channel it emits.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct FearTuning {
    /// A unit's fear per nearby crab (tracks `THREAT_CRAB`).
    pub per_crab: f32,
    /// A unit's fear of the watcher (tracks `THREAT_ANOMALY`).
    pub of_anomaly: f32,
    /// A crab's fear of the squad's gunfire (tracks `THREAT_GUN`).
    pub crab_of_gunfire: f32,
}

/// Stigmergy deposit *strengths* (the amount laid per event/second). The paired evaporate/diffuse/radius
/// for each channel live in [`crate::ai::tuning::AiTuning`]; several channels are designed so
/// `deposit ≈ evaporate` (a cell reads as a "count"), so evolving one without the other shifts semantics —
/// that is why both sides are promoted together.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct DepositTuning {
    /// `THREAT_GUN` laid at muzzle + impact per shot.
    pub threat_per_shot: f32,
    /// Blood scent laid into `SCENT` at a death (crab or boss).
    pub blood_scent: f32,
    /// `CRAB_DENSITY` laid per crab per second (the "reads-as-a-count" rate).
    pub crab_density_rate: f32,
    /// `THREAT_CRAB` (dread) laid per crab per second.
    pub crab_menace_rate: f32,
    /// `MEAT` laid per fruit/carrion source per second.
    pub meat_rate: f32,
    /// `THREAT_ANOMALY` aura laid by the living watcher per second.
    pub anomaly_aura_rate: f32,
    /// `ALARM` flooded around a freshly wounded crab.
    pub alarm_crab: f32,
    /// `ALARM` flooded around a wounded nest.
    pub alarm_nest: f32,
    /// Rally-vector strength a scout deposits toward live prey.
    pub rally_mark: f32,
}

/// Combat numbers — weapon damage, the crab bite, and hit points. `crab_damage_exponent` makes a pile-on
/// super-linear (`dps · count^exp`), so being swarmed is the real threat, not a single bite.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct CombatTuning {
    /// Damage per laser hit.
    pub laser_damage: f32,
    /// Chance a laser that misses its target strikes a unit in the friendly arc.
    pub friendly_fire_chance: f32,
    /// Damage a friendly-fire hit deals.
    pub friendly_fire_damage: f32,
    /// Per-crab contact damage-per-second (the bite base).
    pub crab_contact_dps: f32,
    /// Super-linear exponent on the biting-crab count.
    pub crab_damage_exponent: f32,
    /// Damage a crab's pounce/jump bite deals.
    pub crab_jump_damage: f32,
    /// A crab's hit points.
    pub crab_hp: f32,
    /// A squad unit's hit points.
    pub unit_hp: f32,
    /// Speed drag a unit suffers per crab clinging to it (`speed / (1 + crabs · drag)`).
    pub crab_drag: f32,
}

/// Swarm economy — breeding, feeding, and the population cap. `crab_count_max` is the operative cap the
/// nests breed toward (the initial spawn count is a separate spawn-structure knob, not promoted here).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct BreedingTuning {
    /// Hard population cap the swarm breeds toward.
    pub crab_count_max: usize,
    /// Minimum seconds between a nest's births (breed rate limiter).
    pub respawn_interval: f32,
    /// Meat consumed per birth.
    pub meat_per_crab: f32,
    /// How strongly delivered meat boosts a nest's spawn drive.
    pub feed_gain: f32,
    /// Ceiling on the accumulated spawn boost.
    pub spawn_boost_max: f32,
    /// Per-second decay of the spawn boost.
    pub spawn_boost_decay: f32,
    /// Local `CRAB_DENSITY` above which breeding is suppressed (territorial).
    pub crowd_cap: f32,
    /// Per-second rise of a crab's HUNGER drive (pushes foraging/feeding).
    pub hunger_rate: f32,
    /// Per-second drain of HUNGER while feeding.
    pub hunger_sate_rate: f32,
}

/// The watcher (boss). `start_hp` is the dominant fight-length lever. The `cull_*` knobs govern how it
/// swats biting crabs off itself. The vestigial `CONTACT_DPS` (a death-camera mass weight, never applied
/// as damage) is deliberately **not** promoted — it is not a dynamics knob.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct BossTuning {
    /// The watcher's hit points.
    pub start_hp: f32,
    /// Seconds the watcher recoils/flees after being hit.
    pub scared_time: f32,
    /// Minimum seconds between the watcher's lightning zaps.
    pub zap_cadence: f32,
    /// Biting crabs on the watcher before it swats.
    pub cull_threshold: usize,
    /// Crabs within this radius of the watcher's centre are eaten by a swat.
    pub cull_radius: f32,
    /// Most crabs one swat removes (bounds the swarm hit).
    pub cull_max: usize,
    /// Seconds between swats.
    pub cull_cooldown: f32,
}

/// Root simulation-tuning resource. Extend with new sections as later phases need them; keep
/// [`SimTuning::default`] bit-identical to the shipped consts, guarded by the deterministic-core hash.
#[derive(bevy::prelude::Resource, Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct SimTuning {
    pub fear: FearTuning,
    pub deposit: DepositTuning,
    pub combat: CombatTuning,
    pub breeding: BreedingTuning,
    pub boss: BossTuning,
}

impl Default for SimTuning {
    fn default() -> Self {
        Self {
            fear: FearTuning {
                per_crab: 0.08,
                of_anomaly: 0.9,
                crab_of_gunfire: 0.2,
            },
            deposit: DepositTuning {
                threat_per_shot: 0.6,
                blood_scent: 4.0,
                crab_density_rate: 0.4,
                crab_menace_rate: 0.5,
                meat_rate: 0.5,
                anomaly_aura_rate: 0.4,
                alarm_crab: 2.0,
                alarm_nest: 4.0,
                rally_mark: 4.0,
            },
            combat: CombatTuning {
                laser_damage: 10.0,
                friendly_fire_chance: 0.2,
                friendly_fire_damage: 5.0,
                crab_contact_dps: 3.0,
                crab_damage_exponent: 1.5,
                crab_jump_damage: 8.0,
                crab_hp: 25.0,
                unit_hp: 100.0,
                crab_drag: 0.15,
            },
            breeding: BreedingTuning {
                crab_count_max: 90,
                respawn_interval: 5.0,
                meat_per_crab: 1.0,
                feed_gain: 6.0,
                spawn_boost_max: 9.0,
                spawn_boost_decay: 1.0,
                crowd_cap: 5.0,
                hunger_rate: 0.03,
                hunger_sate_rate: 0.3,
            },
            boss: BossTuning {
                start_hp: 2400.0,
                scared_time: 1.6,
                zap_cadence: 0.35,
                cull_threshold: 4,
                cull_radius: 1.4,
                cull_max: 6,
                cull_cooldown: 2.0,
            },
        }
    }
}

/// Range-check every knob. One path, no fallback: an out-of-range value is a loud `Err` the loader
/// surfaces (`load_game_config`), never a silent clamp. The bounds are physical-plausibility gates that
/// the shipped defaults sit comfortably inside; the offline search's `WorldGenome` bounds table is tighter
/// still (it also caps the *upper* end per knob to keep worlds playable).
pub fn validate_tuning(t: &SimTuning) -> Result<(), String> {
    let positive = |name: &str, v: f32| -> Result<(), String> {
        if v > 0.0 && v.is_finite() {
            Ok(())
        } else {
            Err(format!("sim tuning: {name} must be finite and > 0 (got {v})"))
        }
    };
    let non_negative = |name: &str, v: f32| -> Result<(), String> {
        if v >= 0.0 && v.is_finite() {
            Ok(())
        } else {
            Err(format!("sim tuning: {name} must be finite and >= 0 (got {v})"))
        }
    };
    let probability = |name: &str, v: f32| -> Result<(), String> {
        if (0.0..=1.0).contains(&v) {
            Ok(())
        } else {
            Err(format!("sim tuning: {name} must be a probability in [0,1] (got {v})"))
        }
    };

    // Fear gains are positive multipliers on a threat sample.
    positive("fear.per_crab", t.fear.per_crab)?;
    positive("fear.of_anomaly", t.fear.of_anomaly)?;
    positive("fear.crab_of_gunfire", t.fear.crab_of_gunfire)?;

    // Deposit strengths are positive amounts.
    positive("deposit.threat_per_shot", t.deposit.threat_per_shot)?;
    positive("deposit.blood_scent", t.deposit.blood_scent)?;
    positive("deposit.crab_density_rate", t.deposit.crab_density_rate)?;
    positive("deposit.crab_menace_rate", t.deposit.crab_menace_rate)?;
    positive("deposit.meat_rate", t.deposit.meat_rate)?;
    positive("deposit.anomaly_aura_rate", t.deposit.anomaly_aura_rate)?;
    positive("deposit.alarm_crab", t.deposit.alarm_crab)?;
    positive("deposit.alarm_nest", t.deposit.alarm_nest)?;
    positive("deposit.rally_mark", t.deposit.rally_mark)?;

    // Combat.
    positive("combat.laser_damage", t.combat.laser_damage)?;
    probability("combat.friendly_fire_chance", t.combat.friendly_fire_chance)?;
    non_negative("combat.friendly_fire_damage", t.combat.friendly_fire_damage)?;
    positive("combat.crab_contact_dps", t.combat.crab_contact_dps)?;
    if !(t.combat.crab_damage_exponent >= 1.0 && t.combat.crab_damage_exponent.is_finite()) {
        return Err(format!(
            "sim tuning: combat.crab_damage_exponent must be finite and >= 1 (got {})",
            t.combat.crab_damage_exponent
        ));
    }
    positive("combat.crab_jump_damage", t.combat.crab_jump_damage)?;
    positive("combat.crab_hp", t.combat.crab_hp)?;
    positive("combat.unit_hp", t.combat.unit_hp)?;
    non_negative("combat.crab_drag", t.combat.crab_drag)?;

    // Breeding.
    if t.breeding.crab_count_max == 0 {
        return Err("sim tuning: breeding.crab_count_max must be >= 1".into());
    }
    positive("breeding.respawn_interval", t.breeding.respawn_interval)?;
    positive("breeding.meat_per_crab", t.breeding.meat_per_crab)?;
    positive("breeding.feed_gain", t.breeding.feed_gain)?;
    positive("breeding.spawn_boost_max", t.breeding.spawn_boost_max)?;
    positive("breeding.spawn_boost_decay", t.breeding.spawn_boost_decay)?;
    positive("breeding.crowd_cap", t.breeding.crowd_cap)?;
    positive("breeding.hunger_rate", t.breeding.hunger_rate)?;
    positive("breeding.hunger_sate_rate", t.breeding.hunger_sate_rate)?;

    // Boss.
    positive("boss.start_hp", t.boss.start_hp)?;
    positive("boss.scared_time", t.boss.scared_time)?;
    positive("boss.zap_cadence", t.boss.zap_cadence)?;
    if t.boss.cull_threshold == 0 {
        return Err("sim tuning: boss.cull_threshold must be >= 1".into());
    }
    positive("boss.cull_radius", t.boss.cull_radius)?;
    if t.boss.cull_max == 0 {
        return Err("sim tuning: boss.cull_max must be >= 1".into());
    }
    positive("boss.cull_cooldown", t.boss.cull_cooldown)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_defaults_validate() {
        assert!(validate_tuning(&SimTuning::default()).is_ok());
    }

    #[test]
    fn sim_default_equals_shipped_config() {
        // The byte-identity guard for the const→config migration: the `sim:` slice in the shipped
        // `config.ron` must equal `SimTuning::default()` exactly. A transcription typo in the RON reds this
        // test instead of silently shifting a gameplay value (and the deterministic-core replay hash). We do
        // NOT use `#[serde(default)]` on the slice, precisely so a missing/renamed field is a loud parse
        // error here rather than a silent fallback (one path, no fallback).
        let cfg = crate::config::load_game_config().expect("shipped game config must load");
        assert_eq!(
            cfg.sim,
            SimTuning::default(),
            "assets/config/config.ron `sim:` slice drifted from the shipped SimTuning defaults"
        );
    }

    #[test]
    fn validator_rejects_out_of_range() {
        let mut t = SimTuning::default();
        t.combat.friendly_fire_chance = 1.5;
        assert!(validate_tuning(&t).is_err(), "a >1 probability must be rejected");

        let mut t = SimTuning::default();
        t.combat.crab_damage_exponent = 0.5;
        assert!(validate_tuning(&t).is_err(), "an exponent < 1 must be rejected");

        let mut t = SimTuning::default();
        t.breeding.crab_count_max = 0;
        assert!(validate_tuning(&t).is_err(), "a zero population cap must be rejected");

        let mut t = SimTuning::default();
        t.fear.per_crab = 0.0;
        assert!(validate_tuning(&t).is_err(), "a non-positive fear gain must be rejected");
    }
}
