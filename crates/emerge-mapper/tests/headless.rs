//! **The editor's wiring, without a window.**
//!
//! Every system in this crate reads and writes resources, which is exactly what an `App` can be made
//! to do in a test. Before the lib/bin split there was nothing to link against, so the only way to
//! find out whether a system was registered — or whether a `Res<T>` it takes exists — was to run the
//! editor and look at it. That meant taking over a machine's keyboard and display to answer questions
//! a test can answer in milliseconds.
//!
//! # What these check, and what they cannot
//!
//! The **arithmetic** is unit-tested where it lives (`descriptor::pick_cell`, `view::pan_direction`,
//! `keys::repeating`). What those tests cannot see is whether a system was added to the schedule, or
//! whether it panics on its first run because a resource nobody registered is missing — which Bevy
//! 0.19 does rather than skipping the system (`CLAUDE.md`), and which is the single most common way
//! this editor has broken.
//!
//! So these boot the real plugins with `MinimalPlugins` — no window, no renderer, no GPU — and run
//! frames. A missing resource, a bad system signature or an ambiguity panics here.
//!
//! Rendering is genuinely out of scope: `MinimalPlugins` draws nothing, so "does the highlight look
//! right" is not a question this file can ask. It asks the one that actually broke things.

use bevy::prelude::*;
use emerge_mapper::harness;

mod fixtures;
use fixtures::Fixture;

/// An app with nothing that needs a screen.
fn headless() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app
}

/// **A resource every system takes must exist the moment the plugin is added.**
///
/// `KeysPlugin`'s own doc says why it goes first: `Live` is read from three plugins, so no one of
/// them can own it, and a missing `Res<T>` panics its system rather than skipping it (Bevy 0.19,
/// `CLAUDE.md`).
///
/// # Registration, not execution
///
/// This deliberately does **not** call `update()`. `init_resource` runs at plugin-build time, so the
/// resources are there to check immediately — whereas running a frame would run `sense_context`,
/// which `KeysPlugin` registers and which reads six resources that *other* plugins own. Adding
/// `KeysPlugin` alone and stepping it panics, which is a fact about plugin order rather than about
/// this registration, and asserting it here would tie this test to the whole editor booting.
///
/// What still needs a real app is "no system panics on frame one", and that needs a renderer for the
/// thumbnail booth and the previews. `TESTING.md`'s `--features test-harness` is how the game crate
/// answers that; the editor has no equivalent yet.
#[test]
fn the_keys_plugin_registers_what_three_other_plugins_read() {
    let mut app = headless();
    app.add_plugins(emerge_mapper::keys::KeysPlugin);

    assert!(
        app.world().get_resource::<emerge_mapper::keys::Live>().is_some(),
        "`Live` is read from three plugins and must be registered by the one that owns it"
    );
    assert!(
        app.world().get_resource::<emerge_mapper::keys::Repeat>().is_some(),
        "`Repeat` is taken by the aim keys; without it that system panics on its first frame"
    );
}

/// The picking resource is registered by the plugin that reads it, on the same rule.
#[test]
fn the_tiles_plugin_registers_the_resources_its_systems_take() {
    let mut app = headless();
    app.add_plugins(emerge_mapper::tiles::TilesPlugin);

    for (name, present) in [
        ("LatticePick", app.world().get_resource::<emerge_mapper::tiles::LatticePick>().is_some()),
        ("CellEdit", app.world().get_resource::<emerge_mapper::tiles::CellEdit>().is_some()),
        ("Mode", app.world().get_resource::<emerge_mapper::tiles::Mode>().is_some()),
        // The Tiles tab's width field. `editor::not_typing` and `editor::sense_context` both read it
        // as a bare `Res`, and both are run conditions — which Bevy 0.19 evaluates with **no**
        // short-circuit, so an unregistered one panics every frame regardless of which tab is live.
        ("ScaleEdit", app.world().get_resource::<emerge_mapper::tiles::ScaleEdit>().is_some()),
    ] {
        assert!(present, "TilesPlugin does not register {name}, so its readers panic on frame one");
    }
}

/// The same contract for the Map tab's three tool resources.
///
/// Each was added beside a system that takes it as a bare `Res`/`ResMut`, and in Bevy 0.19 a missing
/// one **panics its system** rather than skipping it. Registration is checked without `update()` for
/// the reason the test above gives: `init_resource` runs at plugin-build time, and stepping a lone
/// plugin exercises plugin *order* rather than this registration.
#[test]
fn the_editor_plugin_registers_the_tool_resources_its_systems_take() {
    let mut app = headless();
    app.add_plugins(emerge_mapper::editor::EditorPlugin);

    for (name, present) in [
        // The piece in hand, under the move tool.
        ("MoveDrag", app.world().get_resource::<emerge_mapper::editor::MoveDrag>().is_some()),
        // The cell fine placement is confined to while the modifier is down.
        ("FineAnchor", app.world().get_resource::<emerge_mapper::editor::FineAnchor>().is_some()),
        // The box being dragged out to fill.
        ("PlaceDrag", app.world().get_resource::<emerge_mapper::editor::PlaceDrag>().is_some()),
        // What the piece-verbs would act on, written for the UNDER readout. `refresh_status` takes
        // it as a bare `Res<_>`, which panics its system in 0.19 if nobody registered it.
        ("UnderCursor", app.world().get_resource::<emerge_mapper::editor::UnderCursor>().is_some()),
        // The drawn grid's spacing. `draw_map_grid` takes it as a bare `Res<_>`.
        ("GridSpacing", app.world().get_resource::<emerge_mapper::editor::GridSpacing>().is_some()),
    ] {
        assert!(present, "EditorPlugin does not register {name}, so its readers panic on frame one");
    }
}

