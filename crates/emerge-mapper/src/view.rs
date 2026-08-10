//! **The camera and the ground** — an isometric rig over an infinite grid.
//!
//! Same shape as the game's `camera.rs`, because an author who has been driving the game should not
//! have to learn a second set of keys: **WASD** pans along the screen axes, the **wheel** zooms,
//! **Q/E** rotate the view in quarter detents. Orthographic, so a piece reads the way it will in the
//! game rather than at whatever perspective the editor happened to pick.
//!
//! # The grid is Bevy's, not ours
//!
//! **The ground is the map's, not a dev tool's.** This used `bevy::dev_tools::infinite_grid`, whose
//! own settings block promised *"one metre between minor lines, so the grid IS the snap"* — and
//! `emerge_core::grid::SNAP` is 0.5, so every square was two cells. It also drew forever, which made
//! the ground outside `Map::bounds` look exactly as buildable as the ground inside it. Both are now
//! `editor::draw_map_grid`, bounded by the map and spaced by the constant that defines a cell.
//!
//! # One camera, and it carries a marker
//!
//! `MainCamera` exists here for the same reason it exists in the game, and the reason is worth
//! restating because it cost a day there: `Single<.., With<Camera3d>>` **silently skips its system**
//! when a second 3D camera appears, so a thumbnail booth or a gizmo overlay can kill nine unrelated
//! systems with no error anywhere. Filter positively on the marker, never on `Camera3d` alone.

use std::f32::consts::TAU;

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;

use crate::keys::{self, Action};
use bevy::camera::ScalingMode;

/// The editor's one 3D camera. See the module note.
#[derive(Component)]
pub struct MainCamera;

/// Where the camera sits relative to what it is looking at, before yaw. The game's own iso offset.
const ISO_OFFSET: Vec3 = Vec3::new(12.0, 12.0, 12.0);

/// [`ISO_OFFSET`]'s elevation above the ground plane: `atan(12 / hypot(12, 12)) = atan(1/sqrt 2)`.
/// The default for [`Rig::elevation`], so the map view is bit-close to what it always was.
pub const ISO_ELEVATION: f32 = 0.615_479_7;

/// [`ISO_OFFSET`]'s length — the camera's distance from its focus. Constant across elevation:
/// an orthographic camera's image does not depend on distance, only on direction and the
/// viewport height, so one length serves every preset.
const ISO_DISTANCE: f32 = 20.784_609; // |(12, 12, 12)| = 12 * sqrt 3

/// Quarter turns, so the grid stays square to the screen at every detent.
const ROTATION_STEPS: u32 = 4;

const MIN_ZOOM: f32 = 4.0;
/// The furthest out the rig goes. `pub(crate)` because the Compose sheet has to know it: a gallery
/// that needs more than this to be seen whole is cropped, and cropped silently reads as complete.
pub(crate) const MAX_ZOOM: f32 = 80.0;
const ZOOM_STEP: f32 = 2.0;
/// Metres a second, matching the game's `src/camera.rs` so the two feel the same.
const PAN_SPEED: f32 = 16.0;

/// Where the camera is looking and how far out.
#[derive(Resource)]
pub struct Rig {
    pub focus: Vec3,
    /// Orthographic viewport height in metres — the zoom.
    pub height: f32,
    /// Current eased yaw, and the detent it is heading for.
    pub yaw: f32,
    pub goal_yaw: f32,
    /// Radians above the ground plane. [`ISO_ELEVATION`] everywhere but the anim bench's camera
    /// presets — judging foot contact needs a near-ground line of sight, and yaw alone cannot
    /// lower the eye.
    pub elevation: f32,
}

impl Default for Rig {
    fn default() -> Self {
        Rig {
            focus: Vec3::ZERO,
            height: 18.0,
            yaw: 0.0,
            goal_yaw: 0.0,
            elevation: ISO_ELEVATION,
        }
    }
}

