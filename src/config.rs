//! Unified game configuration — the single RON file (`assets/config/config.ron`) that holds every
//! data-driven knob in the game, deserialized once into one [`GameConfig`] resource.
//!
//! Before this module each subsystem read its own file (`gore.ron`, `impact_fx.ron`, `ai_tuning.ron`,
//! `assets/dungeon.ron`, `assets/placement/{furniture,metropolis}.ron`, `assets/dialogue/script.dialogue.ron`)
//! with its own load path — and the FX knobs silently fell back to built-in defaults when their file was
//! absent. Both are gone: there is now **one path, no fallback**. [`ConfigPlugin`] (registered first,
//! before any consumer plugin) reads and validates the master file at `build` time and inserts
//! [`GameConfig`]; every downstream plugin pulls its own slice out of that resource in its own `build`,
//! exactly the way `FogPlugin` reads the `Dungeon` resource `DungeonPlugin` inserts (the dialogue graph
//! is the one slice cloned into its own `DialogueScript` resource, since its runtime systems read it
//! directly). A missing or malformed file is a loud panic here at startup, never a silent default world.
//!
//! The one config file that stays standalone is `assets/config/furniture_kenney.ron` — a test-only
//! asset-swap fixture whose entire purpose is proving the furniture kit is swappable by swapping a
//! single file; merging it would defeat that. The acceptance test loads it directly.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ai::tuning::AiTuning;
use crate::dialogue::model::{self, DialogueScript};
use crate::dungeon::{self, DungeonConfig};
use crate::gore::{self, GoreSettings};
use crate::impact_fx::ImpactFxSettings;
use crate::mycelia::{self, MyceliaConfig};
use crate::placement::manifest::{self, FurnitureManifest};
use crate::placement::solvers::metropolis::MetropolisWeights;
use crate::sim::{self, SimTuning};
use crate::vhs::VhsConfig;

/// Path to the unified config file, relative to the project-root working directory (matches the old
/// per-file paths, which were also cwd-relative). Required: a missing file is a loud startup panic.
const GAME_CONFIG_PATH: &str = "assets/config/config.ron";

/// The `placement:` section: the furniture manifest (asset adapter), the Metropolis layout weights,
/// and the density knobs (how much furniture per room). A nested struct so the placement inputs read
/// as one logical group in the RON.
#[derive(Deserialize)]
pub struct PlacementConfig {
    pub furniture: FurnitureManifest,
    pub metropolis: MetropolisWeights,
    pub density: PlacementDensity,
}

/// The furniture *density* knobs — how much furniture a room gets and how it is spaced. Previously
/// hardcoded in `placement::furnish`; promoted to config so the offline level search (`squad_ai::
/// level_genome`) can evolve furniture amount the way `mycelia` already exposes mushroom amount, and so
/// a chosen elite is a readable RON diff. `Copy` so the search can pass it by value into `furnish_all`.
/// (The pure rendering-fit contracts — `FURNITURE_SCALE`, `SURFACE_INSET`, `WALL_LIGHT_HEIGHT` — stay in
/// code: rescaling furniture would push pieces through the 2.4 m ceiling, so they are not "amount" dials.)
#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub struct PlacementDensity {
    /// Cap on tiled decor props (WFC scatter) per room.
    pub tiled_per_room: usize,
    /// Cap on freestanding furniture pieces per room.
    pub freestanding_per_room: usize,
    /// Cap on scatter props rested on support surfaces per room.
    pub scatter_per_room: usize,
    /// Wall lights (sconces) placed per room.
    pub wall_lights_per_room: usize,
    /// Minimum centre-to-centre spacing (metres) between freestanding pieces (a Soft `MinDistance`).
    pub freestanding_min_gap: f32,
    /// Max centre-to-centre distance (metres) a `Near` band pulls same-`group` pieces (toilet + sink).
    pub group_near_max: f32,
}

/// Validate the density knobs. Counts are capped so a runaway search can't request thousands of props
/// per room; the two spacing distances must be finite and positive. One `Err`, no fallback.
pub fn validate_density(d: &PlacementDensity) -> Result<(), String> {
    /// Sane per-room ceiling — well above any authored value, a guard against a degenerate genome.
    const MAX_PER_ROOM: usize = 64;
    for (name, n) in [
        ("tiled_per_room", d.tiled_per_room),
        ("freestanding_per_room", d.freestanding_per_room),
        ("scatter_per_room", d.scatter_per_room),
        ("wall_lights_per_room", d.wall_lights_per_room),
    ] {
        if n > MAX_PER_ROOM {
            return Err(format!("placement.density.{name} = {n} exceeds the {MAX_PER_ROOM} ceiling"));
        }
    }
    if !(d.freestanding_min_gap.is_finite() && d.freestanding_min_gap > 0.0) {
        return Err(format!(
            "placement.density.freestanding_min_gap must be finite and > 0 (got {})",
            d.freestanding_min_gap
        ));
    }
    if !(d.group_near_max.is_finite() && d.group_near_max > 0.0) {
        return Err(format!(
            "placement.density.group_near_max must be finite and > 0 (got {})",
            d.group_near_max
        ));
    }
    Ok(())
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
    pub sim: SimTuning,
    pub vhs: VhsConfig,
    pub mycelia: MyceliaConfig,
    pub dialogue: DialogueScript,
}

/// The evolvable **world-dynamics** surface, as one value: the field-propagation tuning (`ai_tuning`)
/// plus the simulation-dynamics tuning (`sim`). This is the slice-pair the offline search evolves (see
/// `squad_ai::world_genome`) and the harness installs for one rollout (`sim_harness::SimConfig::config`).
/// Both members are `Copy` + `Serialize`, so an evolved world decodes to a readable RON diff — the
/// reward-hacking guard (Skalse et al., "Defining and Characterizing Reward Hacking", arXiv:2209.13085).
#[derive(Clone, Copy)]
pub struct WorldConfig {
    pub ai: AiTuning,
    pub sim: SimTuning,
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
    validate_density(&cfg.placement.density)?;
    gore::validate_settings(&cfg.gore)?;
    mycelia::validate_config(&cfg.mycelia)?;
    // Cross-slice: the mold's damp table must name exactly the room types the dungeon can emit. Neither
    // slice can check this alone, and this is the one place both are in hand. A missing tag would otherwise
    // surface as a runtime error deep in habitat selection, on some seeds only.
    mycelia::validate_damp_coverage(&cfg.mycelia, &cfg.dungeon.room_types)?;
    crate::ai::tuning::validate_tuning(&cfg.ai_tuning)?;
    sim::validate_tuning(&cfg.sim)?;
    model::validate_script(&cfg.dialogue)?;
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
