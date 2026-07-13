//! Foundation vs. Slop — library crate root.
//!
//! The SCP Foundation holds the line against "slop" entities — deliberately ugly,
//! uncanny-valley monsters churned out by SCP-9191, a rogue monster-generating AI.
//!
//! This stage is an explorable, WFC-generated dungeon: one Bevy plugin per domain
//! (dungeon, world lighting, camera, fog of war, crab/smiley enemies). The richer "slop"
//! enemy/combat systems are not built yet — they'll be added in a later step.
//!
//! The crate is split lib+bin: all domain modules live here (so integration tests under
//! `tests/` and the headless `sim_harness` can reuse them), and `main.rs` is a thin
//! binary that calls [`run`].

// Bevy's filtered queries produce unavoidably long tuple types; this lint fights
// idiomatic ECS code, so it's disabled crate-wide (the standard Bevy convention).
#![allow(clippy::type_complexity)]

pub mod audio;
/// Data-driven acoustic-stimulus + audio tuning — the `audio:` config slice. The propagation/salience
/// of the acoustic stigmergy channels (`ai::field::NOISE_*`) and the per-faction perception gains that
/// turn sound into a stimulus agents react to; evolvable by the offline audio search (`squad_ai::
/// audio_genome`). Mirrors `ai::tuning` / `sim`.
pub mod audio_tuning;
pub mod autogib;
pub mod blood_lens;
pub mod ai;
pub mod ai_overlay;
pub mod camera;
pub mod config;
pub mod crab;
#[cfg(debug_assertions)]
pub mod devshot;
pub mod dialogue;
pub mod dungeon;
pub mod enemy;
pub mod flowfield;
pub mod fog;
pub mod geom;
pub mod gore;
pub mod health;
pub mod juice;
pub mod impact_fx;
pub mod laser;
pub mod light;
pub mod mycelia;
pub mod nest;
pub mod orca;
pub mod parasite;
pub mod pathfind;
pub mod psi_vision;
pub mod placement;
pub mod rng;
pub mod selection;
pub mod settings;
/// Data-driven simulation-dynamics tuning (combat, swarm economy, deposits, fear, boss) — the `sim:`
/// config slice. Mirrors `ai::tuning`; together they form the `WorldConfig` the offline search evolves.
pub mod sim;
/// Headless deterministic replay/liveness harness — opt-in so it never enters the shipped binary.
#[cfg(feature = "test-harness")]
pub mod sim_harness;
/// Perceptual (SSIM) image comparison for FX/render visual-regression — opt-in test infrastructure.
#[cfg(feature = "test-harness")]
pub mod visual_regression;
pub mod squad;
pub mod squad_ai;
pub mod surface_nav;
pub mod time_control;
pub mod ui;
pub mod util;
pub mod vhs;
pub mod wfc;
pub mod world;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::winit::{UpdateMode, WinitSettings};

/// Gravity for the (gib-only) physics world. Heavier than real 9.81 so chunks fall snappily and
/// settle fast — arcade feel over realism. Only `RigidBody::Dynamic` gib chunks are affected;
/// nothing else in the game is a physics body (see `gore`/`autogib`).
const GIB_GRAVITY: f32 = 18.0;