/// **`Cmd+2` is the bare `2` with a subject**, and the two do not shadow each other.
///
/// The same pairing `S`/`Cmd+S` and `Z`/`Cmd+Z` already rely on: `just_pressed` refuses a bare
/// binding while the modifier is down and a modified one while it is not, so one key can carry both
/// "the Tiles tab" and "the Tiles tab, about this piece". Asserted here because getting it wrong is
/// silent — the bare key would simply switch tabs and the send would look unimplemented.
#[test]
fn sending_a_tile_to_be_edited_is_the_modified_tab_key() {
    use emerge_mapper::keys::{binding, just_pressed, Action, Context, MOD_KEYS};

    let send = binding(Action::EditTile);
    let tab = binding(Action::TilesTab);
    assert_eq!(send.key, tab.key, "it is the tab key, with a modifier");
    assert!(send.needs_mod && !tab.needs_mod);

    // Bare `2` switches tabs and does not send.
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(KeyCode::Digit2);
    assert!(just_pressed(&input, Context::Map, Action::TilesTab));
    assert!(!just_pressed(&input, Context::Map, Action::EditTile));

    // A FRESH input, not `clear()`: `clear` keeps the pressed state, so an already-held key never
    // re-registers as just-pressed and the assertion below would fail for an unrelated reason.
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(MOD_KEYS[0]);
    input.press(KeyCode::Digit2);
    assert!(just_pressed(&input, Context::Map, Action::EditTile));
    assert!(
        !just_pressed(&input, Context::Map, Action::TilesTab),
        "the modified chord must not also switch tabs, or the send would be one frame of a tab change"
    );
}

/// **The move tool has a key, and it is one the left hand can reach without moving.**
///
/// `B` is the last free letter in the `Q W E R T / A S D F G / Z X C V B` cluster. It is also the
/// Tiles tab's `ScanMesh`, which is legal only because the two tabs are never live together — the
/// case `keys::Context` exists to model, and `the_key_space_has_no_collisions` is what polices it.
#[test]
fn the_move_tool_sits_in_the_left_hand_cluster() {
    use emerge_mapper::keys::{binding, Action, Context};
    assert_eq!(binding(Action::MoveMode).key, KeyCode::KeyB);
    assert_eq!(binding(Action::MoveMode).context, Context::Map);
    // Shared with the Tiles tab's mesh rescan, deliberately.
    assert_eq!(binding(Action::ScanMesh).key, KeyCode::KeyB);
    assert_eq!(binding(Action::ScanMesh).context, Context::Tiles);
    assert!(!Context::Map.overlaps(Context::Tiles));
}

/// **The census answers for the action it was asked about.**
///
/// `binding` does not panic on a missing row — it falls back to `BINDINGS[0]`, which is
/// Tab / "next tab". So this asserted the wrong thing twice over: its doc claimed a panic, and its
/// assertions were that the returned row's `chord` and `does` are non-empty, which `BINDINGS[0]`
/// satisfies. **A new `Action` with no row silently bound itself to Tab and this test stayed green.**
///
/// Checking the row's own `action` field is what makes it fail, because that is the one field the
/// fallback cannot fake.
#[test]
fn every_action_resolves_to_its_own_binding_at_runtime() {
    use emerge_mapper::keys::{binding, Action};
    // The ones added most recently, and the ones most likely to be forgotten next.
    for action in [
        Action::ScanMesh,
        Action::RotateMeshX,
        Action::RotateMeshY,
        Action::RotateMeshZ,
        Action::Remove,
        Action::AimReset,
        Action::TurnPieceLeft,
        Action::TurnPieceRight,
        Action::FocusCandidates,
        Action::FocusLibrary,
    ] {
        let b = binding(action);
        assert_eq!(
            b.action, action,
            "{action:?} has no row in the census — it resolved to `{}` ({}), which is what the \
             fallback returns for anything missing",
            b.chord, b.does
        );
        assert!(!b.chord.is_empty(), "{action:?} has no chord to show");
        assert!(!b.does.is_empty(), "{action:?} has no description");
    }
}

/// **Removal moved to `X` and aim-straight to `V`.** Asserted through the census rather than by
/// reading the source, because the census is what the key panel renders and what the editor obeys —
/// if these two disagree with each other the panel is lying.
#[test]
fn the_map_keys_sit_under_the_left_hand() {
    use emerge_mapper::keys::{binding, Action};
    assert_eq!(binding(Action::Remove).key, KeyCode::KeyX);
    assert_eq!(binding(Action::Remove).chord, "X");
    assert_eq!(binding(Action::AimReset).key, KeyCode::KeyV);
    assert_eq!(binding(Action::AimReset).chord, "V");
    // And the mesh operations, which share one row of the key list.
    assert_eq!(binding(Action::ScanMesh).key, KeyCode::KeyB);
    assert_eq!(binding(Action::RotateMeshX).key, KeyCode::KeyN);
    assert_eq!(binding(Action::RotateMeshY).key, KeyCode::KeyO);
    assert_eq!(binding(Action::RotateMeshZ).key, KeyCode::KeyP);
}

