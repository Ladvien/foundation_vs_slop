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

mod camera;
mod dungeon;
mod fog;
mod occlusion;
mod player;
mod wfc;
mod world;

use bevy::prelude::*;

fn main() {
    App::new()
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
            player::PlayerPlugin,
            fog::FogPlugin,
            occlusion::OcclusionPlugin,
        ))
        .run();
}
