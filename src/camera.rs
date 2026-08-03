//! Isometric orthographic RTS camera. It's a free-panning rig — **WASD** (or arrow keys) scroll
//! the map, the mouse wheel zooms, **Q/E** rotate the view in discrete detents, and middle-mouse
//! drag pulls the view around. The camera drives a single `focus` point and always sits at the iso
//! offset from it. It no longer follows any character (the squad is commanded by mouse; see
//! `selection`).

use bevy::camera::{Hdr, ScalingMode};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::light::ShadowFilteringMethod;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::time::Real;

use crate::config::GameConfig;
use crate::input::Action;

use std::f32::consts::TAU;

use crate::dungeon::Dungeon;
use crate::juice::Trauma;
use crate::time_control::SimBlocked;

/// World-space camera offset from the focus point (rotated by the current yaw). Equal-ish axes give
/// the iso tilt. The audio spatial listener anchors on the ground plane too, but reads `CameraRig::
/// focus` directly rather than subtracting this — un-rotated recovery was wrong at every yaw detent
/// but the first (see `audio::sync_listener`).
pub const ISO_OFFSET: Vec3 = Vec3::new(12.0, 12.0, 12.0);
/// Peak screen-shake offset (world units) at full trauma. Applied as `SHAKE_MAX * trauma²`.
const SHAKE_MAX: f32 = 0.85;
/// Peak shake roll (radians) at full trauma — a small camera twist for extra kick.
const SHAKE_ROLL: f32 = 0.035;
/// Initial vertical world units shown.
const VIEWPORT_HEIGHT: f32 = 12.0;
/// Zoom bounds, in vertical world units shown. `pub` because the mold's perceptual speed limit is a
/// function of the orthographic viewport height, and its unit tests must prove the bound holds across the
/// whole zoom range (see `mycelia::perceptual`).
pub const MIN_ZOOM: f32 = 5.0;
pub const MAX_ZOOM: f32 = 34.0;
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
/// Ease rate for a `glide_to` focus pull (dialog framing, recenter) — same Holmér construction as
/// the yaw, but deliberately gentler than `ROTATE_SMOOTHING`: the player asked for the view to move
/// to the dialog "without jarring", and a 9.0 pull across half a map reads as a yank.
const GLIDE_SMOOTHING: f32 = 4.0;
/// A glide within this of its goal (world units) snaps exactly and clears — well under a pixel at
/// any zoom, and the exact-landing-then-skip shape keeps the ease from asymptoting forever.
const GLIDE_SNAP: f32 = 0.02;

/// Screen-aligned "into the scene" ground direction (camera forward flattened). Panning uses this
/// so "up" scrolls away from the camera, not along a world axis.
pub const SCREEN_FORWARD: Vec3 = Vec3::new(-1.0, 0.0, -1.0);
/// Screen-aligned "right" on the ground plane — perpendicular to [`SCREEN_FORWARD`].
pub const SCREEN_RIGHT: Vec3 = Vec3::new(1.0, 0.0, -1.0);

/// Where the camera is looking and from how far.
///
/// `pub` because Site-67 legitimately needs to aim the camera too, and it cannot do so through
/// `focus_camera_on_spawn`: that reads `Res<Dungeon>` and is keyed to a run, whereas the Site is what
/// exists when there is no run. A second place needing the same seam is the point at which a private
/// field becomes a hidden API — see `dungeon::DOORWAY_HEIGHT` for the same call.
///
/// Set `focus` and the per-frame `drive_camera` eases to it; to jump instantly, also write the camera
/// `Transform` (what [`snap_camera_to`] does).
#[derive(Resource)]
pub struct CameraRig {
    pub focus: Vec3,
    height: f32,
    /// Current camera yaw (radians) about the focus — eases toward `target_yaw` each frame.
    yaw: f32,
    /// Snapped goal yaw. Q/E step it by `TAU / ROTATION_STEPS`; rapid taps accumulate.
    target_yaw: f32,
    /// When set, `drive_camera` glides `focus` toward this ground point (eased on real time, so it
    /// works while a conversation freezes the sim) and clears it on arrival. **Any manual pan input
    /// clears it immediately** — the glide never fights the player. Writers: the dialogue runtime
    /// (frame the current speaker — the player-requested "move the view to the dialog, don't jar")
    /// and `Action::CameraRecenter`.
    pub glide_to: Option<Vec3>,
}

