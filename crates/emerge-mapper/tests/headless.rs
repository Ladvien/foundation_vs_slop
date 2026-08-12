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

/// **`Cmd`+remove opens the piece under the cursor for editing**, and does not collide with the bare
/// remove key the Tiles tab uses.
///
/// It was `Cmd`+the tab key, paired with "2 switches to Tiles" so one key carried both the tab and
/// the tab-about-this-piece. An author asked for it on the remove chord instead — "get this out of my
/// way and let me fix it" — and the bare remove key is unbound on the Map, so the new home pairs with
/// nothing. The collision that WOULD matter is the Tiles tab's own bare and shifted remove, and
/// `just_pressed` separates them by modifier the same way `S`/`Cmd+S` are separated.
#[test]
fn opening_a_piece_to_be_defined_is_the_modified_remove_key() {
    use emerge_mapper::keys::{binding, just_pressed, Action, Context, MOD_KEYS, REMOVE_KEY};

    let send = binding(Action::EditTile);
    assert_eq!(send.key, REMOVE_KEY, "it is the remove key, with the command modifier");
    assert!(send.needs_mod);

    // Bare remove on the Tiles tab removes; it does not send anything to be defined.
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(REMOVE_KEY);
    assert!(just_pressed(&input, Context::Meshes, Action::RemoveTile));
    assert!(!just_pressed(&input, Context::Meshes, Action::EditTile));

    // A FRESH input, not `clear()`: `clear` keeps the pressed state, so an already-held key never
    // re-registers as just-pressed.
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(MOD_KEYS[0]);
    input.press(REMOVE_KEY);
    assert!(just_pressed(&input, Context::Map, Action::EditTile));
    assert!(
        !just_pressed(&input, Context::Meshes, Action::RemoveTile),
        "the modified chord must not also remove, or one press would do two things"
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
    assert_eq!(binding(Action::ScanMesh).context, Context::Meshes);
    assert!(!Context::Map.overlaps(Context::Meshes));
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
        assert_eq!(project.policy.face_bands, 1);
    }

    /// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
    ///
    /// Reported live: sending `site/floor` over from the PLACE list "didn't open the item in Tiles",
    /// and a second piece did. `edit_subject` is unit-tested and answers `site/floor` correctly, so
    /// what is asserted here is the other half — that the door the answer is handed to actually
    /// opens, **on the first send of a session**, for the piece that failed.
    ///
    /// It reads the shipped kit deliberately: the report is about `site/floor`, which is a member of
    /// all four authored site tiles, and a fixture would be checking that the fixture is what the
    /// fixture is. The pair `rebuild_detail` guards on is the thing under suspicion — it needs the
    /// id in **both** `measured` and the layered library, while the door only checked the latter.
    #[test]
    fn the_first_send_of_a_session_opens_the_piece_it_names() {
        let mut app = harness::build_headless(&root(), "untitled_map", Some("site"))
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..10 {
            app.update();
        }

        // Untouched: this is the first send of the session, which is the case reported.
        assert!(
            !app.world()
                .resource::<emerge_mapper::tiles::ImportState>()
                .scanned,
            "this test is about the FIRST send — a scanned tab is a different case"
        );

        let world = app.world_mut();
        world.resource_scope(|world, project: bevy::prelude::Mut<emerge_mapper::project::Project>| {
            world.resource_scope(|world, mut import: bevy::prelude::Mut<emerge_mapper::tiles::ImportState>| {
                let mut mode = world.resource_mut::<emerge_mapper::tiles::Mode>();
                let mut state = emerge_mapper::editor::EditorState::default();
                emerge_mapper::editor::send_to_tiles_for_test(
                    Ok("site/floor".to_owned()),
                    &project,
                    &mut state,
                    &mut mode,
                    &mut import,
                );
                assert!(
                    !state.status.has_problem(),
                    "the door refused `site/floor`: {}",
                    state.status.problem_text()
                );
                assert_eq!(
                    import.selected_library_id.as_deref(),
                    Some("site/floor"),
                    "the piece was not focused on the Tiles tab"
                );
                assert!(
                    matches!(*mode, emerge_mapper::tiles::Mode::Meshes),
                    "the tab did not change"
                );
                // **The pair the detail pane guards on.** `send_to_tiles` checks the layered
                // library; the pane needs the measurements too, and returns early showing nothing
                // when they disagree — which is exactly "it switched tabs and the item wasn't there".
                assert!(
                    import.editing(&project.measured).is_some(),
                    "`site/floor` is not in the MEASURED layer, so the detail pane draws nothing"
                );
                assert!(
                    import.placed(&project).is_some(),
                    "`site/floor` is not in the layered library as placed"
                );
            });
        });
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

    /// **Enter on a tile already in the library updates it; it does not refuse.**
    ///
    /// Reported live: *"I make changes to a tile on the Tiles tab, but when I go to save it it says
    /// there's already that item."* Every field on that pane writes through `persist` as it is
    /// edited, so the author had already saved — and the save key answered *"already in the library
    /// — pick a candidate below to add one"*, which reads as a refusal of the work they just did.
    ///
    /// Driven through the key MESSAGE rather than a hand-set `ButtonInput`, because the input plugin
    /// clears `just_pressed` at the top of every frame.
    #[test]
    fn enter_on_a_library_tile_updates_it_rather_than_refusing() {
        let root = Fixture::new("update").descriptor("wall", "alpha").build("m");
        let mut app = harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        app.update();

        let tap = |app: &mut App, key: KeyCode, logical: bevy::input::keyboard::Key| {
            for state in [bevy::input::ButtonState::Pressed, bevy::input::ButtonState::Released] {
                app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
                    key_code: key,
                    logical_key: logical.clone(),
                    state,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
                app.update();
            }
        };
        tap(&mut app, KeyCode::Tab, bevy::input::keyboard::Key::Tab);
        for _ in 0..3 {
            app.update();
        }

        // Focus the library entry — exactly what `on_library_click` and the Map's `Cmd`+remove
        // both write, and the one discriminant `ImportState::editing` follows.
        {
            let mut state = app
                .world_mut()
                .resource_mut::<emerge_mapper::tiles::ImportState>();
            state.selected_library_id = Some("wall".to_owned());
        }
        for _ in 0..3 {
            app.update();
        }

        // **Counted, not "is the log empty".** Entering the tab with nothing focused raises its own
        // problem, so an emptiness check here would be reading somebody else's message and passing
        // or failing for the wrong reason.
        let before = app
            .world()
            .get_resource::<emerge_mapper::tiles::ImportState>()
            .map(|s| s.status.problems().len())
            .unwrap_or_else(|| panic!("no import state"));

        tap(&mut app, KeyCode::Enter, bevy::input::keyboard::Key::Enter);
        for _ in 0..3 {
            app.update();
        }

        let state = app
            .world()
            .get_resource::<emerge_mapper::tiles::ImportState>()
            .unwrap_or_else(|| panic!("no import state"));
        assert_eq!(
            state.status.problems().len(),
            before,
            "committing an unchanged library tile must raise nothing new: {}",
            state.status.problem_text()
        );
        let said = state.status.note_text();
        assert!(
            !said.contains("pick a candidate"),
            "the save key must not send an author who edited a tile off to add a different one: {said}"
        );
        assert!(
            said.contains("up to date"),
            "Enter has to report what it did to the focused tile, and it said: {said}"
        );

        // Nothing was replaced or dropped on the way through the write.
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("no project"));
        assert!(
            project.library.get("wall").is_some(),
            "an update must leave the tile in the library"
        );
        assert_eq!(
            project.measured.descriptors.len(),
            1,
            "an update writes the entry that is there — it never adds a second"
        );
    }

    /// **A candidate whose id is already taken is refused, and told where to update instead.**
    ///
    /// The other half of the Tiles-tab spec: *"update what's there, not replace it."* Enter on a
    /// **library** entry updates it — `enter_on_a_library_tile_updates_it_rather_than_refusing`
    /// pins that. Enter on a **candidate** that names the same id must not: a candidate is what a
    /// mesh scan can see, so it carries no tags, note, mount or lattice, and writing it over the
    /// entry would take those out. That is the replace the author ruled against.
    ///
    /// The candidate is staged directly rather than scanned, because the scan only offers meshes
    /// the library does NOT have — which is precisely why this collision is hard to reach by hand
    /// and worth pinning.
    #[test]
    fn a_candidate_that_names_a_taken_id_is_refused_and_names_the_update_route() {
        let root = Fixture::new("collide")
            .descriptor("wall", "alpha")
            .pack("beta", &["spare"])
            .build("m");
        let mut app = harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        app.update();

        let tap = |app: &mut App, key: KeyCode, logical: bevy::input::keyboard::Key| {
            for state in [bevy::input::ButtonState::Pressed, bevy::input::ButtonState::Released] {
                app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
                    key_code: key,
                    logical_key: logical.clone(),
                    state,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
                app.update();
            }
        };
        tap(&mut app, KeyCode::Tab, bevy::input::keyboard::Key::Tab);
        for _ in 0..3 {
            app.update();
        }

        // Point the selected candidate's proposal at an id the library already owns.
        {
            let mut state = app
                .world_mut()
                .resource_mut::<emerge_mapper::tiles::ImportState>();
            state.selected_library_id = None;
            assert!(!state.candidates.is_empty(), "the fixture wrote an unimported mesh");
            let at = state.selected;
            state.candidates[at].proposed.id = "wall".to_owned();
        }
        for _ in 0..3 {
            app.update();
        }

        let before = app.world().resource::<emerge_mapper::project::Project>().measured.descriptors.len();
        tap(&mut app, KeyCode::Enter, bevy::input::keyboard::Key::Enter);
        for _ in 0..3 {
            app.update();
        }

        let project = app.world().resource::<emerge_mapper::project::Project>();
        assert_eq!(
            project.measured.descriptors.len(),
            before,
            "a colliding candidate must not be written — not as a second row, and not over the first"
        );
        let state = app.world().resource::<emerge_mapper::tiles::ImportState>();
        let said = state.status.note_text();
        assert!(
            said.contains("already in the library"),
            "the refusal must say why: `{said}`"
        );
        assert!(
            said.contains("select it above") || said.contains("edit that tile"),
            "and it must name the UPDATE route, not only offer a rename: `{said}`"
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

    /// **The composition grammar, driven from the editor** — FVS-R-7's remaining half.
    ///
    /// The modified `G` builds a grammar whose prototypes are whole compositions and lays the result
    /// as **stamps**, not placements. Asserting the medium is the point: the solver's rows used to
    /// come back as `Placed` carrying a composition id, which the library cannot resolve, so every row
    /// was dropped while the status line said it had worked.
    ///
    /// Driven through the key rather than by calling the writer, because the registration and the
    /// binding are exactly what this file exists to answer — and pressed from a system before
    /// `Phase::Act`, since Bevy clears `ButtonInput` in `PreUpdate`.
    #[test]
    fn the_modified_generate_lays_compositions_as_stamps_and_undo_takes_them_back() {
        let root = Fixture::new("gen-composed")
            .descriptor("floor", "alpha")
            .descriptor("rug", "alpha")
            .bounded_composition("tile_floor", (1.0, 1.0, 1.0), &[("floor", "floor", (0.0, 0.0))])
            .bounded_composition("tile_rug", (1.0, 1.0, 1.0), &[("rug", "rug", (0.0, 0.0))])
            // A hand-placed row, so "leaves the placements alone" is an observation rather than a
            // vacuous truth. Without it the routing is unexercised and deleting every placement here
            // would pass.
            .place("rug", (0.5, 0.5))
            .build("m");
        let mut app = emerge_mapper::harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..3 {
            app.update();
        }
        let placements_before = app.world().resource::<Project>().map.placements.len();
        assert!(placements_before > 0, "the fixture must hand-place something for this to mean anything");
        assert!(app.world().resource::<Project>().map.stamps.is_empty(), "nothing stamped yet");

        fn press_composed(
            mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
        ) {
            let b = emerge_mapper::keys::binding(emerge_mapper::keys::Action::GenerateComposed);
            keys.press(emerge_mapper::keys::MOD_KEYS[0]);
            keys.press(b.key);
        }
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                press_composed,
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();

        let project = app.world().resource::<Project>();
        let stamped = project.map.stamps.len();
        assert!(stamped > 0, "the modified G laid nothing — the composition source is unwired");
        assert!(
            project.map.stamps.iter().all(|s| s.of.starts_with("tile_")),
            "every stamp names one of the fixture's compositions: {:?}",
            project.map.stamps.iter().map(|s| s.of.clone()).collect::<Vec<_>>()
        );
        assert_eq!(
            project.map.placements.len(),
            placements_before,
            "a grammar over compositions writes references, never expanded rows"
        );

        // Closed under inversion, the same standard every other bulk edit here is held to.
        emerge_mapper::editor::undo_for_test(app.world_mut());
        app.update();
        assert!(
            app.world().resource::<Project>().map.stamps.is_empty(),
            "undo left the generated stamps in the map"
        );
        emerge_mapper::editor::redo_for_test(app.world_mut());
        app.update();
        assert_eq!(
            app.world().resource::<Project>().map.stamps.len(),
            stamped,
            "redo did not put the generated stamps back"
        );
    }

    /// **A wish the kit cannot grant is reported, not refused — the editor keeps drawing.**
    ///
    /// The composed generate asks for a quarter of the region to close into rooms. This fixture has
    /// two floor tiles and no wall anywhere, so that is unreachable by construction. Before the
    /// constraint solver, an unmeetable region produced `status.problem` and an empty map; now the
    /// arrangement lands and the shortfall is a note beside it.
    ///
    /// The distinction is the whole editor-facing point of the change: a red banner means *nothing
    /// happened*, and something did.
    #[test]
    fn a_region_that_cannot_make_rooms_still_generates_and_says_so() {
        let root = Fixture::new("gen-no-walls")
            .descriptor("floor", "alpha")
            .descriptor("rug", "alpha")
            .bounded_composition("tile_floor", (1.0, 1.0, 1.0), &[("floor", "floor", (0.0, 0.0))])
            .bounded_composition("tile_rug", (1.0, 1.0, 1.0), &[("rug", "rug", (0.0, 0.0))])
            .place("rug", (0.5, 0.5))
            .build("m");
        let mut app = emerge_mapper::harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..3 {
            app.update();
        }
        // Driven through the real binding, like the test above — there is no test-only entry point
        // to the generate, and adding one would be a second way to run it.
        fn press_composed(
            mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
        ) {
            let b = emerge_mapper::keys::binding(emerge_mapper::keys::Action::GenerateComposed);
            keys.press(emerge_mapper::keys::MOD_KEYS[0]);
            keys.press(b.key);
        }
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                press_composed,
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();

        let state = app.world().resource::<emerge_mapper::editor::EditorState>();
        assert!(
            !state.status.has_problem(),
            "an unmeetable wish must not read as a failure: {}",
            state.status.problem_text()
        );
        let said = state.status.note_text();
        assert!(said.contains("could not close"), "the shortfall must be said out loud: {said}");
        assert!(
            !app.world().resource::<Project>().map.stamps.is_empty(),
            "and the arrangement must still be on the map"
        );
    }

    /// **A stamp is one thing, and Delete takes the instance — never a member of it.**
    ///
    /// FVS-R-14. Stamped rows carry no `Placement` on purpose, which kept every tool off them; what
    /// was missing was an identity for the instance. This asserts the three halves that identity is
    /// made of: the parent entity owns the rows' lifetime (`Children` is `linked_spawn` in Bevy
    /// 0.19), `pick_subject` resolves a row to its stamp rather than to itself, and Delete removes
    /// the whole instance and inverts cleanly in both directions.
    ///
    /// The composition has **two** members deliberately: with one, "removed the instance" and
    /// "removed the row" are the same observation and the test would pass either way.
    #[test]
    fn a_stamp_is_one_thing_and_delete_takes_the_instance() {
        use emerge_mapper::editor::{Subject, pick_subject};

        let root = Fixture::new("instance")
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
        for _ in 0..3 {
            app.update();
        }

        let stamp_id = {
            let project = app.world().resource::<Project>();
            assert_eq!(project.map.stamps.len(), 1, "no stamp landed");
            project.map.stamps[0].id.clone()
        };

        // **One parent, and it owns the rows.** Counted rather than assumed: a parent per ROW would
        // also satisfy "a parent exists", and it is the thing that would silently make Delete take
        // one member.
        let instances: Vec<(bevy::prelude::Entity, usize)> = {
            let mut q = app
                .world_mut()
                .query::<(
                    bevy::prelude::Entity,
                    &emerge_mapper::editor::StampInstance,
                    &bevy::prelude::Children,
                )>();
            q.iter(app.world())
                .map(|(e, inst, kids)| {
                    assert_eq!(inst.id, stamp_id, "an instance naming a stamp the map does not have");
                    (e, kids.len())
                })
                .collect()
        };
        assert_eq!(instances.len(), 1, "one stamp must draw as one instance");
        assert_eq!(
            instances[0].1, 2,
            "the instance must own both expanded rows, or Delete cannot be about the whole of it"
        );

        // **A row resolves to its stamp.** Probed at the chair, one metre north of the anchor — a
        // member, not the stamp's own centre, because reaching through the instance is exactly the
        // failure this rule exists to prevent.
        {
            let project = app.world().resource::<Project>();
            let picture = app.world().resource::<emerge_mapper::editor::StampPicture>();
            assert_eq!(picture.rows.len(), 2, "the picture index must describe every drawn row");
            assert_eq!(
                pick_subject(project, picture, (2.0, 1.0)),
                Some(Subject::Stamp(stamp_id.clone())),
                "a click on a member is a click on the instance"
            );
        }

        // Delete, through the call the click makes.
        {
            let world = app.world_mut();
            world.resource_scope(|world, mut project: bevy::prelude::Mut<Project>| {
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                emerge_mapper::editor::delete_stamp_for_test(&stamp_id, &mut project, &mut state);
            });
        }
        for _ in 0..3 {
            app.update();
        }
        assert!(
            app.world().resource::<Project>().map.stamps.is_empty(),
            "Delete on an instance must take the stamp off the map"
        );
        let left = {
            let mut q = app
                .world_mut()
                .query::<&emerge_mapper::editor::StampInstance>();
            q.iter(app.world()).count()
        };
        assert_eq!(left, 0, "the instance must be gone with its stamp");
        assert!(
            app.world()
                .resource::<emerge_mapper::editor::StampPicture>()
                .rows
                .is_empty(),
            "the picture index must describe the entities that exist, and there are none"
        );

        // **Closed under inversion, both ways.** `UnstampedMany` used to invert to a tail drain,
        // which is right only when the stamps removed were the tail — so a redo after a mid-list
        // removal took a different stamp off. One stamp cannot catch that; what this pins is that
        // the pair round-trips at all.
        emerge_mapper::editor::undo_for_test(app.world_mut());
        app.update();
        {
            let project = app.world().resource::<Project>();
            assert_eq!(project.map.stamps.len(), 1, "undo did not put the stamp back");
            assert_eq!(project.map.stamps[0].id, stamp_id);
        }
        emerge_mapper::editor::redo_for_test(app.world_mut());
        app.update();
        assert!(
            app.world().resource::<Project>().map.stamps.is_empty(),
            "redo must take the same stamp off again"
        );
    }

    /// **A stamp moves as one thing, and the move inverts both ways.**
    ///
    /// FVS-R-14's move arm. The rows are derived, so moving is one field — `Stamped::at` — and
    /// `redraw_stamps` puts the pieces where the new value says. Asserting on the ROWS rather than
    /// on the field is what makes that a claim rather than a restatement: a move that wrote `at` and
    /// left the picture behind would pass a field check.
    #[test]
    fn moving_a_stamp_takes_every_row_with_it_and_undo_brings_them_back() {
        let root = Fixture::new("movestamp")
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
        for _ in 0..3 {
            app.update();
        }

        let id = app.world().resource::<Project>().map.stamps[0].id.clone();
        // Where the picture says the rows are, before.
        let rows_at = |app: &bevy::prelude::App| -> Vec<(f32, f32)> {
            let mut v: Vec<(f32, f32)> = app
                .world()
                .resource::<emerge_mapper::editor::StampPicture>()
                .rows
                .iter()
                .map(|r| r.at)
                .collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            v
        };
        let before = rows_at(&app);
        assert_eq!(before.len(), 2, "both rows must be drawn to begin with");

        {
            let world = app.world_mut();
            world.resource_scope(|world, mut project: bevy::prelude::Mut<Project>| {
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                emerge_mapper::editor::move_stamp_for_test(&id, (7.0, 5.0), &mut project, &mut state);
            });
        }
        for _ in 0..3 {
            app.update();
        }

        assert_eq!(
            app.world().resource::<Project>().map.stamps[0].at,
            (7.0, 5.0),
            "the move writes `Stamped::at`"
        );
        let after = rows_at(&app);
        assert_eq!(after.len(), 2, "the instance must still own both rows after moving");
        let shift = (5.0_f32, 3.0_f32);
        for (a, b) in before.iter().zip(after.iter()) {
            assert!(
                (b.0 - a.0 - shift.0).abs() < 1e-3 && (b.1 - a.1 - shift.1).abs() < 1e-3,
                "every row moves by the same offset — {a:?} -> {b:?}, wanted +{shift:?}"
            );
        }

        // Closed under inversion in both directions: a move is a move either way.
        emerge_mapper::editor::undo_for_test(app.world_mut());
        app.update();
        assert_eq!(app.world().resource::<Project>().map.stamps[0].at, (2.0, 2.0));
        assert_eq!(rows_at(&app), before, "undo puts every row back");
        emerge_mapper::editor::redo_for_test(app.world_mut());
        app.update();
        assert_eq!(app.world().resource::<Project>().map.stamps[0].at, (7.0, 5.0));
    }

    /// **A captured stamp nests by reference, and the nesting round-trips.**
    ///
    /// FVS-R-14's last two clauses: `CloneSet` carries either a piece or a stamp, and capturing a
    /// box that holds one emits `Body::Composition { id }` rather than the rows it expands to.
    ///
    /// The distinction is the whole item. A group that copied the expanded rows would be a
    /// snapshot: editing the inner composition afterwards would stop reaching it, which is exactly
    /// the flattening `stamping_writes_a_reference_and_undo_takes_it_back` pins against for a map.
    /// So this asserts the **body kind**, and then that stamping the outer group puts the inner
    /// one's pieces on the map through the reference.
    #[test]
    fn a_captured_stamp_nests_by_reference_and_round_trips() {
        use emerge_core::composition::Body;

        let root = Fixture::new("nest")
            .descriptor("table", "alpha")
            .descriptor("chair", "alpha")
            // **Bounded, because only a bounded group can size the one that nests it.** An
            // anchored composition claims no tile, so there is no honest height to fold in — the
            // capture refuses it by name, which is its own small proof that the guard works.
            .bounded_composition(
                "break_table",
                (2.0, 1.2, 2.0),
                &[("table", "table", (0.0, 0.0)), ("chair_north", "chair", (0.0, -1.0))],
            )
            .build("m");
        let mut app = emerge_mapper::harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..3 {
            app.update();
        }

        // A set holding one stamp and nothing else — the case that would divide by zero if the
        // anchor were averaged over placements alone.
        let set = emerge_mapper::editor::CloneSet {
            pieces: Vec::new(),
            stamps: vec![emerge_mapper::editor::CloneStamp {
                of: "break_table".to_owned(),
                offset: (0.0, 0.0),
                yaw: 0.0,
                overrides: Vec::new(),
                owned: false,
                owned_because: None,
                note: None,
            }],
            centre_off: (0.0, 0.0),
            half: (1.0, 1.0),
            yaw: 0.0,
        };

        let comp = {
            let project = app.world().resource::<Project>();
            emerge_mapper::editor::composition_from_set(
                &set,
                "mess_corner",
                &project.library,
                &project.compositions,
            )
            .unwrap_or_else(|e| panic!("capture refused: {e}"))
        };
        assert_eq!(comp.members.len(), 1, "one stamp in, one member out");
        assert!(
            matches!(&comp.members[0].body, Body::Composition { id } if id == "break_table"),
            "a captured stamp must nest by REFERENCE — copying its rows would flatten it, and \
             editing `break_table` afterwards would stop reaching this group. Got {:?}",
            comp.members[0].body
        );

        // And the reference resolves: stamping the outer group puts the inner one's pieces down.
        {
            let world = app.world_mut();
            world.resource_scope(|world, mut project: bevy::prelude::Mut<Project>| {
                project.compositions.compositions.push(comp.clone());
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                let mut compose = ComposeState {
                    armed: Some("mess_corner".to_owned()),
                    ..Default::default()
                };
                emerge_mapper::editor::stamp_here_for_test(
                    &mut project,
                    &mut state,
                    &mut compose,
                    (4.0, 4.0),
                );
            });
        }
        for _ in 0..3 {
            app.update();
        }

        let project = app.world().resource::<Project>();
        assert_eq!(project.map.stamps.len(), 1, "the outer group stamped");
        assert_eq!(project.map.stamps[0].of, "mess_corner");
        assert!(
            project.map.placements.is_empty(),
            "nesting must not write expanded rows into the map — the map holds the reference"
        );
        // Two rows drawn THROUGH the nested reference is what proves it resolved rather than
        // merely parsed.
        let picture = app.world().resource::<emerge_mapper::editor::StampPicture>();
        assert_eq!(
            picture.rows.len(),
            2,
            "the nested composition's two members must reach the map through the reference"
        );
    }

    /// **The focal group stands up with its neighbours either side** — the carousel, asked of a
    /// running app rather than of the layout function.
    ///
    /// The layout is unit-tested; what no unit test can see is the schedule. `restage_group` gained a
    /// resource, a change-guard and a parent-per-group hierarchy, and in Bevy 0.19 a system whose
    /// `Res<T>` was never `init_resource`d panics rather than skipping.
    #[test]
    fn the_carousel_stands_the_focal_group_up_with_its_neighbours() {
        let root = Fixture::new("sheet")
            .descriptor("floor", "alpha")
            .descriptor("wall", "alpha")
            .bounded_composition("tile_a", (1.0, 2.4, 1.0), &[("floor", "floor", (0.0, 0.0))])
            .bounded_composition(
                "tile_b",
                (1.0, 2.4, 1.0),
                &[("floor", "floor", (0.0, 0.0)), ("wall", "wall", (0.0, -0.4))],
            )
            .bounded_composition("tile_c", (1.0, 2.4, 1.0), &[("floor", "floor", (0.0, 0.0))])
            .build("m");
        let mut app = emerge_mapper::harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        *app.world_mut().resource_mut::<Mode>() = Mode::Compose;
        for _ in 0..5 {
            app.update();
        }

        // Three groups, all within the wings of the first, so all three stand.
        let strip = app.world().resource::<emerge_mapper::compose::StagedCarousel>();
        assert_eq!(strip.0.slots.len(), 3, "the strip did not stand every neighbour up");
        assert_eq!(strip.0.focal().map(|s| s.index), Some(0), "the focal group is the selected one");
        assert!(strip.0.tallest > 0.0, "a strip of no height frames nothing");

        // Four rows across three groups — so this counts the whole strip standing, not one group.
        let staged = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<emerge_mapper::compose::StagedMember>>()
            .iter(app.world())
            .count();
        assert_eq!(staged, 4, "every member of every visible group has to stand up");

        // **Nothing respawns while nothing changes.** `restage_group` writes `status.problem` on a
        // bad group, which re-marks its own resource changed — an unbounded despawn/respawn loop
        // before the staging key closed it.
        let ids: Vec<_> = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<emerge_mapper::compose::StagedMember>>()
            .iter(app.world())
            .collect();
        for _ in 0..5 {
            app.update();
        }
        let after: Vec<_> = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<emerge_mapper::compose::StagedMember>>()
            .iter(app.world())
            .collect();
        assert_eq!(ids, after, "the sheet was rebuilt with nothing having changed");

        // **The strip is not rewritten when nothing changed.** `ResMut` marks a resource changed on
        // any deref_mut, and `tiles::stage_camera` re-frames on that edge — so an unconditional write
        // threw the author's pan and zoom away on every edit that re-ran the staging system.
        use bevy::ecs::change_detection::DetectChanges;
        let settled = app
            .world()
            .get_resource_ref::<emerge_mapper::compose::StagedCarousel>()
            .map(|r| r.last_changed());
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(
            app.world()
                .get_resource_ref::<emerge_mapper::compose::StagedCarousel>()
                .map(|r| r.last_changed()),
            settled,
            "the strip was rewritten with nothing having changed, which re-frames the camera and \
             discards the author's pan and zoom"
        );

        // **The step key, driven.** This test used to assign `selected` directly, so `step_carousel`
        // could have been unregistered or reading the wrong action and nothing would have noticed —
        // which is exactly the registration question this file exists to answer.
        //
        // Pressed from a system rather than before `update()`: Bevy clears `ButtonInput` in
        // `PreUpdate`, so a press written outside the frame is gone before `Phase::Act` runs. It
        // fires once, because pressing an already-pressed key does not re-arm `just_pressed`.
        fn press_step(mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>) {
            keys.press(emerge_mapper::keys::binding(emerge_mapper::keys::Action::CarouselNext).key);
        }
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(press_step, emerge_mapper::keys::Phase::Act),
        );
        app.update();
        assert_eq!(
            app.world().resource::<ComposeState>().selected,
            1,
            "the carousel key has to move the focus, or the verb is unwired"
        );

        // Stepping the carousel re-lays it: a different group becomes focal, and the wings change.
        app.world_mut().resource_mut::<ComposeState>().selected = 1;
        for _ in 0..3 {
            app.update();
        }
        let strip = app.world().resource::<emerge_mapper::compose::StagedCarousel>();
        assert_eq!(strip.0.focal().map(|s| s.index), Some(1), "stepping did not move the focus");
        assert!(
            strip.0.slots.iter().any(|s| s.offset == -1),
            "the group before the focal one has to appear once there is one"
        );
    }

    /// **Clicking a miniature brings it to the middle** — the one carousel verb no test had ever
    /// exercised.
    ///
    /// # What this can and cannot reach
    ///
    /// It drives `pick_along`, which is everything from the ray onward: the hit test against the laid
    /// out strip, the write to `selected`, and the member-cursor reset. It does **not** drive
    /// `cursor_ray`, and that is a limit of the harness rather than a gap left open —
    /// `MinimalPlugins` has no window, so the camera has no render target and both
    /// `world_to_viewport` and `viewport_to_world` answer `Err`. A version of this test that went
    /// through the projection was written first and asserted nothing at all; it only became visible
    /// because it was made to fail loudly when the aim never happened.
    ///
    /// So `pick_along` is `pub` for the same reason `toggle_arm` is: the part that is ours is
    /// separable from the part that is the engine's, and only one of them can be checked here.
    #[test]
    fn clicking_a_miniature_brings_it_to_the_middle() {
        let root = Fixture::new("pickable")
            .descriptor("floor", "alpha")
            .bounded_composition("tile_a", (1.0, 2.4, 1.0), &[("floor", "floor", (0.0, 0.0))])
            .bounded_composition("tile_b", (1.0, 2.4, 1.0), &[("floor", "floor", (0.0, 0.0))])
            .build("m");
        let mut app = emerge_mapper::harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        *app.world_mut().resource_mut::<Mode>() = Mode::Compose;
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(app.world().resource::<ComposeState>().selected, 0);

        // Where the neighbour actually stands, taken from the strip rather than assumed.
        let strip = app.world().resource::<emerge_mapper::compose::StagedCarousel>().0.clone();
        let neighbour = *strip
            .slots
            .iter()
            .find(|s| s.offset == 1)
            .unwrap_or_else(|| panic!("the neighbour has to be on the strip to be clicked"));

        // Straight down at its centre, in the stage's own space — the ray a click there produces.
        let origin = emerge_mapper::compose::COMPOSE_STAGE
            + bevy::prelude::Vec3::new(neighbour.at.0, 10.0, neighbour.at.1);
        let dir = bevy::prelude::Vec3::NEG_Y;

        let mut state = std::mem::take(&mut *app.world_mut().resource_mut::<ComposeState>());
        let moved = emerge_mapper::compose::pick_along(&strip, origin, dir, &mut state);
        assert!(moved, "the ray has to hit the miniature it was aimed at");
        assert_eq!(
            state.selected, neighbour.index,
            "clicking a miniature has to bring it to the middle"
        );
        assert_eq!(
            state.member, 0,
            "and the member cursor resets, because a different group has different members"
        );

        // **Clicking the one already selected is not a change**, so it must not reset anything —
        // aimed at the same slot, because `strip` is still last frame's layout and the slot at the
        // stage origin is the group that *was* focal.
        state.member = 3;
        assert!(
            !emerge_mapper::compose::pick_along(&strip, origin, dir, &mut state),
            "re-picking what is already selected is not a change"
        );
        assert_eq!(state.member, 3, "so the member cursor must survive it");
    }
}

/// **The open name box is UI, and every "is the pointer over UI" test has to agree.**
///
/// That question is asked as "is any `Hovered` true" — `view::drive` for the scroll wheel,
/// `place_on_click` for the world click, `compose::pick_slot` for the strip. The dialog carried no
/// `Hovered` at all, so scrolling over a visible, open prompt zoomed the world behind it.
///
/// The full-screen backdrop deliberately stays click-through, which is why this asserts on the inner
/// panel specifically rather than on "some entity under the pointer".
#[test]
fn the_open_name_box_answers_the_over_ui_question() {
    use bevy::picking::hover::Hovered;

    let root = Fixture::new("namebox").descriptor("floor", "alpha").build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let hoverable = app
        .world_mut()
        .query_filtered::<bevy::prelude::Entity, (
            bevy::prelude::With<Hovered>,
            bevy::prelude::With<emerge_mapper::chrome::NameBox>,
        )>()
        .iter(app.world())
        .count();
    assert_eq!(
        hoverable, 0,
        "the full-screen backdrop must stay click-through — it is a prompt, not a modal"
    );

    // The inner panel is a child of the box, and it is the part that has to answer.
    let panel_has_hovered = app
        .world_mut()
        .query_filtered::<&bevy::prelude::Children, bevy::prelude::With<emerge_mapper::chrome::NameBox>>()
        .iter(app.world())
        .flat_map(|kids| kids.iter().collect::<Vec<_>>())
        .any(|kid| app.world().get::<Hovered>(kid).is_some());
    assert!(
        panel_has_hovered,
        "the visible dialog carries no `Hovered`, so every over-UI test reads it as open world and \
         a scroll over it zooms the map behind it"
    );
}

/// **`Cmd`+remove falls to the PLACE selection when nothing is under the cursor.**
///
/// Reported live: *"I want to send back an item that is selected in the Place scroll area."* The
/// verb resolved its subject with `nearest_placement`, so it only ever reached a piece standing on
/// the map; over the list it refused.
///
/// **The first fix for that was wrong and this test is the shape of why.** It keyed on whether the
/// pointer was over the interface, asked as `Hovered` — which `bevy_picking` writes from the
/// *window's* cursor, so it is false for an injected pointer and false for an author who moved the
/// mouse off the row they had just selected. A test could not reach the branch at all, so it passed
/// while the feature did not work, and it was reported twice. The rule is now ordered rather than
/// conditional, which is a rule a test can drive whole.
///
/// The armed row is deliberately **not** the descriptor the map places, so no assertion here can be
/// satisfied by the other arm answering first.
#[test]
fn cmd_remove_falls_to_the_place_selection() {
    use emerge_mapper::editor::edit_subject;

    let root = Fixture::new("sendback")
        .descriptor("floor", "alpha")
        .descriptor("wall", "alpha")
        .place("floor", (0.0, 0.0))
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let wall = {
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("no project"));
        assert_eq!(
            project.map.placements.first().map(|p| p.descriptor.as_str()),
            Some("floor"),
            "the map must place the OTHER piece, or this proves nothing"
        );
        project
            .library
            .descriptors
            .iter()
            .position(|d| d.id == "wall")
            .unwrap_or_else(|| panic!("the fixture wrote `wall`"))
    };

    // Both resources are read live rather than copied out, so what the rule is asked about is what
    // the editor actually holds — and neither one outlives a call, which is what lets the arming
    // between them borrow the world back.
    let arm = |app: &mut App, brush: Option<usize>| {
        app.world_mut()
            .resource_mut::<emerge_mapper::editor::EditorState>()
            .brush = brush;
    };
    let ask = |app: &App, under: Option<usize>| {
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("no project"));
        let state = app
            .world()
            .get_resource::<emerge_mapper::editor::EditorState>()
            .unwrap_or_else(|| panic!("no editor state"));
        edit_subject(project, state, under)
    };

    arm(&mut app, Some(wall));
    // **A piece under the cursor is the answer**, and the armed row is not consulted for it.
    assert_eq!(
        ask(&app, Some(0)),
        Ok("floor".to_owned()),
        "pointing at a piece has to open that piece"
    );
    // **Failing that, the PLACE selection** — the whole of the reported gap. `wall` is deliberately
    // not the descriptor the map places, so this cannot be satisfied by the first arm answering.
    assert_eq!(
        ask(&app, None),
        Ok("wall".to_owned()),
        "with nothing under the cursor the armed PLACE row is the subject"
    );
    // **And a refusal that names both places it looked**, rather than one of them.
    arm(&mut app, None);
    let refused = ask(&app, None).unwrap_err();
    assert!(
        refused.contains("cursor") && refused.contains("PLACE"),
        "a refusal has to say where it looked, and said: {refused}"
    );
}

