//! **emerge-mapper** — a standalone world-building editor.
//!
//! It opens a *project directory*, not a game: `emerge/vocab.ron` says what tokens exist,
//! `emerge/library.ron` says what can be placed, meshes resolve under the same root, and the map it
//! writes is `emerge_core::map::Map` — which any engine can read, because the schema crate has no
//! engine in it. `crates/emerge-core/tests/engine_free.rs` fails the build if that stops being true.
//!
//! ```text
//! emerge-mapper [project-dir] [map-name]
//! ```
//!
//! The second argument is a **name**, not a filename: `emerge-mapper . site_67` opens (or starts)
//! `assets/emerge/site_67.map.ron`. Names are snake_case and are *forced* into it rather than
//! checked, so there is no path through this program on which a map is called something the
//! filesystem and the schema disagree about. Both arguments default, so a bare
//! `cargo run -p emerge-mapper` from the workspace root opens the shipped library.
//!
//! # Why it is a separate binary and not a mode of the game
//!
//! `sim_harness.rs` is the precedent: the same plugin graph, a different entry point, *"not a second
//! code path"*. An editor welded into the game inherits the game's title screen, its save system, its
//! camera rules and its notion of what a level is — which is exactly how the F7 Site editor ended up
//! only able to edit one hub. The reusable part is the schema and the solvers, and those live in
//! `emerge-core` where a second game can have them.
//!
//! # Before writing any Bevy here, read the vendored 0.19 source
//!
//! Not bevy.org, which tracks `main`. `CLAUDE.md` lists the traps this project has already paid for;
//! the two that bite an editor hardest are that **a `Single<.., With<Camera3d>>` silently skips its
//! system** when a second 3D camera exists, and that **every run condition is evaluated** — no
//! short-circuit — so a bare `Res<T>` in a `.run_if` panics whenever that resource is absent.

mod devshot;
mod editor;
mod fill;
mod keys;
mod tiles;
mod project;
mod thumbs;
mod view;

use std::path::PathBuf;

use bevy::prelude::*;

use project::Project;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().unwrap_or_else(|| ".".to_owned()));
    // A NAME, not a filename — `emerge-mapper . site_67` opens assets/emerge/site_67.map.ron.
    let map_name = args.next().unwrap_or_else(|| "untitled_map".to_owned());

    // **Open the project before standing up the window.** A failure here is fatal and prints what is
    // wrong with which file; the alternative is an editor that comes up with an empty palette, which
    // looks exactly like an editor whose project has no assets. Same call `SourceMap::parse` makes.
    let project = match Project::open(&root, &map_name) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("emerge-mapper: cannot open {}: {e}", root.display());
            std::process::exit(1);
        }
    };

    let project_name = project.map.name.clone();
    App::new()
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
        .insert_resource(ClearColor(Color::srgb(0.035, 0.033, 0.030)))
        // **One knob for the whole interface.** `UiScale` multiplies every `Val::Px` and every font
        // size, so the panels grow together and nothing has to be re-tuned relative to anything else
        // — the alternative is forty constants that drift apart the first time one is missed.
        .insert_resource(UiScale(1.2))
        .add_plugins((
            view::ViewPlugin,
            editor::EditorPlugin,
            thumbs::ThumbsPlugin,
            tiles::TilesPlugin,
            devshot::DevShotPlugin,
        ))
        .run();
}
