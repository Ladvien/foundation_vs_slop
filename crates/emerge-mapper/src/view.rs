//! **The camera and the ground** — an isometric rig over an infinite grid.
//!
//! Same shape as the game's `camera.rs`, because an author who has been driving the game should not
//! have to learn a second set of keys: **WASD** pans along the screen axes, the **wheel** zooms,
//! **Q/E** rotate the view in quarter detents. Orthographic, so a piece reads the way it will in the
//! game rather than at whatever perspective the editor happened to pick.
//!
//! # The grid is Bevy's, not ours
//!
//! `bevy::dev_tools::infinite_grid` (0.19) draws the ground plane in a fullscreen shader, computed
//! per pixel and faded with distance so the horizon does not alias. Verified in the vendored source
//! at `bevy_dev_tools-0.19.0/src/infinite_grid.rs`; it needs the `bevy_dev_tools` feature, which this
//! crate's manifest turns on.
//!
//! # One camera, and it carries a marker
//!
//! `MainCamera` exists here for the same reason it exists in the game, and the reason is worth
//! restating because it cost a day there: `Single<.., With<Camera3d>>` **silently skips its system**
//! when a second 3D camera appears, so a thumbnail booth or a gizmo overlay can kill nine unrelated
//! systems with no error anywhere. Filter positively on the marker, never on `Camera3d` alone.

use std::f32::consts::TAU;

use bevy::dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;

use crate::keys::{self, Action};
use bevy::camera::ScalingMode;

/// The editor's one 3D camera. See the module note.
#[derive(Component)]
pub struct MainCamera;

/// Where the camera sits relative to what it is looking at, before yaw. The game's own iso offset.
const ISO_OFFSET: Vec3 = Vec3::new(12.0, 12.0, 12.0);

/// Quarter turns, so the grid stays square to the screen at every detent.
const ROTATION_STEPS: u32 = 4;

const MIN_ZOOM: f32 = 4.0;
const MAX_ZOOM: f32 = 80.0;
const ZOOM_STEP: f32 = 2.0;
const PAN_SPEED: f32 = 14.0;

/// Where the camera is looking and how far out.
#[derive(Resource)]
pub struct Rig {
    pub focus: Vec3,
    /// Orthographic viewport height in metres — the zoom.
    pub height: f32,
    /// Current eased yaw, and the detent it is heading for.
    pub yaw: f32,
    pub goal_yaw: f32,
}

impl Default for Rig {
    fn default() -> Self {
        Rig {
            focus: Vec3::ZERO,
            height: 18.0,
            yaw: 0.0,
            goal_yaw: 0.0,
        }
    }
}

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InfiniteGridPlugin)
            .init_resource::<Rig>()
            .add_systems(Startup, setup)
            .add_systems(Update, drive);
    }
}

fn setup(mut commands: Commands, rig: Res<Rig>) {
    commands.spawn((
        Name::new("editor camera"),
        MainCamera,
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: rig.height,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(rig.focus + ISO_OFFSET).looking_at(rig.focus, Vec3::Y),
        // `AmbientLight` is a COMPONENT in Bevy 0.19 and applies per-camera; the resource form is
        // global. On the camera is both correct and the only form that compiles.
        AmbientLight {
            brightness: 320.0,
            ..default()
        },
    ));

    commands.spawn((
        Name::new("ground grid"),
        InfiniteGrid,
        InfiniteGridSettings {
            // One metre between minor lines, so the grid IS the snap: an author counting squares is
            // counting the same units the map records. A grid at some other scale is a decoration
            // that happens to be near the truth, which is worse than none.
            scale: 1.0,
            fadeout_distance: 140.0,
            ..InfiniteGridSettings::default()
        },
    ));

    // A key light and enough ambient to read a mesh's form. Directional rather than point, because
    // this scene is not a room — it is a plane an author is looking down at.
    commands.spawn((
        DirectionalLight {
            illuminance: 6_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(6.0, 12.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

}

#[allow(clippy::too_many_arguments)]
fn drive(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    // The wheel belongs to whatever is under the cursor: a scrolling palette and a zooming camera
    // both want it, and reading the raw wheel means scrolling the list also zooms the world out from
    // under it. Every pickable widget carries `Hovered`; readouts are `Pickable::IGNORE` and have none.
    hovered_ui: Query<&bevy::picking::hover::Hovered>,
    mut rig: ResMut<Rig>,
    camera: Option<Single<(&mut Transform, &mut Projection), With<MainCamera>>>,
) {
    let Some(camera) = camera else { return };
    let over_ui = hovered_ui.iter().any(|h| h.0);
    let dt = time.delta_secs();

    if scroll.delta.y != 0.0 && !over_ui {
        rig.height = (rig.height - scroll.delta.y * ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    let step = TAU / ROTATION_STEPS as f32;
    if keys::just_pressed(&keys, Action::TurnViewLeft) {
        rig.goal_yaw += step;
    }
    if keys::just_pressed(&keys, Action::TurnViewRight) {
        rig.goal_yaw -= step;
    }
    // Ease toward the detent rather than snapping, so a rapid double-tap reads as one smooth turn.
    let to_goal = rig.goal_yaw - rig.yaw;
    if to_goal.abs() > 1e-4 {
        rig.yaw += to_goal * (10.0 * dt).min(1.0);
    } else {
        rig.yaw = rig.goal_yaw;
    }

    // Pan along the SCREEN axes, not the world's, so "up" always means away from the viewer no matter
    // which detent the view is at.
    let mut wish = Vec2::ZERO;
    for (action, dir) in [
        (Action::PanForward, Vec2::new(0.0, -1.0)),
        (Action::PanBack, Vec2::new(0.0, 1.0)),
        (Action::PanLeft, Vec2::new(-1.0, 0.0)),
        (Action::PanRight, Vec2::new(1.0, 0.0)),
    ] {
        if keys::pressed(&keys, action) {
            wish += dir;
        }
    }
    if wish != Vec2::ZERO {
        let yaw_rot = Quat::from_rotation_y(rig.yaw);
        let screen = yaw_rot * Vec3::new(wish.x, 0.0, wish.y).normalize_or_zero();
        // Scaled by zoom: at a wide view a keypress should cross a similar fraction of the screen as
        // it does close in, which is what makes panning feel the same at every zoom.
        let zoom_scale = rig.height / 18.0;
        rig.focus += screen * PAN_SPEED * dt * zoom_scale;
    }

    let (mut tf, mut proj) = camera.into_inner();
    let iso = Quat::from_rotation_y(rig.yaw) * ISO_OFFSET;
    *tf = Transform::from_translation(rig.focus + iso).looking_at(rig.focus, Vec3::Y);
    *proj = Projection::from(OrthographicProjection {
        scaling_mode: ScalingMode::FixedVertical {
            viewport_height: rig.height,
        },
        ..OrthographicProjection::default_3d()
    });
}

/// Where the cursor meets the ground plane, in world metres.
///
/// The editor's whole spatial input is this one function, so it is worth being exact about: a ray
/// through the cursor, intersected with `y = 0`. `None` when the cursor is off-window or the ray runs
/// parallel to the ground — both are "there is no honest answer", and the callers treat them as such
/// rather than falling back to the origin.
pub fn cursor_ground(
    window: &Window,
    camera: &Camera,
    cam_tf: &GlobalTransform,
) -> Option<Vec3> {
    let cursor = window.cursor_position()?;
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    let denom = ray.direction.y;
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = -ray.origin.y / denom;
    (t > 0.0).then(|| ray.origin + *ray.direction * t)
}