/// Published each frame for the dungeon's view-relative wall cutaway. `to_camera` is the horizontal
/// direction from the focus toward the camera (the yawed iso diagonal); a wall's inner face is toward
/// the camera — and so occludes the room and should be squashed — when its outward normal has a
/// positive dot with this. Only the per-axis sign matters at the 90° detents, but it's kept continuous
/// so the cutaway can ease across a turn (see `dungeon::update_cutaway`).
///
/// `viewport_height` is the orthographic `ScalingMode::FixedVertical` extent — the vertical span of world
/// the window shows. Because the projection is orthographic, world→screen scale is constant with depth, so
/// this single number converts world units to visual angle. `mycelia::perceptual` uses it to hold the
/// mold's growth just under the human motion-detection threshold at whatever zoom the player is actually at.
#[derive(Resource)]
pub struct CameraView {
    pub to_camera: Vec3,
    pub viewport_height: f32,
}

impl Default for CameraView {
    fn default() -> Self {
        // Seeded to the startup zoom, not 0.0: this resource is readable before `setup_camera` runs, and a
        // zero viewport height would divide the perceptual speed budget down to nothing.
        Self { to_camera: Vec3::ZERO, viewport_height: VIEWPORT_HEIGHT }
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        // `drive_camera` reads the binding table non-optionally; the plugin that registers a reader
        // is what guarantees the resource exists (see `input::claim_bindings`).
        crate::input::claim_bindings(app);
        app.insert_resource(CameraRig {
            focus: Vec3::ZERO,
            height: VIEWPORT_HEIGHT,
            yaw: 0.0,
            target_yaw: 0.0,
            glide_to: None,
        })
        .init_resource::<CameraView>()
        // The camera OUTLIVES a run — the title screen needs one, and `DespawnOnExit` would take it
        // away between expeditions. So it is split rather than made run-scoped (FVS-A-5): the entity is
        // spawned once on `Startup`, and `focus_camera_on_spawn` re-aims it at each new dungeon.
        .add_systems(Startup, setup_camera)
        .add_systems(
            OnEnter(crate::session::RunState::Active),
            focus_camera_on_spawn.in_set(crate::session::RunBuild::Populate),
        )
        // Read `SimBlocked` only after its sole writer has settled this frame, so opening/closing a
        // menu never leaks or drops a frame of pan. (No-op in the headless harness, where
        // `sync_sim_blocked` isn't registered — an `.after` on an absent system is simply ignored.)
        .add_systems(
            Update,
            drive_camera.after(crate::ui::state::sync_sim_blocked),
            );
    }
}

/// One frame of a `glide_to` focus pull: returns the new focus and whether it has arrived (at which
/// point the caller clears the goal). Split out of [`drive_camera`] so the "never jars the player"
/// property is unit-testable without an `App` — the ease is frame-rate independent, so the path taken
/// is the same at 30 fps and 240 fps, and no single frame can teleport more than the remaining
/// distance.
pub fn glide_step(focus: Vec3, goal: Vec3, dt: f32) -> (Vec3, bool) {
    let delta = goal - focus;
    if delta.length() <= GLIDE_SNAP {
        return (goal, true);
    }
    let ease = 1.0 - (-GLIDE_SMOOTHING * dt).exp();
    (focus + delta * ease, false)
}

/// Smooth pseudo-noise in `[-1, 1]` from two detuned sines — a cheap Perlin stand-in for shake so the
/// motion shudders smoothly instead of jittering per frame. `seed` decorrelates the axes.
fn shake_noise(t: f32, seed: f32) -> f32 {
    (t * 37.0 + seed).sin() * 0.6 + (t * 91.0 + seed * 2.3).sin() * 0.4
}