/// Build and run the full windowed game. The headless test harness (`sim_harness`, behind the
/// `test-harness` feature) constructs an equivalent `App` without render/winit/audio so the same
/// gameplay plugins can be driven deterministically off-screen.
pub fn run() {
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
        // Render-only high-refresh smoothing: `PhysicsPlugins` already brings
        // `bevy_transform_interpolation`'s `TransformInterpolationPlugin` (avian uses it for physics
        // interpolation), so we must NOT add it again — Bevy panics on a duplicate unique plugin. Each
        // mover instead opts in per-entity via the `TransformInterpolation` component at its spawn site
        // (units/enemies/crabs/bolts); without it every entity steps at 60 Hz and judders on a 120/144 Hz
        // panel. The plugin eases `Transform` between fixed ticks but restores the authoritative value in
        // `FixedFirst` *before* each tick, so movers that integrate `transform.translation` don't drift.
        // The exact-hash harness runs physics-off (no `PhysicsPlugins`), so interpolation is absent there
        // and the opt-in components stay inert — `snapshot_hash` reads authoritative transforms.
        // ConfigPlugin must precede every consumer: it loads + validates the unified
        // `assets/config/config.ron` and inserts the `GameConfig` resource in its `build`, which the
        // dungeon/placement/ai/gore/impact_fx/vhs plugins each read at build time to pull their slice.
        // DungeonPlugin in turn precedes FogPlugin: it inserts the `Dungeon` resource in its `build`,
        // which FogPlugin reads at build time to size the fog grid.
        .add_plugins((
            config::ConfigPlugin,
            // `LightFieldPlugin` (the CPU illuminance grid creatures read) is grouped with dungeon+placement
            // it depends on, and kept harness-visible — unlike the windowed `LightingPlugin` below — so the
            // determinism gate covers its bake. Nested here (not a 16th top-level element) to stay under
            // Bevy's 15-plugin tuple cap.
            (dungeon::DungeonPlugin, placement::PlacementPlugin, light::LightFieldPlugin),
            world::WorldPlugin,
            camera::CameraPlugin,
            (squad::SquadPlugin, squad_ai::SquadAiPlugin),
            selection::SelectionPlugin,
            fog::FogPlugin,
            health::HealthPlugin,
            (
                ai::AiPlugin,
                enemy::EnemyPlugin,
                crab::CrabPlugin,
                nest::NestPlugin,
                parasite::ParasitePlugin,
            ),
            laser::LaserPlugin,
            impact_fx::ImpactFxPlugin,
            (
                time_control::TimeControlPlugin,
                juice::JuicePlugin,
                gore::GorePlugin,
                autogib::AutogibPlugin,
            ),
            audio::GameAudioPlugin,
            // Cosmetic render/FX. Mycelia (GPU-compute mold ambience) lives here and is registered ONLY
            // here, never in the headless `sim_harness` — which is precisely what keeps it outside the
            // deterministic core. Its `grazing` systems DO steer crabs (hunger + the MEAT field) and run on
            // `FixedUpdate`; the harness never registers this plugin, so they cannot perturb `snapshot_hash`.
            // See the `mycelia` module docs before moving any of it.
            // `LightingPlugin` (real fixture lights + GTAO/contact shadows) sits here because it is
            // cosmetic/GPU and windowed-only — deliberately NOT in `sim_harness`, so the deterministic
            // core never depends on a GPU (the harness keeps the plain `world` ambient+directional). The
            // gameplay `LightField` it will own is registered separately so the harness CAN see it.
            (vhs::VhsPlugin, blood_lens::BloodLensPlugin, mycelia::MyceliaPlugin, light::LightingPlugin),
            // Windowed game-system UI (HUD, menus, state machine) + world-space dialogue bubbles.
            // Both registered only here, never in the headless harness, so they stay outside the
            // deterministic core (see `ui` docs). Dialogue needs `MenuState` (from `UiPlugin`) for the
            // sim-freeze during a modal exchange; it is cosmetic/`Update`, never `FixedUpdate`.
            //
            // `PsiVisionPlugin` (the Psionic's diegetic field-sight — a mechanic) and `AiOverlayPlugin`
            // (the F3 squad-AI state label — a dev tool) sit in this group because both read the bubble
            // assets `DialoguePlugin` sets up, and both are cosmetic `Update` systems that the harness
            // never registers. Grouped in a nested tuple to stay under Bevy's 16-element plugin limit.
            (
                ui::UiPlugin,
                dialogue::DialoguePlugin,
                psi_vision::PsiVisionPlugin,
                ai_overlay::AiOverlayPlugin,
            ),
        ));

    // Pinned simulation runs on `FixedUpdate` at a fixed 60 Hz, so gameplay advances at the same rate
    // regardless of render frame rate (movement is dt-scaled, so real-time speed is unchanged — the sim
    // just steps deterministically). Cosmetic/FX/input systems stay on `Update`. See the per-plugin
    // `FixedUpdate` registrations (ai, squad, enemy, crab, nest, laser).
    app.insert_resource(Time::<bevy::time::Fixed>::from_hz(60.0));

    // devshot is a dev-only in-process screenshot tool — strip it (and its `mod`) from release builds
    // (see CLAUDE.md). Gating both the registration and `mod devshot;` on `debug_assertions` keeps the
    // release binary free of the module and its per-frame `screenshot.request` sentinel polling.
    #[cfg(debug_assertions)]
    app.add_plugins(devshot::DevShotPlugin);

    // The watcher's "is the player looking at it?" gaze — WINDOWED-ONLY. It reads the live camera (which
    // eases over wall-clock time), so registering it only here keeps it out of the headless deterministic
    // harness: `enemy::smiley_reflex` there reads a stable `WatchedByPlayer(false)` and stays
    // bit-reproducible. See `enemy::snapshot_player_gaze`.
    app.add_systems(Update, enemy::snapshot_player_gaze);

    // The gestation "twitching lump" tell — WINDOWED-ONLY cosmetic (spawns child meshes on infested hosts),
    // so the headless deterministic core spawns nothing and its goldens are untouched. See
    // `parasite::drive_infestation_tell`.
    app.add_systems(Update, parasite::drive_infestation_tell);

    app.run();
}
