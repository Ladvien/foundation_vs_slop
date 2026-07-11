//! Data-driven acoustic-stimulus + audio tuning, deserialized from the `audio:` slice of the unified
//! `assets/config/config.ron` at startup (loaded + validated once by `crate::config::ConfigPlugin`).
//! Required config — one path, no fallback: a missing or malformed file is a loud failure at the loader.
//!
//! This slice is what makes **sound a perception stimulus** rather than a one-way cosmetic output. The
//! gameplay sites that already emit an [`crate::audio::Sfx`] also deposit into the acoustic stigmergy
//! channels (`ai::field::NOISE_SQUAD` / `NOISE_SWARM`); agents read those channels through the existing
//! fear/attraction machinery. The *numbers* that govern how far a sound carries, how salient each event
//! is, and how strongly each faction reacts live here so the offline audio search (`squad_ai::
//! audio_genome`) can evolve them and a chosen elite is a readable RON diff. The waveform mix in
//! `crate::audio` stays cosmetic and is not tuned here.
//!
//! Channel *propagation* reuses [`ChannelTuning`] (the same evaporate/diffuse/deposit_radius model as
//! every other stigmergy channel); it is composed into the field defs in `ai::init_fields` (slots 7–8),
//! alongside the `ai_tuning.fields` channels (slots 0–6). Keeping the acoustic knobs in their own slice
//! (rather than growing `AiTuning`) lets the audio search evolve them without touching `world_genome`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ai::tuning::ChannelTuning;

/// Propagation + per-event salience of the two acoustic stigmergy channels. `deposit amount` per event
/// is the "loudness" of that sound as a *stimulus* (not its playback gain): a louder event floods a
/// wider, stronger acoustic bloom that more distant creatures can sense.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct AcousticStimulusTuning {
    /// Propagation of `NOISE_SQUAD` — din from squad entities (fire, bolt impacts, unit death).
    pub noise_squad: ChannelTuning,
    /// Propagation of `NOISE_SWARM` — din from crab entities (death squelch).
    pub noise_swarm: ChannelTuning,
    /// Deposit amount for a squad weapon discharge (`Sfx::Fire`).
    pub fire_loudness: f32,
    /// Deposit amount for a bolt striking a wall (`Sfx::ImpactWall`, squad-emitted).
    pub impact_wall_loudness: f32,
    /// Deposit amount for a bolt striking flesh (`Sfx::ImpactFlesh`, squad-emitted).
    pub impact_flesh_loudness: f32,
    /// Deposit amount for a crab death (`Sfx::EnemyDeath`).
    pub enemy_death_loudness: f32,
    /// Deposit amount for a unit death.
    pub unit_death_loudness: f32,
}

/// How strongly each faction reacts to the *other* faction's din. Fear pulls toward `Flee`; the
/// investigate gate/draw pulls toward approaching the din's hotspot. Their relative magnitude decides
/// the emergent **sign** — whether a swarm scatters from a firefight or converges on it.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct AcousticPerceptionTuning {
    /// Repulsion gain: `NOISE_SQUAD` fed into the crab `FEAR` drive (`TrackMaxFields`).
    pub crab_fear_of_din: f32,
    /// Repulsion gain: `NOISE_SWARM` fed into the unit `FEAR` drive (`TrackMaxFields`).
    pub unit_fear_of_din: f32,
    /// Attraction weight for a crab drawn toward the squad's din (reserved; the MVP investigate
    /// behaviour is gated by [`Self::investigate_threshold`] and its behaviour rank).
    pub crab_draw_to_din: f32,
    /// Sensed-din threshold above which a crab's "investigate the din" behaviour activates
    /// (`Fact::NoiseHere` Step gate).
    pub investigate_threshold: f32,
}

/// Root audio-tuning resource — the `audio:` config slice. All-continuous, `Copy`, so an evolved value
/// decodes to a readable RON diff (the reward-hacking guard, mirroring [`crate::config::WorldConfig`]).
#[derive(Resource, Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct AudioTuning {
    pub stimulus: AcousticStimulusTuning,
    pub perception: AcousticPerceptionTuning,
}

impl Default for AudioTuning {
    fn default() -> Self {
        Self {
            stimulus: AcousticStimulusTuning {
                // The squad's din carries: a firefight seeps down a corridor (wide radius, gentle
                // diffusion, moderate fade) so a swarm a room away can sense — and react to — the fight.
                noise_squad: ChannelTuning { evaporate: 0.8, diffuse: 0.18, deposit_radius: 5.0 },
                // The swarm's din is a little tighter and sharper than the squad's.
                noise_swarm: ChannelTuning { evaporate: 0.7, diffuse: 0.15, deposit_radius: 4.0 },
                // Sustained gunfire is the loudest ongoing source; a lone death is a sharp one-off.
                fire_loudness: 0.6,
                impact_wall_loudness: 0.3,
                impact_flesh_loudness: 0.5,
                enemy_death_loudness: 0.7,
                unit_death_loudness: 0.9,
            },
            perception: AcousticPerceptionTuning {
                // All din-response gains ship at 0: the acoustic channels are wired and audible, but nothing
                // REACTS to them at the shipped config, so the sim (and the replay golden) is exactly the
                // creature-only-fear game. The din contribution is ADDITIVE (see `DriveRule::TrackMaxPlusDin`)
                // rather than max-shadowed, so raising these gains gives the offline audio search a real
                // gradient — sound becomes a stimulus the moment the search (or a designer) turns it up.
                crab_fear_of_din: 0.0,
                unit_fear_of_din: 0.0,
                crab_draw_to_din: 0.0,
                investigate_threshold: 0.5,
            },
        }
    }
}

