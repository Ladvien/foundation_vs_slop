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
    for c in name.chars() {
        tap_key(
            app,
            bevy::input::keyboard::Key::Character(c.to_string().into()),
            KeyCode::KeyA,
        );
    }
    tap_key(app, bevy::input::keyboard::Key::Enter, KeyCode::Enter);
    for _ in 0..2 {
        app.update();
    }
}

/// **Open a named blank tile, the way an author does** — `N`, the prompt, `Enter`. The Tiles tab
/// no longer opens a tile on arrival (the Tiles page is what arrival shows), so every test that
/// drops or nudges must open one first; this is the one path, driven through the same keys and the
/// same message stream the guide scripts use.
fn open_tile(app: &mut App, name: &str) {
    press_once(app, emerge_mapper::keys::binding(emerge_mapper::keys::Action::BuildNew).key);
    name_the_tile(app, name);
}

/// **One keystroke, the way a field really receives it** — a press and a release written to the
/// `KeyboardInput` message stream, then a frame.
///
/// Not `ButtonInput`: every text handler in this crate reads the stream and matches `logical_key`,
/// which is the distinction `bevy_debugger/input` exists to honour and the reason an agent could not
/// type into this editor until it did. Shared rather than re-closed per test — it was written out
/// twice before this, identically, which is the drift the crate's own chrome module exists against.
fn tap_key(app: &mut App, logical: bevy::input::keyboard::Key, code: KeyCode) {
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
}

/// The `KeyCode` a lowercase letter arrives on.
///
/// [`tap_key`] writes only the message stream, which every text handler in this crate matches on
/// `logical_key` — so the code is not what makes typing work. It is stated truthfully anyway: a test
/// that claimed every letter was `KeyA` would be lying about the input it drove, and the next reader
/// would believe it.
fn letter_key(c: char) -> KeyCode {
    const KEYS: [KeyCode; 26] = [
        KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF,
        KeyCode::KeyG, KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL,
        KeyCode::KeyM, KeyCode::KeyN, KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR,
        KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU, KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX,
        KeyCode::KeyY, KeyCode::KeyZ,
    ];
    match (c.to_ascii_lowercase() as u32)
        .checked_sub(u32::from(b'a'))
        .and_then(|i| KEYS.get(i as usize))
    {
        Some(k) => *k,
        None => panic!("`{c}` is not an ASCII letter — a vocabulary token is expected to be one"),
    }
}

