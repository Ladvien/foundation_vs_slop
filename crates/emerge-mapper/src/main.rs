//! **emerge-mapper** — the binary. Argument parsing and the app; everything else is the
//! library beside this, so `tests/` can drive the editor without a window. See `lib.rs`.

use emerge_mapper::{args, chooser, harness, screen, screen::Screen};

use bevy::prelude::*;

/// **One application, two screens.**
///
/// This was three processes: a supervisor that never opened a window, a menu process per lap, and an
/// editor process per door. The shape existed because **winit builds at most one event loop per
/// process** — a second `App::run` dies on `RecreationAttempt` — so a menu you could return to had
/// to be a fresh process, and a supervisor had to spawn it.
///
/// Asked for at the keyboard, 2026-08-16: *"can we not open a whole another editing window? I'd like
/// to keep the same bevy application running across whether it's the UI or the editor."*
///
/// The winit limit was never the real obstacle: one process can show a menu and an editor in one
/// window as long as they are two *states* rather than two `App`s. The real obstacle was that in
/// Bevy 0.19 a **missing `Res<T>` panics its system**, and about a hundred parameter positions take
/// `Res<Project>` — so a menu inside the editor's `App` meant gating every one of them. That is what
/// `screen.rs` now does, and this file is what a binary should be again: parse arguments, build one
/// app, run it.
fn main() -> AppExit {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let root = args::root_of(&argv);

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                // Meshes are named relative to the project, so the project IS the asset root.
                // Absolute, because Bevy resolves a relative `file_path` against `CARGO_MANIFEST_DIR`
                // — which under `cargo run -p emerge-mapper` is the CRATE, so `./assets` landed on
                // `crates/emerge-mapper/assets` and every mesh 404'd.
                file_path: root.to_string_lossy().into_owned() + "/assets",
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    // **One window for both screens**, so the title is state rather than a build-time
                    // decision — see [`name_the_window`].
                    title: "emerge-mapper".to_owned(),
                    // **Fullscreen for automated checks.** A virtual pointer is ABSOLUTE over the
                    // whole output, so a screen fraction only means a window fraction when the
                    // window IS the screen.
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
    .insert_resource(ClearColor(Color::srgb(0.035, 0.033, 0.030)))
    // **One knob for the whole interface.** `UiScale` multiplies every `Val::Px` and every font
    // size, so the panels grow together and nothing has to be re-tuned relative to anything else.
    .insert_resource(UiScale(emerge_mapper::chrome::EDITOR_UI_SCALE));

    // The state both halves are gated by, and the two transitions that load and unload a door.
    app.add_plugins(screen::ScreenPlugin);

    // **Both screens' plugins, always added.** Which one *runs* is the state's business, not the
    // plugin list's — a list that changed with the screen would be two plugin graphs, and only one
    // of them would ever be the one under test.
    // `--kit` off the one splitter, so the menu's preselection and the door's authoring kit cannot
    // disagree about which word was the flag's value.
    app.add_plugins(chooser::ChooserPlugin {
        root: root.clone(),
        preselect: args::split(&argv).kit.map(str::to_owned),
    });
    harness::add_editor_plugins(&mut app);
    // Behind the feature, so an ordinary build has no `bevy_remote` in its dependency graph at all.
    #[cfg(feature = "debugger")]
    harness::add_debugger_plugins(&mut app);

    app.add_systems(Update, name_the_window);

    // **A door named on the command line skips the menu**, by choosing on its behalf: the menu's
    // output IS this argv (`args::open`), so `emerge-mapper . --door kit --kit ozea` and pressing
    // Enter on the same row take the identical path in.
    //
    // **Opened here, before the app runs**, for the same reason `install_font` refuses here: a door
    // that will not open has to be said on the terminal the flag was typed into. There is no screen
    // to draw it on yet, and entering `Screen::Editor` without a project is a panic rather than a
    // message — see `chooser::Chosen`.
    if args::names_a_door(&argv) {
        match args::open(&argv) {
            Ok(opened) => {
                app.insert_resource(chooser::Chosen(opened));
                app.add_systems(Startup, |mut next: ResMut<NextState<Screen>>| {
                    next.set(Screen::Editor);
                });
            }
            Err(e) => {
                eprintln!("emerge-mapper: {e}");
                std::process::exit(1);
            }
        }
    }

    // **Replace the default face rather than hand a handle to 41 call sites.** `TextFont::default()`
    // names `AssetId::default()`, which is where `TextPlugin` puts the 95-codepoint subset.
    if let Err(e) = harness::install_font(&mut app, &root) {
        eprintln!("emerge-mapper: {e}");
        std::process::exit(1);
    }

    app.run()
}

/// **The window says which screen it is showing**, and follows a door change.
///
/// It was set once when the window was built, which was fine while a door was a process and its
/// title could be decided before the window existed. One window across both screens makes the title
/// state — and a stale title is exactly how an author ends up in the Map door believing they are in
/// the Kit door, which is where an evening went on 2026-08-16 before the log said `— map — test`.
fn name_the_window(
    screen: Res<State<Screen>>,
    door: Option<Res<emerge_mapper::tiles::Door>>,
    open_map: Option<Res<emerge_mapper::project::OpenMap>>,
    project: Option<Res<emerge_mapper::project::Project>>,
    mut windows: Query<&mut Window>,
) {
    let want = match (screen.get(), door.as_deref()) {
        (Screen::Menu, _) => "emerge-mapper — choose a kit or a map".to_owned(),
        (Screen::Editor, Some(d)) => {
            let door = d.label().to_lowercase();
            match (open_map.as_deref(), project.as_deref()) {
                (Some(m), _) => format!("emerge-mapper — {door} — {}", m.map.name),
                (None, Some(p)) => format!("emerge-mapper — {door} — {}", p.namespace),
                _ => format!("emerge-mapper — {door}"),
            }
        }
        (Screen::Editor, None) => "emerge-mapper".to_owned(),
    };
    for mut w in &mut windows {
        if w.title != want {
            w.title = want.clone();
        }
    }
}
