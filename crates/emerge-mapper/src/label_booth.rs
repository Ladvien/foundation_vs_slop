//! **The labeling booth** — two 640 px renders of one asset, for the VLM's eyes.
//!
//! The thumbnail booth photographs every piece once at 128 px for the palette; that is too small
//! to judge from. This booth serves `labels.rs`: stage one subject at its own far corner, take a
//! three-quarter front and a three-quarter rear view at [`SHOT_PX`], read both back off the GPU,
//! and hand the raw images to whoever queued the job. It reuses the thumbnail booth's staging,
//! lighting, readiness gates and framing (`thumbs::{stage_mesh, erect_booth, subject_bounds,
//! has_mesh}`) so there is exactly one photographic truth in this editor — and it inherits both of
//! that module's paid-for traps: a `Mesh3d` component is not a drawable mesh, and the camera never
//! stays aimed at a finished shot with nothing staged (it parks on [`ShotRig::scratch`]).
//!
//! One trap of its own: **capture is asynchronous.** `Screenshot::image(..)` copies the target
//! texture out over the next frame(s), so the camera must not re-aim at angle B until angle A's
//! `ScreenshotCaptured` has actually landed — re-aiming early photographs B into A's slot. The
//! state machine waits in [`Phase::AwaitCapture`] for exactly that reason, frame-bounded so a
//! wedged GPU path drops the job loudly instead of hanging the queue.
//!
//! Headless (`backends: None`) this module is inert by design: nothing runs until a job is
//! queued, and the harness never queues one — captures need a real adapter. The decision-shaped
//! parts (angles, framing) are pure functions with GPU-free tests.

use bevy::camera::primitives::Aabb;
use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget, ScalingMode};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

use crate::thumbs::{erect_booth, has_mesh, stage_mesh, subject_bounds, Subject};
use crate::tiles::EditTarget;

/// The labeling corner — the fourth one. `thumbs::BOOTH` owns `(4096, 0, 4096)`, `tiles::STAGE`
/// owns `(-4096, 0, 4096)`, the anim bench owns `(-4096, 0, -4096)`; no two subjects are ever in
/// shot together and the two booths never contend for a camera.
const LAB_BOOTH: Vec3 = crate::stages::LABEL_BOOTH;

/// Shot edge in pixels. Big enough for a VLM to judge shape and parts, small enough that two of
/// them base64 into a request the local endpoint takes without complaint.
pub const SHOT_PX: u32 = 640;

/// Frames to hold after aiming before the shot is taken — the thumbnail booth's settle, for the
/// same render-target-change-lands-late reason.
const SETTLE_FRAMES: u32 = 4;

/// Camera distance as a multiple of the subject's largest dimension — `thumbs::FRAMING`'s value;
/// restated because that one is private and a labeling shot wants the same proven framing.
const FRAMING: f32 = 1.7;

/// How long to wait for a staged GLB before dropping the job. One unloadable asset must not stall
/// a 450-item batch.
const MESH_WAIT_FRAMES: u32 = 600;

/// How long to wait for a capture to land. A capture is a texture copy plus a channel hop — a
/// second of frames is generous, and past it the GPU path is wedged and the job drops loudly.
const CAPTURE_WAIT_FRAMES: u32 = 120;

/// The same opaque backdrop the thumbnails use — the model should see the asset the author sees.
/// Through chrome, so "the same" is a fact the compiler holds rather than a promise.
const BACKDROP: Color = crate::chrome::SLOT_BG;

/// The two view directions, as offsets per unit extent: the palette's proven three-quarter front,
/// and its mirror — the far side a single thumbnail never shows.
pub(crate) fn angle_offset(angle: usize) -> Vec3 {
    match angle {
        0 => Vec3::new(1.0, 0.85, 1.0),
        _ => Vec3::new(-1.0, 0.85, -1.0),
    }
}

/// Where the camera stands and how tall its viewport is, for one angle over a measured subject.
pub(crate) fn aim(centre: Vec3, extent: f32, angle: usize) -> (Transform, f32) {
    let tf = Transform::from_translation(centre + angle_offset(angle) * extent * FRAMING)
        .looking_at(centre, Vec3::Y);
    (tf, extent * 1.35)
}

/// The labeling camera. Every other camera query filters positively on `view::MainCamera`, which
/// is what keeps a second render-target camera from breaking them — the thumbnail booth's own
/// argument, at `order: -2` so the two booths never race each other's pass.
#[derive(Component)]
pub struct LabelCamera;