/// **`Shift+B` puts the armed piece down.**
///
/// Reported live: arming the box left the palette showing a highlighted row and the brush ghost
/// previewing a placement while the author dragged a capture box — two subjects under one cursor.
/// Driven through the real key message, because what is being asserted is that the *binding* reaches
/// it: setting `state.tool` by hand would pass with the handler deleted.
#[test]
fn arming_the_box_clears_the_armed_piece() {
    let root = Fixture::new("armclear")
        .descriptor("floor", "alpha")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    {
        let mut state = app
            .world_mut()
            .resource_mut::<emerge_mapper::editor::EditorState>();
        state.brush = Some(0);
    }
    app.update();

    // Shift+B, both halves in one frame so the modifier is held as the key goes down.
    for (key, logical) in [
        (KeyCode::ShiftLeft, bevy::input::keyboard::Key::Shift),
        (KeyCode::KeyB, bevy::input::keyboard::Key::Character("b".into())),
    ] {
        app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
            key_code: key,
            logical_key: logical,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    }
    for _ in 0..3 {
        app.update();
    }

    let state = app
        .world()
        .get_resource::<emerge_mapper::editor::EditorState>()
        .unwrap_or_else(|| panic!("no editor state"));
    assert!(
        matches!(state.tool, emerge_mapper::editor::Tool::Clone),
        "Shift+B has to arm the clone tool, or this test is asserting nothing about it"
    );
    assert!(
        state.brush.is_none(),
        "arming the box must put the brush down — a highlighted palette row and a capture box are \
         two subjects under one cursor"
    );
}

