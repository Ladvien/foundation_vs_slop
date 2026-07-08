//! Isometric orthographic RTS camera. It's a free-panning rig — **WASD** (or arrow keys) scroll
//! the map, the mouse wheel zooms, **Q/E** rotate the view in discrete detents, and middle-mouse
//! drag pulls the view around. The camera drives a single `focus` point and always sits at the iso
//! offset from it. It no longer follows any character (the squad is commanded by mouse; see
//! `selection`).

use bevy::camera::ScalingMode;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::time::Real;

use std::f32::consts::TAU;

use crate::dungeon::Dungeon;
use crate::juice::Trauma;
use crate::time_control::SimBlocked;

/// World-space camera offset from the focus point. Equal-ish axes give the iso tilt. `pub` so the
/// audio spatial listener can recover the ground focus point (`camera_pos - ISO_OFFSET`) to anchor
/// itself on the plane instead of ~20 units up at the camera (see `audio::sync_listener`).
pub const ISO_OFFSET: Vec3 = Vec3::new(12.0, 12.0, 12.0);
/// Peak screen-shake offset (world units) at full trauma. Applied as `SHAKE_MAX * trauma²`.
const SHAKE_MAX: f32 = 0.85;
/// Peak shake roll (radians) at full trauma — a small camera twist for extra kick.
const SHAKE_ROLL: f32 = 0.035;
/// Initial vertical world units shown.
const VIEWPORT_HEIGHT: f32 = 12.0;
const MIN_ZOOM: f32 = 5.0;
const MAX_ZOOM: f32 = 34.0;
const ZOOM_STEP: f32 = 2.0;
const PAN_SPEED: f32 = 16.0;
const DRAG_SCALE: f32 = 0.03;
/// Discrete rotation detents in a full turn — Q/E snap the yaw by `TAU / ROTATION_STEPS` per press
/// (4 → 90° clicks). Each stop is a true iso *corner* view: the camera looks down one of the four
/// (±X,±Z) diagonals, so exactly two adjacent wall edges face it — the pair the knee-wall cutaway
/// squashes (see `dungeon::update_cutaway`). The ~35° iso pitch is preserved at every stop, since
/// yawing about world Y never changes the offset's height-to-horizontal ratio.
const ROTATION_STEPS: u32 = 4;
/// Exponential-smoothing rate for the yaw ease toward `target_yaw`; higher = snappier settle.
/// Frame-rate independent via `1 − exp(−k·dt)` (Holmér, "Lerp smoothing is broken", 2023).
const ROTATE_SMOOTHING: f32 = 9.0;

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
    /// Current camera yaw (radians) about the focus — eases toward `target_yaw` each frame.
    yaw: f32,
    /// Snapped goal yaw. Q/E step it by `TAU / ROTATION_STEPS`; rapid taps accumulate.
    target_yaw: f32,
}

/// Published each frame for the dungeon's view-relative wall cutaway. `to_camera` is the horizontal
/// direction from the focus toward the camera (the yawed iso diagonal); a wall's inner face is toward
/// the camera — and so occludes the room and should be squashed — when its outward normal has a
/// positive dot with this. Only the per-axis sign matters at the 90° detents, but it's kept continuous
/// so the cutaway can ease across a turn (see `dungeon::update_cutaway`).
#[derive(Resource, Default)]
pub struct CameraView {
    pub to_camera: Vec3,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CameraRig {
            focus: Vec3::ZERO,
            height: VIEWPORT_HEIGHT,
            yaw: 0.0,
            target_yaw: 0.0,
        })
        .init_resource::<CameraView>()
        .add_systems(Startup, setup_camera)
        // Read `SimBlocked` only after its sole writer has settled this frame, so opening/closing a
        // menu never leaks or drops a frame of pan. (No-op in the headless harness, where
        // `sync_sim_blocked` isn't registered — an `.after` on an absent system is simply ignored.)
        .add_systems(
            Update,
            drive_camera.after(crate::ui::state::sync_sim_blocked),
        );
    }
}

/// Smooth pseudo-noise in `[-1, 1]` from two detuned sines — a cheap Perlin stand-in for shake so the
/// motion shudders smoothly instead of jittering per frame. `seed` decorrelates the axes.
fn shake_noise(t: f32, seed: f32) -> f32 {
    (t * 37.0 + seed).sin() * 0.6 + (t * 91.0 + seed * 2.3).sin() * 0.4
}

