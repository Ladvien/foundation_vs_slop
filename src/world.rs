//! Environment lighting for the dungeon. No ground plane — the WFC dungeon lays down
//! its own floor tiles. Bright, warm, even ambient gives the flat fluorescent glow of the
//! Backrooms; the player's torch adds a subtle hotspot. Fog still hides *unexplored* tiles
//! entirely (black void), which is the eerie part — see `fog`.

use bevy::prelude::*;

use crate::config::GameConfig;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        // Pull the environment-fill values from the one `lighting:` config slice (shared with
        // `light::LightingPlugin`, which owns the fixtures) so there is a single source of truth for
        // every light knob — one path, no hardcoded second copy. `ConfigPlugin` runs first, so
        // `GameConfig` exists here at build time (same seam every consumer plugin uses).
        let cfg = app.world().resource::<GameConfig>().lighting.clone();
        app.insert_resource(GlobalAmbientLight {
            // Warm fluorescent fill — the Backrooms' flat glow, dimmed enough that fixtures read.
            color: Color::srgb(cfg.ambient_color[0], cfg.ambient_color[1], cfg.ambient_color[2]),
            brightness: cfg.ambient_brightness,
            ..default()
        })
        .add_systems(Startup, setup_lighting);
    }
}

fn setup_lighting(mut commands: Commands, config: Res<GameConfig>) {
    // A weak, steep key light so the low-poly tiles have some directional shading
    // without washing out the torch-lit fog gradient.
    commands.spawn((
        DirectionalLight {
            illuminance: config.lighting.key_illuminance,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(6.0, 14.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