/// One capture job: whose labels these shots are for.
#[derive(Clone, Debug)]
pub struct ShotJob {
    pub target: EditTarget,
    pub mesh: String,
    pub scale: f32,
}

/// Both renders of one subject, raw off the GPU — PNG encoding belongs to the async task that
/// ships them, not to a frame system.
#[derive(Message)]
pub struct ShotsReady {
    pub target: EditTarget,
    pub mesh: String,
    pub images: [Image; 2],
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    /// Waiting for the staged GLB to become drawable.
    Staging,
    /// Aimed at `angle`, holding for the render target to carry a real frame.
    Settling { angle: usize, held: u32 },
    /// Shot requested for `angle`; waiting for its `ScreenshotCaptured` to land.
    AwaitCapture { angle: usize, waited: u32 },
}

/// The booth's whole state — a serial queue (the booth photographs one subject at a time, and the
/// local endpoint runs one request at a time anyway) plus the state machine over it.
#[derive(Resource, Default)]
pub struct ShotRig {
    queue: std::collections::VecDeque<ShotJob>,
    phase: Phase,
    /// Render targets for the two angles, and the scratch the camera parks on. Created at setup;
    /// `None` only in a world without image assets (the headless harness), where no job can run.
    targets: Option<[Handle<Image>; 2]>,
    scratch: Option<Handle<Image>>,
    shots: [Option<Image>; 2],
    waited: u32,
    model: Option<Entity>,
    camera: Option<Entity>,
    booth: Option<Entity>,
}

impl ShotRig {
    /// Queue a job unless the same target is already queued or being photographed.
    pub fn push_unique(&mut self, job: ShotJob) {
        let same_target = |t: &EditTarget| *t == job.target;
        if self.queue.iter().any(|j| same_target(&j.target)) {
            return;
        }
        self.queue.push_back(job);
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.phase, Phase::Idle) && self.queue.is_empty()
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Drop everything not yet photographed. The in-flight subject finishes — half a photo shoot
    /// is worth completing, and the state machine's teardown handles the rest.
    pub fn clear_queue(&mut self) -> usize {
        let dropped = self.queue.len();
        self.queue.clear();
        dropped
    }
}

pub struct LabelBoothPlugin;

impl Plugin for LabelBoothPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShotRig>()
            .add_message::<ShotsReady>()
            .add_systems(OnEnter(crate::screen::Screen::Editor), setup)
            .add_systems(Update,
                (drive_shots)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            );
    }
}

/// A [`SHOT_PX`] render target the screenshot pass can copy OUT of — `new_target_texture` sets
/// `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_DST` but not `COPY_SRC`, and the capture is a
/// texture-to-buffer copy with this image as the source.
fn shot_target(images: &mut Assets<Image>) -> Handle<Image> {
    let mut img = Image::new_target_texture(SHOT_PX, SHOT_PX, TextureFormat::Rgba8UnormSrgb, None);
    img.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    img.data = Some(vec![0; (SHOT_PX * SHOT_PX * 4) as usize]);
    images.add(img)
}

/// `Option<ResMut<Assets<Image>>>`: the headless harness has no render half, and a bare `ResMut`
/// would panic the system there.
fn setup(mut rig: ResMut<ShotRig>, images: Option<ResMut<Assets<Image>>>) {
    let Some(mut images) = images else { return };
    rig.targets = Some([shot_target(&mut images), shot_target(&mut images)]);
    rig.scratch = Some(shot_target(&mut images));
}

/// Park the camera on the scratch target — never left aimed at a finished shot (the thumbnail
/// booth's trap, same reason).
fn park(
    rig: &ShotRig,
    cams: &mut Query<
        (&mut Transform, &mut Projection, &mut RenderTarget),
        With<LabelCamera>,
    >,
) {
    let (Some(cam), Some(scratch)) = (rig.camera, rig.scratch.as_ref()) else {
        return;
    };
    if let Ok((_, _, mut target)) = cams.get_mut(cam) {
        *target = RenderTarget::Image(ImageRenderTarget {
            handle: scratch.clone(),
            scale_factor: 1.0,
        });
    }
}

/// Drop the current job — loudly — and reset for the next.
fn abandon(
    why: &str,
    commands: &mut Commands,
    rig: &mut ShotRig,
    cams: &mut Query<
        (&mut Transform, &mut Projection, &mut RenderTarget),
        With<LabelCamera>,
    >,
) {
    if let Some(job) = rig.queue.front() {
        warn!("labeling shot of `{}` abandoned: {why}", job.mesh);
    }
    park(rig, cams);
    if let Some(model) = rig.model.take() {
        commands.entity(model).despawn();
    }
    rig.queue.pop_front();
    rig.shots = [None, None];
    rig.waited = 0;
    rig.phase = Phase::Idle;
}