/// **The UNDER readout stops at the panel**, like the verbs it reports on.
///
/// `sense_under_cursor` had no over-the-interface gate at all, so with the cursor resting on the
/// PLACE list the status block named whatever placement stood behind the panel and said
/// "Cmd+Delete edits it" — a promise about a key that acts on the PLACE selection there.
///
/// **Asked of `under_readout` rather than of the running system, and that is the finding.** The
/// first version of this test drove the whole app, put `view::Pointer` on a real node's centre, and
/// asserted the block was blank. It passed — and it passed just as well with the gate deleted,
/// because headless has no viewport, so `under_cursor_target` returns `None` and the line is blank
/// whatever the rule does. Mutation-testing caught it. The rule is a pure function now so the
/// assertion has something to bite on.
#[test]
fn the_under_readout_is_blank_while_the_pointer_is_on_a_panel() {
    use emerge_mapper::editor::under_readout;

    let piece = emerge_core::map::Placed {
        paint: 0,
        id: "crate@7".to_owned(),
        descriptor: "alpha/crate".to_owned(),
        at: (1.0, 1.0),
        yaw: 0.0,
        lift: 0.0,
        tip: (0, 0),
        on: None,
        owned: false,
        owned_because: None,
        patch: None,
        note: None,
    };

    // Over the world, pointing at something: the line names it and the key that acts on it.
    let said = under_readout(false, Some(&piece));
    assert!(said.contains("crate@7"), "the readout must name the piece: `{said}`");
    assert!(
        said.contains(&emerge_mapper::keys::chord_text(emerge_mapper::keys::binding(
            emerge_mapper::keys::Action::EditTile
        ))),
        "the chord comes from the census so this line cannot name a key the build does not read: \
         `{said}`"
    );
    // Over a panel, with the very same piece behind it: silent.
    assert_eq!(
        under_readout(true, Some(&piece)),
        "",
        "a panel is drawn over the map, so the block must not promise an edit to what is behind it"
    );
    // And bare floor stays blank, or the row is never empty and the eye stops reading it.
    assert_eq!(under_readout(false, None), "");
}

