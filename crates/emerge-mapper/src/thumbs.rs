//! **Palette previews** — one baked render of every descriptor in the library.
//!
//! A palette of ids tells you what a piece is *called*, which is not the question an author is
//! asking. `crt_a` and `crt_b` differ in a way no name conveys, and a library of hundreds is
//! unusable without pictures.
//!
//! # The photo booth
//!
//! Staging happens at [`BOOTH`], four kilometres from the origin, so the booth camera sees the staged
//! piece and nothing else. No `RenderLayers`, which is where layer-based masking usually goes wrong
//! on GLB scene children. One camera walks the library: stage a piece, wait for it to be genuinely
//! ready, aim, render into that piece's image, move on — then the camera **despawns**, because a live
//! render target costs a full pass every frame forever.
//!
//! # Two traps this was built already knowing about
//!
//! Both were paid for in the game's editor and are the reason this is not a naive port.
//!
//! **A `Mesh3d` component is not a mesh that can be drawn.** The component appears as soon as the
//! scene instantiates; the mesh and material *assets* may still be uploading. So the gate is both
//! signals — the component exists AND `is_loaded_with_dependencies`.
//!
//! **Never leave the camera aimed at a finished thumbnail with nothing staged.** This system runs in
//! `Update`, so a despawn lands before the renderer extracts and the next frame draws an empty booth
//! through whatever image the camera still points at. In the game that silently cleared three
//! specific pieces — whichever ones the following GLB was too slow to repaint — and the log
//! cheerfully reported all 45 baked. Here the camera parks on [`Thumbnails::scratch`] whenever no
//! subject is standing there, so a real image is its target only while its piece is present.

use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget, ScalingMode};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::camera::primitives::Aabb;
use bevy::render::render_resource::TextureFormat;

use crate::project::Project;

/// Where pieces are staged for their portrait — far enough that nothing else can wander into frame.
const BOOTH: Vec3 = Vec3::new(4096.0, 0.0, 4096.0);

/// Thumbnail edge in pixels. Small enough that a few hundred are trivial memory, large enough to
/// tell a stool from a chair.
const THUMB_PX: u32 = 128;

/// Frames to hold a staged piece after it is ready. Wide enough that a render-target change landing a
/// frame late still leaves good frames inside the window.
const SETTLE_FRAMES: u32 = 4;

/// Camera distance as a multiple of the subject's largest dimension.
const FRAMING: f32 = 1.7;

/// How long to wait for a staged GLB before giving up on it. Generous but bounded: one unloadable
/// asset must not leave every later row blank.
const MESH_WAIT_FRAMES: u32 = 600;

/// An opaque tile rather than a transparent cut-out, so a row reads the same at rest, hovered and
/// armed — and so thin or glazed geometry is not composited away to nothing.
const BACKDROP: Color = Color::srgb(0.14, 0.135, 0.125);

/// The booth camera. Every other camera query filters positively on `view::MainCamera`, which is what
/// stops this one breaking them; this marker is just how the baker finds its own camera back.
#[derive(Component)]
pub struct ThumbnailCamera;

/// The staged subject, carrying its scene handle so [`bake`] can ask whether it is ready to draw.
#[derive(Component)]
struct Subject(Handle<WorldAsset>);

#[derive(Resource)]
pub struct Thumbnails {
    /// Parallel to the library. Handles are created up front and stay stable, so the palette binds
    /// them before anything is rendered — a row simply shows an empty tile until its turn comes.
    images: Vec<Handle<Image>>,
    /// A throwaway target that absorbs every frame in which no subject is staged. See the module note.
    scratch: Handle<Image>,
    next: usize,
    model: Option<Entity>,
    settled: u32,
    waited: u32,
    camera: Option<Entity>,
    booth: Option<Entity>,
    /// Whether the booth has been dismantled. Separate from "every piece rendered": gating the run
    /// condition on the latter stops the system one frame before it can tear the booth down.
    finished: bool,
}

impl Thumbnails {
    pub fn image(&self, ix: usize) -> Option<Handle<Image>> {
        self.images.get(ix).cloned()
    }
    fn done(&self) -> bool {
        self.next >= self.images.len()
    }
}

pub struct ThumbsPlugin;

impl Plugin for ThumbsPlugin {
    fn build(&self, app: &mut App) {
        // `setup` is registered by the editor's Startup chain, not here: the palette binds these
        // handles, so it must run after them, and one chain is easier to be sure of than two plugins
        // agreeing about order.
        app.add_systems(Update, bake.run_if(unfinished));
    }
}

/// Build the handles and stand the booth up. First link of the editor's Startup chain.
pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, project: Res<Project>) {
    let thumbs = ensure(&mut commands, &mut images, project.library.descriptors.len());
    commands.insert_resource(thumbs);
}

/// `Option<Res<_>>`, never a bare `Res<_>`: Bevy 0.19 evaluates **every** run condition — there is no
/// short-circuit — and a missing resource in one panics the system at param validation. That shipped
/// to the game's `main` once and crashed every launch.
fn unfinished(thumbs: Option<Res<Thumbnails>>) -> bool {
    thumbs.is_some_and(|t| !t.finished)
}

