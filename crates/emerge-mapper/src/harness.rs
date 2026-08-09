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
        crate::keys::KeysPlugin,
        crate::chrome::ChromePlugin,
        crate::view::ViewPlugin,
        crate::editor::EditorPlugin,
        crate::thumbs::ThumbsPlugin,
        crate::tiles::TilesPlugin,
        crate::compose::ComposePlugin,
        crate::anim_tab::AnimTabPlugin,
        crate::label_booth::LabelBoothPlugin,
        crate::labels::LabelsPlugin,
        // Two plugins, two jobs: the capture rig is the shared crate, the verbs are ours.
        bevy_devshot::DevShotPlugin,
        crate::devshot::DrivePlugin,
    ))
}

/// **The shipped face, installed over Bevy's default asset id.**
///
/// Both entry points do this, because it is not cosmetic: the embedded default is 95 codepoints of
/// ASCII, so every `—` in the editor's copy — and in the refusals `emerge-core` hands back — draws as
/// a tofu box. A harness whose text ran through a different font would measure different layout.
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
    let project = crate::project::Project::open(root, map, kit)?;
    let mut app = App::new();

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
    .insert_resource(ClearColor(Color::srgb(0.035, 0.033, 0.030)))
    .insert_resource(UiScale(1.2));

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
