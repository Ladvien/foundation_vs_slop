//! **A frame on demand, triggered by a file.**
//!
//! Run this, then from another terminal:
//!
//! ```sh
//! touch screenshot.request
//! ```
//!
//! The next frame writes `screenshot.png` into the working directory, rendered straight from the GPU.
//! No OS screen-capture, no accessibility prompt, no compositor, and nobody at the keyboard.
//!
//! That last part is the whole design. The thing that wants a screenshot is usually not a person: it
//! is a Makefile, a CI job, a capture script, or an agent driving a window it does not own. A sentinel
//! file works for all of them and over SSH; a key binding works for none of them.
//!
//! Run: `cargo run -p bevy_devshot --example capture`

use bevy::prelude::*;
use bevy_devshot::DevShotPlugin;

#[derive(Component)]
struct Spin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "bevy_devshot — touch screenshot.request".into(),
                // 0.19 takes physical pixels as `u32` — there is no `(f32, f32)` conversion.
                resolution: (800u32, 600u32).into(),
                ..default()
            }),
            ..default()
        }))
        // In a real game, gate this: `#[cfg(debug_assertions)]`. A shipped build has no reason to
        // watch the filesystem for a screenshot request.
        .add_plugins(DevShotPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, spin)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.2, 1.2, 1.2))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.36, 0.22),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.4, 0.0),
        Spin,
    ));

    // A floor, so the shot has something to show depth against.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(12.0, 12.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.16, 0.18),
            ..default()
        })),
        Transform::from_xyz(0.0, -0.4, 0.0),
    ));

    commands.spawn((
        DirectionalLight { illuminance: 8_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 2.2, 4.5).looking_at(Vec3::new(0.0, 0.3, 0.0), Vec3::Y),
    ));

    info!("running — `touch screenshot.request` in this directory to capture a frame");
}

fn spin(time: Res<Time>, mut cubes: Query<&mut Transform, With<Spin>>) {
    for mut tf in &mut cubes {
        tf.rotate_y(time.delta_secs() * 0.8);
        tf.rotate_x(time.delta_secs() * 0.3);
    }
}
