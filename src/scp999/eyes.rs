//! SCP-999's two big darling eyes — the procedural WGSL eye material (`assets/shaders/scp999_eyes.wgsl`)
//! on a camera-facing billboard quad, plus the systems that attach it and drive its glance/blink each
//! frame. This mirrors the smiley enemy's face pattern (`enemy::SmileyMaterial` + `update_smiley_faces`):
//! a fragment-only `Material` with coverage-as-alpha (`AlphaMode::Blend`), billboarded to the fixed iso
//! camera, biased toward the camera along its view axis so the eyes draw *in front of* the translucent
//! gel. Cosmetic + windowed-only (registered by `Scp999VisualsPlugin`); writes only a child `Transform` +
//! a material uniform, so it never touches hashed `(Transform, Health)` state.

use bevy::pbr::Material;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use super::{BlobJiggle, Scp999, Scp999Motion};
use crate::util::hash01_u32;

// Eye-quad size + placement, scaled off the gel's render scale so they track the dome as it shrinks/grows.
/// World size of the eye billboard quad (spans the two eyes across the front of the dome).
const EYE_QUAD_SIZE: f32 = super::RENDER_SCALE * 1.9;
/// Height up the dome (~2/3 of its height) where the eyes sit.
const EYE_UP: f32 = super::RENDER_SCALE * 0.95;
/// How far out along the *camera-facing horizontal* to seat the eyes — onto the dome's NEAR hemisphere, so
/// the dome bulk is always BEHIND them and can never occlude them, and (because it recomputes from the live
/// camera each frame) they orbit to the front as the view yaws (Q/E). This is the "eyes spin with it,
/// don't vanish when turned" fix — the old fixed-front seat sank behind the fat dome at some yaws.
const EYE_FRONT: f32 = super::RENDER_SCALE * 1.15;
/// Small extra bias straight toward the camera so the eye plane clears the gel surface without z-fighting
/// (pure depth shift under the orthographic iso camera — on-screen position unchanged; as `enemy`'s face).
const EYE_DEPTH_BIAS: f32 = super::RENDER_SCALE * 0.6;
/// Glance strength: how far the irises swing toward the comforted member (face-space, ~[-0.4, 0.4]).
const LOOK_AMOUNT: f32 = 0.34;
/// Blink cadence: a short triangle-pulse blink once per period, phased per blob so they don't blink in sync.
const BLINK_PERIOD: f32 = 3.7;
const BLINK_DUR: f32 = 0.16;

// Eye bounce — each eye is a detuned 2D spring that tracks a scaled version of the body's jiggle, so both
// eyes bob WITH the deformation but at different rates, never in lockstep (the "independent" the design asks).
/// Maps the body's gross deform (lateral, vertical) → eye-space offset. Small: the eyes bob, not fly off.
const EYE_BOB_GAIN: Vec2 = Vec2::new(0.05, 0.06);
/// The two eyes' spring frequencies (rad/s), deliberately detuned so left/right desynchronise.
const EYE_BOB_OMEGA_L: f32 = 33.0;
const EYE_BOB_OMEGA_R: f32 = 41.0;
/// Low damping → the eyes overshoot + jiggle (googly), not glide.
const EYE_BOB_ZETA: f32 = 0.28;

/// GPU uniform — mirrors `Scp999EyesUniform` in `scp999_eyes.wgsl` (field order + types MUST match; the
/// layout is `vec2 + f32 + f32` = 16 B, so no padding is needed).
#[derive(Clone, ShaderType)]
struct Scp999EyesUniform {
    look: Vec2,
    /// Per-eye bounce offset (eye-space); each tracks the body jiggle on its own detuned spring.
    bob_l: Vec2,
    bob_r: Vec2,
    blink: f32,
    joy: f32,
}

/// The eye billboard's fragment-only material.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct Scp999EyesMaterial {
    #[uniform(0)]
    settings: Scp999EyesUniform,
}

