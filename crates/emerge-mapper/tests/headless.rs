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

/// **Type a tile's name into the prompt and commit it.**
///
/// Naming became explicit on 2026-08-15: `N` opens `chrome::NameBox` rather than minting
/// `<kit>/tile_n`, and `Cmd+S` on a still-provisional tile raises the same prompt instead of
/// writing that name to the kit. Both are answered the same way, so both are answered here.
///
/// The characters go in as **`KeyboardInput` messages**, not `ButtonInput`: every text field in this
/// crate reads the message stream and matches `logical_key`, which is the distinction
/// `bevy_debugger/input` exists to honour.
fn name_the_tile(app: &mut App, name: &str) {
    let tap = |app: &mut App, logical: bevy::input::keyboard::Key, code: KeyCode| {
        for state in [
            bevy::input::ButtonState::Pressed,
            bevy::input::ButtonState::Released,
        ] {
            app.world_mut()
                .write_message(bevy::input::keyboard::KeyboardInput {
                    key_code: code,
                    logical_key: logical.clone(),
                    state,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
        }
        app.update();
    };
    for c in name.chars() {
        tap(
            app,
            bevy::input::keyboard::Key::Character(c.to_string().into()),
            KeyCode::KeyA,
        );
    }
    tap(app, bevy::input::keyboard::Key::Enter, KeyCode::Enter);
    for _ in 0..2 {
        app.update();
    }
}

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
        app.world()
            .get_resource::<emerge_mapper::keys::Live>()
            .is_some(),
        "`Live` is read from three plugins and must be registered by the one that owns it"
    );
    assert!(
        app.world()
            .get_resource::<emerge_mapper::keys::Repeat>()
            .is_some(),
        "`Repeat` is taken by the aim keys; without it that system panics on its first frame"
    );
}

