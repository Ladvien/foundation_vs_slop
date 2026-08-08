//! **Booting the whole editor with no window, no GPU and no keyboard.**
//!
//! `harness::build_headless` assembles the *same* plugin graph `src/main.rs` ships — via the one
//! `add_editor_plugins` list — with `WgpuSettings { backends: None }` and no primary window, and
//! hands it back for `app.update()` to step.
//!
//! That matters more here than the usual "tests are nice" argument. In Bevy 0.19 a missing `Res<T>`
//! **panics its system** rather than skipping it, and every run condition is evaluated with no
//! short-circuit — so "does this app survive its first frame" is a question no unit test can answer
//! and no amount of arithmetic coverage substitutes for. Before the lib/bin split the only way to ask
//! was to run the editor and look at it, which meant taking over somebody's keyboard and display.
//!
//! Run: `cargo run -p emerge-mapper --example boot_headless -- <project-root> [map-name]`
//!
//! `<project-root>` is a directory holding `assets/` and the map RON — the same argument the binary
//! takes. No project is fabricated here: without one, this prints usage and stops.

use std::path::Path;

use emerge_mapper::harness;

const FRAMES: usize = 12;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(root) = args.get(1) else {
        eprintln!("usage: cargo run -p emerge-mapper --example boot_headless -- <project-root> [map-name]");
        eprintln!();
        eprintln!("  <project-root>  a directory containing `assets/` and the map RON files");
        eprintln!("  [map-name]      defaults to `untitled_map`; created if the project has none");
        eprintln!();
        eprintln!("The editor needs a real project on disk, and inventing one here would be a second");
        eprintln!("source of truth for what a project looks like. Point it at yours.");
        std::process::exit(2);
    };
    let map = args.get(2).map(String::as_str).unwrap_or("untitled_map");

    let root = Path::new(root);
    if !root.is_dir() {
        eprintln!("`{}` is not a directory", root.display());
        std::process::exit(2);
    }

    println!("Building the editor headless from {} (map `{map}`)...", root.display());
    let mut app = match harness::build_headless(root, map, None) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("\nbuild_headless refused:\n  {e}");
            eprintln!("\nThat refusal is the feature — a project with a hole in it is rejected at the");
            eprintln!("door rather than half-loaded into an editor that looks fine until you save.");
            std::process::exit(1);
        }
    };

    println!("Built. Stepping {FRAMES} frames — every system runs, nothing is drawn.\n");
    for frame in 0..FRAMES {
        app.update();
        if frame == 0 {
            println!("  frame 0 survived — no system panicked on a missing resource");
        }
    }

    let entities = app.world().entities().len();
    println!("  {FRAMES} frames stepped, {entities} entities alive");
    println!("\n✔ the shipped plugin graph boots, in milliseconds, on a machine with no display.");
    println!("  This is the sanctioned way to check the editor's wiring — never drive a real keyboard.");
}