/// **Panning is screen-aligned at every rotation detent**, driven through the same function the
/// camera system calls. The unit test in `view` proves the geometry; this proves the geometry the
/// editor actually links against, from outside the crate.
#[test]
fn the_pan_keys_move_the_view_along_the_screen_axes() {
    use emerge_mapper::view::pan_direction;
    use std::f32::consts::TAU;

    for detent in 0..4 {
        let yaw = detent as f32 * TAU / 4.0;
        let forward = pan_direction(Vec2::new(0.0, -1.0), yaw);
        let right = pan_direction(Vec2::new(1.0, 0.0), yaw);
        // Perpendicular, on the ground, and opposite to their own opposites.
        assert!(forward.dot(right).abs() < 1e-4, "detent {detent}: not perpendicular");
        assert!(forward.y.abs() < 1e-6 && right.y.abs() < 1e-6, "panning must stay on the ground");
        assert!(
            (forward + pan_direction(Vec2::new(0.0, 1.0), yaw)).length() < 1e-4,
            "detent {detent}: forward and back must cancel"
        );
        assert!(
            (right + pan_direction(Vec2::new(-1.0, 0.0), yaw)).length() < 1e-4,
            "detent {detent}: right and left must cancel"
        );
    }
}

/// **Ray picking answers for the shipped wall**, from outside the crate and with no camera involved.
///
/// The face convention is the one thing here that a screenshot would have checked badly and a test
/// checks exactly: looking at a wall from +X must report EAST, and the near column, not the far one.
#[test]
fn pointing_at_a_wall_picks_the_face_you_are_looking_at() {
    use emerge_core::descriptor::{pick_cell, Face};
    let origin = [0.0, 0.0, 0.0];
    let size = [3.0, 2.4, 0.5];
    let div = (6, 5, 1);

    let ((x, _, _), face) = pick_cell([9.0, 1.2, 0.25], [-1.0, 0.0, 0.0], origin, size, div)
        .unwrap_or_else(|| panic!("a ray at the wall must hit it"));
    assert_eq!(face, Some(Face::East));
    assert_eq!(x, 5, "the near column");

    // From above: a cell, and no face, because adjacency is horizontal.
    let (_, none) = pick_cell([1.5, 9.0, 0.25], [0.0, -1.0, 0.0], origin, size, div)
        .unwrap_or_else(|| panic!("a ray from above must hit it"));
    assert_eq!(none, None);
}

/// **The whole editor, stepped.**
///
/// These are the tests the GUI driving was standing in for. `harness::build_headless` boots the real
/// plugin graph — the same list `main.rs` uses — with no window, no wgpu device and no audio, and
/// steps it by hand.
///
/// What they catch is the class that has actually broken this editor: in Bevy 0.19 a missing `Res<T>`
/// **panics its system** rather than skipping it, and every run condition is evaluated with no
/// short-circuit. Both are invisible until a frame runs.
mod stepped {
    use super::*;

    /// The workspace root, which is also the editor's project root and asset root.
    fn root() -> std::path::PathBuf {
        // `CARGO_MANIFEST_DIR` is `crates/emerge-mapper`.
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or_else(|| panic!("the crate must live two levels under the workspace"))
            .to_path_buf()
    }

    /// **The editor boots and survives frames.** If any system takes a resource nobody registered,
    /// or a run condition reads an absent one, this panics — which is the whole point.
    ///
    /// Ten frames rather than one: `Startup` runs on the first, and several systems only do work on a
    /// later frame (`rebuild_detail` on a changed resource, `thumbs` after its booth is torn down).
    #[test]
    fn the_editor_boots_and_steps_without_panicking() {
        // A project written for this test. What is being asked is "does the schedule hold together",
        // which has nothing to do with which meshes happen to be in `assets/`.
        let root = Fixture::new("boots")
            .descriptor("wall", "alpha")
            .pack("beta", &["candidate"])
            .place("wall", (0.0, 0.0))
            .build("m");
        let mut app = harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..10 {
            app.update();
        }
    }

    /// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
    ///
    /// The rule is that a test about the *editor* uses `Fixture` and never the real `assets/`, so
    /// importing a kit cannot break the suite. This one is the exception the rule needs: what it
    /// asserts IS a fact about what ships, and checking it against a fixture would be checking that
    /// the fixture is what the fixture is.
    /// **The anim bench's measurement pipeline runs headless**: entering the tab loads the
    /// manifest, the selected rig enters the queue, and one stepped frame later its report exists —
    /// the same three systems the watcher and check-all feed.
    #[test]
    fn the_anim_bench_measures_the_selected_rig_through_the_queue() {
        let mut app = harness::build_headless(&root(), "untitled_map", None)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..2 {
            app.update();
        }
        app.world_mut().insert_resource(emerge_mapper::tiles::Mode::Anim);
        for _ in 0..10 {
            app.update();
        }
        let bench = app.world().resource::<emerge_mapper::anim_tab::BenchState>();
        assert!(bench.loaded, "entering the tab did not load the manifest");
        let selected = bench
            .names()
            .get(bench.selected)
            .map(|s| (*s).to_owned())
            .unwrap_or_else(|| panic!("no selectable rig"));
        let reports = app
            .world()
            .resource::<emerge_mapper::anim_watch::BenchReports>();
        assert!(
            reports.by_rig.contains_key(&selected),
            "no report for `{selected}` after ten frames"
        );
    }