/// **"Is the pointer on a panel" answered against the real layout.**
///
/// Reported live, twice: with the cursor on a PLACE row, `Cmd`+remove picked up the tile *underneath
/// the list*. So `view::over_ui` was answering false over the panel and the ordered rule correctly
/// took its first arm.
///
/// This is the assertion that was missing both times. The first version of the check read `Hovered`,
/// which no headless test can set — `bevy_picking` writes it from the window's cursor — so there was
/// nothing to write. Reading the rects makes it ordinary: boot the editor, take a real palette row's
/// `ComputedNode` and `UiGlobalTransform`, and ask about its own centre.
///
/// The **scale factor is the thing that was wrong** and so it is the thing pinned: the node rect is
/// physical, `view::Pointer` is logical, and a test that passed only at scale 1.0 would say nothing
/// about the Retina window the report came from.
#[test]
fn the_pointer_is_over_the_panel_when_it_is_over_a_row() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};

    let root = Fixture::new("overui").descriptor("floor", "alpha").build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..6 {
        app.update();
    }

    // Any laid-out interactive node will do — what is asserted is the arithmetic, not which row.
    let (size, centre) = {
        let mut q = app
            .world_mut()
            .query_filtered::<(&ComputedNode, &UiGlobalTransform), bevy::prelude::With<bevy::picking::hover::Hovered>>();
        q.iter(app.world())
            .map(|(n, tf)| (n.size(), tf.translation))
            .find(|(size, _)| size.x > 1.0 && size.y > 1.0)
            .unwrap_or_else(|| panic!("no laid-out interactive UI node — this test would prove nothing"))
    };

    for scale in [1.0_f32, 2.0] {
        // The pointer is logical, the rect is physical: the centre in logical pixels is the physical
        // centre divided by the factor. Getting this backwards is the bug being pinned.
        let logical_centre = centre / scale;
        let nodes: Vec<(ComputedNode, UiGlobalTransform)> = {
            let mut q = app
                .world_mut()
                .query_filtered::<(&ComputedNode, &UiGlobalTransform), bevy::prelude::With<bevy::picking::hover::Hovered>>();
            q.iter(app.world()).map(|(n, tf)| (n.clone(), *tf)).collect()
        };
        let borrowed: Vec<(&ComputedNode, &UiGlobalTransform)> =
            nodes.iter().map(|(n, tf)| (n, tf)).collect();

        assert!(
            emerge_mapper::view::over_ui(Some(logical_centre), scale, borrowed.iter().copied()),
            "a pointer on a row's own centre must read as over the interface (scale {scale}, \
             node {size:?} at {centre:?})"
        );
        // Far outside every panel: the map, where the piece under the cursor IS the answer.
        assert!(
            !emerge_mapper::view::over_ui(Some(Vec2::new(-5000.0, -5000.0)), scale, borrowed.iter().copied()),
            "a pointer nowhere near a panel must read as the world (scale {scale})"
        );
    }
    // No cursor is not "over the world" — it is no answer, and every other reader treats it so.
    assert!(!emerge_mapper::view::over_ui(None, 1.0, [].into_iter()));
}