/// Spawn the one persistent camera. Deliberately reads no `Dungeon`: it is created before any world
/// exists (see the plugin note), and [`focus_camera_on_spawn`] aims it once there is one.
fn setup_camera(
    mut commands: Commands,
    rig: Res<CameraRig>,
    mut view: ResMut<CameraView>,
    config: Res<GameConfig>,
    mut images: ResMut<Assets<Image>>,
) {
    // yaw = 0 ⇒ camera looks from (+X,+Z); seed the cutaway so the E/S near walls are already knee-high
    // on the first rendered frame (no startup squash animation).
    view.to_camera = Vec3::new(1.0, 0.0, 1.0);
    view.viewport_height = rig.height;
    let cfg = &config.lighting;
    // The ambient path. `crate::world` owns what the light *is* (and documents why it is an environment
    // map rather than a flat fill); Bevy requires the component ride the camera, so it is attached here.
    let env_map = images.add(crate::world::interior_env_cubemap(cfg));
    commands.spawn((
        Camera3d::default(),
        // The one camera every gameplay system means. See `crate::MainCamera`.
        crate::MainCamera,
        // The one camera mesh picking is allowed to cast from. Required because
        // `dialogue::plugin` sets `MeshPickingSettings::require_markers = true` — see there for the
        // bug that forced it (a decorative light shaft was eating every click on the dialogue
        // choices, and picking is opt-out by default).
        bevy::picking::mesh_picking::MeshPickingCamera,
        // Which camera the transform gizmo casts through (dev-only `site_editor`, F7). Bevy makes the
        // marker optional when exactly one camera exists, which is true today — it is written down
        // anyway for the same reason `MeshPickingCamera` above is: a second camera added later would
        // otherwise make the gizmo silently pick the wrong one, and a marker is cheaper to read than
        // that bug is to find.
        bevy::gizmos::transform_gizmo::TransformGizmoCamera,
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: rig.height,
            },
            ..OrthographicProjection::default_3d()
        }),
        // The camera was LDR, and that quietly capped the whole emissive layer: TonyMcMapface clips
        // anything much above mid-grey straight to white, so `fixture_emissive`, the mycelia glow, the TV
        // static and the laser bolts were each tuned *down* to stay under it and none of them could read
        // as brighter than a sheet of paper. With `Hdr` they can, and `Bloom` is what turns that headroom
        // into a visible halo around a lit tube.
        Hdr,
        Bloom { intensity: cfg.bloom_intensity, ..Bloom::NATURAL },
        // The one shadow-casting light is the directional key (`world::setup_lighting`). `light.rs`'s
        // note on the TV spotlight already diagnosed staircase artifacts and named filtering as a lever
        // that was never actually pulled; this pulls it for every shadow in the game at once.
        ShadowFilteringMethod::Gaussian,
        GeneratedEnvironmentMapLight {
            environment_map: env_map,
            intensity: cfg.env_brightness,
            ..default()
        },
        Transform::from_translation(rig.focus + ISO_OFFSET).looking_at(rig.focus, Vec3::Y),
    ));
}

impl CameraRig {
    /// The orthographic viewport height — how many world units tall the window shows.
    pub fn zoom(&self) -> f32 {
        self.height
    }

    /// Set the zoom, clamped to the same range the mouse wheel uses.
    ///
    /// Clamped rather than free so no caller can park the camera somewhere the player cannot get
    /// back from with the wheel, and so `mycelia::perceptual`'s speed limit — which is a function of
    /// this number — keeps the bounds its own tests assert.
    pub fn set_zoom(&mut self, height: f32) {
        self.height = height.clamp(MIN_ZOOM, MAX_ZOOM);
    }
}

/// Point the camera at `focus` immediately, without easing. For entering a *different place* (the Site,
/// a run) rather than for per-frame motion.
pub fn snap_camera_to(focus: Vec3, rig: &mut CameraRig, cams: &mut Query<&mut Transform, With<crate::MainCamera>>) {
    rig.focus = focus;
    // A glide aimed in the previous place (a conversation mid-flight when the run ended, say) must
    // not drag the camera back across the new one.
    rig.glide_to = None;
    for mut t in cams.iter_mut() {
        *t = Transform::from_translation(focus + ISO_OFFSET).looking_at(focus, Vec3::Y);
    }
}