/// The picking resource is registered by the plugin that reads it, on the same rule.
#[test]
fn the_tiles_plugin_registers_the_resources_its_systems_take() {
    let mut app = headless();
    app.add_plugins(emerge_mapper::tiles::TilesPlugin);

    for (name, present) in [
        (
            "LatticePick",
            app.world()
                .get_resource::<emerge_mapper::tiles::LatticePick>()
                .is_some(),
        ),
        (
            "CellEdit",
            app.world()
                .get_resource::<emerge_mapper::tiles::CellEdit>()
                .is_some(),
        ),
        (
            "Mode",
            app.world()
                .get_resource::<emerge_mapper::tiles::Mode>()
                .is_some(),
        ),
        // The Tiles tab's width field. `editor::not_typing` and `editor::sense_context` both read it
        // as a bare `Res`, and both are run conditions — which Bevy 0.19 evaluates with **no**
        // short-circuit, so an unregistered one panics every frame regardless of which tab is live.
        (
            "ScaleEdit",
            app.world()
                .get_resource::<emerge_mapper::tiles::ScaleEdit>()
                .is_some(),
        ),
    ] {
        assert!(
            present,
            "TilesPlugin does not register {name}, so its readers panic on frame one"
        );
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
        (
            "MoveDrag",
            app.world()
                .get_resource::<emerge_mapper::editor::MoveDrag>()
                .is_some(),
        ),
        // The cell fine placement is confined to while the modifier is down.
        (
            "FineAnchor",
            app.world()
                .get_resource::<emerge_mapper::editor::FineAnchor>()
                .is_some(),
        ),
        // The box being dragged out to fill.
        (
            "PlaceDrag",
            app.world()
                .get_resource::<emerge_mapper::editor::PlaceDrag>()
                .is_some(),
        ),
        // What the piece-verbs would act on, written for the UNDER readout. `refresh_status` takes
        // it as a bare `Res<_>`, which panics its system in 0.19 if nobody registered it.
        (
            "UnderCursor",
            app.world()
                .get_resource::<emerge_mapper::editor::UnderCursor>()
                .is_some(),
        ),
        // The drawn grid's spacing. `draw_map_grid` takes it as a bare `Res<_>`.
        (
            "Rung",
            app.world()
                .get_resource::<emerge_mapper::editor::Rung>()
                .is_some(),
        ),
    ] {
        assert!(
            present,
            "EditorPlugin does not register {name}, so its readers panic on frame one"
        );
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
    use emerge_mapper::keys::{
        Action, Context, Live, MOD_KEYS, REMOVE_KEY, Stance, binding, just_pressed,
    };

    let send = binding(Action::EditTile);
    assert_eq!(
        send.key, REMOVE_KEY,
        "it is the remove key, with the command modifier"
    );
    assert!(send.needs_mod);

    // Bare remove on the Tiles tab removes; it does not send anything to be defined.
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(REMOVE_KEY);
    assert!(just_pressed(
        &input,
        Live(Context::Meshes, Stance::Idle),
        Action::RemoveTile
    ));
    assert!(!just_pressed(
        &input,
        Live(Context::Meshes, Stance::Idle),
        Action::EditTile
    ));

    // A FRESH input, not `clear()`: `clear` keeps the pressed state, so an already-held key never
    // re-registers as just-pressed.
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(MOD_KEYS[0]);
    input.press(REMOVE_KEY);
    assert!(just_pressed(
        &input,
        Live(Context::Map, Stance::Idle),
        Action::EditTile
    ));
    assert!(
        !just_pressed(
            &input,
            Live(Context::Meshes, Stance::Idle),
            Action::RemoveTile
        ),
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
    use emerge_mapper::keys::{Action, Context, binding};
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
    use emerge_mapper::keys::{Action, binding};
    // The ones added most recently, and the ones most likely to be forgotten next.
    for action in [
        Action::ScanMesh,
        Action::RotateMeshX,
        Action::RotateMeshY,
        Action::RotateMeshZ,
        Action::Remove,
        Action::Straighten,
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
    use emerge_mapper::keys::{Action, binding};
    assert_eq!(binding(Action::Remove).key, KeyCode::KeyX);
    assert_eq!(binding(Action::Remove).chord, "X");
    assert_eq!(binding(Action::Straighten).key, KeyCode::KeyV);
    assert_eq!(binding(Action::Straighten).chord, "V");
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
        assert!(
            forward.dot(right).abs() < 1e-4,
            "detent {detent}: not perpendicular"
        );
        assert!(
            forward.y.abs() < 1e-6 && right.y.abs() < 1e-6,
            "panning must stay on the ground"
        );
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
    use emerge_core::descriptor::{Face, pick_cell};
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
        let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes).unwrap_or_else(|e| panic!("{e}"));
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
        let mut app = harness::build_headless_at(&root(), "untitled_map", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..2 {
            app.update();
        }
        app.world_mut()
            .insert_resource(emerge_mapper::tiles::Mode::Anim);
        for _ in 0..10 {
            app.update();
        }
        let bench = app
            .world()
            .resource::<emerge_mapper::anim_tab::BenchState>();
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
        let mut app = harness::build_headless_at(&root(), "untitled_map", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..2 {
            app.update();
        }
        app.world_mut()
            .insert_resource(emerge_mapper::tiles::Mode::Anim);
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
        app.world_mut()
            .insert_resource(emerge_mapper::tiles::Mode::Map);
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
        let mut app = harness::build_headless_at(&root(), "untitled_map", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..2 {
            app.update();
        }
        app.world_mut()
            .insert_resource(emerge_mapper::tiles::Mode::Anim);
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
        let plots = app
            .world()
            .resource::<emerge_mapper::anim_plots::BenchPlots>();
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
    ///
    /// # It also guards the kit against being emptied
    ///
    /// `assets/emerge/site/` is **shared with the game** — `src/site/kit.rs::SITE_PROJECT_DIR` — so
    /// it is not a scratchpad. On 2026-08-15 it was cleared to make a blank slate to author on, and
    /// that took 32 game tests down with it (`site::{kit,layout,pieces,people,smart}`, `emerge_map`)
    /// while this suite stayed green, because nothing here reads the game's side. The blank slate
    /// now lives in its own kit, `assets/emerge/ozea/`. **A piece count is the cheap alarm** for that
    /// happening again, which is why this asserts the kit is populated rather than merely loadable.
    #[test]
    fn the_editor_boots_on_the_site_kit() {
        let mut app = harness::build_headless_at(&root(), "untitled_map", Some("site"), emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..10 {
            app.update();
        }
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("the project resource is gone"));
        // The claim that matters and is why this test exists: the real project on disk opens, and
        // the editor survives frames on it — a missing `Res<T>` panics its system in Bevy 0.19
        // rather than skipping, and no unit test can see that.
        assert!(
            !project.library.descriptors.is_empty(),
            "the SHIPPED site kit is empty. This directory is the game's kit too, so an empty one \
             is a broken game, not a blank canvas — author on `--kit ozea` instead and put this \
             back with `git checkout HEAD -- assets/emerge/site/`"
        );
        // The kit's own configuration is the project rather than the content.
        assert_eq!(project.lattice.face_bands, 1);
    }

    /// **Every tab offers the way out, and the pointer can take it.**
    ///
    /// Asked for at the keyboard: *"when we go into the map editor, we actually need a button to go
    /// back to the main UI."* There was only `Cmd+O` — a key nothing on screen mentioned, in the
    /// one place where not finding it means closing the window.
    ///
    /// Four tabs build their own furniture, so the failure this guards is a fifth arriving without
    /// one, or a panel losing it in a rewrite. It also asserts the entity is **pickable**: the panel
    /// root is `Pickable::IGNORE` so the world stays reachable through it, and a button inheriting
    /// that would look exactly like a working one and answer no clicks at all.
    #[test]
    fn every_panel_offers_the_way_back() {
        let mut app = harness::build_headless_at(&root(), "untitled_map", Some("site"), emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..10 {
            app.update();
        }
        let mut q = app
            .world_mut()
            .query::<(&emerge_mapper::chrome::BackButton, &bevy::picking::Pickable)>();
        let found: Vec<_> = q.iter(app.world()).collect();
        assert!(
            found.len() >= 4,
            "each tab's panel needs its own way back; found {}",
            found.len()
        );
        assert!(
            found
                .iter()
                .all(|(_, p)| p.should_block_lower || p.is_hoverable),
            "a back button inheriting the panel root's `Pickable::IGNORE` answers no clicks"
        );
    }

    /// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
    ///
    /// Reported live: sending `site/floor` over from the PLACE list "didn't open the item in Tiles",
    /// and a second piece did. `edit_subject` is unit-tested and answers `site/floor` correctly, so
    /// what is asserted here is the other half — that the door the answer is handed to actually
    /// opens, **on the first send of a session**, for the piece that failed.
    ///
    /// It read the shipped kit until 2026-08-15, when that kit was deliberately emptied — so it now
    /// builds its own project, which is this crate's default rule anyway. Nothing about the defect
    /// was corpus-specific: the pair `rebuild_detail` guards on needs the id in **both** `measured`
    /// and the layered library, while the door only checked the latter, and that is true of any
    /// descriptor.
    #[test]
    fn the_first_send_of_a_session_opens_the_piece_it_names() {
        let root = Fixture::new("first-send")
            .descriptor("floor", "site")
            .build("untitled_map");
        let mut app =
            harness::build_headless(&root, "untitled_map", None).unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..10 {
            app.update();
        }

        // Untouched: this is the first send of the session, which is the case reported. Deliberately
        // NOT the Meshes door — that door scans on the way in, which is a different case.
        assert!(
            !app.world()
                .resource::<emerge_mapper::tiles::ImportState>()
                .scanned,
            "this test is about the FIRST send — a scanned tab is a different case"
        );

        let world = app.world_mut();
        world.resource_scope(
            |world, project: bevy::prelude::Mut<emerge_mapper::project::Project>| {
                world.resource_scope(
                    |world, mut import: bevy::prelude::Mut<emerge_mapper::tiles::ImportState>| {
                        let mut mode = world.resource_mut::<emerge_mapper::tiles::Mode>();
                        let mut state = emerge_mapper::editor::EditorState::default();
                        emerge_mapper::editor::send_to_tiles_for_test(
                            Ok("floor".to_owned()),
                            &project,
                            &mut state,
                            &mut mode,
                            &mut import,
                        );
                        assert!(
                            !state.status.has_problem(),
                            "the door refused `floor`: {}",
                            state.status.problem_text()
                        );
                        assert_eq!(
                            import.selected_library_id.as_deref(),
                            Some("floor"),
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
                            "`floor` is not in the MEASURED layer, so the detail pane draws nothing"
                        );
                        assert!(
                            import.placed(&project).is_some(),
                            "`site/floor` is not in the layered library as placed"
                        );
                    },
                );
            },
        );
    }

    /// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
    ///
    /// The end of the load chain, pinned: measurements on disk, policy layered over them, lattice
    /// validated, and an **authored** subgrid still intact in front of an author. `site/wall` is the
    /// subject because it is the one shipped piece whose lattice is hand-authored rather than
    /// derived — ten cells down its run face, every one carrying the `wall` edge token.
    ///
    /// It cannot be repointed at a `Fixture`: every descriptor `Fixture` writes carries
    /// `subgrid: None`, so a fixture version would assert nothing. That is also why it is here
    /// rather than deleted — the derivation *door* is covered on both sides by
    /// `derived_edges_refuse_an_undeclared_token_and_say_which` and
    /// `derived_edges_land_once_the_project_declares_them`, but **that an authored lattice survives
    /// the disk round-trip** has no other guard.
    ///
    /// # It was retired for a day, and the reason it came back is the point
    ///
    /// On 2026-08-15 `assets/emerge/site/` was emptied to make a blank slate, so this test's subject
    /// vanished and it was deleted with its reasoning left in place of its body. The kit turned out
    /// to be the **game's** kit as well (§1 of the blank-slate handoff); it was restored and the
    /// blank slate moved to `assets/emerge/site_v2/`, which brought the authored wall back with it.
    /// A test deleted because its corpus disappeared is worth re-reading whenever the corpus returns.
    #[test]
    fn the_authored_edge_tokens_reach_the_editor() {
        let mut app = harness::build_headless_at(&root(), "untitled_map", Some("site"), emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..10 {
            app.update();
        }
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("the project resource is gone"));

        let wall = project
            .library
            .get("site/wall")
            .unwrap_or_else(|| panic!("`site/wall` is not in the layered library"));
        let subgrid = wall
            .subgrid
            .as_ref()
            .unwrap_or_else(|| panic!("`site/wall` reached the editor with no authored subgrid"));

        let edged: Vec<&emerge_core::descriptor::SubCell> = subgrid
            .cells
            .iter()
            .filter(|c| c.edge.as_deref() == Some("wall"))
            .collect();
        assert_eq!(
            edged.len(),
            10,
            "`site/wall` ships ten authored `wall` cells down its run face; the layered library \
             handed the editor {}. An authored lattice that does not survive the disk round-trip \
             is a wall that stops sealing rooms, and nothing else in this suite would notice.",
            edged.len()
        );
        // All on one face — the run — which is what makes them a *run* face rather than a scatter.
        assert!(
            edged.iter().all(|c| c.at.0 == 0),
            "the authored cells left the run face: {:?}",
            edged.iter().map(|c| c.at).collect::<Vec<_>>()
        );
    }

    /// **The id counter starts past everything the file already names.** It used to start at zero
    /// every session, so reopening a saved map re-minted its own `wall@1`, `wall@2`, … — and undo,
    /// which despawns by id match, then swept the originals off the screen along with the fill it
    /// was taking back. The counter must clear the largest `@n` in the file, whatever shape the
    /// other ids take.
    #[test]
    fn minted_ids_start_past_what_the_map_already_names() {
        let mut map = emerge_core::map::Map::default();
        for id in [
            "wall@7",
            "crate@12",
            "records_desk",
            "oddly@named@3",
            "x@notanumber",
        ] {
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
        let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
        app.update();
        let open = app
            .world()
            .get_resource::<emerge_mapper::project::OpenMap>()
            .unwrap_or_else(|| panic!("no open map"));
        let want = emerge_mapper::editor::next_id_after(&open.map);
        // The HIGH-WATER MARK, not the next id: `next_id_after` returns the largest `@n` on file
        // and every mint site increments before it formats. Worth pinning, because the name reads
        // like the other thing.
        assert_eq!(want, 41, "the fixture's highest authored id is `wall@41`");
        let state = app
            .world()
            .get_resource::<emerge_mapper::editor::EditorState>()
            .unwrap_or_else(|| panic!("no editor state"));
        assert_eq!(
            state.minted(),
            want,
            "the counter must start where the file stops"
        );
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
        let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes).unwrap_or_else(|e| panic!("{e}"));
        app.update();

        // Enter the Tiles tab the way the author does: the Tab key, which is also what triggers
        // the first scan. As a real input MESSAGE, not a hand-set `ButtonInput` — the input plugin
        // clears `just_pressed` at the top of every frame, so a hand-set press is wiped before any
        // editor system can read it.
        // **No `Tab` tap.** It used to be how a test reached the Meshes panel; the Kit door
        // opens on it. `Tab` now cycles that door's three panels, so tapping it here would
        // walk straight off the panel under test.
        for _ in 0..3 {
            app.update();
        }

        let state = app
            .world()
            .get_resource::<emerge_mapper::tiles::ImportState>()
            .unwrap_or_else(|| panic!("no import state"));
        assert!(state.scanned, "entering the tab must have scanned");
        assert!(
            !state.candidates.is_empty(),
            "the fixture wrote three unimported meshes"
        );
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
        let root = Fixture::new("update")
            .descriptor("wall", "alpha")
            .build("m");
        let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes).unwrap_or_else(|e| panic!("{e}"));
        app.update();

        let tap = |app: &mut App, key: KeyCode, logical: bevy::input::keyboard::Key| {
            for state in [
                bevy::input::ButtonState::Pressed,
                bevy::input::ButtonState::Released,
            ] {
                app.world_mut()
                    .write_message(bevy::input::keyboard::KeyboardInput {
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
        // **Opened on the Tiles panel** rather than tapped into it: `Tab` cycles the Kit
        // door's three panels, so a tap here depends on which one the door opened on.
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
        let _open = app
            .world()
            .get_resource::<emerge_mapper::project::OpenMap>()
            .unwrap_or_else(|| panic!("no open map"));
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
        let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes).unwrap_or_else(|e| panic!("{e}"));
        app.update();

        let tap = |app: &mut App, key: KeyCode, logical: bevy::input::keyboard::Key| {
            for state in [
                bevy::input::ButtonState::Pressed,
                bevy::input::ButtonState::Released,
            ] {
                app.world_mut()
                    .write_message(bevy::input::keyboard::KeyboardInput {
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
        // **Opened on the Tiles panel** rather than tapped into it: `Tab` cycles the Kit
        // door's three panels, so a tap here depends on which one the door opened on.
        for _ in 0..3 {
            app.update();
        }

        // Point the selected candidate's proposal at an id the library already owns.
        {
            let mut state = app
                .world_mut()
                .resource_mut::<emerge_mapper::tiles::ImportState>();
            state.selected_library_id = None;
            assert!(
                !state.candidates.is_empty(),
                "the fixture wrote an unimported mesh"
            );
            let at = state.selected;
            state.candidates[at].proposed.id = "wall".to_owned();
        }
        for _ in 0..3 {
            app.update();
        }

        let before = app
            .world()
            .resource::<emerge_mapper::project::Project>()
            .measured
            .descriptors
            .len();
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
        let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes).unwrap_or_else(|e| panic!("{e}"));
        app.update();
        // **No `Tab` tap.** It used to be how a test reached the Meshes panel; the Kit door
        // opens on it. `Tab` now cycles that door's three panels, so tapping it here would
        // walk straight off the panel under test.
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
            let (a, mesh_a) = picks
                .next()
                .unwrap_or_else(|| panic!("no unblocked candidates"));
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
            let mut q = app.world_mut().query::<&emerge_mapper::tiles::PreviewOf>();
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
        let err = harness::build_headless_at(std::path::Path::new("/nonexistent"), "m", None, emerge_mapper::tiles::Mode::Meshes)
            .err()
            .unwrap_or_default();
        assert!(!err.is_empty(), "opening nothing must say so");
    }
}

/// **A map's kit selection narrows the palette, and cannot narrow it into a lie.**
///
/// The other half of the checkbox: turning a kit off has to actually stop offering its pieces, and
/// turning off a kit the map is *standing on* must not hide the rows that describe what is already
/// there. `Project::palette_namespaces` folds the in-use set back in for exactly that, which is what
/// lets the control be a checkbox rather than a decision with consequences.
///
/// **The library is untouched either way.** Every bound kit still loads, so a placement always
/// resolves and a composition may still seat two kits' pieces — this is a filter on what an author
/// is *offered*, never on what a map can mean.
#[test]
fn the_maps_kit_selection_narrows_the_palette_but_never_hides_what_is_placed() {
    use emerge_mapper::editor::{palette_indices, EditorState};
    use emerge_mapper::filter::Filters;
    use emerge_mapper::project::{OpenMap, Project};

    let root = Fixture::new("map-palette")
        .descriptor("bench", "props")
        .kit("site", "ozea", &["site/wall"])
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let offered = |app: &mut App| -> Vec<String> {
        let world = app.world_mut();
        let project = world.resource::<Project>();
        let open = world.resource::<OpenMap>();
        let state = world.resource::<EditorState>();
        let filters = world.resource::<Filters>();
        palette_indices(project, open, state, filters)
            .into_iter()
            .filter_map(|i| project.library.descriptors.get(i).map(|d| d.id.clone()))
            .collect()
    };

    // Nothing chosen means everything offered.
    let all = offered(&mut app);
    assert!(
        all.iter().any(|id| id == "bench") && all.iter().any(|id| id == "site/wall"),
        "an empty selection offers every bound kit: {all:?}"
    );

    // Turn `site` off: its pieces stop being offered, the furniture kit's do not.
    app.world_mut().resource_mut::<OpenMap>().map.palette = vec!["furniture".to_owned()];
    app.update();
    let narrowed = offered(&mut app);
    assert!(
        narrowed.iter().any(|id| id == "bench"),
        "the kit that is still on keeps its rows: {narrowed:?}"
    );
    assert!(
        !narrowed.iter().any(|id| id == "site/wall"),
        "and the one turned off loses them: {narrowed:?}"
    );
    // **But the library still has it**, which is why a placement cannot be stranded.
    assert!(
        app.world().resource::<Project>().library.get("site/wall").is_some(),
        "the selection filters the palette, never what the map can resolve"
    );

    // **Now the other direction, which is where this was broken.**
    //
    // Turning off the kit whose ids are **flat** — the shape every shipped kit has. The first
    // version read the namespace out of the id, so `bench` belonged to no kit, matched no
    // selection, and was offered whatever was ticked: the control was inert on the only project
    // that matters and the test above still passed, because it only ever turned off a *namespaced*
    // kit. Driving the shipped project is what found it.
    app.world_mut().resource_mut::<OpenMap>().map.palette = vec!["site".to_owned()];
    app.update();
    let flat_off = offered(&mut app);
    assert!(
        !flat_off.iter().any(|id| id == "bench"),
        "a kit with flat ids turns off like any other — `Project::kit_of` asks which library \
         defines a piece, never what its id spells: {flat_off:?}"
    );
    assert!(
        flat_off.iter().any(|id| id == "site/wall"),
        "and the one still on keeps its rows: {flat_off:?}"
    );

    // In use protects a flat kit too, for the same reason.
    {
        let mut open = app.world_mut().resource_mut::<OpenMap>();
        open.map.placements.push(emerge_core::map::Placed {
            id: "bench@0".to_owned(),
            descriptor: "bench".to_owned(),
            ..Default::default()
        });
    }
    app.update();
    assert!(
        offered(&mut app).iter().any(|id| id == "bench"),
        "a placed flat piece keeps its palette row whatever the selection says"
    );
    {
        let mut open = app.world_mut().resource_mut::<OpenMap>();
        open.map.placements.clear();
        open.map.palette = vec!["furniture".to_owned()];
    }
    app.update();

    // Now place one of its pieces and the row comes back, still with `site` turned off.
    {
        let mut open = app.world_mut().resource_mut::<OpenMap>();
        open.map.placements.push(emerge_core::map::Placed {
            id: "wall@0".to_owned(),
            descriptor: "site/wall".to_owned(),
            ..Default::default()
        });
    }
    app.update();
    let with_placement = offered(&mut app);
    assert!(
        with_placement.iter().any(|id| id == "site/wall"),
        "a kit the map stands on is offered whatever the selection says — otherwise the author \
         cannot find, match or re-place the pieces in front of them: {with_placement:?}"
    );
}

/// **A tile can seat two kits' pieces, and a map can stamp it.** This is what the whole collections
/// split was for.
///
/// Before it, `compositions.ron` sat beside `library.ron` inside a kit directory and `Project::open`
/// loaded exactly one kit — so a tile authored in `site` was invisible to every map opened on
/// `furniture`, and a tile naming both could not be validated at all, because neither kit's library
/// could answer for the other's pieces.
///
/// Now the compositions are the **project's**, the library a map resolves against is every bound
/// kit **merged**, and the map is stamped without caring which directory anything came from. The
/// stamp itself is unchanged — it was always a reference expanded at load and never written back,
/// which is what makes editing the tile change every map that stamped it.
#[test]
fn a_tile_can_seat_two_kits_pieces_and_a_map_can_stamp_it() {
    let root = Fixture::new("cross-kit")
        .descriptor("bench", "props")
        .kit("site", "ozea", &["site/wall"])
        .composition(
            "nook",
            &[
                ("bench", "bench", (0.0, 0.0)),
                ("wall", "site/wall", (0.0, 0.6)),
            ],
        )
        .build("m");

    let project = emerge_mapper::project::Project::open(&root, None)
        .unwrap_or_else(|e| panic!("{e}"));
    let open = emerge_mapper::project::OpenMap::open(&project, "m")
        .unwrap_or_else(|e| panic!("{e}"));

    // **One library, two directories.** Neither kit could have validated this tile alone.
    assert!(
        project.library.get("bench").is_some() && project.library.get("site/wall").is_some(),
        "both kits' pieces resolve in the merge"
    );
    assert!(
        project.compositions.compositions.iter().any(|c| c.id == "nook"),
        "and the tile that names both loaded, which means it validated against that merge"
    );

    // **And a map stamps it.** `expand` is the one expander the game's loader also uses, so a stamp
    // that resolves here resolves there.
    let mut map = open.map.clone();
    map.stamps.push(emerge_core::composition::Stamped {
        id: "nook@0".to_owned(),
        of: "nook".to_owned(),
        ..Default::default()
    });
    let out = emerge_core::composition::expand(
        &map,
        &map.stamps,
        &project.compositions.compositions,
        &project.library,
    )
    .unwrap_or_else(|e| panic!("a cross-kit tile has to stamp: {e}"));
    assert_eq!(
        out.placements.len(),
        2,
        "one row per member, from two different kits: {:?}",
        out.placements.iter().map(|p| &p.descriptor).collect::<Vec<_>>()
    );
}

/// **Two directories can provide one namespace — one at a time, which is what binding is for.**
///
/// `site/` and `site_greybox/` define the **identical** 45 `site/*` ids. That is what makes one a
/// re-skin of the other, and it is why "what does `site/floor` mean" is a question about the
/// *project*: neither directory can answer it about itself.
///
/// So the pair is not loaded together — `kits.ron` refuses to bind one namespace twice, naming both
/// skins — and a project picks one. **Swapping the skin is editing one line**, and every map,
/// composition and id resolves against the other directory without moving.
///
/// Until `Fixture::kit` existed nothing in this suite could build the shape at all; every fixture
/// was a single root kit, so this was pinned only by an asset-contract test reading the shipped
/// corpus, which is the corpus dependence this file exists to avoid.
#[test]
fn a_re_skin_pair_binds_one_at_a_time_and_either_resolves() {
    let root = Fixture::new("two-kits")
        .descriptor("lamp", "props")
        .kit("site", "ozea", &["site/floor", "site/wall"])
        .kit("site_greybox", "grey", &["site/floor", "site/wall"])
        .build("m");

    // Both bound at once is the ambiguity binding exists to resolve, and it is refused by name.
    let e = emerge_mapper::project::Project::open(&root, None)
        .err()
        .unwrap_or_else(|| panic!("one namespace, two directories: that has to be refused"));
    assert!(e.contains("bound twice"), "{e}");
    assert!(e.contains("site_greybox"), "and it names both skins: {e}");

    for skin in ["site", "site_greybox"] {
        Fixture::bind(
            &root,
            &[("furniture", "furniture"), ("site", skin)],
            "furniture",
        );
        let p = emerge_mapper::project::Project::open(&root, None)
            .unwrap_or_else(|e| panic!("`{skin}` should open: {e}"));
        for id in ["site/floor", "site/wall"] {
            assert!(
                p.library.get(id).is_some(),
                "`{skin}` provides `{id}` — being a provider of the namespace is the whole claim"
            );
        }
        // **And the merge is a merge.** The furniture kit's piece resolves in the same library, from
        // a different directory, which is the feature: a tile may seat both.
        assert!(
            p.library.get("lamp").is_some(),
            "every bound kit is loaded, not just the one work lands in"
        );
    }
}

/// **A kit's namespace comes from its pieces, not from its directory.**
///
/// The sharp case, and the one that was getting the right answer for the wrong reason:
/// `assets/emerge/site_v2/` held pieces named `site/*` and correctly minted `site/tile_n`, because
/// a directory is a *skin* and the namespace is the *interface* it implements. What it did to get
/// there was read `descriptors.first()` and split on `/` — so the answer depended on which piece
/// happened to sort first, and on an unnamespaced library it substituted the literal `"kit"`.
///
/// Both halves are pinned here: a directory named for something else still answers `site`, and a
/// library with no namespace at all answers with its own directory rather than a word nobody chose.
#[test]
fn a_kits_namespace_comes_from_its_pieces_not_its_directory() {
    let root = Fixture::new("ns-from-pieces")
        .descriptor("lamp", "props")
        .kit("greybox", "grey", &["site/floor", "site/wall"])
        .build("m");

    let named = emerge_mapper::project::Project::open(&root, Some("greybox"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        named.namespace, "site",
        "a kit implementing `site/*` belongs to `site` however its directory is spelled"
    );

    let flat = emerge_mapper::project::Project::open(&root, None)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        flat.namespace, "furniture",
        "and a library with no namespace answers with its own directory, never a literal"
    );
}

/// **A library in two namespaces is refused at open, and the refusal names both.**
///
/// One directory implements one namespace: that is what lets a project bind `site` to this kit or
/// to another providing the same pieces. Reading only the first descriptor answered this question
/// by accident, and answered it differently depending on the order of the file.
#[test]
fn a_kit_in_two_namespaces_is_refused_at_open() {
    let root = Fixture::new("ns-mixed")
        .descriptor("lamp", "props")
        .kit("muddle", "mix", &["site/floor", "lab/bench"])
        .build("m");

    let e = emerge_mapper::project::Project::open(&root, Some("muddle"))
        .err()
        .unwrap_or_else(|| panic!("a kit cannot implement two namespaces"));
    assert!(e.contains("site") && e.contains("lab/bench"), "{e}");
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
    use emerge_mapper::project::{OpenMap, Project};
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
                &[
                    ("table", "table", (0.0, 0.0)),
                    ("chair_north", "chair", (0.0, -1.0)),
                ],
            )
            .build("m");
        let mut app = emerge_mapper::harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..3 {
            app.update();
        }

        let before = app.world().resource::<OpenMap>().map.placements.len();
        app.world_mut().resource_mut::<ComposeState>().armed = Some("break_table".to_owned());

        // Through the same call the click makes, so this cannot pass while the click path is broken.
        {
            let world = app.world_mut();
            world.resource_scope(|world, mut project: bevy::prelude::Mut<Project>| {
                world.resource_scope(|world, mut open: bevy::prelude::Mut<OpenMap>| {
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                let mut compose = ComposeState {
                    armed: Some("break_table".to_owned()),
                    ..Default::default()
                };
                emerge_mapper::editor::stamp_here_for_test(
                    &mut project,
                    &mut open,
                    &mut state,
                    &mut compose,
                    (2.0, 2.0),
                );
                });
            });
        }
        app.update();

        let open = app.world().resource::<OpenMap>();
        assert_eq!(open.map.stamps.len(), 1, "no stamp landed");
        assert_eq!(open.map.stamps[0].of, "break_table");
        assert_eq!(
            open.map.placements.len(),
            before,
            "expansion must NOT be written into placements — the map holds the reference"
        );

        // **And it comes back off.** `Undo` is closed under inversion, so a stamp has to invert to
        // something that inverts back to a stamp; asserting only the forward direction would pass
        // for an entry that undoes and then cannot be redone.
        emerge_mapper::editor::undo_for_test(app.world_mut());
        app.update();
        assert!(
            app.world().resource::<OpenMap>().map.stamps.is_empty(),
            "undo left the stamp in the map"
        );
        emerge_mapper::editor::redo_for_test(app.world_mut());
        app.update();
        let open = app.world().resource::<OpenMap>();
        assert_eq!(
            open.map.stamps.len(),
            1,
            "redo did not put the stamp back"
        );
        assert_eq!(open.map.stamps[0].of, "break_table");
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
            .bounded_composition(
                "tile_floor",
                (1.0, 1.0, 1.0),
                &[("floor", "floor", (0.0, 0.0))],
            )
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
        let placements_before = app.world().resource::<OpenMap>().map.placements.len();
        assert!(
            placements_before > 0,
            "the fixture must hand-place something for this to mean anything"
        );
        assert!(
            app.world().resource::<OpenMap>().map.stamps.is_empty(),
            "nothing stamped yet"
        );

        // **One-shot, latched.** This used to press every frame with no `done` flag, which made the
        // test depend on Bevy's arbitrary order between this and the one-shot press that follows:
        // both are `.before(Phase::Act)` and mutually unordered, so whichever ran last decided
        // whether the chord was still down when the next key arrived. It passed until an unrelated
        // system registration reshuffled the schedule (2026-08-14).
        fn press_composed(
            mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
            mut done: bevy::prelude::Local<bool>,
        ) {
            if !*done {
                let b = emerge_mapper::keys::binding(emerge_mapper::keys::Action::GenerateComposed);
                keys.press(emerge_mapper::keys::MOD_KEYS[0]);
                keys.press(b.key);
                *done = true;
            }
        }
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                press_composed,
                emerge_mapper::keys::Phase::Act,
            ),
        );
        app.update();

        // **The generate proposes; it does not write.** See `editor::Proposal` and
        // `keys::Stance::Proposed` — apply-on-keypress is what Alvarez et al. 2018 found was losing
        // work. So the map must be untouched here, and the acceptance below is what lands it.
        assert!(
            app.world().resource::<OpenMap>().map.stamps.is_empty(),
            "the modified G must propose, not write — nothing may reach the map before Enter"
        );
        assert!(
            app.world()
                .resource::<emerge_mapper::editor::Proposal>()
                .0
                .is_some(),
            "and a proposal must be waiting, or the keypress did nothing at all"
        );

        // A fresh input, then Enter: `press_composed` holds its keys down, and a held modifier would
        // make `Enter` read as a chord nobody bound.
        fn accept(
            mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
            mut done: bevy::prelude::Local<bool>,
        ) {
            if !*done {
                keys.release_all();
                keys.press(
                    emerge_mapper::keys::binding(emerge_mapper::keys::Action::AcceptProposal).key,
                );
                *done = true;
            }
        }
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(accept, emerge_mapper::keys::Phase::Act),
        );
        app.update();

        let open = app.world().resource::<OpenMap>();
        let stamped = open.map.stamps.len();
        assert!(
            stamped > 0,
            "the modified G laid nothing — the composition source is unwired"
        );
        assert!(
            open.map.stamps.iter().all(|s| s.of.starts_with("tile_")),
            "every stamp names one of the fixture's compositions: {:?}",
            open
                .map
                .stamps
                .iter()
                .map(|s| s.of.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            open.map.placements.len(),
            placements_before,
            "a grammar over compositions writes references, never expanded rows"
        );

        // Closed under inversion, the same standard every other bulk edit here is held to.
        emerge_mapper::editor::undo_for_test(app.world_mut());
        app.update();
        assert!(
            app.world().resource::<OpenMap>().map.stamps.is_empty(),
            "undo left the generated stamps in the map"
        );
        emerge_mapper::editor::redo_for_test(app.world_mut());
        app.update();
        assert_eq!(
            app.world().resource::<OpenMap>().map.stamps.len(),
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
            .bounded_composition(
                "tile_floor",
                (1.0, 1.0, 1.0),
                &[("floor", "floor", (0.0, 0.0))],
            )
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
        // **One-shot, latched.** This used to press every frame with no `done` flag, which made the
        // test depend on Bevy's arbitrary order between this and the one-shot press that follows:
        // both are `.before(Phase::Act)` and mutually unordered, so whichever ran last decided
        // whether the chord was still down when the next key arrived. It passed until an unrelated
        // system registration reshuffled the schedule (2026-08-14).
        fn press_composed(
            mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
            mut done: bevy::prelude::Local<bool>,
        ) {
            if !*done {
                let b = emerge_mapper::keys::binding(emerge_mapper::keys::Action::GenerateComposed);
                keys.press(emerge_mapper::keys::MOD_KEYS[0]);
                keys.press(b.key);
                *done = true;
            }
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
        // **The wish, named — not a percentage the solver never reported.** `Solved::unmet` is a
        // weight and the enclosure wish is charged all-or-nothing, so it cannot say *how much* was
        // missed; the line used to print `ENCLOSURE_WISH` as the part that failed, which told an
        // author who got 99 cells of 100 that a quarter of the region had not closed.
        assert!(
            said.contains("enclosure"),
            "the shortfall must be said out loud: {said}"
        );
        assert!(
            !said.contains("could not close"),
            "and it must not claim a shortfall the solver never measured: {said}"
        );
        // **The arrangement is proposed, not written** — the shortfall is reported about a layout
        // the author has not accepted yet, which is the point of saying it before the door rather
        // than after. `editor::Proposal`.
        let waiting = app
            .world()
            .resource::<emerge_mapper::editor::Proposal>()
            .0
            .as_ref()
            .unwrap_or_else(|| panic!("an unmeetable wish must still produce a layout to look at"));
        assert!(
            !waiting.stamps.is_empty(),
            "and that layout must actually contain the arrangement"
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
                &[
                    ("table", "table", (0.0, 0.0)),
                    ("chair_north", "chair", (0.0, -1.0)),
                ],
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
                world.resource_scope(|world, mut open: bevy::prelude::Mut<OpenMap>| {
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                let mut compose = ComposeState {
                    armed: Some("break_table".to_owned()),
                    ..Default::default()
                };
                emerge_mapper::editor::stamp_here_for_test(
                    &mut project,
                    &mut open,
                    &mut state,
                    &mut compose,
                    (2.0, 2.0),
                );
                });
            });
        }
        for _ in 0..3 {
            app.update();
        }

        let stamp_id = {
            let open = app.world().resource::<OpenMap>();
            assert_eq!(open.map.stamps.len(), 1, "no stamp landed");
            open.map.stamps[0].id.clone()
        };

        // **One parent, and it owns the rows.** Counted rather than assumed: a parent per ROW would
        // also satisfy "a parent exists", and it is the thing that would silently make Delete take
        // one member.
        let instances: Vec<(bevy::prelude::Entity, usize)> = {
            let mut q = app.world_mut().query::<(
                bevy::prelude::Entity,
                &emerge_mapper::editor::StampInstance,
                &bevy::prelude::Children,
            )>();
            q.iter(app.world())
                .map(|(e, inst, kids)| {
                    assert_eq!(
                        inst.id, stamp_id,
                        "an instance naming a stamp the map does not have"
                    );
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
            let open = app.world().resource::<OpenMap>();
            let picture = app
                .world()
                .resource::<emerge_mapper::editor::StampPicture>();
            assert_eq!(
                picture.rows.len(),
                2,
                "the picture index must describe every drawn row"
            );
            assert_eq!(
                pick_subject(project, open, picture, (2.0, 1.0)),
                Some(Subject::Stamp(stamp_id.clone())),
                "a click on a member is a click on the instance"
            );
        }

        // Delete, through the call the click makes.
        {
            let world = app.world_mut();
            world.resource_scope(|world, _project: bevy::prelude::Mut<Project>| {
                world.resource_scope(|world, mut open: bevy::prelude::Mut<OpenMap>| {
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                emerge_mapper::editor::delete_stamp_for_test(&stamp_id, &mut open, &mut state);
                });
            });
        }
        for _ in 0..3 {
            app.update();
        }
        assert!(
            app.world().resource::<OpenMap>().map.stamps.is_empty(),
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
            let open = app.world().resource::<OpenMap>();
            assert_eq!(
                open.map.stamps.len(),
                1,
                "undo did not put the stamp back"
            );
            assert_eq!(open.map.stamps[0].id, stamp_id);
        }
        emerge_mapper::editor::redo_for_test(app.world_mut());
        app.update();
        assert!(
            app.world().resource::<OpenMap>().map.stamps.is_empty(),
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
                &[
                    ("table", "table", (0.0, 0.0)),
                    ("chair_north", "chair", (0.0, -1.0)),
                ],
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
                world.resource_scope(|world, mut open: bevy::prelude::Mut<OpenMap>| {
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                let mut compose = ComposeState {
                    armed: Some("break_table".to_owned()),
                    ..Default::default()
                };
                emerge_mapper::editor::stamp_here_for_test(
                    &mut project,
                    &mut open,
                    &mut state,
                    &mut compose,
                    (2.0, 2.0),
                );
                });
            });
        }
        for _ in 0..3 {
            app.update();
        }

        let id = app.world().resource::<OpenMap>().map.stamps[0].id.clone();
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
            world.resource_scope(|world, _project: bevy::prelude::Mut<Project>| {
                world.resource_scope(|world, mut open: bevy::prelude::Mut<OpenMap>| {
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                emerge_mapper::editor::move_stamp_for_test(
                    &id,
                    (7.0, 5.0),
                    &mut open,
                    &mut state,
                );
                });
            });
        }
        for _ in 0..3 {
            app.update();
        }

        assert_eq!(
            app.world().resource::<OpenMap>().map.stamps[0].at,
            (7.0, 5.0),
            "the move writes `Stamped::at`"
        );
        let after = rows_at(&app);
        assert_eq!(
            after.len(),
            2,
            "the instance must still own both rows after moving"
        );
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
        assert_eq!(
            app.world().resource::<OpenMap>().map.stamps[0].at,
            (2.0, 2.0)
        );
        assert_eq!(rows_at(&app), before, "undo puts every row back");
        emerge_mapper::editor::redo_for_test(app.world_mut());
        app.update();
        assert_eq!(
            app.world().resource::<OpenMap>().map.stamps[0].at,
            (7.0, 5.0)
        );
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
                &[
                    ("table", "table", (0.0, 0.0)),
                    ("chair_north", "chair", (0.0, -1.0)),
                ],
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
                world.resource_scope(|world, mut open: bevy::prelude::Mut<OpenMap>| {
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                let mut compose = ComposeState {
                    armed: Some("mess_corner".to_owned()),
                    ..Default::default()
                };
                emerge_mapper::editor::stamp_here_for_test(
                    &mut project,
                    &mut open,
                    &mut state,
                    &mut compose,
                    (4.0, 4.0),
                );
                });
            });
        }
        for _ in 0..3 {
            app.update();
        }

        let open = app.world().resource::<OpenMap>();
        assert_eq!(open.map.stamps.len(), 1, "the outer group stamped");
        assert_eq!(open.map.stamps[0].of, "mess_corner");
        assert!(
            open.map.placements.is_empty(),
            "nesting must not write expanded rows into the map — the map holds the reference"
        );
        // Two rows drawn THROUGH the nested reference is what proves it resolved rather than
        // merely parsed.
        let picture = app
            .world()
            .resource::<emerge_mapper::editor::StampPicture>();
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
                &[
                    ("floor", "floor", (0.0, 0.0)),
                    ("wall", "wall", (0.0, -0.4)),
                ],
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
        let strip = app
            .world()
            .resource::<emerge_mapper::compose::StagedCarousel>();
        assert_eq!(
            strip.0.slots.len(),
            3,
            "the strip did not stand every neighbour up"
        );
        assert_eq!(
            strip.0.focal().map(|s| s.index),
            Some(0),
            "the focal group is the selected one"
        );
        assert!(strip.0.tallest > 0.0, "a strip of no height frames nothing");

        // Four rows across three groups — so this counts the whole strip standing, not one group.
        let staged = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<emerge_mapper::compose::StagedMember>>()
            .iter(app.world())
            .count();
        assert_eq!(
            staged, 4,
            "every member of every visible group has to stand up"
        );

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
        assert_eq!(
            ids, after,
            "the sheet was rebuilt with nothing having changed"
        );

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
        fn press_step(
            mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
        ) {
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
        let strip = app
            .world()
            .resource::<emerge_mapper::compose::StagedCarousel>();
        assert_eq!(
            strip.0.focal().map(|s| s.index),
            Some(1),
            "stepping did not move the focus"
        );
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
        let strip = app
            .world()
            .resource::<emerge_mapper::compose::StagedCarousel>()
            .0
            .clone();
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

    let root = Fixture::new("namebox")
        .descriptor("floor", "alpha")
        .build("m");
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
        let open = app
            .world()
            .get_resource::<emerge_mapper::project::OpenMap>()
            .unwrap_or_else(|| panic!("no open map"));
        assert_eq!(
            open
                .map
                .placements
                .first()
                .map(|p| p.descriptor.as_str()),
            Some("floor"),
            "the map must place the OTHER piece, or this proves nothing"
        );
        app.world()
            .resource::<emerge_mapper::project::Project>()
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
        let open = app
            .world()
            .get_resource::<emerge_mapper::project::OpenMap>()
            .unwrap_or_else(|| panic!("no open map"));
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("no project"));
        let state = app
            .world()
            .get_resource::<emerge_mapper::editor::EditorState>()
            .unwrap_or_else(|| panic!("no editor state"));
        edit_subject(project, open, state, under)
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
        (
            KeyCode::KeyB,
            bevy::input::keyboard::Key::Character("b".into()),
        ),
    ] {
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
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
    assert!(
        said.contains("crate@7"),
        "the readout must name the piece: `{said}`"
    );
    assert!(
        said.contains(&emerge_mapper::keys::chord_text(
            emerge_mapper::keys::binding(emerge_mapper::keys::Action::EditTile)
        )),
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

    let root = Fixture::new("overui")
        .descriptor("floor", "alpha")
        .build("m");
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
            .unwrap_or_else(|| {
                panic!("no laid-out interactive UI node — this test would prove nothing")
            })
    };

    for scale in [1.0_f32, 2.0] {
        // The pointer is logical, the rect is physical: the centre in logical pixels is the physical
        // centre divided by the factor. Getting this backwards is the bug being pinned.
        let logical_centre = centre / scale;
        let nodes: Vec<(ComputedNode, UiGlobalTransform)> = {
            let mut q = app
                .world_mut()
                .query_filtered::<(&ComputedNode, &UiGlobalTransform), bevy::prelude::With<bevy::picking::hover::Hovered>>();
            q.iter(app.world())
                .map(|(n, tf)| (n.clone(), *tf))
                .collect()
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
            !emerge_mapper::view::over_ui(
                Some(Vec2::new(-5000.0, -5000.0)),
                scale,
                borrowed.iter().copied()
            ),
            "a pointer nowhere near a panel must read as the world (scale {scale})"
        );
    }
    // No cursor is not "over the world" — it is no answer, and every other reader treats it so.
    assert!(!emerge_mapper::view::over_ui(None, 1.0, [].into_iter()));
}

/// **Every tab carries a badge, and it boots silent.**
///
/// The badge is the stale count's own text child, in its own colour, because `style_tabs` owns
/// every `TabLabel`'s `TextColor` per frame — a `DANGER` written into the label was stomped a
/// frame later, which is how "the one word here allowed to shout" rendered in the tab's ordinary
/// grey. The label itself must no longer carry the count: one fact, one node.
#[test]
fn the_tab_badge_is_its_own_node_and_boots_empty() {
    let root = Fixture::new("badge").descriptor("floor", "alpha").build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let badges: Vec<String> = {
        let mut q = app
            .world_mut()
            .query_filtered::<&bevy::ui::widget::Text, With<emerge_mapper::tiles::TabBadge>>();
        q.iter(app.world()).map(|t| t.0.clone()).collect()
    };
    // **One badge per tab of the door this app opened**, not per `Mode`. The strip belongs to the
    // door now (`Door::tabs`), so `Mode::ALL` counted a five-tab strip nothing draws — the fixed
    // count outlived the shape it described. Derived from the same `Mode::default()`
    // `build_headless` passes, so the two cannot disagree about which door is standing.
    let door = emerge_mapper::tiles::Door::showing(emerge_mapper::tiles::Mode::default());
    let tabs = door.tabs().len();
    assert_eq!(badges.len(), tabs, "one badge per tab, found {}", badges.len());
    assert!(
        badges.iter().all(String::is_empty),
        "no rig has been measured, so every badge must be silent: {badges:?}"
    );

    let mut labels = app
        .world_mut()
        .query_filtered::<&bevy::ui::widget::Text, With<emerge_mapper::tiles::TabLabel>>();
    assert!(
        labels.iter(app.world()).all(|t| !t.0.contains("STALE")),
        "the count lives on the badge now — a label carrying STALE is the stomped-colour bug back"
    );
}

/// **Every pane that clips can scroll.**
///
/// `bevy_ui_widgets`' wheel handler only serves `With<ScrollArea>`, so a node with
/// `overflow-y: scroll` and no `ScrollArea` clips its content and then refuses the wheel — the
/// overflow is unreachable by any input. The Compose body shipped exactly that: a hand copy of
/// `chrome::scroll_list` with every field except the one that makes it scrollable, on the longest
/// generated pane in the editor. This pins the CLASS: any future pane that clips without scrolling
/// fails here by construction, whichever tab it lands on.
#[test]
fn every_pane_that_clips_can_scroll() {
    use bevy::ui::OverflowAxis;
    use bevy::ui_widgets::ScrollArea;

    let root = Fixture::new("scrolls").descriptor("floor", "alpha").build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let mut clipping = 0;
    let mut q = app.world_mut().query::<(Entity, &Node, Option<&ScrollArea>)>();
    for (entity, node, area) in q.iter(app.world()) {
        if node.overflow.y != OverflowAxis::Scroll {
            continue;
        }
        clipping += 1;
        assert!(
            area.is_some(),
            "{entity} clips its overflow (overflow-y: scroll) but carries no ScrollArea, so the \
             wheel cannot reach what it clipped — spawn it through `chrome::scroll_list`"
        );
    }
    // Every tab spawns its panels (hidden) at Startup, so the map palette, both tiles panes, both
    // anim panes and the compose body are all present. A count collapse means the query stopped
    // seeing what it claims to check, which would make the assertion above vacuous.
    assert!(
        clipping >= 4,
        "only {clipping} clipping panes found — the scroll panes are not being seen, so this test \
         proves nothing"
    );
}

/// **`Z` and `C` reach the set in hand, not the brush.**
/// **`R` and `T` reach the set in hand, before the brush and before the piece under the cursor.**
///
/// The turn arithmetic is pinned by `a_turned_set_lands_where_a_turned_stamp_would`; what was not
/// pinned is that the *binding* gets there. `CloneDrag::held` is private, so this goes through
/// `hold_set_for_test` and then drives the real key message — the brush's own yaw is asserted
/// unchanged, because "turned something" and "turned the right thing" are different claims.
///
/// It read `Z`/`C` until those keys retired into the turn cluster (2026-08-14). The set keeps its
/// place at the head of that cluster's order: the click stamps the set, so turning anything else
/// would be aiming something that is not going anywhere.
#[test]
fn the_turn_keys_reach_the_set_in_hand_and_leave_the_brush_alone() {
    let root = Fixture::new("turnset")
        .descriptor("floor", "alpha")
        .build("m");
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

    // `T` — the real message, so this cannot pass with the binding removed.
    app.world_mut()
        .write_message(bevy::input::keyboard::KeyboardInput {
            key_code: emerge_mapper::keys::binding(emerge_mapper::keys::Action::TurnPieceRight).key,
            logical_key: bevy::input::keyboard::Key::Character("t".into()),
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
    let _open = app
        .world()
        .get_resource::<emerge_mapper::project::OpenMap>()
        .unwrap_or_else(|| panic!("no open map"));
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

/// **The grid starts on the tile, and the tile is a rung you can actually land on.**
///
/// This used to pin `GridSpacing`, a *drawn* grid cycled by `J` through `[0.5, 1.0, 2.0, 4.0]` m
/// while the lattice a piece landed on was chosen by a held modifier. Two mechanisms, one key on the
/// wrong one, and two of its four steps were lines no piece could ever sit on.
///
/// There is one ladder now — `editor::Rung` — so what this pins is that the editor opens on the
/// coarsest rung of it, which is the kit's module and what an author counts in.
#[test]
fn the_grid_starts_on_the_tile_rung() {
    let mut app = headless();
    app.add_plugins(emerge_mapper::editor::EditorPlugin);
    let rung = app
        .world()
        .get_resource::<emerge_mapper::editor::Rung>()
        .unwrap_or_else(|| panic!("EditorPlugin does not register Rung"));
    assert_eq!(
        rung.0,
        emerge_core::grid::SnapLevel::Tile,
        "the editor opens on the tile rung; a square is meant to be one kit tile"
    );
    // The property the old assertion was really about: whatever the ladder's top rung is, it must be
    // one a piece can land on. It is, by construction — `snap_level` and `draw_map_grid` now take the
    // same `SnapLevel` — and this says so where a reader of the test will see it.
    assert_eq!(
        rung.0
            .pitch(emerge_core::kits::Lattice::default().snap_divisor),
        emerge_core::grid::TILE,
        "the coarsest rung is the tile itself"
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
    let open = app
        .world()
        .get_resource::<emerge_mapper::project::OpenMap>()
        .unwrap_or_else(|| panic!("no open map"));
    assert_eq!(
        project.library.descriptors.len(),
        2,
        "two descriptors were written"
    );
    assert_eq!(open.map.placements.len(), 1, "one placement was written");
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
    // Cloned rather than held: `keep_as_group` wants both resources at once, and two live borrows
    // of one world is what `resource_scope` exists to avoid. The map is only read here.
    let open = world.resource::<emerge_mapper::project::OpenMap>().map.clone();
    let open = emerge_mapper::project::OpenMap {
        map: open,
        map_path: std::path::PathBuf::from("m.map.ron"),
        dirty: false,
    };
    let world = app.world_mut();
    let mut project = world.resource_mut::<emerge_mapper::project::Project>();
    assert!(
        project.compositions.compositions.is_empty(),
        "the fixture writes no groups"
    );

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
    let kept = emerge_mapper::editor::keep_as_group(&mut project, &open, &set, "Mess Table", false)
        .unwrap_or_else(|e| panic!("the composition must be kept: {e}"));
    assert_eq!(
        kept,
        emerge_mapper::editor::Kept::Made("mess_table".to_owned()),
        "a name nothing holds is made outright, and forced into snake_case"
    );
    assert_eq!(
        project.compositions.compositions.len(),
        1,
        "it was adopted in memory"
    );

    // And it is on disk, parseable, with the members the set held.
    let path = root.join("assets/emerge/compositions.ron");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let reread = emerge_core::composition::Compositions::parse(&text)
        .unwrap_or_else(|e| panic!("what was written must parse: {e}"));
    let c = reread
        .compositions
        .first()
        .unwrap_or_else(|| panic!("no group on disk"));
    assert_eq!(c.id, "mess_table");
    let ids: Vec<&str> = c.members.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["lamp", "table"], "members are stored sorted by id");

    // **Capturing over the name asks first, and writes nothing until it is answered.**
    //
    // It used to refuse outright. That made compositions append-only the moment the Compose tab
    // stopped being able to edit one — and made the send-back verb's own advice, "edit the group
    // first", impossible to follow.
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    let asked = emerge_mapper::editor::keep_as_group(&mut project, &open, &set, "mess_table", false)
        .unwrap_or_else(|e| panic!("capturing over a name must ask, not refuse: {e}"));
    assert_eq!(
        asked,
        emerge_mapper::editor::Kept::WouldReplace {
            id: "mess_table".to_owned(),
            stamps: 0
        },
        "the first press asks"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap_or_default(),
        before,
        "and writes nothing"
    );

    // The second press redefines it in place — same id, so no stamp anywhere is stranded.
    let done = emerge_mapper::editor::keep_as_group(&mut project, &open, &set, "mess_table", true)
        .unwrap_or_else(|e| panic!("the confirmed replace must land: {e}"));
    assert_eq!(
        done,
        emerge_mapper::editor::Kept::Replaced {
            id: "mess_table".to_owned(),
            stamps: 0
        }
    );
    assert_eq!(
        project.compositions.compositions.len(),
        1,
        "replacing redefines the one that was there rather than adding a second"
    );
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let reread = emerge_core::composition::Compositions::parse(&text)
        .unwrap_or_else(|e| panic!("what was written must parse: {e}"));
    assert_eq!(
        reread.compositions.len(),
        1,
        "and one composition reached disk, not two"
    );
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
    *app.world_mut().resource_mut::<emerge_mapper::tiles::Mode>() = emerge_mapper::tiles::Mode::Map;
    app.update();
    assert_eq!(
        app.world().resource::<emerge_mapper::keys::Live>().0,
        emerge_mapper::keys::Context::Map,
        "with no field open the tab's verbs are live"
    );

    app.world_mut()
        .resource_mut::<emerge_mapper::editor::EditorState>()
        .grouping = Some(String::new());
    app.update();
    assert_eq!(
        app.world().resource::<emerge_mapper::keys::Live>().0,
        emerge_mapper::keys::Context::Typing,
        "while a name is being typed the keyboard belongs to the text, or every letter is a verb"
    );

    // And it hands the keyboard back, or the tab is dead after one capture.
    app.world_mut()
        .resource_mut::<emerge_mapper::editor::EditorState>()
        .grouping = None;
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
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("tile_grows")
        // 1.21 m reaches 0.605 from a centred anchor and one cell only reaches 0.5, so this needs a
        // second cell — and 0.81 across does not, which is what makes the assertion below specific.
        .sized_descriptor("pallet", "alpha", 0.81, 1.21)
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, key: KeyCode) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>,
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
    }

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
    let whole =
        |v: f32| (v / emerge_core::grid::TILE - (v / emerge_core::grid::TILE).round()).abs();
    assert!(
        whole(after.0) < 1e-4 && whole(after.2) < 1e-4,
        "and it grows in whole tiles, never a fraction of one: {after:?}"
    );

    // **And it is legible as a group the solver cannot place** — `from_compositions` skips anything
    // that is not one cell, and finding that out from a generate that quietly never uses it is the
    // bad version.
    //
    // **Asserted on the tile, not on the problem log.** This used to demand `status.has_problem()`,
    // and the author's own log showed what that cost: `refit` raised a fresh sticky problem on every
    // size change, so one continuous nudge left fifteen — `2 x 3`, `2 x 4`, `3 x 4`, `4 x 4` — none
    // folding, because `Status` folds consecutive *identical* lines and each carried a different
    // size. They then outlived the tile, and the panel read `MEMBERS: nothing yet` beneath twelve
    // warnings about a 4 x 4. The fact is a property, so the panel states it beside the size and this
    // asserts the property.
    assert!(
        !emerge_mapper::build::is_one_cell(after),
        "a grown tile must read as one the solver cannot place: {after:?}"
    );

    // **And the panel says so, beside the size.** The point of moving this off the problem log is
    // that it is visible whenever it is true — so the test that used to check an alert fired checks
    // the line is on screen. `MinimalPlugins` draws nothing, but the UI tree is real and its `Text`
    // is what a reader would read.
    app.update();
    let mut texts = app.world_mut().query::<&bevy::prelude::Text>();
    let shown: Vec<String> = texts.iter(app.world()).map(|t| t.0.clone()).collect();
    assert!(
        shown.iter().any(|t| t.contains("hand-stamped")),
        "the TILE block must qualify the size with what it costs. Saw: {shown:?}"
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
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("tile_align")
        // 0.2 m across in a 1 m tile: flush left is -0.4, which is not a multiple of either rung.
        // **Square, and that is deliberate.** Left/right walk the members now, so the only bare
        // nudge axis is Z — and a piece spanning the tile on Z grows the envelope the moment it is
        // nudged, which is a fact about that piece rather than about the pair being tested here.
        .sized_descriptor("panel", "alpha", 0.2, 0.2)
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>,
                      mut done: bevy::prelude::Local<bool>| {
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

    once(&mut app, vec![binding(Action::BuildArm).key]);
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    assert_eq!(at(&app), (0.0, 0.0), "brought in centred");

    // Bare arrow at depth 1: one ladder stop, not a flush — `J` first, because at the top of the
    // ladder the only stops ARE centre and flush, and this test is about the bare/shifted pair
    // being two different verbs. **Down, because left/right walk the members now** — see
    // `keys::Action::MemberPrev`.
    once(&mut app, vec![binding(Action::BuildRung).key]);
    once(&mut app, vec![binding(Action::BuildBack).key]);
    let nudged = at(&app);
    assert_ne!(nudged, (0.0, 0.0), "the unshifted arrow must still nudge");
    assert!(
        nudged.1.abs() < 0.4,
        "one stop at depth 1 is a third of the span, not the edge — got {nudged:?}"
    );

    // Shifted: straight to the edge, wherever it was.
    once(
        &mut app,
        vec![KeyCode::ShiftLeft, binding(Action::AlignLeft).key],
    );
    let flush = at(&app);
    assert!(
        (flush.0 + 0.4).abs() < 1e-4,
        "Shift+left must put a 0.2 m panel flush at -0.4 in a 1 m tile — got {flush:?}"
    );
    assert!(
        (flush.1 - nudged.1).abs() < 1e-6,
        "and it must not move the axis the nudge moved: {flush:?} from {nudged:?}"
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
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("tile_undo")
        .descriptor("floor", "alpha")
        .descriptor("wall", "alpha")
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>,
                      mut done: bevy::prelude::Local<bool>| {
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
    once(
        &mut app,
        vec![KeyCode::SuperLeft, binding(Action::UndoBuild).key],
    );
    let one = members(&app);
    assert_eq!(
        one.len(),
        1,
        "one undo takes the second mesh back out: {one:?}"
    );
    assert_eq!(
        one[0], two[0],
        "and it is the FIRST that survives, not whichever sorted first"
    );

    once(
        &mut app,
        vec![KeyCode::SuperLeft, binding(Action::UndoBuild).key],
    );
    assert!(members(&app).is_empty(), "the second undo empties the tile");

    // And forward again, because a history that only goes one way is half a history.
    once(
        &mut app,
        vec![
            KeyCode::SuperLeft,
            KeyCode::ShiftLeft,
            binding(Action::RedoBuild).key,
        ],
    );
    assert_eq!(members(&app), one, "redo puts the first mesh back");
    once(
        &mut app,
        vec![
            KeyCode::SuperLeft,
            KeyCode::ShiftLeft,
            binding(Action::RedoBuild).key,
        ],
    );
    assert_eq!(members(&app), two, "and the second");

    // **The envelope travels with it.** `refit` runs before the recorder, so a resize is part of the
    // step that caused it rather than a separate thing to undo — otherwise every drop would cost two
    // presses to take back.
    once(
        &mut app,
        vec![KeyCode::SuperLeft, binding(Action::UndoBuild).key],
    );
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
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("tile_round_trip")
        .descriptor("floor", "alpha")
        .descriptor("wall", "alpha")
        .slot_token("wall-fixture")
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>,
                      mut done: bevy::prelude::Local<bool>| {
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
    once(&mut app, vec![binding(Action::BuildBack).key]);
    once(&mut app, vec![binding(Action::BuildUp).key]);
    once(
        &mut app,
        vec![KeyCode::ShiftLeft, binding(Action::BuildSlot).key],
    );

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
            app.world()
                .resource::<emerge_mapper::tiles::ImportState>()
                .status
                .note_text()
        );
        open.id.clone()
    };

    // `Cmd+S` — Global, and the handler asks which tab is live rather than there being a second key.
    once(
        &mut app,
        vec![KeyCode::SuperLeft, binding(Action::Save).key],
    );
    // Saving a tile the editor named raises the name prompt (2026-08-15): a provisional
    // `<kit>/tile_n` must not reach the kit, because the KIT list is where it would be read back.
    name_the_tile(&mut app, "tile_1");
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
    let reopened = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the saved project must reopen: {e}"));
    let project = reopened
        .world()
        .resource::<emerge_mapper::project::Project>();
    let saved = project
        .compositions
        .compositions
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| {
            panic!(
                "`{id}` must be in compositions.ron after a save; found {:?}",
                project
                    .compositions
                    .compositions
                    .iter()
                    .map(|c| &c.id)
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        saved.members.len(),
        3,
        "every member must survive the round trip"
    );
    let holes = saved
        .members
        .iter()
        .filter(|m| matches!(m.body, emerge_core::composition::Body::Slot { .. }))
        .count();
    assert_eq!(
        holes, 1,
        "the hole is a member like any other, and must come back as one"
    );

    // And it is a tile the map can actually place: cell-sized in plan, or `from_compositions`
    // refuses it by name and the whole authoring loop produces something the solver cannot use.
    let emerge_core::composition::Envelope::Bounded { size } = saved.envelope else {
        panic!("a tile claims a tile");
    };
    // **Whole cells, and as many as its contents need.** Not one cell: the fixture's pieces are 1 m
    // cubes and one of them was moved a rung off centre, so two is the honest answer and the tile
    // resized to say it. What must hold is that the envelope is a whole number of tiles — a
    // fractional one is placeable at no grid spacing at all.
    let whole =
        |v: f32| (v / emerge_core::grid::TILE - (v / emerge_core::grid::TILE).round()).abs();
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
    let open = emerge_mapper::project::OpenMap::open(&project, "m")
        .unwrap_or_else(|e| panic!("{e}"));
    let out = emerge_core::composition::expand(
        &open.map,
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
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("tiles_arrows")
        .descriptor("aaa_floor", "alpha")
        .descriptor("zzz_wall", "alpha")
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>,
                      mut done: bevy::prelude::Local<bool>| {
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
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("tiles_unimported")
        .descriptor("wall", "alpha")
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
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
                move |mut keys: ResMut<ButtonInput<KeyCode>>,
                      mut done: bevy::prelude::Local<bool>| {
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
    use bevy::input::ButtonInput;
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
    use bevy::ui::Display;
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("tiles_banner")
        .descriptor("wall", "alpha")
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    // **One-shot, because a held key does not re-arm `just_pressed`.** Release everything first, so
    // the press this frame is a fresh edge rather than a key the previous system left down.
    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>,
                      mut done: bevy::prelude::Local<bool>| {
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

    // **Shift+Enter drops a hole, and the fixture declares no `slot` tokens** — so this is a refusal
    // by construction rather than by contrivance, and it is the one a real author meets first on a
    // project whose vocabulary has not grown a slot axis yet. A bare `Enter` would *succeed*:
    // `ImportState::editing` falls back to the selected candidate, so a piece is always in hand.
    once(
        &mut app,
        vec![KeyCode::ShiftLeft, binding(Action::BuildSlot).key],
    );
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
            .find(|(_, b)| b.0.contains(&want))
            .map(|(n, _)| n.display)
            .unwrap_or_else(|| panic!("the shared panel must carry a banner for {}", want.label()))
    };

    assert_eq!(
        banner(&mut app, emerge_mapper::tiles::Mode::Tiles),
        Display::Flex,
        "a refusal the Tiles tab raised must be on the Tiles tab's banner"
    );

    // **The leak this guarded against cannot happen any more.** The second half of this test used
    // to switch to the Meshes tab and assert the Tiles banner hid, because the two shared one panel
    // and a stale line about work the author had left behind is a lie on screen. A door shows one
    // thing for the life of the process, so there is no switch and nothing to leak across — what is
    // left worth asserting is that the banner belongs to the door showing it.
    assert!(
        banner(&mut app, emerge_mapper::tiles::Mode::Tiles) == Display::Flex,
        "the banner stays up on the door that raised it"
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

    // Both pieces sized under a cell: the arrows walk the span between centre and flush, and a
    // full-cell piece has no span to walk — its arrows answer with a note instead of movement.
    let root = Fixture::new("build_mode")
        .sized_descriptor("wall", "alpha", 0.2, 0.2)
        .sized_descriptor("floor", "beta", 0.2, 0.2)
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
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
                move |mut keys: bevy::prelude::ResMut<
                    bevy::input::ButtonInput<bevy::prelude::KeyCode>,
                >,
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

    let _key = |a| emerge_mapper::keys::binding(a).key;

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
    assert_eq!(
        (size.0, size.2),
        (emerge_core::grid::TILE, emerge_core::grid::TILE)
    );
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
    assert_eq!(
        placed(&app),
        (0.0, 0.0),
        "a brought-in mesh is centred, bottom on the floor"
    );

    // One stop, one axis — never the diagonal. At the top of the ladder that stop is flush.
    before(&mut app, key(emerge_mapper::keys::Action::BuildBack));
    let moved = placed(&app);
    assert_ne!(
        moved,
        (0.0, 0.0),
        "an arrow must move the member it is focused on"
    );
    assert!(
        (moved.0 != 0.0) ^ (moved.1 != 0.0),
        "exactly one plan axis may move — got {moved:?}"
    );

    // **And the tab does not turn the camera.** It was turned square-on for one commit to make the
    // arrows read straight, which traded the framing the author builds in for a key mapping.
    let rig = app.world().resource::<emerge_mapper::view::Rig>();
    assert_eq!(
        rig.yaw, 0.0,
        "arriving on the Tiles tab must not spin the view"
    );

    // **And the panel keeps up.** This is the half that shipped broken once: the tab changed, the
    // status line said so, and the detail pane went on showing the mesh inspector — which reads as
    // the key having done nothing.
    app.update();
    let mut texts = app.world_mut().query::<&bevy::prelude::Text>();
    let shown: Vec<String> = texts.iter(app.world()).map(|t| t.0.clone()).collect();
    assert!(
        shown.iter().any(|t| t == "TILES"),
        "the strip must name the tab. Saw: {shown:?}"
    );
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
    //
    // **The focused member, not a cursor.** There is no cursor in this tab: the arrows move the
    // member, so the member is the position. The pane used to print a `Build::at` written only by
    // the nudge — stale after every drop, removal and undo, and measured in a different frame from
    // the one its readers used.
    let build = app.world().resource::<Build>();
    let want = match build.open.as_ref().and_then(|c| c.members.get(build.focus)) {
        Some(m) => format!("focus ({:+.3}, {:+.3}) at {:.3} m", m.at.0, m.at.1, m.lift),
        None => "empty — the next drop lands centred".to_owned(),
    };
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
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
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
    fn to_tiles(_done: bevy::prelude::Local<bool>, mut _k: Keys) {
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
    let comp = build
        .open
        .as_ref()
        .unwrap_or_else(|| panic!("a tile is open"));
    let ids: Vec<&str> = comp.members.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["floor", "wall"],
        "both drop, and the list stays sorted"
    );
    assert_eq!(
        build.focus, 1,
        "the focus must be the member just dropped — `R` and Delete act on it, and here that is \
         `wall`, not the `floor` that happens to sort first"
    );

    // **Members, not the ghost.** `build::draw_tile` marks the preview with `StagedTile` *and*
    // `Ghost` on purpose — `editor::fade_ghost` needs the second and the rebuild needs the first —
    // so counting `StagedTile` alone counts the promise as well as the thing.
    //
    // It did not matter until a drop started leaving you holding the next piece (the author's
    // 2026-08-12 report: the arrows went dead after `Enter`), which is when a ghost first stood on
    // this stage at the moment this test looked. The count was right for the wrong reason before;
    // asking `Without<Ghost>` is the question the assertion's own sentence is about.
    let mut staged = app
        .world_mut()
        .query_filtered::<&StagedTile, bevy::prelude::Without<emerge_mapper::editor::Ghost>>();
    assert_eq!(
        staged.iter(app.world()).count(),
        2,
        "both members must stand up on the stage — a tile that is only a list in a panel is the \
         feedback half of the loop missing"
    );
}

/// **The Map tab arms a brush from the keyboard**, which it could not do at all.
///
/// `EditorState::brush` had exactly one writer — `editor::on_row_click`, a mouse observer — so on
/// the tab the code itself calls *"the job"*, choosing what to place required the pointer. The
/// author's brief for this editor was the opposite: *"this should be done by the keyboard, as key
/// strokes are faster."*
///
/// Driven through the same one-shot press helper the tile tests use, because a held key would
/// re-fire every frame and `walk_palette` repeats at [`emerge_mapper::keys::REPEAT_SECS`].
#[test]
fn the_map_palette_walks_from_the_keyboard() {
    use emerge_mapper::editor::EditorState;

    let root = Fixture::new("palette_walk")
        .descriptor("wall", "alpha")
        .descriptor("floor", "beta")
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let step = |app: &mut bevy::prelude::App, key: bevy::prelude::KeyCode| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                move |mut keys: bevy::prelude::ResMut<
                    bevy::input::ButtonInput<bevy::prelude::KeyCode>,
                >,
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

    let brush = |app: &bevy::prelude::App| app.world().resource::<EditorState>().brush;
    let before = brush(&app);

    let key = |a| emerge_mapper::keys::binding(a).key;
    step(&mut app, key(emerge_mapper::keys::Action::PaletteNext));
    let after = brush(&app);

    assert_ne!(
        after, before,
        "an arrow on the Map must move the armed brush — it was {before:?} and stayed there"
    );
    assert!(
        after.is_some(),
        "walking the palette arms something, never nothing"
    );

    // **And back**, so the pair is a walk rather than a one-way ratchet. A fresh `ButtonInput` is
    // not needed here because `step` releases everything before it presses.
    step(&mut app, key(emerge_mapper::keys::Action::PalettePrev));
    assert_eq!(
        brush(&app),
        before,
        "up must undo what down did — the two keys walk one list"
    );
}

/// **The door's other half: `Esc` throws the layout away and the map is untouched.**
///
/// The half that is actually about safety. Alvarez et al. 2018 (`10.1145/3235765.3235815`) added a
/// two-step commit to the Evolutionary Dungeon Designer because apply-on-click was *"occasionally
/// causing work loss due to accidental replacements"* — and a generate here can clear every unpinned
/// row on the map, which is the largest single act this editor has.
///
/// Discarding must record **no undo entry**: nothing was written, so there is nothing to take back,
/// and an undo step that restores a state you were already in is one an author has to press twice.
#[test]
fn a_discarded_layout_leaves_the_map_and_the_undo_stack_alone() {
    use emerge_mapper::editor::{EditorState, Proposal};
    use emerge_mapper::project::OpenMap;

    let root = Fixture::new("gen-discard")
        .descriptor("floor", "alpha")
        .descriptor("rug", "alpha")
        .bounded_composition(
            "tile_floor",
            (1.0, 1.0, 1.0),
            &[("floor", "floor", (0.0, 0.0))],
        )
        .bounded_composition("tile_rug", (1.0, 1.0, 1.0), &[("rug", "rug", (0.0, 0.0))])
        .place("rug", (0.5, 0.5))
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }
    let placements_before = app.world().resource::<OpenMap>().map.placements.len();
    let undo_before = app.world().resource::<EditorState>().undo_depth();

    // One-shot, latched — the sibling in `compose::` carries the full story: an unlatched press
    // runs every frame, is unordered against the `discard` system below, and whichever runs last
    // decides whether `Esc` arrives bare or refused under a still-held modifier.
    fn press_composed(
        mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
        mut done: bevy::prelude::Local<bool>,
    ) {
        if !*done {
            keys.press(emerge_mapper::keys::MOD_KEYS[0]);
            keys.press(
                emerge_mapper::keys::binding(emerge_mapper::keys::Action::GenerateComposed).key,
            );
            *done = true;
        }
    }
    app.add_systems(
        bevy::prelude::Update,
        bevy::prelude::IntoScheduleConfigs::before(press_composed, emerge_mapper::keys::Phase::Act),
    );
    app.update();
    assert!(
        app.world().resource::<Proposal>().0.is_some(),
        "a proposal must be waiting before there is anything to discard"
    );

    fn discard(
        mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
        mut done: bevy::prelude::Local<bool>,
    ) {
        if !*done {
            keys.release_all();
            keys.press(emerge_mapper::keys::binding(emerge_mapper::keys::Action::Cancel).key);
            *done = true;
        }
    }
    app.add_systems(
        bevy::prelude::Update,
        bevy::prelude::IntoScheduleConfigs::before(discard, emerge_mapper::keys::Phase::Act),
    );
    app.update();

    assert!(
        app.world().resource::<Proposal>().0.is_none(),
        "Esc must take the proposal away"
    );
    let open = app.world().resource::<OpenMap>();
    assert!(
        open.map.stamps.is_empty(),
        "and nothing may have reached the map: {:?}",
        open
            .map
            .stamps
            .iter()
            .map(|s| s.of.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        open.map.placements.len(),
        placements_before,
        "the author's own rows are untouched"
    );
    assert_eq!(
        app.world().resource::<EditorState>().undo_depth(),
        undo_before,
        "discarding writes nothing, so it must record nothing to undo"
    );
}

/// **A derivation refuses a token the project has not declared, and names it.**
///
/// FVS-R-26's commit door. `edge` is a closed vocabulary axis and `vocab.rs` says why: the tokens are
/// matched by equality, so *"a typo does not read as a wrong token, it reads as a token that matches
/// nothing."* An empty or narrow axis is *"the honest reading of 'this project has not decided what
/// its tiles present'"* — so a derivation that quietly widened it would be taking a schema decision
/// on the author's behalf. The refusal is the design (author's call, 2026-08-12).
///
/// The fixture ships one token, `wall`, which is deliberately not what the derivation names.
#[test]
fn derived_edges_refuse_an_undeclared_token_and_say_which() {
    use emerge_mapper::project::Project;
    use emerge_mapper::tiles::{DerivedEdges, ImportState};

    let root = Fixture::new("derive-refuse")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    // Stage a derivation directly: `B` needs a real GLB on disk to rasterise, and what is under test
    // here is the door, not the rasteriser — `emerge-core` owns that and tests it.
    let id = app
        .world()
        .resource::<Project>()
        .library
        .descriptors
        .first()
        .map(|d| d.id.clone())
        .unwrap_or_else(|| panic!("the fixture must carry a descriptor"));
    // The stance is per-tab, so the tab has to be the one the door belongs to.
    *app.world_mut().resource_mut::<emerge_mapper::tiles::Mode>() =
        emerge_mapper::tiles::Mode::Meshes;
    app.world_mut()
        .resource_mut::<ImportState>()
        .selected_library_id = Some(id.clone());
    app.world_mut()
        .insert_resource(DerivedEdges(Some(emerge_mapper::tiles::Derived {
            id: id.clone(),
            cells: vec![
                ((0, 0, 0), emerge_core::adjacency::EDGE_SOLID),
                ((1, 0, 0), emerge_core::adjacency::EDGE_OPEN),
            ],
        })));
    app.update();

    fn accept(
        mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
        mut done: bevy::prelude::Local<bool>,
    ) {
        if !*done {
            keys.release_all();
            keys.press(emerge_mapper::keys::binding(emerge_mapper::keys::Action::AcceptEdges).key);
            *done = true;
        }
    }
    app.add_systems(
        bevy::prelude::Update,
        bevy::prelude::IntoScheduleConfigs::before(accept, emerge_mapper::keys::Phase::Act),
    );
    app.update();

    let state = app.world().resource::<ImportState>();
    let said = state.status.problem_text();
    // **The vocabulary's own words, not a second set.** `Vocabularies::masks` already refuses an
    // undeclared token by name and prints the axis as it stands; the door surfaces that message
    // rather than composing a rival one. Asserted on the token and the axis so the test fails if the
    // door ever starts writing its own.
    assert!(
        said.contains(emerge_core::adjacency::EDGE_SOLID),
        "the refusal must name the undeclared token, and it reads `{said}`"
    );
    assert!(
        said.contains("edge"),
        "and name the axis it belongs to: `{said}`"
    );
    assert!(
        said.contains("vocab.ron"),
        "and say where to declare it: `{said}`"
    );
    // Nothing may reach the lattice: refusing after a partial write would leave a piece carrying
    // tokens the project cannot load.
    let project = app.world().resource::<Project>();
    let wrote = project
        .measured
        .descriptors
        .iter()
        .find(|d| d.id == id)
        .and_then(|d| d.subgrid.as_ref())
        .is_some_and(|g| g.cells.iter().any(|c| c.edge.is_some()));
    assert!(!wrote, "a refused derivation must write nothing at all");
}

/// **And with the tokens declared, accepting writes them.**
///
/// The other side of the same door — and the assertion that makes the refusal above a gate rather
/// than a wall.
#[test]
fn derived_edges_land_once_the_project_declares_them() {
    use emerge_mapper::project::Project;
    use emerge_mapper::tiles::{DerivedEdges, ImportState};

    let root = Fixture::new("derive-accept")
        .descriptor("wall", "alpha")
        .edge_tokens(&[
            emerge_core::adjacency::EDGE_SOLID,
            emerge_core::adjacency::EDGE_OPEN,
        ])
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let id = app
        .world()
        .resource::<Project>()
        .library
        .descriptors
        .first()
        .map(|d| d.id.clone())
        .unwrap_or_else(|| panic!("the fixture must carry a descriptor"));
    // The stance is per-tab, so the tab has to be the one the door belongs to.
    *app.world_mut().resource_mut::<emerge_mapper::tiles::Mode>() =
        emerge_mapper::tiles::Mode::Meshes;
    app.world_mut()
        .resource_mut::<ImportState>()
        .selected_library_id = Some(id.clone());
    app.world_mut()
        .insert_resource(DerivedEdges(Some(emerge_mapper::tiles::Derived {
            id: id.clone(),
            cells: vec![((0, 0, 0), emerge_core::adjacency::EDGE_SOLID)],
        })));
    app.update();

    fn accept(
        mut keys: bevy::prelude::ResMut<bevy::input::ButtonInput<bevy::prelude::KeyCode>>,
        mut done: bevy::prelude::Local<bool>,
    ) {
        if !*done {
            keys.release_all();
            keys.press(emerge_mapper::keys::binding(emerge_mapper::keys::Action::AcceptEdges).key);
            *done = true;
        }
    }
    app.add_systems(
        bevy::prelude::Update,
        bevy::prelude::IntoScheduleConfigs::before(accept, emerge_mapper::keys::Phase::Act),
    );
    app.update();

    let project = app.world().resource::<Project>();
    let token = project
        .measured
        .descriptors
        .iter()
        .find(|d| d.id == id)
        .and_then(|d| d.subgrid.as_ref())
        .and_then(|g| g.at((0, 0, 0)))
        .and_then(|c| c.edge.clone());
    assert_eq!(
        token.as_deref(),
        Some(emerge_core::adjacency::EDGE_SOLID),
        "the accepted token must be on the cell it was derived for"
    );
    assert!(
        app.world().resource::<DerivedEdges>().0.is_none(),
        "and the proposal is spent, so a second Enter cannot apply it twice"
    );
}

/// **A drop leaves the arrows moving what was dropped — whichever key brought it in.**
///
/// Reported by the author at the keyboard, 2026-08-12: *"once I've selected a mesh by hitting enter
/// or space, the arrow keys then move that mesh around"*. They did not. `Enter` brings a piece into
/// the tile without `Space`, so `Build::placing` stayed false, so `keys::Stance` stayed `Idle`, so
/// the arrows went on walking the library while a member sat focused in the tile.
///
/// **This drives `Enter` alone, on purpose** — the path that was broken. The `Space`-first path was
/// already covered by `the_tiles_tab_opens_a_tile_and_walks_its_grid`, and it is what made the bug
/// invisible: every test took the one route that happened to work.
#[test]
fn a_dropped_member_moves_under_the_arrows_without_space_first() {
    use emerge_mapper::build::Build;

    // Both sized under a cell, so whichever row the walk lands on has a span for the arrows — a
    // full-cell piece answers arrows with a note rather than movement, which is not this test.
    let root = Fixture::new("drop-then-nudge")
        .sized_descriptor("wall", "alpha", 0.2, 0.2)
        .sized_descriptor("floor", "beta", 0.2, 0.2)
        .build("test_map");
    let mut app = emerge_mapper::harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let step = |app: &mut bevy::prelude::App, key: bevy::prelude::KeyCode| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                move |mut keys: bevy::prelude::ResMut<
                    bevy::input::ButtonInput<bevy::prelude::KeyCode>,
                >,
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

    // Pick a row, then drop it with Enter — and never press Space.
    step(&mut app, key(emerge_mapper::keys::Action::TileListNext));
    step(&mut app, key(emerge_mapper::keys::Action::BuildDrop));

    let at = |app: &bevy::prelude::App| -> (f32, f32) {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .and_then(|c| c.members.get(app.world().resource::<Build>().focus))
            .map(|m| m.at)
            .unwrap_or_else(|| panic!("Enter must bring a member into the tile"))
    };
    let before = at(&app);

    step(&mut app, key(emerge_mapper::keys::Action::BuildBack));
    let after = at(&app);

    assert_ne!(
        after, before,
        "after a drop the arrows must move the member — it sat at {before:?} and stayed there, \
         which is the bug: the stance was keyed on how the piece was picked up rather than on \
         whether there is one to move"
    );
    assert!(
        (after.0 != before.0) ^ (after.1 != before.1),
        "and exactly one plan axis moves — got {before:?} -> {after:?}"
    );
}

/// **A flush that cannot move says so, instead of looking like a dead key.**
///
/// Found by authoring, 2026-08-12, and found the hard way: a `0.1 x 1.0 m` wall flushed *along its
/// length* is a genuine no-op — `aligned` returns `(size/2 - span/2) * dir`, and a piece already
/// spanning the tile on that axis is as flush as it can get. The arithmetic is right and nothing
/// moves, which from the keyboard is indistinguishable from a keystroke that never arrived. I did it
/// twice in a row with the source open and only found out by reading the saved RON.
///
/// That is the `refused`-versus-`did nothing` gap `docs/2026-08-11-editor-visual-inspection.md`
/// records as D2 — *"The information exists; only the channel is missing."*
///
/// A **note**, not a problem: nothing went wrong, and the useful half is naming the axis that would
/// move instead.
#[test]
fn a_flush_along_the_axis_a_piece_already_fills_says_why_nothing_moved() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("flush_noop")
        // A metre long and a tenth thick — the shape of every wall in the site kit.
        .sized_descriptor("wall", "alpha", 0.1, 1.0)
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    fn once(app: &mut App, chord: Vec<KeyCode>) {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>,
                      mut done: bevy::prelude::Local<bool>| {
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

    once(&mut app, vec![binding(Action::BuildArm).key]);
    once(&mut app, vec![binding(Action::BuildDrop).key]);

    // Along the wall's length: it already spans the tile on Z, so there is nowhere to go.
    once(
        &mut app,
        vec![KeyCode::ShiftLeft, binding(Action::AlignForward).key],
    );
    let after = at(&app);
    assert_eq!(
        after.1, 0.0,
        "a piece spanning the tile cannot move on that axis"
    );

    let said = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .status
        .note_text();
    assert!(
        said.contains("already flush"),
        "a flush that moves nothing must say so — it said `{said}`"
    );
    assert!(
        said.contains("left/right"),
        "and name the axis that WOULD move, which is the half the author needs: `{said}`"
    );

    // And across it, the flush still lands — the message must not be covering a broken verb.
    once(
        &mut app,
        vec![KeyCode::ShiftLeft, binding(Action::AlignLeft).key],
    );
    let flush = at(&app);
    assert!(
        (flush.0 + 0.45).abs() < 1e-4,
        "Shift+left must put a 0.1 m wall flush at -0.45 in a 1 m tile — got {flush:?}"
    );
}

/// **Undo after two drops takes the second mesh back out.**
///
/// Reported by the author at the keyboard, 2026-08-12: *"If bring one mesh in, then another, when I
/// hit undo it doesn't remove the second mesh I added."*
///
/// `undo_steps_back_through_the_meshes_brought_into_a_tile` already covers a two-drop undo, so if
/// this reproduces, the difference is in how the drops are driven — which is the same shape as the
/// arrows bug: the covered route worked and the travelled one did not.
#[test]
fn undo_after_two_drops_removes_the_second_mesh() {
    use emerge_mapper::build::Build;

    // Sized under a cell so the nudge run has travel — the ladder gives a full-cell piece none.
    let root = Fixture::new("undo-two-drops")
        .sized_descriptor("alpha_one", "alpha", 0.2, 0.2)
        .sized_descriptor("beta_two", "beta", 0.2, 0.2)
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let step = |app: &mut bevy::prelude::App, chord: Vec<bevy::prelude::KeyCode>| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                move |mut keys: bevy::prelude::ResMut<
                    bevy::input::ButtonInput<bevy::prelude::KeyCode>,
                >,
                      mut done: bevy::prelude::Local<bool>| {
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
    };
    let key = |a| emerge_mapper::keys::binding(a).key;
    let n = |app: &bevy::prelude::App| {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .map_or(0, |c| c.members.len())
    };

    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
    assert_eq!(n(&app), 1, "the first drop puts one member in");

    // A different mesh for the second drop, the way an author picks the next piece.
    step(&mut app, vec![key(emerge_mapper::keys::Action::Cancel)]);
    step(
        &mut app,
        vec![key(emerge_mapper::keys::Action::TileListNext)],
    );
    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
    assert_eq!(n(&app), 2, "the second drop puts a second member in");

    // **Which meshes, not how many.** The count alone cannot tell "undo removed the one I just
    // brought in" from "undo removed the other one", and the author reported exactly that second
    // thing: *"when I undo after the second mesh, it throws out the first mesh, not the most recent
    // one."*
    let sources = |app: &bevy::prelude::App| -> Vec<String> {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .map(|c| {
                c.members
                    .iter()
                    .map(|m| match &m.body {
                        emerge_core::composition::Body::Descriptor { id, .. } => id.clone(),
                        _ => "<slot>".to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let both = sources(&app);
    assert_eq!(both.len(), 2, "two distinct meshes are in: {both:?}");
    let first_in = both
        .iter()
        .find(|s| s.contains("alpha"))
        .unwrap_or_else(|| panic!("the first drop was the first library row: {both:?}"))
        .clone();
    let second_in = both
        .iter()
        .find(|s| s.contains("beta"))
        .unwrap_or_else(|| panic!("the second drop was the next row: {both:?}"))
        .clone();

    step(
        &mut app,
        vec![
            emerge_mapper::keys::MOD_KEYS[0],
            key(emerge_mapper::keys::Action::UndoBuild),
        ],
    );
    assert_eq!(
        n(&app),
        1,
        "undo must take the second mesh back out — it left {} in the tile",
        n(&app)
    );
    assert_eq!(
        sources(&app),
        vec![first_in.clone()],
        "and it must be the SECOND mesh that went — `{second_in}` was the most recent one in, so \
         `{first_in}` is what should be left"
    );

    // **A run of nudges costs one undo step, not one per keystroke.**
    //
    // This is the half that was broken, and the arrows make it acute: they repeat at
    // `keys::REPEAT_SECS`, so holding one for a second is about seven entries. An author who nudged
    // a piece into place and pressed `Cmd+Z` walked back through the taps one at a time and reported
    // that undo did not remove the mesh — it was removing the nudges.
    //
    // Ousterhout §6.7: the *policy for grouping actions* belongs to the layer that knows what a user
    // thinks one act is. Moving a piece is one act however many taps it took.
    let undo = vec![
        emerge_mapper::keys::MOD_KEYS[0],
        key(emerge_mapper::keys::Action::UndoBuild),
    ];
    let at = |app: &bevy::prelude::App| {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .and_then(|c| c.members.get(app.world().resource::<Build>().focus))
            .map(|m| m.at)
            .unwrap_or_else(|| panic!("a member must be focused"))
    };

    step(&mut app, vec![key(emerge_mapper::keys::Action::Cancel)]);
    step(
        &mut app,
        vec![key(emerge_mapper::keys::Action::TileListNext)],
    );
    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
    let landed = at(&app);
    for _ in 0..4 {
        step(&mut app, vec![key(emerge_mapper::keys::Action::BuildBack)]);
    }
    assert_eq!(n(&app), 2, "dropped and nudged four times");
    assert_ne!(at(&app), landed, "the nudges moved it");

    // One undo puts the whole run back — not a quarter of it.
    step(&mut app, undo.clone());
    assert_eq!(
        n(&app),
        2,
        "the mesh is still in: a nudge run is not a drop"
    );
    assert_eq!(
        at(&app),
        landed,
        "four nudges must cost ONE undo — it landed at {landed:?} and came back to {:?}",
        at(&app)
    );

    // And the next one takes the drop itself, which is the act before it.
    step(&mut app, undo);
    assert_eq!(
        n(&app),
        1,
        "the second undo removes the mesh that was dropped"
    );
}

/// **Undo removes the most recent drop, even when the list shows it first.**
///
/// `place` uses `insert_sorted`, so a tile's MEMBERS list is in **id order, not the order you
/// dropped them**. Bring in `zulu` and then `alfa` and the panel shows `alfa` on top — so an undo
/// that correctly removes `alfa` looks like it threw out "the first mesh".
///
/// This pins the behaviour so the two readings can be told apart: the most recent drop goes,
/// whatever the list order was.
#[test]
fn undo_removes_the_most_recent_drop_not_the_first_row() {
    use emerge_mapper::build::Build;

    let root = Fixture::new("undo-sorted")
        // Row 0 sorts LAST, row 1 sorts FIRST — so the second drop lands at the top of the list.
        .descriptor("zulu", "alpha")
        .descriptor("alfa", "beta")
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let step = |app: &mut bevy::prelude::App, chord: Vec<bevy::prelude::KeyCode>| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                move |mut keys: bevy::prelude::ResMut<
                    bevy::input::ButtonInput<bevy::prelude::KeyCode>,
                >,
                      mut done: bevy::prelude::Local<bool>| {
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
    };
    let key = |a| emerge_mapper::keys::binding(a).key;
    let sources = |app: &bevy::prelude::App| -> Vec<String> {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .map(|c| {
                c.members
                    .iter()
                    .map(|m| match &m.body {
                        emerge_core::composition::Body::Descriptor { id, .. } => id.clone(),
                        _ => "<slot>".to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
    let after_first = sources(&app);
    assert_eq!(after_first.len(), 1, "one in: {after_first:?}");
    assert!(
        after_first[0].contains("zulu"),
        "the first drop is row 0: {after_first:?}"
    );

    step(&mut app, vec![key(emerge_mapper::keys::Action::Cancel)]);
    step(
        &mut app,
        vec![key(emerge_mapper::keys::Action::TileListNext)],
    );
    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
    let both = sources(&app);
    assert_eq!(both.len(), 2, "two in: {both:?}");
    // The presentation that makes a correct undo look wrong.
    assert!(
        both[0].contains("alfa"),
        "the SECOND drop sorts to the top of the list — that is what makes this confusing: {both:?}"
    );

    step(
        &mut app,
        vec![
            emerge_mapper::keys::MOD_KEYS[0],
            key(emerge_mapper::keys::Action::UndoBuild),
        ],
    );
    assert_eq!(
        sources(&app),
        after_first,
        "undo must remove the most recent drop (`alfa`), leaving the first (`zulu`) — even though \
         the list showed `alfa` on top"
    );

    // **And it says which one it took.** `"undo — 1 in the tile"` cannot distinguish a correct undo
    // from the wrong one when the list order is not the drop order, which is exactly the reading the
    // author landed on. Naming the piece is what makes the count unambiguous.
    let said = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .status
        .note_text();
    assert!(
        said.contains("alfa"),
        "the undo must name the piece it removed, or a right answer still reads as a wrong one —          it said `{said}`"
    );
    assert!(said.contains("out"), "and say it went out: `{said}`");
}

/// **Drop, remove, drop, remove — the cycle an author runs while trying pieces out.**
///
/// Reported by the author, 2026-08-12: *"when I add a mesh, take it away and add a mesh, take it
/// away. It doesn't work."* Every existing tile test builds up and never tears down, so a tile that
/// has been emptied and refilled is a state nothing covered.
#[test]
fn a_tile_survives_being_emptied_and_refilled() {
    use emerge_mapper::build::Build;
    use emerge_mapper::tiles::ImportState;

    let root = Fixture::new("empty-refill")
        .descriptor("alpha_one", "alpha")
        .descriptor("beta_two", "beta")
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let step = |app: &mut bevy::prelude::App, chord: Vec<bevy::prelude::KeyCode>| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                move |mut keys: bevy::prelude::ResMut<
                    bevy::input::ButtonInput<bevy::prelude::KeyCode>,
                >,
                      mut done: bevy::prelude::Local<bool>| {
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
    };
    let key = |a| emerge_mapper::keys::binding(a).key;
    let n = |app: &bevy::prelude::App| {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .map_or(0, |c| c.members.len())
    };
    let said = |app: &bevy::prelude::App| -> String {
        app.world()
            .resource::<ImportState>()
            .status
            .note_text()
            .to_owned()
    };


    for round in 1..=2 {
        step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
        assert_eq!(
            n(&app),
            1,
            "round {round}: the drop must put a member in — said `{}`",
            said(&app)
        );

        step(
            &mut app,
            vec![key(emerge_mapper::keys::Action::BuildDropMember)],
        );
        assert_eq!(
            n(&app),
            0,
            "round {round}: Delete must take it back out — said `{}`",
            said(&app)
        );

        // **And the stage empties with it.** `Build::open` is the model; the staged entities are
        // what an author actually sees. A removal that leaves the mesh standing on the stage is
        // indistinguishable from a removal that did not happen — and every test here had been
        // asserting the model.
        app.update();
        let mut staged = app.world_mut().query_filtered::<
            &emerge_mapper::build::StagedTile,
            bevy::prelude::Without<emerge_mapper::editor::Ghost>,
        >();
        assert_eq!(
            staged.iter(app.world()).count(),
            0,
            "round {round}: the removed mesh must leave the stage too, not just the model"
        );
    }

    // And a third drop after two full cycles still lands, which is what "it doesn't work" would
    // most likely mean: the tile ends up in a state that refuses the next piece.
    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
    assert_eq!(
        n(&app),
        1,
        "a tile emptied twice must still accept a piece — said `{}`",
        said(&app)
    );
    step(
        &mut app,
        vec![key(emerge_mapper::keys::Action::BuildDropMember)],
    );
    assert_eq!(n(&app), 0, "and back out again");

    // **After the tile is empty the arrows go back to the library, and the next piece is a
    // different one.**
    //
    // This is the end the first fix broke. A drop used to set `Build::placing`, so removing the last
    // member left it true over an empty tile: the arrows went on trying to move a piece that was no
    // longer there, the library selection never moved, and the next `Enter` re-dropped the *same*
    // mesh. Measured live over BRP — two captures of the "second" drop came back byte-identical.
    //
    // The stance reads `Build::focus` now, so an empty tile is `Idle` by construction.
    let sources = |app: &bevy::prelude::App| -> Vec<String> {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .map(|c| {
                c.members
                    .iter()
                    .map(|m| match &m.body {
                        emerge_core::composition::Body::Descriptor { id, .. } => id.clone(),
                        _ => "<slot>".to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
    let first = sources(&app);
    step(
        &mut app,
        vec![key(emerge_mapper::keys::Action::BuildDropMember)],
    );
    assert_eq!(n(&app), 0, "emptied again");
    step(
        &mut app,
        vec![key(emerge_mapper::keys::Action::TileListNext)],
    );
    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
    assert_ne!(
        sources(&app),
        first,
        "an arrow over an empty tile must walk the library, so the next drop is a DIFFERENT mesh — \
         it brought in `{first:?}` twice"
    );
    step(
        &mut app,
        vec![key(emerge_mapper::keys::Action::BuildDropMember)],
    );

    // **The same cycle with `Space` in it**, which is how the loop is actually driven: take the
    // piece, drop it, take it away, take the next one. `BuildArm` is a *toggle*, and a drop now
    // leaves `placing` true — so the arm-drop-remove-arm rhythm has to be checked, not assumed.
    for round in 1..=2 {
        step(&mut app, vec![key(emerge_mapper::keys::Action::BuildArm)]);
        step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
        assert_eq!(
            n(&app),
            1,
            "arm round {round}: Space then Enter must land a piece — said `{}`",
            said(&app)
        );
        step(
            &mut app,
            vec![key(emerge_mapper::keys::Action::BuildDropMember)],
        );
        assert_eq!(
            n(&app),
            0,
            "arm round {round}: Delete must take it out — said `{}`",
            said(&app)
        );
    }
}

/// **A new tile starts a new history — undo bottoms out at blank, not at the tile you left.**
///
/// Reported by the author, 2026-08-12: *"it all works except for the last undo. It just goes back to
/// a different mesh instead of blank."*
///
/// `tile_history` watches the tile rather than hooking each verb, which is what makes every mutation
/// covered by construction — but opening a tile is not a mutation, it is a new document, and the
/// watcher recorded it as an edit. So `N` pushed the tile you had just left onto the stack and undo
/// walked back into it. `TileHistory`'s own note already makes this argument about the two *tabs*;
/// this is the same argument one level down, and sharper, because `Cmd+S` saves under the open
/// tile's id — an undo that swapped the document could write one tile's members under another's name.
#[test]
fn a_new_tile_does_not_undo_into_the_one_before_it() {
    use emerge_mapper::build::Build;

    let root = Fixture::new("new-tile-history")
        .descriptor("alpha_one", "alpha")
        .descriptor("beta_two", "beta")
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let step = |app: &mut bevy::prelude::App, chord: Vec<bevy::prelude::KeyCode>| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                move |mut keys: bevy::prelude::ResMut<
                    bevy::input::ButtonInput<bevy::prelude::KeyCode>,
                >,
                      mut done: bevy::prelude::Local<bool>| {
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
    };
    let key = |a| emerge_mapper::keys::binding(a).key;
    let n = |app: &bevy::prelude::App| {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .map_or(0, |c| c.members.len())
    };

    // A tile with something in it, then a fresh one.
    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
    assert_eq!(n(&app), 1, "the first tile has a member");
    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildNew)]);
    name_the_tile(&mut app, "second");
    assert_eq!(n(&app), 0, "N opens a blank tile, once it has been named");

    // Undo, repeatedly. It must never resurrect the tile that was left.
    let undo = vec![
        emerge_mapper::keys::MOD_KEYS[0],
        key(emerge_mapper::keys::Action::UndoBuild),
    ];
    for press in 1..=3 {
        step(&mut app, undo.clone());
        assert_eq!(
            n(&app),
            0,
            "undo #{press} after a new tile must leave it blank — it brought back {} member(s) from \
             the tile before it",
            n(&app)
        );
    }

    // And the new tile's own edits still undo, so the reset did not cost the history its job.
    step(&mut app, vec![key(emerge_mapper::keys::Action::BuildDrop)]);
    assert_eq!(n(&app), 1, "the new tile takes a member");
    step(&mut app, undo);
    assert_eq!(
        n(&app),
        0,
        "and that member undoes, back to the blank this tile started as"
    );
}

/// **Every reachable state of the Tiles tab, against the arrows — the characterisation.**
///
/// Five separate things decide what an arrow does on this tab: whether a tile is open, whether a
/// member is focused, `Build::placing`, how many members there are, and which library row is
/// selected. Nobody had enumerated the combinations, and every bug reported from the keyboard on
/// 2026-08-12 was a combination nobody had considered:
///
/// - `Enter` brings a piece in without `Space`, so `placing` stayed false and the arrows went dead
///   over a focused member;
/// - a drop then *set* `placing`, so removing the last member left it true over an empty tile and the
///   arrows tried to move a piece that was gone — the next `Enter` re-dropped the same mesh;
/// - keying the stance purely on the focus made it permanent, so with anything in the tile the
///   library could never be walked and every second drop was a repeat.
///
/// All three are the same fault: **an arrow that does nothing**. That is the invariant, and it is
/// asserted here over states built the only honest way — by the key sequences that reach them.
///
/// This is a characterisation test. It says what the tab *does*, so a rework can be checked against
/// behaviour rather than against memory.
#[test]
fn no_reachable_tiles_state_leaves_the_arrows_doing_nothing() {
    use emerge_mapper::build::Build;
    use emerge_mapper::keys::Action;
    use emerge_mapper::tiles::ImportState;

    // What an arrow could observably do. If none of these moves, the key was dead.
    #[derive(Debug, PartialEq, Clone)]
    struct Observable {
        row: Option<String>,
        at: Option<(f32, f32)>,
        members: usize,
    }

    let read = |app: &bevy::prelude::App| -> Observable {
        let build = app.world().resource::<Build>();
        Observable {
            row: app
                .world()
                .resource::<ImportState>()
                .selected_library_id
                .clone(),
            at: build
                .open
                .as_ref()
                .and_then(|c| c.members.get(build.focus))
                .map(|m| m.at),
            members: build.open.as_ref().map_or(0, |c| c.members.len()),
        }
    };

    let press = |app: &mut bevy::prelude::App, chord: Vec<bevy::prelude::KeyCode>| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                move |mut keys: bevy::prelude::ResMut<
                    bevy::input::ButtonInput<bevy::prelude::KeyCode>,
                >,
                      mut done: bevy::prelude::Local<bool>| {
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
    };
    let key = |a| emerge_mapper::keys::binding(a).key;

    // Every state named by the sequence that reaches it — so a state that stops being reachable
    // fails here rather than quietly dropping out of coverage.
    let states: Vec<(&str, Vec<Action>)> = vec![
        ("arrived on the door", vec![]),
        // `BuildNew` is deliberately absent from these paths: arriving on the tab already opens a
        // blank tile, and since naming became explicit (2026-08-15) `N` opens the name PROMPT — a
        // typing state, in which the census offers nothing and this invariant does not apply.
        ("a blank tile", vec![]),
        (
            "blank, piece taken",
            vec![Action::BuildArm],
        ),
        (
            "one member, just dropped",
            vec![Action::BuildDrop],
        ),
        (
            "one member, released with Esc",
            vec![Action::BuildDrop, Action::Cancel],
        ),
        (
            "emptied again",
            vec![Action::BuildDrop, Action::BuildDropMember],
        ),
        (
            "two members",
            vec![
                Action::BuildDrop,
                Action::Cancel,
                Action::TileListNext,
                Action::BuildDrop,
            ],
        ),
        (
            "undone back to blank",
            vec![Action::BuildDrop, Action::UndoBuild],
        ),
    ];

    let mut dead = Vec::new();
    for (name, path) in &states {
        // Sized under a cell: the invariant is "an offered key does something", and the ladder
        // gives a full-cell piece no travel by design — that case has its own pin,
        // `an_arrow_on_a_piece_that_fills_the_axis_says_so`.
        let root = Fixture::new(&format!("matrix-{}", name.replace(' ', "-")))
            .sized_descriptor("alpha_one", "alpha", 0.2, 0.2)
            .sized_descriptor("beta_two", "beta", 0.2, 0.2)
            .build("m");
        let mut app = emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles)
            .unwrap_or_else(|e| panic!("{e}"));
        app.update();
        for a in path {
            // Undo carries the platform modifier; everything else here is a bare key.
            let chord = if *a == Action::UndoBuild {
                vec![emerge_mapper::keys::MOD_KEYS[0], key(*a)]
            } else {
                vec![key(*a)]
            };
            press(&mut app, chord);
        }

        // **Ask the census what it claims is live here, then check each claim.**
        //
        // The invariant is not "every arrow does something" — the tab never promised that, and
        // left/right are deliberately unbound while choosing. It is that **the key list does not
        // lie**: a row an author can read off the held-`K` overlay in this state must do what it
        // says. All three bugs reported from the keyboard were exactly that — the census showed the
        // arrows as live and they were not, because the stance was derived from the wrong fact.
        //
        // `keys::Live` is read from the app rather than recomputed, so this checks the editor's own
        // answer rather than a copy of it.
        //
        // **One frame to settle first.** `sense_context` writes `Live` in `Phase::Sense`, which runs
        // *before* `Phase::Act` — so straight after a press it still holds the answer from before
        // that press. Reading it unsettled reports a stance the editor has already moved on from,
        // which is a property of the schedule and not a bug in the tab.
        app.update();
        let live = *app.world().resource::<emerge_mapper::keys::Live>();
        // `MemberPrev` rather than `MemberNext`: a drop focuses the member it just added, which on
        // a sorted list is often the last one — so `next` saturates there for the same reason `up`
        // saturates at row 0. Walking *back* can always move while there is more than one member,
        // and with one member both directions saturate, which is checked below rather than here.
        //
        // And only where there is somewhere to walk: with a single member both directions saturate,
        // which is the same legitimate no-op as `up` at row 0 of the library. The verb is still
        // covered — the `two members` state below is where it has to move.
        let mut probes = vec![Action::TileListNext, Action::BuildBack];
        if read(&app).members > 1 {
            probes.push(Action::MemberPrev);
        }
        let claimed: Vec<Action> = probes
            .into_iter()
            .filter(|a| {
                let b = emerge_mapper::keys::binding(*a);
                b.context == emerge_mapper::keys::Context::Tiles
                    && emerge_mapper::keys::in_context(b.context, live.1).any(|x| x.action == *a)
            })
            .collect();
        assert!(
            !claimed.is_empty(),
            "`{name}`: the census claims no arrow at all is live here, which cannot be right — an \
             author's hand is on the arrows in every state of this tab (live: {live:?})"
        );
        for a in claimed {
            let before = read(&app);
            press(&mut app, vec![key(a)]);
            let after = read(&app);
            if before == after {
                dead.push(format!(
                    "`{name}` — the key list offers {a:?} and it did nothing (live: {live:?}, \
                     state: {before:?})"
                ));
            }
        }

        // **And `Esc` gets you back to the list from anywhere.**
        //
        // The tab prints this promise twice — the `Space` row reads *"take the piece / Esc puts it
        // back"* and a drop answers *"Arrows move it, Esc goes back to the list"*. A dead-key check
        // cannot see it broken, because the arrows still move a piece; they just move it when the
        // author wanted to choose the next one. That was the third bug, and it took three unrelated
        // undo tests going red to surface it.
        if live.1 == emerge_mapper::keys::Stance::Holding {
            press(&mut app, vec![key(Action::Cancel)]);
            app.update();
            let after = *app.world().resource::<emerge_mapper::keys::Live>();
            if after.1 != emerge_mapper::keys::Stance::Idle {
                dead.push(format!(
                    "`{name}` — Esc must put the piece back and return to the library, and the tab \
                     says so in as many words; the stance stayed {:?}",
                    after.1
                ));
            }
        }
    }

    assert!(
        dead.is_empty(),
        "{} state(s) leave an arrow doing nothing. Every bug reported on this tab has been one of \
         these:\n  {}",
        dead.len(),
        dead.join("\n  ")
    );
}

/// **The focus can be moved, and the tile can be emptied** — the two verbs the tab never had.
///
/// Both reported from the keyboard, 2026-08-12: *"once I place mesh down, and I place the second
/// mesh down, how do I switch between two meshes to edit its placement?"* and *"how do I clear the
/// tile creation area after I've added a mesh."* You could not do either. `Build::focus` is what
/// `R`, `Delete`, the arrows and the flush all act on and it is drawn in amber — and every writer of
/// it was a side effect: a drop set it, removal and undo clamped it. Emptying a tile meant pressing
/// `Delete` once per member, and `N` gave you a *different* tile rather than clearing this one.
///
/// See `docs/tiles_tab_contract.md` — these are the Placing clauses for `left`/`right` and
/// `Shift+Delete`.
#[test]
fn the_focus_walks_the_members_and_shift_delete_empties_the_tile() {
    use emerge_mapper::build::Build;
    use emerge_mapper::keys::Action;

    let root = Fixture::new("focus-walk")
        .descriptor("alpha_one", "alpha")
        .descriptor("beta_two", "beta")
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let press = |app: &mut bevy::prelude::App, chord: Vec<bevy::prelude::KeyCode>| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                move |mut keys: bevy::prelude::ResMut<
                    bevy::input::ButtonInput<bevy::prelude::KeyCode>,
                >,
                      mut done: bevy::prelude::Local<bool>| {
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
    };
    let key = |a| emerge_mapper::keys::binding(a).key;
    let focus = |app: &bevy::prelude::App| app.world().resource::<Build>().focus;
    let n = |app: &bevy::prelude::App| {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .map_or(0, |c| c.members.len())
    };

    // Two members in.
    press(&mut app, vec![key(Action::BuildDrop)]);
    press(&mut app, vec![key(Action::Cancel)]);
    press(&mut app, vec![key(Action::TileListNext)]);
    press(&mut app, vec![key(Action::BuildDrop)]);
    assert_eq!(n(&app), 2, "two members to walk between");
    let landed = focus(&app);

    // **Walking back reaches the other one**, which is the whole point: the first mesh was
    // unreachable once the second was down.
    press(&mut app, vec![key(Action::MemberPrev)]);
    let walked = focus(&app);
    assert_ne!(
        walked, landed,
        "left must step to the other member — it stayed on {landed}"
    );

    // And forward comes back, so it is a walk rather than a one-way door.
    press(&mut app, vec![key(Action::MemberNext)]);
    assert_eq!(focus(&app), landed, "right must come back");

    // **Saturating, not wrapping** — a focus that jumped from the last member to the first would be
    // the largest possible move on the smallest keystroke, the argument `SnapLevel::finer` makes.
    press(&mut app, vec![key(Action::MemberNext)]);
    assert_eq!(focus(&app), landed, "the end of the list stays put");

    // **Shift+Delete empties it, and one undo brings the whole tile back.**
    press(
        &mut app,
        vec![bevy::prelude::KeyCode::ShiftLeft, key(Action::ClearTile)],
    );
    assert_eq!(n(&app), 0, "Shift+Delete must empty the tile");

    press(
        &mut app,
        vec![emerge_mapper::keys::MOD_KEYS[0], key(Action::UndoBuild)],
    );
    assert_eq!(
        n(&app),
        2,
        "and it is one step, so a single undo puts both members back — that is what makes it safe \
         to offer"
    );

    // The bare key still removes exactly one, or the shifted form has eaten its sibling.
    press(&mut app, vec![key(Action::BuildDropMember)]);
    assert_eq!(n(&app), 1, "bare Delete removes one member, not the tile");
}

/// **A refused mount names what would satisfy it.**
///
/// Reported from the keyboard, 2026-08-12 — three in a row, building a bedroom tile:
///
/// > `crt_a` mounts to a `support` and nothing here offers one.
/// > `plant_b` mounts to a `support` and nothing here offers one.
/// > `lamp_tall` mounts to a `worktop` and nothing here offers one.
///
/// Each refusal was correct and each was a dead end: it named what was missing and not what to do
/// about it, so the only route forward was guessing which of seventy-five library rows offers a
/// `support`. The editor has that list — `Offers::surfaces` is a field, and `stack::offers_for` is
/// the predicate the refusal itself just used.
///
/// `docs/2026-08-11-editor-visual-inspection.md` records this shape as D2: *"The information exists;
/// only the channel is missing."*
#[test]
fn a_refused_mount_names_a_piece_that_would_hold_it() {
    use emerge_mapper::build::Build;
    use emerge_mapper::keys::Action;
    use emerge_mapper::tiles::ImportState;

    let root = Fixture::new("mount-refusal")
        // A guest that needs a worktop, and a host that offers one — so the refusal has something
        // true to point at. `aa_` / `zz_` fix the library order, and therefore the drop order.
        .mounted_descriptor("aa_lamp", "alpha", "worktop")
        .surface_descriptor("zz_desk", "beta", "worktop")
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let press = |app: &mut bevy::prelude::App, chord: Vec<bevy::prelude::KeyCode>| {
        app.add_systems(
            bevy::prelude::Update,
            bevy::prelude::IntoScheduleConfigs::before(
                move |mut keys: bevy::prelude::ResMut<
                    bevy::input::ButtonInput<bevy::prelude::KeyCode>,
                >,
                      mut done: bevy::prelude::Local<bool>| {
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
    };
    let key = |a| emerge_mapper::keys::binding(a).key;

    // Drop the lamp into an empty tile: nothing holds it, so this must refuse.
    press(&mut app, vec![key(Action::BuildDrop)]);
    assert_eq!(
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .map_or(0, |c| c.members.len()),
        0,
        "a guest with no host must not land"
    );

    let said = app
        .world()
        .resource::<ImportState>()
        .status
        .problem_text()
        .to_owned();
    assert!(
        said.contains("worktop"),
        "the refusal names what is wanted: `{said}`"
    );
    assert!(
        said.contains("zz_desk"),
        "and names a piece that offers one, or the author is left to guess which of the library \
         does — `{said}`"
    );
    assert!(
        said.contains("Shift+Enter") || said.contains("hole"),
        "and keeps the slot route it already offered: `{said}`"
    );
    // The padding bug that shipped in the message an author actually read.
    assert!(
        !said.contains("  "),
        "no run of spaces from a broken line continuation: `{said}`"
    );
}

/// **A guide is a contract with a person, and this is the half a machine can check.**
///
/// `bevy_debugger/guide+watch` refuses a checkpoint nobody registered — by name, listing what would
/// have worked — which is right at runtime and far too late. By then the author is at the keyboard
/// with a script that stops at step four, and the whole reason the channel exists is that their time
/// is the expensive part of the loop.
///
/// So both halves are asserted here, on every file this crate ships under `guides/`:
///
/// 1. **Every named checkpoint is registered.** A typo'd name is a stranded exercise.
/// 2. **Every registered checkpoint actually runs**, against a booted editor. This is the Bevy 0.19
///    trap the harness exists for: a missing `Res<T>` **panics its system** rather than skipping it,
///    and a one-shot system is not in any schedule, so nothing else in the suite would ever call it.
///    A checkpoint that panics the first time an author reaches its step is worse than no checkpoint.
///
/// It also pins two shapes the schema allows and that are easy to get wrong by leaving a field out:
/// a step with `"checkpoint": null` is a **supported state** (only a person can judge it), and every
/// step needs `recovery` — the field Chauvergne et al. 2023 could not find in one of twenty-one
/// shipped tutorials.
#[cfg(feature = "debugger")]
#[test]
fn every_checkpoint_a_shipped_guide_names_is_registered_and_runs() {
    use bevy_debugger_bevy::Checkpoints;

    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(emerge_mapper::guided::GUIDES_DIR);
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));

    let root = Fixture::new("guides")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let registered = app.world().resource::<Checkpoints>().names();
    assert!(!registered.is_empty(), "GuidePlugin registered nothing");

    // **The chooser is a second app with a second vocabulary**, and a guide says which it drives.
    // Checking every script against the editor's names would have been the easy fix and the wrong
    // one: it would let an editor script name a chooser condition and still pass.
    let mut chooser_app = App::new();
    chooser_app
        .insert_resource(emerge_mapper::chooser::Chooser::new(
            std::path::PathBuf::from("."),
            emerge_mapper::chooser::Catalog { kits: Vec::new(), maps: Vec::new() },
            None,
        ))
        .add_plugins(emerge_mapper::chooser::ChooserGuidePlugin);
    let chooser_names = chooser_app.world().resource::<Checkpoints>().names();
    assert!(
        !chooser_names.is_empty(),
        "ChooserGuidePlugin registered nothing"
    );

    let mut seen = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        seen += 1;
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
        let script: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} is not valid JSON: {e}"));
        let steps = script["steps"]
            .as_array()
            .unwrap_or_else(|| panic!("{name} has no `steps` array"));
        assert!(!steps.is_empty(), "{name} has an empty script");

        // Which app this script drives. Absent means the editor, so every existing guide is
        // unchanged and only a script that needs the other vocabulary has to say so.
        let (app_name, vocabulary) = match script["app"].as_str() {
            Some("chooser") => ("chooser", &chooser_names),
            Some(other) => panic!(
                "{name}: `app` is `{other}`, which is not an app this editor has. Use `chooser`, \
                 or leave it out for the editor."
            ),
            None => ("editor", &registered),
        };

        for step in steps {
            let label = step["label"].as_str().unwrap_or("");
            assert!(!label.is_empty(), "{name} has a step with no label");
            assert!(
                step["recovery"].as_str().is_some_and(|r| !r.is_empty()),
                "{name}: step `{label}` has no recovery. That is the field twenty-one of \
                 twenty-one shipped tutorials were missing, and the one an author needs at the \
                 exact moment the step does not work"
            );
            // `null` is a real value here: a step only a person can judge.
            let Some(checkpoint) = step["checkpoint"].as_str() else {
                continue;
            };
            assert!(
                vocabulary.iter().any(|r| r == checkpoint),
                "{name}: step `{label}` watches `{checkpoint}`, which the {app_name} does not \
                 register. It would park for ever. Registered: {}",
                vocabulary.join(", ")
            );
        }
    }
    assert!(seen > 0, "no guides found under {}", dir.display());

    // Every one of them runs. A panic here is the test failing, and it names the system.
    for name in &registered {
        let Some(id) = app.world().resource::<Checkpoints>().get(name) else {
            panic!("`{name}` was listed and then could not be fetched");
        };
        // Every checkpoint takes `In<Value>` now, so a step can be as specific as its claim.
        // `null` is what a step with no `with` supplies.
        if let Err(e) = app.world_mut().run_system_with(id, serde_json::Value::Null) {
            panic!("checkpoint `{name}` could not run: {e}");
        }
    }
}

/// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
///
/// `every_checkpoint_a_shipped_guide_names_is_registered_and_runs` proves a step's *checkpoint*
/// resolves, and the drive tests prove a script can be walked — but every one of those runs on a
/// `Fixture`, so none of them can see that a card tells the author to select a piece which is not
/// there. That is a guide stranding its author at step two while the whole suite reports green.
///
/// # It was written red, and what it caught was not the guides
///
/// Four shipped scripts name `site/floor`, `site/wall`, `site/wall_low` and `site/tile_4`. When
/// this was written on 2026-08-15 none of them existed, and the obvious reading was that the cards
/// had rotted and needed rewriting. **They had not.** `assets/emerge/site/` had been emptied to
/// make a blank slate to author on — and that directory is also `src/site/kit.rs::SITE_PROJECT_DIR`,
/// the game's shipped kit, so the same clear-out had quietly taken 32 game tests down. The guides
/// were pointing at pieces that *should* have been there. Restoring the kit and moving the blank
/// slate to `assets/emerge/ozea/` turned this green without a word of any card changing.
///
/// Worth keeping in mind for the next failure here: **the cheaper explanation is that the corpus
/// moved, not that the prose is stale.**
///
/// It scans the card *text* rather than a structured field because that is where the ids are — in
/// `label`, `goal` and `do` — which is also exactly what the author reads off the overlay.
#[test]
fn every_piece_a_shipped_guide_names_exists_in_the_shipped_kit() {
    // The workspace root — `CARGO_MANIFEST_DIR` is `crates/emerge-mapper`. Spelled here because
    // `stepped::root` is private to that module.
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| panic!("the crate must live two levels under the workspace"))
        .to_path_buf();
    let mut app = harness::build_headless(&workspace, "untitled_map", Some("site"))
        .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..10 {
        app.update();
    }
    let project = app
        .world()
        .get_resource::<emerge_mapper::project::Project>()
        .unwrap_or_else(|| panic!("the project resource is gone"));

    // A card may name a mesh (a library descriptor) or a tile (a composition) — `repair_the_kit`
    // reopens `site/tile_4`, which is the latter. Both are things an author selects by that id, so
    // both count as existing.
    let known: Vec<String> = project
        .library
        .descriptors
        .iter()
        .map(|d| d.id.clone())
        .chain(
            project
                .compositions
                .compositions
                .iter()
                .map(|c| c.id.clone()),
        )
        .collect();

    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(emerge_mapper::guided::GUIDES_DIR);
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));

    // A kit-qualified id — `site/floor`. Prose in these cards contains no other slashed token, and
    // the two guides that name no piece at all (`author_a_tile`, `place_and_generate`) are the
    // proof: they come back empty rather than matching something incidental.
    let looks_like_an_id = |tok: &str| -> bool {
        let Some((kit, piece)) = tok.split_once('/') else {
            return false;
        };
        let ok = |s: &str| {
            !s.is_empty()
                && s.starts_with(|c: char| c.is_ascii_lowercase())
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        };
        ok(kit) && ok(piece)
    };

    let mut stranded: Vec<String> = Vec::new();
    let mut scanned = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        scanned += 1;
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {name}: {e}"));

        let mut missing: Vec<String> = text
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '/'))
            .filter(|tok| looks_like_an_id(tok))
            .filter(|tok| !known.iter().any(|k| k == tok))
            .map(str::to_owned)
            .collect();
        missing.sort();
        missing.dedup();
        if !missing.is_empty() {
            stranded.push(format!("  {name}: {}", missing.join(", ")));
        }
    }
    assert!(scanned > 0, "no guides found under {}", dir.display());

    stranded.sort();
    assert!(
        stranded.is_empty(),
        "these shipped guides tell an author to select pieces the kit does not contain, so they \
         strand at the step that names one. The drive tests pass because they run on a fixture — \
         this is the assertion that reads what actually ships:\n{}\n\nThe kit holds {} \
         descriptor(s) and {} tile(s).",
        stranded.join("\n"),
        project.library.descriptors.len(),
        project.compositions.compositions.len()
    );
}

/// **The shipped script, driven — because a script whose checkpoints nobody has watched pass is a
/// script that will strand its author.**
///
/// `every_checkpoint_a_shipped_guide_names_is_registered_and_runs` proves the names resolve. That is
/// necessary and not close to sufficient: a step can name a real condition that the tab it is written
/// for cannot reach, and the author finds out by standing in front of it.
///
/// This walks `guides/author_a_tile.json` in order, presses the keys an author following it would
/// press, and asserts each step's checkpoint goes from **false to true** at that step and no earlier.
/// The false half matters as much: a checkpoint already true when its step comes up is not measuring
/// the step, and a script full of those reads as a green run while proving nothing.
///
/// It earned its keep before it was finished. Two of the ten steps first written here were wrong —
/// the Tiles tab is `3` and the script said `4` (which is Compose), and a "derive the edges" step
/// watched `edges are staged`, which is written by `B` on the **Meshes** tab off a mesh's subgrid and
/// is not a tile verb at all. Both were found by reading `keys.rs` while writing this; neither was
/// visible to the name check, because both names existed.
///
/// **The key sequence is the one thing here a machine cannot derive**, and it is deliberately not
/// derived: the hints are prose for a person, and a test that parsed them would be testing a parser.
/// What it does instead is bind the sequence to the *bindings* rather than to literal key codes, so a
/// rebound key moves this test with it.
#[cfg(feature = "debugger")]
#[test]
fn the_tile_authoring_script_can_actually_be_followed() {
    use bevy_debugger_bevy::Checkpoints;
    use emerge_mapper::keys::Action;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(emerge_mapper::guided::GUIDES_DIR)
        .join("author_a_tile.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let script: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("bad JSON: {e}"));
    let empty = vec![];
    let steps = script["steps"].as_array().unwrap_or(&empty);

    // A project with something to bring in. Two descriptors, so "another piece" has one to be.
    let root = Fixture::new("script")
        .descriptor("floor_a", "kit")
        .descriptor("wall_a", "kit")
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let key = |a| emerge_mapper::keys::binding(a).key;
    let press = |app: &mut App, chord: Vec<KeyCode>| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    };

    /// What an author does at each step, by label. Sequences, not single keys, because "bring a
    /// floor in" is a walk and a drop.
    fn keystrokes(label: &str) -> Vec<Vec<Action>> {
        match label {
            // Nothing to press: the door IS the tab, chosen on the way in.
            "open the Tiles tab" => vec![],
            // `N` opens the name field; the field is typed into and committed below, out of band,
            // because text is a message stream rather than a key press.
            "start a tile" => vec![vec![Action::BuildNew]],
            // **The walk is part of the step**, and leaving it out is what made this test green
            // for the wrong reason. The script says *"walk the library with up and down — press
            // Enter on site/floor"*; driving only the Enter left nothing picked in the library, so
            // `ImportState::editing` fell back to the focused CANDIDATE — and while a candidate's
            // proposed id carried its pack folder (`site/floor`), that fallback collided with a
            // real library id and the drop went through. It was measuring the collision, not the
            // step. One `TileListNext` from nothing picked lands on the first row, `site/floor`,
            // which is also why the wall step below needs exactly one more.
            "bring a floor in" => vec![vec![Action::TileListNext], vec![Action::BuildDrop]],
            // Nothing to press: the previous drop is what puts a piece in hand. That IS the step —
            // it asserts the state the drop left behind, which is the bug reported from the keyboard.
            "the arrows should now move the piece" => vec![],
            // `Esc` first, and that is the friction this exercise found: `TileListNext` is bound at
            // `Stance::Idle` only, so the arrows cannot walk the library while a piece is in hand.
            // The script's first draft said "press Enter again, pick a wall", which would have
            // dropped a second copy of the floor.
            "bring a wall in as well" => vec![
                vec![Action::Cancel],
                vec![Action::TileListNext],
                vec![Action::BuildDrop],
            ],
            "is the tile still one cell" => vec![],
            "save it" => vec![vec![Action::Save]],
            _ => vec![],
        }
    }

    let mut reached = 0;
    for step in steps {
        let label = step["label"].as_str().unwrap_or("");
        let Some(name) = step["checkpoint"].as_str() else {
            // A step only a person can judge. Nothing to drive and nothing to assert; the script
            // says so with `null` and the watch stream says so with `waiting_on_a_person`.
            continue;
        };
        let Some(id) = app.world().resource::<Checkpoints>().get(name) else {
            panic!("`{name}` is not registered — the other test should have caught this");
        };

        // **Two kinds of step, and the difference is whether anything is pressed.**
        //
        // An *action* step must find its checkpoint false on arrival, or it is measuring nothing —
        // that check is what caught "start a tile", which asked the author to press N and name a tile
        // when the tab had already opened one for them under a generated id.
        //
        // An *observation* step presses nothing: it asks the author to look at what the previous step
        // left behind, and its checkpoint being true IS the pass. "The arrows should now move the
        // piece" is the whole reason the exercise exists — it is the bug reported from the keyboard
        // on 2026-08-12 — and it is an observation, not an action.
        let strokes = keystrokes(label);
        if !strokes.is_empty() {
            let before = app
                .world_mut()
                .run_system_with(
                    id,
                    step.get("with").cloned().unwrap_or(serde_json::Value::Null),
                )
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                !before,
                "step `{label}` watches `{name}`, which was ALREADY true before the step ran. An \
                 action step whose condition already holds measures nothing"
            );
        }

        for chord in strokes {
            // A binding's modifier is part of the chord, and `Cmd+S` is the only one this script
            // reaches. Driven through `MOD_KEYS` rather than a named `SuperLeft`, so the test says
            // what the editor says on both platforms.
            let mut codes: Vec<KeyCode> = chord.iter().copied().map(key).collect();
            if chord
                .iter()
                .any(|a| emerge_mapper::keys::binding(*a).needs_mod)
            {
                codes.push(emerge_mapper::keys::MOD_KEYS[0]);
            }
            press(&mut app, codes);
            // **Saving a never-named tile asks for a name** (2026-08-15), so a script's `Cmd+S`
            // step is two acts: the key, then the answer. Handled at the press rather than in an
            // arm of `keystrokes`, because it is a property of the door and not of any one script.
            if app
                .world()
                .resource::<emerge_mapper::build::Build>()
                .naming
                .is_some()
            {
                name_the_tile(&mut app, "named_by_the_test");
            }
        }
        // The name field is a text field, so it reads `KeyboardInput` **messages** rather than
        // `ButtonInput` — the distinction `bevy_debugger/input` exists to honour, and the reason an
        // agent could press keys but not type into them until it wrote the stream. This is what
        // `{"kind":"Keyboard","text":"kit/tile_a"}` followed by `{"key":"Enter"}` does over BRP.
        if label == "start a tile" {
            let mut tap = |logical: bevy::input::keyboard::Key, code: KeyCode| {
                for state in [
                    bevy::input::ButtonState::Pressed,
                    bevy::input::ButtonState::Released,
                ] {
                    app.world_mut()
                        .write_message(bevy::input::keyboard::KeyboardInput {
                            key_code: code,
                            logical_key: logical.clone(),
                            state,
                            text: None,
                            repeat: false,
                            window: Entity::PLACEHOLDER,
                        });
                }
                app.update();
            };
            for c in "kit/tile_a".chars() {
                tap(
                    bevy::input::keyboard::Key::Character(c.to_string().into()),
                    KeyCode::KeyA,
                );
            }
            tap(bevy::input::keyboard::Key::Enter, KeyCode::Enter);
        }
        for _ in 0..3 {
            app.update();
        }

        let after = app
            .world_mut()
            .run_system_with(
                id,
                step.get("with").cloned().unwrap_or(serde_json::Value::Null),
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            after,
            "step `{label}` says pressing {:?} makes `{name}` true, and it did not. An author \
             following this script stops here.",
            keystrokes(label)
        );
        reached += 1;
    }
    assert!(
        reached >= 6,
        "only {reached} checkpointed steps were driven"
    );
}

/// **The feedback script, driven — the contract `the_tile_authoring_script_can_actually_be_followed`
/// holds, applied to `guides/tile_feedback.json`.**
///
/// Same discipline: walk the steps in order, press what an author following the card would press,
/// and assert each step's checkpoint goes **false to true at its own step**. The script exists to
/// collect keyboard feedback on this branch's new verbs — four-arrow movement, the KIT tab reopen,
/// the centre-snap — so several of its steps are judgement calls with `"checkpoint": null`; those
/// are passed over here exactly the way the app passes over them: a machine cannot answer them.
///
/// The fixture **mirrors the shipped site kit** rather than reading it: the same descriptor ids
/// (`site/floor`, `site/wall`, `site/wall_low`), four committed tiles, and tile_4's low wall dead
/// centre — the defect the script has the author repair. Mirroring keeps this test off the real
/// `assets/` per this crate's rule; `every_piece_a_shipped_guide_names_exists_in_the_shipped_kit`
/// is the asset-contract half that pins the mirror to what actually ships, for every guide rather
/// than only this one.
#[cfg(feature = "debugger")]
#[test]
fn the_tile_feedback_script_can_actually_be_followed() {
    use bevy_debugger_bevy::Checkpoints;
    use emerge_mapper::keys::Action;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(emerge_mapper::guided::GUIDES_DIR)
        .join("tile_feedback.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let script: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("bad JSON: {e}"));
    let empty = vec![];
    let steps = script["steps"].as_array().unwrap_or(&empty);

    // `.pack("site/site", ..)` writes the same bytes the descriptors below overwrite; it is here
    // for its side effect — `assets/site/site/` must exist before a descriptor whose id carries
    // the `site/` namespace can write its mesh under `assets/site/`.
    let root = Fixture::new("feedback")
        .pack("site/site", &["floor", "wall", "wall_low"])
        .descriptor("site/floor", "site")
        .sized_descriptor("site/wall", "site", 0.1, 1.0)
        .sized_descriptor("site/wall_low", "site", 0.2, 1.0)
        .bounded_composition(
            "site/tile_1",
            (1.0, 4.0, 1.0),
            &[("floor", "site/floor", (0.0, 0.0))],
        )
        .bounded_composition(
            "site/tile_2",
            (1.0, 4.0, 1.0),
            &[
                ("floor", "site/floor", (0.0, 0.0)),
                ("wall", "site/wall", (0.45, 0.0)),
            ],
        )
        .bounded_composition(
            "site/tile_3",
            (1.0, 4.0, 1.0),
            &[
                ("floor", "site/floor", (0.0, 0.0)),
                ("wall", "site/wall", (-0.45, 0.0)),
            ],
        )
        .bounded_composition(
            "site/tile_4",
            (1.0, 4.0, 1.0),
            &[
                ("floor", "site/floor", (0.0, 0.0)),
                ("wall_low", "site/wall_low", (0.0, 0.0)),
            ],
        )
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let key = |a| emerge_mapper::keys::binding(a).key;
    let press = |app: &mut App, chord: Vec<KeyCode>| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    };

    /// What an author does at each step, by label — sequences, not single keys.
    fn keystrokes(label: &str) -> Vec<Vec<Action>> {
        match label {
            // Nothing to press: the door IS the tab, chosen on the way in.
            "open the Tiles tab" => vec![],
            // **The walk is part of the step**, and leaving it out is what made this test green
            // for the wrong reason. The script says *"walk the library with up and down — press
            // Enter on site/floor"*; driving only the Enter left nothing picked in the library, so
            // `ImportState::editing` fell back to the focused CANDIDATE — and while a candidate's
            // proposed id carried its pack folder (`site/floor`), that fallback collided with a
            // real library id and the drop went through. It was measuring the collision, not the
            // step. One `TileListNext` from nothing picked lands on the first row, `site/floor`,
            // which is also why the wall step below needs exactly one more.
            "bring a floor in" => vec![vec![Action::TileListNext], vec![Action::BuildDrop]],
            // Observation: the drop is what put a piece in hand. Nothing to press.
            "the piece should be in hand" => vec![],
            "bring a wall in as well" => vec![
                vec![Action::Cancel],
                vec![Action::TileListNext],
                vec![Action::BuildDrop],
            ],
            "is the tile still one cell" => vec![],
            "save it" => vec![vec![Action::Save]],
            // `Esc` releases the piece first — `KitEnter` is bound at `Stance::Idle` — then right
            // opens the KIT tab, three steps walk tile_1 → tile_4, and right again opens it.
            // **Four walks, not three.** The save two steps earlier put a NAMED tile in the kit
            // (naming became explicit 2026-08-15), and `commit_composition` sorts by id — so
            // `site/named_by_the_test` lands ahead of `site/tile_1` and every row moved down one.
            // The script's prose says "walk down to site/tile_4", which is still what an author
            // does; only the count a machine needs is different.
            "reopen tile_4 from the kit" => vec![
                vec![Action::Cancel],
                vec![Action::KitEnter],
                vec![Action::KitNext],
                vec![Action::KitNext],
                vec![Action::KitNext],
                vec![Action::KitNext],
                vec![Action::KitOpen],
            ],
            // Members sort by id — floor, then wall_low — and `open_saved` lands focus at 0, so
            // one step of `.` reaches the low wall; Shift+right flushes its 0.2 m to x = 0.4.
            "flush the low wall against a side" => {
                vec![vec![Action::MemberNext], vec![Action::AlignRight]]
            }
            "save the repair" => vec![vec![Action::Save]],
            _ => vec![],
        }
    }

    let mut reached = 0;
    for step in steps {
        let label = step["label"].as_str().unwrap_or("");
        let Some(name) = step["checkpoint"].as_str() else {
            // A step only a person can judge. Nothing to drive and nothing to assert.
            continue;
        };
        let Some(id) = app.world().resource::<Checkpoints>().get(name) else {
            panic!("`{name}` is not registered — the other test should have caught this");
        };

        // An *action* step must find its checkpoint false on arrival, or it measures nothing; an
        // *observation* step presses nothing, and its checkpoint being true IS the pass — the full
        // argument is on `the_tile_authoring_script_can_actually_be_followed`.
        let strokes = keystrokes(label);
        if !strokes.is_empty() {
            let before = app
                .world_mut()
                .run_system_with(
                    id,
                    step.get("with").cloned().unwrap_or(serde_json::Value::Null),
                )
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                !before,
                "step `{label}` watches `{name}`, which was ALREADY true before the step ran. An \
                 action step whose condition already holds measures nothing"
            );
        }

        for chord in strokes {
            let mut codes: Vec<KeyCode> = chord.iter().copied().map(key).collect();
            if chord
                .iter()
                .any(|a| emerge_mapper::keys::binding(*a).needs_mod)
            {
                codes.push(emerge_mapper::keys::MOD_KEYS[0]);
            }
            // `Align*` wants Shift **held**, and a fresh chord releases everything first — so the
            // shift key rides in the same chord, the way a hand holds it down through the arrow.
            if chord
                .iter()
                .any(|a| emerge_mapper::keys::binding(*a).needs_shift == Some(true))
            {
                codes.push(KeyCode::ShiftLeft);
            }
            press(&mut app, codes);
            // **Saving a never-named tile asks for a name** (2026-08-15), so a script's `Cmd+S`
            // step is two acts: the key, then the answer. Handled at the press rather than in an
            // arm of `keystrokes`, because it is a property of the door and not of any one script.
            if app
                .world()
                .resource::<emerge_mapper::build::Build>()
                .naming
                .is_some()
            {
                name_the_tile(&mut app, "named_by_the_test");
            }
        }
        for _ in 0..3 {
            app.update();
        }

        let after = app
            .world_mut()
            .run_system_with(
                id,
                step.get("with").cloned().unwrap_or(serde_json::Value::Null),
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            after,
            "step `{label}` says pressing {:?} makes `{name}` true, and it did not. An author \
             following this script stops here.",
            keystrokes(label)
        );
        reached += 1;
    }
    assert!(
        reached >= 9,
        "only {reached} checkpointed steps were driven"
    );
}

/// **Retired 2026-08-15 — superseded, and its replacement is stricter.**
///
/// `the_feedback_script_still_matches_the_shipped_kit` held one guide script to the corpus: it
/// checked that `guides/tile_feedback.json` could still send an author to `site/floor` and identify
/// `site/tile_4`. That job now belongs to
/// `every_piece_a_shipped_guide_names_exists_in_the_shipped_kit`, which does it for **all six**
/// scripts instead of one, by scanning the card text every author actually reads.
///
/// Worth recording that the note replaced here claimed *"the scripts were rewritten to author from
/// scratch"*. **They were not** — that is precisely what the new ratchet caught, and what it took
/// to notice the kit itself had gone missing.

/// **A tile that is too big says which member made it too big.**
///
/// The size line said `1 x 3 tiles — hand-stamped, too big to generate` and stopped there. Found
/// during a guided run, from the keyboard: six members in the tile, one of them nudged 0.67 m off
/// centre, and no way to learn which. The existing test asserted the *count*, which was right the
/// whole time — the missing thing was never a number, it was a name.
///
/// The doubling is the part nobody guesses and so the part that has to be stated. `fit_envelope`
/// measures `|offset| + span/2` and the envelope is centred on the anchor, so it reaches that far
/// on *both* sides: 0.67 m off centre costs 1.34 m, which is what turns one cell into three.
///
/// Two negative cases are pinned beside it, because a message that fires when nothing is wrong is
/// how a useful line becomes one people stop reading (`docs/ui.md` §3.4, the alert budget).
#[test]
fn a_tile_too_big_to_generate_names_the_member_that_did_it() {
    use emerge_core::composition::{Body, Member};

    let root = Fixture::new("too_big_why")
        // A wall's shape, from the shipped kit: thin in X, a whole cell long in Z.
        .sized_descriptor("wall", "alpha", 0.1, 1.0)
        // Bigger than a cell all by itself, and centred. Nothing to nudge.
        .sized_descriptor("sofa", "alpha", 0.8, 2.0)
        .build("m");
    let app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    let library = &app
        .world()
        .resource::<emerge_mapper::project::Project>()
        .library;

    let at = |id: &str, x: f32, z: f32| Member {
        id: id.to_owned(),
        body: Body::Descriptor {
            id: id.to_owned(),
            tip: (0, 0),
            on: None,
            patch: None,
        },
        at: (x, z),
        yaw: 0.0,
        lift: 0.0,
        paint: 0,
        of_fingerprint: None,
        note: None,
    };

    // The author's actual tile, reduced to what matters: several centred walls and one nudged.
    let members = vec![
        at("wall", 0.0, 0.0),
        at("wall", 0.45, 0.0),
        at("wall", 0.0, 0.67),
    ];
    let size = emerge_mapper::build::fit_envelope(&members, library, 4.0);
    assert_eq!(
        emerge_mapper::build::tiles_across(size),
        (1, 3),
        "0.67 off centre plus half a metre of wall reaches 1.17, and the envelope has to reach that \
         far both ways: {size:?}"
    );

    let Some(why) = emerge_mapper::build::what_made_it_big(&members, library, size) else {
        panic!("a tile that cannot be generated must say which member did it");
    };
    assert!(why.contains("wall"), "names the piece: {why}");
    assert!(why.contains("0.67"), "and how far off centre it is: {why}");
    assert!(
        why.contains("1.34"),
        "and what that costs, since the doubling is the surprise: {why}"
    );
    assert!(
        why.contains('Z'),
        "and on which axis, since the other one was fine: {why}"
    );

    // A piece simply bigger than a cell is not somebody's mistake, and has no offset to correct.
    let big = vec![at("sofa", 0.0, 0.0)];
    let size = emerge_mapper::build::fit_envelope(&big, library, 4.0);
    assert_eq!(emerge_mapper::build::tiles_across(size), (1, 2));
    assert!(
        emerge_mapper::build::what_made_it_big(&big, library, size).is_none(),
        "naming an offset of zero would be worse than saying nothing"
    );

    // And a tile that fits says nothing at all.
    let fits = vec![at("wall", 0.45, 0.0)];
    let size = emerge_mapper::build::fit_envelope(&fits, library, 4.0);
    assert!(emerge_mapper::build::is_one_cell(size));
    assert!(emerge_mapper::build::what_made_it_big(&fits, library, size).is_none());
}

/// **A step that authors a new tile must not pass by reopening an old one.**
///
/// This is the defect that made the transcript untrustworthy, and it is worth stating exactly,
/// because it looked like the tool working. An author ran the site-kit script; the step "build and
/// save the corner tile" reported PASS; `compositions.ron` contained no corner tile, then or after.
/// The transcript recorded 1/1 for work that never happened, and `k/n` being believable is the whole
/// reason this module exists.
///
/// The cause was a condition weaker than the step that claimed it. `the tile is saved` asks whether
/// *whatever is currently open* is committed — so it says yes for a tile saved ten minutes ago, and
/// says nothing at all about which tile, whether it is new, or what is in it.
///
/// The fix is not a better sentence, it is arguments: `the kit has tiles` with `{"n": 3}` counts what
/// is committed, and a count **cannot go down**, so revisiting old work cannot re-satisfy it. That
/// property is what this test pins — prefer a monotonic condition in any script that authors more
/// than one thing.
#[cfg(feature = "debugger")]
#[test]
fn reopening_a_saved_tile_cannot_pass_a_step_that_asks_for_a_new_one() {
    use bevy_debugger_bevy::Checkpoints;
    use serde_json::json;

    let root = Fixture::new("monotonic")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let run = |app: &mut App, name: &str, args: serde_json::Value| -> bool {
        let Some(id) = app.world().resource::<Checkpoints>().get(name) else {
            panic!("`{name}` is not registered");
        };
        app.world_mut()
            .run_system_with(id, args)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };

    // Two tiles committed, and the second one left open — the state the author was actually in.
    let saved = |app: &mut App, id: &str| {
        let comp = emerge_core::composition::Composition {
            id: id.to_owned(),
            envelope: emerge_core::composition::Envelope::Bounded {
                size: (1.0, 4.0, 1.0),
            },
            members: vec![],
            locations: vec![],
            note: None,
        };
        let mut project = app
            .world_mut()
            .resource_mut::<emerge_mapper::project::Project>();
        project.compositions.compositions.push(comp.clone());
        app.world_mut()
            .resource_mut::<emerge_mapper::build::Build>()
            .open = Some(comp);
    };
    saved(&mut app, "kit/tile_1");
    saved(&mut app, "kit/tile_2");

    // The weak condition says yes, which is the bug: `kit/tile_2` is open and committed, so a step
    // asking for a *third* tile passes without one existing.
    assert!(
        run(&mut app, "the tile is saved", json!(null)),
        "the open tile is committed, so the weak condition holds — this is the state that lied"
    );

    // The monotonic one counts what is on disk and is not fooled.
    assert!(run(&mut app, "the kit has tiles", json!({"n": 2})));
    assert!(
        !run(&mut app, "the kit has tiles", json!({"n": 3})),
        "two tiles exist, so a step that authors the third must NOT pass"
    );

    // And it passes the moment a third actually exists.
    saved(&mut app, "kit/tile_3");
    assert!(run(&mut app, "the kit has tiles", json!({"n": 3})));
}

/// **A corner is two walls that are NOT parallel, and the condition has to know the units.**
///
/// `the tile has turns` counts distinct quarter-turns among the members, which is the only thing in
/// this vocabulary that can tell a corner from two walls side by side.
///
/// It is pinned because the first version divided by `FRAC_PI_2` — treating `Member::yaw` as radians
/// when `build::turn` writes `(m.yaw + 90.0).rem_euclid(360.0)`, i.e. degrees. That version would
/// have **passed the case it was written for**: 90/1.5708 rounds to 57, which is not 0, so a two-wall
/// corner still counted two turns. It fails at 270, which rounds to 172 and collides with 0. Right by
/// accident on the example you tried is the same defect as plain wrong, and harder to notice.
#[cfg(feature = "debugger")]
#[test]
fn a_corner_is_told_from_two_parallel_walls_and_the_units_are_degrees() {
    use bevy_debugger_bevy::Checkpoints;
    use emerge_core::composition::{Body, Composition, Envelope, Member};
    use serde_json::json;

    let root = Fixture::new("turns").descriptor("wall", "alpha").build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let at = |yaw: f32| Member {
        id: format!("wall_{yaw}"),
        body: Body::Descriptor {
            id: "wall".to_owned(),
            tip: (0, 0),
            on: None,
            patch: None,
        },
        at: (0.0, 0.0),
        yaw,
        lift: 0.0,
        paint: 0,
        of_fingerprint: None,
        note: None,
    };
    let open = |app: &mut App, yaws: &[f32]| {
        app.world_mut()
            .resource_mut::<emerge_mapper::build::Build>()
            .open = Some(Composition {
            id: "kit/t".to_owned(),
            envelope: Envelope::Bounded {
                size: (1.0, 4.0, 1.0),
            },
            members: yaws.iter().copied().map(at).collect(),
            locations: vec![],
            note: None,
        });
    };
    let turns = |app: &mut App, n: u64| -> bool {
        let Some(id) = app
            .world()
            .resource::<Checkpoints>()
            .get("the tile has turns")
        else {
            panic!("`the tile has turns` is not registered");
        };
        app.world_mut()
            .run_system_with(id, json!({ "n": n }))
            .unwrap_or_else(|e| panic!("{e}"))
    };

    open(&mut app, &[0.0, 0.0]);
    assert!(!turns(&mut app, 2), "two parallel walls are not a corner");

    open(&mut app, &[0.0, 90.0]);
    assert!(turns(&mut app, 2), "a quarter turn apart is");

    // The case the radians version got wrong: 270 must not read as 0.
    open(&mut app, &[0.0, 270.0]);
    assert!(
        turns(&mut app, 2),
        "and so is three quarters, which the radians version collided with 0"
    );

    // A full turn is the same wall.
    open(&mut app, &[0.0, 360.0]);
    assert!(!turns(&mut app, 2), "360 is 0, not a second direction");
}

/// **An ASSET-CONTRACT test: can the solver actually use the site kit's tiles?**
///
/// It reads the shipped project on purpose, and that is the exception the fixture rule allows —
/// what it asserts *is* a fact about what ships. Authoring tiles is only worth doing if
/// `grammar::from_compositions` turns them into prototypes, and every guided step up to now proved
/// they were *saved*, which is a different claim and the weaker one.
///
/// `skipped` is the useful output: it names each composition it could not make a tile of, and why.
/// A tile the solver cannot use is a tile authored for nothing.
#[test]
fn the_site_kit_tiles_become_solver_prototypes() {
    let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(std::path::Path::to_path_buf)
    else {
        panic!("the crate must sit two levels under the repo root");
    };
    let project = emerge_mapper::project::Project::open(&root, Some("site"))
        .unwrap_or_else(|e| panic!("the shipped site kit must open: {e}"));

    let tiles = &project.compositions.compositions;
    println!("\nsite kit: {} composition(s)", tiles.len());
    for c in tiles {
        println!(
            "  {:<14} {:?}  {} member(s)",
            c.id,
            c.envelope,
            c.members.len()
        );
    }
    if tiles.is_empty() {
        println!("nothing authored yet — nothing to check");
        return;
    }

    let composed = emerge_core::grammar::from_compositions(
        tiles,
        &project.library,
        project.lattice.face_bands,
        1.0,
        emerge_core::composition::agrees,
    )
    .unwrap_or_else(|e| panic!("the site kit's tiles make no grammar at all: {e}"));

    for s in &composed.skipped {
        println!("  SKIPPED: {s}");
    }
    // One prototype is always `Empty` — the grammar's way of saying "nothing goes here".
    let authored = composed.grammar.prototypes.len().saturating_sub(1);
    println!(
        "\ngrammar: {authored} authored prototype(s) + Empty, {} face interface(s)\n",
        composed.faces.iter().filter(|f| f.is_some()).count()
    );

    assert!(
        composed.skipped.is_empty(),
        "a tile the solver cannot use is a tile authored for nothing: {:?}",
        composed.skipped
    );
    // **More prototypes than tiles, and that is the point.** A tile is a quarter-turn object:
    // `from_compositions` emits one prototype per distinct rotation, so a symmetric floor yields one
    // and a wall or a corner yields four. Four authored tiles became ten placeable prototypes.
    //
    // Asserting equality here was wrong and worth recording: it would have read the rotation
    // expansion — the thing that makes a small kit go a long way — as a defect.
    assert!(
        authored >= tiles.len(),
        "every authored tile should yield at least one prototype: {authored} from {}",
        tiles.len()
    );
}

/// **All four arrows move the focused piece, in the four directions the screen shows.**
///
/// Reported from the keyboard on 2026-08-13: the arrows were not intuitive. They were right, and the
/// reason was invisible from the key table alone — `up`/`down` moved the piece and `left`/`right`
/// walked the member list, so on an isometric view the arrows offered two of the four diagonals the
/// screen suggests and the other two did something unrelated.
///
/// `step_in_view` maps a screen wish through the camera yaw, so the fix was a wish rather than any
/// new geometry. What this pins is that all four produce a *distinct* world step: a version that
/// mapped `left` and `right` onto the same axis, or onto the axis `up` already uses, would still
/// "work" for anyone testing one direction at a time.
#[test]
fn all_four_arrows_step_the_piece_in_four_different_directions() {
    use bevy::math::Vec2;
    use emerge_mapper::build::step_in_view;

    // The four screen wishes the bindings produce. Negative y is up, the convention
    // `view::pan_direction` reads.
    let wishes = [
        ("up", Vec2::new(0.0, -1.0)),
        ("down", Vec2::new(0.0, 1.0)),
        ("left", Vec2::new(-1.0, 0.0)),
        ("right", Vec2::new(1.0, 0.0)),
    ];

    // Checked at several camera yaws, because the whole point is that the mapping follows the view.
    // A 45-degree isometric yaw is the one the editor opens on; the others guard the rounding.
    for yaw in [0.0_f32, 45.0, 90.0, 135.0, 180.0, 225.0] {
        let yaw = yaw.to_radians();
        let steps: Vec<(i32, i32)> = wishes.iter().map(|(_, w)| step_in_view(*w, yaw)).collect();
        for (name, step) in wishes.iter().map(|(n, _)| n).zip(&steps) {
            assert_ne!(
                *step,
                (0, 0),
                "`{name}` must move the piece at yaw {:.0}deg",
                yaw.to_degrees()
            );
        }
        let mut sorted = steps.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            4,
            "the four arrows must reach four different cells at yaw {:.0}deg, not {steps:?}",
            yaw.to_degrees()
        );
    }
}

/// **Restored after being deleted by accident, which is its own lesson.** This test was removed in
/// f8d0553 by a slice that meant to drop one duplicated block and took everything between two
/// markers with it. The suite stayed green, because a deleted test cannot fail — the same shape as
/// every other defect found today, one level further out.
/// **The kit is visible from the tab that makes it, and a tile can be reopened.**
///
/// The Tiles tab could author tiles and never show them. `open_blank` was the only opener, so every
/// tile was a new one: an author who finished four could not see the set, could not tell a duplicate
/// from a new one, and could not correct one. That is not hypothetical — a guided run produced
/// `site/tile_4` with its low wall in the middle of the tile instead of flush against an edge, and
/// the only way to fix it was to hand-edit `compositions.ron`.
///
/// The verbs cost no new key. `left`/`right` were this tab's one unbound pair at `Idle`, and
/// `docs/tiles_tab_contract.md` recorded why: *"There is one list on this tab, so there is nothing to
/// switch between."* There are two now.
#[test]
fn the_kit_can_be_walked_and_a_saved_tile_reopened() {
    use emerge_mapper::build::Build;
    use emerge_mapper::keys::{Action, Stance, binding};

    let root = Fixture::new("kit_list")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let press = |app: &mut App, key: KeyCode| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    let key = |a| binding(a).key;

    // Two tiles in the kit, distinguishable by member count.
    {
        let mut project = app
            .world_mut()
            .resource_mut::<emerge_mapper::project::Project>();
        for (id, members) in [("kit/one", 0usize), ("kit/two", 0usize)] {
            let _ = members;
            project
                .compositions
                .compositions
                .push(emerge_core::composition::Composition {
                    id: id.to_owned(),
                    envelope: emerge_core::composition::Envelope::Bounded {
                        size: (1.0, 4.0, 1.0),
                    },
                    members: vec![],
                    locations: vec![],
                    note: None,
                });
        }
    }

    // `right` opens the kit, and that IS the stance — so the key list changes with it.
    press(&mut app, key(Action::KitEnter));
    assert_eq!(
        app.world().resource::<Build>().browsing,
        Some(0),
        "right shows the kit"
    );
    // One more tick before reading the stance: `census` reads `Build` in the same frame the key
    // handler writes it, so the list it draws is one frame behind the flag. Imperceptible to a
    // person and worth stating rather than hiding behind a loop.
    app.update();
    assert_eq!(
        app.world().resource::<emerge_mapper::keys::Live>().1,
        Stance::Browsing,
        "and the census follows, or the key list would be describing the wrong state"
    );

    press(&mut app, key(Action::KitNext));
    assert_eq!(
        app.world().resource::<Build>().browsing,
        Some(1),
        "down walks it"
    );
    // Saturating at the end, like the member walk: holding an arrow should stop, not wrap.
    press(&mut app, key(Action::KitNext));
    assert_eq!(
        app.world().resource::<Build>().browsing,
        Some(1),
        "and stops at the end"
    );

    // `right` again descends into the tile — the verb the tab never had.
    press(&mut app, key(Action::KitOpen));
    let build = app.world().resource::<Build>();
    assert_eq!(
        build.open.as_ref().map(|c| c.id.as_str()),
        Some("kit/two"),
        "the selected tile is open for editing"
    );
    assert_eq!(build.browsing, None, "and the list closes behind it");
    // **Reopening lands you able to edit**, which is the whole reason the verb exists. This
    // asserted the opposite for an hour: `open_saved` cleared `placing`, so an author who reopened a
    // tile got `Stance::Idle` -- arrows walking the library, `,`/`.` not bound at all -- with the
    // tile they had just asked to edit sitting there untouchable. Reported from the keyboard within
    // a minute of the verb shipping: "these keys aren't doing anything".
    assert!(
        build.placing,
        "reopening a tile is holding it: there is nothing else to pick up"
    );

    // `Esc` backs out of the list without opening anything — invariant 2, one stance further.
    press(&mut app, key(Action::KitEnter));
    assert!(app.world().resource::<Build>().browsing.is_some());
    press(&mut app, key(Action::Cancel));
    assert_eq!(
        app.world().resource::<Build>().browsing,
        None,
        "Esc always returns to Choosing"
    );
}

/// **A tile reopened with nothing in it stays Idle**, because then there genuinely is nothing to
/// move. `focused` decides alongside `placing`, and this is what stops the fix above overshooting
/// into the failure it replaced — `placing` true over an empty tile, arrows trying to move a piece
/// that is not there, and the next `Enter` re-dropping the same mesh.
///
/// Both ends have now been wrong once each. `docs/tiles_tab_contract.md` is where that is written
/// down; this is the executable half.
#[test]
fn reopening_an_empty_tile_leaves_the_arrows_walking() {
    use emerge_mapper::build::{Build, open_saved};

    let root = Fixture::new("reopen_empty")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let empty = emerge_core::composition::Composition {
        id: "kit/empty".to_owned(),
        envelope: emerge_core::composition::Envelope::Bounded {
            size: (1.0, 4.0, 1.0),
        },
        members: vec![],
        locations: vec![],
        note: None,
    };
    {
        let mut build = app.world_mut().resource_mut::<Build>();
        open_saved(&mut build, empty);
    }
    for _ in 0..3 {
        app.update();
    }

    assert!(
        app.world().resource::<Build>().placing,
        "the flag is set either way; it is `focused` that has to decide"
    );
    assert_eq!(
        app.world().resource::<emerge_mapper::keys::Live>().1,
        emerge_mapper::keys::Stance::Idle,
        "an empty tile has nothing for the arrows to move, so they must walk the library"
    );
}

/// **Centre and flush are both stops of the arrow ladder, at every depth.**
///
/// Two keyboard reports, one design. First (2026-08-12): *"the movements of a mesh should include a
/// centre placement too"* — a flush left the piece off every lattice rung for good. The interim fix
/// snapped nudges to a tile-centred lattice, which made the centre reachable and the flush position
/// unreachable — the same defect, mirrored. Then (2026-08-14): *"it starts in the center, left
/// moves it flush left ... press J once, then Left, then it moves between flush (outer grid line)
/// and center."* So the ladder divides the span between centre and flush — `aligned`'s own answer —
/// and both ends are stops **by construction**, at every depth `J` reaches.
#[test]
fn the_ladder_reaches_both_centre_and_flush_at_every_depth() {
    use emerge_mapper::build::ladder_step;

    // The span of a 0.2 m piece in a 1 m tile: flush sits at 0.4, on no rung of any divisor.
    let f = 0.4_f32;

    // Depth 0: one press from centre lands flush, one press back lands centre — exactly.
    assert_eq!(ladder_step(0.0, f, 3, 0, 1), f);
    assert_eq!(ladder_step(f, f, 3, 0, -1), 0.0);
    // The ladder ends at flush: a press outward there moves nothing (the handler says so).
    assert_eq!(ladder_step(f, f, 3, 0, 1), f);
    assert_eq!(ladder_step(-f, f, 3, 0, -1), -f);

    // Depth 1: thirds of the span. From centre the first stop is f/3 — "between flush and center"
    // — and flush is still the exact last stop, not `3 * (f / 3)` up to rounding.
    let s = ladder_step(0.0, f, 3, 1, 1);
    assert!(
        (s - f / 3.0).abs() < 1e-6,
        "first stop at depth 1 is a third of the span: {s}"
    );
    assert_eq!(
        ladder_step(2.0 * f / 3.0, f, 3, 1, 1),
        f,
        "the top of the ladder is flush, exactly"
    );

    // An off-ladder start (a hand-edited or reopened tile) lands ON the ladder first.
    let onto = ladder_step(0.19, f, 3, 1, -1);
    assert!(
        (onto - f / 3.0).abs() < 1e-6,
        "0.19 walks down onto the f/3 stop, got {onto}"
    );

    // Out and back returns exactly to the centre, at the deepest rung too.
    let out = ladder_step(0.0, f, 3, 2, 1);
    assert_eq!(
        ladder_step(out, f, 3, 2, -1),
        0.0,
        "out and back must return exactly"
    );

    // A piece that fills the axis has no ladder — the position is returned untouched, and the
    // handler answers with a note instead of movement.
    assert_eq!(ladder_step(0.0, 0.0, 3, 0, 1), 0.0);
}

/// **Flush and the ladder's outermost stop are the same number, exactly — one coordinate system.**
///
/// The session verdict this behaviour shipped under (2026-08-14) was "Shift-flush stays, and the
/// plain arrows end where it lands". Two tests already agreed on `0.4` for one wall, but both
/// derived it by hand — nothing compared the two verbs' own arithmetic, and until `flush_reach`
/// they used two float expressions (`size*0.5 - span*0.5` vs `(size - span)*0.5`) that f32 does
/// not promise agree. This asserts bit-equality across shapes and depths, so an edit that splits
/// the expressions again fails here by name instead of as a piece landing a ULP beside its own
/// grid line and the "already at the flush stop" answer arriving one press late.
#[test]
fn the_flush_verb_and_the_ladder_terminal_agree_exactly() {
    use emerge_mapper::build::{aligned, flush_reach, ladder_step};

    for (size, span) in [
        (1.0_f32, 0.1_f32),
        (1.0, 0.2),
        (1.0, 0.46),
        (2.0, 0.3),
        (1.0, 0.9),
    ] {
        let flush = aligned((0.0, 0.0), (span, span), (size, 4.0, size), (1, 0)).0;
        let f = flush_reach(size, span);
        for depth in 0..3_u32 {
            // Walk the whole ladder from the centre to its last stop.
            let mut pos = 0.0_f32;
            for _ in 0..3_u32.pow(depth) {
                pos = ladder_step(pos, f, 3, depth, 1);
            }
            assert_eq!(pos, flush, "size {size}, span {span}, depth {depth}");
            // And flushing an already-flush piece is a no-op in the same coordinates.
            assert_eq!(
                aligned((pos, 0.0), (span, span), (size, 4.0, size), (1, 0)).0,
                pos,
                "size {size}, span {span}: flush must not move a piece the ladder parked flush"
            );
        }
    }
}

/// **`J` cycles three depths and wraps, and a new tile starts back at the top.**
///
/// The session verdict, verbatim: *"press J once for smaller grid, then press J again for even
/// smaller grid, and a third press would reset to original."* The arithmetic tests hold each depth;
/// nothing pressed `J` twice, so the wrap itself — and the depth resetting when a different tile
/// opens, which `open_blank`/`open_saved` promise — was unpinned until here.
#[test]
fn the_j_ladder_cycles_three_depths_and_a_new_tile_resets_it() {
    let root = Fixture::new("j-cycle")
        .sized_descriptor("wall", "alpha", 0.2, 0.2)
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let press = |app: &mut App, key: KeyCode| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    let depth = |app: &App| app.world().resource::<emerge_mapper::build::Build>().depth;

    assert_eq!(
        depth(&app),
        0,
        "a fresh tile opens at the top of the ladder"
    );

    press(&mut app, key(emerge_mapper::keys::Action::BuildRung));
    assert_eq!(depth(&app), 1, "one J: thirds of the span");
    press(&mut app, key(emerge_mapper::keys::Action::BuildRung));
    assert_eq!(depth(&app), 2, "two: ninths");
    press(&mut app, key(emerge_mapper::keys::Action::BuildRung));
    assert_eq!(
        depth(&app),
        0,
        "the third press wraps back to the original — the author's words"
    );

    press(&mut app, key(emerge_mapper::keys::Action::BuildRung));
    assert_eq!(depth(&app), 1);
    press(&mut app, key(emerge_mapper::keys::Action::BuildNew));
    // Naming is explicit now: `N` opens the prompt and the tile arrives on `Enter`.
    name_the_tile(&mut app, "another");
    assert_eq!(
        depth(&app),
        0,
        "a new tile is a new document, back at the top of the ladder"
    );
}

/// **The held member is marked for the brightness lift — and only the held member.**
///
/// Asked for at the keyboard, 2026-08-14: a subtle highlight on the piece in hand, resolving to the
/// true material on `Escape`. The marker is placed by `drive_build_preview` from the expanded row
/// id (`build/<member>`), so this also pins that id arithmetic: a wrong prefix would mark nothing
/// and the lift would silently never appear.
#[test]
fn the_held_member_carries_the_highlight_marker_until_released() {
    let root = Fixture::new("held-brightens")
        .sized_descriptor("wall", "alpha", 0.2, 0.2)
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let press = |app: &mut App, key: KeyCode| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    let held = |app: &mut App| -> usize {
        app.world_mut()
            .query_filtered::<Entity, With<emerge_mapper::editor::HeldPiece>>()
            .iter(app.world())
            .count()
    };

    press(&mut app, key(emerge_mapper::keys::Action::BuildDrop));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        held(&mut app),
        1,
        "the dropped-and-held member carries the marker"
    );

    // Escape releases the piece; the rebuild carries no marker, so the original material returns.
    press(&mut app, key(emerge_mapper::keys::Action::Cancel));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        held(&mut app),
        0,
        "a released piece is unmarked — its true colours are back"
    );
}

/// **An arrow on a piece that fills the axis says so, instead of looking like a dead key.**
///
/// The ladder gives a full-cell piece no travel *by design* — a floor moved off centre only ever
/// grew the tile — so the matrix ratchet's fixtures use sub-cell pieces and this pins the remaining
/// case: the census still offers the arrows, and the honest answer is a note, the exact manners
/// `a_flush_along_the_axis_a_piece_already_fills_says_why_nothing_moved` established for flush.
#[test]
fn an_arrow_on_a_piece_that_fills_the_axis_says_so() {
    let root = Fixture::new("full-axis-note")
        .descriptor("floor", "alpha")
        .build("test_map");
    let mut app = harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let press = |app: &mut App, key: KeyCode| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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

    press(&mut app, key(emerge_mapper::keys::Action::BuildDrop));
    press(&mut app, key(emerge_mapper::keys::Action::BuildBack));

    let build = app.world().resource::<emerge_mapper::build::Build>();
    let at = build
        .open
        .as_ref()
        .and_then(|c| c.members.first())
        .map(|m| m.at)
        .unwrap_or_else(|| panic!("the drop must put a member in the tile"));
    assert_eq!(at, (0.0, 0.0), "a full-cell piece does not move");
    let note = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .status
        .note_text()
        .to_owned();
    assert!(
        note.contains("fills the tile"),
        "the panel says why nothing moved, naming the layer keys as the way out: {note}"
    );
}

/// **The picked mesh ghosts while you are still choosing it.**
///
/// Asked for at the keyboard, 2026-08-14: *"when I select a mesh, but haven't yet hit enter ...
/// there should be a semitransparent rendering of the mesh selected. Like a preview."* The ghost
/// existed and was gated on `Build::placing` — shown after a piece was taken, never while one was
/// being picked, which is when a preview earns its keep. The one stance it stays out of is
/// Browsing: the kit list selects a tile, not a mesh.
#[test]
fn the_picked_mesh_ghosts_before_enter() {
    let root = Fixture::new("choose-ghost")
        .sized_descriptor("wall", "alpha", 0.2, 0.2)
        .bounded_composition(
            "alpha/tile_1",
            (1.0, 4.0, 1.0),
            &[("wall", "wall", (0.0, 0.0))],
        )
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let press = |app: &mut App, key: KeyCode| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    let ghosts = |app: &mut App| -> usize {
        app.world_mut()
            .query_filtered::<Entity, (
                With<emerge_mapper::build::StagedTile>,
                With<emerge_mapper::editor::Ghost>,
            )>()
            .iter(app.world())
            .count()
    };

    // Arriving on the tab arms the first library row — nothing taken, nothing dropped — and that
    // selection alone is what the ghost previews.
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        ghosts(&mut app),
        1,
        "the armed selection ghosts before any Enter"
    );

    // Browsing the kit hides it: the cursor there is on a tile, and a mesh ghost under it would be
    // previewing the wrong kind of thing.
    press(&mut app, key(emerge_mapper::keys::Action::KitEnter));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        ghosts(&mut app),
        0,
        "no mesh ghost while the kit list is up"
    );

    // And Esc backs out of the kit, so the preview returns with the library list.
    press(&mut app, key(emerge_mapper::keys::Action::Cancel));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        ghosts(&mut app),
        1,
        "backing out of the kit brings the preview back"
    );
}

/// **The `MESHES | KIT` strip does not scroll away with the list.**
///
/// Reported from the keyboard, 2026-08-14: *"the header bar that has 'Meshes Kit (4)' scrolls too,
/// instead of being frozen."* It was the first child inside the scroll container. Asserted through
/// hierarchy rather than pixels — no ancestor of the strip's text may have a scrolling `overflow`
/// — because that is the fact that makes it frozen, and it holds without a window or a layout
/// pass. NOT `ScrollPosition`: 0.19's `Node` `#[require]`s that on every UI node, so an ancestor
/// carrying one proves nothing. The `IN LIBRARY` heading is the contrast: it lives in the
/// scrolling list, so it must have such an ancestor.
#[test]
fn the_list_tab_strip_sits_outside_the_scroll_container() {
    use bevy::ui::OverflowAxis;

    let root = Fixture::new("frozen-strip")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app = harness::build_headless(&root, "m", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    // Onto the Tiles tab, so the strip and the list both exist.
    *app.world_mut().resource_mut::<emerge_mapper::tiles::Mode>() =
        emerge_mapper::tiles::Mode::Tiles;
    for _ in 0..3 {
        app.update();
    }

    let scrolled = |app: &mut App, needle: &str| -> Option<bool> {
        let text_entity = app
            .world_mut()
            .query::<(Entity, &Text)>()
            .iter(app.world())
            .find(|(_, t)| t.0.starts_with(needle))
            .map(|(e, _)| e)?;
        let mut e = text_entity;
        let mut inside_scroll = false;
        while let Some(child_of) = app.world().get::<ChildOf>(e) {
            e = child_of.0;
            if app
                .world()
                .get::<Node>(e)
                .is_some_and(|n| n.overflow.y == OverflowAxis::Scroll)
            {
                inside_scroll = true;
            }
        }
        Some(inside_scroll)
    };

    // `KIT (` rather than `MESHES`: the top tab bar has a MESHES chip of its own, and a needle
    // matching two texts asserts about whichever the query happens to iterate first.
    assert_eq!(
        scrolled(&mut app, "KIT ("),
        Some(false),
        "the tab strip must have no scrolling ancestor — frozen above the list"
    );
    assert_eq!(
        scrolled(&mut app, "IN LIBRARY"),
        Some(true),
        "the rows themselves still live in the scroll container"
    );
}

/// Load a shipped guide and hand back one step's `(checkpoint, with)` by label — so the drive tests
/// below track the JSON they exercise, and an edit to a script moves its test or fails it by name.
fn guide_step(file: &str, label: &str) -> (String, serde_json::Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(emerge_mapper::guided::GUIDES_DIR)
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let script: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{file}: bad JSON: {e}"));
    let step = script["steps"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["label"] == label))
        .unwrap_or_else(|| panic!("{file} has no step labelled `{label}`"));
    let name = step["checkpoint"]
        .as_str()
        .unwrap_or_else(|| panic!("{file}: step `{label}` has no checkpoint"))
        .to_owned();
    (
        name,
        step.get("with").cloned().unwrap_or(serde_json::Value::Null),
    )
}

/// Evaluate a named checkpoint with a step's own args.
#[cfg(feature = "debugger")]
fn checkpoint(app: &mut App, name: &str, with: serde_json::Value) -> bool {
    use bevy_debugger_bevy::Checkpoints;
    let Some(id) = app.world().resource::<Checkpoints>().get(name) else {
        panic!("`{name}` is not registered");
    };
    app.world_mut()
        .run_system_with(id, with)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// **The repair script, driven — `guides/repair_the_kit.json` against the shipped kit's shape.**
///
/// The fixture mirrors the site kit the way `the_tile_feedback_script_can_actually_be_followed`'s
/// does — same ids, four committed tiles, tile_4's low wall dead centre — and every checkpoint is
/// read from the JSON, false before its step and true after it.
#[cfg(feature = "debugger")]
#[test]
fn the_repair_script_can_actually_be_followed() {
    use emerge_mapper::keys::Action;

    let root = Fixture::new("repair-script")
        .pack("site/site", &["floor", "wall_low"])
        .descriptor("site/floor", "site")
        .sized_descriptor("site/wall_low", "site", 0.2, 1.0)
        .bounded_composition(
            "site/tile_1",
            (1.0, 4.0, 1.0),
            &[("floor", "site/floor", (0.0, 0.0))],
        )
        .bounded_composition(
            "site/tile_2",
            (1.0, 4.0, 1.0),
            &[("floor", "site/floor", (0.0, 0.0))],
        )
        .bounded_composition(
            "site/tile_3",
            (1.0, 4.0, 1.0),
            &[("floor", "site/floor", (0.0, 0.0))],
        )
        .bounded_composition(
            "site/tile_4",
            (1.0, 4.0, 1.0),
            &[
                ("floor", "site/floor", (0.0, 0.0)),
                ("wall_low", "site/wall_low", (0.0, 0.0)),
            ],
        )
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let key = |a| emerge_mapper::keys::binding(a).key;
    let press = |app: &mut App, chord: Vec<KeyCode>| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    };
    let walk = |app: &mut App, label: &str, chords: Vec<Vec<Action>>| {
        let (name, with) = guide_step("repair_the_kit.json", label);
        // **A step the door already satisfies is skipped, not failed.** The walk's rule is that a
        // step starts false and the keys make it true — which is what proves the step does
        // something. A door arrives already on one of its panels, so the tab step is true before
        // anything is pressed: that is the door working, not the guide being wrong, and asserting
        // false here would be asserting the old shape.
        if checkpoint(app, &name, with.clone()) {
            return;
        }
        for chord in chords {
            let mut codes: Vec<KeyCode> = chord.iter().copied().map(key).collect();
            if chord
                .iter()
                .any(|a| emerge_mapper::keys::binding(*a).needs_mod)
            {
                codes.push(emerge_mapper::keys::MOD_KEYS[0]);
            }
            press(app, codes);
            // **Saving a never-named tile asks for a name** (2026-08-15), so a script's `Cmd+S`
            // step is two acts: the key, then the answer. Handled at the press rather than in an
            // arm of `keystrokes`, because it is a property of the door and not of any one script.
            if app
                .world()
                .resource::<emerge_mapper::build::Build>()
                .naming
                .is_some()
            {
                name_the_tile(app, "named_by_the_test");
            }
        }
        for _ in 0..3 {
            app.update();
        }
        assert!(
            checkpoint(app, &name, with),
            "`{label}`: `{name}` did not come true"
        );
    };

    walk(&mut app, "open the Tiles tab", vec![]);
    walk(
        &mut app,
        "reopen tile_4 from the kit",
        vec![
            vec![Action::KitEnter],
            vec![Action::KitNext],
            vec![Action::KitNext],
            vec![Action::KitNext],
            vec![Action::KitOpen],
        ],
    );
    // `.` reaches the low wall (members sort floor, wall_low); the plain arrow's last stop IS flush.
    walk(
        &mut app,
        "flush the low wall against a side",
        vec![vec![Action::MemberNext], vec![Action::BuildRight]],
    );
    walk(&mut app, "save the repair", vec![vec![Action::Save]]);
}

/// **The Map script, driven — `guides/place_and_generate.json`, with the mouse's two steps stood in
/// for directly.** A click cannot be injected (`view::Pointer` is the agent path and FVS-R-25 the
/// open defect), so the placement step writes the rows a click would write and the test pins the
/// checkpoint arithmetic; every key step is driven for real, `Cmd+G` through the door included.
#[cfg(feature = "debugger")]
#[test]
fn the_map_script_can_actually_be_followed() {
    use emerge_mapper::keys::Action;
    use emerge_mapper::project::OpenMap;

    let root = Fixture::new("map-script")
        .descriptor("floor", "alpha")
        .bounded_composition(
            "tile_floor",
            (1.0, 1.0, 1.0),
            &[("floor", "floor", (0.0, 0.0))],
        )
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let key = |a| emerge_mapper::keys::binding(a).key;
    let press = |app: &mut App, chord: Vec<KeyCode>| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    };
    let file = "place_and_generate.json";

    // **The checkpoint is true on arrival now.** The step used to be reached by pressing `1` from
    // another tab; a door is chosen on the way in, so this app IS the Map door from its first frame.
    // What the assertion is worth has changed with it: it no longer proves a key works, it proves
    // the checkpoint still names a state the editor actually reaches.
    for _ in 0..2 {
        app.update();
    }

    let (name, with) = guide_step(file, "open the Map tab");
    assert!(
        checkpoint(&mut app, &name, with),
        "pressing 1 opens the Map tab"
    );

    // The mouse's step: two clicks' worth of rows, written the way a click writes them.
    let (name, with) = guide_step(file, "arm a piece and place a few");
    assert!(
        !checkpoint(&mut app, &name, with.clone()),
        "the fixture map starts empty"
    );
    {
        let mut open = app.world_mut().resource_mut::<OpenMap>();
        for (i, at) in [(0.5, 0.5), (1.5, 0.5)].into_iter().enumerate() {
            open.map.placements.push(emerge_core::map::Placed {
                id: format!("floor@{i}"),
                descriptor: "floor".to_owned(),
                at,
                ..Default::default()
            });
        }
    }
    app.update();
    assert!(
        checkpoint(&mut app, &name, with),
        "two placements satisfy the step"
    );

    let (name, with) = guide_step(file, "generate from the kit's tiles");
    assert!(!checkpoint(&mut app, &name, with.clone()));
    press(
        &mut app,
        vec![
            key(Action::GenerateComposed),
            emerge_mapper::keys::MOD_KEYS[0],
        ],
    );
    for _ in 0..2 {
        app.update();
    }
    assert!(
        checkpoint(&mut app, &name, with),
        "Cmd+G must stage a proposal"
    );

    let (name, with) = guide_step(file, "keep it");
    assert!(
        !checkpoint(&mut app, &name, with.clone()),
        "kept must be false while it waits"
    );
    press(&mut app, vec![key(Action::AcceptProposal)]);
    for _ in 0..2 {
        app.update();
    }
    assert!(
        checkpoint(&mut app, &name, with),
        "Enter keeps the proposal as stamps"
    );

    let (name, with) = guide_step(file, "save the map");
    assert!(
        !checkpoint(&mut app, &name, with.clone()),
        "keeping a proposal dirties the map"
    );
    press(
        &mut app,
        vec![key(Action::Save), emerge_mapper::keys::MOD_KEYS[0]],
    );
    for _ in 0..2 {
        app.update();
    }
    assert!(checkpoint(&mut app, &name, with), "Cmd+S saves the map");
}

/// **The edges script, driven — `guides/derive_edges.json`.** The walk to a mesh and the `B` scan
/// are stood in for directly, on the derive tests' own precedent: the rasteriser needs a real GLB
/// and the *door* is what the steps pin. `Enter` is driven for real.
#[cfg(feature = "debugger")]
#[test]
fn the_edges_script_can_actually_be_followed() {
    use emerge_mapper::keys::Action;
    use emerge_mapper::project::Project;
    use emerge_mapper::tiles::{Derived, DerivedEdges, ImportState};

    let root = Fixture::new("edges-script")
        .pack("site/site", &["floor"])
        .descriptor("site/floor", "site")
        .edge_tokens(&[
            emerge_core::adjacency::EDGE_SOLID,
            emerge_core::adjacency::EDGE_OPEN,
        ])
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let key = |a| emerge_mapper::keys::binding(a).key;
    let press = |app: &mut App, chord: Vec<KeyCode>| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    };
    let file = "derive_edges.json";

    // True on arrival: this app is built on the Meshes door, which is what the step now describes.
    let (name, with) = guide_step(file, "open the Meshes tab");
    assert!(
        checkpoint(&mut app, &name, with),
        "the Meshes door's own checkpoint must hold on the door it names"
    );

    // The walk, stood in for: the list is one row here and the checkpoint is about arrival.
    let (name, with) = guide_step(file, "select the floor");
    assert!(
        !checkpoint(&mut app, &name, with.clone()),
        "nothing is selected at boot"
    );
    app.world_mut()
        .resource_mut::<ImportState>()
        .selected_library_id = Some("site/floor".to_owned());
    app.update();
    assert!(
        checkpoint(&mut app, &name, with),
        "the named mesh is selected"
    );

    // `B`, stood in for — the derive tests' documented reason: the rasteriser wants a real GLB.
    let (name, with) = guide_step(file, "derive its edges");
    assert!(
        !checkpoint(&mut app, &name, with.clone()),
        "nothing staged yet"
    );
    app.world_mut().insert_resource(DerivedEdges(Some(Derived {
        id: "site/floor".to_owned(),
        cells: vec![
            ((0, 0, 0), emerge_core::adjacency::EDGE_SOLID),
            ((1, 0, 0), emerge_core::adjacency::EDGE_OPEN),
        ],
    })));
    app.update();
    assert!(
        checkpoint(&mut app, &name, with),
        "the derivation is staged"
    );

    let (name, with) = guide_step(file, "keep the derived edges");
    assert!(
        !checkpoint(&mut app, &name, with.clone()),
        "no token has landed yet"
    );
    press(&mut app, vec![key(Action::AcceptEdges)]);
    for _ in 0..2 {
        app.update();
    }
    assert!(
        checkpoint(&mut app, &name, with),
        "Enter writes the tokens onto the lattice"
    );
    let _ = app.world().resource::<Project>();
}

/// **A tile is composed only from JUDGED meshes; the definition bench shows everything.**
///
/// Asked for at the keyboard, 2026-08-15: *"unlabeled meshes shouldn't show on the tiles tab."*
/// Two entities, one predicate — `labels::needs_labels` is the same test the VLM batch picks its
/// targets by, so "what the labeler still owes you" and "what you cannot build with yet" cannot
/// drift apart.
///
/// The Meshes tab deliberately does **not** hide judged meshes: it is where a piece is defined, and
/// where `Shift+Delete` sends one back to the candidates stripped. Hiding them there would leave a
/// labeled mesh with nowhere to be selected for un-labelling.
#[test]
fn the_tiles_palette_lists_only_judged_meshes() {
    use emerge_mapper::filter::Filters;
    use emerge_mapper::tiles::{Mode, library_ids_for_test};

    // Two pieces: one fully judged (the `Fixture` default) and one still owing an answer.
    let root = Fixture::new("judged-split")
        .descriptor("judged", "alpha")
        .unjudged_descriptor("raw", "alpha")
        .build("m");
    let app = harness::build_headless(&root, "m", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    let project = app.world().resource::<emerge_mapper::project::Project>();
    let filters = Filters::default();

    let composing = library_ids_for_test(project, &filters, true, None);
    assert!(
        composing.iter().any(|id| id == "judged"),
        "the judged piece composes: {composing:?}"
    );
    assert!(
        !composing.iter().any(|id| id == "raw"),
        "an unjudged piece has no mount, kind or description to compose WITH: {composing:?}"
    );

    let defining = library_ids_for_test(project, &filters, false, None);
    assert!(
        defining.iter().any(|id| id == "raw") && defining.iter().any(|id| id == "judged"),
        "the Meshes tab shows both, or un-labelling has nowhere to happen: {defining:?}"
    );
    let _ = Mode::Tiles;
}

/// **A mesh with a proposal still waiting is not composable — completed AND confirmed.**
///
/// Asked for at the keyboard, 2026-08-15: *"before any mesh shows up there, make sure its labels
/// are completed and confirmed."* `needs_labels` answers only the first half, and a machine can
/// satisfy it on its own — but a suggestion nobody has looked at is a **question**, which is the
/// entire reason the labeler stages proposals behind a door. A batch running with auto-confirm
/// answers its own questions, which is what makes the two asks consistent rather than opposed.
#[cfg(feature = "debugger")]
#[test]
fn a_mesh_awaiting_a_proposal_stays_out_of_the_tiles_palette() {
    use emerge_mapper::filter::Filters;
    use emerge_mapper::labels::{Entry, Suggestions};
    use emerge_mapper::tiles::{EditTarget, library_ids_for_test};

    let root = Fixture::new("pending-gate")
        .descriptor("judged", "alpha")
        .build("m");
    let app = harness::build_headless(&root, "m", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    let project = app.world().resource::<emerge_mapper::project::Project>();
    let filters = Filters::default();

    // Fully judged and nothing pending: composable.
    assert!(
        library_ids_for_test(project, &filters, true, None)
            .iter()
            .any(|id| id == "judged"),
        "a settled mesh composes"
    );

    // The same mesh with a proposal waiting on a human is NOT settled, however complete its
    // fields are — the machine has asked a question and nobody has answered it.
    let mut pending = Suggestions::default();
    pending.insert(
        &EditTarget::Library("judged".to_owned()),
        Entry::for_test("judged.glb"),
    );
    let composing = library_ids_for_test(project, &filters, true, Some(&pending));
    assert!(
        !composing.iter().any(|id| id == "judged"),
        "a mesh with an unanswered proposal must not compose: {composing:?}"
    );
    // ...and the definition bench still shows it, which is where the question gets answered.
    assert!(
        library_ids_for_test(project, &filters, false, Some(&pending))
            .iter()
            .any(|id| id == "judged"),
        "the Meshes tab is where U and Y live, so it must still list it"
    );
}

/// **A walk rights the piece it asked about — not the row the author left highlighted — and it
/// stops asking.**
///
/// Asked for at the keyboard, 2026-08-18: *"if the mesh is upside down, it can detect that, and
/// send back a command to rotate it so many times (snapped to grid) to get it upright?"*
///
/// Three properties, and the middle one is a bug this test exists to keep fixed:
///
/// - **The count is obeyed.** `needs_turn.turns` is quarter turns; two of them is 180 degrees, in
///   one act rather than two photograph-and-re-ask cycles.
/// - **The turn lands on the TARGET.** `rotate_mesh` writes to the *focused* piece, because its
///   other callers are the N/P keys where the focus is the subject — so a batch, which carries an
///   explicit target, turned whichever row the author happened to be standing on, and wrote the
///   file when that row was a library entry. Nothing failed; the wrong mesh simply went sideways.
/// - **It stops.** A righting re-photographs and asks again, so a model that keeps saying "not
///   upright" turns a piece for ever — and four quarter turns is where it started, so the loop is
///   silent as well as endless. Past `MAX_RIGHTINGS` the proposal is dropped with a sentence.
#[cfg(feature = "debugger")]
#[test]
fn a_walk_rights_the_piece_it_asked_about_and_then_stops() {
    use emerge_mapper::labels::{Entry, LabelQueue, Suggestions};
    use emerge_mapper::project::Project;
    use emerge_mapper::tiles::{EditTarget, ImportState};
    use emerge_mapper::vlm::NeedsTurn;

    let root = Fixture::new("righting")
        .descriptor("on_its_head", "alpha")
        .descriptor("innocent", "alpha")
        .build("m");
    let mut app = harness::build_headless(&root, "m", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));

    // The author's highlight is on the row the walk is NOT asking about.
    app.world_mut()
        .resource_mut::<ImportState>()
        .selected_library_id = Some("innocent".to_owned());

    let ask = |app: &mut App, turns: u8| {
        let mut e = Entry::for_test("alpha/on_its_head.glb");
        e.suggestion.needs_turn = Some(NeedsTurn {
            axis: "x".to_owned(),
            turns,
            why: "authored on its head".to_owned(),
        });
        app.world_mut().resource_mut::<Suggestions>().insert(
            &EditTarget::Library("on_its_head".to_owned()),
            e,
        );
        app.world_mut()
            .resource_mut::<LabelQueue>()
            .auto_apply_for_test();
        app.update();
    };
    let rotate_of = |app: &App, id: &str| {
        app.world()
            .resource::<Project>()
            .measured
            .get(id)
            .and_then(|d| d.align.rotate)
    };

    ask(&mut app, 2);
    assert_eq!(
        rotate_of(&app, "on_its_head"),
        Some((180, 0, 0)),
        "two quarter turns about X is one half turn, applied in one act"
    );
    assert_eq!(
        rotate_of(&app, "innocent"),
        None,
        "the row the author was standing on is untouched — the turn follows the target"
    );
    assert!(
        app.world().resource::<Suggestions>().pending() == 0,
        "the proposal is spent: the piece is re-photographed and asked again"
    );

    // The second ask is the correction an odd turn taken the wrong way needs, so it is allowed.
    ask(&mut app, 1);
    assert_eq!(rotate_of(&app, "on_its_head"), Some((270, 0, 0)));

    // The third is a loop. The proposal is dropped rather than turned, and the mesh stays put.
    ask(&mut app, 1);
    assert_eq!(
        rotate_of(&app, "on_its_head"),
        Some((270, 0, 0)),
        "past the ceiling nothing more is turned"
    );
    assert_eq!(
        app.world().resource::<Suggestions>().pending(),
        0,
        "and the proposal does not stay staged: `auto_apply_batch` reaches for the first staged \
         entry every frame, so a refusal that kept it would retry sixty times a second for ever"
    );
}

/// **A tile is named by its author, not by the editor.**
///
/// Asked for at the keyboard, 2026-08-15: *"can we make sure that naming tiles that we create is
/// explicit and intuitive?"* The tab minted `<kit>/tile_1`, `tile_2`, … with no verb to say
/// otherwise — invisible while tiles were, and unreadable the moment the KIT list showed them back.
///
/// Three properties, and the third is the one that bit: the prompt must know **why** it was raised.
/// One field serves two verbs — `N` names a tile that does not exist yet, `Cmd+S` names one that
/// does — and a first version inferred the difference from whether the open tile had members, which
/// silently renamed and saved the tile in hand when the author had asked for a new one.
#[test]
fn a_tile_takes_the_name_its_author_types() {
    use emerge_mapper::build::{Build, NameThen};
    use emerge_mapper::keys::Action;

    let root = Fixture::new("explicit-naming")
        .sized_descriptor("wall", "alpha", 0.2, 0.2)
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    for _ in 0..3 {
        app.update();
    }
    let press = |app: &mut App, chord: Vec<KeyCode>| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    };
    let key = |a| emerge_mapper::keys::binding(a).key;
    let open_id = |app: &App| {
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .map(|c| c.id.clone())
            .unwrap_or_default()
    };

    for _ in 0..2 {
        app.update();
    }
    // The tab still opens something immediately — an editor that demanded a name before it would
    // show you anything would be worse — but that name is the editor's, and it is marked as such.
    assert!(
        app.world().resource::<Build>().provisional,
        "the arrival tile is the editor's guess"
    );

    // **`N` asks, and asking is all it does.** The tile arrives on `Enter`, under the typed name.
    press(&mut app, vec![key(Action::BuildNew)]);
    let prompt = app.world().resource::<Build>().naming.clone();
    assert_eq!(
        prompt.map(|p| p.then),
        Some(NameThen::Open),
        "`N` raises the prompt, and it records that a NEW tile is what was asked for"
    );
    name_the_tile(&mut app, "corner_north");
    // **`furniture/`, not `wall/` and no longer `kit/`.** The fixture's descriptors carry no namespace
    // to inherit, so the tile is named after the kit's own directory. It used to
    // be the literal `"kit"`, which is a namespace nobody chose and which collided across every
    // unnamespaced kit; `assets/emerge/compositions.ron` still carries a `kit/tile_1` from that.
    // `wall/` is the third wrong answer, and the one `descriptors.first()` used to give.
    assert_eq!(
        open_id(&app),
        "furniture/corner_north",
        "the tile takes the name that was typed, under the namespace its kit implements"
    );
    assert!(
        !app.world().resource::<Build>().provisional,
        "and it is the author's name now"
    );
    assert_eq!(
        app.world()
            .resource::<Build>()
            .open
            .as_ref()
            .map_or(1, |c| c.members.len()),
        0,
        "`N` opened a BLANK tile — the earlier one was not renamed out from under the author"
    );

    // **A tile the editor named cannot reach the kit unasked.** `Cmd+S` raises the same prompt with
    // a different intent, and answering it names and saves in one act.
    press(&mut app, vec![key(Action::BuildNew)]);
    name_the_tile(&mut app, "corner_south");
    press(&mut app, vec![key(Action::BuildDrop)]);
    for _ in 0..2 {
        app.update();
    }
    press(
        &mut app,
        vec![key(Action::Save), emerge_mapper::keys::MOD_KEYS[0]],
    );
    for _ in 0..2 {
        app.update();
    }
    // Already named, so this saved rather than asking.
    assert!(
        app.world().resource::<Build>().naming.is_none(),
        "a named tile just saves"
    );
    assert!(
        app.world()
            .resource::<emerge_mapper::project::Project>()
            .compositions
            .compositions
            .iter()
            .any(|c| c.id == "furniture/corner_south"),
        "and it lands in the kit under the author's name, in the namespace that kit implements"
    );
}

/// **`F` puts the keyboard in the filter box, and `Enter` gives it back.**
///
/// The box had one writer — a mouse click — on the tab whose whole argument is that keystrokes are
/// faster, which made "narrow the list" the one thing an author had to leave the keyboard for (and
/// made it uninstructable in a guide script). Asked for at the keyboard, 2026-08-15.
///
/// The `Enter` half is the part worth pinning hardest: it must leave the box **without** the same
/// keypress falling through to `BuildDrop` and dropping a piece. That is the `xseam` shape the tab
/// has paid for before — six descriptors once arrived in `library.ron` from an `Enter` that
/// committed a field and then kept going.
#[test]
fn f_focuses_the_filter_and_enter_hands_the_keyboard_back() {
    use emerge_mapper::filter::{Filters, Pane};
    use emerge_mapper::keys::Action;

    let root = Fixture::new("filter-keys")
        .sized_descriptor("wall", "alpha", 0.2, 0.2)
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let press = |app: &mut App, key: KeyCode| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    let focus = |app: &App| app.world().resource::<Filters>().focus_pane();
    let members = |app: &App| {
        app.world()
            .resource::<emerge_mapper::build::Build>()
            .open
            .as_ref()
            .map_or(0, |c| c.members.len())
    };

    for _ in 0..2 {
        app.update();
    }
    assert_eq!(focus(&app), None, "the tab does not open typing");

    press(&mut app, key(Action::FocusFilter));
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        focus(&app),
        Some(Pane::Candidates),
        "`F` puts the cursor in the box"
    );
    // **One more frame before typing.** Every field here drains the message stream while it is shut,
    // so the keystroke that opens it cannot become its first character (`keys.rs`, the `xseam` bug).
    // The test used to get this frame for free from the tab press that preceded `F`; a door is
    // arrived at rather than pressed, so the frame has to be asked for.
    app.update();

    // Type into it the way the editor really receives text — a message stream, not `ButtonInput`.
    let tap = |app: &mut App, logical: bevy::input::keyboard::Key, code: KeyCode| {
        for state in [
            bevy::input::ButtonState::Pressed,
            bevy::input::ButtonState::Released,
        ] {
            app.world_mut()
                .write_message(bevy::input::keyboard::KeyboardInput {
                    key_code: code,
                    logical_key: logical.clone(),
                    state,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
        }
        app.update();
    };
    tap(
        &mut app,
        bevy::input::keyboard::Key::Character("w".into()),
        KeyCode::KeyW,
    );
    assert_eq!(
        app.world().resource::<Filters>().text(Pane::Candidates),
        "w",
        "the box takes the key"
    );

    let before = members(&app);
    tap(&mut app, bevy::input::keyboard::Key::Enter, KeyCode::Enter);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        focus(&app),
        None,
        "`Enter` hands the keyboard back to the tab"
    );
    assert_eq!(
        app.world().resource::<Filters>().text(Pane::Candidates),
        "w",
        "and keeps the filter — `Esc` is the key that throws it away"
    );
    assert_eq!(
        members(&app),
        before,
        "and that same Enter must NOT fall through to the drop: leaving a field is one act"
    );
}

/// **`right` goes into the kit and `left` comes back out** — the column browser, both directions.
///
/// This key has now been wrong twice in opposite ways, which is why it is pinned rather than
/// trusted. The KIT strip shipped promising *"right reopens / left back"* over an **unbound**
/// `left`; the first fix reworded the strip to name `Esc`, making the prose honest and leaving the
/// author pressing a dead key anyway. Reported at the keyboard, 2026-08-15: *"I would expect left
/// to move back to meshes, but it doesn't."* The promise was right and the binding was missing.
///
/// `Esc` still backs out — it backs out of everything, and `no_reachable_tiles_state_leaves_the_
/// arrows_doing_nothing` covers that — so this asserts the direction the idiom implies, in both
/// directions, against `Build::browsing` itself.
#[test]
fn the_kit_list_is_entered_with_right_and_left_comes_back() {
    use emerge_mapper::keys::Action;

    let root = Fixture::new("kit-left-back")
        .descriptor("wall", "alpha")
        .bounded_composition(
            "alpha/tile_1",
            (1.0, 4.0, 1.0),
            &[("wall", "wall", (0.0, 0.0))],
        )
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let press = |app: &mut App, key: KeyCode| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    let browsing = |app: &App| {
        app.world()
            .resource::<emerge_mapper::build::Build>()
            .browsing
    };

    // The tab opens holding the piece it armed, and the kit is an `Idle` verb — `Esc` puts it down.
    press(&mut app, key(Action::Cancel));
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        browsing(&app),
        None,
        "the tab does not open on the kit list"
    );

    press(&mut app, key(Action::KitEnter));
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        browsing(&app),
        Some(0),
        "`right` shows the kit, cursor at the top"
    );

    press(&mut app, key(Action::KitLeave));
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        browsing(&app),
        None,
        "`left` must come back to the meshes — the strip has promised this since the kit shipped"
    );

    // And the two are different keys doing different things, not one key toggling: `right` from
    // the kit reopens a tile rather than leaving it.
    press(&mut app, key(Action::KitEnter));
    press(&mut app, key(Action::KitOpen));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(browsing(&app), None, "reopening also leaves the list");
    assert!(
        app.world()
            .resource::<emerge_mapper::build::Build>()
            .open
            .is_some(),
        "but it leaves with a tile open, which is what tells the two apart"
    );
}

/// **The palette rows live in a scroll container the follow can move.**
///
/// The behavioural half of F-9 — the scroll actually tracking the arrows — needs a window and a
/// layout pass, so it is verified at the keyboard; what holds headless is the structure that makes
/// it possible (a row inside a `overflow: scroll` ancestor) and the arithmetic
/// (`chrome::scroll_to_reveal`'s own unit tests). This is the structural half, the shape
/// `the_list_tab_strip_sits_outside_the_scroll_container` established.
#[test]
fn the_palette_rows_live_in_a_scroll_container() {
    use bevy::ui::OverflowAxis;

    let root = Fixture::new("palette-scrolls")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app = harness::build_headless(&root, "m", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    for _ in 0..3 {
        app.update();
    }

    // The palette renders each descriptor id as a row. Other panels may render the same id, so the
    // claim is existential: at least one such text sits under a scrolling ancestor.
    let rows: Vec<Entity> = app
        .world_mut()
        .query::<(Entity, &Text)>()
        .iter(app.world())
        .filter(|(_, t)| t.0 == "wall")
        .map(|(e, _)| e)
        .collect();
    assert!(
        !rows.is_empty(),
        "the palette must render its one descriptor as a row"
    );
    let any_scrolled = rows.iter().any(|&row| {
        let mut e = row;
        while let Some(child_of) = app.world().get::<ChildOf>(e) {
            e = child_of.0;
            if app
                .world()
                .get::<Node>(e)
                .is_some_and(|n| n.overflow.y == OverflowAxis::Scroll)
            {
                return true;
            }
        }
        false
    });
    assert!(
        any_scrolled,
        "a palette row must have a scrolling ancestor for the follow to move"
    );
}

/// One tile member, spelled out — `composition::Member` has no `Default`, deliberately: every field
/// of it is a decision somebody made about where a piece sits.
#[cfg(feature = "debugger")]
fn member(row: &str, descriptor: &str, yaw: f32) -> emerge_core::composition::Member {
    emerge_core::composition::Member {
        id: row.to_owned(),
        body: emerge_core::composition::Body::Descriptor {
            id: descriptor.to_owned(),
            tip: (0, 0),
            on: None,
            patch: None,
        },
        at: (0.0, 0.0),
        yaw,
        lift: 0.0,
        paint: 0,
        of_fingerprint: None,
        note: None,
    }
}

/// **The room script, driven — `guides/build_a_room.json`.**
///
/// Same contract the other drive tests hold: walk the steps in order, put the editor into the state
/// the card's `do` describes, and assert each checkpoint goes **false to true at its own step**. A
/// card whose checkpoint is already true when the step begins teaches nothing, and one that never
/// becomes true strands the author — this catches both.
///
/// # What is stood in for, and why
///
/// The room half is stamped with the **mouse**, and a click cannot be injected here — `view::Pointer`
/// is the agent path and FVS-R-25 is the open defect — so those steps write the `Stamped` rows a
/// click would write, exactly as `the_map_script_can_actually_be_followed` writes the placements its
/// own click step would. What is pinned either way is the **checkpoint arithmetic**: that the counts
/// the card asks for are the counts the described work actually produces.
///
/// The tile half is real key presses.
///
/// The count arguments are the fragile part and the reason this exists. `the tile has turns` counts
/// **distinct quarter-turns**, so a corner is `n: 2` — floor and first wall at 0, the turned wall at
/// 1 — and the `n: 1` first written here would have passed on any non-empty tile at all. A
/// checkpoint that cannot fail reads as a guarantee, which is worse than no checkpoint.
#[cfg(feature = "debugger")]
#[test]
fn the_room_script_can_actually_be_followed() {
    use emerge_mapper::build::Build;

    use emerge_mapper::project::OpenMap;

    let root = Fixture::new("room-script")
        .descriptor("floor", "site")
        .descriptor("wall", "site")
        .descriptor("wall_doorway", "site")
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let key = |a| emerge_mapper::keys::binding(a).key;
    let press = |app: &mut App, chord: Vec<KeyCode>| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
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
    };
    let file = "build_a_room.json";
    let settle = |app: &mut App| {
        for _ in 0..2 {
            app.update();
        }
    };

    // **Step 1 is satisfied by the door.** It used to start false because the editor booted on Map
    // and `3` was what reached the Tiles tab. The Kit door opens on Meshes and holds Tiles one key
    // away, so what this step now asserts is that the panel the guide names is one this door has —
    // which is the fact worth checking, and the only one still available.
    settle(&mut app);
    let (name, with) = guide_step(file, "open the Tiles tab");
    press(&mut app, vec![key(emerge_mapper::keys::Action::TabSlot2)]);
    settle(&mut app);
    assert!(
        checkpoint(&mut app, &name, with),
        "the Kit door's second panel is Tiles"
    );

    // **The corner.** Two walls, the second turned a quarter — which is what makes it a corner and
    // not a doubled wall, and is exactly what `the tile has turns` is counting.
    let (turns, with) = guide_step(file, "floor it, then stand two walls at right angles");
    // **The count is the assertion, not a detail of it.** `the tile has turns` counts DISTINCT
    // quarter-turns, so a corner is two — floor and first wall at 0, the turned wall at 1. Asking
    // for one would be satisfied by any tile with a single piece in it, and the false-then-true
    // walk below would still pass, because a *blank* tile has none. That is a checkpoint which
    // cannot fail dressed as one that can, so it is pinned by value here.
    assert_eq!(
        with.get("n").and_then(|v| v.as_u64()),
        Some(2),
        "the corner step must ask for TWO distinct quarter-turns; `n: 1` passes on any tile with a \
         piece in it and would wave through a doubled wall as a corner"
    );
    assert!(
        !checkpoint(&mut app, &turns, with.clone()),
        "a blank tile has no quarter-turns, so the corner step starts false"
    );
    {
        // The members the card's three Enters and one R produce. Written rather than key-driven
        // because the card's own wording ("walk to site/floor") depends on list order, which is a
        // property of the corpus and not of the editor.
        let mut build = app.world_mut().resource_mut::<Build>();
        if let Some(open) = build.open.as_mut() {
            for (id, yaw) in [("site/floor", 0.0), ("site/wall", 0.0), ("site/wall", 90.0)] {
                open.members.push(member(
                    &format!("{}_{}", id.replace('/', "_"), open.members.len()),
                    id,
                    yaw,
                ));
            }
        }
    }
    settle(&mut app);
    assert!(
        checkpoint(&mut app, &turns, with),
        "floor and wall at 0 plus a wall turned a quarter is TWO distinct quarter-turns — if this \
         fails, the card is asking for a corner the editor does not produce"
    );

    // **The wall tile and the door tile** ask `the tile contains` by id, which is the check that
    // stops the card sending an author to a piece the kit does not have under that name.
    for (label, id) in [
        ("floor it and flush ONE wall to one side", "site/wall"),
        (
            "floor it and flush the doorway to one side",
            "site/wall_doorway",
        ),
    ] {
        let (name, with) = guide_step(file, label);
        assert_eq!(
            with.get("ids").and_then(|v| v.as_array()).map(|a| a.len()),
            Some(1),
            "`{label}` should name exactly one piece"
        );
        {
            let mut build = app.world_mut().resource_mut::<Build>();
            if let Some(open) = build.open.as_mut() {
                open.members.clear();
                open.members.push(member("the_piece", id, 0.0));
            }
        }
        settle(&mut app);
        assert!(
            checkpoint(&mut app, &name, with),
            "`{label}` asks for {id}, and a tile holding exactly that did not satisfy it"
        );
    }

    // **The room.** Nine stamps: four wall runs, four corners, one door — the counts the card asks
    // for at its three stamping steps, checked as the monotonic ladder they are.
    let steps = [
        ("lay the four walls", 4usize),
        ("close the four corners", 8),
        ("put the door in", 9),
    ];
    for (label, want) in steps {
        let (name, with) = guide_step(file, label);
        assert_eq!(
            with.get("n").and_then(|v| v.as_u64()),
            Some(want as u64),
            "`{label}` should ask for {want} tiles on the map"
        );
        assert!(
            !checkpoint(&mut app, &name, with.clone()),
            "`{label}` must start false — it asks for more tiles than are down"
        );
        {
            let mut open = app.world_mut().resource_mut::<OpenMap>();
            while open.map.stamps.len() < want {
                let n = open.map.stamps.len();
                open.map.stamps.push(emerge_core::composition::Stamped {
                    id: format!("stamp@{n}"),
                    of: "tile".to_owned(),
                    at: (n as f32, 0.0),
                    ..Default::default()
                });
            }
        }
        settle(&mut app);
        assert!(
            checkpoint(&mut app, &name, with),
            "`{label}` wants {want} stamped tiles and {want} did not satisfy it. Note this counts \
             `Map::stamps`, NOT placements: a room built from tiles writes references, and the \
             placement count stays at zero throughout"
        );
    }

    // And the placement count really does stay at zero — the reason `the map has tiles on it` had to
    // exist rather than reusing `the map has placements`.
    let open = app.world().resource::<OpenMap>();
    assert!(
        open.map.placements.is_empty(),
        "nine tiles were stamped and {} loose placements appeared; if a stamp ever expands into \
         rows on the map, this card's counts stop meaning what they say",
        open.map.placements.len()
    );
}

/// **An ASSET-CONTRACT test — it reads the shipped corpus on purpose.**
///
/// The chooser exists because nothing on screen said which kit was loaded, and the piece count is
/// the fact that answers it. This asserts that fact is available from what actually ships: the
/// scan finds the shipped kits, and it can tell the blank one from a populated one.
///
/// # Why the counts are not pinned exactly
///
/// `site`'s piece count changes the moment somebody authors a piece, which is what this editor is
/// *for* — pinning 45 would make importing a mesh a failing test, the corpus-dependence trap
/// `tests/fixtures/mod.rs` exists to avoid. What is pinned is the **contract**: every shipped kit
/// scans, `site_v2` is empty by design (`docs/2026-08-15-blank-slate-handoff.md` §1) and `site` is
/// not, and the root kit is reachable with no `--kit` at all.
#[test]
fn the_chooser_sees_the_shipped_project() {
    use emerge_mapper::chooser::Catalog;

    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| panic!("the crate must live two levels under the workspace"))
        .to_path_buf();

    let catalog = Catalog::scan(&workspace).unwrap_or_else(|e| panic!("{e}"));
    let labels: Vec<&str> = catalog.kits.iter().map(|k| k.label.as_str()).collect();

    // **`furniture`, where `emerge` used to be.** The project root stopped being a kit on
    // 2026-08-16: `assets/emerge/` holds `vocab.ron`, `kits.ron`, `compositions.ron` and `maps/`,
    // and every library lives in a kit directory beside them. The 75 flat ids that used to sit at
    // the root are `furniture` now, which is what the root directory always held.
    assert!(
        labels.contains(&"furniture"),
        "the furniture kit did not scan. Found: {labels:?}"
    );
    assert!(
        !labels.contains(&"emerge"),
        "the project root is not a kit — that conflation is what the split undid: {labels:?}"
    );

    let pieces = |label: &str| -> usize {
        catalog
            .kits
            .iter()
            .find(|k| k.label == label)
            .map_or_else(|| panic!("`{label}` is missing"), |k| k.pieces)
    };
    assert!(
        pieces("furniture") > 0,
        "the furniture kit is empty. It is `assets/emerge/library.ron`'s 75 pieces, moved rather \
         than cleared — put it back with `git checkout HEAD -- assets/emerge/furniture/`"
    );

    // **Every kit is now a subdirectory, and every one is `--kit`-able.** The no-flag mode was the
    // root kit, and there is no root kit; `Project::open(kit: None)` reads `authoring` from
    // `kits.ron` instead, which is a project's statement rather than a directory's accident.
    assert!(
        catalog.kits.iter().all(|k| k.flag.is_some()),
        "no kit is the no-flag one any more: {labels:?}"
    );

    // **The binding is what decides what loads**, and it is a file rather than a scan.
    let kits_path = workspace
        .join("assets/emerge")
        .join(emerge_core::kits::KITS_FILE);
    let kits = emerge_core::kits::Kits::parse(
        &std::fs::read_to_string(&kits_path).unwrap_or_else(|e| panic!("{kits_path:?}: {e}")),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        kits.authoring_bind().is_some(),
        "the shipped project has to say where new work lands"
    );
    for b in &kits.bind {
        assert!(
            labels.contains(&b.dir.as_str()),
            "`{}` is bound but no such directory scanned: {labels:?}",
            b.dir
        );
    }
}

/// **`Esc` with nothing in hand is the way out, because that is the key an author reaches for.**
///
/// Reported at the keyboard, 2026-08-16: *"I'm not seeing any way to go back to the main menu once I
/// enter a map. escape three times."* All three presses correctly did nothing — the Map tab's peel
/// had nothing left to take back and stopped there — while `Cmd+O` and the `‹ kits & maps` button
/// were the two smallest pieces of text on the screen.
///
/// This pins the layer that was missing rather than a new verb: the peel's own comment already
/// promised *"each press steps back out one layer"*, and the map is the outermost layer there is.
/// **The press before it must still peel**, or "one layer per press" becomes "sometimes two" — so a
/// selection is cleared first, the next `Esc` *asks*, and only the third goes.
///
/// The question is the second half of the same report: *"Escape twice when nothing selected should
/// prompt. Are you sure you want to quit? One more escape at that prompt should quit."*
#[test]
fn escape_peels_to_the_selection_then_asks_then_goes() {
    use emerge_mapper::editor::EditorState;

    let root = Fixture::new("escape-out")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let tap = |app: &mut App, key: KeyCode| {
        app.add_systems(
            // **`PreUpdate`, after Bevy's own input pass — not `.before(Phase::Act)`.**
            //
            // The injector and `editor::answer_the_leaving_prompt` were both `.before(Phase::Act)`
            // and unordered *relative to each other*, so which ran first was arbitrary — and they
            // conflict on `ButtonInput`, so the executor picks. This test passed alone and failed in
            // the full suite for exactly that reason. Pressing after `InputSystems` (which is what
            // clears `just_pressed`) makes the press visible to **every** `Update` system, which is
            // a superset of what `.before(Act)` bought and is not a coin toss.
            PreUpdate,
            IntoScheduleConfigs::after(
                move |mut input: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
                    if !*done {
                        input.release_all();
                        input.press(key);
                        *done = true;
                    }
                },
                bevy::input::InputSystems,
            ),
        );
        app.update();
    };

    // **The way back is a state change, not an exit.** It was `AppExit` with a code the parent
    // process compared against; both screens are one application now (`screen.rs`), so leaving sets
    // `Screen::Menu`. Read as *pending*: `StateTransition` runs before `Update`, so the set made by
    // this frame's key applies on the next one — and checking it here rather than stepping again
    // keeps the door's teardown out of the middle of the test.
    let heading_back = |app: &App| {
        matches!(
            app.world()
                .get_resource::<NextState<emerge_mapper::screen::Screen>>(),
            Some(NextState::Pending(emerge_mapper::screen::Screen::Menu))
        )
    };

    // Something in hand — the brush is an index into the palette. The first `Esc` spends itself on
    // that, and nothing exits.
    app.world_mut().resource_mut::<EditorState>().brush = Some(0);
    tap(&mut app, KeyCode::Escape);
    assert!(
        app.world().resource::<EditorState>().brush.is_none(),
        "the selection is the layer this press peels"
    );
    assert!(
        !heading_back(&app),
        "one layer per press — clearing the brush must not also leave the map"
    );

    // Nothing left to peel, so the next one reaches the map itself — and asks rather than going.
    tap(&mut app, KeyCode::Escape);
    assert!(
        app.world().resource::<EditorState>().leaving,
        "the last layer is a question, not a departure"
    );
    assert!(
        !heading_back(&app),
        "asking is not going — leaving silently on a reflex key is what this question exists for"
    );

    // And the third answers it.
    tap(&mut app, KeyCode::Escape);
    assert!(
        heading_back(&app),
        "`Esc` at the question is yes, on a clean map where it can lose nothing"
    );
}

/// **`Cmd+O` goes back to the menu, and refuses to take unsaved work with it.**
///
/// The editor is a child process of the chooser, so "back to the menu" is an exit: the handler
/// writes `AppExit` with `chooser::BACK_TO_MENU` and `main.rs`'s loop shows the chooser again. What
/// matters here is the guard in front of it — leaving with unsaved edits would discard them with one
/// keystroke and no way back, because the undo stack does not survive the process.
///
/// A **refusal** rather than a confirmation, deliberately: it cannot lose anything, and an author
/// who genuinely wants to discard closes the window, which nobody presses by accident.
#[test]
fn the_menu_key_refuses_to_leave_unsaved_work() {
    use emerge_mapper::keys::{Action, MOD_KEYS, binding};
    use emerge_mapper::project::OpenMap;

    let root = Fixture::new("menu-key")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let chord = |app: &mut App| {
        let keys = vec![MOD_KEYS[0], binding(Action::MainMenu).key];
        app.add_systems(
            // **`PreUpdate`, after Bevy's own input pass — not `.before(Phase::Act)`.**
            //
            // The injector and `editor::answer_the_leaving_prompt` were both `.before(Phase::Act)`
            // and unordered *relative to each other*, so which ran first was arbitrary — and they
            // conflict on `ButtonInput`, so the executor picks. This test passed alone and failed in
            // the full suite for exactly that reason. Pressing after `InputSystems` (which is what
            // clears `just_pressed`) makes the press visible to **every** `Update` system, which is
            // a superset of what `.before(Act)` bought and is not a coin toss.
            PreUpdate,
            IntoScheduleConfigs::after(
                move |mut input: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
                    if !*done {
                        input.release_all();
                        for k in &keys {
                            input.press(*k);
                        }
                        *done = true;
                    }
                },
                bevy::input::InputSystems,
            ),
        );
        app.update();
    };
    let tap = |app: &mut App, key: KeyCode| {
        app.add_systems(
            // **`PreUpdate`, after Bevy's own input pass — not `.before(Phase::Act)`.**
            //
            // The injector and `editor::answer_the_leaving_prompt` were both `.before(Phase::Act)`
            // and unordered *relative to each other*, so which ran first was arbitrary — and they
            // conflict on `ButtonInput`, so the executor picks. This test passed alone and failed in
            // the full suite for exactly that reason. Pressing after `InputSystems` (which is what
            // clears `just_pressed`) makes the press visible to **every** `Update` system, which is
            // a superset of what `.before(Act)` bought and is not a coin toss.
            PreUpdate,
            IntoScheduleConfigs::after(
                move |mut input: ResMut<bevy::input::ButtonInput<KeyCode>>,
                      mut done: Local<bool>| {
                    if !*done {
                        input.release_all();
                        input.press(key);
                        *done = true;
                    }
                },
                bevy::input::InputSystems,
            ),
        );
        app.update();
    };
    // **The way back is a state change, not an exit.** It was `AppExit` with a code the parent
    // process compared against; both screens are one application now (`screen.rs`), so leaving sets
    // `Screen::Menu`. Read as *pending*: `StateTransition` runs before `Update`, so the set made by
    // this frame's key applies on the next one — and checking it here rather than stepping again
    // keeps the door's teardown out of the middle of the test.
    let heading_back = |app: &App| {
        matches!(
            app.world()
                .get_resource::<NextState<emerge_mapper::screen::Screen>>(),
            Some(NextState::Pending(emerge_mapper::screen::Screen::Menu))
        )
    };
    // **Cancel the pending departure and stay in the door.**
    //
    // This test asks the same question three ways — clean, dirty-and-discarded, saved — and each
    // answer used to write an `AppExit` message that changed nothing, so the next third could run in
    // the same world. A state change is not inert: letting it apply runs `OnExit(Editor)`, which
    // despawns the editor and drops the `Project`, and the rest of the test would then be asserting
    // about an empty world. Clearing the pending transition is the test standing still, not the
    // editor behaving differently.
    let stay = |app: &mut App| {
        app.world_mut()
            .resource_mut::<NextState<emerge_mapper::screen::Screen>>()
            .set(emerge_mapper::screen::Screen::Editor);
        *app.world_mut()
            .resource_mut::<NextState<emerge_mapper::screen::Screen>>() = NextState::Unchanged;
    };
    let leaving = |app: &App| {
        app.world()
            .resource::<emerge_mapper::editor::EditorState>()
            .leaving
    };

    // **Dirty: it must not go.**
    {
        let mut open = app.world_mut().resource_mut::<OpenMap>();
        open.map.placements.push(emerge_core::map::Placed {
            id: "floor@1".to_owned(),
            descriptor: "floor".to_owned(),
            ..Default::default()
        });
        open.dirty = true;
    }
    chord(&mut app);
    assert!(
        !heading_back(&app),
        "Cmd+O left with unsaved edits on the map — that discards work the undo stack cannot get \
         back, because it does not survive the process"
    );
    assert!(
        leaving(&app),
        "it must ASK rather than merely refuse: a refusal cannot lose anything and is also a dead \
         end, and the author still wants to leave"
    );
    let said = app
        .world()
        .resource::<emerge_mapper::editor::EditorState>()
        .status
        .problems()
        .iter()
        .map(|p| p.text.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        said.contains("unsaved")
            && said.contains(" S ")
            && said.contains('D')
            && said.contains("Esc"),
        "the question has to name every answer, per docs/ui.md §1.4; it said: {said}"
    );

    // **Esc stays, and changes nothing.** The first press of anything must never be the one that
    // loses work.
    tap(&mut app, KeyCode::Escape);
    assert!(!leaving(&app), "Esc puts the question away");
    assert!(
        !heading_back(&app),
        "and does not leave");
    assert!(
        app.world().resource::<OpenMap>().dirty,
        "nor save behind your back"
    );

    // **`D` is the one answer that discards, and it takes a key that means nothing else here.**
    chord(&mut app);
    assert!(leaving(&app), "asking again");
    tap(&mut app, KeyCode::KeyD);
    assert!(
        heading_back(&app),
        "D discards and goes — the whole reason this asks instead of refusing"
    );
    stay(&mut app);

    // **Saved: it still asks, and `Esc` is what answers yes.**
    //
    // It used to go straight out. Asked for at the keyboard on 2026-08-16 — leaving on a reflex key
    // with no question is what made the first `Esc` fix feel like the editor had crashed. Every way
    // out asks now, including the deliberate chord, because a chord and a reflex key meaning
    // different things costs an author their model of the editor.
    {
        let mut open = app.world_mut().resource_mut::<OpenMap>();
        open.dirty = false;
    }
    chord(&mut app);
    assert!(
        leaving(&app),
        "a clean map asks once before it goes — `Esc` on a reflex must not leave silently"
    );
    assert!(
        !heading_back(&app),
        "asking is not going");
    tap(&mut app, KeyCode::Escape);
    assert!(
        heading_back(&app),
        "and `Esc` at that question answers it yes — with nothing unsaved it cannot lose anything, \
         which is the whole reason it is allowed to be the confirming key here and the cancelling \
         one when the map is dirty"
    );
}

/// **The first arrow press on the Tiles tab selects the FIRST piece**, not the second.
///
/// With nothing picked, an arrow both establishes the selection and walks it — and those used to
/// be the same press: the handler seeded row 0 and then fell through into the walk, which read
/// `at = 0` and stepped to row 1. So an author arriving at the Tiles tab and pressing `down` landed
/// on the second piece in the list, and the first could only be reached by pressing `up` afterwards.
///
/// It hid behind a test that was passing for the wrong reason. `the_tile_feedback_script_can_
/// actually_be_followed` drove `Enter` without the walk its own script describes, and
/// `ImportState::editing` falls back to the focused *candidate* when nothing in the library is
/// picked — so while `import::proposed_id` qualified a candidate by its pack folder, that fallback
/// carried the id `site/floor`, which collided with a real library id and passed the drop's
/// library check. Two defects cancelling.
#[test]
fn the_first_arrow_press_lands_on_the_first_piece() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{IntoScheduleConfigs, KeyCode, ResMut, Update};
    use emerge_mapper::keys::{Action, binding};

    // Two pieces, and the assertion is about which of them a single press reaches.
    // The pack matters: with no candidates at all the tab picks the first library row on arrival,
    // and the press this test is about never happens. A real kit always has unimported meshes.
    let root = Fixture::new("first_press")
        .pack("alpha/scan", &["spare"])
        .descriptor("alpha/floor", "alpha")
        .descriptor("alpha/wall", "alpha")
        .build("test_map");
    let mut app =
        harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
            .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    assert!(
        app.world()
            .resource::<emerge_mapper::tiles::ImportState>()
            .selected_library_id
            .is_none(),
        "arriving picks nothing — that is the state this test is about"
    );

    let key = binding(Action::TileListNext).key;
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

    let picked = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .selected_library_id
        .clone();
    assert_eq!(
        picked.as_deref(),
        Some("alpha/floor"),
        "one press of `{}` must land on the FIRST piece. Landing on `alpha/wall` means the press \
         that establishes the selection also walked it, and the first row is unreachable going down.",
        binding(Action::TileListNext).chord
    );
}

/// **The whole application draws into one surface, and an agent reads that surface.**
///
/// Before this existed, `bevy_debugger/screenshot` mirrored only the *world* into a square image and
/// could never show a panel — Bevy draws a UI tree to one camera — so every question about the
/// interface fell back to a window capture, which macOS only keeps current while the window is on
/// screen. Answering "what does this panel look like" meant taking the display of whoever was at the
/// machine.
///
/// Four facts hold it together and each one has already been broken once:
///
/// 1. **The surface exists**, built in `SurfacePlugin::build` rather than in `Startup` — `bevy_state`
///    runs its transition schedule *before* the startup ones, so `OnEnter(Editor)` fires first and
///    `view::setup` panicked on a missing `Res<Surface>`.
/// 2. **Its three cameras survive a screen change.** They were swept away by a teardown that spelled
///    the "every root a screen owns" rule as its own second copy, which left the editor with no
///    camera for its interface and *nothing in the log*.
/// 3. **Exactly one camera is the default UI camera**, or Bevy warns and picks by order — and the
///    order pick only ever considers cameras rendering to the window, which this one does not.
/// 4. **The map camera renders into the same image**, so a capture carries the world as well as the
///    interface.
#[test]
fn the_application_draws_into_one_surface_an_agent_can_read() {
    let root = Fixture::new("one-surface")
        .descriptor("wall", "alpha")
        .place("wall", (0.0, 0.0))
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let surface = app
        .world()
        .get_resource::<emerge_mapper::surface::Surface>()
        .expect("the surface is how this application draws — no resource means no interface at all");
    let image = surface.image.clone();

    let mut ground = app
        .world_mut()
        .query_filtered::<(), With<emerge_mapper::surface::SurfaceGround>>();
    let mut ui = app
        .world_mut()
        .query_filtered::<(), With<emerge_mapper::surface::SurfaceCamera>>();
    let mut window = app
        .world_mut()
        .query_filtered::<(), With<emerge_mapper::surface::WindowCamera>>();
    let mut mirror = app
        .world_mut()
        .query_filtered::<(), With<emerge_mapper::surface::Mirror>>();
    assert_eq!(ground.iter(app.world()).count(), 1, "one clearing pass");
    assert_eq!(
        ui.iter(app.world()).count(),
        1,
        "the interface's camera was swept away by a teardown once, and nothing logged"
    );
    assert_eq!(window.iter(app.world()).count(), 1, "one camera on the window");
    assert_eq!(mirror.iter(app.world()).count(), 1, "one sprite carrying it");

    let mut defaults = app
        .world_mut()
        .query_filtered::<(), (With<bevy::camera::Camera>, With<bevy::ui::IsDefaultUiCamera>)>();
    assert_eq!(
        defaults.iter(app.world()).count(),
        1,
        "two would make Bevy warn and fall back to the highest-order WINDOW camera — which the \
         surface camera is not, so the interface would leave the image silently"
    );

    let mut world_cams = app
        .world_mut()
        .query_filtered::<&bevy::camera::RenderTarget, With<emerge_mapper::view::MainCamera>>();
    let target = world_cams
        .iter(app.world())
        .next()
        .cloned()
        .expect("the map camera");
    match target {
        bevy::camera::RenderTarget::Image(t) => assert_eq!(
            t.handle, image,
            "the map must render into the same surface, or a capture shows an interface floating \
             over nothing"
        ),
        other => panic!("the map camera renders to {other:?}, not to the surface"),
    }
}