/// **`Z` and `C` reach the set in hand, not the brush.**
///
/// The turn arithmetic is pinned by `a_turned_set_lands_where_a_turned_stamp_would`; what was not
/// pinned is that the *binding* gets there. `CloneDrag::held` is private, so this goes through
/// `hold_set_for_test` and then drives the real key message — the brush's own yaw is asserted
/// unchanged, because "turned something" and "turned the right thing" are different claims.
#[test]
fn the_aim_keys_turn_the_set_in_hand_and_leave_the_brush_alone() {
    let root = Fixture::new("turnset").descriptor("floor", "alpha").build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let before_brush = {
        let mut state = app
            .world_mut()
            .resource_mut::<emerge_mapper::editor::EditorState>();
        state.tool = emerge_mapper::editor::Tool::Clone;
        state.brush_yaw
    };
    {
        let set = emerge_mapper::editor::CloneSet {
            pieces: vec![emerge_mapper::editor::ClonePiece {
                descriptor: "floor".to_owned(),
                offset: (0.0, 0.0),
                yaw: 0.0,
                tip: (0, 0),
                lift: 0.0,
                note: None,
                owned: false,
                owned_because: None,
                on: emerge_mapper::editor::CloneHost::Layer,
            }],
            stamps: Vec::new(),
            centre_off: (0.0, 0.0),
            half: (0.5, 0.5),
            yaw: 0.0,
        };
        let mut drag = app
            .world_mut()
            .resource_mut::<emerge_mapper::editor::CloneDrag>();
        emerge_mapper::editor::hold_set_for_test(set, &mut drag);
    }
    app.update();

    // `C` — the real message, so this cannot pass with the binding removed.
    app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
        key_code: KeyCode::KeyC,
        logical_key: bevy::input::keyboard::Key::Character("c".into()),
        state: bevy::input::ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    for _ in 0..3 {
        app.update();
    }

    let drag = app
        .world()
        .get_resource::<emerge_mapper::editor::CloneDrag>()
        .unwrap_or_else(|| panic!("no clone drag"));
    let turned = drag
        .held_for_test()
        .unwrap_or_else(|| panic!("the set left the hand"));
    assert!(
        turned > 0.0,
        "the aim key must turn the set in hand; it is still at {turned} deg"
    );
    let after_brush = app
        .world()
        .resource::<emerge_mapper::editor::EditorState>()
        .brush_yaw;
    assert_eq!(
        after_brush, before_brush,
        "with a set in hand the brush is not the subject — turning it would aim something that is \
         not going anywhere"
    );
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

/// **A captured group reaches disk, and comes back.**
///
/// The conversion is unit-tested against hand-built data; this covers the half that cannot be — the
/// commit door. It validates the whole set, writes atomically, and only then adopts, so a refusal
/// leaves both the file and the in-memory project as they were.
///
/// Driven by calling the door rather than through the editor, because `bevy_debugger/input` carries
/// no cursor position and the gesture that fills a `CloneSet` is a box DRAG. The keyboard half —
/// `M` opens the field, Enter commits — is `group_name_keys`, and it calls exactly this.
#[test]
fn a_captured_group_is_written_and_reads_back() {
    let root = Fixture::new("capture")
        .descriptor("table", "alpha")
        .descriptor("lamp", "alpha")
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let world = app.world_mut();
    let mut project = world.resource_mut::<emerge_mapper::project::Project>();
    assert!(project.compositions.compositions.is_empty(), "the fixture writes no groups");

    let set = emerge_mapper::editor::CloneSet {
        pieces: vec![
            emerge_mapper::editor::ClonePiece {
                descriptor: "table".to_owned(),
                offset: (0.0, 0.0),
                yaw: 0.0,
                tip: (0, 0),
                lift: 0.0,
                note: None,
                owned: false,
                owned_because: None,
                on: emerge_mapper::editor::CloneHost::Layer,
            },
            emerge_mapper::editor::ClonePiece {
                descriptor: "lamp".to_owned(),
                offset: (0.5, 0.0),
                yaw: 90.0,
                tip: (0, 0),
                lift: 0.0,
                note: None,
                owned: false,
                owned_because: None,
                on: emerge_mapper::editor::CloneHost::Layer,
            },
        ],
        centre_off: (0.25, 0.0),
        half: (0.75, 0.5),
        stamps: Vec::new(),
        yaw: 0.0,
    };
    let kept = emerge_mapper::editor::keep_as_group(&mut project, &set, "Mess Table", false)
        .unwrap_or_else(|e| panic!("the composition must be kept: {e}"));
    assert_eq!(
        kept,
        emerge_mapper::editor::Kept::Made("mess_table".to_owned()),
        "a name nothing holds is made outright, and forced into snake_case"
    );
    assert_eq!(project.compositions.compositions.len(), 1, "it was adopted in memory");

    // And it is on disk, parseable, with the members the set held.
    let path = root.join("assets/emerge/compositions.ron");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let reread = emerge_core::composition::Compositions::parse(&text)
        .unwrap_or_else(|e| panic!("what was written must parse: {e}"));
    let c = reread.compositions.first().unwrap_or_else(|| panic!("no group on disk"));
    assert_eq!(c.id, "mess_table");
    let ids: Vec<&str> = c.members.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["lamp", "table"], "members are stored sorted by id");

    // **Capturing over the name asks first, and writes nothing until it is answered.**
    //
    // It used to refuse outright. That made compositions append-only the moment the Compose tab
    // stopped being able to edit one — and made the send-back verb's own advice, "edit the group
    // first", impossible to follow.
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    let asked = emerge_mapper::editor::keep_as_group(&mut project, &set, "mess_table", false)
        .unwrap_or_else(|e| panic!("capturing over a name must ask, not refuse: {e}"));
    assert_eq!(
        asked,
        emerge_mapper::editor::Kept::WouldReplace { id: "mess_table".to_owned(), stamps: 0 },
        "the first press asks"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), before, "and writes nothing");

    // The second press redefines it in place — same id, so no stamp anywhere is stranded.
    let done = emerge_mapper::editor::keep_as_group(&mut project, &set, "mess_table", true)
        .unwrap_or_else(|e| panic!("the confirmed replace must land: {e}"));
    assert_eq!(
        done,
        emerge_mapper::editor::Kept::Replaced { id: "mess_table".to_owned(), stamps: 0 }
    );
    assert_eq!(
        project.compositions.compositions.len(),
        1,
        "replacing redefines the one that was there rather than adding a second"
    );
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let reread = emerge_core::composition::Compositions::parse(&text)
        .unwrap_or_else(|e| panic!("what was written must parse: {e}"));
    assert_eq!(reread.compositions.len(), 1, "and one composition reached disk, not two");
}

/// **The name field takes the keyboard, so typing a name cannot also drive the tab.**
///
/// Naming a captured composition on the Map used to dispatch a Map verb for every letter — typing
/// `corner` fired aim, turn, rename-map and turn-view — because `EditorState::grouping` was missing
/// from the guard that decides who owns the keyboard.
///
/// This used to be asserted through Compose's own name field, which was the same guard reached by a
/// second door. Authoring collapsed onto the Map, so `grouping` is now the **only** text field that
/// can be open while the Map holds the keyboard, and this is the one place the property lives.
///
/// Testable despite the field itself not being: `keys::Live` is a resource, and it is the one thing
/// standing between a keystroke and a verb. `bevy_debugger/input` writes `ButtonInput` and not the
/// `KeyboardInput` stream, so an agent cannot type here — but it can assert who owns the keyboard.
#[test]
fn naming_a_composition_takes_the_keyboard_from_the_verbs() {
    let root = fixtures::Fixture::new("naming")
        .descriptor("wall", "alpha")
        .bounded_composition("bay", (1.0, 2.4, 1.0), &[("north", "wall", (0.0, 0.0))])
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    *app.world_mut().resource_mut::<emerge_mapper::tiles::Mode>() =
        emerge_mapper::tiles::Mode::Map;
    app.update();
    assert_eq!(
        app.world().resource::<emerge_mapper::keys::Live>().0,
        emerge_mapper::keys::Context::Map,
        "with no field open the tab's verbs are live"
    );

    app.world_mut().resource_mut::<emerge_mapper::editor::EditorState>().grouping =
        Some(String::new());
    app.update();
    assert_eq!(
        app.world().resource::<emerge_mapper::keys::Live>().0,
        emerge_mapper::keys::Context::Typing,
        "while a name is being typed the keyboard belongs to the text, or every letter is a verb"
    );

    // And it hands the keyboard back, or the tab is dead after one capture.
    app.world_mut().resource_mut::<emerge_mapper::editor::EditorState>().grouping = None;
    app.update();
    assert_eq!(
        app.world().resource::<emerge_mapper::keys::Live>().0,
        emerge_mapper::keys::Context::Map
    );
}

