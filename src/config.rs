//! Unified game configuration — the single RON file (`assets/config/config.ron`) that holds every
//! data-driven knob in the game, deserialized once into one [`GameConfig`] resource.
//!
//! Before this module each subsystem read its own file (`gore.ron`, `impact_fx.ron`, `ai_tuning.ron`,
//! `assets/dungeon.ron`, `assets/placement/{furniture,metropolis}.ron`) with its own load path — and
//! the FX knobs silently fell back to built-in defaults when their file was absent. Both are gone:
//! there is now **one path, no fallback**. [`ConfigPlugin`] (registered first, before any consumer
//! plugin) reads and validates the master file at `build` time and inserts [`GameConfig`]; every
//! downstream plugin pulls its own slice out of that resource in its own `build`, exactly the way
//! `FogPlugin` reads the `Dungeon` resource `DungeonPlugin` inserts. A missing or malformed file is a
//! loud panic here at startup, never a silent default world.
//!
//! The one config file that stays standalone is `assets/config/furniture_kenney.ron` — a test-only
//! asset-swap fixture whose entire purpose is proving the furniture kit is swappable by swapping a
//! single file; merging it would defeat that. The acceptance test loads it directly.

use bevy::prelude::*;
use serde::Deserialize;

use crate::ai::tuning::AiTuning;
use crate::dungeon::{self, DungeonConfig};
use crate::gore::{self, GoreSettings};
use crate::impact_fx::ImpactFxSettings;
use crate::mycelia::{self, MyceliaConfig};
use crate::placement::manifest::{self, FurnitureManifest};
use crate::placement::solvers::metropolis::MetropolisWeights;
use crate::vhs::VhsConfig;

/// Path to the unified config file, relative to the project-root working directory (matches the old
/// per-file paths, which were also cwd-relative). Required: a missing file is a loud startup panic.
const GAME_CONFIG_PATH: &str = "assets/config/config.ron";

/// The `placement:` section: the furniture manifest (asset adapter) plus the Metropolis layout weights.
/// A nested struct so the two placement inputs read as one logical group in the RON.
#[derive(Deserialize)]
pub struct PlacementConfig {
    pub furniture: FurnitureManifest,
    pub metropolis: MetropolisWeights,
}

/// The whole game's data-driven configuration, deserialized from [`GAME_CONFIG_PATH`]. Each field is the
/// exact struct its subsystem already used; the master RON simply nests them under one top-level tuple.
#[derive(Resource, Deserialize)]
pub struct GameConfig {
    pub dungeon: DungeonConfig,
    pub placement: PlacementConfig,
    pub gore: GoreSettings,
    pub impact_fx: ImpactFxSettings,
    pub ai_tuning: AiTuning,
    pub vhs: VhsConfig,
    pub mycelia: MyceliaConfig,
}

/// Read, parse, and validate the unified config. One path: any read, parse, or per-slice validation
/// failure is an `Err` the caller (`ConfigPlugin::build`) surfaces loudly — there is no default config.
/// Validation reuses each subsystem's own validator so the invariants are identical to the pre-merge
/// per-file loads (dungeon generation invariants, the WFC Tiled-prototype cap, the gore autogib range).
pub fn load_game_config() -> Result<GameConfig, String> {
    let text = std::fs::read_to_string(GAME_CONFIG_PATH)
        .map_err(|e| format!("cannot read {GAME_CONFIG_PATH}: {e}"))?;
    let cfg: GameConfig =
        ron::from_str(&text).map_err(|e| format!("{GAME_CONFIG_PATH} parse error: {e}"))?;
    dungeon::validate_config(&cfg.dungeon)?;
    manifest::validate_manifest(&cfg.placement.furniture)?;
    gore::validate_settings(&cfg.gore)?;
    mycelia::validate_config(&cfg.mycelia)?;
    // Cross-slice: the mold's damp table must name exactly the room types the dungeon can emit. Neither
    // slice can check this alone, and this is the one place both are in hand. A missing tag would otherwise
    // surface as a runtime error deep in habitat selection, on some seeds only.
    mycelia::validate_damp_coverage(&cfg.mycelia, &cfg.dungeon.room_types)?;
    Ok(cfg)
}

/// Loads the unified [`GameConfig`] at `build` time and inserts it as a resource. Must be registered
/// **first**, before any plugin that consumes a slice (dungeon, placement, ai, gore, impact_fx, vhs),
/// so the resource exists when those plugins' own `build` methods read it.
pub struct ConfigPlugin;

impl Plugin for ConfigPlugin {
    fn build(&self, app: &mut App) {
        let config = load_game_config().unwrap_or_else(|e| panic!("config: {e}"));
        app.insert_resource(config);
    }
}
