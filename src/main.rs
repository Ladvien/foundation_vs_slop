//! Foundation vs. Slop
//!
//! The SCP Foundation holds the line against "slop" entities — deliberately ugly,
//! uncanny-valley monsters churned out by SCP-9191, a rogue monster-generating AI.
//!
//! This stage is an explorable, WFC-generated dungeon: one Bevy plugin per domain
//! (dungeon, world lighting, camera, fog of war, crab/smiley enemies). The richer "slop"
//! enemy/combat systems are not built yet — they'll be added in a later step.

// Bevy's filtered queries produce unavoidably long tuple types; this lint fights
// idiomatic ECS code, so it's disabled crate-wide (the standard Bevy convention).
#![allow(clippy::type_complexity)]

mod audio;
mod autogib;
mod blood_lens;
mod ai;
mod camera;
mod crab;
#[cfg(debug_assertions)]
mod devshot;
mod dungeon;
mod enemy;
mod flowfield;
mod fog;
mod gore;
mod health;
mod juice;
mod impact_fx;
mod laser;
mod nest;
mod occlusion;
mod orca;
mod placement;
mod rng;
mod selection;
mod squad;
mod surface_nav;
mod util;
mod vhs;
mod wfc;
mod world;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::winit::{UpdateMode, WinitSettings};

/// Gravity for the (gib-only) physics world. Heavier than real 9.81 so chunks fall snappily and
/// settle fast — arcade feel over realism. Only `RigidBody::Dynamic` gib chunks are affected;
/// nothing else in the game is a physics body (see `gore`/`autogib`).
const GIB_GRAVITY: f32 = 18.0;

fn main() {
    let mut app = App::new();
    app
        // Keep rendering at full rate even when the window is unfocused/occluded, so the game
        // stays live in the background (and the `devshot` in-process screenshots aren't black).
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Foundation vs. Slop".into(),
                // Launch borderless-fullscreen on the current monitor (fills the screen at the desktop
                // resolution, no mode switch). `BorderlessFullscreen` over exclusive `Fullscreen` so
                // alt-tab / the in-process `devshot` capture stay well-behaved.
                mode: bevy::window::WindowMode::BorderlessFullscreen(
                    bevy::window::MonitorSelection::Current,
                ),
                ..default()
            }),
            ..default()
        }))
        // avian3d rigid-body physics — deliberately scoped: only gib chunks are dynamic bodies and
        // only the floor + walls are static colliders (see `gore`/`autogib`/`dungeon`). Units,
        // enemies, and lasers keep their own custom movement and never touch the solver.
        .add_plugins(PhysicsPlugins::default())
        .insert_resource(Gravity(Vec3::NEG_Y * GIB_GRAVITY))
        // DungeonPlugin must precede FogPlugin: it inserts the `Dungeon` resource in
        // its `build`, which FogPlugin reads at build time to size the fog grid.
        .add_plugins((
            (dungeon::DungeonPlugin, placement::PlacementPlugin),
            world::WorldPlugin,
            camera::CameraPlugin,
            squad::SquadPlugin,
            selection::SelectionPlugin,
            fog::FogPlugin,
            occlusion::OcclusionPlugin,
            health::HealthPlugin,
            (
                ai::AiPlugin,
                enemy::EnemyPlugin,
                crab::CrabPlugin,
                nest::NestPlugin,
            ),
            laser::LaserPlugin,
            impact_fx::ImpactFxPlugin,
            (juice::JuicePlugin, gore::GorePlugin, autogib::AutogibPlugin),
            audio::GameAudioPlugin,
            (vhs::VhsPlugin, blood_lens::BloodLensPlugin),
        ));

    // devshot is a dev-only in-process screenshot tool — strip it (and its `mod`) from release builds
    // (see CLAUDE.md). Gating both the registration and `mod devshot;` on `debug_assertions` keeps the
    // release binary free of the module and its per-frame `screenshot.request` sentinel polling.
    #[cfg(debug_assertions)]
    app.add_plugins(devshot::DevShotPlugin);

    app.run();
}