    /// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
    ///
    /// Staging needs a rigged, animated GLB, and there is no honest way to synthesise one: a made-up
    /// skeleton with made-up clips would be asserting that the fixture is what the fixture is. What
    /// this checks is that a REAL rig streams in, gets its blender, and retires — which is a fact
    /// about the shipped asset and the code together.
    /// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
    ///
    /// The rule is that a test about the *editor* uses `Fixture` and never the real `assets/`, so
    /// importing a kit cannot break the suite. This one is the exception the rule needs: what it
    /// asserts IS a fact about what ships, and checking it against a fixture would be checking that
    /// the fixture is what the fixture is.
    /// **The staged figure spawns configured and retires with the tab, without a panic.**
    ///
    /// What this deliberately does NOT assert: the streamed-in `AnimationPlayer` reaching the
    /// blender. Scene assets never finish loading in this deviceless harness — the tiles preview's
    /// root sits at `Loading` forever too — so the attach-and-hold contract is pinned where a real
    /// player exists: `emerge-anim`'s `a_held_phase_freezes_while_weights_still_ease`. What IS this
    /// harness's to check: the stage spawns for the selection with the manifest's scale and the
    /// blend source, the scrub state survives frames, and retiring it does not panic — which it
    /// did, until `build_headless` gave the render-sync hooks their ledger (see the harness).
    #[test]
    fn the_staged_figure_spawns_configured_and_retires() {
        let mut app = harness::build_headless(&root(), "untitled_map", None)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..2 {
            app.update();
        }
        app.world_mut().insert_resource(emerge_mapper::tiles::Mode::Anim);
        for _ in 0..3 {
            app.update();
        }
        {
            let mut bench = app
                .world_mut()
                .resource_mut::<emerge_mapper::anim_tab::BenchState>();
            let at = bench
                .names()
                .iter()
                .position(|n| *n == "valkyrie")
                .unwrap_or_else(|| panic!("no valkyrie in the manifest"));
            bench.selected = at;
        }
        for _ in 0..5 {
            app.update();
        }
        let (transform, source) = app
            .world_mut()
            .query_filtered::<(&Transform, &emerge_anim::BlendSource), With<emerge_mapper::anim_stage::BenchStage>>()
            .iter(app.world())
            .next()
            .map(|(t, s)| (*t, s.slots.len()))
            .unwrap_or_else(|| panic!("no staged figure for the valkyrie"));
        assert_eq!(
            transform.translation,
            emerge_mapper::anim_stage::BENCH_STAGE,
            "the figure stands at the bench's own corner"
        );
        assert!(
            (transform.scale.x - 1.13).abs() < 1.0e-6,
            "the figure wears the manifest's scale, not a literal"
        );
        assert_eq!(source, 10, "all ten valkyrie slots are resident");

        // The default mix is every gait; pausing into a scrub holds across frames without panic.
        {
            let mut scrub = app
                .world_mut()
                .resource_mut::<emerge_mapper::anim_stage::BenchScrub>();
            assert_eq!(scrub.mixed.len(), 6, "six gaits in the default mix");
            scrub.playing = false;
            scrub.phase = 0.25;
        }
        for _ in 0..5 {
            app.update();
        }
        let scrub = app
            .world()
            .resource::<emerge_mapper::anim_stage::BenchScrub>();
        assert!(!scrub.playing && (scrub.phase - 0.25).abs() < 1.0e-6);

        // Leaving the tab retires the stage — lights and all, which is the despawn that used to
        // panic the deviceless world.
        app.world_mut().insert_resource(emerge_mapper::tiles::Mode::Map);
        for _ in 0..3 {
            app.update();
        }
        let remaining = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, With<emerge_mapper::anim_stage::BenchStage>>()
            .iter(app.world())
            .count();
        assert_eq!(remaining, 0, "the staged figure must not outlive the tab");
    }

