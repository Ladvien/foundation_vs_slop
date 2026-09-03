//! **One surface, drawn once — the window and an agent see the same pixels.**
//!
//! Everything this editor draws goes into a single `Image`: the world from [`crate::view::MainCamera`],
//! the interface on top of it, and nothing else. The window then shows that image. An agent asking
//! `bevy_debugger/screenshot` reads the same handle, so what it gets back is what the author is
//! looking at — including the panels.
//!
//! # Why this exists
//!
//! It did not, and the cost was the author's screen.
//!
//! The editor used to draw the interface straight to the window and mirror only the *world* to an
//! offscreen square for the agent. Bevy draws a UI tree to **one** camera, so the mirror could never
//! receive a panel — measured on 2026-08-18 against a running editor, same screen, same second: the
//! BRP capture returned one chair on black, and the whole-frame sentinel capture returned the
//! `MESHES AND TILES` panel, the tab strip, the candidate list and the compass. In an editor the
//! interface is most of what there is to look at, so every visual question fell back to
//! `bevy_devshot`, which reads the **window surface** — and macOS keeps that current only while the
//! window is actually on screen (7,188 distinct colours focused, **1** behind something else). So
//! looking at a panel meant taking the display of whoever was at the machine, which is the one thing
//! the deleted `debug_capture` module was written to avoid.
//!
//! Rendering to an image and mirroring it to the window has no such dependency. `chooser.rs` already
//! did exactly this — for sharpness rather than for capture — and this module is that rig lifted to
//! serve both screens, so there is one answer to "how does this application draw" instead of two.
//!
//! # The pointer, and the one conversion
//!
//! Drawing through an image moves every screen coordinate into the image's space, and getting that
//! wrong is silent: a click lands somewhere else and nothing logs. It is one conversion, applied
//! once, and the rest of the crate was already written to receive it.
//!
//! The target is sized in **physical** pixels (see [`fit_surface_to_window`] for why — it is what
//! makes the type sharp) and carries `scale_factor: 1.0`, so *the image's logical pixels are physical
//! pixels*. [`crate::view::sense_pointer`] therefore stores the pointer already multiplied by the
//! window's scale factor, and then:
//!
//! - **`cursor_ground`/`cursor_ray`** feed `Camera::viewport_to_world`, which measures against
//!   `logical_viewport_rect` (`bevy_camera-0.19.0/src/camera.rs:803`) — the image's, now physical.
//!   Match, with no change at any of the fourteen call sites.
//! - **`crate::view::over_ui`** multiplies the pointer by `camera.target_scaling_factor()`, taken
//!   from the same place `bevy_ui`'s backend takes it. Against an image target that factor is
//!   **1.0**, and a pointer already in surface space needs no second conversion. Also unchanged.
//! - **`compose::place_labels`** divides `world_to_viewport` by `Res<UiScale>`, which this module
//!   writes. It follows the surface automatically, which is what reading the resource bought.
//!
//! # UI picking has to be told where the pointer is
//!
//! This is the part that does not follow, and it is load-bearing: every panel carries `Hovered`, and
//! that is the gate stopping a click on a row from *also* dropping a piece on the map behind it.
//!
//! `bevy_ui`'s backend keeps a pointer only for cameras whose normalized `RenderTarget` **equals the
//! pointer's own target** (`bevy_ui-0.19.0/src/picking_backend.rs:129`). A mouse targets the window;
//! a camera drawing to an image never matches it, so the whole interface would stop answering the
//! pointer — with no error anywhere. [`retarget_pointer`] re-points it, and the same source line
//! says what position to hand over: the backend scales by `camera.target_scaling_factor()`, which is
//! 1.0 here, so the position must already be **physical**.
//!
//! Nothing about that is a second definition of where the cursor is — [`crate::view::Pointer`]
//! remains the one answer, and this writes the same number into the form the backend reads.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::picking::PickingSystems;
use bevy::picking::pointer::PointerLocation;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

