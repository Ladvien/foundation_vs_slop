//! Hot-tunable numeric knobs for the AI layer, deserialized from the `ai_tuning:` slice of the unified
//! `assets/config/config.ron` at startup (loaded + validated once by `crate::config::ConfigPlugin`).
//! Required config — one path, no fallback: a missing or malformed file is a loud failure at the loader.
//!
//! Structure lives in code (behaviours, drives, channels are type-safe Rust); only the *numbers* live
//! here, so a designer can retune emergence — evaporation rates, curve steepness, drive gains — and
//! relaunch, without recompiling. Sections are added as later phases need them.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::field::{ChannelDef, RallyDef, CHANNEL_COUNT, FieldId};

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
    pub alarm: ChannelTuning,
}

impl FieldsTuning {
    /// Assemble the per-channel defs in [`FieldId`] slot order.
    pub fn channel_defs(&self) -> [ChannelDef; CHANNEL_COUNT] {
        let mut defs = [ChannelDef::default(); CHANNEL_COUNT];
        defs[FieldId::SCENT.0] = self.scent.into();
        defs[FieldId::THREAT.0] = self.threat.into();
        defs[FieldId::CRAB_DENSITY.0] = self.crab_density.into();
        defs[FieldId::MEAT.0] = self.meat.into();
        defs[FieldId::ALARM.0] = self.alarm.into();
        defs
    }
}

/// Tuning for the vectorial rally pheromone (mirrors [`RallyDef`]). Not a scalar channel — it has its
/// own decay/accumulate model (Tang et al. 2019), so it lives outside [`FieldsTuning`].
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct RallyTuning {
    pub decay: f32,
    pub accumulate: f32,
    pub deposit_radius: f32,
}

impl From<RallyTuning> for RallyDef {
    fn from(t: RallyTuning) -> Self {
        RallyDef {
            decay: t.decay,
            accumulate: t.accumulate,
            deposit_radius: t.deposit_radius,
        }
    }
}

/// Root tuning resource. Extend with new sections (`drives`, `steer`, `think`) in later phases.
#[derive(Resource, Clone, Copy, Serialize, Deserialize)]
pub struct AiTuning {
    pub fields: FieldsTuning,
    pub rally: RallyTuning,
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
                // Alarm floods ~one room around a wounded crab (large radius, no diffusion so it stays a
                // localized bloom rather than seeping mapwide) and fades over ~2–3 s so the muster is a
                // sharp retaliatory surge, not a permanent aggro. Refreshed by every fresh wound.
                alarm: ChannelTuning {
                    evaporate: 0.5,
                    diffuse: 0.0,
                    deposit_radius: 5.0,
                },
            },
            // Rally vectors decay over a few seconds (call-off), accumulate scout deposits, and smear a
            // couple of cells so the massing swarm reads a smooth bearing toward the prey.
            rally: RallyTuning {
                decay: 0.3,
                accumulate: 0.5,
                deposit_radius: 2.0,
            },
        }
    }
}
