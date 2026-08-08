//! **Offscreen frames for an agent, without touching the machine you are using.**
//!
//! `bevy_debugger/screenshot` (from `bevy_debugger_bevy`) captures an `Image` a camera renders to.
//! This module owns that camera and that image, because which view is worth capturing is a question
//! only the game can answer.
//!
//! # Why not just capture the window
//!
//! `Screenshot::primary_window()` reads the window surface, which macOS only keeps current while the
//! window is actually on screen. Measured here: the same capture returns **7,188 distinct colours**
//! with the window focused and **1** — a flat rectangle — with Safari in front. The only way to make
//! that path produce a frame is to raise the window, which steals focus, may switch Spaces, and
//! interrupts whatever the person at the keyboard is doing.
//!
//! A camera rendering to an `Image` has no such dependency. It is pure Bevy — no OS screen capture, no
//! window manager, no `screencapture(1)` — and it works while the game is buried, on another Space, or
//! minimised. That is the whole reason this module exists.
//!
//! # Why a second camera is safe here
//!
//! `src/lib.rs` records that a second `Camera3d` once broke nine systems at once, because
//! `Single<.., With<Camera3d>>` matched two entities and `Single` *silently skips* rather than
//! erroring. That was fixed by filtering **positively**: 29 sites now say `With<MainCamera>`, and no
//! query in the tree assumes a lone camera. This camera deliberately does **not** carry `MainCamera`,
//! so it is invisible to every one of them — the same argument `ThumbnailCamera` relies on.
//!
//! # Cost
//!
//! The scene is rendered twice per frame while the feature is on. That is why this is behind the
//! opt-in `debugger` feature and never in a shipped or determinism build.

use bevy::camera::{Hdr, ImageRenderTarget, RenderTarget, ScalingMode};
use bevy::image::Image;
use bevy::light::{GeneratedEnvironmentMapLight, ShadowFilteringMethod};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::window::PrimaryWindow;

use bevy_debugger_bevy::DebugCaptureTarget;

/// The camera that renders the agent's view into [`DebugCaptureTarget`]'s image.
///
/// It carries no [`crate::MainCamera`], which is exactly what keeps it invisible to the 29 camera
/// queries that filter positively on that marker.
#[derive(Component)]
pub struct DebugCaptureCamera;

/// Mirrors the player's view into an offscreen image so an agent can look at the game without
/// touching the window.
pub struct DebugCapturePlugin;

impl Plugin for DebugCapturePlugin {
    fn build(&self, app: &mut App) {
        // The camera itself is spawned by `camera::setup`, beside the real one — see `spawn_mirror`.
        // Cosmetic and observational: `Update`, never `FixedUpdate`. Nothing here writes pinned state,
        // and the feature is absent from every determinism run regardless.
        app.add_systems(Update, (follow_main_camera, match_window_size));
    }
}

/// Allocate the render target at `width`x`height`.
///
/// `new_target_texture` sets the `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_DST` usage flags a camera
/// target needs — hand-building the descriptor is the usual way to get a silently black capture, as
/// `site_editor::thumbs` records.
fn new_target(images: &mut Assets<Image>, width: u32, height: u32) -> Handle<Image> {
    let mut img = Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
    img.data = Some(vec![0; (width as usize) * (height as usize) * 4]);
    images.add(img)
}

/// Physical size of the primary window, floored at 1 so a zero-sized window cannot produce a
/// zero-sized texture.
fn window_size(window: &Window) -> (u32, u32) {
    (
        (window.physical_width()).max(1),
        (window.physical_height()).max(1),
    )
}

/// Spawn the mirror camera, called from `camera::setup` with the **same** values the real camera gets.
///
/// Built there rather than from a `Startup` system of its own so parity is by construction: the
/// environment map, exposure and bloom are the ones the player's camera is using, not a second guess
/// at them. The first offscreen capture written without this rendered the squad against black — the
/// walls are lit almost entirely by `GeneratedEnvironmentMapLight`, and a camera without it sees a
/// different game.
pub fn spawn_mirror(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    env_map: Handle<Image>,
    env_brightness: f32,
    bloom_intensity: f32,
    viewport_height: f32,
) {
    // Sized provisionally; `match_window_size` corrects it on the first frame a window exists, and
    // whenever it changes thereafter.
    let (w, h) = (1920, 1200);
    let handle = new_target(images, w, h);

    commands.spawn((
        Name::new("debug capture camera"),
        DebugCaptureCamera,
        Camera3d::default(),
        Camera {
            // Before the main camera, so a capture taken this frame shows this frame.
            order: -1,
            ..default()
        },
        // `RenderTarget` is its own component in 0.19 — one of `Camera`'s `#[require]`s, not a field.
        RenderTarget::Image(ImageRenderTarget { handle: handle.clone(), scale_factor: 1.0 }),
        // Everything below mirrors `camera::setup`. Drop any of it and the capture stops looking like
        // the game: without `Hdr` the emissive layer clips, without the environment map the geometry
        // goes black.
        Hdr,
        Bloom { intensity: bloom_intensity, ..Bloom::NATURAL },
        ShadowFilteringMethod::Gaussian,
        GeneratedEnvironmentMapLight {
            environment_map: env_map,
            intensity: env_brightness,
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height },
            ..OrthographicProjection::default_3d()
        }),
        // Seeded properly by `follow_main_camera` on the first pass.
        Transform::default(),
    ));

    commands.insert_resource(DebugCaptureTarget { image: handle });
    info!("debug capture: offscreen target ready at {w}x{h}");
}

/// Copy the player camera's pose and projection onto the capture camera, so the offscreen image shows
/// what the player would see.
fn follow_main_camera(
    main: Option<Single<(&GlobalTransform, &Projection), With<crate::MainCamera>>>,
    capture: Option<Single<(&mut Transform, &mut Projection), (With<DebugCaptureCamera>, Without<crate::MainCamera>)>>,
) {
    let (Some(main), Some(capture)) = (main, capture) else {
        return;
    };
    let (main_tf, main_proj) = *main;
    let (mut tf, mut proj) = capture.into_inner();
    *tf = main_tf.compute_transform();
    *proj = main_proj.clone();
}

/// Keep the target the same size as the window, so a region given in window pixels means the same
/// thing in the capture.
///
/// Compares sizes rather than listening for `WindowResized`. An event-driven version only works if an
/// event actually fires — it does at startup on macOS, but relying on that makes the capture
/// resolution depend on a platform behaviour nobody promised. Comparing is cheap and cannot miss.
fn match_window_size(
    mut images: ResMut<Assets<Image>>,
    target: Option<ResMut<DebugCaptureTarget>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut cameras: Query<&mut RenderTarget, With<DebugCaptureCamera>>,
) {
    let (Some(mut target), Some(window)) = (target, window) else {
        return;
    };
    let (w, h) = window_size(&window);
    let current = images.get(&target.image).map(|i| {
        let s = i.texture_descriptor.size;
        (s.width, s.height)
    });
    if current == Some((w, h)) {
        return;
    }
    let handle = new_target(&mut images, w, h);
    for mut rt in &mut cameras {
        *rt = RenderTarget::Image(ImageRenderTarget { handle: handle.clone(), scale_factor: 1.0 });
    }
    target.image = handle;
    info!("debug capture: target sized to {w}x{h}");
}