/// **The layer the mirror sprite is on, and the surface cameras are not.**
///
/// Without the split the offscreen camera drew the sprite showing its own render target — the same
/// texture as colour attachment and sampled source in one pass — and the frame came back a flat
/// `000000` with nothing in the log at all. `chooser.rs` paid for this once; the constant moves here
/// with the rig.
pub const MIRROR_LAYER: usize = 1;

/// The image every camera in this application draws into, and the one an agent reads.
#[derive(Resource)]
pub struct Surface {
    pub image: Handle<Image>,
}

/// **On every entity the rig owns, so [`crate::screen::scene_roots`] can leave them alone.**
///
/// The teardown sweeps every parentless entity carrying `Transform` or `Node`, which is exactly what
/// three cameras and a sprite are. Windows and monitors are already excluded there as the things
/// that outlive both screens; the surface is now a second, and for the same reason — it is *how the
/// application draws*, not something a screen made.
#[derive(Component)]
pub struct SurfaceRig;

/// Clears the surface, and draws nothing. Lowest order, always present.
///
/// A pass of its own because the two cameras above it must not clear: the world would wipe a stale
/// interface and the interface would wipe the world. On the menu screen there is no world camera at
/// all, so "let the world clear it" is not a rule that holds on both screens — and one rule that
/// holds everywhere is worth a camera that does nothing else.
#[derive(Component)]
pub struct SurfaceGround;

/// The camera the interface is drawn to — **the default UI camera for the whole application**.
///
/// `IsDefaultUiCamera` is what puts *everyone's* tree on the image rather than only the one file
/// that names a target: Bevy picks the highest-order camera rendering to the primary window when a
/// node names none (`bevy_ui-0.19.0/src/ui_node.rs:2934`), so without this the guide overlay — which
/// spawns its root bare — went to the window camera and appeared in no capture.
#[derive(Component)]
pub struct SurfaceCamera;

/// Shows the surface, and is the only camera pointed at the window.
#[derive(Component)]
pub struct WindowCamera;

/// The sprite carrying the surface into the window.
#[derive(Component)]
pub struct Mirror;

pub struct SurfacePlugin;