/// **The tile resizes to hold what is dropped into it, through the real keys.**
///
/// `fit_envelope` and `refit` are unit-tested; what those cannot see is whether anything *calls*
/// them — `refit_tile` is its own system, ordered after `build_keys`, and a system that is never
/// registered is exactly the class of defect this file exists for. The author asked for this
/// directly: *"as many whole tiles as needed to capture the object."*
#[test]
fn dropping_an_oversized_mesh_grows_the_tile() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
    use emerge_mapper::keys::{binding, Action};

    let root = Fixture::new("tile_grows")
        // 1.21 m reaches 0.605 from a centred anchor and one cell only reaches 0.5, so this needs a
        // second cell — and 0.81 across does not, which is what makes the assertion below specific.
        .sized_descriptor("pallet", "alpha", 0.81, 1.21)
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, key: KeyCode) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>, mut done: bevy::prelude::Local<bool>| {
                    if !*done {
                        keys.release_all();
                        keys.press(key);
                        *done = true;
                    }
                },
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();
    }

    once(&mut app, binding(Action::TilesTab).key);
    let before = match app.world().resource::<emerge_mapper::build::Build>().open {
        Some(ref c) => match c.envelope {
            emerge_core::composition::Envelope::Bounded { size } => size,
            _ => panic!("a tile claims a tile"),
        },
        None => panic!("arriving opens a tile"),
    };
    assert_eq!(
        (before.0, before.2),
        (emerge_core::grid::TILE, emerge_core::grid::TILE),
        "an empty tile is one cell"
    );

    once(&mut app, binding(Action::BuildArm).key);
    once(&mut app, binding(Action::BuildDrop).key);
    app.update();

    let after = match app.world().resource::<emerge_mapper::build::Build>().open {
        Some(ref c) => match c.envelope {
            emerge_core::composition::Envelope::Bounded { size } => size,
            _ => panic!("a tile claims a tile"),
        },
        None => panic!("still open"),
    };
    // **The property, not the numbers.** A piece this size cannot fit one cell, so the tile grew —
    // that is what is being pinned. The exact count still depends on where the drop lands, and that
    // is changing: a brought-in mesh is to be *centred* rather than corner-aligned to the cursor's
    // cell (decided 2026-08-12), which makes this 1 x 2 instead of the 2 x 3 a corner-aligned drop
    // from the middle cell produces. Asserting the count today would only have to be rewritten with
    // it, and would say nothing extra in the meantime.
    assert!(
        after.0 > before.0 || after.2 > before.2,
        "a mesh too big for one cell must grow the tile — was {before:?}, now {after:?}"
    );
    let whole = |v: f32| (v / emerge_core::grid::TILE - (v / emerge_core::grid::TILE).round()).abs();
    assert!(
        whole(after.0) < 1e-4 && whole(after.2) < 1e-4,
        "and it grows in whole tiles, never a fraction of one: {after:?}"
    );

    // **And it says what that costs**, because `from_compositions` skips anything that is not one
    // cell — a group this size is stamped by hand rather than generated, and finding that out from
    // a generate that quietly never uses it is the bad version.
    let status = &app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .status;
    assert!(
        status.has_problem(),
        "growing past one cell must be said, not silent"
    );
}

/// **Shift+arrow flushes the mesh to that side, and the plain arrow still nudges.**
///
/// The pair is the point. `bs` states both, because a bare `b` is indifferent to Shift and would
/// swallow the chord — the collision the census exists to catch, and the one
/// `RemoveTile`/`DemoteTile` already set the precedent for. So this checks that Shift+arrow reaches
/// the align verb **and** that the unshifted arrow still reaches the nudge: either alone would pass
/// against a binding that had eaten the other.
#[test]
fn shift_arrow_flushes_the_mesh_and_the_bare_arrow_still_nudges() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
    use emerge_mapper::keys::{binding, Action};

    let root = Fixture::new("tile_align")
        // 0.2 m across in a 1 m tile: flush left is -0.4, which is not a multiple of either rung.
        .sized_descriptor("panel", "alpha", 0.2, 1.0)
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>, mut done: bevy::prelude::Local<bool>| {
                    if !*done {
                        keys.release_all();
                        for k in &chord {
                            keys.press(*k);
                        }
                        *done = true;
                    }
                },
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();
    }
    let at = |app: &App| -> (f32, f32) {
        app.world()
            .resource::<emerge_mapper::build::Build>()
            .open
            .as_ref()
            .and_then(|c| c.members.first())
            .map(|m| m.at)
            .unwrap_or_else(|| panic!("a member must be in the tile"))
    };

    once(&mut app, vec![binding(Action::TilesTab).key]);
    once(&mut app, vec![binding(Action::BuildArm).key]);
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    assert_eq!(at(&app), (0.0, 0.0), "brought in centred");

    // Bare arrow: a nudge of one rung, not a flush.
    once(&mut app, vec![binding(Action::BuildLeft).key]);
    let nudged = at(&app);
    assert_ne!(nudged, (0.0, 0.0), "the unshifted arrow must still nudge");
    assert!(
        nudged.0.abs() < 0.4,
        "a nudge is one rung, not the edge — got {nudged:?}"
    );

    // Shifted: straight to the edge, wherever it was.
    once(&mut app, vec![KeyCode::ShiftLeft, binding(Action::AlignLeft).key]);
    let flush = at(&app);
    assert!(
        (flush.0 + 0.4).abs() < 1e-4,
        "Shift+left must put a 0.2 m panel flush at -0.4 in a 1 m tile — got {flush:?}"
    );
    assert!(
        (flush.1 - nudged.1).abs() < 1e-6,
        "and it must not move the other axis: {flush:?} from {nudged:?}"
    );

    // **The tile did not grow to hold it.** Flush is the extreme position that still fits, so an
    // envelope that fits its contents must stay exactly one cell — a grow here would mean the verb
    // had overshot the edge it was aiming at.
    let size = match app.world().resource::<emerge_mapper::build::Build>().open {
        Some(ref c) => match c.envelope {
            emerge_core::composition::Envelope::Bounded { size } => size,
            _ => panic!("a tile claims a tile"),
        },
        None => panic!("still open"),
    };
    assert_eq!(
        (size.0, size.2),
        (emerge_core::grid::TILE, emerge_core::grid::TILE),
        "flush is the extreme position that still fits, so the tile stays one cell"
    );
}

/// **Undo steps back through the tile, one brought-in mesh at a time.**
///
/// Reported from use: *"Undo is not working on the tiles tab when I bring in two meshes."* It was not
/// broken, it was **absent** — `UndoTile` is bound in `Context::Meshes` only, over `library.ron`
/// edits, and nothing ever snapshotted the tile in hand. So `Cmd+Z` on this tab reached no handler
/// at all.
///
/// Two meshes specifically, because one would pass against a history that only ever holds the empty
/// tile: the second undo is the one that has to find the first mesh still there.
#[test]
fn undo_steps_back_through_the_meshes_brought_into_a_tile() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
    use emerge_mapper::keys::{binding, Action};

    let root = Fixture::new("tile_undo")
        .descriptor("floor", "alpha")
        .descriptor("wall", "alpha")
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>, mut done: bevy::prelude::Local<bool>| {
                    if !*done {
                        keys.release_all();
                        for k in &chord {
                            keys.press(*k);
                        }
                        *done = true;
                    }
                },
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();
    }
    let members = |app: &App| -> Vec<String> {
        app.world()
            .resource::<emerge_mapper::build::Build>()
            .open
            .as_ref()
            .map(|c| c.members.iter().map(|m| m.id.clone()).collect())
            .unwrap_or_default()
    };

    once(&mut app, vec![binding(Action::TilesTab).key]);
    once(&mut app, vec![binding(Action::BuildArm).key]);
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    // A different piece, so the two steps are distinguishable by name rather than by count alone.
    once(&mut app, vec![binding(Action::BuildArm).key]);
    once(&mut app, vec![binding(Action::BuildBack).key]);
    once(&mut app, vec![binding(Action::BuildArm).key]);
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    let two = members(&app);
    assert_eq!(two.len(), 2, "two meshes are in the tile: {two:?}");

    // `Cmd+Z` — the tab's own stack, not the mesh tab's.
    once(&mut app, vec![KeyCode::SuperLeft, binding(Action::UndoBuild).key]);
    let one = members(&app);
    assert_eq!(one.len(), 1, "one undo takes the second mesh back out: {one:?}");
    assert_eq!(one[0], two[0], "and it is the FIRST that survives, not whichever sorted first");

    once(&mut app, vec![KeyCode::SuperLeft, binding(Action::UndoBuild).key]);
    assert!(members(&app).is_empty(), "the second undo empties the tile");

    // And forward again, because a history that only goes one way is half a history.
    once(
        &mut app,
        vec![KeyCode::SuperLeft, KeyCode::ShiftLeft, binding(Action::RedoBuild).key],
    );
    assert_eq!(members(&app), one, "redo puts the first mesh back");
    once(
        &mut app,
        vec![KeyCode::SuperLeft, KeyCode::ShiftLeft, binding(Action::RedoBuild).key],
    );
    assert_eq!(members(&app), two, "and the second");

    // **The envelope travels with it.** `refit` runs before the recorder, so a resize is part of the
    // step that caused it rather than a separate thing to undo — otherwise every drop would cost two
    // presses to take back.
    once(&mut app, vec![KeyCode::SuperLeft, binding(Action::UndoBuild).key]);
    assert_eq!(members(&app).len(), 1, "one press, one step");
}

