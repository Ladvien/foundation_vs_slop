//! Isometric orthographic camera with two modes: **follow** the player (the map
//! scrolls past the viewport as the agent moves) and **free pan** (scout the dungeon
//! independently). Both drive a single `focus` point; the camera always sits at the
//! iso offset from it. `Tab` toggles modes; the mouse wheel zooms.

use bevy::camera::ScalingMode;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

use crate::dungeon::Dungeon;
use crate::player::Player;

/// World-space camera offset from the focus point. Equal-ish axes give the iso tilt.
const ISO_OFFSET: Vec3 = Vec3::new(12.0, 12.0, 12.0);
/// Initial vertical world units shown; ~12 tiles against a 40-tile map → scrollable.
const VIEWPORT_HEIGHT: f32 = 12.0;
const MIN_ZOOM: f32 = 5.0;
const MAX_ZOOM: f32 = 40.0;
const ZOOM_STEP: f32 = 2.0;
const PAN_SPEED: f32 = 14.0;
const DRAG_SCALE: f32 = 0.03;

/// Screen-aligned "into the scene" ground direction (camera forward flattened).
/// Movement and panning use this so "up" is away from the camera, not a world axis.
pub const SCREEN_FORWARD: Vec3 = Vec3::new(-1.0, 0.0, -1.0);
/// Screen-aligned "right" on the ground plane — perpendicular to [`SCREEN_FORWARD`].
pub const SCREEN_RIGHT: Vec3 = Vec3::new(1.0, 0.0, -1.0);

#[derive(PartialEq, Eq, Clone, Copy)]
enum CameraMode {
    Follow,
    FreePan,
}

/// The camera's target point and view settings, shared by both modes.
#[derive(Resource)]
struct CameraRig {
    focus: Vec3,
    mode: CameraMode,
    height: f32,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CameraRig {
            focus: Vec3::ZERO,
            mode: CameraMode::Follow,
            height: VIEWPORT_HEIGHT,
        })
        .add_systems(Startup, setup_camera)
        // Run after the player has moved this frame so the follow camera never reads a
        // stale position (which would jitter the world relative to the hero).
        .add_systems(Update, drive_camera.after(crate::player::player_movement));
    }
}

fn setup_camera(mut commands: Commands, dungeon: Res<Dungeon>, mut rig: ResMut<CameraRig>) {
    // Start focused on the player's spawn so there is no opening lurch.
    rig.focus = dungeon.spawn_world();
    commands.spawn((
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: rig.height,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(rig.focus + ISO_OFFSET).looking_at(rig.focus, Vec3::Y),
    ));
}

#[allow(clippy::too_many_arguments)]
fn drive_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut rig: ResMut<CameraRig>,
    player: Single<&Transform, (With<Player>, Without<Camera3d>)>,
    camera: Single<(&mut Transform, &mut Projection), (With<Camera3d>, Without<Player>)>,
) {
    if keys.just_pressed(KeyCode::Tab) {
        rig.mode = match rig.mode {
            CameraMode::Follow => CameraMode::FreePan,
            CameraMode::FreePan => CameraMode::Follow,
        };
    }

    if scroll.delta.y != 0.0 {
        rig.height = (rig.height - scroll.delta.y * ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    let dt = time.delta_secs();
    match rig.mode {
        CameraMode::Follow => {
            // Lock the focus exactly on the player so it stays centred every frame — no
            // rubber-banding lead/lag between the hero and the scrolling walls.
            rig.focus = player.translation;
        }
        CameraMode::FreePan => {
            let mut pan = Vec3::ZERO;
            if keys.pressed(KeyCode::ArrowUp) {
                pan += SCREEN_FORWARD;
            }
            if keys.pressed(KeyCode::ArrowDown) {
                pan -= SCREEN_FORWARD;
            }
            if keys.pressed(KeyCode::ArrowRight) {
                pan += SCREEN_RIGHT;
            }
            if keys.pressed(KeyCode::ArrowLeft) {
                pan -= SCREEN_RIGHT;
            }
            if let Some(dir) = pan.try_normalize() {
                rig.focus += dir * PAN_SPEED * dt;
            }
            // Middle-mouse drag to pull the map around.
            if mouse_buttons.pressed(MouseButton::Middle) {
                let d = mouse_motion.delta;
                rig.focus += (-d.x * SCREEN_RIGHT + d.y * SCREEN_FORWARD) * DRAG_SCALE;
            }
        }
    }

    let (mut transform, mut projection) = camera.into_inner();
    *transform = Transform::from_translation(rig.focus + ISO_OFFSET).looking_at(rig.focus, Vec3::Y);
    if let Projection::Orthographic(ortho) = projection.as_mut() {
        ortho.scaling_mode = ScalingMode::FixedVertical {
            viewport_height: rig.height,
        };
    }
}