    /// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
    ///
    /// The rule is that a test about the *editor* uses `Fixture` and never the real `assets/`, so
    /// importing a kit cannot break the suite. This one is the exception the rule needs: what it
    /// asserts IS a fact about what ships, and checking it against a fixture would be checking that
    /// the fixture is what the fixture is.
    /// **The plots paint pixels for a gait rig.** Select the valkyrie (the one rig with gaits),
    /// step, and the height plot's image must be non-uniform — a curve landed.
    #[test]
    fn the_plots_paint_the_valkyrie_curves() {
        let mut app = harness::build_headless(&root(), "untitled_map", None)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..2 {
            app.update();
        }
        app.world_mut().insert_resource(emerge_mapper::tiles::Mode::Anim);
        for _ in 0..3 {
            app.update();
        }
        // Point the selection at the valkyrie by name — the manifest's order is the list's order.
        {
            let mut bench = app
                .world_mut()
                .resource_mut::<emerge_mapper::anim_tab::BenchState>();
            let at = bench
                .names()
                .iter()
                .position(|n| *n == "valkyrie")
                .unwrap_or_else(|| panic!("no valkyrie in the manifest"));
            bench.selected = at;
        }
        for _ in 0..5 {
            app.update();
        }
        let plots = app.world().resource::<emerge_mapper::anim_plots::BenchPlots>();
        assert_eq!(plots.plotted.as_deref(), Some("valkyrie"));
        let handle = plots.height.clone();
        let images = app.world().resource::<Assets<bevy::image::Image>>();
        let data = images
            .get(&handle)
            .and_then(|i| i.data.as_ref())
            .unwrap_or_else(|| panic!("the height plot has no pixel data"));
        let first = &data[0..4];
        assert!(
            data.chunks(4).any(|px| px != first),
            "the height plot is uniform — no curve was painted"
        );
    }