#[allow(clippy::too_many_arguments)]
fn drive_shots(
    mut commands: Commands,
    mut rig: ResMut<ShotRig>,
    assets: Res<AssetServer>,
    children: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    subjects: Query<&Subject>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
    mut cams: Query<(&mut Transform, &mut Projection, &mut RenderTarget), With<LabelCamera>>,
    mut ready: MessageWriter<ShotsReady>,
) {
    // Idle with an empty queue: bring the booth down (and its always-rendering camera with it).
    if matches!(rig.phase, Phase::Idle) && rig.queue.is_empty() {
        if let Some(cam) = rig.camera.take() {
            commands.entity(cam).despawn();
        }
        if let Some(booth) = rig.booth.take() {
            commands.entity(booth).despawn();
        }
        return;
    }

    let rig = &mut *rig;
    match rig.phase {
        Phase::Idle => {
            // A queued job, and targets exist (a world without image assets can never run one).
            let Some(job) = rig.queue.front().cloned() else { return };
            if rig.targets.is_none() || rig.scratch.is_none() {
                abandon("no render targets in this world", &mut commands, rig, &mut cams);
                return;
            }
            if rig.booth.is_none() {
                rig.booth = Some(erect_booth(&mut commands, LAB_BOOTH));
            }
            if rig.camera.is_none() {
                let Some(scratch) = rig.scratch.clone() else { return };
                rig.camera = Some(spawn_camera(&mut commands, &scratch));
            }
            rig.model = Some(stage_mesh(
                &mut commands,
                &assets,
                &job.mesh,
                job.scale,
                LAB_BOOTH,
            ));
            rig.shots = [None, None];
            rig.waited = 0;
            rig.phase = Phase::Staging;
        }
        Phase::Staging => {
            let Some(model) = rig.model else {
                abandon("the staged model vanished", &mut commands, rig, &mut cams);
                return;
            };
            let ready_to_draw = subjects
                .get(model)
                .is_ok_and(|s| assets.is_loaded_with_dependencies(&s.0))
                && has_mesh(model, &children, &meshes);
            if !ready_to_draw {
                rig.waited += 1;
                if rig.waited > MESH_WAIT_FRAMES {
                    abandon(
                        "no drawable mesh within the wait budget",
                        &mut commands,
                        rig,
                        &mut cams,
                    );
                }
                return;
            }
            rig.waited = 0;
            rig.phase = Phase::Settling { angle: 0, held: 0 };
        }
        Phase::Settling { angle, held } => {
            let (Some(model), Some(cam), Some(targets)) =
                (rig.model, rig.camera, rig.targets.as_ref())
            else {
                abandon("the booth lost a piece of itself", &mut commands, rig, &mut cams);
                return;
            };
            // **Measure before borrowing the camera.** `abandon` needs `&mut cams`, so the refusal
            // below cannot be written after `cams.get_mut` has handed out `tf`/`proj`/`target`. The
            // measurement needs neither, so it goes first and the ordering is not a workaround.
            let Some((lo, hi)) = subject_bounds(model, &children, &bounds) else {
                // Unreachable behind `ready_to_draw` in `Phase::Staging`: a drawable mesh has an
                // `Aabb`. Loud rather than guessed, because a camera aimed at `LAB_BOOTH` with a
                // 1 m span bakes six plausible pictures of nothing and the labeller reads them as
                // the piece.
                abandon(
                    "the staged mesh measured to nothing",
                    &mut commands,
                    rig,
                    &mut cams,
                );
                return;
            };
            let (centre, extent) = crate::thumbs::aim_and_span(lo, hi);
            let Ok((mut tf, mut proj, mut target)) = cams.get_mut(cam) else {
                return;
            };
            let (aim_tf, viewport) = aim(centre, extent, angle);
            *tf = aim_tf;
            *proj = Projection::from(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: viewport,
                },
                ..OrthographicProjection::default_3d()
            });
            *target = RenderTarget::Image(ImageRenderTarget {
                handle: targets[angle].clone(),
                scale_factor: 1.0,
            });
            if held + 1 <= SETTLE_FRAMES {
                rig.phase = Phase::Settling { angle, held: held + 1 };
                return;
            }
            // The frame is real: ask for the copy, then WAIT for it — re-aiming before the
            // capture lands would photograph the next angle into this slot.
            commands
                .spawn(Screenshot::image(targets[angle].clone()))
                .observe(
                    move |shot: On<ScreenshotCaptured>, mut rig: ResMut<ShotRig>| {
                        rig.shots[angle] = Some(shot.image.clone());
                    },
                );
            rig.phase = Phase::AwaitCapture { angle, waited: 0 };
        }
        Phase::AwaitCapture { angle, waited } => {
            if rig.shots[angle].is_none() {
                if waited + 1 > CAPTURE_WAIT_FRAMES {
                    abandon(
                        "the capture never landed (GPU path wedged?)",
                        &mut commands,
                        rig,
                        &mut cams,
                    );
                    return;
                }
                rig.phase = Phase::AwaitCapture { angle, waited: waited + 1 };
                return;
            }
            if angle == 0 {
                rig.phase = Phase::Settling { angle: 1, held: 0 };
                return;
            }
            // Both shots landed. Park BEFORE despawning the subject (the thumbs trap), then hand
            // the job's images over.
            park(rig, &mut cams);
            if let Some(model) = rig.model.take() {
                commands.entity(model).despawn();
            }
            let (Some(a), Some(b)) = (rig.shots[0].take(), rig.shots[1].take()) else {
                abandon("a landed capture went missing", &mut commands, rig, &mut cams);
                return;
            };
            if let Some(job) = rig.queue.pop_front() {
                ready.write(ShotsReady {
                    target: job.target,
                    mesh: job.mesh,
                    images: [a, b],
                });
            }
            rig.phase = Phase::Idle;
        }
    }
}