/// Create the image handles and stand up the booth. Runs before the palette, so every row has a
/// handle to bind.
fn ensure(commands: &mut Commands, images: &mut Assets<Image>, count: usize) -> Thumbnails {
    let mut blank = || {
        // `new_target_texture` sets the RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_DST usage a camera
        // target needs; hand-building the descriptor is the usual way to a silently black thumbnail.
        let mut img =
            Image::new_target_texture(THUMB_PX, THUMB_PX, TextureFormat::Rgba8UnormSrgb, None);
        img.data = Some(vec![0; (THUMB_PX * THUMB_PX * 4) as usize]);
        images.add(img)
    };
    let handles = (0..count).map(|_| blank()).collect();
    let scratch = blank();

    // Point lights, so nothing outside their range is touched, and no shadows — a 128 px portrait
    // cannot show one and every shadow map costs a pass.
    let booth = commands
        .spawn((
            Name::new("thumbnail booth"),
            Transform::from_translation(BOOTH),
            Visibility::Inherited,
        ))
        .with_children(|b| {
            for (offset, intensity) in [
                (Vec3::new(2.0, 3.0, 2.0), 400_000.0),
                (Vec3::new(-3.0, 2.0, 1.0), 150_000.0),
                (Vec3::new(0.0, 2.0, -3.0), 120_000.0),
            ] {
                b.spawn((
                    PointLight {
                        intensity,
                        range: 20.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(offset),
                ));
            }
        })
        .id();

    Thumbnails {
        images: handles,
        scratch,
        next: 0,
        model: None,
        settled: 0,
        waited: 0,
        camera: None,
        booth: Some(booth),
        finished: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn bake(
    mut commands: Commands,
    mut thumbs: ResMut<Thumbnails>,
    project: Res<Project>,
    assets: Res<AssetServer>,
    children: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    subjects: Query<&Subject>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
    mut cams: Query<(&mut Transform, &mut Projection, &mut RenderTarget), With<ThumbnailCamera>>,
) {
    if thumbs.done() {
        teardown(&mut commands, &mut thumbs);
        return;
    }
    let ix = thumbs.next;
    let Some(d) = project.library.descriptors.get(ix).cloned() else {
        return;
    };

    let Some(model) = thumbs.model else {
        match stage(&mut commands, &assets, &d) {
            Some(model) => {
                thumbs.model = Some(model);
                thumbs.settled = 0;
                thumbs.waited = 0;
                if thumbs.camera.is_none() {
                    let scratch = thumbs.scratch.clone();
                    thumbs.camera = Some(spawn_camera(&mut commands, &scratch));
                }
            }
            // A descriptor with no mesh has no portrait, and that is not an error — it is a
            // descriptor that has not been given a mesh yet. Skip to the next rather than stalling
            // the whole library behind it.
            None => thumbs.next += 1,
        }
        return;
    };

    let ready = subjects
        .get(model)
        .is_ok_and(|s| assets.is_loaded_with_dependencies(&s.0))
        && has_mesh(model, &children, &meshes);
    if !ready {
        thumbs.waited += 1;
        if thumbs.waited > MESH_WAIT_FRAMES {
            // Loud, and then keep walking: a piece whose GLB never instantiates would otherwise stall
            // the library behind it, leaving every later row blank with no clue why.
            warn!(
                "`{}` produced no drawable mesh in {MESH_WAIT_FRAMES} frames; its palette row will \
                 have no preview",
                d.id
            );
            commands.entity(model).despawn();
            thumbs.model = None;
            thumbs.next += 1;
        }
        return;
    }

    if let Some(cam_entity) = thumbs.camera {
        if let Ok((mut tf, mut proj, mut target)) = cams.get_mut(cam_entity) {
            // **Frame what is actually there, not what the descriptor claims.**
            //
            // Aiming from `extent.footprint`/`height` assumes the geometry sits at the origin with
            // its base at zero, and plenty of meshes do not: `lamp_tall` is a ceiling fitting and
            // `wall_light` is a wall plate, both authored where they hang. Framing them from the
            // origin pointed the camera at empty air and baked two blank tiles — the same *symptom*
            // as the render-target race, from a completely different cause, which is the argument for
            // measuring rather than assuming twice over.
            let (centre, extent) = subject_bounds(model, &children, &bounds)
                .unwrap_or((BOOTH + Vec3::Y * 0.5, subject_extent(&d)));
            *tf = Transform::from_translation(centre + Vec3::new(1.0, 0.85, 1.0) * extent * FRAMING)
                .looking_at(centre, Vec3::Y);
            *proj = Projection::from(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: extent * 1.35,
                },
                ..OrthographicProjection::default_3d()
            });
            *target = RenderTarget::Image(ImageRenderTarget {
                handle: thumbs.images[ix].clone(),
                scale_factor: 1.0,
            });
        }
    }

    thumbs.settled += 1;
    if thumbs.settled <= SETTLE_FRAMES {
        return;
    }

    // Park on the scratch target BEFORE letting go of the subject — see the module note.
    if let Some(cam_entity) = thumbs.camera {
        if let Ok((_, _, mut target)) = cams.get_mut(cam_entity) {
            *target = RenderTarget::Image(ImageRenderTarget {
                handle: thumbs.scratch.clone(),
                scale_factor: 1.0,
            });
        }
    }
    commands.entity(model).despawn();
    thumbs.model = None;
    thumbs.next += 1;
}

/// The world-space centre and largest dimension of everything actually drawn under `root`.
///
/// `None` when nothing under it has an `Aabb` yet, which the caller treats as "fall back to what the
/// descriptor says" — that path is reachable only in the frames before the mesh exists, and the
/// readiness gate above means the bake does not aim then.
fn subject_bounds(
    root: Entity,
    children: &Query<&Children>,
    bounds: &Query<(&Aabb, &GlobalTransform)>,
) -> Option<(Vec3, f32)> {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut any = false;
    let mut queue = vec![root];
    while let Some(e) = queue.pop() {
        if let Ok((aabb, tf)) = bounds.get(e) {
            // The AABB is in the mesh's own space; the corners have to go through the world transform
            // before they mean anything, because the scene root may scale and rotate them.
            let c = Vec3::from(aabb.center);
            let h = Vec3::from(aabb.half_extents);
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    for sz in [-1.0, 1.0] {
                        let corner = tf.transform_point(c + h * Vec3::new(sx, sy, sz));
                        lo = lo.min(corner);
                        hi = hi.max(corner);
                        any = true;
                    }
                }
            }
        }
        if let Ok(kids) = children.get(e) {
            queue.extend(kids.iter());
        }
    }
    any.then(|| {
        let size = hi - lo;
        ((lo + hi) * 0.5, size.x.max(size.y).max(size.z).max(0.05))
    })
}