    /// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
    ///
    /// The rule is that a test about the *editor* uses `Fixture` and never the real `assets/`, so
    /// importing a kit cannot break the suite. This one is the exception the rule needs: what it
    /// asserts IS a fact about what ships, and checking it against a fixture would be checking that
    /// the fixture is what the fixture is.
    /// And on a kit, which is a different library, a different policy and 45 more pieces.
    #[test]
    fn the_editor_boots_on_the_site_kit() {
        let mut app = harness::build_headless(&root(), "untitled_map", Some("site"))
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..10 {
            app.update();
        }
        // The kit really did load, so this is not passing on an empty project.
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("the project resource is gone"));
        assert!(
            project.library.descriptors.len() >= 40,
            "the site kit has 45 pieces; got {}",
            project.library.descriptors.len()
        );
        assert_eq!(project.policy.divisions, 1);
    }

    /// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
    ///
    /// The rule is that a test about the *editor* uses `Fixture` and never the real `assets/`, so
    /// importing a kit cannot break the suite. This one is the exception the rule needs: what it
    /// asserts IS a fact about what ships, and checking it against a fixture would be checking that
    /// the fixture is what the fixture is.
    /// **The authored tokens survive the real load path**, and the layered library the editor reads
    /// carries them. This is the end of the chain the whole branch built: measurements on disk →
    /// policy layered → lattice validated → in front of an author.
    #[test]
    fn the_authored_edge_tokens_reach_the_editor() {
        let mut app = harness::build_headless(&root(), "untitled_map", Some("site"))
            .unwrap_or_else(|e| panic!("{e}"));
        app.update();
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("no project"));

        let wall = project
            .library
            .get("site/wall")
            .unwrap_or_else(|| panic!("site/wall is in the kit"));
        let grid = wall
            .subgrid
            .as_ref()
            .unwrap_or_else(|| panic!("site/wall's authored lattice did not survive the load"));
        assert_eq!(
            grid.cells.iter().filter(|c| c.edge.as_deref() == Some("wall")).count(),
            10,
            "the wall's run-faces are five cells each"
        );

        // And the measurements underneath are still unstretched — the kit-corruption fix, checked
        // through the editor's own loader rather than through `write_library`'s tests.
        let measured = project
            .measured
            .get("site/wall")
            .unwrap_or_else(|| panic!("no measured wall"));
        assert_eq!(measured.align.stretch_y, None, "the policy layer must not be in the file");
    }

    /// **The id counter starts past everything the file already names.** It used to start at zero
    /// every session, so reopening a saved map re-minted its own `wall@1`, `wall@2`, … — and undo,
    /// which despawns by id match, then swept the originals off the screen along with the fill it
    /// was taking back. The counter must clear the largest `@n` in the file, whatever shape the
    /// other ids take.
    #[test]
    fn minted_ids_start_past_what_the_map_already_names() {
        let mut map = emerge_core::map::Map::default();
        for id in ["wall@7", "crate@12", "records_desk", "oddly@named@3", "x@notanumber"] {
            map.placements.push(emerge_core::map::Placed {
                id: id.into(),
                descriptor: "wall".into(),
                ..emerge_core::map::Placed::default()
            });
        }
        assert_eq!(emerge_mapper::editor::next_id_after(&map), 12);
        assert_eq!(
            emerge_mapper::editor::next_id_after(&emerge_core::map::Map::default()),
            0,
            "an empty map seeds nothing"
        );
    }

    /// And the seed really lands in the booted editor: a map with `@n` ids on file means the
    /// state's next mint must clear every one of them.
    ///
    /// The ids are written by this test rather than read out of a shipped map, so what it pins is
    /// the rule and not whatever the corpus happens to be numbered up to.
    #[test]
    fn the_booted_editor_seeds_its_id_counter_from_the_file() {
        let root = Fixture::new("mint")
            .descriptor("wall", "alpha")
            .place_as("wall@7", "wall", (0.0, 0.0))
            .place_as("wall@41", "wall", (2.0, 0.0))
            .build("m");
        let mut app = harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        app.update();
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("no project"));
        let want = emerge_mapper::editor::next_id_after(&project.map);
        // The HIGH-WATER MARK, not the next id: `next_id_after` returns the largest `@n` on file
        // and every mint site increments before it formats. Worth pinning, because the name reads
        // like the other thing.
        assert_eq!(want, 41, "the fixture's highest authored id is `wall@41`");
        let state = app
            .world()
            .get_resource::<emerge_mapper::editor::EditorState>()
            .unwrap_or_else(|| panic!("no editor state"));
        assert_eq!(state.minted(), want, "the counter must start where the file stops");
    }

    /// **Folding a pack must not lose it.** The first scan folds packs the library holds nothing
    /// from — but a folded pack is a HEADER with a `>`, never an absence. Every pack directory the
    /// scan found must appear as a text node in the candidate list, folded or not.
    #[test]
    fn untouched_packs_start_folded_and_keep_their_headers() {
        // Two packs of candidates and a map that places from neither, so both fold — the state a
        // fresh map opens into, stated by the fixture rather than inferred from whatever is in
        // `assets/` on the day the test runs.
        let root = Fixture::new("folds")
            .descriptor("wall", "alpha")
            .pack("beta", &["one", "two"])
            .pack("gamma", &["three"])
            .build("m");
        let mut app = harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        app.update();

        // Enter the Tiles tab the way the author does: the Tab key, which is also what triggers
        // the first scan. As a real input MESSAGE, not a hand-set `ButtonInput` — the input plugin
        // clears `just_pressed` at the top of every frame, so a hand-set press is wiped before any
        // editor system can read it.
        let tap = |app: &mut App, state: bevy::input::ButtonState| {
            app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Tab,
                logical_key: bevy::input::keyboard::Key::Tab,
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        };
        tap(&mut app, bevy::input::ButtonState::Pressed);
        tap(&mut app, bevy::input::ButtonState::Released);
        for _ in 0..3 {
            app.update();
        }

        let state = app
            .world()
            .get_resource::<emerge_mapper::tiles::ImportState>()
            .unwrap_or_else(|| panic!("no import state"));
        assert!(state.scanned, "entering the tab must have scanned");
        assert!(!state.candidates.is_empty(), "the fixture wrote three unimported meshes");
        // Recompute the pack directories the way the list groups them.
        let mut dirs: Vec<String> = Vec::new();
        for c in &state.candidates {
            let dir = c.mesh.rsplit_once('/').map_or(".", |(d, _)| d).to_owned();
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        assert!(
            !state.folded_packs.is_empty(),
            "this map places from no candidate pack, so they start folded"
        );
        let folded = state.folded_packs.clone();

        // Every pack — folded or not — must be visible as a header row's text.
        let mut texts: Vec<String> = Vec::new();
        let mut query = app.world_mut().query::<&bevy::ui::widget::Text>();
        for t in query.iter(app.world()) {
            texts.push(t.0.clone());
        }
        for dir in &dirs {
            assert!(
                texts.iter().any(|t| t == dir),
                "pack `{dir}` has no header in the UI — a folded pack must still be a row.\n\
                 folded: {folded:?}"
            );
        }
        let chevrons = texts.iter().filter(|t| t.as_str() == ">").count();
        assert!(
            chevrons >= folded.len().min(dirs.len()),
            "{} folded pack(s) but only {chevrons} `>` chevron(s) rendered",
            folded.len().min(dirs.len())
        );
        // And a folded pack SAYS it is hiding rows — the word is what keeps "folded" from
        // reading as "gone".
        assert!(
            texts.iter().any(|t| t.contains("hidden — click to open")),
            "a folded header must say what it hides"
        );

        // The default selection is VISIBLE: its pack is open, even when every pack started
        // folded — a tab must never open with its selection hidden inside a fold.
        let state = app
            .world()
            .get_resource::<emerge_mapper::tiles::ImportState>()
            .unwrap_or_else(|| panic!("no import state"));
        let sel_dir = state
            .candidates
            .get(state.selected)
            .map(|c| c.mesh.rsplit_once('/').map_or(".", |(d, _)| d).to_owned())
            .unwrap_or_else(|| panic!("the default selection points at nothing"));
        assert!(
            !state.folded_packs.contains(&sel_dir),
            "the selected candidate's pack `{sel_dir}` must be open"
        );
    }

    // **The fold rule is unit-tested beside the code now** (`tiles::pack_fold_tests`), over a
    // synthetic project. It used to be here, asserting that a pack the *library* imports from stays
    // open — the rule until the question moved one step — and it could only check that by reading
    // the shipped corpus. A test bound to the real assets fails the day somebody imports a kit,
    // which is the thing this editor exists to do.

    /// **The first candidate an author clicks stages, like every later one.** Reported live:
    /// "the first mesh I click on doesn't load — I have to click another, then back." The click
    /// path is `on_candidate_click` writing `ImportState::selected`; this drives that exact write
    /// for first-click, second-click and back-again, and asserts the staged preview follows.
    #[test]
    fn the_first_clicked_candidate_stages_like_any_other() {
        // One candidate, written here. What is being asked is whether the FIRST click stages the
        // same way a later one does — a fact about the editor, not about the art.
        let root = Fixture::new("stage")
            .descriptor("wall", "alpha")
            // TWO candidates: the test's whole subject is that the FIRST click behaves like the
            // second, so it needs a second one to compare against.
            .pack("beta", &["candidate_a", "candidate_b"])
            .build("m");
        let mut app = harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        app.update();
        let tap = |app: &mut App, state: bevy::input::ButtonState| {
            app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Tab,
                logical_key: bevy::input::keyboard::Key::Tab,
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        };
        tap(&mut app, bevy::input::ButtonState::Pressed);
        tap(&mut app, bevy::input::ButtonState::Released);
        for _ in 0..3 {
            app.update();
        }

        // Two unblocked candidates to bounce between, by index.
        let (a, b, mesh_a, mesh_b) = {
            let state = app
                .world()
                .get_resource::<emerge_mapper::tiles::ImportState>()
                .unwrap_or_else(|| panic!("no import state"));
            let mut picks = state
                .candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.blocked())
                .map(|(i, c)| (i, c.mesh.clone()));
            let (a, mesh_a) = picks.next().unwrap_or_else(|| panic!("no unblocked candidates"));
            let (b, mesh_b) = picks.next().unwrap_or_else(|| panic!("only one candidate"));
            (a, b, mesh_a, mesh_b)
        };

        let click = |app: &mut App, ix: usize| {
            let mut state = app
                .world_mut()
                .resource_mut::<emerge_mapper::tiles::ImportState>();
            // Exactly what `on_candidate_click` writes.
            state.selected = ix;
            state.selected_library_id = None;
            for _ in 0..3 {
                app.update();
            }
        };
        let staged_mesh = |app: &mut App| -> Option<String> {
            let mut q = app
                .world_mut()
                .query::<&emerge_mapper::tiles::PreviewOf>();
            let metas: Vec<String> = q.iter(app.world()).map(|p| p.0.clone()).collect();
            assert!(metas.len() <= 1, "two staged previews at once: {metas:?}");
            metas.into_iter().next()
        };

        click(&mut app, a);
        assert_eq!(
            staged_mesh(&mut app).as_deref(),
            Some(mesh_a.as_str()),
            "the FIRST clicked candidate must stage"
        );
        click(&mut app, b);
        assert_eq!(staged_mesh(&mut app).as_deref(), Some(mesh_b.as_str()));
        click(&mut app, a);
        assert_eq!(staged_mesh(&mut app).as_deref(), Some(mesh_a.as_str()));
    }

    /// **A missing project is refused, not opened empty.** `Project::open`'s own rule, checked
    /// through the harness because that is the path the binary takes.
    #[test]
    fn a_project_that_is_not_there_is_refused() {
        let err = harness::build_headless(std::path::Path::new("/nonexistent"), "m", None)
            .err()
            .unwrap_or_default();
        assert!(!err.is_empty(), "opening nothing must say so");
    }
}