fn spawn_camera(commands: &mut Commands, first_target: &Handle<Image>) -> Entity {
    commands
        .spawn((
            Name::new("labeling camera"),
            LabelCamera,
            Camera3d::default(),
            Camera {
                // Before the thumbnail camera's -1, so the two booths' passes never interleave a
                // half-drawn frame into each other's targets.
                order: -2,
                clear_color: ClearColorConfig::Custom(BACKDROP),
                ..default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle: first_target.clone(),
                scale_factor: 1.0,
            }),
            AmbientLight {
                brightness: 900.0,
                ..default()
            },
            Projection::from(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical { viewport_height: 2.0 },
                ..OrthographicProjection::default_3d()
            }),
            Transform::from_translation(LAB_BOOTH + Vec3::splat(3.0)).looking_at(LAB_BOOTH, Vec3::Y),
        ))
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two angles look at the subject from opposite horizontal quadrants at the same height —
    /// front and rear three-quarters, not two of the same view.
    #[test]
    fn the_two_angles_are_horizontal_mirrors() {
        let a = angle_offset(0);
        let b = angle_offset(1);
        assert_eq!(a.y, b.y);
        assert_eq!(Vec3::new(-a.x, a.y, -a.z), b);
    }

    /// Framing is measured-extent-proportional and always looks at the centre.
    #[test]
    fn aim_frames_the_measured_subject() {
        let centre = Vec3::new(4096.0, 0.4, -4096.0);
        for angle in [0, 1] {
            for extent in [0.3_f32, 2.0] {
                let (tf, viewport) = aim(centre, extent, angle);
                assert!((viewport - extent * 1.35).abs() < 1.0e-6);
                let dist = tf.translation.distance(centre);
                // 1e-2, not 1e-4: at the 4 km booth corner an f32 ulp is ~0.0005 m, and a
                // ~5 m distance recovered from ~4096-magnitude coordinates carries that noise.
                assert!(
                    (dist - angle_offset(angle).length() * extent * FRAMING).abs() < 1.0e-2,
                    "distance scales with extent"
                );
                // Looking at the centre: the forward axis points from the camera to the subject.
                let fwd = tf.forward();
                let to_centre = (centre - tf.translation).normalize();
                assert!(fwd.dot(to_centre) > 0.999, "angle {angle} extent {extent}");
            }
        }
    }

    /// The queue refuses a duplicate target and reports counts honestly.
    #[test]
    fn the_queue_is_unique_by_target() {
        let mut rig = ShotRig::default();
        let job = |id: &str| ShotJob {
            target: EditTarget::Library(id.to_owned()),
            mesh: format!("kit/{id}.glb"),
            scale: 1.0,
        };
        rig.push_unique(job("a"));
        rig.push_unique(job("a"));
        rig.push_unique(job("b"));
        assert_eq!(rig.queued(), 2);
        assert_eq!(rig.clear_queue(), 2);
        assert!(rig.is_idle());
    }
}
