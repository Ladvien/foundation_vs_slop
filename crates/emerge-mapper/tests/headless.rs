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
    assert!(just_pressed(&input, Context::Tiles, Action::RemoveTile));
    assert!(!just_pressed(&input, Context::Tiles, Action::EditTile));

    // A FRESH input, not `clear()`: `clear` keeps the pressed state, so an already-held key never
    // re-registers as just-pressed.
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(MOD_KEYS[0]);
    input.press(REMOVE_KEY);
    assert!(just_pressed(&input, Context::Map, Action::EditTile));
    assert!(
        !just_pressed(&input, Context::Tiles, Action::RemoveTile),
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
        assert_eq!(project.policy.face_bands, 1);
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

/// **`Cmd`+remove over the interface sends the PLACE selection**, not "nothing here to edit".
///
/// Reported live: *"I want to send back an item that is selected in the Place scroll area."* The
/// verb resolved its subject with `nearest_placement`, so it only ever reached a piece standing on
/// the map; over the list — where the author's cursor is, on the row they just clicked — it refused.
///
/// The armed row is deliberately **not** the descriptor the map places, so an assertion cannot be
/// satisfied by the other branch answering first. Both branches are driven, including the two
/// refusals, because a rule with an untested arm is a rule with an arm nobody has read.
#[test]
fn cmd_remove_over_the_interface_sends_the_place_selection() {
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
    let ask = |app: &App, on_ui: bool, under: Option<usize>| {
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("no project"));
        let state = app
            .world()
            .get_resource::<emerge_mapper::editor::EditorState>()
            .unwrap_or_else(|| panic!("no editor state"));
        edit_subject(on_ui, project, state, under)
    };

    arm(&mut app, Some(wall));
    // Over the interface: the armed row, whatever the map is showing.
    assert_eq!(
        ask(&app, true, Some(0)),
        Ok("wall".to_owned()),
        "the armed PLACE row is the subject when the pointer is on the interface"
    );
    // Over the map: the piece under the cursor, and the armed row is not consulted.
    assert_eq!(
        ask(&app, false, Some(0)),
        Ok("floor".to_owned()),
        "over the map the cursor decides, not the palette"
    );
    // Neither branch falls through to the other when its own subject is missing.
    assert_eq!(ask(&app, false, None), Err("nothing here to edit".to_owned()));
    arm(&mut app, None);
    assert_eq!(
        ask(&app, true, Some(0)),
        Err("nothing is selected in PLACE".to_owned()),
        "an empty palette selection must refuse, never quietly take the piece under the cursor"
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
