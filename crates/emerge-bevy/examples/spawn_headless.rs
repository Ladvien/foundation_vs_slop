//! **One spawner, proven without a GPU or a single asset on disk.**
//!
//! A library and a map are built in code, validated into an `EmergeWorld`, and handed to
//! `EmergePlugin` — which is the same path the editor's preview and the game both take. That is the
//! crate's whole claim: a map cannot look one way in the editor and another in the game, because
//! there is only one function that turns a placement into an entity.
//!
//! Runs with `WgpuSettings { backends: None }`: every render type is registered so material plugins
//! build and `Assets<Mesh>` exists, but no adapter, device or queue is created. Mesh handles resolve
//! to nothing here — which is fine, because what is being demonstrated is placement, not pixels.
//!
//! Run: `cargo run -p emerge-bevy --example spawn_headless`

use bevy::prelude::*;
use emerge_bevy::{draw_yaw, origin_of, EmergePlugin, EmergeWorld, Placement};
use emerge_core::descriptor::{Descriptor, Extent, Face};
use emerge_core::kits::Lattice;
use emerge_core::library::{Library, LIBRARY_VERSION};
use emerge_core::map::{Map, Placed};
use emerge_core::vocab::Vocabularies;

fn descriptor(id: &str, front: Option<Face>) -> Descriptor {
    let mut d = Descriptor {
        id: id.to_owned(),
        mesh: Some(format!("{id}.glb")),
        extent: Extent { footprint: Some((1.0, 1.0)), height: Some(1.0) },
        ..Descriptor::default()
    };
    d.align.front = front;
    d
}

fn placed(id: &str, descriptor: &str, at: (f32, f32), yaw: f32) -> Placed {
    Placed { id: id.to_owned(), descriptor: descriptor.to_owned(), at, yaw, ..Placed::default() }
}

/// The headless `App`, built the way `emerge-mapper`'s harness builds one.
fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .set(bevy::render::RenderPlugin {
                render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                    bevy::render::settings::WgpuSettings { backends: None, ..default() },
                )),
                ..default()
            })
            // Winit would take the thread; frames are stepped by hand here.
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
            // With no render device there is no render app, so a few plugins log an error about
            // its absence. That is expected here and only obscures the output. `emerge-mapper`'s
            // harness drops the log plugin for the same reason.
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins(EmergePlugin);
    app
}

fn main() {
    // ── A map with a hole in it is refused at load, not half-loaded ──────────────────────────────
    let broken = EmergeWorld::new(
        Library { version: LIBRARY_VERSION, note: None, descriptors: vec![descriptor("crate", None)] },
        Map { name: "broken".into(), placements: vec![placed("a", "ghost", (0.0, 0.0), 0.0)], ..Map::default() },
        Vocabularies::default(),
        Lattice::default(),
    );
    match broken {
        Ok(_) => println!("unexpected: a map naming an undefined descriptor loaded"),
        Err(e) => println!("A map naming a descriptor nothing defines is REFUSED:\n  {e}\n"),
    }

    // ── A consistent library + map ───────────────────────────────────────────────────────────────
    let library = Library {
        version: LIBRARY_VERSION,
        note: Some("example kit".into()),
        descriptors: vec![
            descriptor("crate", None),
            // A mesh authored facing East needs a correction on top of the authored yaw. Without it,
            // every chair in a real map came out sideways to its table.
            descriptor("chair", Some(Face::East)),
            descriptor("table", None),
        ],
    };
    let map = Map {
        name: "example_room".into(),
        placements: vec![
            placed("crate_a", "crate", (-2.0, 1.0), 0.0),
            placed("table_1", "table", (0.0, 0.0), 0.0),
            placed("chair_n", "chair", (0.0, -1.0), 0.0),
            placed("chair_s", "chair", (0.0, 1.0), 180.0),
        ],
        ..Map::default()
    };

    let world = match EmergeWorld::new(library, map, Vocabularies::default(), Lattice::default()) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("the example's own library and map disagree: {e}");
            std::process::exit(1);
        }
    };

    println!("Resolved once at load — not per spawn:");
    println!("  {} descriptors, {} placements", world.masks.len(), world.y.len());
    for (p, y) in world.map.placements.iter().zip(world.y.iter()) {
        let pos = origin_of(p.at, world.map.origin, *y);
        // `entry` hands back the descriptor together with its resolved masks.
        let yaw = world.entry(&p.descriptor).map(|(d, _)| draw_yaw(d, p.yaw)).unwrap_or(p.yaw);
        println!(
            "  {:<9} {:<6} at ({:>5.1},{:>5.1}) → world ({:>5.1},{:>5.1},{:>5.1})  authored yaw {:>5.1}° → drawn {:>6.1}°",
            p.id, p.descriptor, p.at.0, p.at.1, pos.x, pos.y, pos.z, p.yaw, yaw,
        );
    }
    println!("\n  The two chairs were authored 180° apart but the mesh's own East front shifts both by");
    println!("  90° — that correction lives in one function, so the editor and the game agree.\n");

    // ── Hand it to the plugin and step frames ────────────────────────────────────────────────────
    let mut app = headless_app();
    let expected = world.map.placements.len();
    app.insert_resource(world);

    // `spawn_world` runs on `Update` when the resource is added, so one step is enough — a second
    // proves it does not re-spawn.
    app.update();
    let after_one = app.world_mut().query::<&Placement>().iter(app.world()).count();
    app.update();
    let after_two = app.world_mut().query::<&Placement>().iter(app.world()).count();

    println!("Spawned entities carrying `Placement`: {after_one} after one frame, {after_two} after two.");
    if after_one == expected && after_two == expected {
        println!("✔ every placement became exactly one entity, and the spawn did not repeat");
    } else {
        eprintln!("✘ expected {expected}; the plugin is not behaving as documented");
        std::process::exit(1);
    }
}
