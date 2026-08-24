//! **emerge-mapper** — a standalone world-building editor.
//!
//! It opens a *project directory*, not a game: `emerge/vocab.ron` says what tokens exist,
//! `emerge/library.ron` says what can be placed, meshes resolve under the same root, and the map it
//! writes is `emerge_core::map::Map` — which any engine can read, because the schema crate has no
//! engine in it. `crates/emerge-core/tests/engine_free.rs` fails the build if that stops being true.
//!
//! ```text
//! emerge-mapper [project-dir] [map-name] [--kit <name>]
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

//! # A library as well as a binary
//!
//! **So the editor can be tested without a screen.** Every system here reads and writes resources,
//! which is exactly what an `App` in a test can drive — but a bin-only crate has nothing an
//! integration test can link against, so the only way to check a system was wired at all was to run
//! the editor and look at it. That meant taking over the machine's keyboard and display to answer
//! questions like "is this resource registered", which is both slow and rude.
//!
//! The game crate made the same split for the same reason (`TESTING.md`). `main.rs` keeps only
//! argument parsing and the app; everything else lives here, where `tests/` can reach it.

pub mod anim_cache;
pub mod args;
pub mod badges;
pub mod anim_plots;
pub mod anim_stage;
pub mod anim_tab;
pub mod anim_watch;
pub mod build;
pub mod chooser;
pub mod chrome;
pub mod confirm;
pub mod compass;
pub mod compose;
pub mod devshot;
pub mod editor;
pub mod fill;
pub mod filter;
#[cfg(feature = "debugger")]
pub mod guided;
pub mod harness;
pub mod keys;
pub mod label_booth;
pub mod labels;
pub mod notice;
pub mod project;
pub mod screen;
pub mod stages;
pub mod surface;
pub mod thumbs;
pub mod tiles;
pub mod token_prompt;
pub mod view;
pub mod vlm;