/// The subject's largest dimension, from what the descriptor records. The fallback for the frames
/// before any `Aabb` exists; floored, so it never puts the camera inside the mesh.
fn subject_extent(d: &emerge_core::descriptor::Descriptor) -> f32 {
    let (w, dep) = d.extent.footprint.unwrap_or((1.0, 1.0));
    let h = d.extent.height.unwrap_or(1.0);
    let scale = d.align.scale.unwrap_or(1.0);
    (w.max(dep).max(h) * scale).max(0.25)
}

fn stage(
    commands: &mut Commands,
    assets: &AssetServer,
    d: &emerge_core::descriptor::Descriptor,
) -> Option<Entity> {
    let mesh = d.mesh.as_ref()?;
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(mesh.clone()));
    Some(
        commands
            .spawn((
                Name::new("thumbnail subject"),
                Subject(scene.clone()),
                // The same scale a real placement applies, so the portrait shows the piece at the
                // size it will actually be rather than at whatever the artist exported.
                Transform::from_translation(BOOTH)
                    .with_scale(Vec3::splat(d.align.scale.unwrap_or(1.0))),
                Visibility::Inherited,
            ))
            .with_child((WorldAssetRoot(scene), Transform::default()))
            .id(),
    )
}

fn spawn_camera(commands: &mut Commands, first_target: &Handle<Image>) -> Entity {
    commands
        .spawn((
            Name::new("thumbnail camera"),
            ThumbnailCamera,
            Camera3d::default(),
            Camera {
                // Before the main camera, so the palette shows this frame's bake rather than the last.
                order: -1,
                clear_color: ClearColorConfig::Custom(BACKDROP),
                ..default()
            },
            // `RenderTarget` IS its own component in 0.19 — one of `Camera`'s `#[require]`s rather
            // than a field on it.
            RenderTarget::Image(ImageRenderTarget {
                handle: first_target.clone(),
                scale_factor: 1.0,
            }),
            // Component form, so it lifts this camera's exposure only; the resource form is global.
            AmbientLight {
                brightness: 900.0,
                ..default()
            },
            Projection::from(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: 2.0,
                },
                ..OrthographicProjection::default_3d()
            }),
            Transform::from_translation(BOOTH + Vec3::splat(3.0)).looking_at(BOOTH, Vec3::Y),
        ))
        .id()
}

fn has_mesh(root: Entity, children: &Query<&Children>, meshes: &Query<(), With<Mesh3d>>) -> bool {
    let mut queue = vec![root];
    while let Some(e) = queue.pop() {
        if meshes.get(e).is_ok() {
            return true;
        }
        if let Ok(kids) = children.get(e) {
            queue.extend(kids.iter());
        }
    }
    false
}

fn teardown(commands: &mut Commands, thumbs: &mut Thumbnails) {
    thumbs.finished = true;
    if let Some(cam) = thumbs.camera.take() {
        commands.entity(cam).despawn();
        // Logged because the bake is otherwise invisible: if a piece never instantiates, this line is
        // what says how far it got.
        info!("baked {} palette thumbnail(s); booth torn down", thumbs.images.len());
    }
    if let Some(booth) = thumbs.booth.take() {
        commands.entity(booth).despawn();
    }
    if let Some(model) = thumbs.model.take() {
        commands.entity(model).despawn();
    }
}
