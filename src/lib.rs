//! Foundation vs. Slop — library crate root.
//!
//! The SCP Foundation holds the line against "slop" entities — deliberately ugly,
//! uncanny-valley monsters churned out by SCP-9191, a rogue monster-generating AI.
//!
//! This stage is an explorable, WFC-generated dungeon: one Bevy plugin per domain
//! (dungeon, world lighting, camera, fog of war, crab/smiley enemies). The richer "slop"
//! enemy/combat systems are not built yet — they'll be added in a later step.
//!
//! The crate is split lib+bin: all domain modules live here (so integration tests under
//! `tests/` and the headless `sim_harness` can reuse them), and `main.rs` is a thin
//! binary that calls [`run`].

// Bevy's filtered queries produce unavoidably long tuple types; this lint fights
// idiomatic ECS code, so it's disabled crate-wide (the standard Bevy convention).
#![allow(clippy::type_complexity)]

/// Cosmetic pose blending — the shared clip-weight/gait-phase driver every skinned model goes
/// through (squad figurine, crab, manca). Never touches hashed sim state; see its module docs.
/// The pose blender, moved to `crates/emerge-anim` so the editor can drive the same one the game
/// does. Re-exported here because every call site says `crate::anim::…` and none of them needed to
/// know. See that crate's manifest for why it moved.
pub use emerge_anim as anim;
pub mod antagonist;
/// Config-bake machinery (RON splicing + golden re-pinning) shared with the `train` binary.
pub mod bake;
pub mod audio;
/// Data-driven acoustic-stimulus + audio tuning — the `audio:` config slice. The propagation/salience
/// of the acoustic stigmergy channels (`ai::field::NOISE_*`) and the per-faction perception gains that
/// turn sound into a stimulus agents react to; evolvable by the offline audio search (`squad_ai::
/// audio_genome`). Mirrors `ai::tuning` / `sim`.
pub mod audio_tuning;
pub mod autogib;
pub mod behavior_tuning;
pub mod blood_lens;
pub mod broadcast;
pub mod lure;
pub mod ai;
pub mod ai_overlay;
pub mod almond_water;
pub mod camera;
pub mod config;
pub mod containment;
pub mod crab;
/// Dev-only in-process screenshot tool, moved to `crates/bevy_devshot`. The re-export keeps the
/// `#[cfg(debug_assertions)]` gate exactly where it was, so `devshot::DevShotPlugin` does not exist in
/// a release build and the registration at the bottom of `run` cannot compile there. The crate is
/// still *built* in release — a path dependency is unconditional — but it is 32 lines nothing
/// references, dropped at link. That is the honest description; the gate is about this crate's view
/// of the module, not about the code's existence.
#[cfg(debug_assertions)]
pub use bevy_devshot as devshot;
/// Dev-only visual-debug region capture: Ctrl+P → drag a screenspace rectangle → save just that region
/// to `debug_screenshots/` with a snap sound, so a later session can see what the player pointed at.
/// Debug-only, stripped from release like `devshot`.
#[cfg(debug_assertions)]
pub mod region_capture;
/// Dev-only **rig tripwire**: warns, once per rig and by name, when a skinned character's joints come
/// apart — the shape that renders as the stretched spikes a player captured on 2026-07-29 and could
/// only describe as "Wtf? No ideae what this is." Debug-only, stripped from release like `devshot`.
#[cfg(debug_assertions)]
pub mod rig_watch;
pub mod rigs;
/// Dev-only performance overlay (FPS / frame-ms / entity-count / CPU / mem, toggled with F4) plus the
/// frame-time/entity/system-info diagnostics it reads. Debug-only, stripped from release like `devshot`.
#[cfg(debug_assertions)]
pub mod perf_hud;
/// Dev-only **spatial FPS probe** — samples frame time at 2 Hz, tags each sample with the dungeon cell
/// the camera is looking at and the visible scene census there, and writes
/// `debug_screenshots/fps_trace.csv` + `fps_hotspots.md`. `perf_hud` says the frame rate *now*; this
/// says *where* it drops, which is the question a "it's slow in places" report actually asks.
/// Debug-only, stripped from release like `perf_hud`.
#[cfg(debug_assertions)]
pub mod perf_probe;
/// SCP-610 ("the flesh that hates") — quarantine gameplay content, registered in the shipped plugin
/// list AND the headless harness, so this declaration must stay unconditional. Take care inserting
/// near it: a module declared between a `#[cfg]` attribute and its target module steals the gate —
/// exactly that stripped this module from release (with four live references) while silently
/// un-gating the Research Room into the shipped binary.
pub mod scp610;
/// Dev-only **Research Room** (`FVS_RESEARCH_ROOM=1`): boots into the real WFC dungeon — the actual game,
/// with every auto-spawner running natively — and arms an F6 spawn palette on top, so any creature / prop
/// / character can be dropped in, tuned, and screenshotted, and evolved elites witnessed. Debug-only,
/// stripped from release like `devshot`/`region_capture`/`perf_hud`.
#[cfg(debug_assertions)]
pub mod research_room;
/// Dev-only **Site-67 editor** (`FVS_SITE_EDITOR=1`): an F7 palette for authoring the hub's dressing
/// in the live isometric view, with `site::layout`'s placement rules checked per-edit instead of at
/// load, writing back to `site67.ron` without destroying its comments. Debug-only, stripped from
/// release like `research_room`/`devshot`/`region_capture`.
#[cfg(debug_assertions)]
pub mod site_editor;
pub mod dialogue;
pub mod director;
pub mod dungeon;
/// Evolved-elite runtime overlay: `FVS_*_ELITE` env vars install a search elite (behaviour / world / audio
/// / levels config slices, or an RL `NeuralPolicy`) at startup without editing `config.ron`.
pub mod emerge_map;
pub mod elite_overlay;
pub mod enemy;
pub mod flowfield;
pub mod fog;
pub mod gore;
pub mod hair;
pub mod health;
/// The keyboard registry — every binding in one place, with a collision test. See `src/input/`.
pub mod input;
pub mod juice;
pub mod impact_fx;
pub mod laser;
pub mod light;
pub mod mold;
pub mod mycelia;
pub mod nest;
/// Hand-rolled ORCA local avoidance, moved to `crates/bevy_orca` so it can be used without the game.
/// Re-exported here because every call site says `crate::orca::…` and none of them needed to know —
/// the same shim `anim` gets above. See that crate's manifest for why it depends on `bevy_math`
/// rather than `bevy`.
pub use bevy_orca as orca;
pub mod parasite;
pub mod palette;
pub mod pathfind;
pub mod personnel;
pub mod psi_vision;
pub mod placement;
/// SCP-999 — the friendly "Tickle Monster" comfort blob: seeks the most-anxious squad member and tickles
/// away their FEAR (the game's one fear-*lowering* creature). Split gameplay/cosmetic plugins for the
/// determinism gate; see the module docs.
pub mod scp999;
/// SCP-1048 "Builder Bear" — the benign original plus the three hostile copies it assembles from
/// scavenged material. The one creature that *builds more of itself* mid-episode; the original is
/// deliberately unshootable, so the counter is to keep it under observation. Split gameplay/cosmetic
/// plugins for the determinism gate; see the module docs.
pub mod scp1048;
pub mod selection;
/// The Engineer's sensor drone — the only thing that turns the minimap on. See `src/sensor.rs`.
pub mod sensor;
pub mod knowledge;
pub mod persist;
pub mod research;
pub mod session;
pub mod site;
pub mod settings;
/// Data-driven simulation-dynamics tuning (combat, swarm economy, deposits, fear, boss) — the `sim:`
/// config slice. Mirrors `ai::tuning`; together they form the `WorldConfig` the offline search evolves.
pub mod sim;
/// Headless deterministic replay/liveness harness — opt-in so it never enters the shipped binary.
#[cfg(feature = "test-harness")]
pub mod sim_harness;
/// Registers the shared `foundation::noise` WGSL import library (windowed-only).
pub mod shader_lib;
/// Perceptual (SSIM) image comparison for FX/render visual-regression — opt-in test infrastructure.
#[cfg(feature = "test-harness")]
pub mod visual_regression;
pub mod squad;
pub mod squad_ai;
pub mod surface_nav;
pub mod time_control;
pub mod ui;
pub mod util;
pub mod vhs;
pub mod world;

