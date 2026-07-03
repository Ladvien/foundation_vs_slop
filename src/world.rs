//! Environment lighting for the dungeon. No ground plane — the WFC dungeon lays down
//! its own floor tiles. Bright, warm, even ambient gives the flat fluorescent glow of the
//! Backrooms; the player's torch adds a subtle hotspot. Fog still hides *unexplored* tiles
//! entirely (black void), which is the eerie part — see `fog`.

use bevy::prelude::*;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight {
            // Warm fluorescent fill — bright and even, the Backrooms' flat glow.
            color: Color::srgb(1.0, 0.98, 0.9),
            brightness: 500.0,
            ..default()
        })
        .add_systems(Startup, setup_lighting);
    }
}

fn setup_lighting(mut commands: Commands) {
    // A weak, steep key light so the low-poly tiles have some directional shading
    // without washing out the torch-lit fog gradient.
    commands.spawn((
        DirectionalLight {
            illuminance: 2_500.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(6.0, 14.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