/// **One chord, as `ButtonInput`** — for the verbs, which read that rather than the stream.
///
/// `release_all` before `press` is load-bearing: `just_pressed` needs a transition, and a key left
/// down by an earlier call would otherwise never fire again. The `Local` latch is the other half —
/// without it the key is held forever and `keys::repeating` starts auto-repeating it.
fn press_once(app: &mut App, key: KeyCode) {
    app.add_systems(
        Update,
        IntoScheduleConfigs::before(
            move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>, mut done: Local<bool>| {
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

/// **Every laid-out string inside the tag block**, headings and chips and the count line alike.
///
/// Walks up from each `Text` to see whether `ControlId::Tags` is an ancestor, which is how
/// `the_tag_axes_have_a_block_to_stand_in` asks the same question — the block is a subtree, not a
/// marker on each leaf. Zero-sized nodes are dropped: a chip that is spawned and laid out at nothing
/// is invisible, and a test that counted it would pass over an empty pane.
fn tag_block_text(app: &mut App) -> Vec<String> {
    use bevy::ui::ComputedNode;

    let block = {
        let mut q = app
            .world_mut()
            .query::<(Entity, &emerge_mapper::chrome::Control)>();
        q.iter(app.world())
            .find(|(_, c)| c.0 == emerge_mapper::keys::ControlId::Tags)
            .map(|(e, _)| e)
    };
    let Some(block) = block else {
        panic!("the detail pane draws no `ControlId::Tags` node at all");
    };
    let mut q = app.world_mut().query::<(Entity, &Text, &ComputedNode)>();
    let found: Vec<(Entity, String)> = q
        .iter(app.world())
        .filter(|(_, _, n)| n.size() != Vec2::ZERO)
        .map(|(e, t, _)| (e, t.0.clone()))
        .collect();
    let world = app.world();
    found
        .into_iter()
        .filter(|(e, _)| {
            let mut up = Some(*e);
            while let Some(x) = up {
                if x == block {
                    return true;
                }
                up = world.get::<ChildOf>(x).map(|p| p.parent());
            }
            false
        })
        .map(|(_, t)| t)
        .collect()
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
        Action, Context, Holder, Live, MOD_KEYS, REMOVE_KEY, Stance, binding, just_pressed,
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
        Live(Context::Meshes, Stance::Idle, Holder::Tab),
        Action::RemoveTile
    ));
    assert!(!just_pressed(
        &input,
        Live(Context::Meshes, Stance::Idle, Holder::Tab),
        Action::EditTile
    ));

    // A FRESH input, not `clear()`: `clear` keeps the pressed state, so an already-held key never
    // re-registers as just-pressed.
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(MOD_KEYS[0]);
    input.press(REMOVE_KEY);
    assert!(just_pressed(
        &input,
        Live(Context::Map, Stance::Idle, Holder::Tab),
        Action::EditTile
    ));
    assert!(
        !just_pressed(
            &input,
            Live(Context::Meshes, Stance::Idle, Holder::Tab),
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
    /// **The subject is the kit `assets/emerge/kits.ron` declares as `authoring`** — the one the
    /// editor opens when nobody says otherwise, which is `furniture`. It used to be `site`, and that
    /// is the whole lesson: `assets/emerge/site/` was **shared with the game**
    /// (`src/site/kit.rs::SITE_PROJECT_DIR`), so when it was cleared on 2026-08-15 to make a blank
    /// slate, 32 game tests went down with it while this suite stayed green. It was cleared again on
    /// 2026-08-16 and this time for good — `kits.ron` says so in its own note, *"the kit was cleared
    /// … and is being re-authored"* — so a test naming it was pinned to a directory the project had
    /// deliberately stopped shipping.
    ///
    /// **A piece count is the cheap alarm** for a kit being emptied, which is why this asserts the
    /// library is populated rather than merely loadable.
    #[test]
    fn the_editor_boots_on_the_shipped_kit() {
        // `None`, because the subject named above is the `authoring` field rather than a directory:
        // `Project::open(root, None)` resolves it, so this follows the declaration wherever it goes.
        let mut app = harness::build_headless_at(&root(), "untitled_map", None, emerge_mapper::tiles::Mode::Meshes)
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
            "the kit `assets/emerge/kits.ron` names as `authoring` is empty, so the editor opens on \
             nothing. A blank slate belongs in a kit of its own — bind it in `kits.ron` and author \
             there rather than emptying the one every default open lands in."
        );
        // The kit's own configuration is the project rather than the content.
        assert_eq!(project.lattice.face_bands, 1);
    }

    /// **The way out is chrome, and there is exactly one of it.**
    ///
    /// Asked for at the keyboard twice. First: *"when we go into the map editor, we actually need a
    /// button to go back to the main UI."* There was only `Cmd+O`, and a key nothing on screen
    /// mentions is a key nobody finds. A button was added to each tab's panel — and it was reported
    /// missing **again**: *"When I enter kit editing, there's no clear way to get back to the main
    /// menu."*
    ///
    /// `docs/2026-08-17-one-application.md` §3 found the reason, and it was not contrast: drawn
    /// inside a panel on inspector ground it read as that panel's content, and nothing at window
    /// level was navigation at all. So this test **inverted**. It used to demand one back button per
    /// tab, `found.len() >= 4`, and four copies was the defect rather than the feature: a way out
    /// each panel places is a way out each panel can forget, and the Rigs door drew it somewhere
    /// else from the Map door.
    ///
    /// Two things are asserted. **Exactly one**, so a panel that grows its own again fails here. And
    /// that it is **pickable**: the frame is `Pickable::IGNORE` so the world stays reachable through
    /// it, and a button inheriting that would look exactly like a working one and answer no clicks.
    #[test]
    fn the_way_out_is_chrome_and_there_is_one_of_it() {
        let root = Fixture::new("one-way-out")
            .descriptor("wall", "alpha")
            .place("wall", (0.0, 0.0))
            .build("test_map");
        let mut app = harness::build_headless(&root, "test_map", None)
            .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
        for _ in 0..4 {
            app.update();
        }

        let chrome_bar = app
            .world()
            .get_resource::<emerge_mapper::chrome::Frame>()
            .map(|f| f.chrome_bar)
            .expect("the frame owns the chrome bar the way out lives on");

        let mut q = app.world_mut().query::<(
            bevy::ecs::entity::Entity,
            &emerge_mapper::chrome::BackButton,
            &bevy::picking::Pickable,
        )>();
        let found: Vec<_> = q
            .iter(app.world())
            .map(|(e, _, p)| (e, p.should_block_lower || p.is_hoverable))
            .collect();
        assert_eq!(
            found.len(),
            1,
            "the way out is chrome, not panel furniture — found {} of them, which is the shape that \
             got it reported missing twice",
            found.len()
        );
        assert!(
            found[0].1,
            "a back button inheriting the frame's `Pickable::IGNORE` answers no clicks"
        );

        let parent = app
            .world()
            .get::<bevy::ecs::hierarchy::ChildOf>(found[0].0)
            .map(|c| c.parent());
        assert_eq!(
            parent,
            Some(chrome_bar),
            "it belongs to the chrome bar. Inside a panel it reads as that panel's content, which \
             is exactly why nobody found it."
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

    /// **An authored lattice survives the disk round-trip**, edge tokens and all.
    ///
    /// The end of the load chain, pinned: descriptors on disk, policy layered over them, lattice
    /// validated, and a **hand-authored** subgrid still intact in front of an author. The derivation
    /// *door* is covered on both sides by `derived_edges_refuse_an_undeclared_token_and_say_which`
    /// and `derived_edges_land_once_the_project_declares_them`; that an already-authored lattice
    /// comes back out of the file has no other guard.
    ///
    /// # It was an asset-contract test twice, and lost its subject twice
    ///
    /// It read the shipped `site/wall` — the one piece whose lattice was hand-authored rather than
    /// derived — because `Fixture` wrote `subgrid: None` on every descriptor and so could assert
    /// nothing. On 2026-08-15 that kit was emptied and this test was deleted; the kit came back and
    /// it did with it. On 2026-08-16 the kit was cleared for good (`kits.ron`: *"being
    /// re-authored"*), and pinning a loader invariant to a corpus that has now moved out from under
    /// it twice is the fixture rule's whole argument, made twice.
    ///
    /// So `Fixture::authored_lattice` writes one, and the round-trip is still real: the fixture
    /// emits RON, `policy::layered_library` reads it, and those are not the same code.
    #[test]
    fn the_authored_edge_tokens_reach_the_editor() {
        // Four cells wide, so the piece has exactly four divisions across and a lattice that fills
        // its run face is legal. `wall` is the edge token the fixture's vocabulary already declares.
        let root = Fixture::new("authored-lattice")
            .authored_lattice("wall", "alpha", "wall", 4)
            .build("m");
        let mut app =
            harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
                .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..4 {
            app.update();
        }
        let project = app
            .world()
            .get_resource::<emerge_mapper::project::Project>()
            .unwrap_or_else(|| panic!("the project resource is gone"));

        let wall = project
            .library
            .get("wall")
            .unwrap_or_else(|| panic!("`wall` is not in the layered library"));
        let subgrid = wall
            .subgrid
            .as_ref()
            .unwrap_or_else(|| panic!("`wall` reached the editor with no authored subgrid"));

        let edged: Vec<&emerge_core::descriptor::SubCell> = subgrid
            .cells
            .iter()
            .filter(|c| c.edge.as_deref() == Some("wall"))
            .collect();
        assert_eq!(
            edged.len(),
            4,
            "four authored `wall` cells went to disk and the layered library handed the editor {}. \
             An authored lattice that does not survive the round-trip is a wall that stops sealing \
             rooms, and nothing else in this suite would notice.",
            edged.len()
        );
        // **Which cells, not only how many.** `len == 4` beside `at.1 == 0 && at.2 == 0` is
        // satisfied by four cells collapsed onto `(0, 0, 0)` — which is exactly what a round-trip
        // that lost the `at` tuple would hand back, and the failure this test is named for. A run is
        // a *run*: four distinct steps along x.
        let mut across: Vec<u32> = edged.iter().map(|c| c.at.0).collect();
        across.sort_unstable();
        assert_eq!(
            across,
            vec![0, 1, 2, 3],
            "the authored cells came back off their own positions: {:?}",
            edged.iter().map(|c| c.at).collect::<Vec<_>>()
        );
        // All on one face — the run — which is what makes them a *run* face rather than a scatter.
        assert!(
            edged.iter().all(|c| c.at.1 == 0 && c.at.2 == 0),
            "the authored cells came back off their own face: {:?}",
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
        // **The chevron says it, and nothing else does.** A folded header used to add the
        // sentence "{n} hidden — click to open"; removed at the keyboard 2026-08-18, because it
        // is a whole sentence on every folded row of a list an author is scrolling. The `>`
        // assertion above is what now carries "folded is distinguishable from gone" — this pins
        // the sentence staying gone, so a future edit has to mean it.
        assert!(
            !texts.iter().any(|t| t.contains("hidden — click to open")),
            "the folded-header sentence was removed on purpose — the chevron says it instead"
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

/// **A map's bash narrows the palette, and cannot narrow it into a lie.**
///
/// The other half of naming a combination: leaving a kit out has to actually stop offering its
/// pieces, and leaving out a kit the map is *standing on* must not hide the rows that describe what
/// is already there. `OpenMap::palette_namespaces` folds the in-use set back in for exactly that,
/// which is what lets a bash be a filter rather than a decision with consequences.
///
/// **The library is untouched either way.** Every bound kit still loads, so a placement always
/// resolves and a composition may still seat two kits' pieces — this is a filter on what an author
/// is *offered*, never on what a map can mean.
#[test]
fn the_maps_bash_narrows_the_palette_but_never_hides_what_is_placed() {
    use emerge_mapper::editor::{palette_indices, Folded};
    use emerge_mapper::filter::Filters;
    use emerge_mapper::project::{OpenMap, Project};

    let root = Fixture::new("map-bash")
        .descriptor("bench", "props")
        .kit("site", "ozea", &["site/wall"])
        // Two combinations, each leaving the other kit out — one namespaced, one flat.
        .bash("only_furniture", &["furniture"])
        .bash("only_site", &["site"])
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    app.update();

    let offered = |app: &mut App| -> Vec<String> {
        let world = app.world_mut();
        let project = world.resource::<Project>();
        let open = world.resource::<OpenMap>();
        let fold = world.resource::<Folded>();
        let filters = world.resource::<Filters>();
        palette_indices(project, open, fold, filters)
            .into_iter()
            .filter_map(|i| project.library.descriptors.get(i).map(|d| d.id.clone()))
            .collect()
    };

    // No bash named means every bound kit offered.
    let all = offered(&mut app);
    assert!(
        all.iter().any(|id| id == "bench") && all.iter().any(|id| id == "site/wall"),
        "a map naming no bash offers every bound kit: {all:?}"
    );

    // Name one that leaves `site` out: its pieces stop being offered, the furniture kit's do not.
    app.world_mut().resource_mut::<OpenMap>().map.bash = Some("only_furniture".to_owned());
    app.update();
    let narrowed = offered(&mut app);
    assert!(
        narrowed.iter().any(|id| id == "bench"),
        "the kit the bash names keeps its rows: {narrowed:?}"
    );
    assert!(
        !narrowed.iter().any(|id| id == "site/wall"),
        "and the one it leaves out loses them: {narrowed:?}"
    );
    // **But the library still has it**, which is why a placement cannot be stranded.
    assert!(
        app.world().resource::<Project>().library.get("site/wall").is_some(),
        "a bash filters the palette, never what the map can resolve"
    );

    // **Now the other direction, which is where this was broken.**
    //
    // Leaving out the kit whose ids are **flat** — the shape every shipped kit has. The first
    // version read the namespace out of the id, so `bench` belonged to no kit, matched no
    // selection, and was offered whatever was named: the control was inert on the only project
    // that matters and the test above still passed, because it only ever left out a *namespaced*
    // kit. Driving the shipped project is what found it.
    app.world_mut().resource_mut::<OpenMap>().map.bash = Some("only_site".to_owned());
    app.update();
    let flat_off = offered(&mut app);
    assert!(
        !flat_off.iter().any(|id| id == "bench"),
        "a kit with flat ids is left out like any other — `Project::kit_of` asks which library \
         defines a piece, never what its id spells: {flat_off:?}"
    );
    assert!(
        flat_off.iter().any(|id| id == "site/wall"),
        "and the one the bash names keeps its rows: {flat_off:?}"
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
        "a placed flat piece keeps its palette row whatever the bash says"
    );
    {
        let mut open = app.world_mut().resource_mut::<OpenMap>();
        open.map.placements.clear();
        open.map.bash = Some("only_furniture".to_owned());
    }
    app.update();

    // Now place one of its pieces and the row comes back, still with `site` out of the bash.
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
        "a kit the map stands on is offered whatever the bash says — otherwise the author \
         cannot find, match or re-place the pieces in front of them: {with_placement:?}"
    );
}

/// **The menu's columns run left to right in the order the data model nests: PROJECTS, KITS, MAPS.**
///
/// `PROJECT ||--o{ KIT` and `PROJECT ||--o{ MAP` (the ERD in `CLAUDE.md`) make the project the root,
/// and the one cross-edge between its children runs `MAP → BASH → KIT` — a map is made of kits, so
/// kits come first and the map is what they add up to.
///
/// **Read off the spawned hierarchy, not off `render()`.** `Screen` carries rows and headers and
/// says nothing about geometry, so the flat rendering cannot see a column order at all — the only
/// place it exists is the child order under `chrome::Frame::body`, which is what this walks. That
/// is also the half a green suite missed for as long as it did: `Focus::ALL` walked
/// projects→kits→policy→maps→settings while the columns read `MAPS | KITS | PROJECTS`, so one `Tab`
/// meant right, then left, then right, and every test still passed.
#[test]
fn the_menu_columns_run_project_then_kits_then_maps() {
    let root = Fixture::new("column-order")
        .descriptor("bench", "props")
        .kit("site", "ozea", &["site/wall"])
        .build("m");

    let mut app = harness::build_headless(&root, "m", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    // **Into the menu.** The harness opens straight into a door, because that is the entry point it
    // exists to serve; the chooser is the other screen and its plugin is added here.
    app.add_plugins(emerge_mapper::chooser::ChooserPlugin {
        root: root.clone(),
        preselect: None,
    });
    app.world_mut()
        .resource_mut::<NextState<emerge_mapper::screen::Screen>>()
        .set(emerge_mapper::screen::Screen::Menu);
    for _ in 0..4 {
        app.update();
    }

    let body = app.world().resource::<emerge_mapper::chrome::Frame>().body;
    // The first `Text` in a column's subtree is its list heading — `spawn_screen` spawns the header
    // before the list under each panel.
    let first_text = |world: &World, at: Entity| -> Option<String> {
        let mut stack = vec![at];
        while let Some(e) = stack.pop() {
            if let Some(t) = world.get::<Text>(e) {
                return Some(t.0.clone());
            }
            if let Some(children) = world.get::<Children>(e) {
                for c in children.iter().rev() {
                    stack.push(c);
                }
            }
        }
        None
    };
    let world = app.world();
    let columns: Vec<String> = world
        .get::<Children>(body)
        .map(|c| c.iter().collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|col| first_text(world, col))
        .collect();

    assert_eq!(
        columns,
        vec!["PROJECTS".to_owned(), "KITS".to_owned(), "MAPS".to_owned()],
        "the columns are the hierarchy, left to right"
    );

    // **And `left`/`right` walk them in that order**, one press per column, so the key and the
    // layout cannot come to disagree again. There is no `Tab`: the rest of the editor navigates
    // with arrows and nothing else — see `Focus`.
    use emerge_mapper::chooser::Focus;
    let mut chooser = app.world_mut().resource_mut::<emerge_mapper::chooser::Chooser>();
    chooser.focus = Focus::Projects;
    let mut walk = vec![chooser.focus];
    for _ in 0..3 {
        chooser.cross(1);
        walk.push(chooser.focus);
    }
    assert_eq!(
        walk,
        vec![Focus::Projects, Focus::Kits, Focus::Maps, Focus::Projects],
        "one press per column, in the order they are drawn, wrapping over the three"
    );
}

/// **A map naming a bash the project does not declare is refused by name at the door.**
///
/// Not resolved to every-kit, which is the only other thing it could do and is a palette nobody
/// chose. `Map::validate` cannot make this call — a map validates in isolation and never sees
/// `kits.ron` — so `OpenMap::open` is the one place both halves are in hand, and the refusal names
/// what was declared so the next keystroke is obvious.
#[test]
fn a_map_naming_an_undeclared_bash_will_not_open() {
    use emerge_mapper::project::{OpenMap, Project};

    let root = Fixture::new("bash-undeclared")
        .descriptor("bench", "props")
        .bash("hub", &["furniture"])
        .build("m");
    let maps = root.join("assets/emerge/maps");
    std::fs::write(
        maps.join("wrong.map.ron"),
        "(version: 6, name: \"wrong\", origin: (0.0, 0.0, 0.0), bounds: (8.0, 3.0, 8.0), \
         bash: Some(\"nope\"), placements: [], stamps: [], locations: [])",
    )
    .unwrap_or_else(|e| panic!("{e}"));

    let project = Project::open(&root, None).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        project.bashes.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
        vec!["hub"],
        "the project carries what `kits.ron` declares"
    );

    let e = OpenMap::open(&project, "wrong")
        .err()
        .unwrap_or_else(|| panic!("an undeclared name must not open"));
    assert!(e.contains("nope"), "the refusal names what the map asked for: {e}");
    assert!(e.contains("Declared: hub"), "and what it could have asked for: {e}");

    // The map that names a declared one opens, so the refusal is about the name and nothing else.
    std::fs::write(
        maps.join("right.map.ron"),
        "(version: 6, name: \"right\", origin: (0.0, 0.0, 0.0), bounds: (8.0, 3.0, 8.0), \
         bash: Some(\"hub\"), placements: [], stamps: [], locations: [])",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let open = OpenMap::open(&project, "right").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(open.map.bash.as_deref(), Some("hub"));
}

/// **A bash cannot strand a placement**, which is the guarantee the whole design rests on.
///
/// A map may name a combination that leaves out a kit whose pieces are already on it — by being
/// given the bash after the fact, or by the bash being edited in `kits.ron` afterwards. Every bound
/// kit loads either way, so the placement still resolves and still draws; what would break is the
/// palette, which would stop offering the rows that describe what the author is looking at.
/// `OpenMap::palette_namespaces` folds the in-use set back in, so it cannot.
#[test]
fn a_bash_that_leaves_out_a_placed_kit_still_offers_it() {
    use emerge_mapper::project::{OpenMap, Project};

    let root = Fixture::new("bash-strand")
        .descriptor("bench", "props")
        .kit("a", "packa", &["a/one"])
        .kit("b", "packb", &["b/two"])
        .bash("only_a", &["a"])
        .place("b/two", (2.0, 2.0))
        .build("m");

    let project = Project::open(&root, None).unwrap_or_else(|e| panic!("{e}"));
    let mut open = OpenMap::open(&project, "m").unwrap_or_else(|e| panic!("{e}"));

    // Naming nothing offers every bound kit, which is where a map starts.
    assert_eq!(
        open.palette_namespaces(&project).into_iter().collect::<Vec<_>>(),
        vec!["a".to_owned(), "b".to_owned(), "furniture".to_owned()]
    );

    open.map.bash = Some("only_a".to_owned());
    assert_eq!(
        open.palette_namespaces(&project).into_iter().collect::<Vec<_>>(),
        vec!["a".to_owned(), "b".to_owned()],
        "`only_a` names one kit and the map stands on the other, so both are offered — \
         `furniture`, which it neither names nor stands on, is not"
    );
    assert!(
        project.library.get("b/two").is_some(),
        "and the library is untouched: a bash filters the palette, never what a map can resolve"
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

    /// **The preview places where the commit will land.** `drive_stamp_ghost` and the commit used to
    /// disagree: the drop seated a stamped set through `stack::resolve_y` while the ghost spawned
    /// its rows at the authored `lift`, so a sinking member previewed on the floor and then landed
    /// 6 cm down. `stamped_heights` is the single owner both read — this pins what it answers for a
    /// sunk member, and that the authored lift is not it.
    #[test]
    fn a_stamp_previews_at_the_height_it_will_land_at() {
        use emerge_mapper::compose::ComposeState;
        use emerge_mapper::project::{OpenMap, Project};

        let root = Fixture::new("stamp-sunk")
            .sunk_descriptor("floor", "alpha", -0.06)
            .bounded_composition("room", (2.0, 2.0, 1.0), &[("floor", "floor", (0.0, 0.0))])
            .build("m");
        let mut app = emerge_mapper::harness::build_headless(&root, "m", None)
            .unwrap_or_else(|e| panic!("{e}"));
        for _ in 0..3 {
            app.update();
        }

        // Through the same call the click makes.
        {
            let world = app.world_mut();
            world.resource_scope(|world, mut project: bevy::prelude::Mut<Project>| {
                world.resource_scope(|world, mut open: bevy::prelude::Mut<OpenMap>| {
                    let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                    let mut compose = ComposeState {
                        armed: Some("room".to_owned()),
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

        let project = app.world().resource::<Project>();
        let open = app.world().resource::<OpenMap>();
        assert_eq!(open.map.stamps.len(), 1, "no stamp landed");
        let expansion = emerge_core::composition::expand(
            &open.map,
            &open.map.stamps,
            &project.compositions.compositions,
            &project.library,
        )
        .unwrap_or_else(|e| panic!("the stamped set must expand: {e}"));
        assert_eq!(
            expansion.placements[0].lift, 0.0,
            "the authored lift is what the preview used to draw — and it is not where the piece lands"
        );
        let ys = emerge_mapper::editor::stamped_heights(&open.map, &project.library, &expansion)
            .unwrap_or_else(|e| panic!("the stamped rows must resolve: {e}"));
        assert_eq!(ys.len(), 1, "one member, one height");
        assert!(
            (ys[0] + 0.06).abs() < 1e-4,
            "the sunk member must seat 6 cm below the floor, where the drop puts it: {ys:?}"
        );
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

        // Delete, through the call the click makes — and with nothing touching `Project`, because
        // `drive_removal` does not. This scope used to take a `Mut<Project>` it never used, which
        // marked the resource changed and let the redraw fire for a reason the app has not got.
        {
            let world = app.world_mut();
            world.resource_scope(|world, mut open: bevy::prelude::Mut<OpenMap>| {
                let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
                emerge_mapper::editor::delete_stamp_for_test(&stamp_id, &mut open, &mut state);
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

/// **The open journal is UI, and it is opaque — so the pointer gates have to see it.**
///
/// It is a solid 80%x76% panel over the viewport, and it carried `Pickable::IGNORE` and no
/// `Hovered` at all. Both ways of asking *"is the pointer on the interface"* look only at the nodes
/// that carry one — `view::over_ui` filters the query, the mouse verbs read a true value — so with
/// the journal open, a click on it stamped or removed a placement on the map **behind** it, unseen
/// under a solid panel, and the wheel zoomed the world instead of scrolling this list.
///
/// The same pair `the_open_name_box_answers_the_over_ui_question` pins one panel along, and the same
/// pair `chrome::panel_root`'s own note records from when only the *rows* carried `Hovered`.
///
/// Asked of the rects rather than of `Hovered`'s value, for the reason
/// `the_pointer_is_over_the_panel_when_it_is_over_a_row` gives: `bevy_picking` writes that value
/// from the window's cursor, which no headless test has.
#[test]
fn the_open_journal_answers_the_over_ui_question() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use emerge_mapper::keys::{Action, MOD_KEYS, binding};

    let root = Fixture::new("journalui")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    // The panel's own rect, whether it is showing or not — `over_ui` is fed every node carrying
    // `Hovered`, so this is the entity the question now reaches.
    let panel_rect = |app: &mut App| -> Option<(Vec2, Vec2)> {
        let mut q = app
            .world_mut()
            .query_filtered::<(&ComputedNode, &UiGlobalTransform), (
                bevy::prelude::With<emerge_mapper::chrome::JournalPanel>,
                bevy::prelude::With<bevy::picking::hover::Hovered>,
            )>();
        q.iter(app.world())
            .map(|(n, tf)| (n.size(), tf.translation))
            .next()
    };
    assert!(
        panel_rect(&mut app).is_some(),
        "the journal panel carries no `Hovered`, so every over-UI gate reads the open journal as \
         open world — a click on it lands on the map underneath"
    );

    // **Shut, it is a zero rect**, which both the picking backend and `over_ui` skip. That is what
    // makes carrying `Hovered` safe on a panel that is absent from almost every frame.
    let over = |app: &mut App, at: Vec2| -> bool {
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
        emerge_mapper::view::over_ui(Some(at), 1.0, borrowed.iter().copied())
    };
    let (shut_size, _) = panel_rect(&mut app).unwrap_or_else(|| panic!("the panel must exist"));
    assert_eq!(
        shut_size,
        Vec2::ZERO,
        "a `Display::None` journal must occupy nothing, or it answers for the window while away"
    );

    // Cmd+E, the key an author presses.
    let once = |app: &mut App, chord: Vec<KeyCode>| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
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
    once(&mut app, vec![MOD_KEYS[0], binding(Action::ShowErrors).key]);
    for _ in 0..3 {
        app.update();
    }

    let (size, centre) =
        panel_rect(&mut app).unwrap_or_else(|| panic!("the panel must still exist"));
    assert!(
        size.x > 1.0 && size.y > 1.0,
        "the journal did not open, so this test would prove nothing — it measures {size:?}"
    );
    assert!(
        over(&mut app, centre),
        "a pointer on the open journal's own centre must read as over the interface — it is a \
         solid panel, and a click falling through it edits the map nobody can see"
    );
    assert!(
        !over(&mut app, Vec2::new(-5000.0, -5000.0)),
        "and a pointer nowhere near a panel is still the world"
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
    // The tab opens on the Tiles page — name a tile into existence before any drop.
    open_tile(&mut app, "tile");

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

    let before = match &app.world().resource::<emerge_mapper::build::Build>().open {
        Some(c) => match c.envelope {
            emerge_core::composition::Envelope::Bounded { size } => size,
            _ => panic!("a tile claims a tile"),
        },
        None => panic!("a tile must be open before a drop"),
    };
    assert_eq!(
        (before.0, before.2),
        (emerge_core::grid::TILE, emerge_core::grid::TILE),
        "an empty tile is one cell"
    );

    // **Enter is the drop AND the hold** — `Space` arms, but `Enter` is `Idle`-scoped, so the
    // old arm-then-drop pair is refused by the census. The drop alone arms (`placing = true`).
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
    // The tab opens on the Tiles page — a tile must be named into existence before any drop.
    open_tile(&mut app, "tile");

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

    // **Enter is the drop AND the hold** — `Space` arms, but `Enter` is `Idle`-scoped, so the
    // old arm-then-drop pair is refused by the census. The drop alone arms.
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
        "one stop at the ladder is a third of the span, not the edge — got {nudged:?}"
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

    // The tile did not grow to hold it. Flush is the extreme position that still fits, so an
    // envelope that fits its contents must stay exactly one cell — a grow here would mean the verb
    // had overshot the edge it was aiming at.
    let size = match &app.world().resource::<emerge_mapper::build::Build>().open {
        Some(c) => match c.envelope {
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

    // The tab opens on the Tiles page now — no tile in hand until one is named.
    open_tile(&mut app, "tile");

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
    // **Which piece went in first, asked rather than assumed.** It used to be inferred from
    // `two[0]` — that `Composition::members` comes back in drop order — which held only while the
    // library list happened to hand the arrows the pieces in the order the fixture defined them.
    // The 2026-08-20 reversal (newest-defined first) broke that coincidence and this test with it,
    // which is the assumption being removed rather than re-encoded.
    let first_in = members(&app);
    assert_eq!(first_in.len(), 1, "one mesh is in: {first_in:?}");
    // A different piece, so the two steps are distinguishable by name rather than by count alone.
    // **Drop is `Idle`-scoped now** — Enter on the Tiles page is `TileOpen`, so a drop while
    // holding (after `Space`) is refused by stance, which is the census's rule. The honest loop:
    // release with `Esc`, walk to the next row at Idle, drop again.
    once(&mut app, vec![binding(Action::Cancel).key]);
    once(&mut app, vec![binding(Action::BuildBack).key]);
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
        one, first_in,
        "and it is the FIRST DROP that survives, not whichever the member list happens to hold at \
         index 0"
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

/// **A refit on another tab is not an edit, so it costs the tile's history nothing.**
///
/// `refit_tile` lost its `*mode != Mode::Tiles` gate deliberately — its own note gives the reason,
/// reported from the keyboard: *"the sizing of the tile around the mesh doesn't take place until you
/// enter the mesh or the tile editing… we want this to happen whenever a mesh gets loaded."* The
/// envelope is read off the contents, so which panel an author happens to be looking at cannot be
/// part of the answer. Removing the gate exposed two bugs it had been masking, and this drives the
/// one no other test can reach.
///
/// `tile_history` is still asleep behind its own Tiles gate, so a measurement landing while the
/// author is on Meshes grows `build.open.envelope` unobserved. `Composition`'s `PartialEq` covers
/// that field and `adjusted_member` deliberately does not — *"the envelope is deliberately not
/// compared"* — so on returning to Tiles the recorder saw a difference, could not classify it as a
/// continuing run, pushed a step nobody took and called `history.future.clear()`. Undo then restored
/// the pre-refit size, which refit again next frame and pushed again: **undo never advanced, and
/// redo was gone for good.**
///
/// # Why no existing test could reach it
///
/// Every other undo test on this tab stays within one tab, and the schedule pins
/// `build_keys → refit_tile → tile_history` inside a single frame — so `seen` and `open` are already
/// envelope-consistent by the time the recorder looks, and the branch is never entered. The
/// excursion is the whole point: leave Tiles, let a measurement land, come back.
///
/// `TileHistory`'s fields are private to `build.rs`, so both halves are asserted through what they
/// do. `past` being untouched is an undo that takes a **real** step back rather than restoring the
/// same members at their old size; `future` surviving is a redo that still has somewhere to go.
#[test]
fn a_refit_on_another_tab_leaves_the_tile_history_alone() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("refit_history")
        .descriptor("floor", "alpha")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();
    open_tile(&mut app, "tile");

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
    // **Released, then stepped.** A latched press stays down, and `keys::repeating` would auto-repeat
    // an `Undo` held across idle frames — which is exactly what these frames are for.
    fn settle(app: &mut App, frames: usize) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release_all();
        for _ in 0..frames {
            app.update();
        }
    }
    let members = |app: &App| -> Vec<String> {
        app.world()
            .resource::<emerge_mapper::build::Build>()
            .open
            .as_ref()
            .map(|c| c.members.iter().map(|m| m.id.clone()).collect())
            .unwrap_or_default()
    };
    let envelope = |app: &App| -> (f32, f32, f32) {
        match app
            .world()
            .resource::<emerge_mapper::build::Build>()
            .open
            .as_ref()
            .map(|c| c.envelope)
        {
            Some(emerge_core::composition::Envelope::Bounded { size }) => size,
            other => panic!("the open tile must claim a tile; it holds {other:?}"),
        }
    };

    // Two drops, so there is a real step to walk back to *and* a real one to walk forward to. The
    // release-and-walk between them is the honest loop — `Enter` is `Idle`-scoped, so a drop while
    // holding is refused by stance.
    once(&mut app, vec![binding(Action::BuildArm).key]);
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    once(&mut app, vec![binding(Action::Cancel).key]);
    once(&mut app, vec![binding(Action::BuildBack).key]);
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    let two = members(&app);
    assert_eq!(two.len(), 2, "two meshes are in the tile: {two:?}");

    // One step back, so `future` is carrying an entry the excursion could destroy.
    once(
        &mut app,
        vec![KeyCode::SuperLeft, binding(Action::UndoBuild).key],
    );
    settle(&mut app, 2);
    let one = members(&app);
    assert_eq!(one.len(), 1, "one undo takes the second mesh back out: {one:?}");

    // **Off to Meshes, where `tile_history` is asleep and `refit_tile` is not.**
    *app.world_mut().resource_mut::<emerge_mapper::tiles::Mode>() =
        emerge_mapper::tiles::Mode::Meshes;
    settle(&mut app, 2);
    let before = envelope(&app);

    // **A measurement lands.** `fit_envelope` measures a member through `library.get(id)`, so this
    // is the case `refit_tile`'s own note describes: a piece whose footprint was not known yet spans
    // nothing and the tile fits to one cell, and the tile has to grow when the number arrives. Both
    // lists, because `Project` carries the measured layer and the merged one and `refit` reads the
    // merge.
    {
        let member = one
            .first()
            .cloned()
            .unwrap_or_else(|| panic!("a member must survive the undo"));
        let mut project = app
            .world_mut()
            .resource_mut::<emerge_mapper::project::Project>();
        let project = &mut *project;
        let mut touched = 0;
        for list in [
            &mut project.measured.descriptors,
            &mut project.library.descriptors,
        ] {
            for d in list.iter_mut().filter(|d| d.id == member) {
                d.extent.footprint = Some((3.0, 3.0));
                touched += 1;
            }
        }
        assert!(touched > 0, "`{member}` must be in one of the two lists");
    }
    settle(&mut app, 4);
    let grown = envelope(&app);
    assert!(
        grown.0 > before.0 && grown.2 > before.2,
        "the measurement did not grow the envelope ({before:?} -> {grown:?}), so nothing below is \
         about a refit at all"
    );
    assert_eq!(
        members(&app),
        one,
        "and it moved no member — a refit is a derived size, not an edit"
    );

    // Back to Tiles. These are the frames the recorder wakes up on and sees a difference it did not
    // watch happen: with the fix it adopts the new envelope, without it pushes a phantom step and
    // clears `future`.
    *app.world_mut().resource_mut::<emerge_mapper::tiles::Mode>() =
        emerge_mapper::tiles::Mode::Tiles;
    settle(&mut app, 2);

    // **`past` was not topped with a phantom.** One undo empties the tile, which is the step the
    // author actually took. Before the fix the top of `past` held the pre-refit tile — the same
    // members at their old size — so this press restored what was already on screen, refit it again,
    // pushed again, and undo never advanced however many times it was pressed.
    once(
        &mut app,
        vec![KeyCode::SuperLeft, binding(Action::UndoBuild).key],
    );
    settle(&mut app, 2);
    assert!(
        members(&app).is_empty(),
        "one undo after a refit on another tab must step back a real edit; the tile still holds {:?}",
        members(&app)
    );

    // **`future` was not cleared.** Both redos land, and each one is an entry the phantom step's
    // `history.future.clear()` used to destroy.
    once(
        &mut app,
        vec![
            KeyCode::SuperLeft,
            KeyCode::ShiftLeft,
            binding(Action::RedoBuild).key,
        ],
    );
    settle(&mut app, 2);
    assert_eq!(
        members(&app),
        one,
        "the first redo puts the first mesh back — `future` still held it"
    );
    once(
        &mut app,
        vec![
            KeyCode::SuperLeft,
            KeyCode::ShiftLeft,
            binding(Action::RedoBuild).key,
        ],
    );
    settle(&mut app, 2);
    assert_eq!(
        members(&app),
        two,
        "and the second redo puts the second one back — the entry a recorded refit would have thrown \
         away"
    );
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
    // The tab opens on the Tiles page — name the tile into existence, which lands on the Meshes
    // page ready for drops. The tile is named here, so `Cmd+S` writes under this id directly.
    open_tile(&mut app, "tile_1");

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
    // **Each drop is a hold.** `Enter` drops the picked mesh and leaves it in hand (`placing`),
    // `Esc` puts it back, `down` walks the library at Idle — so the honest rhythm is
    // drop / release / walk / drop.
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    once(&mut app, vec![binding(Action::Cancel).key]);
    once(&mut app, vec![binding(Action::TileListNext).key]);
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    // The wall lands centred like everything else, then moves — which is the model: bring it in,
    // then adjust it.
    once(&mut app, vec![binding(Action::BuildBack).key]);
    once(&mut app, vec![binding(Action::BuildUp).key]);
    // The slot is its own verb: `Shift+Enter`, Idle-scoped like the drop — release the wall first.
    once(&mut app, vec![binding(Action::Cancel).key]);
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
    // The tile was named when it was opened (`open_tile` above), so the save writes under that id
    // directly — no name prompt.
    // **Refusals only.** The status also carries the size notice — a tile bigger than one cell is
    // not solver content and says so — which is that rather than a refusal, so asserting "no
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
    assert!(
        first.is_some(),
        "arriving on the Tiles tab must arm a piece, or the first `down` after the drill refuses"
    );

    // **The walk lives on the Meshes page.** On the Tiles page, `down` walks the tile rows
    // (`TileNext` at `Stance::Browsing`); the library is the Meshes page's list (`TileListNext` at
    // `Stance::Idle`). `right` flips the page — with nothing authored yet it still flips, noting
    // that there is nothing to open, because the author came to pick meshes.
    once(&mut app, vec![binding(Action::PageEnter).key]);
    once(&mut app, vec![binding(Action::TileListNext).key]);
    let after = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .selected_library_id
        .clone();

    assert_ne!(
        first, after,
        "a down arrow on the Meshes page must move the piece in hand — it is the verb the loop repeats most"
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

/// **A refusal raised on the Tiles tab reaches the card that speaks for it.**
///
/// The Meshes and Tiles tabs share one panel, and the refusals of both go through one `ImportState`.
/// There is one problem card, over the viewport, and `notice::paint_notices` writes the live tab's
/// newest refusal into it — which is also what `Cmd+C` harvests, in an editor where `bevy_ui` offers
/// no other way to get text out of the window. So a card that does not follow the tab is a wrong
/// sentence in somebody's paste buffer, and that is what this asserts.
#[test]
fn a_refusal_on_the_tiles_tab_is_visible_and_stays_there() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{App, IntoScheduleConfigs, KeyCode, ResMut, Update};
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

    // **The card, not a `Node.display` nobody writes.** This used to look up a banner by the tab list
    // it carried and assert `Display::Flex` on it — twice. Both assertions were vacuous: `ProblemBanner`
    // is one card over the viewport whose visibility its *layer* owns (`paint_toast` writes
    // `ToastLayer`), so the card's own `display` was never written by anything and `Display::Flex` is
    // simply `Node`'s default. What is worth asserting is what `paint_notices` actually promises: the
    // card carries the live tab's newest refusal, so `Cmd+C` harvests the right sentence.
    let card = {
        let mut q = app
            .world_mut()
            .query_filtered::<&bevy::prelude::Text, bevy::prelude::With<emerge_mapper::chrome::ProblemBanner>>();
        let world = app.world();
        let all: Vec<String> = q.iter(world).map(|t| t.0.clone()).collect();
        assert_eq!(
            all.len(),
            1,
            "there is exactly one problem card over the viewport, found {}",
            all.len()
        );
        all.into_iter().next().unwrap_or_default()
    };
    let said = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .status
        .problem_text()
        .to_owned();
    assert!(
        card.contains(&said),
        "the toast card reads {card:?} while the tab's refusal is {said:?} — a card that does not \
         follow the live tab is a card `notice::copy_out` harvests the wrong sentence out of"
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

    // The tab opens on the Tiles page — name the tile into existence before walking its grid.
    // `open_tile` lands on the Meshes page with the tile in hand.
    open_tile(&mut app, "tile");

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
        .unwrap_or_else(|| panic!("the named tile must be in hand after opening"));
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
    // **`Enter` is the drop AND the hold** — `Space` arms, but `Enter` is `Idle`-scoped, so the
    // old arm-then-drop pair is refused by the census. The drop alone arms.
    let key = |a| emerge_mapper::keys::binding(a).key;
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
    // The tab opens on the Tiles page — a tile must be named into existence before any drop.
    open_tile(&mut app, "tile");

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
    fn release(mut done: bevy::prelude::Local<bool>, mut k: Keys) {
        once(&mut done, &mut k, emerge_mapper::keys::Action::Cancel);
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
    // A drop leaves the piece in hand (`placing = true`), and `Enter` is `Idle`-scoped — release
    // with `Esc` before the second drop, which is the census's shape for two drops in a row.
    step(&mut app, release);
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

    // The tab opens on the Tiles page — a tile has to be named into existence before any drop.
    open_tile(&mut app, "tile");

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
    // The tab opens on the Tiles page — name the tile before the drop, which lands on Meshes.
    open_tile(&mut app, "tile");

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

    // **Enter is the drop AND the hold** — the arm-then-drop pair is refused by the census.
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
    // The tab opens on the Tiles page — name the tile before the drops, which lands on Meshes.
    open_tile(&mut app, "tile");

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
    assert_eq!(n(&app), 1, "the first drop puts one member in");
    // **Captured here, because `Composition::members` is not in drop order.** Reading it back at
    // the end and calling index 0 "the first drop" held only while the library list handed the
    // arrows its rows in fixture order; the 2026-08-20 reversal ended that and this is the honest
    // question — what was in the tile after ONE drop.
    let first_in = sources(&app)
        .first()
        .cloned()
        .unwrap_or_else(|| panic!("one drop is one member"));

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
    let both = sources(&app);
    assert_eq!(both.len(), 2, "two distinct meshes are in: {both:?}");
    let second_in = both
        .iter()
        .find(|s| **s != first_in)
        .unwrap_or_else(|| panic!("the second drop is the other member: {both:?}"))
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
        // **Defined alfa-first so `zulu` is row 0.** The list is newest-defined first since
        // 2026-08-20, and this test's whole premise is that the piece dropped FIRST is the one that
        // sorts LAST in the member list — that is the presentation which makes a correct undo look
        // wrong. Swap these two and the premise quietly evaporates rather than failing.
        .descriptor("alfa", "beta")
        .descriptor("zulu", "alpha")
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();
    // The tab opens on the Tiles page — name the tile before the drops, which lands on Meshes.
    open_tile(&mut app, "tile");

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
    // The tab opens on the Tiles page — name the tile before the drops, which lands on Meshes.
    open_tile(&mut app, "tile");

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
    // The tab opens on the Tiles page — name the tile before the drops, which lands on Meshes.
    open_tile(&mut app, "tile");

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
        page: Option<usize>,
        at: Option<(f32, f32)>,
        focus: Option<usize>,
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
            page: build.browsing,
            at: build
                .open
                .as_ref()
                .and_then(|c| c.members.get(build.focus))
                .map(|m| m.at),
            // **Focus itself is observable.** `MemberPrev`/`MemberNext` move it, and when two
            // members sit at the same coordinates (the floor's home and the drop's home are both
            // the tile's centre here) the `at` readout cannot see the walk — the amber marker is
            // what the key moved, and a marker is a real thing.
            focus: build.open.as_ref().map(|c| c.members.len().min(build.focus)),
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

    // **Every state named by the sequence that reaches it.** The first two are the Tiles page
    // itself: arrival lands on it with the cursor at row 0, and the page's own arrows walk the
    // tile list. The rest open the tile (the `PageEnter` prologue) and work on the Meshes page —
    // which is where the arrows mean the library or the piece.
    //
    // `BuildNew` is absent from these paths: `N` opens the name PROMPT, a typing state in which
    // the census offers nothing and this invariant does not apply.
    // **Every state named by the sequence that reaches it.** The first two are the Tiles page
    // itself: arrival lands on it with the cursor at row 0, and the page's own arrows walk the
    // tile list. The rest drill with `PageEnter` — which opens the tile AND lands on the Meshes
    // page. Opening a tile that already has a member leaves it in hand (Holding), so each state
    // releases with `Esc` first where the drop must fire at Idle: the tile's own floor member is
    // what `open_saved` holds, and that is the reachable truth of this tab.
    //
    // `BuildNew` is absent from these paths: `N` opens the name PROMPT, a typing state in which
    // the census offers nothing and this invariant does not apply.
    let states: Vec<(&str, Vec<Action>)> = vec![
        ("arrived on the door", vec![]),
        (
            "the Tiles page, tile selected",
            vec![Action::TileNext, Action::TilePrev],
        ),
        (
            "reopened tile, on the Meshes page",
            vec![Action::PageEnter],
        ),
        (
            "reopened, piece taken",
            vec![Action::PageEnter, Action::BuildArm],
        ),
        (
            "one member, just dropped",
            vec![Action::PageEnter, Action::Cancel, Action::BuildDrop],
        ),
        (
            "one member, released with Esc",
            vec![
                Action::PageEnter,
                Action::Cancel,
                Action::BuildDrop,
                Action::Cancel,
            ],
        ),
        (
            "emptied again",
            vec![
                Action::PageEnter,
                Action::Cancel,
                Action::BuildDrop,
                Action::BuildDropMember,
            ],
        ),
        (
            "two members",
            vec![
                Action::PageEnter,
                Action::Cancel,
                Action::BuildDrop,
                Action::Cancel,
                Action::TileListNext,
                Action::BuildDrop,
            ],
        ),
        (
            "undone back to the floor member",
            vec![
                Action::PageEnter,
                Action::Cancel,
                Action::BuildDrop,
                Action::UndoBuild,
            ],
        ),
    ];

    let mut dead = Vec::new();
    for (name, path) in &states {
        // Sized under a cell: the invariant is "an offered key does something", and the ladder
        // gives a full-cell piece no travel by design — that case has its own pin,
        // `an_arrow_on_a_piece_that_fills_the_axis_says_so`.
        // **`beta_two` defined first, so `alpha_one` is row 0 and goes into the tile first.** The
        // library list is newest-defined first since 2026-08-20; with the fixture the other way
        // round the second drop lands the focus on member 0, where `MemberPrev` clamps and this
        // sweep reads the clamp as a dead key. That boundary is not new and is not what this test
        // characterises — it walks the states an author reaches, and the state it was written for
        // is a focus with somewhere to step back to.
        let root = Fixture::new(&format!("matrix-{}", name.replace(' ', "-")))
            // **Oldest first.** Newest-defined is row 0, so `base_floor` — the tiles' own floor —
            // sits at the BOTTOM of the library list and the arrows never walk onto it; `alpha_one`
            // is row 0 and `beta_two` row 1, exactly as the comment below this loop describes.
            .sized_descriptor("base_floor", "beta", 0.2, 0.2)
            .sized_descriptor("beta_two", "beta", 0.2, 0.2)
            .sized_descriptor("alpha_one", "alpha", 0.2, 0.2)
            // **Two tiles, one per page-walk direction.** The page's up/down pair needs a second
            // row to move to — one tile saturates both directions and the census would offer a
            // key that cannot move, which is exactly the dead-key this test exists to catch.
            // The composition schema refuses an empty one ("stamps nothing, which looks exactly
            // like a stamp that failed"), so each carries its own floor.
            //
            // **The floor is named `zz_floor`, sorting AFTER `alpha_one`/`beta_two`.** `place`
            // inserts by id and the drop sets `focus` to the insert index, so a floor named
            // `floor` would put the FIRST drop at index 0 and the second at 1 — where `MemberPrev`
            // from the second saturates at 0 and the walk looks dead. The drops must be the FIRST
            // members, and the name sorts them there.
            .bounded_composition(
                "alpha/tile_1",
                (1.0, 4.0, 1.0),
                &[("zz_floor", "base_floor", (0.0, 0.0))],
            )
            .bounded_composition(
                "alpha/tile_2",
                (1.0, 4.0, 1.0),
                &[("zz_floor", "base_floor", (0.0, 0.0))],
            )
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
        // **What the arrows mean here, asked of the stance.** On the Tiles page the up/down pair
        // walks the tile list (`TilePrev`/`TileNext`); on the Meshes page it walks the library
        // (`TileListPrev`/`TileListNext`), or moves the piece when one is in hand.
        //
        // `MemberPrev` rather than `MemberNext`: a drop focuses the most recent member, which on a
        // sorted list is often the last — so `next` saturates there for the same reason `up`
        // saturates at row 0. Walking *back* can always move while there is more than one member,
        // and with one member both directions saturate, which is checked below rather than here.
        // **What the arrows mean here, asked of the stance.** On the Tiles page the up/down pair
        // walks the tile list; on the Meshes page it walks the library (`TileList*`) or moves the
        // piece when one is in hand.
        //
        // Only the *down* direction is probed on the page: `TilePrev` at row 0 saturates — the
        // same legitimate no-op the library's `up` at row 0 makes, which the original sweep
        // excluded by always probing `next`. Two tiles sit in the kit, so `TileNext` can always
        // move.
        let probes = match live.1 {
            emerge_mapper::keys::Stance::Browsing => vec![Action::TileNext],
            _ => {
                let mut p = vec![Action::TileListNext, Action::BuildBack];
                // **The member walk, probed in the direction that has room.** A drop lands at the
                // insert index, which is 0 when the dropped id sorts before the tile's own floor —
                // so `MemberPrev` there saturates by design, the same legitimate no-op the
                // original sweep excluded at the other end. Asking the focus where it can go keeps
                // the probe honest: the amber marker must move, whichever way that is.
                if read(&app).members > 1 {
                    let focus = app.world().resource::<Build>().focus;
                    p.push(if focus > 0 {
                        Action::MemberPrev
                    } else {
                        Action::MemberNext
                    });
                }
                p
            }
        };
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
        // **Defined in reverse of the drop order.** The list is newest-defined first since
        // 2026-08-20, so `alpha_one` is row 0 and goes in first — which is what leaves the SECOND
        // member at the higher index, and `left` with somewhere to step back to. Defined the other
        // way round the focus lands on 0 and this test measures a clamp instead of a walk.
        .descriptor("beta_two", "beta")
        .descriptor("alpha_one", "alpha")
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();
    // The tab opens on the Tiles page — name the tile before the drops, which lands on Meshes.
    open_tile(&mut app, "tile");

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
        // **Desk first so the lamp is row 0.** The list is newest-defined first since 2026-08-20,
        // and this test needs the guest dropped BEFORE the host — a lamp landing on a desk that is
        // already there is the case that must succeed, not the one under test.
        .surface_descriptor("zz_desk", "beta", "worktop")
        .mounted_descriptor("aa_lamp", "alpha", "worktop")
        .build("m");
    let mut app =
        emerge_mapper::harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles).unwrap_or_else(|e| panic!("{e}"));
    app.update();
    // The tab opens on the Tiles page — name the tile into existence first. `Enter` on the Tiles
    // page opens a row (`TileOpen`), not a drop; the Meshes page is where drops happen, and that
    // is where `open_tile` lands.
    open_tile(&mut app, "tile");

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
            emerge_mapper::chooser::Catalog {
                kits: Vec::new(),
                maps: Vec::new(),
                authoring: None,
                bashes: Vec::new(),
                projects: Vec::new(),
            },
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
            // **Deserialise the step the way the app will, because serde's silence is the whole
            // failure mode.** Every field of `Step` is `#[serde(default)]`, so a sibling of
            // `checkpoint` spelled `args` instead of `with` did not fail — it deserialised to
            // `with: None`, the condition was handed `null` for ever, and the step could never pass.
            // Two steps of `label_a_mesh.json` shipped that way. Reading the JSON as a `Value`, which
            // is all the assertions around this one do, cannot see it; asking `Step` itself can, and
            // it needs no field list kept in step by hand.
            serde_json::from_value::<bevy_debugger_bevy::Step>(step.clone()).unwrap_or_else(|e| {
                panic!(
                    "{name}: step `{label}` is not a `Step`: {e}. If that names an unknown field, \
                     the checkpoint's arguments are `with`, never `args`"
                )
            });
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
    // The kit `kits.ron` declares as `authoring` — what an author gets on a plain open, and so the
    // library every shipped card has to be walkable against. `None` is how that field is resolved;
    // naming `furniture` was a directory standing in for the declaration.
    let mut app = harness::build_headless(&workspace, "untitled_map", None)
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
            //
            // **Except the `start a tile` step, whose prompt this test's own special block owns.**
            // `N` raised it and the block below types the name and presses Enter; answering here
            // first would close the prompt and leave that Enter to fall through to a drop.
            if label != "start a tile"
                && app
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
        //
        // **The auto-answer above skips this step on purpose.** `N` raised the prompt and the
        // block below owns the answer: it types the name and presses Enter itself, and answering
        // the prompt in the chord loop would leave this Enter to fall through to a drop.
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

    // Parked, not gone — see `guided::PENDING_GUIDES_DIR`. It is still driven here so it cannot rot
    // while the kit it names is re-authored.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(emerge_mapper::guided::PENDING_GUIDES_DIR)
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
        // **Defined back to front, so the rows come out front to back.** The library list is
        // newest-defined first since 2026-08-20, and every walk count below is written against the
        // script's prose — "one `right` lands on `site/floor`". Reversing the fixture keeps the
        // counts describing what an author following the script actually does.
        .sized_descriptor("site/wall_low", "site", 0.2, 1.0)
        .sized_descriptor("site/wall", "site", 0.1, 1.0)
        .descriptor("site/floor", "site")
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
            // `N` opens the name field; the field is typed into and committed by the loop's own
            // auto-answer, because text is a message stream rather than a key press.
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
            // Observation: the drop is what put a piece in hand. Nothing to press.
            "the piece should be in hand" => vec![],
            "bring a wall in as well" => vec![
                vec![Action::Cancel],
                vec![Action::TileListNext],
                vec![Action::BuildDrop],
            ],
            "is the tile still one cell" => vec![],
            "save it" => vec![vec![Action::Save]],
            // **The save left the author on the Meshes page with the piece still in hand.** `Esc`
            // puts it back (the stance the arrows answer from), `left` ascends to the Tiles page,
            // then four walks to tile_4 — the save two steps earlier put a NAMED tile in the kit,
            // so `site/named_by_the_test` lands ahead of `site/tile_1` and every row moved down
            // one — and `Enter` reopens it, landing back on the Meshes page where the flush step's
            // arrows work.
            "reopen tile_4 from the kit" => vec![
                vec![Action::Cancel],
                vec![Action::PageLeave],
                vec![Action::TileNext],
                vec![Action::TileNext],
                vec![Action::TileNext],
                vec![Action::TileNext],
                vec![Action::TileOpen],
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

/// **The corner-tile feedback script, driven — the smallest usable room-maker, walked.**
///
/// The second feedback exercise ships in `guides/corner_tile_feedback.json`: floor, two walls, the
/// second turned a quarter, both flushed — the tile a room is made of — with the judgement steps a
/// machine cannot answer left as `checkpoint: null`, exactly like its older sibling
/// `tile_feedback.json`. This walks it the way the authoring driver does: press what an author
/// following the card would press, and assert each step's checkpoint goes **false to true at that
/// step**. The two `null` steps are passed over the way the app passes over them.
///
/// The fixture defines the wall before the floor, so the library reads newest-first and `floor`
/// lands on row 0 — which is the walk the card's prose assumes.
#[cfg(feature = "debugger")]
#[test]
fn the_corner_tile_feedback_script_can_actually_be_followed() {
    use bevy_debugger_bevy::Checkpoints;
    use emerge_mapper::keys::Action;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(emerge_mapper::guided::GUIDES_DIR)
        .join("corner_tile_feedback.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let script: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("bad JSON: {e}"));
    let empty = vec![];
    let steps = script["steps"].as_array().unwrap_or(&empty);

    // **Defined back to front, so the rows come out front to back.** The library list is
    // newest-defined first since 2026-08-20, and the walk counts below are written against the
    // card's prose — "walk to a wall" is one `down` from the armed floor.
    let root = Fixture::new("corner_feedback")
        .descriptor("wall", "kit")
        .descriptor("floor", "kit")
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
            // `N` opens the name field; the field is typed into and committed by the loop's own
            // auto-answer, because text is a message stream rather than a key press.
            "start a tile" => vec![vec![Action::BuildNew]],
            // One `TileListNext` from nothing picked lands on the first row, `floor` — the same
            // arm the arrival seed makes — and Enter drops it.
            "bring a floor in" => vec![vec![Action::TileListNext], vec![Action::BuildDrop]],
            // Observation: the drop is what put a piece in hand. Nothing to press.
            "the floor should be in hand" => vec![],
            // `Esc` puts the floor down (the walk is `Idle`-scoped), one step to the wall, drop.
            "bring a wall in" => vec![
                vec![Action::Cancel],
                vec![Action::TileListNext],
                vec![Action::BuildDrop],
            ],
            // One quarter turn of the focused member — the wall just dropped.
            "turn the second wall a quarter" => vec![vec![Action::BuildTurn]],
            "is the tile still one cell" => vec![],
            "save it" => vec![vec![Action::Save]],
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
        // *observation* step presses nothing, and its checkpoint being true IS the pass.
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
            press(&mut app, codes);
            // The name prompt from `N` is answered out of band, like every drive test: text is a
            // message stream rather than a key press.
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
        reached >= 6,
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

/// **An ASSET-CONTRACT test: can the solver actually use the shipped tiles?**
///
/// It reads the shipped project on purpose, and that is the exception the fixture rule allows —
/// what it asserts *is* a fact about what ships. Authoring tiles is only worth doing if
/// `grammar::from_compositions` turns them into prototypes, and every guided step up to now proved
/// they were *saved*, which is a different claim and the weaker one.
///
/// `skipped` is the useful output: it names each composition it could not make a tile of, and why.
/// A tile the solver cannot use is a tile authored for nothing.
#[test]
fn the_shipped_tiles_become_solver_prototypes() {
    let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(std::path::Path::to_path_buf)
    else {
        panic!("the crate must sit two levels under the repo root");
    };
    // The kit `kits.ron` declares as `authoring`. `compositions.ron` is the **project's**, not any
    // one kit's, so what is under test is every tile that ships — the kit resolved here decides
    // which library those tiles are resolved against.
    //
    // **`None`, which is what a plain open does.** `Project::open(root, None)` resolves that
    // `authoring` field, so passing nothing is what makes the code match the sentence above. Naming
    // `furniture` made the test agree with a *directory* rather than with the declaration it claims
    // to be about, and the day `authoring` moves it would have gone on reading the old kit, green.
    let project = emerge_mapper::project::Project::open(&root, None)
        .unwrap_or_else(|e| panic!("the shipped kit must open: {e}"));

    let tiles = &project.compositions.compositions;
    println!("\nshipped: {} composition(s)", tiles.len());
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
/// The Tiles page is the tab's first page: every authored tile, one row each, `New Tile +` at the
/// top. `right` drills to the Meshes page (opening the tile under the cursor on the way), `left`
/// comes back, and `Enter` reopens the tile under the cursor without drilling.
#[test]
fn the_tiles_page_lists_authored_tiles_and_reopening_works() {
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

    // **The tab opens on the Tiles page** — arrival seeds the cursor at row 0, which is what the
    // strip now shows, and that IS the stance: `Browsing` is the page's own key set.
    assert_eq!(
        app.world().resource::<Build>().browsing,
        Some(0),
        "arrival lands on the Tiles page, cursor at the top"
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

    press(&mut app, key(Action::TileNext));
    assert_eq!(
        app.world().resource::<Build>().browsing,
        Some(1),
        "down walks the tiles"
    );
    // Saturating at the end, like the member walk: holding an arrow should stop, not wrap.
    press(&mut app, key(Action::TileNext));
    assert_eq!(
        app.world().resource::<Build>().browsing,
        Some(1),
        "and stops at the end"
    );

    // **`right` opens the tile under the cursor AND drills into the Meshes page** — the author's
    // shape for the tab: *"push right arrow with a tile selected to add/move to the Meshes tab."*
    press(&mut app, key(Action::PageEnter));
    let build = app.world().resource::<Build>();
    assert_eq!(
        build.open.as_ref().map(|c| c.id.as_str()),
        Some("kit/two"),
        "the tile under the cursor is open for editing"
    );
    assert_eq!(build.browsing, None, "and the page flipped to Meshes");
    // **Reopening lands you able to edit**, which is the whole reason the verb exists. This
    // asserted the opposite for an hour: `open_saved` cleared `placing`, so an author who reopened a
    // tile got `Stance::Idle` -- arrows walking the library, `,`/`.` not bound at all -- with the
    // tile they had just asked to edit sitting there untouchable. Reported from the keyboard within
    // a minute of the verb shipping: "these keys aren't doing anything".
    assert!(
        build.placing,
        "reopening a tile is holding it: there is nothing else to pick up"
    );

    // **`left` comes back to the Tiles page**, and `Esc` backs out of the page entirely.
    press(&mut app, key(Action::PageLeave));
    assert_eq!(
        app.world().resource::<Build>().browsing,
        Some(0),
        "left returns to the Tiles page, cursor where the drill left it"
    );
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

    // The tab opens on the Tiles page — a tile is named into existence before the ladder exists.
    open_tile(&mut app, "tile");

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
    // The tab opens on the Tiles page — name the tile before the drop, which lands on Meshes.
    open_tile(&mut app, "tile");

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
    // The tab opens on the Tiles page — name the tile before the drop, which lands on Meshes.
    open_tile(&mut app, "tile");

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

    // **The Tiles page is up on arrival, and a tile cursor is not a mesh selection** — no ghost.
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        ghosts(&mut app),
        0,
        "no mesh ghost on the Tiles page: the cursor there is on a tile"
    );

    // `right` drills into the Meshes page (and opens the tile under the cursor), so the armed
    // selection — the first library row, armed on arrival — is what the ghost previews.
    press(&mut app, key(emerge_mapper::keys::Action::PageEnter));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        ghosts(&mut app),
        1,
        "the armed selection ghosts on the Meshes page, before any Enter"
    );

    // **`left` ascends only at Idle.** The drill opened a tile with a member in it, so the piece
    // is in hand and `left` moves it — the door idiom: `Esc` puts the piece back, then `left`
    // goes up a page. The ghost goes with the Meshes page either way.
    press(&mut app, key(emerge_mapper::keys::Action::Cancel));
    press(&mut app, key(emerge_mapper::keys::Action::PageLeave));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        ghosts(&mut app),
        0,
        "no mesh ghost on the Tiles page: the cursor there is on a tile"
    );

    // And `right` again brings both back — the pair is a drill, not a toggle.
    press(&mut app, key(emerge_mapper::keys::Action::PageEnter));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        ghosts(&mut app),
        1,
        "returning to the Meshes page brings the preview back"
    );

    // **And the preview is actually translucent — the ask this feature is named for** ("a
    // semitransparent rendering ... like a placeholder"). The fixture's glb carries no material
    // and scene assets never finish loading in this deviceless harness, so the ghost's mesh child
    // never gains one here — what is under test is what `fade_ghost` DOES to a material, so the
    // ghost root is handed a real one and the editor's own system is left to fade it.
    let mat = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let ghost_root = app
        .world_mut()
        .query_filtered::<Entity, (
            With<emerge_mapper::build::StagedTile>,
            With<emerge_mapper::editor::Ghost>,
        )>()
        .iter(app.world())
        .next()
        .expect("the ghost is on screen");
    app.world_mut().entity_mut(ghost_root).insert(MeshMaterial3d(mat));
    // `fade_ghost` runs unconditionally in the editor now — not behind the map door, which was the
    // defect: on the kit door the pair never ran, so the staged piece rendered solid and
    // shadow-casting, indistinguishable from a committed member.
    for _ in 0..3 {
        app.update();
    }
    let faded = app
        .world()
        .get::<MeshMaterial3d<StandardMaterial>>(ghost_root)
        .and_then(|m| app.world().resource::<Assets<StandardMaterial>>().get(&m.0))
        .expect("the ghost root carries the faded material");
    assert_eq!(
        faded.alpha_mode,
        AlphaMode::Blend,
        "the ghost renders with blend alpha — a solid ghost is a committed member"
    );
    assert!(
        (faded.base_color.alpha() - 0.45).abs() < 1e-3,
        "the ghost sits at GHOST_ALPHA (0.45) times the base alpha — got {}",
        faded.base_color.alpha()
    );
    assert!(
        app.world()
            .get::<bevy::light::NotShadowCaster>(ghost_root)
            .is_some(),
        "the ghost casts no shadow — a shadow-carrying preview reads as already-drawn"
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

    // The drill to the Meshes page is a real key press — the page pair is the strip's promise.
    let press = |app: &mut App, key: KeyCode| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<bevy::input::ButtonInput<KeyCode>>,
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

    // **The Tiles page's chip, not the tab bar's.** The top bar has a TILES chip of its own too, so
    // the needle carries the count only the panel strip shows.
    assert_eq!(
        scrolled(&mut app, "TILES ("),
        Some(false),
        "the page strip must have no scrolling ancestor — frozen above the list"
    );
    // **The row MARKER, not a string.** This looked for the `IN LIBRARY` heading, which is gone —
    // the shelf is a chip in the strip now and it carries the count, so saying it twice was the
    // drift `chrome.rs` exists to stop. Re-pointing it at the row's text was the obvious fix and the
    // wrong one: a descriptor id appears in the detail pane too, and the helper takes whichever the
    // query reaches first. `LibraryRow` is what a row IS.
    //
    // **Drilled to the Meshes page first**: the Tiles page is the authored tiles, and the library
    // rows are the Meshes page's — the drill is part of what the strip promises.
    press(
        &mut app,
        emerge_mapper::keys::binding(emerge_mapper::keys::Action::PageEnter).key,
    );
    for _ in 0..3 {
        app.update();
    }
    let row = app
        .world_mut()
        .query_filtered::<Entity, With<emerge_mapper::tiles::LibraryRow>>()
        .iter(app.world())
        .next()
        .expect("the Meshes page draws library rows");
    let mut e = row;
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
    assert!(
        inside_scroll,
        "the rows themselves still live in the scroll container"
    );
}

/// Load a shipped guide and hand back one step's `(checkpoint, with)` by label — so the drive tests
/// below track the JSON they exercise, and an edit to a script moves its test or fails it by name.
fn guide_step(file: &str, label: &str) -> (String, serde_json::Value) {
    // `file` is relative to `guides/`, so a parked exercise is named `pending/<file>` — see
    // `guided::PENDING_GUIDES_DIR` for what parking means and why it is a move rather than a flag.
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
        let (name, with) = guide_step("pending/repair_the_kit.json", label);
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
    // **The Tiles page is already up** — the tab opened on it. Three walks reach tile_4 (rows
    // 0..3 are tile_1..tile_3, this fixture has no `named_*` tile ahead of them), and `Enter`
    // reopens it, landing on the Meshes page where the flush step's arrows work.
    walk(
        &mut app,
        "reopen tile_4 from the kit",
        vec![
            vec![Action::TileNext],
            vec![Action::TileNext],
            vec![Action::TileNext],
            vec![Action::TileOpen],
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
    let file = "pending/derive_edges.json";

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

/// **The definition bench shows the kit being authored, and the composing palette shows the merge.**
///
/// The report that started this: opening a brand-new kit showed 90 rows it does not own, cannot
/// edit and did not make — `Project::open` merges every bound kit into `project.library`, and the
/// Meshes shelf listed that merge. A new kit has to read as empty, because it is.
///
/// **The merge is untouched**, which is the other half. `project.library` is what a map resolves
/// against and what a tile is composed from — a tile may seat two kits' pieces — so the Tiles tab
/// still asks for all of them. Only the bench narrows.
#[test]
fn the_definition_bench_lists_one_kit_and_the_composing_palette_lists_the_merge() {
    use emerge_mapper::filter::Filters;
    use emerge_mapper::tiles::library_ids_for_test;

    // The authoring kit (`furniture`, where the unnamed descriptors land) defines one piece; the
    // kit beside it defines two.
    let root = Fixture::new("bench-one-kit")
        .descriptor("bench", "alpha")
        .kit("site", "ozea", &["site/wall", "site/floor"])
        .build("m");
    let app = harness::build_headless(&root, "m", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    let project = app.world().resource::<emerge_mapper::project::Project>();
    let filters = Filters::default();

    assert_eq!(
        project.namespace, "furniture",
        "the kit being authored is the one `kits.ron` names"
    );
    assert_eq!(
        project.library.descriptors.len(),
        3,
        "the merge still holds every bound kit's pieces"
    );

    let bench = library_ids_for_test(project, &filters, false, None);
    assert_eq!(
        bench,
        vec!["bench".to_owned()],
        "the Meshes tab lists the kit being authored and nothing else: {bench:?}"
    );

    let mut composing = library_ids_for_test(project, &filters, true, None);
    composing.sort();
    assert_eq!(
        composing,
        vec!["bench".to_owned(), "site/floor".to_owned(), "site/wall".to_owned()],
        "and the Tiles tab still composes from every bound kit: {composing:?}"
    );
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
    use emerge_mapper::labels::{Entry, Suggestions};
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
        "and the proposal does not stay staged: `apply_what_arrives` reaches for the first staged \
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
    // **Arrival is the Tiles page, not a tile.** Nothing is open until the author asks: `N` names
    // a new tile and `Enter` on a row reopens an old one. An editor that forced a blank tile open
    // the moment the tab appeared was the thing the page replaced.
    assert!(
        app.world().resource::<Build>().open.is_none(),
        "arriving must not open a tile — the Tiles page is what an arrival shows"
    );
    assert_eq!(
        app.world().resource::<Build>().browsing,
        Some(0),
        "and the cursor is on the page's first row"
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
    assert_eq!(
        app.world().resource::<Build>().browsing,
        None,
        "naming lands on the Meshes page — the point of naming is to start filling the tile"
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

    // **A second tile, named the same way.** Nothing to do but name it — the prompt is the door.
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

/// **A named tile is a row on the Tiles page before it is saved** — the draft.
///
/// Reported at the keyboard, 2026-08-23: *"when I make a new tile it doesn't work as expected. I
/// expect a new entry under 'Tiles' with the name I gave the tile."* The name is given the moment
/// `N`'s prompt is answered, so the row must exist the moment the name exists — not only once
/// `Cmd+S` commits it. The page count, the chip count and the walk all include the draft, and
/// `Esc` back to the page can stand on it.
#[test]
fn a_named_tile_is_a_row_on_the_tiles_page_before_it_is_saved() {
    use emerge_mapper::build::{Build, page_len};
    use emerge_mapper::keys::Action;

    let root = Fixture::new("draft-row")
        .sized_descriptor("wall", "alpha", 0.2, 0.2)
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

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

    // Name a tile; nothing is saved yet.
    press(&mut app, vec![key(Action::BuildNew)]);
    name_the_tile(&mut app, "corner_east");
    for _ in 0..2 {
        app.update();
    }

    // **The draft is a page row from the moment it has a name.**
    let build = app.world().resource::<Build>();
    assert_eq!(
        build.open.as_ref().map(|c| c.id.as_str()),
        Some("furniture/corner_east"),
        "the named tile is in hand"
    );
    assert_eq!(
        page_len(&app.world().resource::<emerge_mapper::project::Project>(), build),
        1,
        "the page counts the draft — the walk clamps to it and the chip shows it"
    );
    let kit = emerge_core::census::of_catalog(
        &app.world().resource::<emerge_mapper::project::Project>().library,
        &app.world().resource::<emerge_mapper::project::Project>().compositions.compositions,
    )
    .compositions;
    assert_eq!(kit, 0, "and nothing is committed yet — the draft is a row, not a save");

    // **A wall goes in first** — an empty tile is refused at save by name, and the draft needs
    // a member to be commit-able at all. The arrival seed armed the first library row, so one
    // Enter drops it.
    press(&mut app, vec![key(Action::BuildDrop)]);
    for _ in 0..2 {
        app.update();
    }

    // **The walk can stand on the draft row.** The drop left the wall in hand, and `left` is
    // Idle-scoped — release with `Esc` first, the same door the committed-row drill uses. Then
    // `left` back to the page (the draft is open, so the drill just flips) leaves the cursor on
    // the single row the page holds.
    press(&mut app, vec![key(Action::Cancel)]);
    press(&mut app, vec![key(Action::PageLeave)]);
    assert_eq!(
        app.world().resource::<Build>().browsing,
        Some(0),
        "the page walk can stand on the draft row"
    );

    // **`Cmd+S` commits it** and the row stays, now as a committed tile.
    press(&mut app, vec![key(Action::Save), emerge_mapper::keys::MOD_KEYS[0]]);
    for _ in 0..2 {
        app.update();
    }
    assert!(
        app.world()
            .resource::<emerge_mapper::project::Project>()
            .compositions
            .compositions
            .iter()
            .any(|c| c.id == "furniture/corner_east"),
        "Cmd+S commits the draft — the row survives, the draft marker does not"
    );
    assert_eq!(
        page_len(&app.world().resource::<emerge_mapper::project::Project>(), &app.world().resource::<Build>()),
        1,
        "after the save it is still one row — the committed tile the draft was"
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

/// **The filter box owns the letters and the list keeps walking** — the arrow this whole feature
/// exists for. Reported at the keyboard, 2026-08-23: typing into the box killed the up/down walk
/// of the list it was narrowing, so an author who typed `w`, saw the row, and pressed `down` got
/// nothing — the filter had narrowed the list the arrows no longer walked.
///
/// `keys::Holder::Filter` is the one deliberate hole in the focus guard: the walk rows the box
/// narrows stay live **on the tab they belong to**, and everything else (the drop, the tabs, the
/// letters) stays suppressed — which is also what makes the final `Enter` hand the keyboard back
/// WITHOUT falling through to a drop, the way it does in
/// [`f_focuses_the_filter_and_enter_hands_the_keyboard_back`]. It was a `Context::Filter` in
/// `Live.0`, which threw the tab away and fired every tab's walk on one arrow.
#[test]
fn the_list_still_walks_while_its_filter_box_holds_the_keys() {
    use bevy::input::keyboard::KeyboardInput;
    use emerge_mapper::filter::{Filters, Pane};
    use emerge_mapper::keys::Action;

    // Three rows so the filter actually narrows and the arrival seed ("floor", newest-first)
    // gets filtered out from under the cursor — the walk then has to RE-ARM onto the first row
    // the filter shows, which is the silent no-op this test pins. No authored tiles: `right`
    // flips to the Meshes page noting there is nothing to open, so the arrows are Idle — the
    // stance the walk is bound at.
    let root = Fixture::new("filter-walk")
        .sized_descriptor("wall", "alpha", 0.2, 0.2)
        .sized_descriptor("wainscot", "alpha", 0.2, 0.2)
        .sized_descriptor("floor", "beta", 0.2, 0.2)
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
    let selected = |app: &App| {
        app.world()
            .resource::<emerge_mapper::tiles::ImportState>()
            .selected_library_id
            .clone()
    };
    let rows = |app: &App| {
        emerge_mapper::tiles::library_ids_for_test(
            app.world().resource::<emerge_mapper::project::Project>(),
            app.world().resource::<Filters>(),
            true,
            None,
        )
    };
    let members = |app: &App| {
        app.world()
            .resource::<emerge_mapper::build::Build>()
            .open
            .as_ref()
            .map_or(0, |c| c.members.len())
    };
    let tap = |app: &mut App, logical: bevy::input::keyboard::Key, code: KeyCode| {
        for state in [
            bevy::input::ButtonState::Pressed,
            bevy::input::ButtonState::Released,
        ] {
            app.world_mut()
                .write_message(KeyboardInput {
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

    for _ in 0..2 {
        app.update();
    }
    // The box the report named is the one under "Meshes" — the drill's list. `right` takes the
    // Meshes page; with nothing authored it still flips, noting there is nothing to open.
    press(&mut app, key(Action::PageEnter));
    for _ in 0..2 {
        app.update();
    }
    press(&mut app, key(Action::FocusFilter));
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<Filters>().focus_pane(),
        Some(Pane::Candidates),
        "`F` puts the cursor in the box"
    );
    // The drain frame — the keystroke that opens a field must not become its first character.
    app.update();

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
    assert_eq!(
        rows(&app),
        vec!["wainscot".to_owned(), "wall".to_owned()],
        "the filter narrowed the list to the two rows that contain `w`"
    );
    // **The filter re-seats the selection on the first row it shows** — the arrival seed ("floor",
    // newest-first) is gone from the list, and `keep_library_selection_visible` puts the cursor on
    // what the author can see, so a half-typed filter never leaves the selection on a hidden row.
    // It fires on the change-detection frame after the keystroke's, so a few frames settle it.
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        selected(&app).as_deref(),
        Some("wainscot"),
        "the filter re-seats the cursor on its first row — the old seed was filtered out"
    );

    // **The walk fires through the open box** — the bug reported at the keyboard. One `down` reaches
    // the second row the filter shows; before the exemption existed, the box holding the keys
    // suppressed the arrows entirely and the author was stranded on the first row.
    press(&mut app, key(Action::TileListNext));
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        selected(&app).as_deref(),
        Some("wall"),
        "one `down` while the box owns the keyboard must walk to the next row the filter shows"
    );
    // And back up again — the pair is a walk in both directions.
    press(&mut app, key(Action::TileListPrev));
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        selected(&app).as_deref(),
        Some("wainscot"),
        "and one `up` walks back to the first row the filter shows"
    );

    // **`Enter` still hands the keyboard back** — the walk exemption is the only hole in the
    // filter focus guard, so the same Enter that blurs the box must not fall through to a drop.
    let before = members(&app);
    tap(&mut app, bevy::input::keyboard::Key::Enter, KeyCode::Enter);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<Filters>().focus_pane(),
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

/// **`right` goes into the Meshes page and `left` comes back to Tiles** — the column browser, both
/// directions.
///
/// This key has now been wrong twice in opposite ways, which is why it is pinned rather than
/// trusted. The KIT strip shipped promising *"right reopens / left back"* over an **unbound**
/// `left`; the first fix reworded the strip to name `Esc`, making the prose honest and leaving the
/// author pressing a dead key anyway. Reported at the keyboard, 2026-08-15: *"I would expect left
/// to move back to meshes, but it doesn't."* The promise was right and the binding was missing —
/// and the two-page drill keeps it, with the page pair as the promise.
///
/// `Esc` still backs out — it backs out of everything, and `no_reachable_tiles_state_leaves_the_
/// arrows_doing_nothing` covers — so this pins the direction the idiom implies, in both directions,
/// against `Build::browsing` itself.
#[test]
fn right_enters_the_meshes_page_and_left_comes_back_to_tiles() {
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

    // **The tab opens on the Tiles page** — arrival seeds the cursor at the top. `right` drills
    // into the Meshes page and opens the tile under the cursor on the way; `left` comes back.
    assert_eq!(
        browsing(&app),
        Some(0),
        "the tab opens on the Tiles page — arrival seeds the cursor at the top"
    );

    press(&mut app, key(Action::PageEnter));
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        browsing(&app),
        None,
        "`right` drills into the Meshes page — opening the tile under the cursor"
    );

    // **`left` ascends only at Idle — with a member in hand the arrows move the piece.** The
    // door idiom: `Esc` puts the piece back, then `left` goes up a page. The strip says "left
    // goes back", which is the promise; the stance decides when the key answers.
    press(&mut app, key(Action::Cancel));
    press(&mut app, key(Action::PageLeave));
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        browsing(&app),
        Some(0),
        "`left` must come back to the Tiles page — the strip has promised this since the kit shipped"
    );

    // And the two are different keys doing different things, not one key toggling: `Enter` opens
    // the tile under the cursor and lands on the Meshes page too, without the strip having moved.
    press(&mut app, key(Action::TileOpen));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(browsing(&app), None, "opening also lands on the Meshes page");
    assert!(
        app.world()
            .resource::<emerge_mapper::build::Build>()
            .open
            .is_some(),
        "and it leaves with a tile open, which is what tells the two apart"
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
    let file = "pending/build_a_room.json";
    let settle = |app: &mut App| {
        for _ in 0..2 {
            app.update();
        }
    };

    // **Step 1 is satisfied by the door.** It used to start false because the editor booted on Map
    // and `3` was what reached the Tiles tab. The Kit door opens on Meshes and holds Tiles one key
    // away, so what this step now asserts is that the panel the guide names is one this door has —
    // which is the fact worth checking, and the only one still available.
    // **Start the corner tile, for real** — the card's `N` + name + Enter. The tile half of this
    // test is real key presses, and a blank tile in hand is the state the member writes below
    // describe (the card's "walk to site/floor" wording depends on list order, which is a
    // property of the corpus, so the three Enters and one R are stood in for directly — but the
    // tile they land in must be one the editor actually opened).
    {
        let (start, _) = guide_step(file, "start the corner tile");
        assert!(
            !checkpoint(&mut app, &start, serde_json::Value::Null),
            "`start the corner tile` must begin false — no tile is open on arrival"
        );
        press(&mut app, vec![key(emerge_mapper::keys::Action::BuildNew)]);
        settle(&mut app);
        name_the_tile(&mut app, "corner");
        settle(&mut app);
    }

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
        app.world()
            .resource::<emerge_mapper::confirm::Confirm>()
            .asking(emerge_mapper::confirm::Asked::LeaveMap),
        "the last layer is a question, not a departure — and it is the ONE question, not a line \
         this door invented for itself"
    );
    assert!(
        !heading_back(&app),
        "asking is not going — leaving silently on a reflex key is what this question exists for"
    );

    // **And `Y` answers it, not a third `Esc`.**
    //
    // This used to be `Esc` three times, which is what the original report asked for — but `Esc`
    // agreeing here while `Esc` refused the chooser's delete is exactly the spread `crate::confirm`
    // was built to end. `Esc` still refuses, so the peel's own promise is intact: the key that
    // backs out never commits anything.
    tap(&mut app, KeyCode::Escape);
    assert!(
        !heading_back(&app),
        "`Esc` at the modal is a synonym for `N` — the back-out key must never be the one that \
         commits"
    );
    app.world_mut()
        .resource_mut::<emerge_mapper::confirm::Confirm>()
        .ask(
            emerge_mapper::confirm::Asked::LeaveMap,
            "Leave this map?",
            "",
            "Go",
            "Stay",
        );
    tap(&mut app, KeyCode::KeyY);
    assert!(
        heading_back(&app),
        "`Y` proceeds, on a clean map where it can lose nothing"
    );
}

/// **`Cmd+O` saves what is open and goes, in one press.**
///
/// # This test used to pin the opposite, and the reversal is the point
///
/// It was `the_menu_key_refuses_to_leave_unsaved_work`, and it asserted a three-way question — `S`
/// save and go, `D` discard and go, `Esc` stay — raised by the chord as well as by `Esc`. Asked for
/// at the keyboard 2026-08-18: *"make sure the Cmd+O button doesn't require a second key press, it
/// just autosaves and takes you back."*
///
/// The question existed to stop work being lost. **Saving loses nothing**, so it has been answered
/// rather than removed: there is no branch here that discards. What survives is the part that was
/// really about reflexes — `Esc` still asks, because `Esc` gets pressed by accident and a chord does
/// not, and `the_escape_peel_asks_before_it_leaves` above is what holds that half.
///
/// The one path out of `save_and_leave` that does not reach the menu is a **refused** save, which is
/// why `dirty` is checked after the chord rather than just the transition: a test that only watched
/// the screen change would go green on a save that silently did nothing.
#[test]
fn the_menu_key_saves_and_goes() {
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

    // **Dirty: it saves, and then it goes — one press, no question.**
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
        !leaving(&app),
        "Cmd+O must not raise the leaving question — that is the second press this was asked to \
         remove, and `Esc` is the key that still asks"
    );
    assert!(
        !app.world().resource::<OpenMap>().dirty,
        "the map had unsaved edits and the chord left anyway without writing them. `dirty` is \
         `OpenMap::save`'s own receipt, so this is the assertion that a silent no-op cannot pass"
    );
    assert!(
        heading_back(&app),
        "and having saved, it goes — in the same press"
    );
    // The write is real: the placement is on disk, not merely flagged clean.
    let on_disk = root.join("assets/emerge/maps/m.map.ron");
    let written = std::fs::read_to_string(&on_disk)
        .unwrap_or_else(|e| panic!("the saved map must be on disk at {on_disk:?}: {e}"));
    assert!(
        written.contains("floor@1"),
        "the autosave has to write the edit it was protecting, not just clear the flag"
    );
    stay(&mut app);

    // **Clean: nothing to write, so it simply goes.**
    {
        let mut open = app.world_mut().resource_mut::<OpenMap>();
        open.dirty = false;
    }
    chord(&mut app);
    assert!(!leaving(&app), "still no question on a clean map");
    assert!(
        heading_back(&app),
        "a clean map has nothing to save and nothing to ask about"
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
    // **The old premise is gone: arrival seeds the first library row** (2026-08-23 — the Tiles
    // page seed arms a piece so the first `Enter` after the drill is a drop rather than a
    // refusal). The press this test is about is the one that *walks*: it must land on the SECOND
    // row, never re-establish the first. No pack here — a focused candidate would block the seed.
    let root = Fixture::new("first_press")
        .descriptor("alpha/floor", "alpha")
        .descriptor("alpha/wall", "alpha")
        .build("test_map");
    let mut app =
        harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
            .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let rows = |app: &App| {
        emerge_mapper::tiles::library_ids_for_test(
            app.world().resource::<emerge_mapper::project::Project>(),
            app.world().resource::<emerge_mapper::filter::Filters>(),
            true,
            None,
        )
    };

    // **The row count first, because both assertions below compare two `Option`s.**
    //
    // `armed` against `rows().first()` and `picked` against `rows().get(1)`: with an empty list both
    // sides of the first are `None`, and with fewer than two rows both sides of the second are — so
    // a fixture that stopped reaching the list, or a filter that emptied it, would pass this test
    // twice over while the seed and the walk did nothing at all. Two descriptors go in; two rows
    // must come out.
    assert_eq!(
        rows(&app).len(),
        2,
        "the library list must hold the fixture's two pieces for either comparison below to mean \
         anything; it holds {:?}",
        rows(&app)
    );

    // **Arrival arms the first row — no press did it.** This is the half the old bug was about:
    // the press that establishes the selection must not also walk it.
    let armed = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .selected_library_id
        .clone();
    assert_eq!(
        armed.as_deref(),
        rows(&app).first().map(String::as_str),
        "arriving must arm the FIRST row — it armed {armed:?}, the list reads {:?}",
        rows(&app)
    );

    // **The walk lives on the Meshes page**: on the Tiles page, `down` walks tile rows
    // (`TileNext` at `Stance::Browsing`). `right` flips the page — with nothing authored it still
    // flips, noting that there is nothing to open, because the author came to pick meshes. Then
    // one `down` must land on the SECOND row, never re-establish the first.
    let press = |app: &mut App, key: KeyCode| {
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
    };
    press(&mut app, binding(Action::PageEnter).key);
    press(&mut app, binding(Action::TileListNext).key);

    let picked = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .selected_library_id
        .clone();
    // **Asked of the list rather than named**, because which id is row 1 is not what this test is
    // about — it is about the press that walks not also re-establishing the seed. Naming the id
    // here made the test go red for the 2026-08-20 reversal, which changed nothing it cares about.
    let want = rows(&app).get(1).cloned();
    assert_eq!(
        picked, want,
        "one press of `{}` must land on the SECOND row — the seed was already on the first. \
         Landing on the first means the press that walks also re-armed the selection.",
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

/// **The frame owns position, and no panel is absolutely positioned any more.**
///
/// Panels used to be `PositionType::Absolute` at fixed pixel widths, anchored to the window edges
/// and floating over a camera that owned the whole window — so nothing on screen filled the window,
/// a panel's height was a number rather than a consequence, and the strip needed `GlobalZIndex(101)`
/// to out-rank the panels it overlapped. Two fifths of a 2560x1406 window was ground nothing used.
///
/// This pins the class rather than any one panel: **a panel that goes back to positioning itself
/// fails here**, whichever tab it lands on. The `Hovered` clause is the second half, and it is not
/// decoration — `view::over_ui` and `view::drive` ask "is the pointer on the interface" by looking
/// for any true `Hovered`, so a frame node carrying one answers yes for the whole window and the map
/// silently stops taking clicks.
#[test]
fn the_frame_owns_position_and_carries_no_hover() {
    let root = Fixture::new("frame-owns-position")
        .descriptor("wall", "alpha")
        .place("wall", (0.0, 0.0))
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let (left, right, slots) = {
        let frame = app
            .world()
            .get_resource::<emerge_mapper::chrome::Frame>()
            .expect("the frame is the layout — no resource means every panel has nowhere to go");
        (
            frame.left,
            frame.right,
            [
                frame.root,
                frame.chrome_bar,
                frame.door_strip,
                frame.viewport,
                frame.status,
            ],
        )
    };

    // Every panel is a child of a dock, and none of them positions itself.
    let mut panels = app
        .world_mut()
        .query_filtered::<(&bevy::ui::Node, &bevy::ecs::hierarchy::ChildOf), With<bevy::picking::hover::Hovered>>();
    let mut docked = 0;
    for (node, parent) in panels.iter(app.world()) {
        if parent.parent() != left && parent.parent() != right {
            continue;
        }
        docked += 1;
        assert_eq!(
            node.position_type,
            bevy::ui::PositionType::Relative,
            "a docked panel that positions itself is the floating-overlay layout coming back"
        );
    }
    assert!(
        docked >= 2,
        "expected the door's panels in the docks, found {docked} — if this ever reads zero the \
         query has stopped seeing panels and the assertion above is vacuous"
    );

    // The frame itself must be invisible to the "is the pointer over UI" question.
    for slot in slots {
        assert!(
            app.world().get::<bevy::picking::hover::Hovered>(slot).is_none(),
            "a frame node carrying `Hovered` answers 'the pointer is on the interface' for the \
             entire window: the map stops taking clicks and the wheel stops zooming, everywhere"
        );
    }
}

/// **The tab strip answers a press, and it is not a `Button`.**
///
/// The chips looked pressable from the day they were written — `UiButton`, `Hovered`, a hover tint
/// — and answered nothing: `on_tab_click` was deleted when the strip became per-door, leaving an
/// affordance advertising a verb it did not have, against `docs/ui.md` §4.2's parity rule.
///
/// Restoring it **as a `Button`** was tried and regressed
/// `the_tile_feedback_script_can_actually_be_followed`, because a focused `ui_widgets::Button` also
/// fires `Activate` on `Enter` and the guide script's commit key changed panel out from under the
/// step. The note left behind concluded it needed "a focus decision, not just an observer"; what it
/// actually needed was for the chip to stop being a `Button`.
///
/// So this pins the shape rather than the behaviour, because the shape is what regressed: a `Tab`
/// carrying `Button` is the `Enter`-steals-the-panel bug returning, and it would be found by a
/// guide script rather than here.
#[test]
fn a_tab_is_not_a_button() {
    let root = Fixture::new("tab-not-a-button")
        .descriptor("wall", "alpha")
        .place("wall", (0.0, 0.0))
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let mut tabs = app
        .world_mut()
        .query_filtered::<(), With<emerge_mapper::tiles::Tab>>();
    let count = tabs.iter(app.world()).count();
    assert!(
        count >= 1,
        "expected the door's strip; found {count} tabs. A query that stops seeing them makes the \
         assertion below vacuous."
    );

    let mut buttons = app.world_mut().query_filtered::<(), (
        With<emerge_mapper::tiles::Tab>,
        With<bevy::ui_widgets::Button>,
    )>();
    assert_eq!(
        buttons.iter(app.world()).count(),
        0,
        "a tab carrying `Button` also fires `Activate` on `Enter`, which is how the commit key came \
         to change panel out from under a guide step. It is a `Pointer<Click>` observer instead."
    );
}

/// **The theme is seeded from the palette, and an empty one would be a fuchsia editor.**
///
/// `UiTheme::default()` is empty; every token miss renders **fuchsia** and warns once. Nothing else
/// in this suite reads a colour back, so a `WidgetsPlugin` that forgot to insert the theme would
/// pass every test and be obvious only on screen — which is the failure mode this whole widget layer
/// is meant to avoid, not demonstrate.
///
/// It also asserts the seeding is *this editor's*, not Feathers'. `docs/ui.md` §5 says of the crate
/// "its visuals are Bevy's editor skin — do not adopt them"; the machinery is adopted and the greys
/// are not, and `PANE_BODY_BG` reading `chrome::PANEL_BG` is what that reconciliation looks like
/// from the outside.
#[test]
fn the_theme_is_seeded_from_the_palette() {
    let root = Fixture::new("theme-seeded")
        .descriptor("wall", "alpha")
        .place("wall", (0.0, 0.0))
        .build("test_map");
    let mut app = harness::build_headless(&root, "test_map", None)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let theme = app
        .world()
        .get_resource::<bevy::feathers::theme::UiTheme>()
        .expect("no `UiTheme` means every token misses and the editor draws fuchsia");
    assert!(
        !theme.0.color.is_empty(),
        "an empty theme is the default, and the default is a fuchsia editor"
    );
    for (token, want, what) in [
        (bevy::feathers::tokens::PANE_BODY_BG, emerge_mapper::chrome::PANEL_BG, "a panel's ground"),
        (bevy::feathers::tokens::WINDOW_BG, emerge_mapper::chrome::VOID, "the window's ground"),
        (bevy::feathers::tokens::TEXT_MAIN, emerge_mapper::chrome::TEXT, "body text"),
        (bevy::feathers::tokens::LISTROW_BG_SELECTED, emerge_mapper::chrome::ROW_SELECTED, "a chosen row"),
    ] {
        assert_eq!(
            theme.0.color.get(&token).copied(),
            Some(want),
            "{what} must come from `chrome`, not from Feathers' own skin"
        );
    }
}

/// **A bar appears only when there is somewhere to scroll, and it appears when there is.**
///
/// Every list in this editor scrolled by wheel and showed **no bar of any kind** — so a pane longer
/// than its panel clipped silently and the author had no way to know. Both halves of the fix are
/// asserted here because each fails differently: a bar that never shows leaves the old defect in
/// place, and a bar that never hides puts furniture on every panel that does not need it.
///
/// The numbers are measured rather than assumed, and the measurement found something. The Compose
/// pane is **2417 px of content in an 833 px viewport** — nearly three screens, almost all of it
/// blank, because that tab still spaces itself with empty `Text` rows (the 2026-08-17 audit's
/// "Compose is a different program", still open as FVS-S-36). It looked like it fit. It does not,
/// and now it says so.
#[test]
fn a_scrollbar_shows_exactly_when_there_is_somewhere_to_scroll() {
    use bevy::ui::Display;

    let measure = |mode: emerge_mapper::tiles::Mode, name: &str| -> Vec<(Display, f32, f32)> {
        let root = Fixture::new(name)
            .descriptor("wall", "alpha")
            .place("wall", (0.0, 0.0))
            .build("m");
        let mut app = harness::build_headless_at(&root, "m", None, mode)
            .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
        for _ in 0..5 {
            app.update();
        }
        let mut q = app.world_mut().query_filtered::<
            (&bevy::ui::Node, &bevy::ui_widgets::Scrollbar),
            bevy::ecs::prelude::With<emerge_mapper::chrome::ScrollTrack>,
        >();
        let targets: Vec<(Display, bevy::ecs::entity::Entity)> =
            q.iter(app.world()).map(|(n, b)| (n.display, b.target)).collect();
        targets
            .into_iter()
            .filter_map(|(d, t)| {
                let c = app.world().get::<bevy::ui::ComputedNode>(t)?;
                Some((d, c.size().y - c.scrollbar_size.y, c.content_size().y))
            })
            // A hidden panel lays out at zero and answers nothing; the live one is the subject.
            .filter(|(_, visible, _)| *visible > 1.0)
            .collect()
    };

    let compose = measure(emerge_mapper::tiles::Mode::Compose, "bar-shows");
    assert!(
        !compose.is_empty(),
        "expected the Compose pane's track; found none, which would make the assertion vacuous"
    );
    for (display, visible, content) in &compose {
        assert!(
            content > visible,
            "the Compose pane was measured at {content} over {visible} — if it now fits, this test \
             is asserting the wrong half and should move to a pane that does not"
        );
        assert_eq!(
            *display,
            Display::Flex,
            "content past the fold with no bar is the defect this replaced: the pane clips, the \
             wheel works, and nothing on screen admits either"
        );
    }

    let map = measure(emerge_mapper::tiles::Mode::Map, "bar-hides");
    let fitting: Vec<_> = map.iter().filter(|(_, v, c)| c <= v).collect();
    assert!(
        !fitting.is_empty(),
        "expected at least one pane whose content fits, to prove the bar hides; found none"
    );
    for (display, visible, content) in fitting {
        assert_eq!(
            *display,
            Display::None,
            "a pane holding {content} in {visible} has nowhere to scroll, and a bar there is \
             furniture on every panel that does not need one"
        );
    }
}

/// **The editor says what its panels are, to something that cannot see them.**
///
/// There was no accessibility at all: `AccessibilityNode` appeared **zero times** in `src/`, so a
/// screen reader met this application as an unlabelled tree of boxes. The strip is the right place
/// to start, because the one thing a reader most needs is which panel you are in and what the
/// alternatives are — and AccessKit has exactly that shape, a `TabList` of `Tab`s.
///
/// The label is asserted against `Mode::label()` rather than a string, because a second copy of a
/// panel's name is a second thing to rename: `chrome::key_census` keeps the same rule for chords.
#[test]
fn the_tab_strip_describes_itself() {
    let root = Fixture::new("strip-a11y")
        .descriptor("wall", "alpha")
        .place("wall", (0.0, 0.0))
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    let mut lists = app.world_mut().query::<&bevy::a11y::AccessibilityNode>();
    let roles: Vec<accesskit::Role> = lists.iter(app.world()).map(|n| n.0.role()).collect();
    assert!(
        roles.contains(&accesskit::Role::TabList),
        "the door's strip has no `TabList` role, so nothing that cannot see the screen can tell \
         these boxes are the way between panels: {roles:?}"
    );
    let tabs = roles.iter().filter(|r| **r == accesskit::Role::Tab).count();
    assert!(
        tabs >= 1,
        "a `TabList` with no `Tab` in it describes nothing; found {tabs}"
    );

    let mut labelled = app
        .world_mut()
        .query::<(&emerge_mapper::tiles::Tab, &bevy::prelude::AccessibleLabel)>();
    let pairs: Vec<(String, String)> = labelled
        .iter(app.world())
        .map(|(t, l)| (t.0.label().to_owned(), l.0.clone()))
        .collect();
    assert!(!pairs.is_empty(), "no tab carries a label to read out");
    for (mode, label) in pairs {
        assert_eq!(
            label, mode,
            "the spoken label and the drawn one must be the same string, or renaming a panel \
             renames it in one place and not the other"
        );
    }
}

/// **The way out asks a question, and every door can show it.**
///
/// Reported at the keyboard, 2026-08-18: *"the command o button doesn't work when I click on it or
/// when I press the shortcut key."* Both fired correctly. `editor::leave_for_menu` arms
/// `EditorState::leaving` and writes the question into `EditorState::status` — and
/// `notice::paint_notices` picks a status **by tab**, so on the Kit and Rigs doors the words went to
/// a status nothing renders. The author pressed the key, saw nothing, and never learned that `Esc`
/// was waiting to confirm. A dead key and an invisible prompt are indistinguishable from the outside,
/// which is why this is asserted on the door where it broke rather than on the Map.
#[test]
fn the_leaving_question_is_visible_on_a_door_that_is_not_the_map() {
    let root = Fixture::new("leaving-visible")
        .descriptor("wall", "alpha")
        .place("wall", (0.0, 0.0))
        .build("m");
    // The Kit door: the one whose panel shows `ImportState`, not `EditorState`.
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    assert!(
        !app.world()
            .resource::<emerge_mapper::confirm::Confirm>()
            .is_open(),
        "nothing is being asked yet, so no prompt should be up"
    );

    // What `Cmd+O` and the `< kits & maps` click both call. The question moved to
    // `crate::confirm`'s modal, so this asserts on THAT rather than on the band — see below.
    app.world_mut().resource_scope(
        |world, mut confirm: Mut<emerge_mapper::confirm::Confirm>| {
            let mut state = world.resource_mut::<emerge_mapper::editor::EditorState>();
            emerge_mapper::editor::leave_for_menu(false, &mut state, &mut confirm);
        },
    );
    app.update();

    // **The band stays empty now, and the modal is what carries the question.** The old assertion
    // here was that `chrome::LeavingPrompt` lit up; that band was the whole reason this prompt
    // spoke a different language from the chooser's and the labeller's, so it went with them —
    // see `crate::confirm`.
    assert!(
        app.world()
            .resource::<emerge_mapper::confirm::Confirm>()
            .asking(emerge_mapper::confirm::Asked::LeaveMap),
        "leaving a door must raise the one prompt; a door that arms `leaving` and shows nothing \
         reads as a dead key rather than as a missing question"
    );
}

/// **A receipt does not follow you to the next tab, and a problem does.**
///
/// Reported at the keyboard 2026-08-18 as *"click on Tiles and it rotates our selected mesh"*. It
/// does not: driven over BRP, the preview's rotation quaternion is identical on both tabs and
/// `library.ron` is never written. What followed the author across was the **note** — one `String`
/// on the `ImportState` that Meshes and Tiles share, only ever overwritten — still reading
/// `lamp_tall 270,270,180 deg` from a turn made minutes earlier. A message announcing a rotation,
/// on a tab just arrived at, beside a piece genuinely lying on its side, is a complete story.
///
/// Both halves are pinned, because clearing the wrong one would be the more expensive bug: a
/// refusal that vanished on a tab switch is how an author never learns why a save did not happen.
#[test]
fn a_note_does_not_survive_a_tab_change_but_a_problem_does() {
    let root = Fixture::new("sticky_note")
        .pack("alpha/scan", &["spare"])
        .descriptor("alpha/floor", "alpha")
        .build("m");
    let mut app =
        harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles)
            .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    {
        let mut state = app
            .world_mut()
            .resource_mut::<emerge_mapper::tiles::ImportState>();
        state.status.note("alpha/floor 270,270,180 deg".to_owned());
        state.status.problem("NOT SAVED: disk is full".to_owned());
    }
    // **Compose, and the choice of destination is what makes this test able to fail at all.**
    //
    // Arriving on Tiles writes its own line ("building `alpha/tile_1` — ...") and arriving on
    // Meshes writes "loading spare.glb …", so with either as the destination the stale note is
    // overwritten whether or not anything cleared it. Two earlier versions of this test did exactly
    // that and passed with the fix commented out. Compose does not touch `ImportState::status`, so
    // what is in the note after landing there is only ever what survived the switch.
    //
    // Verified both ways: with `clear_note` commented out this reads back
    // "alpha/floor 270,270,180 deg"; with it in, the empty string.
    let slot = emerge_mapper::keys::binding(
        emerge_mapper::keys::Action::tab_slot(2).unwrap_or_else(|| panic!("no third slot")),
    )
    .key;
    // **`PreUpdate`, after Bevy's own input pass.** `keyboard_input_system` clears `just_pressed`
    // at the top of the frame, so a press written before `update()` is gone before any `Update`
    // system sees it — the trap `docs/bevy_debugger_mcp.md` records and the shape every other
    // key-driving test in this file uses.
    app.add_systems(
        PreUpdate,
        IntoScheduleConfigs::after(
            move |mut input: ResMut<bevy::input::ButtonInput<KeyCode>>, mut done: Local<bool>| {
                if !*done {
                    input.release_all();
                    input.press(slot);
                    *done = true;
                }
            },
            bevy::input::InputSystems,
        ),
    );
    app.update();
    app.update();

    let state = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>();
    assert!(
        *app.world().resource::<emerge_mapper::tiles::Mode>()
            != emerge_mapper::tiles::Mode::Tiles,
        "the slot key must actually have changed tab, or this test proves nothing"
    );
    // Asserted on the stale text rather than on emptiness, because the rule is "a receipt does not
    // outlive the tab that earned it" — not "a tab arrives silent". `enter_tab` clears before the
    // arriving tab gets to speak, so a tab with something to say still says it.
    let note = state.status.note_text();
    assert!(
        !note.contains("270,270,180"),
        "the rotate receipt followed the author to the next tab, which is the whole bug — it reads \
         as something the tab switch just did. Note now: {note:?}"
    );
    assert_eq!(
        state.status.problem_text(),
        "NOT SAVED: disk is full",
        "a problem is a state the editor is IN — clearing it on a tab change would lose the one \
         message an author most needs to still be there"
    );
}

/// **The Meshes stage stands a piece where the shipped spawner will, pivot and all.**
///
/// It used to stage at `STAGE.xz - align.pivot`, centring the bounding box on the placement point.
/// Measured over BRP 2026-08-18 against the live editor, that put the same piece **0.42 m** from
/// where the Tiles tab stands it — and the Tiles tab was right: `emerge_bevy::spawn_descriptor` puts
/// the file's origin on the placement point and applies no pivot, and it is the spawner a map
/// placement and a tile member both go through. `src/placement/furnish.rs:431` is the one caller
/// that does apply `- rot * pivot`, and the mapper does not author for it.
///
/// Chosen at the keyboard: preview the path this editor's output actually takes. The visible
/// consequence is deliberate — a mesh whose origin is not its bounding-box centre now sits off its
/// own footprint rectangle here, which is what it will do in the game.
///
/// The `y_offset` half is asserted in the same breath because the two are one decision: the height
/// **is** carried (`stack::datum` adds it to every placed piece), the XZ shift is not.
#[test]
fn the_mesh_stage_stands_a_piece_where_the_spawner_will() {
    use emerge_mapper::tiles::STAGE;

    const PIVOT: (f32, f32) = (0.31, 0.22);
    const Y_OFFSET: f32 = 0.4;

    let root = Fixture::new("stage_pivot")
        .pack("alpha/scan", &["spare"])
        .descriptor("alpha/floor", "alpha")
        .build("m");
    let mut app =
        harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }
    {
        app.world_mut()
            .resource_mut::<emerge_mapper::tiles::ImportState>()
            .selected_library_id = Some("alpha/floor".to_owned());
        let mut project = app
            .world_mut()
            .resource_mut::<emerge_mapper::project::Project>();
        // **Both lists.** `drive_preview` stages from `measured` (the tab describes what was
        // measured); `library` is what the row selection names. Setting one only is how the first
        // run of this test read back y = 0.0 and looked like the offset had stopped working.
        let mut touched = 0;
        let project = &mut *project;
        for list in [
            &mut project.measured.descriptors,
            &mut project.library.descriptors,
        ] {
            for d in list.iter_mut().filter(|d| d.id == "alpha/floor") {
                // A piece whose geometry is NOT centred on its file origin — the only case pivot
                // changes anything.
                d.align.pivot = Some(PIVOT);
                d.align.y_offset = Some(Y_OFFSET);
                touched += 1;
            }
        }
        assert!(touched > 0, "the fixture descriptor must be in one of the two lists");
    }
    for _ in 0..8 {
        app.update();
    }

    let mut q = app.world_mut().query::<&Transform>();
    let staged: Vec<Vec3> = q
        .iter(app.world())
        .map(|t| t.translation)
        // Only things standing on this stage; the stage camera sits twelve metres above and back.
        .filter(|v| (v.x - STAGE.x).abs() < 2.0 && (v.z - STAGE.z).abs() < 2.0 && v.y < 2.0)
        .collect();

    let want = Vec3::new(STAGE.x, STAGE.y + Y_OFFSET, STAGE.z);
    assert!(
        staged.iter().any(|v| (*v - want).length() < 1e-3),
        "no staged piece stands at {want:?} — the pivot is being applied again, or the y_offset \
         stopped being. Found: {staged:?}"
    );
    assert!(
        !staged
            .iter()
            .any(|v| (v.x - (STAGE.x - PIVOT.0)).abs() < 1e-3),
        "a piece is staged at STAGE.x - pivot.0, which is the shift `spawn_descriptor` never \
         applies — the preview is promising a position nothing will honour. Found: {staged:?}"
    );
}

// ── The key badges ───────────────────────────────────────────────────────────────────────────────
//
// The held-`K` overlay used to be a centred table of every chord live in the tab. It is now a badge
// drawn on the thing each chord acts on (`emerge_mapper::badges`), and these are what stop the
// promise underneath it — *nothing falls off* — from being a hope.
//
// `keys::tests::every_live_binding_gets_exactly_one_badge` is the arithmetic half and needs no app.
// What only a booted editor can answer is whether the places the census names are actually **on
// screen**, which is the failure mode that would silently drop a verb.

/// Hold a key down for the rest of the test.
///
/// **Latched**, because a system that presses every frame passes or fails on Bevy's arbitrary
/// system ordering — two tests here did exactly that and flipped when an unrelated system was added.
/// `ButtonInput::clear` in `PreUpdate` clears `just_pressed` and leaves `pressed`, so one press is a
/// held key for as long as the app runs.
fn hold(app: &mut App, keys_down: Vec<KeyCode>) {
    app.add_systems(
        Update,
        bevy::prelude::IntoScheduleConfigs::before(
            move |mut input: ResMut<bevy::input::ButtonInput<KeyCode>>,
                  mut done: Local<bool>| {
                if !*done {
                    for k in &keys_down {
                        input.press(*k);
                    }
                    *done = true;
                }
            },
            emerge_mapper::keys::Phase::Act,
        ),
    );
}

/// The five panels a badge overlay has to serve, each with the door that shows it.
///
/// **`tiles::Mode::ALL`, not a list typed out here.** It was `const TABS: [Mode; 5] = [..]`, written
/// by hand: adding a `Mode` variant compiled, dropped the new panel from the eleven tests that loop
/// this, and *lowered* every `checked >= TABS.len()` anti-vacuity floor in them at the same time —
/// so a new panel made the suite weaker and nothing said so. `Mode::ALL` is generated with the enum
/// by `keys::enumerated`, so there is nowhere to leave a sixth one out of.
const TABS: [emerge_mapper::tiles::Mode; emerge_mapper::tiles::Mode::ALL.len()] =
    emerge_mapper::tiles::Mode::ALL;

/// An editor open on `mode` with the shortcut key held, stepped until its layout is real.
fn badges_up(root: &std::path::Path, mode: emerge_mapper::tiles::Mode) -> App {
    let mut app = harness::build_headless_at(root, "m", None, mode).unwrap_or_else(|e| panic!("{e}"));
    hold(
        &mut app,
        vec![emerge_mapper::keys::binding(emerge_mapper::keys::Action::Shortcuts).key],
    );
    // Layout lands in `PostUpdate` after the frame that spawns a cluster, and `place_badges` reads it
    // in `Update` — so the reveal is one frame behind the rebuild by construction. Several frames,
    // because the panels themselves are change-gated rebuilds.
    for _ in 0..8 {
        app.update();
    }
    app
}

/// Which `ControlId`s are laid out right now — the fact that decides whether a verb sits on its
/// control or joins the legend.
///
/// **Exactly one, not at-least-one**, because that is what `badges::sole_control` answers and this
/// helper feeds `badges::resolve`. Asking a different question here would let a duplicated id keep
/// `Home::Control` in the expectation while the editor sent it to the legend — the same two-predicate
/// drift `sole_control` exists to close, reintroduced in the test that checks it.
fn controls_on_screen(app: &mut App) -> Vec<emerge_mapper::keys::ControlId> {
    use bevy::ui::ComputedNode;
    let mut q = app
        .world_mut()
        .query::<(&emerge_mapper::chrome::Control, &ComputedNode)>();
    let world = app.world();
    emerge_mapper::keys::ControlId::ALL
        .into_iter()
        .filter(|id| {
            q.iter(world)
                .filter(|(c, node)| c.0 == *id && node.size() != Vec2::ZERO)
                .count()
                == 1
        })
        .collect()
}

/// **Put a piece in the detail pane**, because that is when the paned controls exist at all — the
/// pane draws nothing until something is selected, and an overlap rule enforced against an empty
/// pane is enforced against a different, smaller layout than the one an author works in.
///
/// # It could stage nothing and say nothing
///
/// It was a pair of `if let`s over an `Option`, so a fixture with an empty library — or a tab with
/// no `ImportState` — silently did nothing at all. Seven tests call this and every one of them
/// filters `Visibility::Hidden` clusters out, so their `checked >=` floors were satisfiable by
/// exactly the unstaged, smaller layout the note above calls the wrong one to measure. And it
/// stepped four frames where `badges_up` steps eight, for the reason `badges_up` gives: the panels
/// are change-gated rebuilds and the reveal is a frame behind the rebuild. Four is a different
/// layout again.
///
/// So: loud when there is nothing to stage, and it checks that the staging landed.
fn stage_a_piece(app: &mut App) {
    let id = app
        .world()
        .resource::<emerge_mapper::project::Project>()
        .library
        .descriptors
        .first()
        .map(|d| d.id.clone())
        .unwrap_or_else(|| {
            panic!(
                "stage_a_piece was handed a project with an empty library, so there is nothing to \
                 put in the detail pane and every rule the caller is about would be enforced \
                 against an empty one. Give the fixture a descriptor."
            )
        });
    match app
        .world_mut()
        .get_resource_mut::<emerge_mapper::tiles::ImportState>()
    {
        Some(mut state) => state.selected_library_id = Some(id.clone()),
        None => panic!(
            "no `ImportState` to stage `{id}` in — the resource the whole detail pane is drawn from \
             is absent, and staging silently did nothing before this said so"
        ),
    }
    // Eight, for parity with `badges_up`: same reason, same number.
    for _ in 0..8 {
        app.update();
    }
    assert_eq!(
        app.world()
            .resource::<emerge_mapper::tiles::ImportState>()
            .selected_library_id
            .as_deref(),
        Some(id.as_str()),
        "`{id}` did not stay staged, so the pane below is not the one an author works in"
    );
    // **And the pane really grew controls, on the two tabs that have any inside a fold.**
    //
    // `chrome::Control(Detail)` sits *on* the scrolling pane on every tab, so the controls that live
    // *inside* one are the Meshes and Tiles detail builders' — `IdField`, `Mount`, `Tags`, `Mesh`,
    // `CellGrid`, `Tile`, `Grid`, `Members`. Those are exactly the tabs whose pane is drawn from
    // `ImportState`, so those are the tabs where a no-op stage is visible, and the fold rules the
    // callers check have nothing to measure anywhere else.
    let mode = *app.world().resource::<emerge_mapper::tiles::Mode>();
    if matches!(
        mode,
        emerge_mapper::tiles::Mode::Meshes | emerge_mapper::tiles::Mode::Tiles
    ) {
        assert!(
            paned_controls(app) > 0,
            "{}: `{id}` is staged and not one laid-out `chrome::Control` sits inside a scrolling \
             pane, so the detail block was not drawn and every fold rule below measures nothing",
            mode.label()
        );
    }
}

/// How many laid-out `chrome::Control` nodes sit **inside** a `ScrollArea` — the same walk
/// `badges::fold_of` makes, counted. Strict about *inside*: a node that **is** the pane is a whole
/// control with open ground beside it, not a row within one.
fn paned_controls(app: &mut App) -> usize {
    use bevy::ui::ComputedNode;
    let named: Vec<Entity> = {
        let mut q = app
            .world_mut()
            .query::<(Entity, &emerge_mapper::chrome::Control, &ComputedNode)>();
        q.iter(app.world())
            .filter(|(_, _, n)| n.size() != Vec2::ZERO)
            .map(|(e, ..)| e)
            .collect()
    };
    let world = app.world();
    named
        .into_iter()
        .filter(|e| {
            let mut up = world.get::<ChildOf>(*e).map(|p| p.parent());
            while let Some(x) = up {
                if world.get::<bevy::ui_widgets::ScrollArea>(x).is_some() {
                    return true;
                }
                up = world.get::<ChildOf>(x).map(|p| p.parent());
            }
            false
        })
        .count()
}

/// **Scroll every pane to `y` logical pixels**, and say how many took the write.
///
/// `ScrollPosition` is logical where `ComputedNode`/`UiGlobalTransform` are physical, and
/// `ui_layout_system` clamps it to `content - size` and floors it — so a pane whose content fits
/// stays where it is however large a number it is handed. That is why a caller asks for several
/// offsets rather than assuming one lands.
///
/// Nothing in the editor writes these back over us: the three followers are armed by
/// `chrome::Follow` on a *selection change*, and the panes that hold the paned controls
/// (`DetailPane`, `ComposeBody`, `SlotPane`) carry no follower at all — each says `FOLLOW-OK:` at
/// its spawn.
fn scroll_every_pane(app: &mut App, y: f32) -> usize {
    let panes: Vec<Entity> = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<bevy::ui_widgets::ScrollArea>>();
        q.iter(app.world()).collect()
    };
    let mut wrote = 0;
    for e in panes {
        if let Some(mut at) = app.world_mut().get_mut::<bevy::ui::ScrollPosition>(e) {
            at.0.y = y;
            wrote += 1;
        }
    }
    wrote
}

/// What should be drawn on this tab, in this stance — the census, resolved against what is on screen
/// through `badges::resolve`, which is the editor's own rule rather than a second copy of it.
fn expected_badges(
    app: &mut App,
    mode: emerge_mapper::tiles::Mode,
) -> Vec<emerge_mapper::keys::Badge> {
    let stance = app.world().resource::<emerge_mapper::keys::Live>().1;
    let on_screen = controls_on_screen(app);
    // **The door trims before the home resolves**, in that order, because that is the order
    // `badges::rebuild_badges` does it in — `2` and `3` are `Context::Global` but the strip they act
    // on is the door's, so on a one-panel door they are not live at all.
    let panels = emerge_mapper::tiles::Door::showing(mode).tabs().len();
    emerge_mapper::keys::badges(mode.context(), stance)
        .into_iter()
        .chain(emerge_mapper::keys::badges(
            emerge_mapper::keys::Context::Global,
            stance,
        ))
        .filter_map(|b| b.on_a_door_of(panels))
        .map(|mut b| {
            b.home = emerge_mapper::badges::resolve(b.home, &on_screen);
            b
        })
        .collect()
}

/// **Holding the key labels everything this tab can do, and nothing it cannot.**
///
/// The counts, per tab, against the census's own answer — so a verb that stopped being drawn fails
/// here rather than being reported missing from the keyboard two sessions later, which is what
/// happened to `R` and `Shift+Delete` under the old list.
#[test]
fn holding_k_puts_a_badge_on_everything_this_tab_can_do() {
    use emerge_mapper::badges::{Badge, BadgeCluster};

    let root = Fixture::new("badgecount")
        .descriptor("floor", "alpha")
        .build("m");

    for mode in TABS {
        let mut app = badges_up(&root, mode);
        let want = expected_badges(&mut app, mode);

        // **Every `Text` beneath the badge, not just its direct children.** The legend's description
        // sits inside a wrapper that constrains its width — see `badges.rs` — so a one-level read
        // sees the chord and misses the words, which is indistinguishable from the words being gone.
        let drawn: Vec<String> = {
            let mut q = app.world_mut().query_filtered::<Entity, With<Badge>>();
            let roots: Vec<Entity> = q.iter(app.world()).collect();
            let world = app.world();
            roots
                .into_iter()
                .map(|root| {
                    let mut out = String::new();
                    let mut queue = vec![root];
                    while let Some(e) = queue.pop() {
                        if let Some(t) = world.get::<Text>(e) {
                            out.push_str(&t.0);
                        }
                        if let Some(kids) = world.get::<Children>(e) {
                            // Reversed, so a depth-first walk reads left to right.
                            queue.extend(kids.iter().rev());
                        }
                    }
                    out
                })
                .collect()
        };
        let clusters = {
            let mut q = app.world_mut().query::<&BadgeCluster>();
            q.iter(app.world()).count()
        };

        let mut homes: Vec<emerge_mapper::keys::Home> = Vec::new();
        for b in &want {
            if !homes.contains(&b.home) {
                homes.push(b.home);
            }
        }
        assert_eq!(
            clusters,
            homes.len(),
            "{}: {} anchor(s) named by the census, {clusters} cluster(s) drawn",
            mode.label(),
            homes.len()
        );

        let mut drawn_sorted = drawn.clone();
        drawn_sorted.sort();
        // **A badge in a dock carries its words; a badge in a band is the bare chord.**
        //
        // One shape per side — see `keys::ControlId::in_a_band`. Expecting the chord alone everywhere
        // was the first cut, and it is what let `Enter` sit beside a list of filenames saying nothing
        // about adding a piece to the library.
        let mut want_sorted: Vec<String> = want
            .iter()
            .map(|b| match b.home {
                emerge_mapper::keys::Home::Control(id) if id.in_a_band() => b.chord.clone(),
                _ => format!("{}{}", b.chord, b.does),
            })
            .collect();
        want_sorted.sort();
        assert_eq!(
            drawn_sorted, want_sorted,
            "{}: the badges on screen are not the chords the census says are live",
            mode.label()
        );
    }
}

/// **Every control the census homes a verb at is on screen, or that verb is in the legend** — the
/// one that matters.
///
/// A `ControlId` is only allowed to name a node that is laid out for the *whole* of every state some
/// binding homes to it in — and where the design says a control is genuinely absent
/// (`ControlId::Grid` with no tile open says so in its own doc), `badges::resolve` has to send its
/// verbs to the legend and the legend has to be there to take them. Two visible nodes claiming one
/// id is a bug in either case. So this runs against **two** projects, one with a piece to select and
/// one with nothing in it at all: a pane that renders nothing until something is selected drops its
/// badges exactly when a new author needs them.
///
/// Zero size is the visibility test, because `chrome::panel_root`'s hidden form is `Display::None`
/// and a node that is not displayed is never laid out.
///
/// # It could only ever detect a duplicate
///
/// It walked `expected_badges`, which passes every home through `badges::resolve` **before** the
/// loop — and `resolve`'s entire job is to turn a home that is not on screen into `Home::Legend`.
/// The loop then skipped every non-`Control` home. So an absent control was demoted before the
/// check and filtered out after it: `visible >= 1` held by construction, only `visible >= 2` was
/// reachable, and the documented failure — a pane that draws nothing until something is selected —
/// was precisely the case that got skipped. The second, empty fixture added to catch it strictly
/// *reduced* the ids that reached the check.
///
/// It walks the census **unresolved** now: the homes the bindings themselves name, before `resolve`
/// has an opinion. Three things are then asked of each, and each is a different way to be drawn
/// nowhere — two nodes claiming one id, `resolve` failing to demote a control that is not there, and
/// a resolved home with no cluster built for it at all.
#[test]
fn every_control_the_census_homes_a_verb_at_is_on_screen() {
    use bevy::ui::ComputedNode;
    use emerge_mapper::keys::Home;

    let populated = Fixture::new("badgehome")
        .descriptor("floor", "alpha")
        .place("floor", (0.0, 0.0))
        .build("m");
    let empty = Fixture::new("badgehome_empty").build("m");

    let mut missing = Vec::new();
    let mut asked = 0usize;
    for (what, root) in [("a populated kit", &populated), ("an empty kit", &empty)] {
        for mode in TABS {
            let mut app = badges_up(root, mode);
            // **The census's own homes, not `expected_badges`'.** That helper resolves, and
            // resolving is the step this must happen before — see the note above. The door trim and
            // the `Context::Global` chain are `rebuild_badges`' own, in its order.
            let stance = app.world().resource::<emerge_mapper::keys::Live>().1;
            let panels = emerge_mapper::tiles::Door::showing(mode).tabs().len();
            let named: Vec<Home> = emerge_mapper::keys::badges(mode.context(), stance)
                .into_iter()
                .chain(emerge_mapper::keys::badges(
                    emerge_mapper::keys::Context::Global,
                    stance,
                ))
                .filter_map(|b| b.on_a_door_of(panels))
                .map(|b| b.home)
                .collect();
            // **Every cluster the overlay built, placed or not.** Existence rather than
            // `Visibility`: `place_badges` may legitimately leave a box unplaced on a screen with
            // no ground for it, and that is `no_badge_cluster_draws_through_another`'s subject
            // rather than this one. What is asked here is whether the verb was given a home at
            // all — a census entry with no cluster anywhere is a verb the author cannot reach by
            // any route.
            let built: Vec<Home> = {
                let mut q = app
                    .world_mut()
                    .query::<&emerge_mapper::badges::BadgeCluster>();
                q.iter(app.world()).map(|c| c.0).collect()
            };
            // The editor's own answer to *"is this control on screen"*, which is what decides where
            // the badge goes. Compared against the direct count below, so the two cannot drift.
            let on_screen = controls_on_screen(&mut app);
            for home in named {
                let Home::Control(id) = home else { continue };
                asked += 1;
                let visible = {
                    let mut q = app
                        .world_mut()
                        .query::<(&emerge_mapper::chrome::Control, &ComputedNode)>();
                    q.iter(app.world())
                        .filter(|(c, node)| c.0 == id && node.size() != Vec2::ZERO)
                        .count()
                };
                let resolved = emerge_mapper::badges::resolve(home, &on_screen);
                let want = if visible == 1 { home } else { Home::Legend };
                if visible > 1 {
                    missing.push(format!(
                        "{what}, {}: {id:?} is laid out {visible} times; two visible nodes claiming \
                         one id is a bug rather than a tie to break, and `badges::sole_control` \
                         answers `None` to both of its callers when it happens",
                        mode.label()
                    ));
                } else if resolved != want {
                    missing.push(format!(
                        "{what}, {}: {id:?} is laid out {visible} time(s) and `badges::resolve` \
                         answered {resolved:?} rather than {want:?}. A control that is not on \
                         screen must be demoted to the legend — a verb that keeps `Home::Control` \
                         with nothing to stand on is drawn neither on a control nor beside its own \
                         prose, which is the one outcome `Home` exists to rule out.",
                        mode.label()
                    ));
                } else if !built.contains(&want) {
                    missing.push(format!(
                        "{what}, {}: {id:?} resolves to {want:?} and no cluster was built for it, \
                         so the verb is drawn nowhere at all",
                        mode.label()
                    ));
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "the census homes verbs where they cannot be read. Either the panel stopped drawing the \
         node, or two nodes claim one id, or the legend that catches the absent ones is not \
         there:\n  {}",
        missing.join("\n  ")
    );
    // Ten `badges_up` boots contribute; a tab whose census names no control at all would be a
    // finding of its own, and zero across all ten means the census stopped answering.
    assert!(
        asked >= 2 * TABS.len(),
        "only {asked} control home(s) were asked about across two fixtures and {} tabs — the \
         census has stopped naming any, so nothing above was checked",
        TABS.len()
    );
}

/// **No badge leaves the window, and the legend does not leave the viewport.**
///
/// A cluster is placed from a rect and then clamped, and the clamp is the whole reason a control in
/// the left dock — whose leading edge has no room beside it — gets a badge on its own edge rather
/// than one drawn off the screen where nobody can read it.

#[test]
fn no_badge_leaves_the_window() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use emerge_mapper::badges::{BadgeCluster, BadgeLayer};

    let root = Fixture::new("badgeclamp")
        .descriptor("floor", "alpha")
        .build("m");

    let mut checked = 0usize;
    for mode in TABS {
        let mut app = badges_up(&root, mode);
        let stage = {
            let viewport = app.world().resource::<emerge_mapper::chrome::Frame>().viewport;
            let mut q = app.world_mut().query::<(&ComputedNode, &UiGlobalTransform)>();
            q.get(app.world(), viewport)
                .ok()
                .map(|(n, tf)| Rect::from_center_size(tf.translation, n.size()))
                .filter(|r| r.size() != Vec2::ZERO)
        };
        let window = {
            let mut q = app
                .world_mut()
                .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<BadgeLayer>>();
            q.iter(app.world())
                .map(|(n, tf)| Rect::from_center_size(tf.translation, n.size()))
                .find(|r| r.size() != Vec2::ZERO)
        };
        let Some(window) = window else {
            // No laid-out layer means no badges to check, and `holding_k_puts_a_badge_on_everything`
            // is what says they exist at all — asserting here too would be one failure reported twice.
            continue;
        };
        let mut out = Vec::new();
        {
            let mut q = app
                .world_mut()
                .query::<(&BadgeCluster, &ComputedNode, &UiGlobalTransform, &Visibility)>();
            for (cluster, node, tf, vis) in q.iter(app.world()) {
                if *vis == Visibility::Hidden || node.size() == Vec2::ZERO {
                    continue;
                }
                checked += 1;
                let rect = Rect::from_center_size(tf.translation, node.size());
                // The legend is clamped to the world's own hole — itself bounded by the window —
                // and a control's badge to the window.
                let bound = match cluster.0 {
                    emerge_mapper::keys::Home::Legend => {
                        stage.map(|s| s.intersect(window)).unwrap_or(window)
                    }
                    emerge_mapper::keys::Home::Control(_) => window,
                };
                // **Only where the bound could hold it.** A cluster wider than the ground it is
                // clamped into overflows by arithmetic — `clamp` cannot put a 583 px legend inside a
                // 500 px viewport — and the headless window is small enough to reach that. The
                // clamp's promise is that a cluster starts inside its bound and ends inside it when
                // there is room; asserting more would be asserting the window is big.
                let fits = bound.size().cmpge(rect.size()).all();
                if rect.min.x < bound.min.x - 0.5
                    || rect.min.y < bound.min.y - 0.5
                    || (fits && (rect.max.x > bound.max.x + 0.5 || rect.max.y > bound.max.y + 0.5))
                {
                    out.push(format!(
                        "{} {:?}: {rect:?} outside {bound:?}",
                        mode.label(),
                        cluster.0
                    ));
                }
            }
        }
        assert!(
            out.is_empty(),
            "these badge clusters are drawn where they cannot be read:\n  {}",
            out.join("\n  ")
        );
    }
    // **The companion assertion.** Every clause above is a `continue` or a filter, so a layer that
    // stopped being laid out — or clusters that never came out of `Visibility::Hidden` — would make
    // this pass while checking nothing.
    assert!(
        checked >= TABS.len(),
        "only {checked} placed cluster(s) were measured across {} tabs; the clamp is being enforced \
         against nothing",
        TABS.len()
    );
}

/// **The badge layer never answers the pointer.**
///
/// `view::over_ui` and `view::drive` ask *"is the pointer on the interface"* by looking for any true
/// `Hovered`, so one on a node covering the window would answer yes everywhere — the map would stop
/// taking clicks and the wheel would stop zooming, with nothing on screen to point at. It is exactly
/// why `chrome::chip` cannot be reused for a badge: it spawns `Hovered` along with its `Button`.
#[test]
fn the_badge_layer_never_answers_the_pointer() {
    use emerge_mapper::badges::BadgeLayer;

    let root = Fixture::new("badgehover")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app = badges_up(&root, emerge_mapper::tiles::Mode::Map);

    // **Every node under the layer, not the three that carry a marker.** The wrapper the
    // description's `Text` is measured inside carries no marker at all, so an `Or<With<..>>` query
    // could not see it — and it was the one node in the subtree spawned without `Pickable::IGNORE`,
    // which is exactly the node `build_hover_map` blocks on by default. The claim is about the
    // subtree, so the test walks the subtree.
    let mut hoverable: Vec<Entity> = Vec::new();
    let mut walked = 0usize;
    let mut clusters = 0usize;
    {
        let mut q = app.world_mut().query_filtered::<Entity, With<BadgeLayer>>();
        let mut stack: Vec<Entity> = q.iter(app.world()).collect();
        while let Some(id) = stack.pop() {
            walked += 1;
            if app
                .world()
                .get::<emerge_mapper::badges::BadgeCluster>(id)
                .is_some()
            {
                clusters += 1;
            }
            if app
                .world()
                .get::<bevy::picking::hover::Hovered>(id)
                .is_some()
            {
                hoverable.push(id);
            }
            if let Some(kids) = app.world().get::<Children>(id) {
                stack.extend(kids.iter());
            }
        }
    }
    assert!(
        hoverable.is_empty(),
        "{} node(s) of the badge layer carry `Hovered` ({hoverable:?}); a layer over the whole \
         window that answers the pointer kills map zoom and click-to-place everywhere",
        hoverable.len()
    );
    // **The anti-vacuity floor, which all five geometric siblings carry and this did not.**
    //
    // Every clause above is an absence, so a layer that was never spawned — or a walk that reached
    // no children — reads exactly the same as a layer that answers no pointer. The subtree is the
    // subject: a box, its chord, and the wrapper the description is measured inside, which is the
    // node this test was widened to see in the first place.
    assert!(
        walked > 1,
        "the walk visited {walked} node(s) under the badge layer, so it is asserting the absence of \
         something that was never drawn"
    );
    assert!(
        clusters > 0,
        "the walk found no `BadgeCluster` under the layer, so the subtree the claim is about was \
         not built — {walked} node(s) were visited"
    );
}

/// **A key still fires while the badges are up**, which is the property the whole design rests on.
///
/// Kurtenbach's principle of rehearsal, quoted in ExposeHK (`10.1145/2470654.2470735`): *"guidance
/// should be a physical rehearsal of the way an expert would issue a command."* If holding `K` had
/// to be released before the key it names would work, the badges would be a cheatsheet with extra
/// steps and every author would rehearse *reading*. `editor::sense_context` never suppressed actions
/// for the old overlay; nothing here may start.
#[test]
fn a_key_still_fires_while_k_is_held() {
    use emerge_mapper::keys::Action;

    let root = Fixture::new("badgelive")
        .descriptor("floor", "alpha")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app = badges_up(&root, emerge_mapper::tiles::Mode::Map);

    let before = app
        .world()
        .resource::<emerge_mapper::editor::EditorState>()
        .brush;
    hold(&mut app, vec![emerge_mapper::keys::binding(Action::PaletteNext).key]);
    for _ in 0..3 {
        app.update();
    }
    let after = app
        .world()
        .resource::<emerge_mapper::editor::EditorState>()
        .brush;
    assert_ne!(
        before, after,
        "walking the palette did nothing while the shortcut key was held — the badges have become a \
         mode you must leave, which is the one thing they must never be"
    );
}

/// **A badge lights while its key is down**, and that is the bridge, not a decoration.
///
/// Cockburn, Gutwin, Scarr & Malacria 2014 (`10.1145/2659796`): a fast path offered beside a slow
/// one does not get adopted on its own, because no single moment hurts enough to justify switching.
/// What works is a bridge where the novice route *rehearses* the expert route — and the badge under
/// your finger lighting on the control is that rehearsal made visible. Its ancestor
/// `chrome::flash_live_rows`, the row-lighting this grew out of, never had a test; this is it.
#[test]
fn a_badge_lights_while_its_key_is_down() {
    use emerge_mapper::badges::Badge;
    use emerge_mapper::keys::Action;

    let root = Fixture::new("badgelit")
        .descriptor("floor", "alpha")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app = badges_up(&root, emerge_mapper::tiles::Mode::Map);
    hold(&mut app, vec![emerge_mapper::keys::binding(Action::PaletteNext).key]);
    for _ in 0..3 {
        app.update();
    }

    let lit: Vec<Vec<Action>> = {
        let mut q = app.world_mut().query::<(&Badge, &BackgroundColor)>();
        q.iter(app.world())
            .filter(|(_, bg)| bg.0 == emerge_mapper::chrome::ROW_SELECTED)
            .map(|(b, _)| b.0.clone())
            .collect()
    };
    // **Two, and the second one is the point.** The walk's badge lights because its key is down —
    // and so does the shortcut key's own badge on the hint line, because that key is down too. The
    // one control whose verb is "show me the verbs" demonstrates itself, which is the cheapest
    // possible answer to ExposeHK's admitted weakness that a modifier-triggered overlay has nothing
    // to announce it.
    assert!(
        lit.iter().any(|a| a.contains(&Action::PaletteNext)),
        "the badge for the key that is down did not light: {lit:?}"
    );
    assert!(
        lit.iter().any(|a| a.contains(&Action::Shortcuts)),
        "the shortcut key is held and its own badge is not lit: {lit:?}"
    );
    assert_eq!(
        lit.len(),
        2,
        "only the badges whose keys are down may light; {} did: {lit:?}",
        lit.len()
    );
}

// **`an_anchored_control_lights_while_k_is_held` lived here.** It pinned a one-pixel `ACCENT`
// outline on every control that anchored a cluster while `K` was held — a second way to say
// "this box is that control's", built before the leaders were. Removed with the system: twenty-odd
// glowing rectangles are the wrong ground to read a hairline against, and
// `every_control_cluster_is_tied_to_its_anchor` is the tie that remains.

/// **The chord is body-size ink; the description stays a footnote.**
///
/// `chrome_census` bans raw font sizes but does not pin roles, so nothing else would notice if the
/// one number this overlay exists to show quietly returned to 9 px. Ink identifies the part: the
/// chord is [`chrome::KEY`], the description [`chrome::DIM`], and no third ink draws in a badge.
#[test]
fn a_badge_chord_reads_at_body_size() {
    use emerge_mapper::badges::Badge;

    let root = Fixture::new("badgetype")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app = badges_up(&root, emerge_mapper::tiles::Mode::Map);

    let body = TextFont::from_font_size(emerge_mapper::chrome::text::BODY).font_size;
    let hint = TextFont::from_font_size(emerge_mapper::chrome::text::HINT).font_size;
    let (mut chords, mut descs) = (0usize, 0usize);
    {
        let mut roots_q = app.world_mut().query_filtered::<Entity, With<Badge>>();
        let roots: Vec<Entity> = roots_q.iter(app.world()).collect();
        let world = app.world();
        for badge in roots {
            let mut queue = vec![badge];
            while let Some(e) = queue.pop() {
                if let (Some(font), Some(color)) = (world.get::<TextFont>(e), world.get::<TextColor>(e))
                {
                    if color.0 == emerge_mapper::chrome::KEY {
                        assert_eq!(font.font_size, body, "a chord away from BODY size");
                        chords += 1;
                    } else if color.0 == emerge_mapper::chrome::DIM {
                        assert_eq!(font.font_size, hint, "a description away from HINT size");
                        descs += 1;
                    } else {
                        panic!("a third ink draws inside a badge: {:?}", color.0);
                    }
                }
                if let Some(kids) = world.get::<Children>(e) {
                    queue.extend(kids.iter().rev());
                }
            }
        }
    }
    assert!(
        chords >= 10 && descs >= 5,
        "checked {chords} chords and {descs} descriptions — too few for the assertion to mean much"
    );
}

/// **Holding a piece puts the member-verbs on the MEMBERS list** — the one stance the two-fixture
/// layout test never visits, driven for real: arm, then drop. A drop continues the hold (`placing`
/// stays true), and it is the drop that gives `Stance::Holding` the member it needs to focus.
#[test]
fn a_held_piece_carries_its_badges_on_the_member_list() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{IntoScheduleConfigs, KeyCode, Local, ResMut, Update};
    use emerge_mapper::badges::{Badge, BadgeCluster};
    use emerge_mapper::keys::{Action, ControlId, Home, Stance, binding};

    let root = Fixture::new("badgehold")
        .sized_descriptor("panel", "alpha", 0.2, 0.2)
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Tiles)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();

    // The tab opens on the Tiles page — name a tile into existence before anything can be held.
    open_tile(&mut app, "tile");

    let once = |app: &mut App, chord: Vec<KeyCode>| {
        app.add_systems(
            Update,
            IntoScheduleConfigs::before(
                move |mut keys: ResMut<ButtonInput<KeyCode>>, mut done: Local<bool>| {
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
    // **A drop IS the hold** — Enter brings the piece in and leaves `placing` true, which is what
    // this stance is about. `Space` first would arm it, but `Enter` is `Idle`-scoped and Space
    // lands in Holding, so the old arm-then-drop pair is refused by the census — the drop alone
    // is the honest path.
    once(&mut app, vec![binding(Action::BuildDrop).key]);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release_all();
    app.update();

    let live = *app.world().resource::<emerge_mapper::keys::Live>();
    assert_eq!(
        live.1,
        Stance::Holding,
        "the drop must leave a piece in hand — `placing` stays true through it, and without that \
         this test drives nothing. (It used to say *arm, drop, arm again*, which is the loop the \
         2026-08-20 stance rules removed: `Enter` is `Idle`-scoped, so the drop alone is the honest \
         path and it is the only key pressed above.)"
    );

    hold(&mut app, vec![binding(Action::Shortcuts).key]);
    for _ in 0..8 {
        app.update();
    }

    assert!(
        controls_on_screen(&mut app).contains(&ControlId::Members),
        "a held piece means an open tile, and an open tile lays out its MEMBERS list"
    );
    let members_cluster: Vec<Entity> = {
        let mut q = app.world_mut().query::<(Entity, &BadgeCluster)>();
        q.iter(app.world())
            .filter(|(_, c)| c.0 == Home::Control(ControlId::Members))
            .map(|(e, _)| e)
            .collect()
    };
    assert_eq!(members_cluster.len(), 1, "one cluster stands on the member list");

    // The three rows a hand gets while holding: move, flush, and the member walk — ten actions.
    let actions: Vec<Action> = {
        let mut q = app.world_mut().query::<(&Badge, &ChildOf)>();
        q.iter(app.world())
            .filter(|(_, parent)| parent.parent() == members_cluster[0])
            .flat_map(|(b, _)| b.0.clone())
            .collect()
    };
    for a in [
        Action::BuildForward,
        Action::BuildBack,
        Action::BuildLeft,
        Action::BuildRight,
        Action::AlignForward,
        Action::AlignBack,
        Action::AlignLeft,
        Action::AlignRight,
        Action::MemberPrev,
        Action::MemberNext,
    ] {
        assert!(actions.contains(&a), "{a:?} is live while holding and not on the member list");
    }

    // The clause that checked the MEMBERS list was *lit* went with the accent halo; what makes the
    // hold's badges legible is the leader, and `every_control_cluster_is_tied_to_its_anchor` holds
    // that across every tab rather than once here.
}

/// **A badge stands on ground nothing else uses.**
///
/// The rule started as *"the gutter on the leading edge, and onto that edge when there is no room"*,
/// and a capture showed what the second half costs at the window's own edge: `Cmd+O` drawn over
/// `‹ ki`, `1, 2, 3` over `M`, `Cmd+C` over `TILE`. A badge that hides the label identifying the
/// control it names has undone its own job.
///
/// **This was "beside its anchor's horizontal span", and that measured the wrong thing.** It was a
/// proxy for "not on the words", chosen when the only ground a box could stand on was the stage —
/// so *beside* and *clear* were the same answer. They are not any more: a pane's empty middle is
/// inside its anchor's span and covers nothing, and it is the placement the whole free-ground search
/// exists to reach. Meanwhile the proxy never guarded a single row of a list, because a list row is
/// named by no `ControlId`.
///
/// So the invariant is stated directly, against the editor's own census
/// (`badges::ink_now`): **no cluster covers ink**, its own anchor's words included. That is strictly
/// stronger than what it replaces — everything the old rule caught, this catches, plus everything on
/// screen the census never named.
#[test]
fn a_badge_stands_on_ground_nothing_else_uses() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use emerge_mapper::badges::BadgeCluster;
    use emerge_mapper::keys::Home;

    let root = Fixture::new("badgeside")
        .descriptor("floor", "alpha")
        .place("floor", (0.0, 0.0))
        .build("m");

    let mut covered = Vec::new();
    let mut checked = 0usize;
    for mode in TABS {
        let mut app = badges_up(&root, mode);
        // **No window clause.** The old rule needed one — an anchor as wide as the window has no
        // side to go to, so "beside it" had to be waived there. Covering nothing has no such
        // exception: there is always somewhere on a screen that is not ink, and if there genuinely
        // is not, that is the finding.
        let clusters: Vec<(Home, Rect)> = {
            let mut q = app
                .world_mut()
                .query::<(&BadgeCluster, &ComputedNode, &UiGlobalTransform, &Visibility)>();
            q.iter(app.world())
                .filter(|(_, n, _, v)| **v != Visibility::Hidden && n.size() != Vec2::ZERO)
                .map(|(c, n, tf, _)| (c.0, Rect::from_center_size(tf.translation, n.size())))
                .collect()
        };
        let ink = emerge_mapper::badges::ink_now(app.world_mut());

        for (home, rect) in clusters {
            checked += 1;
            for used in &ink {
                let hit = rect.intersect(*used);
                // A pixel of touching is two things side by side; anything with area is one drawn
                // through the other.
                if hit.width() > 1.0 && hit.height() > 1.0 {
                    covered.push(format!(
                        "{} {home:?}: badge {rect:?} covers {:.0}x{:.0} px of ink at {:?}",
                        mode.label(),
                        hit.width(),
                        hit.height(),
                        used.min
                    ));
                }
            }
        }
    }
    covered.sort();
    covered.dedup();
    assert!(
        covered.is_empty(),
        "these badges are drawn over something on screen, so a verb or the thing it names is \
         unreadable:\n  {}",
        covered.join("\n  ")
    );
    // The companion assertion: every clause above is a filter, so a layer that stopped being laid
    // out would make this pass having measured nothing.
    assert!(
        checked >= TABS.len(),
        "only {checked} cluster(s) were measured across {} tabs; the rule is being enforced \
         against nothing",
        TABS.len()
    );
}

/// **The triangle count does not move the map's name.**
///
/// Both live in the chrome bar, and the count is the last child of a row whose spacer pushes
/// everything to the right end — so every digit it gained shoved `MAP · untitled_map` left. Reported
/// from the keyboard: the label "bounces around a whole lot as those triangle numbers change", and it
/// got worse once `N`'s badge started riding the label.
///
/// A count that moves its neighbour makes the neighbour unreadable, and the neighbour is the one
/// thing on that bar that says which map you are in. So the count holds a reserved column and the
/// digits grow leftwards into it; this is what says so.
///
/// # It used to inject three strings and measure the same one three times
///
/// It wrote `"9,999,999 tris drawn"` straight into the `Text` and stepped frames — but
/// `refresh_triangle_total` is registered bare in `Update` and rewrites the readout from the live
/// mesh count *before* `PostUpdate` lays anything out, so all three iterations measured the fixture's
/// own total and `moved` was empty by construction. Deleting `min_width` left it green.
///
/// The property is not "three strings put the name in the same place" — the strings cannot be made to
/// stick. It is that **the column is wide enough for the widest string the format can produce, and
/// the node is actually given it**. Both halves are checked, and each fails on its own: shrink
/// `COST_COL` and the first goes; delete `min_width` and the second goes.
#[test]
fn the_triangle_count_does_not_shove_the_maps_name() {
    use bevy::ui::ComputedNode;

    let root = Fixture::new("costcol")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app = harness::build_headless(&root, "m", None).unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..6 {
        app.update();
    }

    // The widest phrase the readout can ever hold, asked of the formatter the readout itself uses.
    // `usize::MAX` rather than a threshold: `refresh_triangle_total` sums every visible mesh and
    // `HEAVY_SCENE` only tints, so nothing in the editor bounds the number.
    let longest = format!(
        "{} tris drawn",
        emerge_mapper::editor::with_thousands(usize::MAX)
    );
    let need = longest.chars().count() as f32 * emerge_mapper::chrome::BODY_CHAR_W
        + 2.0 * emerge_mapper::editor::COST_PAD_X;
    assert!(
        emerge_mapper::editor::COST_COL >= need,
        "the reserved column is {} px and `{longest}` needs {need} px; the node is `min_width`, so \
         it will grow past the reservation and drag the map's name with it",
        emerge_mapper::editor::COST_COL
    );

    // …and the node is actually given the column. Measured off the live layout rather than assumed
    // from the source, which is the half the old version could not see.
    let mut q = app
        .world_mut()
        .query_filtered::<(&ComputedNode, &ChildOf), With<emerge_mapper::editor::TriangleTotal>>();
    let widths: Vec<f32> = {
        let world = app.world();
        q.iter(world)
            .filter_map(|(_, parent)| world.get::<ComputedNode>(parent.parent()))
            .map(|n| n.size().x)
            .collect()
    };
    assert_eq!(
        widths.len(),
        1,
        "expected exactly one triangle readout in the bar, found {} — this test would prove nothing",
        widths.len()
    );
    assert!(
        widths[0] >= emerge_mapper::editor::COST_COL - 0.5,
        "the readout's box measured {} px against a reserved {} px: it is hugging its digits, so \
         every change of magnitude will move `WhereYouAre`",
        widths[0],
        emerge_mapper::editor::COST_COL
    );

    // The reservation is only worth anything if the neighbour it protects is on the bar at all.
    let mut names = app
        .world_mut()
        .query_filtered::<Entity, With<emerge_mapper::chrome::WhereYouAre>>();
    assert_eq!(
        names.iter(app.world()).count(),
        1,
        "no `WhereYouAre` in the bar — the column would be protecting nothing"
    );
}

/// **A badge in one of the frame's bands is level with the control it names.**
///
/// The chrome bar, the door strip and the status band are chrome with no vertical slack, and a badge
/// top-aligned to a control there hangs below it. Reported from the keyboard: they *"need to be in
/// horizontal alignment with the center of that action button… there's not enough room to offset them
/// below."*
///
/// Only the bands. A dock has room, and there a badge stays level with the **row** it names rather
/// than with the middle of the pane holding it — which is the whole reason `I` and `M` read as
/// belonging to the id and mount lines.
#[test]
fn a_badge_in_a_band_is_level_with_its_control() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use emerge_mapper::badges::BadgeCluster;
    use emerge_mapper::keys::Home;

    let root = Fixture::new("badgeband")
        .descriptor("floor", "alpha")
        .build("m");

    let mut off = Vec::new();
    let mut checked = 0usize;
    for mode in TABS {
        let mut app = badges_up(&root, mode);
        let bands = {
            let f = app.world().resource::<emerge_mapper::chrome::Frame>();
            [f.chrome_bar, f.door_strip, f.status]
        };
        let anchors: Vec<(emerge_mapper::keys::ControlId, Rect, bool)> = {
            let mut q = app
                .world_mut()
                .query::<(Entity, &emerge_mapper::chrome::Control, &ComputedNode, &UiGlobalTransform)>();
            let world = app.world();
            q.iter(world)
                .filter(|(_, _, n, _)| n.size() != Vec2::ZERO)
                .map(|(e, c, n, tf)| {
                    let mut at = Some(e);
                    let mut in_band = false;
                    while let Some(x) = at {
                        if bands.contains(&x) {
                            in_band = true;
                            break;
                        }
                        at = world.get::<ChildOf>(x).map(|p| p.parent());
                    }
                    (c.0, Rect::from_center_size(tf.translation, n.size()), in_band)
                })
                .collect()
        };
        let clusters: Vec<(Home, Rect)> = {
            let mut q = app
                .world_mut()
                .query::<(&BadgeCluster, &ComputedNode, &UiGlobalTransform, &Visibility)>();
            q.iter(app.world())
                .filter(|(_, n, _, v)| **v != Visibility::Hidden && n.size() != Vec2::ZERO)
                .map(|(c, n, tf, _)| (c.0, Rect::from_center_size(tf.translation, n.size())))
                .collect()
        };
        for (home, rect) in clusters {
            let Home::Control(id) = home else { continue };
            let Some((_, anchor, true)) = anchors.iter().find(|(a, _, _)| *a == id).copied() else {
                continue;
            };
            checked += 1;
            let drift = (rect.center().y - anchor.center().y).abs();
            if drift > 1.0 {
                off.push(format!(
                    "{} {id:?}: badge centred at {}, control at {}",
                    mode.label(),
                    rect.center().y,
                    anchor.center().y
                ));
            }
        }
    }
    assert!(
        off.is_empty(),
        "these badges sit off the middle of the control they name, in a band with no room to hang \
         below it:\n  {}",
        off.join("\n  ")
    );
    assert!(
        checked >= TABS.len(),
        "only {checked} banded badge(s) were measured across {} tabs; the rule is being enforced \
         against nothing",
        TABS.len()
    );
}

/// **A refusal toasts, and the journal keeps it after `Esc` has cleared the tab.**
///
/// The refusal used to be a block wedged into the status band — twenty-six pixels of chrome holding
/// the longest text this editor renders, so it stood proud of the thing it was supposed to be inside
/// (*"it appears over the status bar at the bottom and isn't quite in alignment… so it looks really
/// bad"*). It became a toast; the toast then duplicated the log sitting in the panel underneath it,
/// which was the same sentence twice. So the log went behind `Cmd+E` and grew up into a **session**
/// journal: *"that log shows every error message that's happened since the beginning of the
/// application."*
///
/// Three things are pinned, and the third is the one that makes the design work: the toast is under
/// the **viewport**, the journal is **not on screen** until asked for, and the journal still has the
/// line **after `Esc`** — because `Esc` clears a tab's working list and the journal is not that.
#[test]
fn a_refusal_toasts_and_the_journal_keeps_it_after_esc() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{IntoScheduleConfigs, KeyCode, ResMut, Update};

    let root = Fixture::new("toast")
        .descriptor("floor", "alpha")
        .place("floor", (0.0, 0.0))
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Map)
        .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let one = |app: &mut App, marker: &str| -> Entity {
        let found = match marker {
            "toast" => {
                let mut q = app
                    .world_mut()
                    .query::<(Entity, &emerge_mapper::chrome::ToastLayer)>();
                q.iter(app.world()).map(|(e, _)| e).next()
            }
            _ => {
                let mut q = app
                    .world_mut()
                    .query::<(Entity, &emerge_mapper::chrome::JournalPanel)>();
                q.iter(app.world()).map(|(e, _)| e).next()
            }
        };
        found.unwrap_or_else(|| panic!("no `{marker}` was spawned"))
    };
    let toast = one(&mut app, "toast");
    let journal = one(&mut app, "journal");
    let display = |app: &App, e: Entity| -> Display {
        app.world()
            .get::<Node>(e)
            .map(|n| n.display)
            .unwrap_or_else(|| panic!("no `Node`"))
    };
    assert_eq!(display(&app, toast), Display::None, "a quiet editor is quiet");
    assert_eq!(
        display(&app, journal),
        Display::None,
        "and the journal is not on screen until it is asked for"
    );

    // **Under the viewport, never under the band** — the region that belongs to nothing else.
    let frame = app
        .world()
        .get_resource::<emerge_mapper::chrome::Frame>()
        .map(|f| (f.viewport, f.status))
        .unwrap_or_else(|| panic!("the frame owns the regions"));
    let mut up = app.world().get::<ChildOf>(toast).map(|p| p.parent());
    let mut ancestors = Vec::new();
    while let Some(e) = up {
        ancestors.push(e);
        up = app.world().get::<ChildOf>(e).map(|p| p.parent());
    }
    assert!(ancestors.contains(&frame.0), "the toast hangs under the viewport");
    assert!(
        !ancestors.contains(&frame.1),
        "and never under the status band, which is the 26 px it did not fit in"
    );

    app.world_mut()
        .resource_mut::<emerge_mapper::editor::EditorState>()
        .status
        .problem("cannot remove: `alpha/floor` still places it".to_owned());
    app.update();
    assert_eq!(
        display(&app, toast),
        Display::Flex,
        "a refusal raises the toast on the frame it is raised"
    );

    let press = |app: &mut App, chord: Vec<KeyCode>| {
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
    };

    // **`Esc` clears the tab's working list** — which is the whole point of the journal being
    // somewhere else.
    press(&mut app, vec![emerge_mapper::keys::binding(emerge_mapper::keys::Action::Cancel).key]);
    assert!(
        !app.world()
            .resource::<emerge_mapper::editor::EditorState>()
            .status
            .has_problem(),
        "Esc takes the tab's problems down"
    );

    press(
        &mut app,
        vec![
            emerge_mapper::keys::MOD_KEYS[0],
            emerge_mapper::keys::binding(emerge_mapper::keys::Action::ShowErrors).key,
        ],
    );
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        display(&app, journal),
        Display::Flex,
        "Cmd+E opens the journal"
    );

    let listed: Vec<String> = {
        let texts: Vec<(Entity, String)> = {
            let mut q = app.world_mut().query::<(Entity, &Text)>();
            q.iter(app.world()).map(|(e, t)| (e, t.0.clone())).collect()
        };
        let world = app.world();
        texts
            .into_iter()
            .filter(|(e, _)| {
                let mut up = Some(*e);
                while let Some(x) = up {
                    if x == journal {
                        return true;
                    }
                    up = world.get::<ChildOf>(x).map(|p| p.parent());
                }
                false
            })
            .map(|(_, t)| t)
            .collect()
    };
    assert!(
        listed.iter().any(|t| t.contains("still places it")),
        "the journal keeps what `Esc` cleared — it holds {listed:?}"
    );
}

/// **The tile sizes itself around its contents wherever the author is standing.**
///
/// `build::refit_tile` used to return early unless the Tiles tab was open, so a tile's envelope
/// could only follow its members while that one panel was on screen. Reported from the keyboard:
/// *"the sizing of the tile around the mesh doesn't take place until you enter the mesh or the tile
/// editing… we want this to happen whenever a mesh gets loaded."*
///
/// The case that makes it visible is the one in the report: `build::fit_envelope` measures a member
/// through `library.get(id)`, so a piece the library does not carry **yet** spans nothing and the
/// tile fits to a single cell. The measurement landing is a `Project` change — and with the tab gate
/// in place, that change was only acted on if the author happened to be standing on Tiles.
///
/// So this drops a piece, leaves for another tab, and grows the piece there.
#[test]
fn the_tile_refits_while_another_tab_is_open() {
    use bevy::input::ButtonInput;
    use bevy::prelude::{IntoScheduleConfigs, KeyCode, ResMut, Update};
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("refit_offtab")
        .sized_descriptor("panel", "alpha", 0.2, 0.2)
        .build("test_map");
    let mut app =
        harness::build_headless_at(&root, "test_map", None, emerge_mapper::tiles::Mode::Tiles)
            .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    app.update();
    // The tab opens on the Tiles page — name the tile before the drop, which lands on Meshes.
    open_tile(&mut app, "tile");

    let once = |app: &mut App, chord: Vec<KeyCode>| {
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
    };
    // **Enter is the drop AND the hold** — the arm-then-drop pair is refused by the census.
    once(&mut app, vec![binding(Action::BuildDrop).key]);

    let size = |app: &App| -> (f32, f32, f32) {
        match &app.world().resource::<emerge_mapper::build::Build>().open {
            Some(c) => match c.envelope {
                emerge_core::composition::Envelope::Bounded { size } => size,
                _ => panic!("a tile claims a tile"),
            },
            None => panic!("the tile must still be open"),
        }
    };
    let one = size(&app);
    assert!(
        (one.0 - emerge_core::grid::TILE).abs() < 1e-4,
        "a 0.2 m piece centred in a tile is one cell across, not {one:?}"
    );

    // **Leave for another tab, then let the piece grow under it** — the shape of a measurement
    // arriving after a drop.
    *app.world_mut().resource_mut::<emerge_mapper::tiles::Mode>() =
        emerge_mapper::tiles::Mode::Meshes;
    app.update();
    {
        let mut project = app
            .world_mut()
            .resource_mut::<emerge_mapper::project::Project>();
        let project = &mut *project;
        let mut touched = 0;
        for list in [
            &mut project.measured.descriptors,
            &mut project.library.descriptors,
        ] {
            for d in list.iter_mut().filter(|d| d.id == "panel") {
                d.extent.footprint = Some((1.4, 1.4));
                touched += 1;
            }
        }
        assert!(touched > 0, "the fixture must carry `panel` to grow it");
    }
    for _ in 0..3 {
        app.update();
    }

    let grown = size(&app);
    assert!(
        grown.0 > one.0 + 1e-4 && grown.2 > one.2 + 1e-4,
        "the envelope stayed {grown:?} while its member grew to 1.4 m — a tile that only refits on \
         one tab is a tile that is the wrong size everywhere else"
    );
}

/// **A pending proposal reaches the description box and STAYS there.**
///
/// The pane holds one value: with a proposal outstanding, the box shows what the model offered, in
/// `SUGGEST`, and `U`/`Y` decide. `tiles::rebuild_detail` did exactly that and it was never on
/// screen, because `tiles::refresh_cells` repaints the same node in place — it exists so a click
/// does not respawn the pane — and it read `d.note` alone. Whatever the pane drew, the repainter
/// overwrote a frame later with `describe it…`.
///
/// Found by looking at the running editor, not here: the source read correctly at both sites and
/// disagreed only in the world. So this steps **past** the rebuild, which is the only way to see the
/// second writer, and `tiles::note_field_text` is now the one place either of them asks.
#[cfg(feature = "debugger")]
#[test]
fn a_pending_proposal_survives_the_in_place_repaint() {
    let root = Fixture::new("proposalbox")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app =
        harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }
    let (id, mesh) = app
        .world()
        .resource::<emerge_mapper::project::Project>()
        .library
        .descriptors
        .first()
        .map(|d| (d.id.clone(), d.mesh.clone().unwrap_or_default()))
        .unwrap_or_else(|| panic!("the fixture must carry a descriptor with a mesh"));

    let target = emerge_mapper::tiles::EditTarget::Library(id.clone());
    app.world_mut()
        .resource_mut::<emerge_mapper::labels::Suggestions>()
        .insert(&target, emerge_mapper::labels::Entry::for_test(&mesh));
    app.world_mut()
        .resource_mut::<emerge_mapper::tiles::ImportState>()
        .selected_library_id = Some(id);

    // **Well past the rebuild.** One frame would only prove `rebuild_detail` is right, which it was
    // the whole time the box was empty on screen.
    for _ in 0..8 {
        app.update();
    }

    let shown: Vec<String> = {
        let mut q = app
            .world_mut()
            .query::<(&Text, &emerge_mapper::tiles::NoteReadout)>();
        q.iter(app.world()).map(|(t, _)| t.0.clone()).collect()
    };
    assert!(
        shown.iter().any(|t| t == "a thing"),
        "the description box does not show the proposal after the pane settles — it holds {shown:?}. \
         `Entry::for_test` proposes `what: \"a thing\"` and no note, and `Suggestion::description` \
         is what turns that into the words the box takes."
    );
}

/// **The tag axes have a block to stand in.**
///
/// Four axes and their whole vocabulary is a wall — `KIND` alone is eighteen chips over three rows on
/// a real kit — so the block is bounded and scrolls within itself. The first attempt at that used
/// `chrome::scroll_list`, which bounds itself with `flex_grow` against a full-height panel: nested
/// inside the detail pane's own scroll there is nothing for it to grow against, its explicit
/// `min_height: 0` let it shrink, and **the whole block vanished** — no heading, no chips, nothing
/// saying a vocabulary existed. It was found in a screenshot, which is the wrong place to find it.
///
/// `chrome::scroll_box` states the height on the wrapper instead. This is what says so: the control
/// has a size, and the fixture's four tokens are inside it.
#[test]
fn the_tag_axes_have_a_block_to_stand_in() {
    use bevy::ui::ComputedNode;

    let root = Fixture::new("tagblock")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app =
        harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }
    let id = app
        .world()
        .resource::<emerge_mapper::project::Project>()
        .library
        .descriptors
        .first()
        .map(|d| d.id.clone())
        .unwrap_or_else(|| panic!("the fixture must carry a descriptor"));
    app.world_mut()
        .resource_mut::<emerge_mapper::tiles::ImportState>()
        .selected_library_id = Some(id);
    // The pane is a change-gated rebuild and the layout lands in the frame after it.
    for _ in 0..4 {
        app.update();
    }

    let tags = {
        let mut q = app
            .world_mut()
            .query::<(Entity, &emerge_mapper::chrome::Control, &ComputedNode)>();
        q.iter(app.world())
            .find(|(_, c, _)| c.0 == emerge_mapper::keys::ControlId::Tags)
            .map(|(e, _, n)| (e, n.size()))
    };
    let Some((tags, size)) = tags else {
        panic!("the detail pane draws no `ControlId::Tags` node at all");
    };
    assert!(
        size.y > 1.0 && size.x > 1.0,
        "the tag block laid out at {size:?} — a nested scroll with nothing to bound it collapses to \
         zero, which is invisible rather than broken. See `chrome::scroll_box`."
    );

    // The fixture ships exactly one token per axis, so all four are what a working block shows.
    let inside: Vec<String> = {
        let mut q = app.world_mut().query::<(Entity, &Text, &ComputedNode)>();
        let found: Vec<(Entity, String)> = q
            .iter(app.world())
            .filter(|(_, _, n)| n.size() != Vec2::ZERO)
            .map(|(e, t, _)| (e, t.0.clone()))
            .collect();
        let world = app.world();
        found
            .into_iter()
            .filter(|(e, _)| {
                let mut up = Some(*e);
                while let Some(x) = up {
                    if x == tags {
                        return true;
                    }
                    up = world.get::<ChildOf>(x).map(|p| p.parent());
                }
                false
            })
            .map(|(_, t)| t)
            .collect()
    };
    for token in ["prop", "inert", "plain", "worktop"] {
        assert!(
            inside.iter().any(|t| t == token),
            "the tag block is laid out but `{token}` is not in it — it holds {inside:?}"
        );
    }
}

/// **A verb whose row is out of view is pinned to its own pane, not floating past it.**
///
/// A row in a `chrome::scroll_list` keeps its rect when it scrolls out of view — only its clip
/// changes. Taken at face value that would aim a leader hundreds of pixels below the pane, level
/// with the status band, naming a row nobody can see. Boxes used to be pinned there bodily, and a
/// scrolled pane stacked them on one edge until the deepest were buried; now the **box** packs in
/// the rail like any other, and it is the **leader's anchor end** that clamps to the pane — the
/// line points at the edge where scrolling would bring the row back.
///
/// The invariant: a paned control's leader begins inside its pane's fold, give or take a hairline.
///
/// # It never scrolled anything, so it could not reach the case it names
///
/// Every pane sat at the top of its content, so every paned control was wholly inside its fold and
/// the clamp had nothing to do — deleting it changed no number here. The one shape that *would*
/// have shown it, a control scrolled entirely past its fold, is skipped by `place_badges` itself
/// (`a.at.intersect(f)` empty → `continue`), leaves the cluster `Visibility::Hidden`, and is
/// filtered out below. So the regression this test is named for was unreachable from it.
///
/// The panes are scrolled now, and by an offset **read off the layout** rather than picked: a pane
/// moved by `row centre − fold top` puts exactly that row half in and half out. `straddled` is the
/// count that says such a row was really measured, and `scrollable` says whether there was any
/// scroll room to produce one — two separate failures, so a quiet test names its own reason.
#[test]
fn a_row_beyond_the_fold_is_pointed_at_from_its_pane() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use emerge_mapper::badges::{BadgeCluster, Lead, LeadSeg};
    use emerge_mapper::keys::Home;

    /// Every laid-out `chrome::Control` that sits **inside** a scrolling pane, as
    /// `(id, the fold, the control's own rect, the pane's logical-per-physical factor, its scroll
    /// room in physical pixels)`. The same ancestry walk `badges::fold_of` makes, and strict about
    /// *inside* for the same reason: a node that **is** the pane is a whole control with open ground
    /// beside it, not a row within one.
    fn paned(app: &mut App) -> Vec<(emerge_mapper::keys::ControlId, Rect, Rect, f32, f32)> {
        let mut q = app
            .world_mut()
            .query::<(Entity, &emerge_mapper::chrome::Control, &ComputedNode)>();
        let ids: Vec<(Entity, emerge_mapper::keys::ControlId)> = q
            .iter(app.world())
            .filter(|(_, _, n)| n.size() != Vec2::ZERO)
            .map(|(e, c, _)| (e, c.0))
            .collect();
        let world = app.world();
        ids.into_iter()
            .filter_map(|(e, id)| {
                let at = world
                    .get::<ComputedNode>(e)
                    .zip(world.get::<UiGlobalTransform>(e))
                    .map(|(n, tf)| Rect::from_center_size(tf.translation, n.size()))?;
                let mut up = world.get::<ChildOf>(e).map(|p| p.parent());
                while let Some(x) = up {
                    if world.get::<bevy::ui_widgets::ScrollArea>(x).is_some() {
                        let (n, tf) = world
                            .get::<ComputedNode>(x)
                            .zip(world.get::<UiGlobalTransform>(x))?;
                        if n.size() == Vec2::ZERO {
                            return None;
                        }
                        let room = (n.content_size.y - n.size.y).max(0.0);
                        return Some((
                            id,
                            Rect::from_center_size(tf.translation, n.size()),
                            at,
                            n.inverse_scale_factor,
                            room,
                        ));
                    }
                    up = world.get::<ChildOf>(x).map(|p| p.parent());
                }
                None
            })
            .collect()
    }

    let root = Fixture::new("badgefold")
        .descriptor("floor", "alpha")
        .place("floor", (0.0, 0.0))
        .build("m");

    let mut loose = Vec::new();
    let mut checked = 0usize;
    let mut straddled = 0usize;
    let mut scrollable = 0usize;
    for mode in TABS {
        let mut app = badges_up(&root, mode);
        stage_a_piece(&mut app);

        // **The offsets come out of the layout, not out of a guess.**
        //
        // Scrolling a pane by `row centre − fold top` puts exactly that row half inside the fold and
        // half above it, which is the one shape the clamp exists for: wholly inside needs no clamp,
        // and wholly outside is skipped by `place_badges` (`a.at.intersect(f)` empty → `continue`),
        // left `Visibility::Hidden`, and filtered out below. One candidate per paned control, capped
        // at the pane's real scroll room, so any pane that can move at all produces a straddle
        // rather than the test hoping a fixed number lands mid-row.
        //
        // `0.0` stays first: it is the unscrolled coverage the `checked` floor is written against.
        let mut offsets: Vec<f32> = vec![0.0];
        for (_, fold, at, inv, room) in paned(&mut app) {
            if room <= 1.0 {
                continue;
            }
            scrollable += 1;
            let want = ((at.center().y - fold.min.y) * inv).clamp(0.0, room * inv);
            if want > 0.5 && !offsets.iter().any(|o| (o - want).abs() < 0.5) {
                offsets.push(want);
            }
        }

        for scroll in offsets {
            scroll_every_pane(&mut app, scroll);
            for _ in 0..8 {
                app.update();
            }
            let folds = paned(&mut app);

            // Every paned cluster's reach segment starts inside its pane.
            let leads: Vec<(emerge_mapper::keys::ControlId, Entity)> = {
                let mut q = app
                    .world_mut()
                    .query::<(&BadgeCluster, &Lead, &Visibility)>();
                q.iter(app.world())
                    .filter(|(.., vis)| **vis != Visibility::Hidden)
                    .filter_map(|(c, lead, _)| match c.0 {
                        Home::Control(id) => Some((id, lead.0[0])),
                        Home::Legend => None,
                    })
                    .collect()
            };
            let mut seg_q = app
                .world_mut()
                .query_filtered::<(&ComputedNode, &UiGlobalTransform, &Visibility), With<LeadSeg>>();
            let world = app.world();
            for (id, seg) in leads {
                let Some((_, fold, at, ..)) = folds.iter().find(|(x, ..)| *x == id) else {
                    continue;
                };
                let Ok((node, tf, vis)) = seg_q.get(world, seg) else {
                    continue;
                };
                if *vis == Visibility::Hidden || node.size() == Vec2::ZERO {
                    loose.push(format!("{}: {id:?} shows no reach segment at all", mode.label()));
                    continue;
                }
                checked += 1;
                // Part-way past the fold: intersecting it, and reaching beyond it. `straddled` is
                // what says the clamp was asked anything at all.
                let seen = at.intersect(*fold);
                if seen.height() > 0.0
                    && (at.min.y < fold.min.y - 0.5 || at.max.y > fold.max.y + 0.5)
                {
                    straddled += 1;
                }
                let y = tf.translation.y;
                if y < fold.min.y - 2.0 || y > fold.max.y + 2.0 {
                    loose.push(format!(
                        "{}: {id:?} is pointed at from y {:.0}, outside its pane {:.0}..{:.0}",
                        mode.label(),
                        y,
                        fold.min.y,
                        fold.max.y
                    ));
                }
            }
        }
    }
    loose.sort();
    loose.dedup();
    assert!(
        loose.is_empty(),
        "these leaders start outside the pane holding the row they name:\n  {}",
        loose.join("\n  ")
    );
    assert!(
        checked >= TABS.len(),
        "only {checked} paned leader(s) were measured across {} tabs; the rule is being enforced \
         against nothing",
        TABS.len()
    );
    assert!(
        scrollable > 0,
        "no `chrome::Control` in the editor sits inside a pane with any scroll room, so a row \
         part-way past its fold is unreachable from here and the rule has no subject. Either the \
         detail pane now fits its content on this fixture, or nothing is laid out inside a \
         `ScrollArea` any more."
    );
    assert!(
        straddled > 0,
        "{scrollable} paned control(s) had scroll room and {checked} leader(s) were measured, and \
         not one named a control reaching past its own fold — so the clamp was never asked anything \
         and deleting it would not move this test. The offsets are derived from the layout, so this \
         means the rows are moving less than half their own height."
    );
}

/// **Nothing covers anything a reader needs: no box over a box, no box over ink, no box over
/// another's line.** This is the arithmetic the overlay kept losing, now stated as the packer's
/// contract.
///
/// **"Ink" replaced "a `chrome::Control` rect", and that was a strengthening.** The nineteen named
/// controls are the anchors the census can *point at*; they are not the set of things a reader
/// reads. Half of what is on screen — every row of a list, every heading, every field, the map's
/// own triangle count — is named by no `ControlId` at all and was unguarded, while a whole detail
/// pane counted as covered when a badge stood on its empty middle, which is the one placement this
/// overlay most wants. `badges::ink_now` is the editor's own answer to both.
///
/// It used to be a trade-off — `step_clear` moved the loser until its bound ran out and then *let
/// the overlap show*, and the first capture on a real window priced that: the legend under the
/// piece list's boxes, cell rows buried beneath their own neighbours. The rail packer and the
/// legend's free-ground search have no give-up arm, so red here is not a census-width problem any
/// more: it means `badges::place_badges` broke its own contract, or a screen genuinely has more
/// badge than stage — either way a bug, not a wording chore.
///
/// Measured at two shapes: the harness default, and the author's actual window in physical texels
/// — the geometry on which a square-surfaced suite stayed green while the real screen buried the
/// legend.
#[test]
fn no_badge_cluster_draws_through_another() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use emerge_mapper::badges::{BadgeCluster, Lead, LeadSeg};

    // **Not `crowded_root`, and that is a stated gap rather than an oversight.** Pointed at a
    // populated kit this rule reports thirteen boxes standing on ink or on each other — and it
    // reports fourteen with the placement this commit replaced, so it is describing a defect that
    // is older than the crossing fix and separate from it. Widening the fixture here would turn one
    // red suite into the record of two different bugs. `BACKLOG.md` carries it with its output.
    let root = Fixture::new("badgeoverlap")
        .descriptor("floor", "alpha")
        .place("floor", (0.0, 0.0))
        .build("m");

    let mut through = Vec::new();
    let mut checked = 0usize;
    for surface in [None, Some((2560u32, 1406u32))] {
        for mode in TABS {
            let mut app = badges_up(&root, mode);
            // The populated pane, not the empty one: the first real-kit capture found the fallback
            // firing exactly where an unselected fixture had nothing to measure.
            stage_a_piece(&mut app);
            if let Some((w, h)) = surface {
                harness::resize_surface(&mut app, w, h).unwrap_or_else(|e| panic!("{e}"));
                for _ in 0..8 {
                    app.update();
                }
            }
            let tag = match surface {
                None => mode.label().to_owned(),
                Some((w, h)) => format!("{} at {w}x{h}", mode.label()),
            };

            let drawn: Vec<(String, Rect, Option<[Entity; 3]>)> = {
                let mut q = app.world_mut().query::<(
                    &BadgeCluster,
                    &ComputedNode,
                    &UiGlobalTransform,
                    &Visibility,
                    Option<&Lead>,
                )>();
                q.iter(app.world())
                    .filter(|(_, n, _, vis, _)| n.size() != Vec2::ZERO && **vis != Visibility::Hidden)
                    .map(|(c, n, tf, _, lead)| {
                        (
                            format!("{:?}", c.0),
                            Rect::from_center_size(tf.translation, n.size()),
                            lead.map(|l| l.0),
                        )
                    })
                    .collect()
            };
            // **The interface as laid out, through the editor's own census.** This used to be the
            // nineteen `chrome::Control` rects, which was the wrong set in both directions: it
            // missed every row, heading and field that is not a named control, and it counted a
            // whole pane whose fill carries nothing to read. `badges::ink_now` is
            // `badges::place_badges`' own answer, `pub` for exactly the reason `badges::resolve` is
            // — a test that restated the rule would pass while the editor used a different one.
            let ink_drawn: Vec<Rect> = emerge_mapper::badges::ink_now(app.world_mut());
            let segs_drawn: Vec<(usize, Rect)> = {
                let mut owner_of: std::collections::HashMap<Entity, usize> = Default::default();
                for (i, (.., lead)) in drawn.iter().enumerate() {
                    if let Some(l) = lead {
                        for e in l {
                            owner_of.insert(*e, i);
                        }
                    }
                }
                let mut q = app.world_mut().query_filtered::<(
                    Entity,
                    &ComputedNode,
                    &UiGlobalTransform,
                    &Visibility,
                ), With<LeadSeg>>();
                q.iter(app.world())
                    .filter(|(_, n, _, vis)| n.size() != Vec2::ZERO && **vis != Visibility::Hidden)
                    .filter_map(|(e, n, tf, _)| {
                        owner_of
                            .get(&e)
                            .map(|i| (*i, Rect::from_center_size(tf.translation, n.size())))
                    })
                    .collect()
            };

            checked += drawn.len();
            let covers = |a: Rect, b: Rect| {
                let hit = a.intersect(b);
                // A pixel of touching is two boxes side by side; anything with area is one drawn
                // through the other.
                hit.width() > 1.0 && hit.height() > 1.0
            };
            for (i, (a_name, a, _)) in drawn.iter().enumerate() {
                for (b_name, b, _) in drawn.iter().skip(i + 1) {
                    if covers(*a, *b) {
                        let hit = a.intersect(*b);
                        through.push(format!(
                            "{tag}: {a_name} and {b_name} overlap by {:.0}x{:.0} px",
                            hit.width(),
                            hit.height()
                        ));
                    }
                }
                for c in &ink_drawn {
                    if covers(*a, *c) {
                        let hit = a.intersect(*c);
                        through.push(format!(
                            "{tag}: {a_name} covers {:.0}x{:.0} px of ink at {:?}",
                            hit.width(),
                            hit.height(),
                            c.min
                        ));
                    }
                }
                for (owner, s) in &segs_drawn {
                    if *owner != i && covers(*a, *s) {
                        through.push(format!(
                            "{tag}: {a_name} covers another badge's leader"
                        ));
                    }
                }
            }
        }
    }
    through.sort();
    through.dedup();
    assert!(
        through.is_empty(),
        "something is covered, so a verb or its ground is on screen and unreadable. The packer has \
         no give-up arm, so this is `badges::place_badges` breaking its own contract — not a \
         census wording chore:\n  {}",
        through.join("\n  ")
    );
    assert!(
        checked >= TABS.len() * 2,
        "only {checked} cluster(s) were measured across two shapes of {} tabs; the rule is being \
         enforced against nothing",
        TABS.len()
    );
}

/// **The ground a badge stands on is ground the interface left, and some badge actually takes it.**
///
/// The companion to `a_badge_stands_on_ground_nothing_else_uses`, which is a *negative*: a packer
/// that hid every box would satisfy it perfectly. This is the positive half — on a real tab, with a
/// piece staged, at least one cluster stands **inside a dock**, which is ground no box could reach
/// before this existed. It is the whole change in one assertion.
///
/// The number that made it worth doing: on a 2560×1406 window the Map tab's left dock carries a
/// panel about 460 px shorter than the column it sits in, and every one of those pixels used to be
/// unreachable — a box could stand only in `frame.viewport`, so it hugged the dock's outer edge and
/// landed on the map instead.
#[test]
fn a_badge_takes_the_ground_a_dock_leaves() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use emerge_mapper::badges::BadgeCluster;

    let root = Fixture::new("badgedock")
        .descriptor("floor", "alpha")
        .place("floor", (0.0, 0.0))
        .build("m");

    let mut inside = Vec::new();
    for mode in TABS {
        let mut app = badges_up(&root, mode);
        stage_a_piece(&mut app);
        let docks: Vec<Rect> = {
            let frame = app
                .world()
                .get_resource::<emerge_mapper::chrome::Frame>()
                .map(|f| [f.left, f.right]);
            let Some(docks) = frame else { continue };
            let mut q = app.world_mut().query::<(&ComputedNode, &UiGlobalTransform)>();
            docks
                .into_iter()
                .filter_map(|e| q.get(app.world(), e).ok())
                .filter(|(n, _)| n.size() != Vec2::ZERO)
                .map(|(n, tf)| Rect::from_center_size(tf.translation, n.size()))
                .collect()
        };
        let mut q = app
            .world_mut()
            .query::<(&BadgeCluster, &ComputedNode, &UiGlobalTransform, &Visibility)>();
        for (cluster, node, tf, vis) in q.iter(app.world()) {
            if *vis == Visibility::Hidden || node.size() == Vec2::ZERO {
                continue;
            }
            let rect = Rect::from_center_size(tf.translation, node.size());
            if docks
                .iter()
                .any(|d| rect.min.x >= d.min.x - 0.5 && rect.max.x <= d.max.x + 0.5)
            {
                inside.push(format!("{} {:?}", mode.label(), cluster.0));
            }
        }
    }
    assert!(
        !inside.is_empty(),
        "no badge on any of the {} tabs stands inside a dock, so the free-ground search is finding \
         nothing and every box is back on the rail beside the map",
        TABS.len()
    );
}

/// **While the shortcuts key is held, the grounds step back — and every word on them does not.**
///
/// The other half of letting a badge stand on a panel's empty middle: a `PANEL_BG` box on a
/// `PANEL_BG` panel is one shape with a line through it. `chrome::GROUND_HELD` is what separates
/// them by depth, and this pins both ends of it — the drop while held, and the exact restore on
/// release, which is what `chrome::Ground` carries its colour for.
#[test]
fn the_grounds_step_back_while_the_shortcut_key_is_held() {
    let root = Fixture::new("groundheld")
        .descriptor("floor", "alpha")
        .place("floor", (0.0, 0.0))
        .build("m");

    let mut app = badges_up(&root, emerge_mapper::tiles::Mode::Map);
    let held: Vec<(f32, f32)> = {
        let mut q = app
            .world_mut()
            .query::<(&emerge_mapper::chrome::Ground, &BackgroundColor)>();
        q.iter(app.world())
            .map(|(g, bg)| (g.0.alpha(), bg.0.alpha()))
            .collect()
    };
    assert!(
        !held.is_empty(),
        "no node carries `chrome::Ground`, so nothing steps back and — worse — `badges::ink` sees \
         every panel's fill as something to read"
    );
    for (rest, now) in &held {
        assert!(
            (now - rest * emerge_mapper::chrome::GROUND_HELD).abs() < 0.001,
            "a ground rests at {rest} and reads {now} with the key down; it should be \
             {}",
            rest * emerge_mapper::chrome::GROUND_HELD
        );
    }

    app.world_mut()
        .resource_mut::<bevy::input::ButtonInput<bevy::prelude::KeyCode>>()
        .release_all();
    for _ in 0..4 {
        app.update();
    }
    let mut q = app
        .world_mut()
        .query::<(&emerge_mapper::chrome::Ground, &BackgroundColor)>();
    for (ground, bg) in q.iter(app.world()) {
        assert_eq!(
            bg.0, ground.0,
            "a ground did not come back to the colour it was spawned with"
        );
    }
}

/// **Every panel and both bands are ground, or their fill reads as something to look at.**
///
/// `badges::ink` subtracts a `chrome::Ground` node from the census and keeps every child of it, so a
/// panel that forgets the marker is a panel whose whole rect goes dead — several hundred square
/// pixels of placeable ground silently lost, with every test green. The two 26-px bands are in the
/// list for a different reason: the banded pass puts a bare chord *inside* a band beside its
/// control, and without the marker every one of those would read as a badge covering ink.
///
/// Checked against the real tree rather than against `chrome.rs`, in the spirit of
/// `a_control_in_a_band_really_is_in_one`: what matters is what got spawned.
#[test]
fn every_panel_and_band_is_ground() {
    use bevy::ui::ComputedNode;

    let root = Fixture::new("grounded")
        .descriptor("floor", "alpha")
        .place("floor", (0.0, 0.0))
        .build("m");

    let mut bare = Vec::new();
    let mut checked = 0usize;
    for mode in TABS {
        let mut app = badges_up(&root, mode);
        stage_a_piece(&mut app);
        let frame = app
            .world()
            .get_resource::<emerge_mapper::chrome::Frame>()
            .map(|f| (f.left, f.right, f.chrome_bar, f.status));
        let Some((left, right, bar, status)) = frame else {
            continue;
        };
        // Every direct child of a dock is a `chrome::panel_root`, and both bands are their own.
        let panels: Vec<Entity> = {
            let mut q = app.world_mut().query::<(Entity, &ChildOf)>();
            q.iter(app.world())
                .filter(|(_, p)| p.parent() == left || p.parent() == right)
                .map(|(e, _)| e)
                .chain([bar, status])
                .collect()
        };
        let mut q = app
            .world_mut()
            .query::<(&ComputedNode, Option<&emerge_mapper::chrome::Ground>)>();
        for entity in panels {
            let Ok((node, ground)) = q.get(app.world(), entity) else {
                continue;
            };
            // A `Display::None` panel belongs to a tab nobody is on; it has no fill on screen to
            // classify. That is the same "laid out" test the whole overlay uses.
            if node.size() == Vec2::ZERO {
                continue;
            }
            checked += 1;
            if ground.is_none() {
                bare.push(format!("{} {entity:?}", mode.label()));
            }
        }
    }
    assert!(
        bare.is_empty(),
        "these panels carry no `chrome::Ground`, so their fill counts as ink and the ground inside \
         them is unreachable:\n  {}",
        bare.join("\n  ")
    );
    assert!(
        checked >= TABS.len(),
        "only {checked} panel(s) were measured across {} tabs; the rule is being enforced against \
         nothing",
        TABS.len()
    );
}

/// **No leader touches another leader.**
///
/// One corridor per rail was crossing-free by construction — boxes packed in their anchors' order is
/// exactly the condition under which right-angle leaders cannot cross (Bekos, Kaufmann, Symvonis &
/// Wolff 2007) — and it was also unreadable: six runs down one strip is one line with tick marks,
/// not six lines. Reported from the keyboard, twice: *"prevent lines from crossing over each
/// other"*, and *"if two lines have a ninety degree angle close to each other, we should stagger
/// those apart."*
///
/// Lanes answer the second and reopen the first, because a reach now has to pass every lane inside
/// its own. `badges::lanes_for` closes it again by walking outward from the analytic preference; this
/// is what says it worked, on a real layout rather than on the four-leader case a unit test can hold.
///
/// **Touching, not merely crossing**: two runs lying along each other read as one line, which is the
/// same failure seen end-on.
#[test]
fn no_two_leaders_cross() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use emerge_mapper::badges::{BadgeCluster, Lead, LeadSeg};

    let root = crowded_root("leadercross");

    let mut met = Vec::new();
    let mut checked = 0usize;
    for surface in CROWDED_SHAPES {
        for mode in TABS {
            let mut app = badges_up(&root, mode);
            stage_a_piece(&mut app);
            if let Some((w, h)) = surface {
                harness::resize_surface(&mut app, w, h).unwrap_or_else(|e| panic!("{e}"));
                for _ in 0..8 {
                    app.update();
                }
            }
            let tag = match surface {
                None => mode.label().to_owned(),
                Some((w, h)) => format!("{} at {w}x{h}", mode.label()),
            };

            // Each cluster's own segments, by the `Lead` that owns them.
            let owned: Vec<(String, [Entity; 3])> = {
                let mut q = app
                    .world_mut()
                    .query::<(&BadgeCluster, &Lead, &Visibility)>();
                q.iter(app.world())
                    .filter(|(_, _, v)| **v != Visibility::Hidden)
                    .map(|(c, l, _)| (format!("{:?}", c.0), l.0))
                    .collect()
            };
            let rect_of = {
                let mut q = app.world_mut().query_filtered::<(
                    Entity,
                    &ComputedNode,
                    &UiGlobalTransform,
                    &Visibility,
                ), With<LeadSeg>>();
                let map: std::collections::HashMap<Entity, Rect> = q
                    .iter(app.world())
                    .filter(|(_, n, _, v)| n.size() != Vec2::ZERO && **v != Visibility::Hidden)
                    .map(|(e, n, tf, _)| (e, Rect::from_center_size(tf.translation, n.size())))
                    .collect();
                map
            };
            let leaders: Vec<(String, Vec<Rect>)> = owned
                .into_iter()
                .map(|(name, segs)| {
                    (name, segs.iter().filter_map(|e| rect_of.get(e).copied()).collect())
                })
                .filter(|(_, segs): &(String, Vec<Rect>)| !segs.is_empty())
                .collect();

            checked += leaders.len();
            for (i, (a_name, a_segs)) in leaders.iter().enumerate() {
                for (b_name, b_segs) in leaders.iter().skip(i + 1) {
                    for p in a_segs {
                        for q in b_segs {
                            let hit = p.intersect(*q);
                            if hit.width() > 0.0 && hit.height() > 0.0 {
                                met.push(format!(
                                    "{tag}: {a_name}'s leader meets {b_name}'s at {:?}",
                                    hit.min
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    met.sort();
    met.dedup();
    assert!(
        met.is_empty(),
        "these leaders cross or lie along one another, so a reader cannot follow either to its \
         control:\n  {}",
        met.join("\n  ")
    );
    assert!(
        checked >= TABS.len() * CROWDED_SHAPES.len(),
        "only {checked} leader(s) were measured across {} shapes of {} tabs; the rule is being \
         enforced against nothing",
        CROWDED_SHAPES.len(),
        TABS.len()
    );
}

/// **Zooming the view does not move a single badge or a single leader.**
///
/// Every key stays live while `K` is held — that is deliberate, and it is what makes the overlay a
/// rehearsal rather than a mode (this crate's `badges.rs` header, on Kurtenbach via ExposeHK). The
/// cost was that the wheel, `W A S D` and `Q E` all still drove the camera, and `place_badges`
/// re-projected the world's envelope through that camera **every frame** to decide what ground a
/// badge must dodge. So reading the overlay and nudging the view at the same time reflowed the
/// whole thing under the author's eyes. Reported from the keyboard: *"I don't think the lines and
/// the keyboard legends should change based on Zoom, should they?"*
///
/// They should not, and the reason is the premise of the design: ExposeHK works by **spatial
/// rehearsal** — you learn where a badge is and reach for it next time. A layout that moves when the
/// camera does is one nobody can learn, and it moves during exactly the seconds the key is held.
///
/// The first fix latched the projection for the length of a hold. The second went further and
/// deleted the reason it existed: badges do not dodge the world at all any more — it is **faded**
/// while the key is held (`chrome::WORLD_HELD`) instead of routed around, because the detour cost
/// more ground than it saved. So `place_badges` now reads no camera and no world geometry, and the
/// guarantee is structural rather than maintained.
///
/// This still earns its keep: it is what would catch a future edit that reintroduces either.
/// Asserted to the pixel, because "barely moves" is the bug in its quiet form.
#[test]
fn zooming_the_view_moves_no_badge_and_no_leader() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use emerge_mapper::badges::{BadgeCluster, LeadSeg};

    let root = crowded_root("badgezoom");

    // The Map draws a bounds cube and the Tiles tab a tile envelope; Meshes stands a mesh on a
    // stage. All three are tabs where a camera move changes the projection.
    for mode in [
        emerge_mapper::tiles::Mode::Map,
        emerge_mapper::tiles::Mode::Tiles,
        emerge_mapper::tiles::Mode::Meshes,
    ] {
        let mut app = badges_up(&root, mode);
        stage_a_piece(&mut app);
        for _ in 0..8 {
            app.update();
        }

        // Every cluster and every leader segment, by the entity that owns it, before the zoom.
        let census = |app: &mut App| -> Vec<(String, Rect)> {
            let mut out: Vec<(String, Rect)> = {
                let mut q = app
                    .world_mut()
                    .query::<(&BadgeCluster, &ComputedNode, &UiGlobalTransform)>();
                q.iter(app.world())
                    .map(|(c, n, tf)| {
                        (
                            format!("{:?}", c.0),
                            Rect::from_center_size(tf.translation, n.size()),
                        )
                    })
                    .collect()
            };
            let mut segs: Vec<(String, Rect)> = {
                let mut q = app.world_mut().query_filtered::<(
                    Entity,
                    &ComputedNode,
                    &UiGlobalTransform,
                ), With<LeadSeg>>();
                q.iter(app.world())
                    .map(|(e, n, tf)| {
                        (
                            format!("leader {e:?}"),
                            Rect::from_center_size(tf.translation, n.size()),
                        )
                    })
                    .collect()
            };
            out.append(&mut segs);
            // `Query` order is not stable across frames any more than across `App`s, so the
            // comparison needs a stated order rather than the iteration's.
            out.sort_by(|a, b| a.0.cmp(&b.0));
            out
        };

        let before = census(&mut app);
        assert!(
            !before.is_empty(),
            "{}: nothing is drawn, so this proves nothing about what moves",
            mode.label()
        );

        // **Zoom, exactly the way the wheel does** — `Rig::height` is the orthographic viewport
        // height in metres, and it is the whole of what zooming means here (`view.rs`).
        {
            let mut rig = app
                .world_mut()
                .get_resource_mut::<emerge_mapper::view::Rig>()
                .expect("the editor carries a camera rig");
            rig.height *= 2.0;
        }
        for _ in 0..8 {
            app.update();
        }

        let after = census(&mut app);
        let moved: Vec<String> = before
            .iter()
            .zip(after.iter())
            .filter(|((_, a), (_, b))| a.min.distance(b.min) > 0.5 || a.max.distance(b.max) > 0.5)
            .map(|((name, a), (_, b))| format!("{name}: {a:?} -> {b:?}"))
            .collect();
        assert_eq!(
            before.len(),
            after.len(),
            "{}: the zoom changed how many badges there are, which is a different bug again",
            mode.label()
        );
        assert!(
            moved.is_empty(),
            "{}: zooming moved the overlay, so the layout cannot be learned:\n  {}",
            mode.label(),
            moved.join("\n  ")
        );
    }
}

/// **The window shapes the crossing rules are enforced at.**
///
/// One shape proves almost nothing here. Which rung of the placement ladder a box lands on is a
/// function of how much free ground the docks leave, and that is a function of the window: the same
/// tab that packs every badge beside its own row at 2560 wide has to send half of them to the rail
/// at 1280. `None` is the harness default; the rest are real laptop shapes, narrowest last, because
/// the narrow ones are where the ladder is actually forced.
const CROWDED_SHAPES: [Option<(u32, u32)>; 4] = [
    None,
    Some((2560, 1406)),
    Some((1680, 1050)),
    Some((1280, 800)),
];

/// **A project with enough in it to fill the docks** — which is the state every badge rule has to
/// hold in and the one the fixtures were not reaching.
///
/// A one-descriptor fixture leaves both docks nearly empty, so every badge hugs its own row on the
/// first rung of the ladder and the hard rungs — the rail, the unconditional floor — are never
/// climbed. That is how a crossing shipped under a green `no_two_leaders_cross`: the rule was true
/// of a layout no author ever sees. The real kit this editor opens carries 689 meshes; forty is
/// enough to make the piece list scroll, the detail pane fill and the free ground run out, which is
/// all this needs to exercise.
fn crowded_root(name: &str) -> std::path::PathBuf {
    let ids: Vec<String> = (0..40).map(|i| format!("piece_{i:02}")).collect();
    let mut fixture = Fixture::new(name);
    for id in &ids {
        fixture = fixture.descriptor(id, "alpha");
    }
    // **`piece_00`, not `alpha/piece_00`.** `descriptor(id, "alpha")` mints the id verbatim and puts
    // its mesh under `assets/alpha/`; the pack is a folder, never a namespace. Both rows named an id
    // nothing carries, so `redraw_placements` dropped them without a word and this "crowded" project
    // had an **empty map** — the same defect the seven `alpha/floor` fixtures had, and the reason
    // `Fixture::place` refuses an unknown id now.
    fixture
        .place("piece_00", (0.0, 0.0))
        .place("piece_01", (2.0, 0.0))
        .build("m")
}

/// **Every control's badges are tied to it** — at least one hairline segment, connected at both
/// ends: one touching the anchor, one touching the box. The tie is what bought the freedom to pack
/// boxes on free ground at all; lose it and a displaced badge is just a floating box again.
#[test]
fn every_control_cluster_is_tied_to_its_anchor() {
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use emerge_mapper::badges::{BadgeCluster, Lead, LeadSeg};
    use emerge_mapper::keys::Home;

    let root = Fixture::new("badgetie")
        .descriptor("floor", "alpha")
        .place("floor", (0.0, 0.0))
        .build("m");

    let mut untied = Vec::new();
    let mut checked = 0usize;
    for surface in [None, Some((2560u32, 1406u32))] {
        for mode in TABS {
            let mut app = badges_up(&root, mode);
            stage_a_piece(&mut app);
            if let Some((w, h)) = surface {
                harness::resize_surface(&mut app, w, h).unwrap_or_else(|e| panic!("{e}"));
                for _ in 0..8 {
                    app.update();
                }
            }
            let tag = match surface {
                None => mode.label().to_owned(),
                Some((w, h)) => format!("{} at {w}x{h}", mode.label()),
            };

            let clusters: Vec<(emerge_mapper::keys::ControlId, Rect, [Entity; 3])> = {
                let mut q = app.world_mut().query::<(
                    &BadgeCluster,
                    &ComputedNode,
                    &UiGlobalTransform,
                    &Visibility,
                    &Lead,
                )>();
                q.iter(app.world())
                    .filter(|(_, n, _, vis, _)| n.size() != Vec2::ZERO && **vis != Visibility::Hidden)
                    .filter_map(|(c, n, tf, _, lead)| match c.0 {
                        Home::Control(id) => {
                            Some((id, Rect::from_center_size(tf.translation, n.size()), lead.0))
                        }
                        Home::Legend => None,
                    })
                    .collect()
            };
            let anchors: Vec<(emerge_mapper::keys::ControlId, Rect)> = {
                let mut q = app
                    .world_mut()
                    .query::<(&emerge_mapper::chrome::Control, &ComputedNode, &UiGlobalTransform)>();
                q.iter(app.world())
                    .filter(|(_, n, _)| n.size() != Vec2::ZERO)
                    .map(|(c, n, tf)| (c.0, Rect::from_center_size(tf.translation, n.size())))
                    .collect()
            };
            let mut seg_q = app.world_mut().query_filtered::<(
                &ComputedNode,
                &UiGlobalTransform,
                &Visibility,
            ), With<LeadSeg>>();
            let world = app.world();
            for (id, cluster, segs) in clusters {
                let Some((_, anchor)) = anchors.iter().find(|(a, _)| *a == id) else {
                    continue;
                };
                checked += 1;
                let shown: Vec<Rect> = segs
                    .iter()
                    .filter_map(|e| seg_q.get(world, *e).ok())
                    .filter(|(n, _, vis)| **vis != Visibility::Hidden && n.size() != Vec2::ZERO)
                    .map(|(n, tf, _)| Rect::from_center_size(tf.translation, n.size()))
                    .collect();
                if shown.is_empty() {
                    untied.push(format!("{tag}: {id:?} has no leader at all"));
                    continue;
                }
                if let Some(fat) = shown.iter().find(|r| r.width().min(r.height()) > 4.0) {
                    untied.push(format!(
                        "{tag}: {id:?} has a {:.0}x{:.0} segment — that is a box, not a hairline",
                        fat.width(),
                        fat.height()
                    ));
                }
                let slack = 3.0;
                let near = |r: &Rect, of: Rect| {
                    let grown = Rect::from_corners(
                        of.min - Vec2::splat(slack),
                        of.max + Vec2::splat(slack),
                    );
                    !grown.intersect(*r).is_empty()
                };
                if !shown.iter().any(|r| near(r, *anchor)) {
                    untied.push(format!("{tag}: {id:?}'s leader never touches the control"));
                }
                if !shown.iter().any(|r| near(r, cluster)) {
                    untied.push(format!("{tag}: {id:?}'s leader never touches its box"));
                }
            }
        }
    }
    untied.sort();
    untied.dedup();
    assert!(
        untied.is_empty(),
        "these badges are not tied to what they name:\n  {}",
        untied.join("\n  ")
    );
    assert!(
        checked >= TABS.len() * 4,
        "only {checked} tied cluster(s) were measured; the rule is being enforced against nothing"
    );
}

/// **A control the census calls banded really is in a band.**
///
/// `keys::ControlId::in_a_band` decides the badge's *shape* — a bare chord in a band, a labelled row
/// in a dock — and it is a stated fact about the editor's layout. A stated fact drifts: move a
/// control out of the chrome bar and the flag goes on claiming there is no room for words beside it,
/// so its verb stays unlabelled for no reason anyone can see.
#[test]
fn a_control_in_a_band_really_is_in_one() {
    use bevy::ui::ComputedNode;

    let root = Fixture::new("badgeband2")
        .descriptor("floor", "alpha")
        .build("m");

    let mut wrong = Vec::new();
    let mut seen = 0usize;
    for mode in TABS {
        let mut app = badges_up(&root, mode);
        let bands = {
            let f = app.world().resource::<emerge_mapper::chrome::Frame>();
            [f.chrome_bar, f.door_strip, f.status]
        };
        let found: Vec<(emerge_mapper::keys::ControlId, Entity)> = {
            let mut q = app
                .world_mut()
                .query::<(Entity, &emerge_mapper::chrome::Control, &ComputedNode)>();
            q.iter(app.world())
                .filter(|(_, _, n)| n.size() != Vec2::ZERO)
                .map(|(e, c, _)| (c.0, e))
                .collect()
        };
        for (id, entity) in found {
            seen += 1;
            let inside = {
                let world = app.world();
                let mut at = Some(entity);
                let mut hit = false;
                while let Some(e) = at {
                    if bands.contains(&e) {
                        hit = true;
                        break;
                    }
                    at = world.get::<ChildOf>(e).map(|p| p.parent());
                }
                hit
            };
            if inside != id.in_a_band() {
                wrong.push(format!(
                    "{}: {id:?} says in_a_band() == {}, and the tree says {inside}",
                    mode.label(),
                    id.in_a_band()
                ));
            }
        }
    }
    wrong.sort();
    wrong.dedup();
    assert!(
        wrong.is_empty(),
        "`ControlId::in_a_band` no longer describes where these controls are, so their badges get \
         the wrong shape:\n  {}",
        wrong.join("\n  ")
    );
    assert!(
        seen >= TABS.len(),
        "only {seen} laid-out control(s) were checked across {} tabs",
        TABS.len()
    );
}

// ── the tag block's keyboard path ────────────────────────────────────────────────────────────────

/// **Typing narrows the tag block, and every axis keeps its heading.**
///
/// The block draws the project's whole vocabulary so an author reads it rather than remembering it —
/// 55 chips on the shipped kit, of which a piece holds three to six. A filter is this editor's
/// answer to a list too long to scan, and `filter.rs`'s module note states the rule it has to obey:
/// *"the rows that survive stay in exactly the order they were in… Nothing is re-ranked, ever."*
/// Sears & Shneiderman 1994 (`10.1145/174630.174632`) is the measurement behind that — menus whose
/// items moved were slower and users disliked them.
///
/// So the two things asserted here are the two things that could go wrong: the chips that do not
/// match are **gone**, and the axes they left behind are **still there**, saying `-`. An axis that
/// vanished with its last chip would move the three below it on every keystroke, and would make
/// "not on this axis" look exactly like "this kit has no such axis".
#[test]
fn the_tag_filter_narrows_the_block_and_every_axis_keeps_its_heading() {
    use emerge_mapper::filter::{Filters, Pane};
    use emerge_mapper::keys::Action;

    let root = Fixture::new("tagfilter")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app =
        harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }
    let id = app
        .world()
        .resource::<emerge_mapper::project::Project>()
        .library
        .descriptors
        .first()
        .map(|d| d.id.clone())
        .unwrap_or_else(|| panic!("the fixture must carry a descriptor"));
    app.world_mut()
        .resource_mut::<emerge_mapper::tiles::ImportState>()
        .selected_library_id = Some(id);
    for _ in 0..4 {
        app.update();
    }

    press_once(&mut app, emerge_mapper::keys::binding(Action::FocusTagFilter).key);
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<Filters>().focus_pane(),
        Some(Pane::Tags),
        "`/` must put the cursor in the tag box — before this the block was 55 mouse targets and \
         no keyboard path at all"
    );
    // One more frame before typing: every field here drains the stream while shut, so the key that
    // opens it cannot become its first character (`keys.rs`, the `xseam` bug).
    app.update();
    for (logical, code) in [
        ("w", KeyCode::KeyW),
        ("o", KeyCode::KeyO),
        ("r", KeyCode::KeyR),
        ("k", KeyCode::KeyK),
    ] {
        tap_key(
            &mut app,
            bevy::input::keyboard::Key::Character(logical.into()),
            code,
        );
    }
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<Filters>().text(Pane::Tags),
        "work",
        "the tag box takes the keys"
    );

    // **The box shows what was typed**, which it did not when this was first put in a frame: the
    // detail pane despawns and respawns on every keystroke, so the box is a new entity each time and
    // `filter::refresh` skipped it — it had already spent the `is_changed` that repaint needed. The
    // block read `1 of 55` under a box that still said `filter tags`.
    let typed: Vec<String> = {
        let mut q = app
            .world_mut()
            .query::<(&Text, &emerge_mapper::filter::FilterText)>();
        q.iter(app.world())
            .filter(|(_, f)| f.0 == Pane::Tags)
            .map(|(t, _)| t.0.clone())
            .collect()
    };
    assert_eq!(
        typed,
        vec!["work_".to_owned()],
        "the tag box must show the search and its caret, not the placeholder it was respawned with"
    );

    let inside = tag_block_text(&mut app);
    assert!(
        inside.iter().any(|t| t == "worktop"),
        "`work` must leave `worktop` standing — the block holds {inside:?}"
    );
    for gone in ["prop", "inert", "plain"] {
        assert!(
            !inside.iter().any(|t| t == gone),
            "`{gone}` does not match `work` and must not be drawn — the block holds {inside:?}"
        );
    }
    for heading in ["KIND", "DOES", "LOOKS", "OFFERS"] {
        assert!(
            inside.iter().any(|t| t == heading),
            "the `{heading}` heading left the block when its chips did — that moves every axis \
             below it on each keystroke. It holds {inside:?}"
        );
    }
    assert_eq!(
        inside.iter().filter(|t| t.as_str() == "-").count(),
        3,
        "the three axes with no match must each say `-` rather than leaving a gap — {inside:?}"
    );
    assert!(
        inside.iter().any(|t| t.contains("Enter takes it")),
        "with exactly one token left the block must say so before the key is pressed — {inside:?}"
    );

    // And `Esc` gives the whole vocabulary back, which is the one key that always does.
    tap_key(&mut app, bevy::input::keyboard::Key::Escape, KeyCode::Escape);
    for _ in 0..4 {
        app.update();
    }
    let back = tag_block_text(&mut app);
    for token in ["prop", "inert", "plain", "worktop"] {
        assert!(
            back.iter().any(|t| t == token),
            "`Esc` must put `{token}` back — the block holds {back:?}"
        );
    }
}

/// **`Enter` in the tag box takes the one match, and refuses a tie rather than guessing.**
///
/// The refusal is not defensive coding: on the shipped vocabulary `door` is a token on **two** axes
/// (`kind` and `effects`), so `door` + `Enter` is genuinely ambiguous, and no amount of further
/// typing separates them. Writing either one would be the editor deciding a tag on the author's
/// behalf. It says how many matched and leaves the keyboard where it is, mid-word.
#[test]
fn enter_in_the_tag_box_takes_the_one_match_and_refuses_a_tie() {
    use emerge_mapper::filter::{Filters, Pane};
    use emerge_mapper::keys::Action;

    let root = Fixture::new("tagenter")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app =
        harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }
    let id = app
        .world()
        .resource::<emerge_mapper::project::Project>()
        .library
        .descriptors
        .first()
        .map(|d| d.id.clone())
        .unwrap_or_else(|| panic!("the fixture must carry a descriptor"));
    app.world_mut()
        .resource_mut::<emerge_mapper::tiles::ImportState>()
        .selected_library_id = Some(id.clone());
    for _ in 0..4 {
        app.update();
    }
    let surfaces = |app: &App, id: &str| {
        app.world()
            .resource::<emerge_mapper::project::Project>()
            .library
            .descriptors
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.offers.surfaces.clone())
            .unwrap_or_default()
    };
    assert!(
        surfaces(&app, &id).is_empty(),
        "the fixture piece starts with no surface token, which is what makes the toggle visible"
    );

    press_once(&mut app, emerge_mapper::keys::binding(Action::FocusTagFilter).key);
    for _ in 0..2 {
        app.update();
    }
    app.update();
    for (logical, code) in [
        ("w", KeyCode::KeyW),
        ("o", KeyCode::KeyO),
        ("r", KeyCode::KeyR),
        ("k", KeyCode::KeyK),
    ] {
        tap_key(
            &mut app,
            bevy::input::keyboard::Key::Character(logical.into()),
            code,
        );
    }
    tap_key(&mut app, bevy::input::keyboard::Key::Enter, KeyCode::Enter);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        surfaces(&app, &id),
        vec!["worktop".to_owned()],
        "`Enter` on a single match must toggle it through the same write a click makes"
    );
    assert_eq!(
        app.world().resource::<Filters>().text(Pane::Tags),
        "",
        "and clear the box, so the next token is typed rather than deleted first"
    );
    assert_eq!(
        app.world().resource::<Filters>().focus_pane(),
        Some(Pane::Tags),
        "and keep the keyboard — `Esc` is what leaves"
    );

    // Now a tie: `p` is in `prop` (kind), `plain` (look) and `worktop` (surfaces).
    tap_key(
        &mut app,
        bevy::input::keyboard::Key::Character("p".into()),
        KeyCode::KeyP,
    );
    // **The WHOLE descriptor, not two of its axes.**
    //
    // It compared `offers.surfaces` and `kind` — and the third token matching `p` is `plain`, which
    // is on the **`look`** axis, which neither of those covers. So an `Enter` that resolved the tie
    // by writing `plain` wrote a tag the author never chose and this test passed it. "Nothing was
    // written" is a claim about the row, so the row is what is compared;
    // `a_settled_effect_survives_the_next_save` compares a whole descriptor for the same reason.
    let row = |app: &App, id: &str| {
        app.world()
            .resource::<emerge_mapper::project::Project>()
            .library
            .descriptors
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .unwrap_or_else(|| panic!("the fixture must carry `{id}`"))
    };
    let before = row(&app, &id);
    tap_key(&mut app, bevy::input::keyboard::Key::Enter, KeyCode::Enter);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        row(&app, &id),
        before,
        "three tokens match `p` — one on each of `kind`, `look` and `surfaces` — so `Enter` must \
         write nothing at all rather than pick one"
    );
    let said = app
        .world()
        .resource::<emerge_mapper::tiles::ImportState>()
        .status
        .note_text()
        .to_owned();
    assert!(
        said.contains('3') && said.contains("match"),
        "a tie has to say how many, so the author knows to keep typing — it said {said:?}"
    );
}

/// **The two objective steps of `guides/label_a_mesh.json` can actually be walked.**
///
/// Both name the checkpoint `the piece carries` and both hand it a payload; the walk is `/`, type the
/// token, `Enter`. The checkpoint must answer `false` before the keystroke and `true` after it, for
/// the axis and token **the script itself names** — which is why the payload is read out of the JSON
/// rather than restated here, and why the fixture declares the script's own `look` words.
///
/// # It shipped unwalkable, and every existing test agreed it was fine
///
/// The payloads were spelled `"args"`. `Step`'s field is `with`, every field of `Step` was
/// `#[serde(default)]`, and serde said nothing — so `with` deserialised to `None`, `carries_token`
/// was handed `null` for ever, and both steps could never pass. Two things hid it:
/// `every_checkpoint_a_shipped_guide_names_is_registered_and_runs` reads the corpus as a
/// `serde_json::Value` and only checks that the checkpoint NAME resolves, and `label_a_mesh.json`
/// had no drive test at all. So the guide looked complete, the suite was green, and an author
/// following the card was parked at step four indefinitely.
///
/// # What this does NOT cover, measured rather than assumed
///
/// Two sibling fixes landed with the typo. The misspelled-axis refusal is a `panic!`, and reaching it
/// needs `catch_unwind` plus a swapped panic hook, which is process-global in a binary whose tests
/// run in parallel — see the note at the foot of the body.
///
/// The other is the layer: the checkpoint resolved the piece through `project.library`, the
/// *layered* view, while `tiles::toggle_tag` writes `project.measured`. Reading the layer you write
/// is right on its face, but **this test cannot tell the two apart and neither can a fixture.** It
/// was tried: a `policy::Patch` pinning the piece's `look` to `plain` does make the layered view
/// differ at open, and yet after the toggle the layered view reads `["plain", "wood", "worn"]` —
/// `pick` replaces, so the patched value has evidently reached the measurement layer, and both
/// layers then carry whatever the author typed. Swapping `measured` for `library` here leaves this
/// test green. Do not take that as licence to swap it back: it means the divergence is unreachable
/// through the fixture, not that the layers agree by design.
#[cfg(feature = "debugger")]
#[test]
fn the_label_a_mesh_script_s_objective_steps_can_actually_be_walked() {
    use emerge_mapper::keys::Action;

    // Read both steps' payloads out of the shipped script. `guide_step` panics if a label moves, so
    // renaming a card moves this test or fails it by name.
    let steps = [
        guide_step("label_a_mesh.json", "take it with Enter"),
        guide_step(
            "label_a_mesh.json",
            "type the next one without leaving the box",
        ),
    ];
    let words: Vec<(String, String)> = steps
        .iter()
        .map(|(name, with)| {
            assert_eq!(
                name, "the piece carries",
                "this test drives `the piece carries`; the script now names {name:?}"
            );
            let axis = with["axis"].as_str().unwrap_or_else(|| {
                panic!(
                    "the step's payload names no axis: {with}. If the key is `args`, it is `with` — \
                     serde drops an unknown field silently and the step can never pass"
                )
            });
            let token = with["token"]
                .as_str()
                .unwrap_or_else(|| panic!("the step's payload names no token: {with}"));
            (axis.to_owned(), token.to_owned())
        })
        .collect();

    let looks: Vec<&str> = words.iter().map(|(_, t)| t.as_str()).collect();
    let root = Fixture::new("labelmeshwalk")
        .descriptor("floor", "alpha")
        .look_tokens(&looks)
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
        .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }
    let id = app
        .world()
        .resource::<emerge_mapper::project::Project>()
        .library
        .descriptors
        .first()
        .map(|d| d.id.clone())
        .unwrap_or_else(|| panic!("the fixture must carry a descriptor"));
    app.world_mut()
        .resource_mut::<emerge_mapper::tiles::ImportState>()
        .selected_library_id = Some(id.clone());
    for _ in 0..4 {
        app.update();
    }

    press_once(&mut app, emerge_mapper::keys::binding(Action::FocusTagFilter).key);
    for _ in 0..3 {
        app.update();
    }

    for ((axis, token), (name, with)) in words.iter().zip(steps.iter()) {
        assert!(
            !checkpoint(&mut app, name, with.clone()),
            "`{token}` must not be on `{axis}` before the step that puts it there — otherwise the \
             step passes without the author doing anything"
        );
        for c in token.chars() {
            let code = letter_key(c);
            tap_key(
                &mut app,
                bevy::input::keyboard::Key::Character(c.to_string().into()),
                code,
            );
        }
        tap_key(&mut app, bevy::input::keyboard::Key::Enter, KeyCode::Enter);
        for _ in 0..4 {
            app.update();
        }
        assert!(
            checkpoint(&mut app, name, with.clone()),
            "after typing `{token}` and pressing Enter the step must pass. It reads `{axis}` on the \
             focused piece, which now holds look {:?}",
            app.world()
                .resource::<emerge_mapper::project::Project>()
                .measured
                .descriptors
                .iter()
                .find(|d| d.id == id)
                .map(|d| d.look.clone())
                .unwrap_or_default()
        );
    }

    // **Not tested here: a misspelled axis panics.** `carries_token`'s `Some(other) => panic!(..)`
    // arm is what stops a card naming `"looks"` parking its author for ever in front of a correctly
    // tagged piece — but reaching it from a test needs `catch_unwind` plus a swapped panic hook, and
    // the hook is process-global in a binary whose tests run in parallel. Buying that assertion with
    // cross-test interference is a worse trade than reading the arm.
}

/// **The open settles the derived half of `effects`, and the first save must not undo it.**
///
/// `Project::open` reconciles `implies` across the whole merged library — *"the moment to reconcile
/// the whole set is when the whole set is in hand"* — because a row written before the rule landed
/// carries the kind without the effect it implies. That correction landed on `project.library`, the
/// **derived** view, and on nothing else: `measured` and each kit's layer keep the authored lists.
///
/// So the first save of anything rebuilt the merge from those unsettled layers and put every row
/// back. Not the row being edited — *every* row, including the ones nobody had touched, until the
/// project was reopened. `changed_ids` then read that settled-against-unsettled difference as news
/// and marked them all for a Map redraw, which is why the second assertion compares a whole
/// descriptor rather than one field: an untouched row that differs at all is a row this door has
/// just claimed was edited.
///
/// The save is driven through the tag box because that is an author's ordinary write — a real
/// `commit_measured`, not a call to it.
#[test]
fn a_settled_effect_survives_the_next_save() {
    use emerge_mapper::keys::{Action, binding};

    let root = Fixture::new("settlesave")
        .kind_implies("powered")
        .descriptor("floor", "alpha")
        .descriptor("bench", "alpha")
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
        .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }

    let row = |app: &App, id: &str| {
        app.world()
            .resource::<emerge_mapper::project::Project>()
            .library
            .descriptors
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .unwrap_or_else(|| panic!("the fixture must carry `{id}`"))
    };

    // Both rows are `kind: ["prop"]`, `effects: ["inert"]` on disk. The open is the only thing that
    // can have put `powered` on them, so this is the state the save has to preserve.
    for id in ["floor", "bench"] {
        assert!(
            row(&app, id).effects.iter().any(|e| e == "powered"),
            "the open did not settle `{id}` — this test would prove nothing about the save. It \
             holds {:?}",
            row(&app, id).effects
        );
    }
    let bench_before = row(&app, "bench");

    // An ordinary write to ONE row: focus the tag box, type the one token that matches, take it.
    app.world_mut()
        .resource_mut::<emerge_mapper::tiles::ImportState>()
        .selected_library_id = Some("floor".to_owned());
    for _ in 0..4 {
        app.update();
    }
    press_once(&mut app, binding(Action::FocusTagFilter).key);
    for _ in 0..2 {
        app.update();
    }
    app.update();
    for (logical, code) in [
        ("w", KeyCode::KeyW),
        ("o", KeyCode::KeyO),
        ("r", KeyCode::KeyR),
        ("k", KeyCode::KeyK),
    ] {
        tap_key(
            &mut app,
            bevy::input::keyboard::Key::Character(logical.into()),
            code,
        );
    }
    tap_key(&mut app, bevy::input::keyboard::Key::Enter, KeyCode::Enter);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        row(&app, "floor").offers.surfaces,
        vec!["worktop".to_owned()],
        "the save did not happen, so nothing below is about a rebuild"
    );

    assert!(
        row(&app, "floor").effects.iter().any(|e| e == "powered"),
        "the edited row lost the effect its kind implies when the library was rebuilt — it holds \
         {:?}",
        row(&app, "floor").effects
    );
    assert_eq!(
        row(&app, "bench"),
        bench_before,
        "a save to another piece rewrote `bench`, which nobody edited — the rebuild dropped the \
         settling the open did, and `changed_ids` will report this row as news and redraw it"
    );
}

/// **A label that arrives applies itself, with nobody pressing anything.**
///
/// The verbs that used to answer a proposal — `U` to apply, `Y` to discard — retired on 2026-08-20.
/// Asked for at the keyboard: *"never ask for confirmation on a labeling except for labeling all…
/// once you approve, everything is automatically labeled and applied."* So the thing to pin is that
/// **no key is pressed in this test at all**: two proposals are staged, frames are stepped, and both
/// have landed on their pieces.
///
/// One per frame is deliberate and asserted too — `apply_suggestion` may re-photograph a piece it
/// had to right first, and a pump that drained the set in one frame would queue those shots faster
/// than the booth can take them.
#[test]
fn a_staged_proposal_applies_itself_with_no_key_pressed() {
    use emerge_mapper::labels::{Entry, Suggestions};
    use emerge_mapper::tiles::EditTarget;

    let root = Fixture::new("autoapply")
        .unjudged_descriptor("floor", "alpha")
        .unjudged_descriptor("wall", "alpha")
        .build("m");
    let mut app =
        harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..3 {
        app.update();
    }
    let pieces: Vec<(String, String)> = app
        .world()
        .resource::<emerge_mapper::project::Project>()
        .library
        .descriptors
        .iter()
        .map(|d| (d.id.clone(), d.mesh.clone().unwrap_or_default()))
        .collect();
    assert_eq!(pieces.len(), 2, "two pieces, so the one-per-frame pump is visible");
    for (id, mesh) in &pieces {
        app.world_mut()
            .resource_mut::<Suggestions>()
            .insert(&EditTarget::Library(id.clone()), Entry::for_test(mesh));
    }
    assert_eq!(app.world().resource::<Suggestions>().pending(), 2);

    app.update();
    assert_eq!(
        app.world().resource::<Suggestions>().pending(),
        1,
        "one per frame — a pump that drained the set would outrun the photo booth"
    );
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<Suggestions>().pending(),
        0,
        "and nothing is left waiting for a keypress that no longer exists"
    );

    // The description each proposal carried is on the piece, which is what "applied" means here.
    for (id, _) in &pieces {
        let note = app
            .world()
            .resource::<emerge_mapper::project::Project>()
            .library
            .get(id)
            .and_then(|d| d.note.clone());
        assert_eq!(
            note.as_deref(),
            Some("a thing"),
            "`{id}` did not take the proposal that was staged for it"
        );
    }
}

/// **Delete sends a library mesh back to `NOT IMPORTED` and leaves the cursor on it there.**
///
/// Reported at the keyboard, 2026-08-20: *"if I delete a mesh on the meshes tab on the right scroll
/// view, it should send it back to not import it, and then switch over so that it has focus on the
/// not imported."* It did neither. `Delete` took the entry out of the library and stopped: no
/// rescan, so the mesh was not among the candidates yet, and `selected` still pointed at whatever
/// row it had pointed at before. `Shift+Delete` did the whole trip — two chords for one act, which
/// is what this merge removed.
///
/// The shelf switch is not asserted separately because it is not separate state: `Shelf` is derived
/// from `selected_library_id.is_none()`, so the two assertions below *are* "the list flipped to
/// NOT IMPORTED with the piece under the highlight".
#[test]
fn delete_sends_a_mesh_back_to_not_imported_and_lands_on_it() {
    use emerge_mapper::keys::Action;
    use emerge_mapper::tiles::ImportState;

    let root = Fixture::new("sendback_cursor")
        // **Two unimported meshes that sort BEFORE the pack the library pieces live in**, so the
        // reborn row cannot be index 0.
        //
        // Without them the candidate list is empty until the delete, the mesh comes back at 0, and
        // `ImportState::selected` was already 0 — so `remove_tile`'s whole cursor move could be
        // deleted and the last assertion here would still hold. `import::scan` sorts by path and
        // `assets/aaa/…` sorts before `assets/alpha/…`, so the reborn row lands at 2.
        //
        // The name is `sendback_cursor` rather than `sendback` because `Fixture::new` keys its temp
        // directory on the name alone: `cmd_remove_falls_to_the_place_selection` already builds a
        // `sendback`, and these run in parallel — two tests sharing one directory, one of which now
        // writes a pack the other would scan.
        .pack("aaa", &["spare_one", "spare_two"])
        .descriptor("floor", "alpha")
        .descriptor("wall", "alpha")
        .build("m");
    let mut app =
        harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..4 {
        app.update();
    }
    let (id, mesh) = app
        .world()
        .resource::<emerge_mapper::project::Project>()
        .library
        .descriptors
        .first()
        .map(|d| (d.id.clone(), d.mesh.clone().unwrap_or_default()))
        .unwrap_or_else(|| panic!("the fixture must carry a descriptor with a mesh"));
    app.world_mut()
        .resource_mut::<ImportState>()
        .selected_library_id = Some(id.clone());
    for _ in 0..3 {
        app.update();
    }
    let candidate_at = |app: &App, mesh: &str| {
        app.world()
            .resource::<ImportState>()
            .candidates
            .iter()
            .position(|c| c.mesh == mesh)
    };
    assert_eq!(
        candidate_at(&app, &mesh),
        None,
        "while it is in the library it is not a candidate — that is what the two shelves mean"
    );

    press_once(&mut app, emerge_mapper::keys::binding(Action::RemoveTile).key);
    for _ in 0..4 {
        app.update();
    }

    assert!(
        app.world()
            .resource::<emerge_mapper::project::Project>()
            .library
            .get(&id)
            .is_none(),
        "`{id}` is out of the library"
    );
    let back = candidate_at(&app, &mesh);
    assert!(
        back.is_some(),
        "and its mesh is back among the candidates — without the rescan it would not be there yet, \
         which is exactly what made the old Delete look like it had done nothing"
    );
    let state = app.world().resource::<ImportState>();
    assert_eq!(
        state.selected_library_id, None,
        "the cursor leaves the library shelf, which is what puts NOT IMPORTED on screen"
    );
    assert_ne!(
        back,
        Some(0),
        "the fixture must put the reborn row somewhere other than index 0, or the assertion below \
         is satisfied by a cursor that never moved — it came back at {back:?}"
    );
    assert_eq!(
        Some(state.selected),
        back,
        "and lands on the piece that was just sent back, not on whichever row it was on before"
    );
}

/// **The library shelf reads newest-defined first, and coming back to it returns you where you were.**
///
/// Asked for at the keyboard, 2026-08-20: *"whenever a user switches over to that list, the most
/// recent item that they're working on is the one that's already selected."* Two things were wrong.
/// `library.ron` is appended to, so the piece you had just imported was at the *bottom* of a list
/// that is 88 rows today and is meant to be 700; and `right` seeded the selection with `.first()`,
/// which was therefore the oldest piece in the project, every time.
///
/// **Only the definition order is reversed.** A row moves when a new piece is defined and at no
/// other time — editing a description does not reshuffle the list, which was declined at the
/// keyboard once the cost was named (`docs/ui.md` §3.5, "fixed positions, never reordered by
/// recency"). That is asserted here too, because it is the half that is easy to lose later.
#[test]
fn the_library_reads_newest_first_and_remembers_the_row_you_left() {
    use emerge_mapper::filter::Filters;
    use emerge_mapper::keys::Action;
    use emerge_mapper::project::Project;
    use emerge_mapper::tiles::{ImportState, library_ids_for_test};

    let root = Fixture::new("newestfirst")
        .descriptor("oldest", "alpha")
        .descriptor("middle", "alpha")
        .descriptor("newest", "alpha")
        .build("m");
    let mut app =
        harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
            .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..4 {
        app.update();
    }
    let shelf = |app: &App| {
        library_ids_for_test(
            app.world().resource::<Project>(),
            app.world().resource::<Filters>(),
            false,
            None,
        )
    };
    assert_eq!(
        shelf(&app),
        vec!["newest".to_owned(), "middle".to_owned(), "oldest".to_owned()],
        "the shelf reads newest-defined first — `library.ron` is appended to, so its own order is \
         oldest first and the piece you just imported was at the bottom"
    );

    // `right` with nothing remembered lands on the top row, which is the piece defined last.
    press_once(&mut app, emerge_mapper::keys::binding(Action::FocusLibrary).key);
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        app.world()
            .resource::<ImportState>()
            .selected_library_id
            .as_deref(),
        Some("newest"),
        "arriving with no history lands on the most recently defined piece, not the oldest"
    );

    // Move to another row, leave for the candidates, and come back.
    app.world_mut()
        .resource_mut::<ImportState>()
        .selected_library_id = Some("middle".to_owned());
    for _ in 0..2 {
        app.update();
    }
    press_once(&mut app, emerge_mapper::keys::binding(Action::FocusCandidates).key);
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        app.world()
            .resource::<ImportState>()
            .selected_library_id,
        None,
        "`left` walks the candidates, which is what puts NOT IMPORTED on screen"
    );
    press_once(&mut app, emerge_mapper::keys::binding(Action::FocusLibrary).key);
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(
        app.world()
            .resource::<ImportState>()
            .selected_library_id
            .as_deref(),
        Some("middle"),
        "coming back returns you to the row you left — before this it seeded `.first()` and threw \
         away where you had got to"
    );

    // **Editing does not reshuffle** — and the edit goes in through the keyboard, because that is
    // the only path that rebuilds the list under assertion.
    //
    // # It used to mutate a list nothing here reads
    //
    // The block wrote `project.measured.descriptors` while `shelf()` reads
    // `project.library.descriptors`, and nothing rebuilds one from the other outside
    // `commit_measured` — so the edit never reached the list being asserted on, and *"the row did
    // not move"* was true of a row nothing had touched. The whole block could be deleted and this
    // stayed green. Typing a token into the tag box and taking it with `Enter` is the write an
    // author actually makes and it goes through `commit_measured`; the neighbouring
    // `a_settled_effect_survives_the_next_save` drives the same path for the same reason.
    app.world_mut()
        .resource_mut::<ImportState>()
        .selected_library_id = Some("oldest".to_owned());
    for _ in 0..4 {
        app.update();
    }
    press_once(&mut app, emerge_mapper::keys::binding(Action::FocusTagFilter).key);
    for _ in 0..2 {
        app.update();
    }
    // One more, as the two neighbouring tag tests do: the box has to be focused *and* laid out
    // before a character means anything to it.
    app.update();
    for (logical, code) in [
        ("w", KeyCode::KeyW),
        ("o", KeyCode::KeyO),
        ("r", KeyCode::KeyR),
        ("k", KeyCode::KeyK),
    ] {
        tap_key(
            &mut app,
            bevy::input::keyboard::Key::Character(logical.into()),
            code,
        );
    }
    tap_key(&mut app, bevy::input::keyboard::Key::Enter, KeyCode::Enter);
    for _ in 0..3 {
        app.update();
    }
    // **The premise, asserted.** `worktop` is the only token in the fixture's vocabulary matching
    // `work`, so `Enter` takes it. Without this the assertion below is about a piece nobody edited,
    // which is exactly the state the old block left it in.
    assert_eq!(
        app.world()
            .resource::<Project>()
            .library
            .descriptors
            .iter()
            .find(|d| d.id == "oldest")
            .map(|d| d.offers.surfaces.clone())
            .unwrap_or_default(),
        vec!["worktop".to_owned()],
        "the keyboard write did not land on `oldest`, so nothing below is about an edited row"
    );
    assert_eq!(
        shelf(&app).last().map(String::as_str),
        Some("oldest"),
        "a piece that was edited jumped out of its place — the order is definition order, and \
         reshuffling under an author's hand is what `docs/ui.md` §3.5 is against"
    );
}

/// **The vocabulary prompt swallows its keystrokes.**
///
/// While `TokenPrompt::open` is set, the Meshes tab's own key handlers must not act — or typing a
/// name containing `n` would rotate the mesh. `N` is `Action::RotateMeshX` on this tab, so the
/// test types a name with an `n` in it and asserts the piece did not turn and the draft's name
/// field now holds the letter. Without the guard in `editor::not_typing`/`sense_context`, `N` is
/// `RotateMeshX` and typing a name turns the mesh.
#[test]
fn the_token_prompt_swallows_the_keys_that_would_rotate_the_mesh() {
    use emerge_mapper::token_prompt::{Field, TokenPrompt};

    let root = Fixture::new("token-prompt-guard")
        .descriptor("floor", "alpha")
        .build("m");
    let mut app = harness::build_headless_at(&root, "m", None, emerge_mapper::tiles::Mode::Meshes)
        .unwrap_or_else(|e| panic!("the fixture project must open: {e}"));
    for _ in 0..3 {
        app.update();
    }
    let id = app
        .world()
        .resource::<emerge_mapper::project::Project>()
        .library
        .descriptors
        .first()
        .map(|d| d.id.clone())
        .unwrap_or_else(|| panic!("the fixture must carry a descriptor"));
    app.world_mut()
        .resource_mut::<emerge_mapper::tiles::ImportState>()
        .selected_library_id = Some(id.clone());
    for _ in 0..4 {
        app.update();
    }

    let rotate_of = |app: &App| {
        app.world()
            .resource::<emerge_mapper::project::Project>()
            .measured
            .get(&id)
            .and_then(|d| d.align.rotate)
    };
    assert_eq!(rotate_of(&app), None, "the piece starts unturned");

    // Open the prompt the way `Shift+T` does.
    app.world_mut()
        .resource_mut::<TokenPrompt>()
        .open = Some(emerge_mapper::token_prompt::Draft {
        axis: emerge_mapper::token_prompt::Axis::Kind,
        name: String::new(),
        note: String::new(),
        field: Field::Name,
        problem: None,
    });
    for _ in 0..2 {
        app.update();
    }

    // Type `n` into the name field — the letter that is `RotateMeshX` on this tab.
    tap_key(
        &mut app,
        bevy::input::keyboard::Key::Character("n".into()),
        KeyCode::KeyN,
    );
    for _ in 0..2 {
        app.update();
    }

    assert_eq!(
        rotate_of(&app),
        None,
        "typing `n` into the prompt must not rotate the mesh — the guard in \
         `editor::not_typing`/`sense_context` is what makes the context read `Typing`"
    );
    let draft = app.world().resource::<TokenPrompt>().open.clone();
    assert_eq!(
        draft.map(|d| d.name),
        Some("n".to_owned()),
        "and the letter lands in the draft's name field"
    );
}
