//! **The palette's preview thumbnails** — one baked render of every kit piece.
//!
//! A palette of 45 enum names tells you what a piece is *called*, which is not the question an author
//! is asking. `WallLow` is a counter, `Slab` is an examination table, and `Pipe` and `PipeCorner`
//! differ in a way no name conveys. So each row carries a small render of the actual mesh.
//!
//! # The photo booth
//!
//! Staging happens at [`BOOTH`], four thousand metres from anything. That is the same
//! isolation-by-distance trick `site::mod` documents for the Site itself — *"achieved by DISTANCE —
//! all of those are dungeon-grid indexed and simply never reach out here"* — and it buys the same
//! thing here: the booth camera sees the staged piece and nothing else, with no `RenderLayers` to
//! propagate onto GLB scene children (which is where layer-based masking usually goes wrong).
//!
//! Its lights are **point** lights, not directional, for the same reason: a `DirectionalLight` is
//! global and would relight the whole hub. Its `AmbientLight` is the **component** form, new in Bevy
//! 0.19, which applies per-camera — the resource form would also have been global.
//!
//! # One camera, forty-five images
//!
//! Forty-five simultaneous cameras would be absurd, so a single camera walks the kit: stage a piece,
//! wait for its GLB to actually instantiate a mesh, point the camera at it, render one frame into that
//! piece's image, move on. About a second for the whole kit, and then the camera **despawns** — a
//! render target left alive would cost a draw of the booth every frame forever.
//!
//! # Why the camera carries [`crate::ThumbnailCamera`]
//!
//! Nine systems in this codebase take `Single<.., With<Camera3d>>`, and `Single` *silently skips its
//! system* when there is not exactly one match. A second camera without that marker stops the audio
//! listener, every billboard, and the camera controls, with no error anywhere. See the marker's own
//! docs; every one of those queries now excludes it.

use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget, ScalingMode};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::site::kit::SiteKit;
use crate::site::pieces::SitePiece;
use crate::site::SiteKitRes;

/// Where pieces are staged for their portrait. Far from the dungeon (origin) and the Site (1024), so
/// nothing else can wander into frame.
const BOOTH: Vec3 = Vec3::new(4096.0, 0.0, 4096.0);

/// Thumbnail edge, in pixels. Small enough that 45 of them are trivial memory, large enough to tell a
/// stool from a chair.
const THUMB_PX: u32 = 128;

/// Frames to hold a staged piece after its mesh appears, before reading the render as done. One frame
/// to render, one of slack for the material/skinning to settle.
const SETTLE_FRAMES: u32 = 2;

/// How far the booth camera sits from the subject, as a multiple of the subject's largest dimension.
const FRAMING: f32 = 1.7;

/// How long to wait for a staged GLB to instantiate a mesh before skipping it. Generous — the first
/// piece is staged while the rest of the game is still streaming in — but bounded, so one unloadable
/// asset cannot leave the remaining forty rows blank forever.
const MESH_WAIT_FRAMES: u32 = 600;

/// Baked previews, one per `SitePiece::ALL` entry.
#[derive(Resource)]
pub struct Thumbnails {
    /// Parallel to [`SitePiece::ALL`]. Handles are created up front and stay stable, so the palette
    /// can bind them before anything has been rendered — a row simply shows an empty image until its
    /// turn comes round.
    images: Vec<Handle<Image>>,
    /// Index into [`SitePiece::ALL`] currently being staged.
    next: usize,
    /// The staged model, despawned when the camera moves on.
    model: Option<Entity>,
    /// Frames the staged piece has had a mesh.
    settled: u32,
    /// Frames spent waiting for the staged piece's GLB to instantiate one.
    waited: u32,
    camera: Option<Entity>,
    booth: Option<Entity>,
    /// Whether the booth has been dismantled.
    ///
    /// Separate from [`Self::done`] and NOT a duplicate of it. The baker's run condition keys on this
    /// rather than on `done()`, because gating on `done()` stops the system the instant the last piece
    /// lands — one frame *before* it can tear the booth down, leaving a render-target camera drawing an
    /// empty room every frame for the rest of the session.
    finished: bool,
}

impl Thumbnails {
    /// The preview for a piece. `None` only if `SitePiece::ALL` and the kit disagree, which
    /// `kit::validate_site_kit` already rules out.
    pub fn image(&self, piece: SitePiece) -> Option<Handle<Image>> {
        let ix = SitePiece::ALL.iter().position(|p| *p == piece)?;
        self.images.get(ix).cloned()
    }