impl Plugin for SurfacePlugin {
    fn build(&self, app: &mut App) {
        // **Built here, not in `Startup`, and that is not a preference.**
        //
        // `bevy_state` inserts its transition schedule *before* the startup ones —
        // `schedule.insert_startup_before(PreStartup, StateTransition)`
        // (`bevy_state-0.19.0/src/app.rs:336`) — so `OnEnter(Editor)` runs before any `Startup`
        // system, and `build_headless_at` enters the editor on the first transition by design.
        // A surface spawned in `Startup` therefore does not exist when `view::setup` asks for it,
        // and in Bevy 0.19 a missing `Res<T>` panics its system: eighty tests, one message.
        //
        // Doing it at build time is also simply true — the surface is not something a frame makes.
        spawn_surface(app.world_mut());
        app
            // **A known crash lives here, and two fixes have been tried and eliminated. Read this
            // before trying a third.**
            //
            // Booting straight into a door (`emerge-mapper . <map>`) dies inside two seconds,
            // reproduced 4/4, while booting through the chooser never does, 0/4:
            //
            // ```text
            // Attachments have differing sizes: the depth attachment's texture view has extent
            // (3396, 1356, 1) but is followed by the color attachment at index 0's texture view
            // which has (3440, 1440, 1)
            // Quitting the application due to Validation RenderError
            // ```
            //
            // Those two extents are the window before and after the compositor finishes sizing it,
            // and `bevy_render`'s default error handler treats a validation error as **fatal**, so
            // the application exits rather than dropping a frame. The menu path survives because by
            // the time the editor's cameras exist the image has settled — which is why this reads as
            // a Map-door bug and is a startup race.
            //
            // **Eliminated 2026-09-03, both measured rather than reasoned about:**
            //
            // 1. *The viewport was stale.* `fit_viewport_to_frame` runs after `UiSystems::Layout`,
            //    which `bevy_ui` chains **after** `CameraUpdateSystems` — so a viewport written here
            //    is unvalidated until the next frame. Clamping it to the image's own extent is a
            //    real fix for a real latent invariant and it is kept below, but the crash is
            //    unchanged: the failing depth extent is the whole previous target, not a dock-hole
            //    rect, so no viewport produced it.
            // 2. *The resize landed after the cameras read it.* Moving this system to `PostUpdate`
            //    `.before(CameraUpdateSystems)` makes the new extent visible to `camera_system` in
            //    the same frame. The crash is byte-identical, so the two cameras are not
            //    disagreeing about *when* they read the target.
            //
            // What is left, and what the next person should measure first: **three cameras share
            // this one image** (ground, world, interface) and only the 3-D one has a depth
            // attachment. `prepare_core_3d_depth_textures` allocates per *target*, so a depth sized
            // for one camera's extracted target info can meet a colour attachment allocated for
            // another's. The world camera is the one spawned late — `view::setup` at
            // `OnEnter(Editor)` — which fits the startup-path evidence exactly.
            .add_systems(Update, fit_surface_to_window)
            // **After layout, because it reads it.** `PostUpdate` is where `ComputedNode` becomes
            // true for this frame; asking in `Update` would chase the previous one, which is the
            // same one-frame lag `chrome::Follow` exists to name.
            .add_systems(
                PostUpdate,
                fit_viewport_to_frame.after(bevy::ui::UiSystems::Layout),
            )
            // **Between the two picking sets that matter**: `ProcessInput` writes `PointerLocation`
            // in `PreUpdate`, `Backend` reads it in the same schedule
            // (`bevy_picking-0.19.0/src/lib.rs:258`). Anywhere else and the retarget is either
            // overwritten or too late.
            .add_systems(
                PreUpdate,
                retarget_pointer
                    .after(PickingSystems::ProcessInput)
                    .before(PickingSystems::Backend),
            );

        // **Guarded at the registration as well as the definition.** `debugger` is default-on for
        // this editor, but a build without it must still compile — and a `cfg`'d system named in an
        // unguarded `add_systems` is the shape that only fails on the configuration nobody runs.
        #[cfg(feature = "debugger")]
        app.add_systems(
            First,
            inject_clicks.after(bevy::picking::PickingSystems::Input),
        );
    }
}

/// A starting size only — [`fit_surface_to_window`] owns it from the first frame that has a window.
/// Headless runs keep it, because there is no window to ask and an image of zero has no valid
/// texture.
///
/// **Window-shaped, deliberately.** It was a 1024 square, and the square told a comfortable lie:
/// with both docks at their fixed widths, a square surface leaves a stage too narrow for the badge
/// legend to stand in at all, so the geometry the headless ratchets policed was one the windowed
/// app never renders — the first real-window capture showed the legend buried under the piece
/// list's boxes while every test was green. This is a 16:9 the docks leave a real stage inside;
/// `emerge_mapper::harness::resize_surface` is how a test asks for another shape.
const INITIAL_W: u32 = 1536;
const INITIAL_H: u32 = 864;

