//! **The editor, stepped rather than run** — one plugin graph, two entry points.
//!
//! `src/main.rs` builds a window and hands control to winit. This builds the *same* editor with no
//! window, no wgpu device and no audio, and returns it for `app.update()` to step.
//!
//! # Why this exists
//!
//! Because the alternative was worse in a way that took a while to admit. Every system in this crate
//! reads and writes resources, so almost everything about the editor is checkable from a test — but
//! the one question a unit test cannot answer is *"does this app get through its first frame"*, and
//! in Bevy 0.19 that question has teeth: a missing `Res<T>` **panics its system** rather than
//! skipping it, and every run condition is evaluated with no short-circuit (`CLAUDE.md`). Those have
//! broken this editor more than any arithmetic in it.
//!
//! With no way to boot the app in a test, the only way to ask was to run the editor and look at it —
//! which meant driving a real keyboard and a real display on a machine somebody else was working on.
//! It was slow, it was unreliable (focus is stolen by whatever else is running, and a stray click
//! lands as a real edit), and it was rude.
//!
//! # This is not a second code path
//!
//! [`add_editor_plugins`] is the one list of the editor's plugins, in the one order that matters, and
//! `main.rs` calls it too. A harness that assembled its own graph would be testing an editor nobody
//! ships — the failure `sim_harness.rs` avoids by the same means, and the reason its comments insist
//! it is "the *same* plugin graph with the device omitted".
//!
//! # No GPU
//!
//! `WgpuSettings { backends: None }` registers every render type — so material plugins build and
//! `Assets<Mesh>` exists — while creating no adapter, device or queue. `sim_harness.rs` measured the
//! same trick on the game and found it changed no simulation output while dropping the GPU
//! requirement entirely. The editor needs it no less: its systems build UI trees and spawn scenes,
//! and none of that reads pixels back.
//!
//! Rendering is therefore genuinely out of scope here. "Does the highlight land on the right cell"
//! is a question for `descriptor::pick_cell`, which answers it in arithmetic.

use std::path::Path;

use bevy::prelude::*;

/// **The editor's own plugins, in the order that matters.** Called by the binary and by the harness.
///
/// `KeysPlugin` is first because it owns `keys::Live`, which three of the others read as a `Res<_>`.
pub fn add_editor_plugins(app: &mut App) -> &mut App {
    app.add_plugins((
        // **First, because it is how this application draws.** Every camera below targets
        // the surface it owns, and `view::setup` reads `Res<Surface>` on `OnEnter(Editor)`.
        crate::surface::SurfacePlugin,
        // **The widget layer's machinery, and the one key it costs.**
        //
        // `FeathersPlugins` brings `TabNavigationPlugin`, whose handler claims `Tab` for focus
        // traversal — and `keys.rs` bound `Tab` to "next panel". Both would have fired: with nothing
        // focused, `dispatch_focused_input` sends to the PRIMARY WINDOW
        // (`bevy_input_focus-0.19.0/src/lib.rs:372`), which is where that observer lives, so it is
        // not gated on the editor having a focus model. The collision test reads `BINDINGS` and
        // cannot see an upstream observer, and the handler is window-scoped so every headless test
        // stays green — it would have surfaced as an author's `Tab` key doing two things at once.
        //
        // Resolved at the keyboard by **retiring `NextTab`**: it only ever did anything on the Kit
        // door, where `1`/`2`/`3` already jump to those same three panels, so what was given up is a
        // duplicate and the census is one row shorter rather than one key more overloaded. `Tab`
        // now means what it means everywhere else.
        //
        // Leaving the plugin out was the alternative and it is worse than it reads: `acquire_focus`
        // is `pub(crate)` and `click_to_focus` is private, so they cannot be registered by hand —
        // dropping the plugin drops all three permanently, and clicking a widget would never move
        // focus.
        crate::chrome::WidgetsPlugin,
        crate::keys::KeysPlugin,
        crate::chrome::ChromePlugin,
        crate::view::ViewPlugin,
        // **Nested, because `add_plugins` tuples cap at 15** (`all_tuples!(.., 0, 15, ..)`,
        // `bevy_app-0.19.0/src/plugin.rs:186`). Grouped rather than renumbered so adding the next
        // one is a nested pair again and not a re-flatten of the whole list.
        (
            crate::compass::CompassPlugin,
            // The one prompt, shared by every feature that asks a question -- see `confirm`.
            crate::confirm::ConfirmPlugin,
            // The key badges. In the shared list because a headless test drives `K` the same way a
            // person does, and because it binds nothing -- unlike `add_debugger_plugins`.
            crate::badges::BadgePlugin,
        ),
        crate::editor::EditorPlugin,
        crate::thumbs::ThumbsPlugin,
        crate::tiles::TilesPlugin,
        crate::compose::ComposePlugin,
        crate::anim_tab::AnimTabPlugin,
        crate::label_booth::LabelBoothPlugin,
        crate::labels::LabelsPlugin,
        crate::notice::NoticePlugin,
        // **Nested, because `add_plugins` tuples cap at 15** (`bevy_app-0.19.0/src/plugin.rs:186`,
        // `all_tuples!(impl_plugins_tuples, 0, 15, P, S)`) and `WidgetsPlugin` was the sixteenth.
        // The error names a `Plugins<_>` bound rather than the cap, so this is worth the comment.
        // Still one list, in one order — the nesting is punctuation, not a second code path.
        (
            // Two plugins, two jobs: the capture rig is the shared crate, the verbs are ours.
            bevy_devshot::DevShotPlugin,
            crate::devshot::DrivePlugin,
        ),
    ));
    // **The guide vocabulary belongs in the shared list; the transport does not.**
    //
    // `add_debugger_plugins` sits outside this function because it binds a port and a test process
    // builds several `App`s. `GuidePlugin` binds nothing — it registers one-shot systems answering
    // `bool` under the editor's own names — and the harness is precisely where they are wanted:
    // `every_checkpoint_a_shipped_guide_names_is_registered` boots this app to prove no shipped
    // script names a condition nobody watches.
    //
    // The `#[cfg]` is on the module rather than on a tuple element, so the list above stays one list.
    #[cfg(feature = "debugger")]
    app.add_plugins(crate::guided::GuidePlugin);
    app
}