// ── The engine-free core, re-exported at the paths it had before the workspace split ───────────
//
// `crate::rng`, `crate::wfc`, `crate::geom` and `crate::placement::{ir, solver, solvers, scatter,
// manifest}` all still resolve. That is deliberate: a split that also rewrote a thousand import
// lines would be unreviewable, and the point of Stage 0b is a diff a human can check.
pub use emerge_core::{geom, rng, wfc};

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::winit::{UpdateMode, WinitSettings};

/// Gravity for the (gib-only) physics world. Heavier than real 9.81 so chunks fall snappily and
/// settle fast — arcade feel over realism. Only `RigidBody::Dynamic` gib chunks are affected;
/// nothing else in the game is a physics body (see `gore`/`autogib`).
const GIB_GRAVITY: f32 = 18.0;

/// While the dev-only region-capture tool (Ctrl+P, see `region_capture`) owns the mouse, the squad
/// move-order in `selection::command_input` must stand down so a capture drag doesn't also march the
/// squad. Defined here (always compiled) and driven only by the debug-only `RegionCapturePlugin`, so the
/// release binary keeps one code path: the resource is always present and simply stays `false`.
#[derive(Resource, Default)]
pub struct DebugCaptureActive(pub bool);

/// Present only while the dev-only region-capture **note box** (see `region_capture`) is open for text
/// entry. Its presence is the single public signal that (a) freezes the sim through the one existing
/// path — `ui::state::sync_sim_blocked` ORs it into `SimBlocked` — and (b) gates other keyboard systems
/// (`pause::toggle_pause`, `region_capture::drive`) via `run_if(not(resource_exists::<NoteInputActive>))`
/// so keystrokes don't leak while typing. Inserted only by the debug-only note box, so in release it is
/// never present and every reader sees the default (unfrozen, ungated) path.
#[derive(Resource)]
pub struct NoteInputActive;