impl Material for Scp999EyesMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/scp999_eyes.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        // Coverage-as-alpha: the eyes composite over the gel/scene, the square quad's corners vanish.
        AlphaMode::Blend
    }
}

/// Marks a blob that already has its eye child (so [`attach_scp999_eyes`] runs once per blob).
#[derive(Component)]
pub(crate) struct Scp999EyesAttached;

/// The eye billboard child; `phase` decorrelates its blink timer, and the two detuned bob springs give
/// each eye its own independent bounce that tracks the body jiggle.
#[derive(Component)]
pub(crate) struct Scp999Eyes {
    phase: f32,
    bob_l: Vec2,
    vel_l: Vec2,
    bob_r: Vec2,
    vel_r: Vec2,
}

/// Give every blob without eyes yet a billboard eye-quad child. Runs on `Update` (windowed-only), so it
/// also fits F6 dev-spawned blobs; the material asset only exists here (the `MaterialPlugin` is
/// windowed-only), which is exactly why the eyes are attached in the cosmetic plugin, not at gameplay spawn.
pub(crate) fn attach_scp999_eyes(
    mut commands: Commands,
    blobs: Query<(Entity, &super::Scp999Seed), (With<Scp999>, Without<Scp999EyesAttached>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<Scp999EyesMaterial>>,
) {
    for (blob, seed) in &blobs {
        let quad = meshes.add(Rectangle::new(EYE_QUAD_SIZE, EYE_QUAD_SIZE));
        let material = materials.add(Scp999EyesMaterial {
            settings: Scp999EyesUniform {
                look: Vec2::ZERO,
                bob_l: Vec2::ZERO,
                bob_r: Vec2::ZERO,
                blink: 0.0,
                joy: 0.55,
            },
        });
        // Phase from the blob's OWN birth seed, not an attach-order counter: this system wires blobs in
        // whatever order their entities are yielded once they exist, so a counter numbered them by a
        // per-run quantity and two runs of one seed gave the same blob different blink phases. The seed is
        // the number `Scp999Seq` exists to provide (see `Scp999Seed`).
        let phase = hash01_u32(seed.0) * BLINK_PERIOD;
        commands.entity(blob).insert(Scp999EyesAttached).with_children(|p| {
            p.spawn((
                Scp999Eyes {
                    phase,
                    bob_l: Vec2::ZERO,
                    vel_l: Vec2::ZERO,
                    bob_r: Vec2::ZERO,
                    vel_r: Vec2::ZERO,
                },
                Mesh3d(quad),
                MeshMaterial3d(material),
                // Placeholder seat; `update_scp999_eyes` reseats it on the camera-facing side every frame.
                Transform::from_translation(Vec3::Y * EYE_UP),
                Visibility::Inherited,
            ));
        });
    }
}

/// Billboard each blob's eye quad toward the iso camera, glance the irises toward the comforted member, and
/// drive the blink + joy. Reads the gameplay-written [`Scp999Motion`]. Cosmetic → `Update`; skipped headless
/// (the `Single<Camera3d>` param finds no camera).
/// One substepped step of a detuned 2D damped spring toward `target` (the eye-bounce integrator) — the
/// two axes are independent oscillators, so this is [`super::jiggle::step_damped`] applied per component
/// rather than a second hand-rolled copy of the same integration.
fn step_eye_bob(x: &mut Vec2, v: &mut Vec2, target: Vec2, omega: f32, zeta: f32, dt: f32) {
    const SUBSTEPS: u32 = 4;
    super::jiggle::step_damped(&mut x.x, &mut v.x, target.x, omega, zeta, dt, SUBSTEPS);
    super::jiggle::step_damped(&mut x.y, &mut v.y, target.y, omega, zeta, dt, SUBSTEPS);
}

#[allow(clippy::type_complexity)]
pub(crate) fn update_scp999_eyes(
    camera: Single<&GlobalTransform, With<Camera3d>>,
    time: Res<Time>,
    blobs: Query<(&GlobalTransform, &Scp999Motion, &BlobJiggle, &Children), With<Scp999>>,
    mut eyes: Query<(&mut Transform, &mut Scp999Eyes, &MeshMaterial3d<Scp999EyesMaterial>), Without<Scp999>>,
    mut mats: ResMut<Assets<Scp999EyesMaterial>>,
) {
    let cam_rot = camera.rotation();
    // Face-space axes: the quad takes the camera rotation, so its local right/up are the camera's — project
    // the world glance onto them to get the shader `look` (exactly `update_smiley_faces`).
    let right = camera.right();
    let up = camera.up();
    let cam_back = camera.back();
    let cam_pos = camera.translation();
    let elapsed = time.elapsed_secs();
    let dt = time.delta_secs().min(super::MAX_FRAME_DT);

    for (gxf, motion, jiggle, children) in &blobs {
        // Seat the eyes on the dome's camera-facing near hemisphere (+ up + a small depth bias), recomputed
        // from the live camera so they orbit to the front as the view yaws and the dome never occludes them.
        // The root is unrotated (identity), so this world offset is also the child's local translation.
        let to_cam = cam_pos - gxf.translation();
        let horiz = Vec3::new(to_cam.x, 0.0, to_cam.z).normalize_or_zero();
        let eye_pos = Vec3::Y * EYE_UP + horiz * EYE_FRONT + cam_back * EYE_DEPTH_BIAS;

        // Glance toward the comforted member (only when it actually has one — otherwise gaze forward/idle).
        let look = match motion.target {
            Some(_) => {
                let to = motion.gaze - gxf.translation();
                Vec2::new(to.dot(*right), to.dot(*up)).normalize_or_zero() * LOOK_AMOUNT
            }
            None => Vec2::ZERO,
        };
        // Delighted mid-tickle (big pupils + brighter glint); a warm baseline otherwise.
        let joy = if motion.tickling { 1.0 } else { 0.55 };
        // The bounce target: the body's gross deform scaled into eye-space. Both eyes chase it, on detuned
        // springs, so they bob WITH the jiggle but never in lockstep (independent).
        let target = jiggle.bounce() * EYE_BOB_GAIN;

        for &child in children {
            if let Ok((mut etf, mut eye, mat)) = eyes.get_mut(child) {
                etf.rotation = cam_rot; // billboard
                etf.translation = eye_pos; // camera-facing near hemisphere + depth bias (recomputed each frame)
                // Blink: a short triangle pulse (0→1→0) at the tail of each phased period.
                let tt = (elapsed + eye.phase).rem_euclid(BLINK_PERIOD);
                let blink = if tt > BLINK_PERIOD - BLINK_DUR {
                    let k = (tt - (BLINK_PERIOD - BLINK_DUR)) / BLINK_DUR;
                    1.0 - (2.0 * k - 1.0).abs()
                } else {
                    0.0
                };
                // Advance the two detuned eye-bounce springs (copy out → step → write back; the fields are
                // Copy `Vec2`, and disjoint mutable borrows through the `Mut` deref are awkward otherwise).
                let (mut bl, mut vl, mut br, mut vr) = (eye.bob_l, eye.vel_l, eye.bob_r, eye.vel_r);
                step_eye_bob(&mut bl, &mut vl, target, EYE_BOB_OMEGA_L, EYE_BOB_ZETA, dt);
                step_eye_bob(&mut br, &mut vr, target, EYE_BOB_OMEGA_R, EYE_BOB_ZETA, dt);
                eye.bob_l = bl;
                eye.vel_l = vl;
                eye.bob_r = br;
                eye.vel_r = vr;
                if let Some(mut m) = mats.get_mut(&mat.0) {
                    m.settings.look = look;
                    m.settings.bob_l = bl;
                    m.settings.bob_r = br;
                    m.settings.blink = blink;
                    m.settings.joy = joy;
                }
            }
        }
    }
}
