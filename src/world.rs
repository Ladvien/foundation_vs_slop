//! Environment lighting for the dungeon. No ground plane — the WFC dungeon lays down
//! its own floor tiles. Ambient is kept low so the player's torch defines what reads as
//! currently-visible versus merely explored (see `fog`).

use bevy::prelude::*;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight {
            // Cold, dim fill — just enough to make explored-but-unlit tiles legible.
            color: Color::srgb(0.5, 0.55, 0.7),
            brightness: 40.0,
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