/// **The agent's way in, when the feature is on.**
///
/// Separate from [`add_editor_plugins`] rather than a `#[cfg]` inside it, because that list is *the*
/// plugin graph and both entry points share it — this is a graph the harness must be able to build
/// without, and a conditional inside the shared list would make "the same plugin graph" a claim with
/// an asterisk on it.
///
/// **`DebuggerPlugin` owns `RemotePlugin`.** Its `build` adds `RemotePlugin::default()
/// .with_method_main(..)` to register the two custom methods, and Bevy rejects a duplicate plugin by
/// name — so adding `RemotePlugin` here as well panics the moment the feature is switched on. Only
/// the HTTP transport is ours to add. `docs/bevy_debugger_mcp.md` records this; the game paid for it
/// first.
///
/// The port comes from **`BEVY_BRP_PORT`**, which is the variable `bevy_debugger_mcp`'s own config
/// already reads — so one knob points both ends at the same socket, and running the editor and the
/// game with the debugger on at once is a matter of setting it rather than of learning a second
/// vocabulary.
#[cfg(feature = "debugger")]
pub fn add_debugger_plugins(app: &mut App) -> &mut App {
    let port = std::env::var("BEVY_BRP_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(bevy::remote::http::DEFAULT_PORT);
    app.add_plugins((
        bevy_debugger_bevy::DebuggerPlugin,
        bevy::remote::http::RemoteHttpPlugin::default().with_port(port),
        // **The capture target is `crate::surface`'s, and it is in the SHARED list, not this one.**
        // There used to be a `DebugCapturePlugin` here owning a second `Camera3d` that mirrored the
        // map into a square image — and it could never see a panel, because Bevy draws a UI tree to
        // one camera. The editor now draws world and interface into one surface and shows it in the
        // window, so the image an agent reads is the image the author is looking at. That belongs
        // in the shared list because it is how the application draws, not a debugging addition.
    ))
}

/// **The shipped face, installed over Bevy's default asset id.**
///
/// Both entry points do this, because it is not cosmetic: the embedded default is 95 codepoints of
/// ASCII, so every `—` in the editor's copy — and in the refusals `emerge-core` hands back — draws as
/// a tofu box. A harness whose text ran through a different font would measure different layout.
/// **Resize the headless surface**, for a test that needs a window shape the default is not.
///
/// The windowed app resizes through `fit_surface_to_window`, which needs a `PrimaryWindow`; a
/// headless run has none, so this writes the image the way that system would. Everything follows
/// from the image — the cameras render into it and the UI lays out against it — so the caller only
/// has to step a few frames afterwards for the new geometry to land. Sizes are physical texels; at
/// the harness's `UiScale` of 1.2, divide by 1.2 for logical pixels.
pub fn resize_surface(app: &mut App, width: u32, height: u32) -> Result<(), String> {
    let handle = app
        .world()
        .get_resource::<crate::surface::Surface>()
        .ok_or("no Surface resource — is the editor built?")?
        .image
        .clone();
    let mut images = app
        .world_mut()
        .get_resource_mut::<Assets<Image>>()
        .ok_or("no image assets")?;
    let Some(mut image) = images.get_mut(&handle) else {
        return Err("the surface image is gone".into());
    };
    image.resize(bevy::render::render_resource::Extent3d {
        width: width.max(1),
        height: height.max(1),
        depth_or_array_layers: 1,
    });
    Ok(())
}

pub fn install_font(app: &mut App, root: &Path) -> Result<(), String> {
    let path = root.join("assets/fonts/FiraMono-Regular.ttf");
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    app.world_mut()
        .resource_mut::<Assets<Font>>()
        .insert(AssetId::default(), Font::from_bytes(bytes))
        .map_err(|e| format!("cannot install {}: {e}", path.display()))
}

/// **A stepped editor**: the real plugins, no window, no device, no sound.
///
/// `root` is the project directory — the same argument the binary takes, and the asset root, because
/// a descriptor's `mesh` path is relative to the project.
pub fn build_headless(root: &Path, map: &str, kit: Option<&str>) -> Result<App, String> {
    build_headless_at(root, map, kit, crate::tiles::Mode::default())
}

/// **The same editor, opened on a named panel.**
///
/// The caller names the **panel** it wants and the door follows (`Door::showing`), because every
/// `Mode` belongs to exactly one door — asking for both would be two facts that can disagree. A test
/// that wants the Tiles panel gets the Kit door with Tiles showing.
pub fn build_headless_at(
    root: &Path,
    map: &str,
    kit: Option<&str>,
    mode: crate::tiles::Mode,
) -> Result<App, String> {
    let door = crate::tiles::Door::showing(mode);
    let project = crate::project::Project::open(root, kit)?;
    // **The map is its own resource**, because four of the five doors do not have one — see
    // `project::OpenMap`. The headless harness stands up the Maps door, which does.
    let open_map = crate::project::OpenMap::open(&project, map)?;
    let mut app = App::new();

    // **The injected-pointer resource, without the plugin that owns it.**
    //
    // `view::sense_pointer` reads `DebugCursor` to let an agent aim the cursor, and in Bevy 0.19 a
    // missing `Res<T>` panics its system rather than skipping it. `add_debugger_plugins` stays out of
    // here on purpose — it binds a port, and a test process builds several `App`s — so the resource
    // has to be provided on its own. Empty, so `sense_pointer` reads the real window exactly as it
    // does for a person.
    //
    // This went red the moment the `debugger` feature became default, which is the feature working:
    // the panic named the system and the parameter.
    #[cfg(feature = "debugger")]
    app.init_resource::<bevy_debugger_bevy::DebugCursor>();

    // Absolute, for the same reason `main.rs` canonicalizes: a relative `file_path` resolves
    // against `CARGO_MANIFEST_DIR` (the CRATE dir under `cargo test -p`), not the workspace.
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: root.to_string_lossy().into_owned() + "/assets",
                ..default()
            })
            // No window, and nothing that would make the app want to exit when it has none.
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            // Every render type, no device. See the module note.
            .set(bevy::render::RenderPlugin {
                render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                    bevy::render::settings::WgpuSettings {
                        backends: None,
                        ..default()
                    },
                )),
                ..default()
            })
            // Winit would take the thread; the point here is to step frames by hand.
            .disable::<bevy::winit::WinitPlugin>()
            // The tracing subscriber is process-global and a test binary may build several apps, so
            // the first would win and the rest would log an error per build. `sim_harness.rs` makes
            // the same call for the same reason: an `App` in a multi-`App` process must not own
            // process-global state.
            .disable::<bevy::log::LogPlugin>()
            // No sound device: `AudioPlugin` opens a real output stream and a mixer thread per app,
            // for audio no test listens to.
            .disable::<bevy::audio::AudioPlugin>(),
    )
    .insert_resource(project)
    .insert_resource(open_map)
    // **Before the plugins**, so `TilesPlugin`'s `init_resource` leaves both alone. The panel is
    // derived rather than passed: a door opens on its first tab (`Door::opens_on`), so there is no
    // second place to state where the Kit door starts.
    .insert_resource(door)
    .insert_resource(mode)
    // **Straight into the door.** This entry point IS a door — the menu is the other screen, and it
    // is what `Screen::default()` gives. Inserting the state rather than initialising it runs
    // `OnEnter(Editor)` on the first transition, which is where every former `Startup` spawn lives.
    .insert_state(crate::screen::Screen::Editor)
    .insert_resource(ClearColor(crate::chrome::VOID))
    // The same knob the binary sets, by the same name — a literal here would be the second
    // definition the constant exists to prevent, and headless is where a drift goes unseen.
    .insert_resource(UiScale(crate::chrome::EDITOR_UI_SCALE));

    // **Despawning a light or a mesh must not panic in here.** `backends: None` registers every
    // render type but skips the render world — and with it `SyncWorldPlugin`, whose
    // `PendingSyncEntity` resource the `on_remove` hooks of every synced component (`PointLight`,
    // `Mesh3d`, …) unconditionally reach for. The hooks ARE registered, so the first test that
    // retired a staged figure panicked inside `sync_component.rs`. Adding the plugin gives the
    // hooks their ledger; nothing consumes the records without a render world, which over a
    // bounded test run is a small vec nobody reads — the honest cost of stepping frames deviceless.
    if !app.is_plugin_added::<bevy::render::sync_world::SyncWorldPlugin>() {
        app.add_plugins(bevy::render::sync_world::SyncWorldPlugin);
    }

    add_editor_plugins(&mut app);
    install_font(&mut app, &root)?;
    Ok(app)
}
