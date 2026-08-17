//! **emerge-mapper** — the binary. Argument parsing and the app; everything else is the
//! library beside this, so `tests/` can drive the editor without a window. See `lib.rs`.

use emerge_mapper::{harness, project};

use std::path::PathBuf;

use bevy::prelude::*;

use project::Project;

fn main() {
    // **`--kit` is pulled out first**, so the two positional arguments keep meaning exactly what they
    // meant before it existed. A kit is a directory under `assets/emerge/` holding a library and its
    // policy layer: the default one is furniture, `--kit site` is the 45-piece architectural set whose
    // walls, corners, doorways and pipes are what edge tokens are for.
    let mut positional: Vec<String> = Vec::new();
    let mut kit: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--kit" => match args.next() {
                Some(name) => kit = Some(name),
                None => {
                    eprintln!("emerge-mapper: --kit needs a name, e.g. `--kit site`");
                    std::process::exit(1);
                }
            },
            other => positional.push(other.to_owned()),
        }
    }
    let root = PathBuf::from(
        positional.first().cloned().unwrap_or_else(|| ".".to_owned()),
    );
    // **Absolute, before anything derives from it.** The asset root below is handed to Bevy's
    // `AssetPlugin.file_path`, which resolves RELATIVE paths against `BEVY_ASSET_ROOT` or
    // `CARGO_MANIFEST_DIR` — and under `cargo run -p emerge-mapper` the manifest dir is the CRATE,
    // so `./assets` landed on `crates/emerge-mapper/assets` and every mesh 404'd. An absolute
    // path replaces whatever base Bevy picked (`Path::join`'s contract), so the editor works the
    // same from `cargo run`, the bare binary, or any cwd — no env var required.
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    // A NAME, not a filename — `emerge-mapper . site_67` opens assets/emerge/site_67.map.ron.
    let map_name = positional
        .get(1)
        .cloned()
        .unwrap_or_else(|| "untitled_map".to_owned());

    // **Open the project before standing up the window.** A failure here is fatal and prints what is
    // wrong with which file; the alternative is an editor that comes up with an empty palette, which
    // looks exactly like an editor whose project has no assets. Same call `SourceMap::parse` makes.
    let project = match Project::open(&root, &map_name, kit.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("emerge-mapper: cannot open {}: {e}", root.display());
            std::process::exit(1);
        }
    };

    let project_name = project.map.name.clone();

    let mut app = App::new();
    app
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    // Meshes are named relative to the project, so the project IS the asset root.
                    file_path: root.to_string_lossy().into_owned() + "/assets",
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: format!("emerge-mapper — {}", project_name),
                        // **Fullscreen for automated checks.** The way this editor gets verified is
                        // `scripts/vinput.py` driving a real kernel input device and
                        // `scripts/framestats.py` measuring the frame — and a virtual pointer is
                        // ABSOLUTE over the whole output, so a screen fraction only means a window
                        // fraction when the window IS the screen. Under a tiling WM the editor gets
                        // an arbitrary slot, and clicks aimed at the palette land on the desktop
                        // instead, which looks exactly like a palette that does not respond.
                        mode: if std::env::var("EMERGE_FULLSCREEN").as_deref() == Ok("1") {
                            bevy::window::WindowMode::BorderlessFullscreen(
                                bevy::window::MonitorSelection::Primary,
                            )
                        } else {
                            bevy::window::WindowMode::Windowed
                        },
                        ..default()
                    }),
                    ..default()
                }),
        )
        .insert_resource(project)
        .insert_resource(ClearColor(emerge_mapper::chrome::VOID))
        // **One knob for the whole interface.** `UiScale` multiplies every `Val::Px` and every font
        // size, so the panels grow together and nothing has to be re-tuned relative to anything else
        // — the alternative is forty constants that drift apart the first time one is missed.
        .insert_resource(UiScale(1.2));

    // **The one list of the editor's plugins**, shared with `harness::build_headless`. A binary and a
    // test that each assembled their own graph would be two editors, and the tests would be checking
    // the one nobody ships.
    harness::add_editor_plugins(&mut app);
    // Behind the feature, so an ordinary build has no `bevy_remote` in its dependency graph at all.
    // Deliberately NOT in `build_headless`: a test app must not bind a socket, and several `App`s
    // are built per test process — the second would fail on an address already in use.
    #[cfg(feature = "debugger")]
    harness::add_debugger_plugins(&mut app);

    // **Replace the default face rather than hand a handle to 41 call sites.** `TextFont::default()`
    // names `AssetId::default()`, which is exactly where `TextPlugin` puts the subset
    // (`bevy_text-0.19.0/src/lib.rs:146`) — so overwriting that one asset re-points every existing
    // `TextFont::from_font_size` at the full 1350-codepoint face at once.
    //
    // This is deliberately NOT the game's `FontAssets` pattern (`docs/ui.md` §5). That rule exists
    // because a call site reaching for `Handle::default()` got the subset; here the default IS the
    // shipped face, so there is no second thing to reach for and no site that can forget. One binary,
    // one face, one place it is decided.
    if let Err(e) = harness::install_font(&mut app, &root) {
        eprintln!("emerge-mapper: {e}");
        std::process::exit(1);
    }

    app.run();
}
