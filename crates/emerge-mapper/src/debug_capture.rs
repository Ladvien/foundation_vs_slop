//! **An offscreen frame of the map, for an agent, without touching the screen you are using.**
//!
//! `bevy_debugger/screenshot` captures the `Image` named by [`DebugCaptureTarget`]. Which view is
//! worth capturing is a question only the application can answer, so the camera and the image are
//! owned here — the same division the game makes in `src/debug_capture.rs`.
//!
//! # Why not capture the window
//!
//! `Screenshot::primary_window()` reads the window surface, which macOS keeps current only while the
//! window is actually on screen. The game measured it: the same capture returns **7,188 distinct
//! colours** focused and **1** — a flat rectangle — with something else in front. Making that path
//! produce a frame means raising the window, which steals focus and interrupts whoever is at the
//! machine. A camera rendering to an `Image` has no such dependency.
//!
//! # What this cannot see, and why `bevy_devshot` stays
//!
//! **The panels.** Bevy draws a UI tree to one camera, so a mirror never receives the interface —
//! and in an editor the interface is most of what there is to look at. `bevy_devshot`'s whole-frame
//! sentinel capture remains the way to see a panel, a banner or the error log; this is the way to
//! see the map, with a region and a zoom that the sentinel path does not offer.
//!
//! # Why a second camera is safe
//!
//! `view.rs` states the trap: `Single<.., With<Camera3d>>` **silently skips** on a non-unique match,
//! so a second 3D camera can kill unrelated systems with no error anywhere. Every camera query in
//! this crate filters positively on [`MainCamera`], and this camera deliberately does not carry it —
//! the same argument the thumbnail booth already relies on.

use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

use bevy_debugger_bevy::DebugCaptureTarget;

use crate::view::MainCamera;

/// Edge of the captured image, physical pixels. Square, because the map view is orthographic and an
/// author frames it by panning rather than by aspect.
const CAPTURE_EDGE: u32 = 1024;

/// The camera that renders the agent's view into [`DebugCaptureTarget`]'s image.
///
/// Carries no [`MainCamera`], which is what keeps it invisible to every camera query in this crate.
#[derive(Component)]
struct DebugCaptureCamera;

pub struct DebugCapturePlugin;

impl Plugin for DebugCapturePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(crate::screen::Screen::Editor), spawn_capture_camera)
            .add_systems(Update,
                (mirror_main_camera)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            );
    }
}

fn spawn_capture_camera(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    clear: Option<Res<ClearColor>>,
) {
    let size = Extent3d {
        width: CAPTURE_EDGE,
        height: CAPTURE_EDGE,
        depth_or_array_layers: 1,
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("emerge-mapper debug capture"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            // `COPY_SRC` is what lets the frame be read back — without it the capture reports a
            // target it cannot read rather than writing a file.
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::COPY_SRC
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);
    let handle = images.add(image);

    commands.spawn((
        DebugCaptureCamera,
        Camera3d::default(),
        Camera {
            // **Before the window camera**, so a capture taken this frame shows this frame — and so
            // this never becomes the camera Bevy hands the UI tree to.
            order: -1,
            // The editor's own ground, so a captured frame reads like the window rather than like a
            // different program. `main.rs` inserts `ClearColor`; a harness that omitted it gets
            // Bevy's default instead of a wrong guess.
            clear_color: clear
                .map(|c| bevy::camera::ClearColorConfig::Custom(c.0))
                .unwrap_or_default(),
            ..default()
        },
        // **`RenderTarget` is its own component in 0.19** — one of `Camera`'s `#[require]`s, not a
        // field on it. `CLAUDE.md` lists this among the traps already paid for.
        RenderTarget::Image(ImageRenderTarget {
            handle: handle.clone(),
            scale_factor: 1.0,
        }),
        // Placed by `mirror_main_camera` on the first frame; this is only a starting point.
        Transform::default(),
    ));

    commands.insert_resource(DebugCaptureTarget { image: handle });
}

/// **Point the mirror wherever the author is looking.**
///
/// Copied every frame rather than parented: the map camera is driven by `view::drive` writing a
/// `Transform` outright, and a child would inherit a scale and a rotation it has no use for. Copying
/// the projection too is what makes the capture show the same metres — an orthographic camera's
/// framing is its `scale`, not its distance, so a mirror with the default projection would show a
/// different amount of map at every zoom level and be quietly wrong rather than obviously wrong.
fn mirror_main_camera(
    main: Option<Single<(&GlobalTransform, &Projection), With<MainCamera>>>,
    capture: Option<
        Single<(&mut Transform, &mut Projection), (With<DebugCaptureCamera>, Without<MainCamera>)>,
    >,
) {
    let (Some(main), Some(capture)) = (main, capture) else {
        return;
    };
    let (from, projection) = *main;
    let (mut transform, mut mirror) = capture.into_inner();
    *transform = from.compute_transform();
    *mirror = projection.clone();
}