fn spawn_surface(world: &mut World) {
    // `main.rs` and `build_headless_at` both insert `ClearColor` before the editor's plugins; a
    // caller that did not gets black rather than a wrong guess.
    let ground = world
        .get_resource::<ClearColor>()
        .map_or(Color::BLACK, |c| c.0);
    let Some(mut images) = world.get_resource_mut::<Assets<Image>>() else {
        // Loud, because the alternative is `view::setup` panicking one schedule later on a
        // parameter whose absence says nothing about the cause. `AssetPlugin` is in `DefaultPlugins`
        // and both entry points add those first, so reaching this means the plugin order moved.
        error!("no `Assets<Image>` when the surface was built — add the editor's plugins after `DefaultPlugins`");
        return;
    };
    let size = Extent3d {
        width: INITIAL_W,
        height: INITIAL_H,
        depth_or_array_layers: 1,
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("emerge-mapper surface"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            // `COPY_SRC` is what lets the frame be read back — without it a capture reports a target
            // it cannot read rather than writing a file.
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

    let target = || {
        // `RenderTarget` is its own component in 0.19 — one of `Camera`'s `#[require]`s, not a field
        // on it. **`scale_factor` stays 1.0**: Bevy's own field doc says *"This should almost always
        // be 1.0"* (`bevy_camera-0.19.0/src/camera.rs:989`) and off that path it renders nothing at
        // all. Density is carried by `UiScale` instead — see [`fit_surface_to_window`].
        RenderTarget::Image(ImageRenderTarget {
            handle: handle.clone(),
            scale_factor: 1.0,
        })
    };

    world.spawn((
        SurfaceRig,
        SurfaceGround,
        Camera2d,
        Camera {
            order: ORDER_GROUND,
            clear_color: bevy::camera::ClearColorConfig::Custom(ground),
            ..default()
        },
        target(),
        // Its own empty layer: it exists to clear, and a camera that could see the mirror would be
        // sampling the texture it is writing.
        RenderLayers::none(),
    ));

    world.spawn((
        SurfaceRig,
        SurfaceCamera,
        bevy::ui::IsDefaultUiCamera,
        Camera2d,
        Camera {
            order: ORDER_UI,
            // Never clears: the world has already drawn into this frame.
            clear_color: bevy::camera::ClearColorConfig::None,
            ..default()
        },
        target(),
    ));

    world.spawn((
        SurfaceRig,
        WindowCamera,
        Camera2d,
        Camera {
            order: ORDER_WINDOW,
            ..default()
        },
        // Sees the mirror and nothing else. The interface is not on this layer, so it reaches the
        // window only by way of the image — one rendering path, and a capture is the same pixels.
        RenderLayers::layer(MIRROR_LAYER),
    ));

    // Scale set by `fit_surface_to_window`: a `Camera2d`'s default projection makes one world unit
    // one *logical* pixel and the target is sized in *physical* ones, so the sprite draws at
    // `1 / scale_factor` to cover exactly the window it mirrors.
    world.spawn((
        SurfaceRig,
        Mirror,
        Sprite::from_image(handle.clone()),
        RenderLayers::layer(MIRROR_LAYER),
    ));

    // Only the debugger needs telling which image to read; the rig itself is not optional, because
    // it is how this application draws at all.
    #[cfg(feature = "debugger")]
    world.insert_resource(bevy_debugger_bevy::DebugCaptureTarget {
        image: handle.clone(),
    });

    world.insert_resource(Surface { image: handle });
}

/// Clear the frame. Draws nothing.
pub const ORDER_GROUND: isize = -3;
/// **The world**, which [`crate::view::setup`] gives [`crate::view::MainCamera`]. Named here so the
/// three passes into one image are one ordered list rather than three numbers in three files.
pub const ORDER_WORLD: isize = -2;
/// The interface, over the world.
pub const ORDER_UI: isize = -1;
/// The window, showing the finished surface. Last, so a capture taken this frame shows this frame.
pub const ORDER_WINDOW: isize = 0;

/// **The interface's scale for a window of this size**, in one place both the runtime and a test can
/// ask.
///
/// # Density is not size, and this editor was answering only one of them
///
/// `UiScale` is multiplied by the *display's* `scale_factor`, which says how many physical pixels a
/// logical one is worth — it says nothing about how much room there is. On a 3396 px window
/// reporting a factor of 1, the shipped constant left body text at 13 physical pixels and the two
/// Meshes docks using under 14 % of the width. Reported at the keyboard as a readability complaint,
/// measured as `docs/ui_audit.md` F2.
///
/// **The obvious fix — a larger constant — was tried and is wrong**, and the way it announced itself
/// is worth keeping: it is the density a *small* window also gets. At 1280 x 800 two
/// [`crate::chrome::CONTROLS_W`] docks scaled by 1.45 leave almost no stage, and
/// `no_badge_cluster_draws_through_another` went red because the badge packer had nowhere to put a
/// legend except on top of a control. A constant cannot answer a question about available room.
///
/// So the base is the density the design is drawn at, and a window wider than [`REFERENCE_W`] grows
/// it in proportion, up to [`MAX_GROWTH`]. Below the reference nothing shrinks: the design has a
/// smallest legible size and a cramped window needs the *stage* back, not smaller type.
pub fn ui_scale_for(logical_width: f32, scale_factor: f32) -> f32 {
    let growth = (logical_width / REFERENCE_W).clamp(1.0, MAX_GROWTH);
    crate::chrome::EDITOR_UI_SCALE * growth * scale_factor
}

/// The width the editor's pixel constants were chosen against — a 16:10 laptop's logical width, and
/// the size below which growth stops.
const REFERENCE_W: f32 = 1600.0;

/// **How far the interface may grow on a large display.** A cap rather than a straight ratio,
/// because a dock is a work surface and not a column of prose: past about a quarter again, a wider
/// monitor should be buying *viewport*, which is the thing an editor actually runs out of.
const MAX_GROWTH: f32 = 1.25;

#[cfg(test)]
mod scale_tests {
    use super::{ui_scale_for, MAX_GROWTH, REFERENCE_W};
    use crate::chrome::EDITOR_UI_SCALE;

    /// **A small window keeps the density the design was drawn at**, which is the half a larger
    /// constant broke. 1280 x 800 is the size every headless test lays out at, and the badge packer
    /// measures its stage in exactly those pixels.
    #[test]
    fn a_small_window_is_not_scaled_up() {
        assert_eq!(ui_scale_for(1280.0, 1.0), EDITOR_UI_SCALE);
        assert_eq!(ui_scale_for(REFERENCE_W, 1.0), EDITOR_UI_SCALE);
    }

    /// **A large one grows, and stops.**
    #[test]
    fn a_large_window_grows_to_the_cap() {
        let wide = ui_scale_for(3396.0, 1.0);
        assert_eq!(wide, EDITOR_UI_SCALE * MAX_GROWTH);
        assert!(
            wide > ui_scale_for(REFERENCE_W, 1.0),
            "a window twice the reference width reads at a larger size, or F2 is not fixed"
        );
        // Halfway up the ramp, so the clamp is not the only thing being tested.
        assert!((ui_scale_for(1800.0, 1.0) - EDITOR_UI_SCALE * 1.125).abs() < 1e-5);
    }

    /// **Density still multiplies on top**, because a 2x display is a separate question from a wide
    /// one and the target is sized in physical pixels either way.
    #[test]
    fn the_display_factor_still_multiplies() {
        assert_eq!(ui_scale_for(1280.0, 2.0), EDITOR_UI_SCALE * 2.0);
    }
}

/// **Fit the surface to the window, and carry the display's density in `UiScale`.**
///
/// The target is sized in **physical** pixels, and that is what makes the type sharp. Reported at the
/// keyboard: *"why isn't the text sharper? that feels like text rendered at a lower resolution and
/// then zoomed in on."* It was exactly that — the interface renders to an image and the window shows
/// that image, so the image *is* the resolution the interface is rasterised at, and it was sized in
/// logical pixels. On a 2x display every glyph edge was an interpolation between texels that were
/// never rendered.
///
/// Taking the size from the window's own `physical_*` rather than multiplying the logical size by
/// the scale factor keeps the two exactly equal, with no rounding to disagree about.
///
/// `UiScale` multiplies every `Val::Px` and every font size, so scaling it by the window's factor
/// lays the same design out twice as large in a twice-as-large target — and rasterises every glyph
/// at that size, which is the whole point. The sprite then halves it back for the window.
fn fit_surface_to_window(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    surface: Option<Res<Surface>>,
    mut mirror: Query<&mut Transform, With<Mirror>>,
    mut ui_scale: ResMut<UiScale>,
    mut images: ResMut<Assets<Image>>,
) {
    // No window is a headless run: there is nothing to fit to, and the initial 16:9 stands.
    let (Ok(window), Some(surface), Ok(mut mirror)) =
        (windows.single(), surface, mirror.single_mut())
    else {
        return;
    };

    let sf = window.scale_factor().max(1.0);
    let (iw, ih) = (
        window.resolution.physical_width().max(1),
        window.resolution.physical_height().max(1),
    );

    let want_ui = ui_scale_for(window.resolution.width(), sf);
    if ui_scale.0 != want_ui {
        ui_scale.0 = want_ui;
    }

    // A `1554`-texel image drawn at `0.5` covers `777` logical pixels — the whole window, one texel
    // per physical pixel, which is what sharp means here.
    let want = Vec3::new(1.0 / sf, 1.0 / sf, 1.0);
    if mirror.scale != want {
        mirror.scale = want;
    }

    // `get_mut` marks the asset modified, so ask before writing — per the standing rule that no
    // system writes unconditionally every frame.
    let already = images.get(&surface.image).map(|i| {
        (
            i.texture_descriptor.size.width,
            i.texture_descriptor.size.height,
        )
    });
    if already != Some((iw, ih))
        && let Some(mut image) = images.get_mut(&surface.image)
    {
        image.resize(Extent3d {
            width: iw,
            height: ih,
            depth_or_array_layers: 1,
        });
    }
}

/// **Give the map camera the frame's hole, so the viewport is a region rather than the window.**
///
/// Panels used to float over a camera that owned the whole window, which is why the world ran under
/// them and why "is the pointer over UI" had to be asked at all before a click could mean anything
/// in the world. With a docked frame the interface has its own ground and the world has its own
/// rectangle, and this is the line between them: `chrome::ViewportSlot`'s computed rect, handed to
/// the camera as `Camera::viewport` in physical pixels.
///
/// `ComputedNode` and `UiGlobalTransform` are already in the surface's space — the interface is laid
/// out at `UiScale = EDITOR_UI_SCALE * scale_factor` into a physical-sized target — so no conversion
/// happens here, which is the whole reason [`fit_surface_to_window`] carries the density in `UiScale`
/// rather than in the target's own `scale_factor`.
///
/// `Changed<ComputedNode>`-gated and compares before writing, per the standing rule: `Camera` is
/// change-detected and the render world reads it.
///
/// # The viewport is clamped to the image, and that is a crash fix
///
/// Reproduced 2026-09-03, twice out of two, by booting straight into a door
/// (`emerge-mapper . <map>`): the application died inside two seconds with
///
/// ```text
/// Attachments have differing sizes: the depth attachment's texture view has extent
/// (3396, 1356, 1) but is followed by the color attachment at index 0's texture view
/// which has (3440, 1440, 1)
/// Quitting the application due to Validation RenderError
/// ```
///
/// Those two numbers are the window before and after the compositor finished sizing it. The frame
/// ordering is the whole story, and it is not obvious from either system on its own:
/// `CameraUpdateSystems` is chained **before** `UiSystems::Layout` in `PostUpdate`
/// (`bevy_ui-0.19.0/src/lib.rs:147-158`), so `camera_system` — which is what validates a viewport
/// against its target — has already run by the time this writes one. A viewport written here is
/// therefore unchecked until the *next* frame, and on a frame where the surface also grew, the pass
/// is assembled from a rect measured against one size and a target that is another.
///
/// Booting through the chooser never hit it, because by the time the editor's cameras exist the menu
/// has been on screen for many frames and the image has settled — which is exactly why this looked
/// like a Map-door bug rather than a startup race.
///
/// So the rect is clamped against the image's own current extent before it is handed over, and a
/// rect that cannot be made to fit is **skipped** rather than shrunk to something nobody laid out:
/// one frame of a slightly stale viewport is invisible, and the alternative is a dead application.
fn fit_viewport_to_frame(
    slot: Query<(&ComputedNode, &UiGlobalTransform), With<crate::chrome::ViewportSlot>>,
    surface: Option<Res<Surface>>,
    images: Res<Assets<Image>>,
    mut camera: Query<&mut Camera, With<crate::view::MainCamera>>,
) {
    let (Ok((node, tf)), Some(surface), Ok(mut camera)) =
        (slot.single(), surface, camera.single_mut())
    else {
        return;
    };
    let Some(target) = images.get(&surface.image).map(|i| {
        UVec2::new(
            i.texture_descriptor.size.width,
            i.texture_descriptor.size.height,
        )
    }) else {
        return;
    };
    let size = node.size();
    // A zero rect is the frame before its first layout, and a camera given a zero viewport renders
    // nothing at all — which looks exactly like a broken scene and says nothing about why.
    if size.x < 1.0 || size.y < 1.0 {
        return;
    }
    let min = tf.translation - size / 2.0;
    let position = min.max(Vec2::ZERO).as_uvec2();
    // The clamp, and the reason the whole function takes `Assets<Image>`.
    if position.x >= target.x || position.y >= target.y {
        return;
    }
    let physical_size = size.as_uvec2().min(target - position);
    if physical_size.x < 1 || physical_size.y < 1 {
        return;
    }
    let want = bevy::camera::Viewport {
        physical_position: position,
        physical_size,
        ..default()
    };
    // `Viewport` has no `PartialEq` in 0.19, so the two fields that matter are compared by hand
    // rather than the whole struct — the point is only to not mark `Camera` changed every frame.
    let same = camera.viewport.as_ref().is_some_and(|v| {
        v.physical_position == want.physical_position && v.physical_size == want.physical_size
    });
    if !same {
        camera.viewport = Some(want);
    }
}

/// **An injected click has to become a picking event, or the interface is look-but-do-not-touch.**
///
/// `retarget_pointer` gets an agent's cursor as far as `Hovered` — a row lights, a tab lights — and
/// then nothing happens when the button goes down. The reason is one layer further in:
/// `bevy_picking`'s `mouse_pick_events` reads **`WindowEvent`** messages, and takes a press's
/// position from the real cursor it has been tracking (`bevy_picking-0.19.0/src/input.rs:161`,
/// `position: *cursor_last`). The debugger writes `MouseButtonInput` messages, which `InputPlugin`
/// folds into `ButtonInput` — so the button is pressed as far as every key handler in this editor is
/// concerned, and invisible to every observer.
///
/// That gap is why the tab strip could be *hovered* over BRP and not *clicked*, and it would have
/// been read as "the observer is wrong" by whoever looked next.
///
/// So when — and only when — an agent has taken the cursor, the button drives picking too:
/// `PointerAction::Press`/`Release` at [`crate::view::Pointer`]'s position on the surface. **The
/// gate is the injected cursor**, not a flag of its own, because that is already the crate's rule
/// for who owns the pointer (`bevy_debugger_bevy::cursor_position`: *"Precedence, not fallback"*).
/// With no injected cursor this writes nothing and the real mouse takes its ordinary path, so a
/// person's click is never doubled.
///
/// `First`, after `PickingSystems::Input`, because `PointerInput` is *consumed* in `PreUpdate` by
/// `ProcessInput` — emitted any later and it would be read a frame after the press.
#[cfg(feature = "debugger")]
fn inject_clicks(
    surface: Option<Res<Surface>>,
    injected: Option<Res<bevy_debugger_bevy::DebugCursor>>,
    pointer: Option<Res<crate::view::Pointer>>,
    mut presses: MessageReader<bevy::input::mouse::MouseButtonInput>,
    mut out: MessageWriter<bevy::picking::pointer::PointerInput>,
) {
    use bevy::input::ButtonState;
    use bevy::picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput};

    let (Some(surface), Some(injected), Some(pointer)) = (surface, injected, pointer) else {
        return;
    };
    // No injected cursor means the real mouse owns the pointer, and `mouse_pick_events` is already
    // doing this job with the position winit gave it.
    if injected.0.is_none() {
        presses.clear();
        return;
    }
    let Some(at) = pointer.0 else {
        presses.clear();
        return;
    };
    let location = Location {
        target: bevy::camera::NormalizedRenderTarget::Image(ImageRenderTarget {
            handle: surface.image.clone(),
            scale_factor: 1.0,
        }),
        position: at,
    };
    for press in presses.read() {
        let button = match press.button {
            bevy::input::mouse::MouseButton::Left => PointerButton::Primary,
            bevy::input::mouse::MouseButton::Right => PointerButton::Secondary,
            bevy::input::mouse::MouseButton::Middle => PointerButton::Middle,
            _ => continue,
        };
        let action = match press.state {
            ButtonState::Pressed => PointerAction::Press(button),
            ButtonState::Released => PointerAction::Release(button),
        };
        out.write(PointerInput::new(PointerId::Mouse, location.clone(), action));
    }
}

/// **Tell the picking backend the pointer is on the surface, not on the window.**
///
/// See the module note: the backend keeps a pointer only for cameras whose render target equals the
/// pointer's own, so without this every `Hovered` in the editor reads false and a click on a panel
/// row falls through to the map behind it.
///
/// **The position is read from [`crate::view::Pointer`] rather than scaled in place**, and that is
/// not a style choice — it is the only form that is correct. `ProcessInput` rewrites
/// `PointerLocation` when the mouse moves and leaves it alone when it does not, so a system that
/// multiplied the field it found would scale a fresh value once and a stale one again every frame it
/// sat still. Reading the resource that already decided where the pointer is keeps one answer to
/// that question — the rule `view::Pointer`'s own doc states — and it is already in surface space.
///
/// It also buys something the window path could never give: an **injected** cursor now reaches
/// `Hovered`. `sense_pointer` deliberately never writes the window's own cursor, because that would
/// move the physical mouse, so until now an agent's pointer was invisible to picking.
///
/// One frame of latency, because `sense_pointer` runs in `Update` and this runs in `PreUpdate`. At
/// 60 Hz that is below the threshold of a hover reading late, and the alternative is a second place
/// deciding where the cursor is.
fn retarget_pointer(
    surface: Option<Res<Surface>>,
    pointer: Option<Res<crate::view::Pointer>>,
    mut pointers: Query<&mut PointerLocation>,
) {
    let (Some(surface), Some(pointer)) = (surface, pointer) else {
        return;
    };
    let onto = bevy::camera::NormalizedRenderTarget::Image(ImageRenderTarget {
        handle: surface.image.clone(),
        scale_factor: 1.0,
    });

    for mut slot in &mut pointers {
        match pointer.0 {
            // Off the window is not a position, and the backend's own reading of `None` is "this
            // pointer hits nothing" — which is the honest answer rather than a stale last hover.
            None => {
                if slot.location.is_some() {
                    slot.location = None;
                }
            }
            Some(at) => {
                // Compare before writing: `PointerLocation` is change-detected and the hovermap is
                // rebuilt off it, so a blind write every frame would make `Changed` meaningless for
                // everything downstream — the standing constraint `chrome::Follow` records.
                let want = bevy::picking::pointer::Location {
                    target: onto.clone(),
                    position: at,
                };
                if slot.location.as_ref() != Some(&want) {
                    slot.location = Some(want);
                }
            }
        }
    }
}
