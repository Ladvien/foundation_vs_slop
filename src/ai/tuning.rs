//! Hot-tunable numeric knobs for the AI layer, loaded from `ai_tuning.ron` at startup — the exact
//! pattern `vhs.rs` uses (`load_config`): read the file if present, swap the resource on success,
//! `warn!` and keep defaults on failure, never write it back (one path, no fallback file).
//!
//! Structure lives in code (behaviours, drives, channels are type-safe Rust); only the *numbers* live
//! here, so a designer can retune emergence — evaporation rates, curve steepness, drive gains — and
//! relaunch, without recompiling. Sections are added as later phases need them.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::field::{ChannelDef, CHANNEL_COUNT, FieldId};

const TUNING_PATH: &str = "ai_tuning.ron";

/// Per-channel tuning (mirrors [`ChannelDef`]).
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct ChannelTuning {
    pub evaporate: f32,
    pub diffuse: f32,
    pub deposit_radius: f32,
}

impl From<ChannelTuning> for ChannelDef {
    fn from(t: ChannelTuning) -> Self {
        ChannelDef {
            evaporate: t.evaporate,
            diffuse: t.diffuse,
            deposit_radius: t.deposit_radius,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct FieldsTuning {
    pub scent: ChannelTuning,
    pub threat: ChannelTuning,
    pub crab_density: ChannelTuning,
    pub meat: ChannelTuning,
}

impl FieldsTuning {
    /// Assemble the per-channel defs in [`FieldId`] slot order.
    pub fn channel_defs(&self) -> [ChannelDef; CHANNEL_COUNT] {
        let mut defs = [ChannelDef::default(); CHANNEL_COUNT];
        defs[FieldId::SCENT.0] = self.scent.into();
        defs[FieldId::THREAT.0] = self.threat.into();
        defs[FieldId::CRAB_DENSITY.0] = self.crab_density.into();
        defs[FieldId::MEAT.0] = self.meat.into();
        defs
    }
}

/// Root tuning resource. Extend with new sections (`drives`, `steer`, `think`) in later phases.
#[derive(Resource, Clone, Copy, Serialize, Deserialize)]
pub struct AiTuning {
    pub fields: FieldsTuning,
}

impl Default for AiTuning {
    fn default() -> Self {
        Self {
            fields: FieldsTuning {
                // Scent lingers and spreads (a trail); threat is sharper and fades fast; density is local.
                scent: ChannelTuning {
                    evaporate: 0.25,
                    diffuse: 0.15,
                    deposit_radius: 1.5,
                },
                threat: ChannelTuning {
                    evaporate: 0.6,
                    diffuse: 0.1,
                    deposit_radius: 2.0,
                },
                crab_density: ChannelTuning {
                    evaporate: 0.4,
                    diffuse: 0.05,
                    deposit_radius: 1.0,
                },
                // Meat lingers and spreads a bit so wandering crabs sense a distant pile.
                meat: ChannelTuning {
                    evaporate: 0.3,
                    diffuse: 0.12,
                    deposit_radius: 2.0,
                },
            },
        }
    }
}

/// Load `ai_tuning.ron` if present; otherwise keep defaults. Mirrors `vhs::load_config`.
pub fn load_tuning(mut tuning: ResMut<AiTuning>) {
    let Ok(text) = std::fs::read_to_string(TUNING_PATH) else {
        return; // no file → keep defaults (one path, nothing written)
    };
    match ron::from_str::<AiTuning>(&text) {
        Ok(loaded) => {
            info!("ai: loaded {TUNING_PATH}");
            *tuning = loaded;
        }
        Err(e) => warn!("ai: failed to parse {TUNING_PATH}, keeping defaults: {e}"),
    }
}