fn focus_camera_on_spawn(
    dungeon: Res<Dungeon>,
    mut rig: ResMut<CameraRig>,
    mut cams: Query<&mut Transform, With<crate::MainCamera>>,
) {
    snap_camera_to(dungeon.spawn_world(), &mut rig, &mut cams);
}

fn drive_camera(
    // `time` (virtual) drives only the gameplay-feel screen shake below; the human camera controls
    // (pan and rotate) run on `real` so they feel identical at any game speed — including paused.
    time: Res<Time>,
    real: Res<Time<Real>>,
    actions: crate::input::Actions,
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
    // The squad's eased centroid, already computed every fixed tick for the cohesion leash. `Option`
    // because it is a squad-AI resource and the camera outlives any run (title screen, Site).
    anchor: Option<Res<crate::squad_ai::cohesion::SquadAnchor>>,
    // Inert `false` in the harness (nothing writes it there), exactly like `SimBlocked` above.
    orders_blocked: Res<crate::time_control::OrdersBlocked>,
    mut rig: ResMut<CameraRig>,
    mut view: ResMut<CameraView>,
    camera: Single<(&mut Transform, &mut Projection), With<crate::MainCamera>>,
) {
    let allow_pan = !sim_blocked.0;

    // Recentre on the squad. The rig deliberately follows nothing — that is what makes it an RTS
    // camera rather than a third-person one — but "follows nothing" and "cannot find them again"
    // are different things, and only the first is a design choice. Routed through the glide so it
    // is a smooth pull rather than a teleport (writing `rig.focus` directly WAS a teleport — the
    // transform below is rebuilt from it the same frame).
    //
    // Gated on `orders_allowed` for the same reason `selection`'s input is, and it is a rule rather
    // than a technicality: **while you are at Site-67 you cannot look at your squad.** Before
    // `input::Action::VisitSite` this branch no-opped there for free — the run was `Idle`, so no squad
    // existed and `anchor.valid` was false. During a visit the anchor IS valid, so without this the
    // key would glide the camera 512+ units back to the dungeon with the Site HUD still up, and hand
    // the player a way to supervise the expedition they are supposed to have left unattended.
    if actions.just_pressed(Action::CameraRecenter) && !orders_blocked.0 {
        if let Some(anchor) = anchor.filter(|a| a.valid) {
            rig.glide_to = Some(anchor.pos);
        }
    }

    if scroll.delta.y != 0.0 {
        rig.height = (rig.height - scroll.delta.y * ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    // Q/E rotate the whole view in discrete detents around the focus. Each press snaps the goal by
    // one step; the current yaw eases toward it below, so rapid taps stack and the camera smoothly
    // chases the accumulated target. Q turns counter-clockwise (from above), E clockwise.
    let step = TAU / ROTATION_STEPS as f32;
    if actions.just_pressed(Action::CameraRotateLeft) {
        rig.target_yaw += step;
    }
    if actions.just_pressed(Action::CameraRotateRight) {
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
    // Publish the horizontal camera direction (the yawed iso diagonal) for the wall cutaway, and the
    // orthographic viewport height (the mold's perceptual speed budget scales with it).
    view.to_camera = yaw_rot * Vec3::new(1.0, 0.0, 1.0);
    view.viewport_height = rig.height;

    // Pan on REAL time, not the sim clock: keyboard panning must feel the same at ×1, ×64, or paused.
    // (Reading the generic `Time` here would resolve to `Time<Virtual>` and scale pan speed with the
    // game-speed multiplier — flying at high speed, dead when paused. Zoom/drag below already use raw
    // per-frame input deltas, so they're speed-independent without needing `dt`.)
    let dt = real.delta_secs();
    // WASD (and arrow keys) scroll the map along the screen axes — unless a menu is open, in which
    // case those keys belong to menu navigation.
    let mut pan = Vec3::ZERO;
    if allow_pan {
        if actions.pressed(Action::CameraPanForward) {
            pan += screen_forward;
        }
        if actions.pressed(Action::CameraPanBack) {
            pan -= screen_forward;
        }
        if actions.pressed(Action::CameraPanRight) {
            pan += screen_right;
        }
        if actions.pressed(Action::CameraPanLeft) {
            pan -= screen_right;
        }
    }
    if let Some(dir) = pan.try_normalize() {
        rig.focus += dir * PAN_SPEED * dt;
        rig.glide_to = None; // manual input wins instantly — the glide never fights the player
    }
    // Middle-mouse drag to pull the map around.
    if mouse_buttons.pressed(MouseButton::Middle) {
        let d = mouse_motion.delta;
        if d != Vec2::ZERO {
            rig.focus += (-d.x * screen_right + d.y * screen_forward) * DRAG_SCALE;
            rig.glide_to = None;
        }
    }

    // Glide toward a requested framing (dialog speaker, recenter). On REAL time: the dialogue
    // overlay freezes the virtual clock, and gliding to a conversation is this feature's whole
    // point (player capture 2026-07-31: "move the game window where the dialog is happening —
    // don't jar the player, though").
    if let Some(goal) = rig.glide_to {
        let (next, arrived) = glide_step(rig.focus, goal, dt);
        rig.focus = next;
        if arrived {
            rig.glide_to = None;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The dialog glide must *never jar the player* (the request that motivated it): it approaches
    /// monotonically, never overshoots, and no single frame jumps the whole gap.
    #[test]
    fn a_glide_approaches_without_teleporting_or_overshooting() {
        let goal = Vec3::new(40.0, 0.0, 30.0);
        let mut focus = Vec3::ZERO;
        let start_dist = (goal - focus).length();
        let dt = 1.0 / 60.0;

        let (first, arrived) = glide_step(focus, goal, dt);
        assert!(!arrived, "a half-map glide cannot complete in one frame");
        let first_step = (first - focus).length();
        assert!(
            first_step < start_dist * 0.5,
            "one frame moved {first_step} of {start_dist} — that is a teleport, not a glide"
        );

        // Monotone approach, and it does terminate.
        let mut prev = start_dist;
        let mut frames = 0;
        loop {
            let (next, done) = glide_step(focus, goal, dt);
            let d = (goal - next).length();
            assert!(d <= prev + 1e-6, "glide moved away from its goal ({prev} -> {d})");
            prev = d;
            focus = next;
            frames += 1;
            if done {
                break;
            }
            assert!(frames < 10_000, "glide never converged");
        }
        assert_eq!(focus, goal, "an arrived glide lands EXACTLY on target, so the caller can clear it");
        // ~4.0/s ease over 40 units: well under a second of travel, not an instant cut.
        assert!((30..600).contains(&frames), "glide took {frames} frames at 60fps — check GLIDE_SMOOTHING");
    }

    /// Frame-rate independence (Holmér 2023): the same elapsed time travels the same distance whether
    /// it arrives as one long frame or several short ones. Without this the camera would drift
    /// further per second on a fast machine — exactly the class of bug the yaw ease already avoids.
    #[test]
    fn glide_travel_does_not_depend_on_frame_rate() {
        let goal = Vec3::new(20.0, 0.0, 0.0);
        let far = {
            let mut f = Vec3::ZERO;
            for _ in 0..4 {
                f = glide_step(f, goal, 1.0 / 240.0).0;
            }
            f
        };
        let near = glide_step(Vec3::ZERO, goal, 1.0 / 60.0).0;
        assert!(
            (far - near).length() < 0.05,
            "4 frames at 240fps ({far:?}) must match 1 frame at 60fps ({near:?})"
        );
    }

    /// An already-framed speaker must not restart the ease — it reports arrival immediately.
    #[test]
    fn a_glide_to_where_we_already_are_is_already_done() {
        let at = Vec3::new(5.0, 0.0, -3.0);
        let (next, arrived) = glide_step(at, at, 1.0 / 60.0);
        assert!(arrived);
        assert_eq!(next, at);
    }
}