/// **A tile survives being saved and reopened — members, hole and all.**
///
/// The round-trip §7 of the tile-authoring plan asked for and which did not exist: *"build floor +
/// wall + a slot, save, reopen, assert members and slot survive."* Every part of it has its own unit
/// test — `build::place`, `Project::commit_composition`, `composition::validate`, the RON codec —
/// and none of them answers whether the whole path holds, which is the only question an author is
/// actually asking when they press `Cmd+S`.
///
/// It is driven through the **keys**, not the resources, on purpose. Calling `commit_composition`
/// directly would pass with the tab unwired, and a tile you cannot reach is not a tile you can save.
#[test]
fn a_tile_survives_a_save_and_a_reopen() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
    use emerge_mapper::keys::{binding, Action};

    let root = Fixture::new("tile_round_trip")
        .descriptor("floor", "alpha")
        .descriptor("wall", "alpha")
        .slot_token("wall-fixture")
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>, mut done: bevy::prelude::Local<bool>| {
                    if !*done {
                        keys.release_all();
                        for k in &chord {
                            keys.press(*k);
                        }
                        *done = true;
                    }
                },
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();
    }

    // A floor, a wall a cell away, and a hole above the wall — the shape the author described:
    // "floor mesh at the lowest, a wall mesh over it, a wall mounted light fixture on the wall mesh".
    once(&mut app, vec![binding(Action::TilesTab).key]);
    // `Space` takes the piece in hand — the arrows steer the tile while it is held and the library
    // list while it is not, which is the whole reason the door exists. Dropping does not put it
    // back, so several pieces go down without re-arming.
    once(&mut app, vec![binding(Action::BuildArm).key]);
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    once(&mut app, vec![binding(Action::BuildArm).key]);
    once(&mut app, vec![binding(Action::BuildBack).key]);
    once(&mut app, vec![binding(Action::BuildArm).key]);
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    // The wall lands centred like everything else, then moves — which is the model: bring it in,
    // then adjust it.
    once(&mut app, vec![binding(Action::BuildRight).key]);
    once(&mut app, vec![binding(Action::BuildUp).key]);
    once(&mut app, vec![KeyCode::ShiftLeft, binding(Action::BuildSlot).key]);

    let id = {
        let build = app.world().resource::<emerge_mapper::build::Build>();
        let open = build
            .open
            .as_ref()
            .unwrap_or_else(|| panic!("a tile must be open after arriving on the tab"));
        assert_eq!(
            open.members.len(),
            3,
            "two pieces and a hole were dropped; the status line said: {}",
            app.world().resource::<emerge_mapper::tiles::ImportState>().status.note_text()
        );
        open.id.clone()
    };

    // `Cmd+S` — Global, and the handler asks which tab is live rather than there being a second key.
    once(&mut app, vec![KeyCode::SuperLeft, binding(Action::Save).key]);
    // **Refusals only.** The status also carries the size notice — a tile bigger than one cell is
    // not solver content and says so — which is information rather than a failure, so asserting "no
    // problems at all" would make this test fail for the tile being large.
    assert!(
        !app.world()
            .resource::<emerge_mapper::tiles::ImportState>()
            .status
            .problems()
            .iter()
            .any(|p| p.line().contains("NOT SAVED")),
        "the save must not refuse: {:?}",
        app.world()
            .resource::<emerge_mapper::tiles::ImportState>()
            .status
            .problems()
            .iter()
            .map(|p| p.line())
            .collect::<Vec<_>>()
    );

    // **A second app on the same directory**, which is what reopening the editor is. Reading the
    // file back through `Project::open` also proves it validates — `commit_composition` refuses on
    // the way out, and this is the other end of that.
    let reopened = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the saved project must reopen: {e}"));
    let project = reopened.world().resource::<emerge_mapper::project::Project>();
    let saved = project
        .compositions
        .compositions
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| {
            panic!(
                "`{id}` must be in compositions.ron after a save; found {:?}",
                project.compositions.compositions.iter().map(|c| &c.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(saved.members.len(), 3, "every member must survive the round trip");
    let holes = saved
        .members
        .iter()
        .filter(|m| matches!(m.body, emerge_core::composition::Body::Slot { .. }))
        .count();
    assert_eq!(holes, 1, "the hole is a member like any other, and must come back as one");

    // And it is a tile the map can actually place: cell-sized in plan, or `from_compositions`
    // refuses it by name and the whole authoring loop produces something the solver cannot use.
    let emerge_core::composition::Envelope::Bounded { size } = saved.envelope else {
        panic!("a tile claims a tile");
    };
    // **Whole cells, and as many as its contents need.** Not one cell: the fixture's pieces are 1 m
    // cubes and one of them was moved a rung off centre, so two is the honest answer and the tile
    // resized to say it. What must hold is that the envelope is a whole number of tiles — a
    // fractional one is placeable at no grid spacing at all.
    let whole = |v: f32| (v / emerge_core::grid::TILE - (v / emerge_core::grid::TILE).round()).abs();
    assert!(
        whole(size.0) < 1e-4 && whole(size.2) < 1e-4,
        "a saved tile measures a whole number of cells, got {size:?}"
    );

    // **Then stamp it**, which is the other half §7 asked for: *"stamp it and assert the expanded
    // rows match."* `composition::expand` is the one seam — the game reaches it from
    // `emerge-bevy`, the editor from four sites — so this is the contract a third engine would
    // implement, checked against a tile that came off the keyboard rather than out of a fixture.
    let stamp = emerge_core::composition::Stamped {
        id: "s1".to_owned(),
        of: id.clone(),
        at: (10.0, 4.0),
        yaw: 0.0,
        overrides: Vec::new(),
        of_fingerprint: None,
        note: None,
        owned: false,
        owned_because: None,
    };
    let out = emerge_core::composition::expand(
        &project.map,
        &[stamp],
        &project.compositions.compositions,
        &project.library,
    )
    .unwrap_or_else(|e| panic!("the tile an author just saved must expand: {e}"));

    // **Two rows and one hole, not three rows.** A `Placed` names a descriptor and every consumer of
    // one expects a mesh, so a slot arriving as a placement would have every reader reaching for a
    // mesh that does not exist. The split is the whole reason `Expansion::slots` is its own field.
    assert_eq!(
        out.placements.len(),
        2,
        "the two pieces place; the hole is not a piece. Got {:?}",
        out.placements.iter().map(|p| &p.id).collect::<Vec<_>>()
    );
    assert_eq!(out.slots.len(), 1, "and the hole comes out as a hole");
    assert_eq!(
        out.slots[0].accepts, "wall-fixture",
        "carrying the token it was declared with, which is what a filler matches on"
    );

    // Provenance reads alike for both, so nothing has to parse an id back apart to learn what a row
    // belongs to.
    assert!(
        out.slots[0].id.starts_with("s1/"),
        "a hole is named `<stamp>/<member>` like a placement: {}",
        out.slots[0].id
    );
    for p in &out.placements {
        assert!(p.id.starts_with("s1/"), "and so is every row: {}", p.id);
    }
}

/// **The library is walkable from the Tiles tab, by keyboard.**
///
/// Picking the piece is half of building a tile, and it was the half that still cost a tab
/// round-trip — `2`, arrow, `3`, `Enter`, once per member — because the arrows were bound only in
/// `Context::Meshes`. The author's requirement for this loop was the keyboard: *"key strokes are
/// faster"*. A shared list that one of the two tabs sharing it cannot reach is shared in name only.
#[test]
fn the_arrows_walk_the_library_from_the_tiles_tab() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
    use emerge_mapper::keys::{binding, Action};

    let root = Fixture::new("tiles_arrows")
        .descriptor("aaa_floor", "alpha")
        .descriptor("zzz_wall", "alpha")
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>, mut done: bevy::prelude::Local<bool>| {
                    if !*done {
                        keys.release_all();
                        for k in &chord {
                            keys.press(*k);
                        }
                        *done = true;
                    }
                },
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();
    }

    once(&mut app, vec![binding(Action::TilesTab).key]);
    let first = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .selected_library_id
        .clone();

    once(&mut app, vec![binding(Action::BuildBack).key]);
    let after = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .selected_library_id
        .clone();

    assert_ne!(
        first, after,
        "an arrow on the Tiles tab must move the piece in hand — it is the verb the loop repeats most"
    );
    assert!(
        after.is_some(),
        "and it must land on a library id, since that is the only legal source for a member"
    );
}

/// **A tile cannot name a mesh the library does not carry.**
///
/// `ImportState::editing` falls back to the focused *candidate* when nothing in the library is
/// selected — and a candidate is a mesh that has been measured and not imported. Dropping one wrote
/// a member naming an id `library.ron` has never heard of, which expands to nothing at stamp time:
/// a tile that looks authored and places nothing. Reachable by hand today — select a candidate on
/// the Meshes tab, press the Tiles tab, press `Enter`. Refused at the door instead.
#[test]
fn a_piece_that_is_not_in_the_library_cannot_be_dropped_into_a_tile() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
    use emerge_mapper::keys::{binding, Action};

    let root = Fixture::new("tiles_unimported")
        .descriptor("wall", "alpha")
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    // A measured-but-unimported mesh, focused — exactly what a scan leaves behind.
    {
        let mut state = app
            .world_mut()
            .resource_mut::<emerge_mapper::tiles::ImportState>();
        state.candidates = vec![emerge_core::import::Candidate {
            mesh: "meshes/ghost.glb".to_owned(),
            proposed: emerge_core::descriptor::Descriptor {
                id: "ghost".to_owned(),
                ..Default::default()
            },
            measured: None,
            front_detail: None,
            triangles: 0,
            findings: Vec::new(),
        }];
        state.selected = 0;
        state.selected_library_id = None;
    }

    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>, mut done: bevy::prelude::Local<bool>| {
                    if !*done {
                        keys.release_all();
                        for k in &chord {
                            keys.press(*k);
                        }
                        *done = true;
                    }
                },
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();
    }

    once(&mut app, vec![binding(Action::TilesTab).key]);
    // **Arriving must not quietly fix this for us**, or the test proves nothing about the drop. The
    // tab only reaches for a library piece when *nothing* is in hand, and a candidate is something.
    app.world_mut()
        .resource_mut::<emerge_mapper::tiles::ImportState>()
        .selected_library_id = None;
    once(&mut app, vec![binding(Action::BuildDrop).key]);

    let members = app
        .world()
        .resource::<emerge_mapper::build::Build>()
        .open
        .as_ref()
        .map_or(0, |c| c.members.len());
    assert_eq!(
        members, 0,
        "a candidate is not a library descriptor, so dropping one must write no member"
    );
    assert!(
        app.world()
            .resource::<emerge_mapper::tiles::ImportState>()
            .status
            .has_problem(),
        "and the refusal must be said out loud, not swallowed"
    );
}