/// **The Compose tab boots, arms a group, and stamps it.**
///
/// The question a unit test cannot answer: does this app survive its first frame with a fourth tab
/// registered. In Bevy 0.19 a missing `Res<T>` panics its system rather than skipping it, and every
/// run condition is evaluated with no short-circuit — so a plugin that forgets to `init_resource`
/// something its systems take is a crash on launch, not a feature that quietly does nothing.
#[cfg(test)]
mod compose {
    use super::Fixture;
    use emerge_mapper::compose::ComposeState;
    use emerge_mapper::project::Project;
    use emerge_mapper::tiles::Mode;

    fn root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap_or_else(|e| panic!("workspace root: {e}"))
    }

    /// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
    ///
    /// The rule is that a test about the *editor* uses `Fixture` and never the real `assets/`, so
    /// importing a kit cannot break the suite. This one is the exception the rule needs: what it
    /// asserts IS a fact about what ships, and checking it against a fixture would be checking that
    /// the fixture is what the fixture is.
    #[test]
    fn the_compose_tab_boots_and_sees_the_shipped_groups() {
        let mut app = emerge_mapper::harness::build_headless(&root(), "compose_probe", None)
            .unwrap_or_else(|e| panic!("{e}"));
        *app.world_mut().resource_mut::<Mode>() = Mode::Compose;
        for _ in 0..10 {
            app.update();
        }
        let project = app.world().resource::<Project>();
        assert!(
            project
                .compositions
                .compositions
                .iter()
                .any(|c| c.id == "break_table"),
            "the shipped compositions.ron did not reach the editor"
        );
        // Nothing armed is a real state, and it is the one an editor opens in.
        assert!(app.world().resource::<ComposeState>().armed.is_none());
    }

    /// Arming and stamping put a **reference** in the map, not the rows — which is the whole reason
    /// the reference model was chosen, and the thing a flattening implementation would pass every
    /// other test while getting wrong.
    #[test]
    fn stamping_writes_a_reference_and_undo_takes_it_back() {
        // A group written for this test. What is asserted is that a stamp is a REFERENCE and that
        // undo takes it back — neither of which is a fact about which groups happen to ship.
        let root = Fixture::new("stamp")
            .descriptor("table", "alpha")
            .descriptor("chair", "alpha")
            .composition(
                "break_table",
                &[("table", "table", (0.0, 0.0)), ("chair_north", "chair", (0.0, -1.0))],
            )
            .build("m");
        let mut app = emerge_mapper::harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..3 {
            app.update();
        }

        let before = app.world().resource::<Project>().map.placements.len();
        app.world_mut().resource_mut::<ComposeState>().armed = Some("break_table".to_owned());

        // Through the same call the click makes, so this cannot pass while the click path is broken.
        {
            let world = app.world_mut();
            world.resource_scope(|world, mut project: bevy::prelude::Mut<Project>| {
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                let mut compose = ComposeState {
                    armed: Some("break_table".to_owned()),
                    ..Default::default()
                };
                emerge_mapper::editor::stamp_here_for_test(
                    &mut project,
                    &mut state,
                    &mut compose,
                    (2.0, 2.0),
                );
            });
        }
        app.update();

        let project = app.world().resource::<Project>();
        assert_eq!(project.map.stamps.len(), 1, "no stamp landed");
        assert_eq!(project.map.stamps[0].of, "break_table");
        assert_eq!(
            project.map.placements.len(),
            before,
            "expansion must NOT be written into placements — the map holds the reference"
        );

        // **And it comes back off.** `Undo` is closed under inversion, so a stamp has to invert to
        // something that inverts back to a stamp; asserting only the forward direction would pass
        // for an entry that undoes and then cannot be redone.
        emerge_mapper::editor::undo_for_test(app.world_mut());
        app.update();
        assert!(
            app.world().resource::<Project>().map.stamps.is_empty(),
            "undo left the stamp in the map"
        );
        emerge_mapper::editor::redo_for_test(app.world_mut());
        app.update();
        let project = app.world().resource::<Project>();
        assert_eq!(project.map.stamps.len(), 1, "redo did not put the stamp back");
        assert_eq!(project.map.stamps[0].of, "break_table");
    }
}