/// Present only when the dev-only Research Room was requested (`FVS_RESEARCH_ROOM=1`, see
/// [`research_room`]). Its presence arms the F6 debug panel (spawn / pause / quantity) over the real,
/// unmodified game — `DungeonPlugin` still generates the full WFC dungeon, and the auto-spawners, the
/// furniture-placement grammar, and mycelia / mold all run exactly as in a normal launch, so the room is
/// game-faithful. Defined here (always compiled, like `DebugCaptureActive`) so release and the headless
/// harness keep ONE path: the only code that inserts it is `#[cfg(debug_assertions)]`, so it is never
/// present there.
#[derive(Resource)]
pub struct ResearchRoomActive;

/// Present only when the dev-only Site editor was requested (`FVS_SITE_EDITOR=1`, see
/// [`site_editor`]). Its presence arms the F7 dressing palette over the real, unmodified Site — the
/// hub still spawns from `site67.ron` exactly as in a normal launch, and the editor only adds an
/// overlay and the ability to write that file back.
///
/// Defined here (always compiled, like [`ResearchRoomActive`] and [`DebugCaptureActive`]) so release
/// and the headless harness keep ONE path: the only code that inserts it is `#[cfg(debug_assertions)]`,
/// so it is never present there.
#[derive(Resource)]
pub struct SiteEditorActive;

/// **The player's camera** — the one `camera::setup_camera` spawns, and the only one any gameplay or
/// UI system means when it says "the camera".
///
/// Filtering on `With<Camera3d>` is not enough and never was, which cost a day to learn: adding
/// `bevy::gizmos::transform_gizmo::TransformGizmoPlugin` makes `bevy_gizmos_render` spawn its **own**
/// `Camera3d` at `order: 1` to draw the gizmo overlay layer. Every
/// `Single<.., With<Camera3d>>` in the tree then matched two entities and — because `Single` *silently
/// skips its system* rather than erroring — the audio listener, every billboard, `selection`'s
/// click-to-command and `camera::drive_camera` all stopped at once, with no message anywhere. That is
/// what "WASD does nothing and clicks do not land" turned out to be.
///
/// So the filter is **positive**: name the camera you mean. A future plugin may add a third camera;
/// this stays correct.
#[derive(Component)]
pub struct MainCamera;

/// Marks a camera that renders a **thumbnail**, not the player's view.
///
/// The dev-only Site editor bakes a preview of every kit piece by staging it in a "photo booth" far
/// from any real geometry and rendering it to an `Image`. That needs a second `Camera3d`, and a second
/// `Camera3d` is a live hazard in this codebase: **nine** systems take `Single<.., With<Camera3d>>`
/// (the audio listener, health-bar and enemy billboards, gore decals, dialogue bubbles, hair, SCP-999's
/// eyes, and `camera::drive_camera` itself), and `Single` *silently skips its system* when the query
/// does not match exactly one entity. Adding a camera without this marker would stop all nine with no
/// error — the unregistered-system failure mode this repo keeps meeting.
///
/// So every one of those queries excludes it, `vhs.rs`-style. Declared here and always compiled — like
/// [`ResearchRoomActive`] and [`DebugCaptureActive`] — so the filters are one path in every build, and
/// only `#[cfg(debug_assertions)]` code ever spawns a camera carrying it.
#[derive(Component)]
pub struct ThumbnailCamera;