impl Rig {
    /// Where the camera sits relative to its focus: [`ISO_DISTANCE`] out along the rig's yaw and
    /// elevation. At the default elevation this is exactly `RotY(yaw) * ISO_OFFSET`, which is
    /// what `offset_matches_the_iso_constant_at_default_elevation` pins.
    pub fn offset(&self) -> Vec3 {
        let horizontal = self.elevation.cos() * std::f32::consts::FRAC_1_SQRT_2;
        let before_yaw = Vec3::new(horizontal, self.elevation.sin(), horizontal) * ISO_DISTANCE;
        Quat::from_rotation_y(self.yaw) * before_yaw
    }
}

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Rig>()
            .init_resource::<Pointer>()
            .add_systems(Startup, setup)
            // **Before anything acts on it.** `Phase::Sense` is where this editor decides who owns an
            // input for the frame — the keyboard already does — so the pointer is read once, there,
            // and every spatial system downstream sees one answer.
            .add_systems(Update, sense_pointer.in_set(keys::Phase::Sense))
            .add_systems(Update, drive.in_set(keys::Phase::Act));
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
    // **The whole system cannot be gated**, because its tail writes the camera transform every frame
    // — a run condition here would freeze the view rather than ignore a key. So the context is a
    // parameter and `keys::just_pressed` does the refusing, per key, in the one place that decides it.
    live: Res<keys::Live>,
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
    if keys::just_pressed(&keys, live.0, Action::TurnViewLeft) {
        rig.goal_yaw += step;
    }
    if keys::just_pressed(&keys, live.0, Action::TurnViewRight) {
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
        if keys::pressed(&keys, live.0, action) {
            wish += dir;
        }
    }
    // Where the camera sits this frame. Needed by the pan basis below as well as by the transform at
    // the end, so it is computed once here rather than derived twice from the same two values.
    let iso = rig.offset();
    if wish != Vec2::ZERO {
        let screen = pan_direction(wish, rig.yaw);
        // **Constant speed, like the game.** This used to scale by `rig.height / 18.0`, on the
        // argument that a keypress should cross the same *fraction of the screen* at every zoom. In
        // the hand it reads as the camera sticking: zoomed in — which is where an author does detail
        // work — panning crawled at a third of the speed the game moves at, and the same keys
        // behaving differently in the two applications is the surprise worth removing.
        // `src/camera.rs:370` is the reference: `rig.focus += dir * PAN_SPEED * dt`, unscaled.
        rig.focus += screen * PAN_SPEED * dt;
    }

    let (mut tf, mut proj) = camera.into_inner();
    *tf = Transform::from_translation(rig.focus + iso).looking_at(rig.focus, Vec3::Y);
    *proj = Projection::from(OrthographicProjection {
        scaling_mode: ScalingMode::FixedVertical {
            viewport_height: rig.height,
        },
        ..OrthographicProjection::default_3d()
    });
}

/// **Where the editor's pointer is**, in logical window pixels — the one answer every spatial system
/// reads.
///
/// Filled once a frame by [`sense_pointer`]. It exists so that "where is the cursor" has a single
/// definition in this crate: a system that asks the `Window` directly is invisible to an agent, and a
/// system that asks this one behaves identically for a person at the machine.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct Pointer(pub Option<Vec2>);

/// Read the pointer once, before anything acts on it.
///
/// With the `debugger` feature on, an injected position takes precedence over the real mouse —
/// through `bevy_debugger_bevy::cursor_position`, which is the plugin's own rule rather than a second
/// copy of it here. **The window's own cursor is never written**: Bevy's windowing backend turns a
/// change to it into a request to move the *physical* pointer, which would drag the mouse out from
/// under whoever is at the machine. `DebugCursor`'s own docs carry the measurement and the line.
pub fn sense_pointer(
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    #[cfg(feature = "debugger")] injected: Res<bevy_debugger_bevy::DebugCursor>,
    mut pointer: ResMut<Pointer>,
) {
    let next = match &window {
        #[cfg(feature = "debugger")]
        Some(w) => bevy_debugger_bevy::cursor_position(w, &injected),
        #[cfg(not(feature = "debugger"))]
        Some(w) => w.cursor_position(),
        // No window at all is a headless run. An injected pointer still has nowhere to be read
        // against — `viewport_to_world` needs a camera with a viewport — so this is honestly `None`.
        None => None,
    };
    if pointer.0 != next {
        pointer.0 = next;
    }
}