    /// Whether every piece has been rendered.
    pub fn done(&self) -> bool {
        self.next >= SitePiece::ALL.len()
    }

    /// Whether the baker has nothing left to do — every piece rendered *and* the booth dismantled.
    pub fn finished(&self) -> bool {
        self.finished
    }
}

/// Create the image handles and stand up the booth. Called once, before the palette is built, so every
/// row has a handle to bind.
pub fn ensure(commands: &mut Commands, images: &mut Assets<Image>) -> Thumbnails {
    let handles = SitePiece::ALL
        .iter()
        .map(|_| {
            // `new_target_texture` sets the RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_DST usage flags
            // a camera target needs; building the descriptor by hand is the usual way to get a
            // silently-black thumbnail.
            let mut img = Image::new_target_texture(
                THUMB_PX,
                THUMB_PX,
                // Explicit rather than `bevy_default()`, which is deprecated in 0.19 —
                // Bevy no longer wants a blessed default format.
                TextureFormat::Rgba8UnormSrgb,
                None,
            );
            // Start transparent so an un-baked row reads as empty rather than as a black square.
            img.data = Some(vec![0; (THUMB_PX * THUMB_PX * 4) as usize]);
            images.add(img)
        })
        .collect();

    // The booth's own lighting. Point lights, so nothing outside their range is touched, and no
    // shadows — a 128 px portrait cannot show one and every shadow map costs a pass.
    let booth = commands
        .spawn((
            Name::new("site editor thumbnail booth"),
            Transform::from_translation(BOOTH),
            Visibility::Inherited,
        ))
        .with_children(|b| {
            for (offset, intensity) in [
                (Vec3::new(2.0, 3.0, 2.0), 400_000.0),   // key
                (Vec3::new(-3.0, 2.0, 1.0), 150_000.0),  // fill
                (Vec3::new(0.0, 2.0, -3.0), 120_000.0),  // rim
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
        next: 0,
        model: None,
        settled: 0,
        waited: 0,
        camera: None,
        booth: Some(booth),
        finished: false,
    }
}

/// Walk the kit, one piece per render.
///
/// Runs only while there is baking left to do, and tears the booth down on the last piece.
pub fn bake(
    mut commands: Commands,
    mut thumbs: ResMut<Thumbnails>,
    kit: Option<Res<SiteKitRes>>,
    assets: Res<AssetServer>,
    children: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    mut cams: Query<
        (&mut Transform, &mut Projection, &mut RenderTarget),
        With<crate::ThumbnailCamera>,
    >,
) {
    let Some(kit) = kit else { return };
    if thumbs.done() {
        teardown(&mut commands, &mut thumbs);
        return;
    }

    let ix = thumbs.next;
    let Some(&piece) = SitePiece::ALL.get(ix) else {
        return;
    };

    // Stage the piece if it is not up yet.
    let Some(model) = thumbs.model else {
        let model = stage(&mut commands, &assets, &kit.0, piece);
        thumbs.model = Some(model);
        thumbs.settled = 0;
        thumbs.waited = 0;
        if thumbs.camera.is_none() {
            thumbs.camera = Some(spawn_camera(&mut commands, &thumbs.images[ix]));
        }
        return;
    };

    // A GLB scene instantiates over several frames; rendering before its meshes exist bakes an empty
    // square. Waiting on the mesh rather than on a frame count is what makes this independent of how
    // fast the disk is.
    if !has_mesh(model, &children, &meshes) {
        thumbs.waited += 1;
        if thumbs.waited > MESH_WAIT_FRAMES {
            // Give up on this one and keep walking. A piece whose GLB never instantiates would
            // otherwise stall the whole kit behind it, leaving every later row blank with no clue
            // why — the palette must degrade to "this one has no picture", loudly, not to "the
            // previews stopped working".
            warn!(
                "site_editor: {piece:?} produced no mesh in {MESH_WAIT_FRAMES} frames; \
                 its palette row will have no preview"
            );
            commands.entity(model).despawn();
            thumbs.model = None;
            thumbs.next += 1;
        }
        return;
    }

    // Aim at the subject and point the camera's target at this piece's image.
    if let Some(cam_entity) = thumbs.camera {
        if let Ok((mut tf, mut proj, mut target)) = cams.get_mut(cam_entity) {
            let extent = subject_extent(&kit.0, piece);
            let centre = BOOTH + Vec3::Y * extent * 0.35;
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

    // Done with this one.
    commands.entity(model).despawn();
    thumbs.model = None;
    thumbs.next += 1;
    debug!("site_editor: baked {piece:?} ({}/{})", thumbs.next, SitePiece::ALL.len());
}

/// Spawn the piece's GLB at the booth, upright and unrotated.
fn stage(
    commands: &mut Commands,
    assets: &AssetServer,
    kit: &SiteKit,
    piece: SitePiece,
) -> Entity {
    let scene: Handle<WorldAsset> =
        assets.load(GltfAssetLabel::Scene(0).from_asset(kit.glb(piece).to_owned()));
    commands
        .spawn((
            Name::new("site editor thumbnail subject"),
            // The same two scales `site::visuals::place` applies, so the portrait shows the piece at
            // the size it will actually be in the hub rather than at whatever the artist exported.
            Transform::from_translation(BOOTH + Vec3::Y * kit.y_offset(piece)).with_scale(
                Vec3::splat(kit.scale(piece)) * Vec3::new(1.0, kit.y_scale(piece), 1.0),
            ),
            Visibility::Inherited,
        ))
        .with_child((
            WorldAssetRoot(scene),
            Transform::default(),
        ))
        .id()
}

/// The booth camera. Orthographic like the game's, so a piece reads the way it will on the floor.
fn spawn_camera(commands: &mut Commands, first_target: &Handle<Image>) -> Entity {
    commands
        .spawn((
            Name::new("site editor thumbnail camera"),
            // Without this marker, nine `Single<.., With<Camera3d>>` systems stop dead. See
            // `crate::ThumbnailCamera`.
            crate::ThumbnailCamera,
            Camera3d::default(),
            Camera {
                // Before the main camera, so the palette shows this frame's bake rather than last
                // frame's.
                order: -1,
                // Transparent, so a thumbnail sits on the button's own colour rather than a black
                // tile. This is a `Camera` field; `ClearColorConfig` is not a component.
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            // `RenderTarget` IS its own component in Bevy 0.19 — it is one of `Camera`'s `#[require]`s
            // rather than a field on it, so it is spawned alongside.
            RenderTarget::Image(ImageRenderTarget {
                handle: first_target.clone(),
                scale_factor: 1.0,
            }),
            // Component form (Bevy 0.19), so it lifts this camera's exposure only — the resource form
            // is global and would wash out the hub.
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

/// Has this staged model actually instantiated a mesh yet?
fn has_mesh(root: Entity, children: &Query<&Children>, meshes: &Query<(), With<Mesh3d>>) -> bool {
    if meshes.contains(root) {
        return true;
    }
    // Breadth-first rather than recursion: a GLB scene can nest several levels and a blown stack in a
    // dev tool is still a crash.
    let mut queue = vec![root];
    while let Some(e) = queue.pop() {
        if meshes.contains(e) {
            return true;
        }
        if let Ok(kids) = children.get(e) {
            queue.extend(kids.iter());
        }
    }
    false
}

/// Roughly how big the piece is, for framing. Uses the kit's authored footprint and height rather than
/// a mesh AABB, so it is available before the GLB finishes loading and agrees with what the placement
/// rules think the piece measures.
fn subject_extent(kit: &SiteKit, piece: SitePiece) -> f32 {
    let (fw, fd) = kit.piece(piece).footprint;
    let h = kit.top_height(piece);
    fw.max(fd).max(h).max(0.25)
}

/// Drop the camera and the booth once every piece is baked. A live render target costs a full pass
/// every frame, and there is nothing left to draw into it.
fn teardown(commands: &mut Commands, thumbs: &mut Thumbnails) {
    thumbs.finished = true;
    if let Some(cam) = thumbs.camera.take() {
        commands.entity(cam).despawn();
        // Logged because the bake is otherwise invisible: if a piece's GLB never instantiates a mesh
        // the walk stalls on it silently, and this line is what says how far it got.
        info!(
            "site_editor: baked {} palette thumbnail(s); booth torn down",
            thumbs.images.len()
        );
    }
    if let Some(booth) = thumbs.booth.take() {
        commands.entity(booth).despawn();
    }
    if let Some(model) = thumbs.model.take() {
        commands.entity(model).despawn();
    }
}