/// **The backdrop goes under the floor, and the floor is not where you would guess.**
///
/// `BOUNDS_FILL` is drawn below the datum so a placed floor occludes it. That was a flat 5 mm, and
/// the site kit's `site/floor` is authored at `y_offset: -0.06` — six centimetres *into* its own
/// floor, which `stack::datum` documents as the ordinary case for a grate. So the backdrop drew over
/// every floor tile in the map and the grid lines sliced them.
///
/// The recessed piece is written by this test rather than borrowed from a kit: what is being pinned
/// is that the depth is **derived from the library**, which a fixture can state exactly and a corpus
/// can only illustrate.
#[test]
fn the_backdrop_sits_under_the_deepest_floor_in_the_library() {
    let root = Fixture::new("backdrop")
        .descriptor("flat", "alpha")
        .sunk_descriptor("grate", "alpha", -0.06)
        .sunk_descriptor("deep_drain", "alpha", -0.21)
        .build("m");
    let app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    let project = app
        .world()
        .get_resource::<emerge_mapper::project::Project>()
        .unwrap_or_else(|| panic!("no project"));
    let drop = emerge_mapper::editor::ground_drop(project);
    for d in &project.library.descriptors {
        let sunk = d.align.y_offset.unwrap_or(0.0);
        assert!(
            -drop < sunk,
            "the backdrop sits at {:.4} m and `{}` is authored at {sunk:.4} m — it would be drawn over",
            -drop,
            d.id
        );
    }
    assert!(
        -drop < -0.21,
        "the depth must follow the DEEPEST piece, not the first one it finds: {:.4}",
        -drop
    );
}

/// **The grid defaults to the kit's module, not to the snap.**
///
/// `grid::SNAP` is 0.5 — where a piece can land — and the site kit builds on a 1 m module, so a
/// grid fixed to the snap draws two squares per floor tile and reads as though the tiles were
/// straddling it. The author owns the setting (`J`); this pins where it starts.
#[test]
fn the_drawn_grid_starts_at_the_kits_module() {
    let mut app = headless();
    app.add_plugins(emerge_mapper::editor::EditorPlugin);
    let spacing = app
        .world()
        .get_resource::<emerge_mapper::editor::GridSpacing>()
        .unwrap_or_else(|| panic!("EditorPlugin does not register GridSpacing"));
    assert!(
        (spacing.0 - 1.0).abs() < 1e-6,
        "the grid starts at {} m; a square is meant to be one kit tile",
        spacing.0
    );
    assert!(
        spacing.0 > emerge_core::grid::SNAP,
        "a default finer than the snap would draw lines no piece can land on"
    );
}


/// **The fixture boots** — a project written from nothing, with no shipped asset in it but the font.
#[test]
fn a_synthetic_project_opens_and_steps() {
    let root = Fixture::new("smoke")
        .descriptor("wall", "alpha")
        .descriptor("crate", "beta")
        .pack("gamma", &["unimported_a", "unimported_b"])
        .place("crate", (0.0, 0.0))
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    for _ in 0..3 {
        app.update();
    }
    let project = app
        .world()
        .get_resource::<emerge_mapper::project::Project>()
        .unwrap_or_else(|| panic!("no project"));
    assert_eq!(project.library.descriptors.len(), 2, "two descriptors were written");
    assert_eq!(project.map.placements.len(), 1, "one placement was written");
}