/// **Is the pointer over a panel** — asked of the layout, not of the picking backend.
///
/// `bevy_picking`'s `Hovered` answers the same question for the mouse verbs, and correctly: a click
/// is delivered by the picking backend, so the two agree by construction. A **keyboard** verb asking
/// it is a different matter, and got this wrong in one shipped commit: `Hovered` is written from the
/// *window's* cursor, which is the one thing [`sense_pointer`] refuses to move for an agent — so the
/// answer is unreachable over BRP and untestable headless. That is the same trap `Pointer` exists to
/// close, entered from the other side.
///
/// So this reads the rects. `ComputedNode::size` is in physical pixels and the pointer is logical,
/// which is what `inverse_scale_factor` converts — the distinction `crate::tiles`'s list arithmetic
/// already had to make once.
///
/// A node with `Pickable::IGNORE` is deliberately still counted: this asks *"is a panel drawn here"*,
/// which is a question about the layout, and click-through is a question about clicks.
pub fn over_ui<'a>(
    cursor: Option<Vec2>,
    nodes: impl IntoIterator<Item = (&'a bevy::ui::ComputedNode, &'a GlobalTransform)>,
) -> bool {
    let Some(cursor) = cursor else {
        // No cursor is not "over the world" — there is no honest answer, and the callers treat a
        // missing pointer as no answer everywhere else in this file.
        return false;
    };
    nodes.into_iter().any(|(node, tf)| {
        let size = node.size() * node.inverse_scale_factor();
        if size.x <= 0.0 || size.y <= 0.0 {
            return false;
        }
        let centre = tf.translation().truncate() * node.inverse_scale_factor();
        let half = size * 0.5;
        (cursor.x - centre.x).abs() <= half.x && (cursor.y - centre.y).abs() <= half.y
    })
}

/// Where the pointer meets the ground plane, in world metres.
///
/// The editor's whole spatial input is this one function, so it is worth being exact about: a ray
/// through the cursor, intersected with `y = 0`. `None` when the cursor is off-window or the ray runs
/// parallel to the ground — both are "there is no honest answer", and the callers treat them as such
/// rather than falling back to the origin.
///
/// Takes the position rather than the `Window` so that there is exactly one place deciding *which*
/// position that is — see [`Pointer`].
/// **The ray under the cursor**, as `(origin, unit direction)` in world metres.
///
/// Split out of [`cursor_ground`] because the ground point is not enough to pick a thing that stands
/// up. Under this rig a screen point over a feature at height `h` intersects `y = 0` roughly `h`
/// metres away from where that feature actually is, so testing a ground point against an object's
/// FLOOR footprint misses everything but its base. Anything picking a volume wants the ray; anything
/// picking a position on the floor wants the intersection below.
///
/// Returned as a pair rather than `Ray3d` so callers do the arithmetic in plain `Vec3` — the picking
/// this feeds is a slab test, not a Bevy query.
pub fn cursor_ray(
    cursor: Option<Vec2>,
    camera: &Camera,
    cam_tf: &GlobalTransform,
) -> Option<(Vec3, Vec3)> {
    let cursor = cursor?;
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    Some((ray.origin, *ray.direction))
}

pub fn cursor_ground(
    cursor: Option<Vec2>,
    camera: &Camera,
    cam_tf: &GlobalTransform,
) -> Option<Vec3> {
    let (origin, dir) = cursor_ray(cursor, camera, cam_tf)?;
    let ray = Ray { origin, dir };
    let denom = ray.dir.y;
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = -ray.origin.y / denom;
    (t > 0.0).then(|| ray.origin + ray.dir * t)
}

/// A ray in plain vectors, so the two readers above share one spelling of it.
struct Ray {
    origin: Vec3,
    dir: Vec3,
}