/// Build and run the full windowed game. The headless test harness (`sim_harness`, behind the
/// `test-harness` feature) constructs an equivalent `App` without render/winit/audio so the same
/// gameplay plugins can be driven deterministically off-screen.
pub fn run() {
    let mut app = App::new();
    // Optional RL-policy elite (`FVS_POLICY_ELITE`): install a learned `NeuralPolicy` as the squad
    // `ActivePolicy` BEFORE `SquadAiPlugin`'s `init_resource::<ActivePolicy>()` (a no-op when present) —
    // the same seam `sim_harness` uses. A bad archive fails loudly rather than silently using the default.
    match elite_overlay::load_policy_elite() {
        Ok(Some((policy, line))) => {
            eprintln!("config: overlaid {line}");
            app.insert_resource(squad_ai::policy::ActivePolicy(Box::new(policy)));
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("FVS_POLICY_ELITE: {e}");
            std::process::exit(1);
        }
    }

    // Dev-only Research Room (`FVS_RESEARCH_ROOM=1`): insert the `ResearchRoomActive` marker that arms the
    // F6 spawn palette, alongside the policy-elite pre-install above. The room is game-faithful — the
    // marker only adds a debug overlay; `DungeonPlugin` still generates the real WFC level and every
    // auto-spawner runs natively. Gated on `debug_assertions` like `devshot`/`region_capture`/`perf_hud`,
    // so the shipped binary and the headless harness never see it and keep one execution path.
    #[cfg(debug_assertions)]
    research_room::install_if_requested(&mut app);

    // Dev-only Site editor (`FVS_SITE_EDITOR=1`): insert the `SiteEditorActive` marker that arms the
    // F7 dressing palette at Site-67. Same shape and same gate as the Research Room above — the Site
    // itself is untouched, so what is being edited is the real hub rather than a preview of it.
    #[cfg(debug_assertions)]
    site_editor::install_if_requested(&mut app);

    // Dev-only: load a map authored in `emerge-mapper` (`FVS_EMERGE_MAP=<name>`). Adds nothing at all
    // when the variable is absent — see `emerge_map::install_if_requested`.
    emerge_map::install_if_requested(&mut app);

    app
        // Keep rendering at full rate even when the window is unfocused/occluded, so the game
        // stays live in the background (and the `devshot` in-process screenshots aren't black).
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Foundation vs. Slop".into(),
                // `FVS_WINDOW=WxH` launches windowed at that exact pixel size instead of
                // borderless-fullscreen. **It exists for one job: deciding whether a frame is CPU- or
                // GPU-bound** (FVS-N-25), which is answered by rendering the identical scene at two
                // pixel counts and seeing whether frame time follows. Without a way to change the
                // pixel count, `perf_probe`'s frame times cannot distinguish "too much geometry" from
                // "too much simulation", and the two have opposite fixes.
                //
                // A malformed value is a loud panic, not a silent fall-back to fullscreen: a
                // measurement run that quietly used the wrong resolution would produce a confident
                // wrong answer, which is worse than not running.
                resolution: match std::env::var("FVS_WINDOW") {
                    Ok(spec) => {
                        // Parsed as one expression so the whole malformed case is a SINGLE panic site
                        // (`tests/panic_budget.rs` counts them, and two sites for one typo is not worth
                        // a point of budget).
                        let parsed = spec.split_once(['x', 'X']).and_then(|(w, h)| {
                            Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
                        });
                        let (w, h) = parsed.unwrap_or_else(|| {
                            panic!("FVS_WINDOW must look like 1720x720 (got {spec:?})")
                        });
                        bevy::window::WindowResolution::new(w, h)
                    }
                    Err(_) => default(),
                },
                // **Vsync OFF in measurement mode, and this is load-bearing.** The first attempt at
                // the FVS-N-25 A/B returned "CPU-bound" with 16.75 ms vs 16.82 ms — because BOTH runs
                // sat at exactly 60.0 fps median, i.e. both were vsync-capped and neither was
                // stressed. A capped frame time is a measure of the display, not of the renderer, and
                // comparing two capped runs can only ever report "no difference". Uncapped, the frame
                // time is free to show what the work actually costs.
                present_mode: if std::env::var("FVS_WINDOW").is_ok() {
                    bevy::window::PresentMode::AutoNoVsync
                } else {
                    bevy::window::PresentMode::default()
                },
                mode: if std::env::var("FVS_WINDOW").is_ok() {
                    bevy::window::WindowMode::Windowed
                } else {
                    // Launch borderless-fullscreen on the current monitor (fills the screen at the
                    // desktop resolution, no mode switch). `BorderlessFullscreen` over exclusive
                    // `Fullscreen` so alt-tab / the in-process `devshot` capture stay well-behaved.
                    bevy::window::WindowMode::BorderlessFullscreen(
                        bevy::window::MonitorSelection::Current,
                    )
                },
                ..default()
            }),
            ..default()
        }))
        // Shared `foundation::noise` WGSL library — must load after `DefaultPlugins` (AssetPlugin's
        // EmbeddedAssetRegistry) and before any material shader specializes, so `#import foundation::noise`
        // resolves for the blood/vhs/impact/mycelia shaders (2026-07-19 review Finding E).
        .add_plugins(shader_lib::ShaderLibraryPlugin);

    // **Bevy Remote Protocol, for agent-driven debugging** (`--features debugger`).
    //
    // Two plugins, per the vendored `bevy/examples/remote/server.rs`: `RemotePlugin` owns the method
    // registry and the request queue, `RemoteHttpPlugin` is the JSON-RPC-over-HTTP transport (default
    // port 15702). Our `bevy_debugger_mcp` server is a SEPARATE PROCESS that speaks MCP to an agent and
    // BRP to this one — so nothing here links it, and the game gains no HTTP client.
    //
    // Feature-gated rather than `debug_assertions`-gated, unlike `devshot`/`region_capture`: BRP is not
    // only an observation channel, it can MUTATE a live `World`. It must be absent from a shipped binary
    // and from every determinism run — an external writer into pinned state is exactly what the goldens
    // exist to catch — and a Cargo feature is the only gate that also removes it from the resolved
    // dependency graph rather than merely from the plugin list.
    // **`DebuggerPlugin` owns `RemotePlugin`, so this must not add a second one.** Bevy rejects a
    // duplicate plugin by name, and `bevy_debugger_bevy::DebuggerPlugin::build` adds
    // `RemotePlugin::default().with_method_main(..)` twice over to register `bevy_debugger/screenshot`
    // and `bevy_debugger/input`. Adding `RemotePlugin` here as well panics the moment the feature is on.
    // Only the HTTP transport is ours to add.
    #[cfg(feature = "debugger")]
    app.add_plugins((
        bevy_debugger_bevy::DebuggerPlugin,
        bevy::remote::http::RemoteHttpPlugin::default(),
    ));

    app
        // avian3d rigid-body physics — deliberately scoped: only gib chunks are dynamic bodies and
        // only the floor + walls are static colliders (see `gore`/`autogib`/`dungeon`). Units,
        // enemies, and lasers keep their own custom movement and never touch the solver.
        .add_plugins(PhysicsPlugins::default())
        .insert_resource(Gravity(Vec3::NEG_Y * GIB_GRAVITY))
        // Render-only high-refresh smoothing: `PhysicsPlugins` already brings
        // `bevy_transform_interpolation`'s `TransformInterpolationPlugin` (avian uses it for physics
        // interpolation), so we must NOT add it again — Bevy panics on a duplicate unique plugin. Each
        // mover instead opts in per-entity via the `TransformInterpolation` component at its spawn site
        // (units/enemies/crabs/bolts); without it every entity steps at 60 Hz and judders on a 120/144 Hz
        // panel. The plugin eases `Transform` between fixed ticks but restores the authoritative value in
        // `FixedFirst` *before* each tick, so movers that integrate `transform.translation` don't drift.
        // The exact-hash harness runs physics-off (no `PhysicsPlugins`), so interpolation is absent there
        // and the opt-in components stay inert — `snapshot_hash` reads authoritative transforms.
        // ConfigPlugin must precede every consumer: it loads + validates the unified
        // `assets/config/config.ron` and inserts the `GameConfig` resource in its `build`, which the
        // dungeon/placement/ai/gore/impact_fx/vhs plugins each read at build time to pull their slice.
        // DungeonPlugin in turn precedes FogPlugin: it inserts the `Dungeon` resource in its `build`,
        // which FogPlugin reads at build time to size the fog grid.
        // NOT YET `AutoStartFirstRun(false)`, and the reason is a measured blocker rather than caution.
        //
        // The windowed game is *meant* to open in Site-67 rather than an expedition, and the seam for
        // that exists (`session::AutoStartFirstRun`). But flipping it makes `RunState::Idle` a state the
        // game genuinely SITS IN, and `Dungeon` does not exist then. Measured: boot panics immediately in
        // `selection::command_input` — "Parameter `Res<Dungeon>` failed validation: Resource does not
        // exist". In Bevy 0.19 a missing `Res<T>` panics; it does not skip the system.
        //
        // There are **90 such sites across 20 files**, many on `FixedUpdate` in the pinned core, so
        // gating them is a real audit with golden risk attached — not something to smuggle in alongside
        // the Site's geometry. Tracked as FVS-G-6. Until it lands, boot keeps auto-starting a run and
        // the windowed game behaves exactly as it does today.
        .add_plugins((
            config::ConfigPlugin,
            // `LightFieldPlugin` (the CPU illuminance grid creatures read) is grouped with dungeon+placement
            // it depends on, and kept harness-visible — unlike the windowed `LightingPlugin` below — so the
            // determinism gate covers its bake. Nested here (not a 16th top-level element) to stay under
            // Bevy's 15-plugin tuple cap.
            // `AlmondWaterPlugin` (the CPU water field creatures forage on + the consuming heal) is grouped
            // here too and kept harness-visible, like `LightFieldPlugin` — its field + heal are pinned. The
            // cosmetic puddle `AlmondWaterVisualPlugin` sits with the windowed FX below, never in the harness.
            // `MoldPlugin` (the CPU reaction-diffusion gameplay mold) is grouped here too and kept
            // harness-visible, like `LightFieldPlugin`/`AlmondWaterPlugin` — it is pinned CPU gameplay state
            // (it reads the LightField to recoil and, via its couplings, dims light / boosts almond-water
            // seep). The GPU `MyceliaPlugin` below is the cosmetic mirror and stays windowed-only.
            (
                dungeon::DungeonPlugin,
                placement::PlacementPlugin,
                light::LightFieldPlugin,
                almond_water::AlmondWaterPlugin,
                mold::MoldPlugin,
            ),
            world::WorldPlugin,
            // Owns the single writer of `input::KeyboardOwned` — the guard that stops a keystroke
            // meant for a focused menu button from also firing a gameplay action. Windowed-only:
            // the harness has no menus, so the flag stays false there and every action reads
            // ungated. Grouped with the camera because the camera is its first consumer.
            (input::InputPlugin, camera::CameraPlugin),
            // `PoseBlendPlugin` runs the one apply pass every skinned model's clip weights go through
            // (squad, crab, manca), so it is registered once here rather than by each creature plugin.
            // Cosmetic, but grouped with the squad because that is where the drivers order against it.
            (rigs::RigsPlugin, anim::PoseBlendPlugin, squad::SquadPlugin, squad_ai::SquadAiPlugin),
            selection::SelectionPlugin,
            fog::FogPlugin,
            // `SessionPlugin` (run outcome: win/lose/still-going) is nested with `HealthPlugin` rather
            // than taking its own slot — the top-level tuple is at Bevy's 15-element cap — and the
            // pairing is honest: health is what kills the squad, and the wipe is what the session
            // resolves on. It is registered in the headless harness too (see `sim_harness`), which is
            // the whole point: the terminal states are inside the deterministic core, not the UI.
            (
                health::HealthPlugin,
                session::SessionPlugin,
                containment::ContainmentPlugin,
                site::SitePlugin,
                // The research economy's ECS half: the tech-tree flags and the completion sweep.
                research::ResearchPlugin,
                // Operative beliefs (FVS-O-1b/O-2). Harness-visible: beliefs modulate FEAR, which feeds
                // Think -> movement -> hashed Transform, so the exact-hash gate must cover it.
                knowledge::KnowledgePlugin,
            ),
            (
                ai::AiPlugin,
                enemy::EnemyPlugin,
                crab::CrabPlugin,
                nest::NestPlugin,
                parasite::ParasitePlugin,
                // SCP-999 comfort blob: its tickle-calm mutates squad FEAR/MORALE, which feeds the pinned
                // AI → movement → hashed Transform, so the GAMEPLAY half is harness-visible (registered in
                // `sim_harness` too). The cosmetic half (`Scp999VisualsPlugin`) is windowed-only, below.
                scp999::Scp999Plugin,
                // SCP-1048: the bears carry `Health` and move on `FixedUpdate`, and the original
                // *builds* hostile copies mid-episode — all of it hashed, so the gameplay half is in
                // `sim_harness` too. The cosmetic half (`Scp1048VisualsPlugin`) is windowed-only, below.
                scp1048::Scp1048Plugin,
                // Gameplay half — also registered in `sim_harness`, same split as the bear.
                scp610::Scp610Plugin,
            broadcast::BroadcastPlugin,
            lure::LurePlugin,
            ),
            laser::LaserPlugin,
            impact_fx::ImpactFxPlugin,
            (
                time_control::TimeControlPlugin,
                juice::JuicePlugin,
                gore::GorePlugin,
                autogib::AutogibPlugin,
            ),
            audio::GameAudioPlugin,
            // Cosmetic render/FX. Mycelia (GPU-compute mold ambience) lives here and is registered ONLY
            // here, never in the headless `sim_harness` — which is precisely what keeps it outside the
            // deterministic core. Its `grazing` systems DO steer crabs (hunger + the MEAT field) and run on
            // `FixedUpdate`; the harness never registers this plugin, so they cannot perturb `snapshot_hash`.
            // See the `mycelia` module docs before moving any of it.
            // `LightingPlugin` (real fixture lights + GTAO/contact shadows) sits here because it is
            // cosmetic/GPU and windowed-only — deliberately NOT in `sim_harness`, so the deterministic
            // core never depends on a GPU (the harness keeps the plain `world` ambient+directional). The
            // gameplay `LightField` it will own is registered separately so the harness CAN see it.
            (
                vhs::VhsPlugin,
                blood_lens::BloodLensPlugin,
                mycelia::MyceliaPlugin,
                light::LightingPlugin,
                // The iridescent Almond Water puddle + the mold moisture-feed. Cosmetic/GPU, windowed-only —
                // never in `sim_harness`, so the deterministic core never depends on it.
                almond_water::visual::AlmondWaterVisualPlugin,
                // Physics-reactive accent hair (see `hair` module docs). Cosmetic/`Update`-only, no
                // collider, no perception feed, never touches hashed state — windowed-only alongside
                // `MyceliaPlugin`/`LightingPlugin`/`AlmondWaterVisualPlugin`, never in `sim_harness`.
                hair::HairPlugin,
                // SCP-999's eyes + soft-body jiggle. Cosmetic (writes only MorphWeights + a billboard
                // Transform + material uniforms), windowed-only — never in `sim_harness`. The gameplay
                // `Scp999Plugin` (seek + tickle-calm) is in the harness-visible creature tuple above.
                scp999::Scp999VisualsPlugin,
                // `Scp1048Plugin` (seeding, the behaviour executor, AND the clip driver) is in the
                // harness-visible creature tuple above; this is only the fog hiding. The clip driver
                // moved out of here because the harness wires the bear's blender but would then never
                // drive it — see the note at its registration in `scp1048::Scp1048Plugin`.
                scp1048::Scp1048VisualsPlugin,
                // Cosmetic half: drives the `mutation` morph. Windowed-only — the weight is not
                // hashed state and putting it in the sim would make it part of `snapshot_hash`.
                scp610::Scp610VisualsPlugin,
                // Site-67's presentation: geometry, avatars, the ASYNC door, specimen cells. Windowed
                // ONLY — it spawns ~150 GLB scenes and nothing it creates carries `Health`, so it can
                // never reach `snapshot_hash`. The Site's GAMEPLAY half (`SitePlugin`) is separate and
                // IS harness-visible.
                site::SiteVisualsPlugin,
                // The rest of the hub's presentation, nested to stay under Bevy's 16-element plugin
                // limit — the same reason the UI group below is nested. All four are windowed-only by
                // the same construction as `SiteVisualsPlugin`: they follow a `PlayerAvatar`, which
                // is a `Transform` with no `Health`, and none of them can reach `snapshot_hash`.
                (
                    // Where in the hub the player is standing. Registered out here rather than inside
                    // `SiteVisualsPlugin` because it owns a resource that UI plugins outside `site::`
                    // read.
                    site::SitePresencePlugin,
                    // Near walls squash so the camera can see into the rooms — the treatment the
                    // dungeon has had since long before the hub existed, reshaped for base-origined
                    // meshes.
                    site::SiteCutawayPlugin,
                    // Footfalls that exist, and a room tone per wing.
                    site::SiteAudioPlugin,
                    // The rooms say their names when you walk in, then retire.
                    site::SiteSignagePlugin,
                ),
            ),
            // Windowed game-system UI (HUD, menus, state machine) + world-space dialogue bubbles.
            // Both registered only here, never in the headless harness, so they stay outside the
            // deterministic core (see `ui` docs). Dialogue needs `MenuState` (from `UiPlugin`) for the
            // sim-freeze during a modal exchange; it is cosmetic/`Update`, never `FixedUpdate`.
            //
            // `PsiVisionPlugin` (the Psionic's diegetic field-sight — a mechanic) and `AiOverlayPlugin`
            // (the F3 squad-AI state label — a dev tool) sit in this group because both read the bubble
            // assets `DialoguePlugin` sets up, and both are cosmetic `Update` systems that the harness
            // never registers. Grouped in a nested tuple to stay under Bevy's 16-element plugin limit.
            (
                ui::UiPlugin,
                // Save/load is windowed-only, and a headless rollout must never touch the player's
                // campaign file. **The safety is this registration, and now only this.** It used to
                // have a second, redundant guard — `save_campaign` fired on `OnEnter(AppState::Site)`,
                // a state the harness never registers — but the save moved to `OnExit(RunState::Active)`
                // so that visiting the Site mid-run cannot snapshot a live expedition
                // (`docs/2026-08-01-two-live-layers.md`). Every rollout ends a run, so adding this
                // plugin to `sim_harness` would now overwrite the campaign on every episode.
                persist::PersistPlugin,
                // The research BENCH (FVS-E-5) — the verb that actually moves a posterior. Windowed
                // for exactly the same reason as save/load above: it is gated on `AppState::Site`,
                // which the harness never registers, so research cannot reach the pinned core.
                research::ResearchLabPlugin,
                // The O5 review + requisition (FVS-P-3). Windowed for the same reason: the review
                // fires on `AppState::Debrief` and the shop lives at `AppState::Site`, neither of
                // which the harness registers.
                site::O5Plugin,
                // Activities: the Paratherapist's two verbs, and the strain they spend. Windowed-only
                // like the rest of the hub's UI; it writes only the persisted `SquadKnowledge`.
                site::ActivitiesPlugin,
                // FVS-L-5's roster screen plus the cross-run belief carry (FVS-G-3). Windowed:
                // the screen is UI, and the carry writes `Knowledge` only at world construction.
                knowledge::RosterPlugin,
                // The records office (FVS-O-4): the cross-run knowledge channel, and the shelf
                // FVS-O-5's planted report will sit on. Windowed except its briefing, which is
                // world construction.
                knowledge::RecordsPlugin,
                // SCP-9191 (FVS-K-4). AFTER RecordsPlugin so the shelf exists to be written to, and
                // windowed-only for the same reason the records office is: the endgame is an argument
                // conducted at Site-67, not in the field.
                antagonist::AntagonistPlugin,
                // FVS-H-3: samples the levels archive for the next expedition's world. Windowed-only,
                // so `OnEnter(RunState::Active)` keeps exactly the nodes the deterministic core has
                // always had — the director cannot move a golden.
                director::DirectorPlugin,
                // FVS-L-4: renders what the director chose. Without it, adaptive difficulty is
                // indistinguishable from randomness — a director the player cannot perceive is one
                // that gets blamed for bad luck.
                ui::briefing::BriefingPlugin,
                dialogue::DialoguePlugin,
                psi_vision::PsiVisionPlugin,
                // The extraction point's column of light, and the sensor drone that turns the
                // minimap on. Windowed-only for the same reason `psi_vision` is: both are
                // presentation, and the harness must not spawn either.
                (containment::extraction::ExtractionBeaconPlugin, sensor::SensorPlugin),
                ai_overlay::AiOverlayPlugin,
            ),
        ));

    // Pinned simulation runs on `FixedUpdate` at a fixed 60 Hz, so gameplay advances at the same rate
    // regardless of render frame rate (movement is dt-scaled, so real-time speed is unchanged — the sim
    // just steps deterministically). Cosmetic/FX/input systems stay on `Update`. See the per-plugin
    // `FixedUpdate` registrations (ai, squad, enemy, crab, nest, laser).
    app.insert_resource(Time::<bevy::time::Fixed>::from_hz(60.0));

    // Spiral-of-death guard: cap how far the virtual clock may advance in one rendered frame, so a single
    // slow frame can't ask `FixedUpdate` to run a runaway burst of sub-steps — each of which re-runs the
    // full field simulation (stigmergy / mold / light / almond-water). Bevy's default `max_delta` is 250 ms
    // (~15 sub-steps at 60 Hz); 100 ms (~6) keeps one hitch from cascading. Under sustained overload the sim
    // gently loses real-time sync instead of spiralling. Windowed-only: the headless harness drives time via
    // `TimeUpdateStrategy::ManualDuration`, so this never touches the deterministic goldens.
    app.insert_resource(Time::<bevy::time::Virtual>::from_max_delta(
        std::time::Duration::from_millis(100),
    ));

    // devshot is a dev-only in-process screenshot tool — strip it (and its `mod`) from release builds
    // (see CLAUDE.md). Gating both the registration and `mod devshot;` on `debug_assertions` keeps the
    // release binary free of the module and its per-frame `screenshot.request` sentinel polling.
    #[cfg(debug_assertions)]
    app.add_plugins(devshot::DevShotPlugin);

    // `DebugCaptureActive` is initialised by the plugins whose systems READ it (`SelectionPlugin`,
    // `UiPlugin`) rather than here, so the guarantee travels with the reader instead of depending on this
    // one call site — a bare `App` that adds `UiPlugin` (the UI-liveness test) used to panic on the
    // missing resource. Only the debug-only `RegionCapturePlugin` below ever flips it true.

    // Dev-only Ctrl+P region capture (screenspace rectangle → cropped PNG + snap). Debug-only, on `Update`,
    // never in the headless harness, so it stays out of the deterministic core and the shipped binary.
    #[cfg(debug_assertions)]
    app.add_plugins(region_capture::RegionCapturePlugin);

    // Dev-only perf overlay (F4) + diagnostics. Debug-only, never in the headless harness, so it stays
    // out of the deterministic core and the shipped binary (see `perf_hud`).
    #[cfg(debug_assertions)]
    app.add_plugins(perf_hud::PerfHudPlugin);

    // Dev-only spatial FPS probe — the "radar for frame drops". Same gating and the same reason: it
    // measures and writes files, touching no simulation state.
    #[cfg(debug_assertions)]
    app.add_plugins(perf_probe::PerfProbePlugin);

    // Dev-only skeleton tripwire. Reads joint transforms and logs; writes nothing, so it cannot reach
    // the deterministic core. See `rig_watch` for why it watches joints rather than the mesh.
    #[cfg(debug_assertions)]
    app.add_plugins(rig_watch::RigWatchPlugin);

    // Dev-only Research Room editor/observation systems (`FVS_RESEARCH_ROOM=1`). Debug-only, all on
    // `Update`, never in the headless harness — outside the deterministic core and the shipped binary.
    #[cfg(debug_assertions)]
    app.add_plugins(research_room::ResearchRoomPlugin);

    // Dev-only Site-67 editor (`FVS_SITE_EDITOR=1` + F7). Same contract as the Research Room above:
    // debug-only, every system on `Update` and gated on `SiteEditorActive`, never in the headless
    // harness. It edits `site67.ron`, which `site::layout` keeps out of the offline search on purpose,
    // so the tool is outside the search by the same argument — see the module header.
    #[cfg(debug_assertions)]
    app.add_plugins(site_editor::SiteEditorPlugin);

    // The gestation "twitching lump" tell — WINDOWED-ONLY cosmetic (spawns child meshes on infested hosts),
    // so the headless deterministic core spawns nothing and its goldens are untouched. See
    // `parasite::drive_infestation_tell`.
    app.add_systems(Update, parasite::drive_infestation_tell);

    app.run();
}