/// **A refusal raised on the Tiles tab is on screen, and does not follow you off it.**
///
/// The Meshes and Tiles tabs share one panel, and `ProblemBanner` carries the tab it speaks for —
/// so the split needed a second banner, and the visibility pass needed to *hide* the banner that is
/// not live rather than skip it. Skipping was safe only while every banner sat in a panel
/// `apply_mode` hid for it. Both halves are asserted here: a refusal shows on the tab that raised
/// it, and is gone on the tab that did not.
#[test]
fn a_refusal_on_the_tiles_tab_is_visible_and_stays_there() {
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
    use bevy::ui::Display;
    use bevy::input::ButtonInput;
    use emerge_mapper::keys::{binding, Action};

    let root = Fixture::new("tiles_banner")
        .descriptor("wall", "alpha")
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    // **One-shot, because a held key does not re-arm `just_pressed`.** Release everything first, so
    // the press this frame is a fresh edge rather than a key the previous system left down.
    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>, mut done: bevy::prelude::Local<bool>| {
                    if !*done {
                        keys.release_all();
                        for k in &chord {
                            keys.press(*k);
                        }
                        *done = true;
                    }
                },
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();
    }

    once(&mut app, vec![binding(Action::TilesTab).key]);
    // **Shift+Enter drops a hole, and the fixture declares no `slot` tokens** — so this is a refusal
    // by construction rather than by contrivance, and it is the one a real author meets first on a
    // project whose vocabulary has not grown a slot axis yet. A bare `Enter` would *succeed*:
    // `ImportState::editing` falls back to the selected candidate, so a piece is always in hand.
    once(&mut app, vec![KeyCode::ShiftLeft, binding(Action::BuildSlot).key]);
    app.update();

    assert!(
        app.world()
            .resource::<emerge_mapper::tiles::ImportState>()
            .status
            .has_problem(),
        "the premise: Shift+Enter with no slot tokens must refuse, or this test proves nothing"
    );

    let banner = |app: &mut App, want: emerge_mapper::tiles::Mode| -> Display {
        let mut q = app
            .world_mut()
            .query::<(&bevy::prelude::Node, &emerge_mapper::chrome::ProblemBanner)>();
        q.iter(app.world())
            .find(|(_, b)| b.0 == want)
            .map(|(n, _)| n.display)
            .unwrap_or_else(|| panic!("the shared panel must carry a banner for {}", want.label()))
    };

    assert_eq!(
        banner(&mut app, emerge_mapper::tiles::Mode::Tiles),
        Display::Flex,
        "a refusal the Tiles tab raised must be on the Tiles tab's banner"
    );

    // And it does not leak: switching tabs hides it, rather than leaving the shared panel showing a
    // line about work the author is no longer doing.
    once(&mut app, vec![binding(Action::MeshesTab).key]);
    app.update();
    assert_eq!(
        banner(&mut app, emerge_mapper::tiles::Mode::Tiles),
        Display::None,
        "the Tiles tab's banner must hide when the Meshes tab is live — they share a panel"
    );
}

/// **The Tiles tab is reachable and it builds.**
///
/// The arithmetic is unit-tested in `build.rs`; what no unit test can see is the wiring — that the
/// tab key reaches a system at all, that `Context::Tiles` takes the keyboard from `Context::Meshes`
/// without the two firing into each other, and that the resources every one of those systems takes
/// exist before the first frame. In Bevy 0.19 a missing `Res<T>` **panics its system** rather than
/// skipping it, and no unit test can answer "does this app survive its first frame".
#[test]
fn the_tiles_tab_opens_a_tile_and_walks_its_grid() {
    use emerge_mapper::build::Build;

    let root = Fixture::new("build_mode")
        .descriptor("wall", "alpha")
        .descriptor("floor", "beta")
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    // Onto the Tiles tab, then into BUILD.
    // **One shot, and that matters more than it looks.** A system added here runs every frame
    // forever, so a helper that merely presses would hold the key down — and `Space` re-toggling
    // `placing` on every frame means the handler takes the arm branch and returns before it ever
    // reaches the cursor. `release_all` first, so the press is a fresh edge rather than a key an
    // earlier helper left down.
    let before = |app: &mut bevy::prelude::App, key: bevy::prelude::KeyCode| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                move |mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
                      mut done: bevy::prelude::Local<bool>| {
                    if !*done {
                        keys.release_all();
                        keys.press(key);
                        *done = true;
                    }
                },
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();
    };

    let key = |a| emerge_mapper::keys::binding(a).key;
    before(&mut app, key(emerge_mapper::keys::Action::TilesTab));

    let build = app.world().resource::<Build>();
    let comp = build
        .open
        .as_ref()
        .unwrap_or_else(|| panic!("entering BUILD with nothing in hand must open a blank tile"));
    // Cell-sized in plan, or the solver can never place it — `from_compositions` refuses any other
    // width by name.
    let emerge_core::composition::Envelope::Bounded { size } = comp.envelope else {
        panic!("a tile claims a tile");
    };
    assert_eq!((size.0, size.2), (emerge_core::grid::TILE, emerge_core::grid::TILE));
    // **A brought-in mesh lands centred, and the arrows move *it*.** There is no cursor: the member
    // is the selection, so the thing on screen and the thing the keys act on are the same object.
    let key = |a| emerge_mapper::keys::binding(a).key;
    before(&mut app, key(emerge_mapper::keys::Action::BuildArm));
    before(&mut app, key(emerge_mapper::keys::Action::BuildDrop));

    let placed = |app: &bevy::prelude::App| -> (f32, f32) {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .and_then(|c| c.members.first())
            .map(|m| m.at)
            .unwrap_or_else(|| panic!("the drop must put a member in the tile"))
    };
    assert_eq!(placed(&app), (0.0, 0.0), "a brought-in mesh is centred, bottom on the floor");

    // One rung, one axis — the neighbouring square, never the diagonal one.
    before(&mut app, key(emerge_mapper::keys::Action::BuildRight));
    let moved = placed(&app);
    assert_ne!(moved, (0.0, 0.0), "an arrow must move the member it is focused on");
    assert!(
        (moved.0 != 0.0) ^ (moved.1 != 0.0),
        "exactly one plan axis may move — got {moved:?}"
    );

    // **And the tab does not turn the camera.** It was turned square-on for one commit to make the
    // arrows read straight, which traded the framing the author builds in for a key mapping.
    let rig = app.world().resource::<emerge_mapper::view::Rig>();
    assert_eq!(rig.yaw, 0.0, "arriving on the Tiles tab must not spin the view");

    // **And the panel keeps up.** This is the half that shipped broken once: the tab changed, the
    // status line said so, and the detail pane went on showing the mesh inspector — which reads as
    // the key having done nothing.
    app.update();
    let mut texts = app.world_mut().query::<&bevy::prelude::Text>();
    let shown: Vec<String> = texts.iter(app.world()).map(|t| t.0.clone()).collect();
    assert!(shown.iter().any(|t| t == "TILES"), "the strip must name the tab. Saw: {shown:?}");
    assert!(
        shown.iter().any(|t| t == "TILE"),
        "the pane must say it is showing a tile rather than a mesh. Saw: {shown:?}"
    );
    let id = app
        .world()
        .resource::<Build>()
        .open
        .as_ref()
        .map(|c| c.id.clone())
        .unwrap_or_default();
    assert!(
        shown.iter().any(|t| t.contains(&id)),
        "the pane must name the tile being built (`{id}`). Saw: {shown:?}"
    );
    // Read from the resource rather than hardcoded, so the assertion is "the pane agrees with the
    // member" rather than "it is at a number I typed" — the second breaks whenever the step or the
    // opening position changes, which is three times so far.
    let at = app.world().resource::<Build>().at;
    let want = format!("cursor {},{},{}", at.0, at.1, at.2);
    assert!(
        shown.iter().any(|t| t.contains(&want)),
        "the pane must show where the focused member is ({want}). Saw: {shown:?}"
    );
}

/// **A dropped piece stands up on the stage, and the focus lands on it.**
///
/// Two things no unit test can see. `build::place` is pure and tested; what it cannot answer is
/// whether the member reaches `composition::expand` and comes back out as an entity — the seam where
/// a tile that looks right in a panel and wrong on the stage would show. And `focus` is what the two
/// verbs that act on "this member" read, so a drop that does not move it silently aims them at
/// whichever member sorted first.
#[test]
fn a_dropped_piece_is_staged_and_takes_the_focus() {
    use emerge_mapper::build::{Build, StagedTile};

    let root = Fixture::new("build_stage")
        // `wall` sorts after `floor`, so a focus that does not follow the drop lands on the wrong one
        // — which is exactly what this asserts against.
        .descriptor("wall", "alpha")
        .descriptor("floor", "beta")
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    // **One-shot, and it releases first.** The idiom elsewhere in this file adds a system that
    // presses every frame, which fires exactly once — *"pressing an already-pressed key does not
    // re-arm `just_pressed`"*. That is enough for one keystroke and wrong for two: the second added
    // system's press is swallowed by the first one still holding the key. Releasing everything and
    // pressing once, guarded by a `Local`, makes each step a genuine keystroke.
    type Keys<'w> = bevy::prelude::ResMut<'w, bevy::input::ButtonInput<bevy::prelude::KeyCode>>;
    fn once(done: &mut bool, k: &mut Keys, action: emerge_mapper::keys::Action) {
        if *done {
            return;
        }
        *done = true;
        k.release_all();
        k.press(emerge_mapper::keys::binding(action).key);
    }
    fn to_tiles(mut done: bevy::prelude::Local<bool>, mut k: Keys) {
        once(&mut done, &mut k, emerge_mapper::keys::Action::TilesTab);
    }
    fn drop_a(mut done: bevy::prelude::Local<bool>, mut k: Keys) {
        once(&mut done, &mut k, emerge_mapper::keys::Action::BuildDrop);
    }
    fn drop_b(mut done: bevy::prelude::Local<bool>, mut k: Keys) {
        once(&mut done, &mut k, emerge_mapper::keys::Action::BuildDrop);
    }
    let step = |app: &mut bevy::prelude::App, sys: fn(bevy::prelude::Local<bool>, Keys)| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(sys, emerge_mapper::keys::Phase::Act),
        );
        app.update();
    };

    step(&mut app, to_tiles);
    // Pick a piece, which is what the right-hand list's arrow keys write. Set directly rather than
    // driven, because the walking is `move_selection`'s own tested job and what is under test here is
    // what happens to the piece *after* it is picked.
    let pick = |app: &mut bevy::prelude::App, id: &str| {
        app.world_mut()
            .resource_mut::<emerge_mapper::tiles::ImportState>()
            .selected_library_id = Some(id.to_owned());
    };

    // **Entering BUILD leaves something picked**, so the first `Enter` is a drop rather than a
    // refusal. Asserted before the explicit picks below override it.
    assert!(
        app.world()
            .resource::<emerge_mapper::tiles::ImportState>()
            .selected_library_id
            .is_some(),
        "entering BUILD with nothing ever picked must arm a piece, or the first keystroke refuses"
    );

    // **Two, and the order is the whole point.** Members are stored sorted by id, so dropping `floor`
    // then `wall` puts the one just dropped at index 1 — and a focus that never moves stays on
    // `floor`. With one member the assertion would pass on the default and prove nothing.
    pick(&mut app, "floor");
    step(&mut app, drop_a);
    pick(&mut app, "wall");
    step(&mut app, drop_b);
    // One more frame: the drop writes `Build`, and the stage is rebuilt by a system reading it.
    app.update();

    let build = app.world().resource::<Build>();
    let comp = build.open.as_ref().unwrap_or_else(|| panic!("a tile is open"));
    let ids: Vec<&str> = comp.members.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["floor", "wall"], "both drop, and the list stays sorted");
    assert_eq!(
        build.focus, 1,
        "the focus must be the member just dropped — `R` and Delete act on it, and here that is \
         `wall`, not the `floor` that happens to sort first"
    );

    let mut staged = app.world_mut().query::<&StagedTile>();
    assert_eq!(
        staged.iter(app.world()).count(),
        2,
        "both members must stand up on the stage — a tile that is only a list in a panel is the \
         feedback half of the loop missing"
    );
}