fn setup_camera(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    mut rig: ResMut<CameraRig>,
    mut view: ResMut<CameraView>,
) {
    // Start focused on the squad's spawn so there is no opening lurch.
    rig.focus = dungeon.spawn_world();
    // yaw = 0 ⇒ camera looks from (+X,+Z); seed the cutaway so the E/S near walls are already knee-high
    // on the first rendered frame (no startup squash animation).
    view.to_camera = Vec3::new(1.0, 0.0, 1.0);
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
    // `time` (virtual) drives only the gameplay-feel screen shake below; the human camera controls
    // (pan and rotate) run on `real` so they feel identical at any game speed — including paused.
    time: Res<Time>,
    real: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    trauma: Res<Trauma>,
    // A blocking UI screen (boot/title/pause/settings/roster) is up: the WASD/arrow keys double as
    // that menu's navigation, so suppress *panning* while it's open — but keep zoom, Q/E rotate, and
    // middle-drag live, since none of those collide with the menu keys and the player still wants to
    // inspect the frozen scene behind the overlay (honoring the `time_control` invariant that pausing
    // never changes how the mouse/other keys respond). This is *not* gated on the `0`-key `UserPaused`
    // — panning while that tactical pause is active is intentional (see below). Stays `false` in the
    // headless harness, so camera control there is unchanged.
    sim_blocked: Res<SimBlocked>,
    mut rig: ResMut<CameraRig>,
    mut view: ResMut<CameraView>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let allow_pan = !sim_blocked.0;

    if scroll.delta.y != 0.0 {
        rig.height = (rig.height - scroll.delta.y * ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    // Q/E rotate the whole view in discrete detents around the focus. Each press snaps the goal by
    // one step; the current yaw eases toward it below, so rapid taps stack and the camera smoothly
    // chases the accumulated target. Q turns counter-clockwise (from above), E clockwise.
    let step = TAU / ROTATION_STEPS as f32;
    if keys.just_pressed(KeyCode::KeyQ) {
        rig.target_yaw += step;
    }
    if keys.just_pressed(KeyCode::KeyE) {
        rig.target_yaw -= step;
    }
    // Ease on REAL time so the rotation feels identical at any game speed and works while paused.
    let ease = 1.0 - (-ROTATE_SMOOTHING * real.delta_secs()).exp();
    rig.yaw += (rig.target_yaw - rig.yaw) * ease;
    // Once settled, snap exactly and wrap both angles together to keep the accumulator bounded.
    if (rig.target_yaw - rig.yaw).abs() < 1e-4 {
        let wrapped = rig.target_yaw.rem_euclid(TAU);
        rig.yaw = wrapped;
        rig.target_yaw = wrapped;
    }
    // Yaw about world Y: rotates the iso offset and the screen-space pan axes in lockstep, so the
    // view spins while WASD/drag stay aligned to the (now-rotated) screen.
    let yaw_rot = Quat::from_rotation_y(rig.yaw);
    let screen_forward = yaw_rot * SCREEN_FORWARD;
    let screen_right = yaw_rot * SCREEN_RIGHT;
    // Publish the horizontal camera direction (the yawed iso diagonal) for the wall cutaway.
    view.to_camera = yaw_rot * Vec3::new(1.0, 0.0, 1.0);

    // Pan on REAL time, not the sim clock: keyboard panning must feel the same at ×1, ×64, or paused.
    // (Reading the generic `Time` here would resolve to `Time<Virtual>` and scale pan speed with the
    // game-speed multiplier — flying at high speed, dead when paused. Zoom/drag below already use raw
    // per-frame input deltas, so they're speed-independent without needing `dt`.)
    let dt = real.delta_secs();
    // WASD (and arrow keys) scroll the map along the screen axes — unless a menu is open, in which
    // case those keys belong to menu navigation.
    let mut pan = Vec3::ZERO;
    if allow_pan {
        if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
            pan += screen_forward;
        }
        if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
            pan -= screen_forward;
        }
        if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
            pan += screen_right;
        }
        if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
            pan -= screen_right;
        }
    }
    if let Some(dir) = pan.try_normalize() {
        rig.focus += dir * PAN_SPEED * dt;
    }
    // Middle-mouse drag to pull the map around.
    if mouse_buttons.pressed(MouseButton::Middle) {
        let d = mouse_motion.delta;
        rig.focus += (-d.x * screen_right + d.y * screen_forward) * DRAG_SCALE;
    }

    // Trauma² screen shake (Eiserloh, GDC 2016): offset the whole view so the iso angle is kept,
    // plus a small roll for kick. The transform is rebuilt from `rig` each frame, so this is purely
    // additive and never accumulates drift.
    let shake_t = trauma.0 * trauma.0;
    let iso = yaw_rot * ISO_OFFSET;
    let (mut transform, mut projection) = camera.into_inner();
    if shake_t > 0.0 {
        let t = time.elapsed_secs();
        let offset = Vec3::new(shake_noise(t, 0.0), shake_noise(t, 7.3), shake_noise(t, 13.7))
            * (SHAKE_MAX * shake_t);
        let roll = shake_noise(t, 21.1) * (SHAKE_ROLL * shake_t);
        *transform = Transform::from_translation(rig.focus + iso + offset)
            .looking_at(rig.focus + offset, Vec3::Y);
        transform.rotate_local_z(roll);
    } else {
        *transform =
            Transform::from_translation(rig.focus + iso).looking_at(rig.focus, Vec3::Y);
    }
    if let Projection::Orthographic(ortho) = projection.as_mut() {
        ortho.scaling_mode = ScalingMode::FixedVertical {
            viewport_height: rig.height,
        };
    }
}
