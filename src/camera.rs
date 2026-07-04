//! Isometric orthographic RTS camera. It's a free-panning rig — **WASD** (or arrow keys) scroll
//! the map, the mouse wheel zooms, and middle-mouse drag pulls the view around. The camera drives
//! a single `focus` point and always sits at the iso offset from it. It no longer follows any
//! character (the squad is commanded by mouse; see `selection`).

use bevy::camera::ScalingMode;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

use crate::dungeon::Dungeon;

/// World-space camera offset from the focus point. Equal-ish axes give the iso tilt.
const ISO_OFFSET: Vec3 = Vec3::new(12.0, 12.0, 12.0);
/// Initial vertical world units shown.
const VIEWPORT_HEIGHT: f32 = 12.0;
const MIN_ZOOM: f32 = 5.0;
const MAX_ZOOM: f32 = 40.0;
const ZOOM_STEP: f32 = 2.0;
const PAN_SPEED: f32 = 16.0;
const DRAG_SCALE: f32 = 0.03;

/// Screen-aligned "into the scene" ground direction (camera forward flattened). Panning uses this
/// so "up" scrolls away from the camera, not along a world axis.
pub const SCREEN_FORWARD: Vec3 = Vec3::new(-1.0, 0.0, -1.0);
/// Screen-aligned "right" on the ground plane — perpendicular to [`SCREEN_FORWARD`].
pub const SCREEN_RIGHT: Vec3 = Vec3::new(1.0, 0.0, -1.0);

/// The camera's target point and zoom.
#[derive(Resource)]
struct CameraRig {
    focus: Vec3,
    height: f32,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CameraRig {
            focus: Vec3::ZERO,
            height: VIEWPORT_HEIGHT,
        })
        .add_systems(Startup, setup_camera)
        .add_systems(Update, drive_camera);
    }
}

fn setup_camera(mut commands: Commands, dungeon: Res<Dungeon>, mut rig: ResMut<CameraRig>) {
    // Start focused on the squad's spawn so there is no opening lurch.
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

fn drive_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut rig: ResMut<CameraRig>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    if scroll.delta.y != 0.0 {
        rig.height = (rig.height - scroll.delta.y * ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    let dt = time.delta_secs();
    // WASD (and arrow keys) scroll the map along the screen axes.
    let mut pan = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        pan += SCREEN_FORWARD;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        pan -= SCREEN_FORWARD;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        pan += SCREEN_RIGHT;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
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

    let (mut transform, mut projection) = camera.into_inner();
    *transform = Transform::from_translation(rig.focus + ISO_OFFSET).looking_at(rig.focus, Vec3::Y);
    if let Projection::Orthographic(ortho) = projection.as_mut() {
        ortho.scaling_mode = ScalingMode::FixedVertical {
            viewport_height: rig.height,
        };
    }
}
