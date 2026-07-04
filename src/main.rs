//! Foundation vs. Slop
//!
//! The SCP Foundation holds the line against "slop" entities — deliberately ugly,
//! uncanny-valley monsters churned out by SCP-9191, a rogue monster-generating AI.
//!
//! This stage is an explorable, WFC-generated dungeon: one Bevy plugin per domain
//! (dungeon, world lighting, camera, player, fog of war). The slop enemy/combat
//! modules are shelved (files kept, not compiled) until they are placed into the
//! dungeon in a later step.

// Bevy's filtered queries produce unavoidably long tuple types; this lint fights
// idiomatic ECS code, so it's disabled crate-wide (the standard Bevy convention).
#![allow(clippy::type_complexity)]

mod audio;
mod camera;
mod devshot;
mod dungeon;
mod enemy;
mod flowfield;
mod fog;
mod health;
mod impact_fx;
mod laser;
mod occlusion;
mod orca;
mod selection;
mod squad;
mod vhs;
mod wfc;
mod world;

use bevy::prelude::*;
use bevy::winit::{UpdateMode, WinitSettings};

fn main() {
    App::new()
        // Keep rendering at full rate even when the window is unfocused/occluded, so the game
        // stays live in the background (and the `devshot` in-process screenshots aren't black).
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Foundation vs. Slop".into(),
                ..default()
            }),
            ..default()
        }))
        // DungeonPlugin must precede FogPlugin: it inserts the `Dungeon` resource in
        // its `build`, which FogPlugin reads at build time to size the fog grid.
        .add_plugins((
            dungeon::DungeonPlugin,
            world::WorldPlugin,
            camera::CameraPlugin,
            squad::SquadPlugin,
            selection::SelectionPlugin,
            fog::FogPlugin,
            occlusion::OcclusionPlugin,
            health::HealthPlugin,
            enemy::EnemyPlugin,
            laser::LaserPlugin,
            impact_fx::ImpactFxPlugin,
            audio::GameAudioPlugin,
            vhs::VhsPlugin,
            devshot::DevShotPlugin,
        ))
        .run();
}