/// **Which way the world moves for a pan key**, in world metres on the ground plane.
///
/// `wish` is in screen terms: `+x` is right, `-y` is "up the screen, away from the viewer" — the
/// signs the key table above uses.
///
/// # It comes from where the camera is, not from the world axes
///
/// This used to be `RotY(yaw) * (wish.x, 0, wish.y)`, which pans along world X and Z. Those are only
/// the screen's axes if the camera looks straight down one of them, and it does not: [`ISO_OFFSET`]
/// is (12, 12, 12), so the view looks along the XZ diagonal and `W` slid the map up-and-sideways at
/// 45 degrees. The comment on the key table has always claimed screen axes; this is the version that
/// delivers them.
///
/// `forward` is the camera's look direction flattened onto the ground — screen "up". `right` is that
/// turned a quarter about +Y, which is `cross(forward, Y)` written out. Both fall out of the same
/// offset the camera transform uses, so they stay right at every rotation detent and would stay
/// right if the pitch changed.
///
/// A camera looking straight down has no ground-projected forward, so there is no honest screen
/// axis to pan along and this returns zero rather than inventing one. [`ISO_OFFSET`] is not
/// straight down, so that is unreachable today.
pub fn pan_direction(wish: Vec2, yaw: f32) -> Vec3 {
    let iso = Quat::from_rotation_y(yaw) * ISO_OFFSET;
    let forward = Vec3::new(-iso.x, 0.0, -iso.z).normalize_or_zero();
    let right = Vec3::new(-forward.z, 0.0, forward.x);
    // `wish.y` is negative for "up the screen", which is why it is subtracted.
    (right * wish.x - forward * wish.y).normalize_or_zero()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The camera's own basis at a given yaw: screen right and screen up, as world vectors.
    fn screen_basis(yaw: f32) -> (Vec3, Vec3) {
        let iso = Quat::from_rotation_y(yaw) * ISO_OFFSET;
        let tf = Transform::from_translation(iso).looking_at(Vec3::ZERO, Vec3::Y);
        (*tf.right(), *tf.up())
    }

    /// **The property the keys are supposed to have**, stated the way an author experiences it: a pan
    /// key moves the world along one screen axis and not at all along the other.
    ///
    /// Checked at every rotation detent, because the old version was wrong at all four and a test at
    /// one could not have told the difference.
    #[test]
    fn each_pan_key_moves_the_view_along_exactly_one_screen_axis() {
        for detent in 0..ROTATION_STEPS {
            let yaw = detent as f32 * TAU / ROTATION_STEPS as f32;
            let (right, up) = screen_basis(yaw);

            for (name, wish, want_x, want_y) in [
                ("W", Vec2::new(0.0, -1.0), 0.0f32, 1.0f32),
                ("S", Vec2::new(0.0, 1.0), 0.0, -1.0),
                ("A", Vec2::new(-1.0, 0.0), -1.0, 0.0),
                ("D", Vec2::new(1.0, 0.0), 1.0, 0.0),
            ] {
                let dir = pan_direction(wish, yaw);
                // How the motion reads on screen. The camera's `up` has a vertical component the
                // ground plane has not, so the screen-vertical reading is scaled — the SIGN, and the
                // other axis being zero, are the claims that matter.
                let on = Vec2::new(dir.dot(right), dir.dot(up));
                let at = format!("{name} at detent {detent}");

                if want_x == 0.0 {
                    assert!(on.x.abs() < 1e-4, "{at}: should not move sideways, moved {:.3}", on.x);
                } else {
                    assert!(
                        on.x.signum() == want_x.signum() && on.x.abs() > 0.1,
                        "{at}: wanted screen-x sign {want_x}, got {:.3}",
                        on.x
                    );
                }
                if want_y == 0.0 {
                    assert!(on.y.abs() < 1e-4, "{at}: should not move vertically, moved {:.3}", on.y);
                } else {
                    assert!(
                        on.y.signum() == want_y.signum() && on.y.abs() > 0.1,
                        "{at}: wanted screen-y sign {want_y}, got {:.3}",
                        on.y
                    );
                }
            }
        }
    }

    /// The elevation extension changes nothing at the default: `offset()` reproduces the shipped
    /// `RotY(yaw) * ISO_OFFSET` bit-close at every detent, and elevation 0 lies in the ground
    /// plane at the same yaw — the anim presets' ground-level view is a pure rotation of the eye,
    /// never a different distance or focus.
    #[test]
    fn the_default_elevation_reproduces_the_iso_offset_exactly() {
        for step in 0..4 {
            let yaw = step as f32 * std::f32::consts::FRAC_PI_2;
            let rig = Rig { yaw, ..Rig::default() };
            let old = Quat::from_rotation_y(yaw) * ISO_OFFSET;
            assert!(
                rig.offset().distance(old) < 1.0e-3,
                "yaw {yaw}: {:?} vs {:?}",
                rig.offset(),
                old
            );
        }
        let grounded = Rig { elevation: 0.0, ..Rig::default() };
        assert!(grounded.offset().y.abs() < 1.0e-4, "elevation 0 must lie in the ground plane");
        assert!(
            (grounded.offset().length() - ISO_DISTANCE).abs() < 1.0e-3,
            "the distance is constant across elevation"
        );
    }

    /// **The regression, named.** The old basis was the world axes turned by yaw; at the shipped
    /// isometric offset that is 45 degrees off the screen, which is what "off putting" was.
    #[test]
    fn the_old_world_axis_basis_was_off_by_an_eighth_turn() {
        let (_, up) = screen_basis(0.0);
        let old = (Quat::from_rotation_y(0.0) * Vec3::new(0.0, 0.0, -1.0)).normalize();
        let new = pan_direction(Vec2::new(0.0, -1.0), 0.0);
        assert!(
            old.angle_between(new) > 0.7,
            "the fix must actually move the axis; got {} rad",
            old.angle_between(new)
        );
        // And only the new one is straight up the screen.
        let (right, _) = screen_basis(0.0);
        assert!(old.dot(right).abs() > 0.5, "the old basis drifted sideways");
        assert!(new.dot(right).abs() < 1e-4, "the new basis does not");
        assert!(new.dot(up) > 0.0, "and still goes away from the viewer");
    }
}