/// Range-check the acoustic knobs. One path, no fallback: an out-of-range value is a loud `Err` the
/// loader (`config::load_game_config`) surfaces, never a silent clamp. Channel bounds mirror
/// `ai::tuning::validate_tuning` (`diffuse` is the load-bearing blur-lerp weight in `[0, 1)`; `evaporate`
/// and `deposit_radius` must be positive). Loudness and perception gains may be zero (a silent event /
/// an indifferent faction is a legitimate point of the search space) but must be finite and non-negative;
/// the investigate threshold gates a Step and must be finite and positive. The offline search's
/// `audio_genome::BOUNDS` is tighter still (it also caps the upper end per knob).
pub fn validate_tuning(t: &AudioTuning) -> Result<(), String> {
    let channel = |name: &str, c: &ChannelTuning| -> Result<(), String> {
        if !(c.evaporate > 0.0 && c.evaporate.is_finite()) {
            return Err(format!(
                "audio.stimulus.{name}.evaporate must be finite and > 0 (got {})",
                c.evaporate
            ));
        }
        if !(0.0..1.0).contains(&c.diffuse) {
            return Err(format!(
                "audio.stimulus.{name}.diffuse must be in [0, 1) — it is a blur lerp weight (got {})",
                c.diffuse
            ));
        }
        if !(c.deposit_radius > 0.0 && c.deposit_radius.is_finite()) {
            return Err(format!(
                "audio.stimulus.{name}.deposit_radius must be finite and > 0 (got {})",
                c.deposit_radius
            ));
        }
        Ok(())
    };
    channel("noise_squad", &t.stimulus.noise_squad)?;
    channel("noise_swarm", &t.stimulus.noise_swarm)?;

    let non_negative = |name: &str, v: f32| -> Result<(), String> {
        if v >= 0.0 && v.is_finite() {
            Ok(())
        } else {
            Err(format!("audio.{name} must be finite and >= 0 (got {v})"))
        }
    };
    non_negative("stimulus.fire_loudness", t.stimulus.fire_loudness)?;
    non_negative("stimulus.impact_wall_loudness", t.stimulus.impact_wall_loudness)?;
    non_negative("stimulus.impact_flesh_loudness", t.stimulus.impact_flesh_loudness)?;
    non_negative("stimulus.enemy_death_loudness", t.stimulus.enemy_death_loudness)?;
    non_negative("stimulus.unit_death_loudness", t.stimulus.unit_death_loudness)?;
    non_negative("perception.crab_fear_of_din", t.perception.crab_fear_of_din)?;
    non_negative("perception.unit_fear_of_din", t.perception.unit_fear_of_din)?;
    non_negative("perception.crab_draw_to_din", t.perception.crab_draw_to_din)?;

    if !(t.perception.investigate_threshold > 0.0 && t.perception.investigate_threshold.is_finite()) {
        return Err(format!(
            "audio.perception.investigate_threshold must be finite and > 0 (got {})",
            t.perception.investigate_threshold
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_valid() {
        validate_tuning(&AudioTuning::default()).expect("shipped default audio tuning must validate");
    }

    #[test]
    fn audio_default_equals_shipped_config() {
        // Byte-identity guard for the const→config migration (mirrors `sim_default_equals_shipped_config`):
        // the `audio:` slice in the shipped `config.ron` must equal `AudioTuning::default()` exactly, so a
        // transcription typo reds this test instead of silently shifting a stimulus value (and, once the
        // acoustic channels land, the deterministic-core replay hash). No `#[serde(default)]` on the slice —
        // a missing/renamed field is a loud parse error, not a silent fallback (one path, no fallback).
        let cfg = crate::config::load_game_config().expect("shipped game config must load");
        assert_eq!(
            cfg.audio,
            AudioTuning::default(),
            "assets/config/config.ron `audio:` slice drifted from the shipped AudioTuning defaults"
        );
    }

    #[test]
    fn rejects_out_of_range_diffuse() {
        let mut t = AudioTuning::default();
        t.stimulus.noise_squad.diffuse = 1.0; // blur lerp weight must be < 1
        assert!(validate_tuning(&t).is_err());
    }

    #[test]
    fn rejects_negative_loudness() {
        let mut t = AudioTuning::default();
        t.stimulus.fire_loudness = -0.1;
        assert!(validate_tuning(&t).is_err());
    }

    #[test]
    fn rejects_nonpositive_investigate_threshold() {
        let mut t = AudioTuning::default();
        t.perception.investigate_threshold = 0.0;
        assert!(validate_tuning(&t).is_err());
    }

    #[test]
    fn allows_zero_gains_and_loudness() {
        // A silent event and an indifferent faction are legitimate points of the search space.
        let mut t = AudioTuning::default();
        t.perception.crab_fear_of_din = 0.0;
        t.perception.unit_fear_of_din = 0.0;
        t.stimulus.enemy_death_loudness = 0.0;
        validate_tuning(&t).expect("zero gains/loudness must be allowed");
    }
}
