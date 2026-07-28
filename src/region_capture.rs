//! Dev-only **visual-debug region capture**. While the game runs, press **Ctrl+P** to arm a screenspace
//! region-select mode, drag a rectangle over whatever you want to point at, and release to save *just that
//! region* to `debug_screenshots/region_<timestamp>.png` — with a short "snap" confirming the capture. A
//! committed `debug_screenshots/README.md` tells a later Claude Code session what the folder holds, so the
//! next agent can literally see what the player was highlighting.
//!
//! The drawn rectangle is a human-supplied **region-of-interest / attention crop** steering a future
//! agent's attention — the "resizable rectangular viewfield" attention mechanism of Wintermute, Xu & Laird
//! (2007, "SORTS: A Human-Level Approach to Real-Time Strategy AI") and the hard-attention "glimpse"; the
//! capture + instructions are the human-in-the-loop steering signal of Mosqueira-Rey et al. (2022,
//! "Human-in-the-loop machine learning: a state of the art").
//!
//! Dev-only: the whole module and its plugin are gated on `debug_assertions` (see `lib.rs`) and stripped
//! from release, exactly like `devshot`. Everything runs on `Update` — no pinned/`FixedUpdate` state — so
//! it can never perturb the deterministic core, and it is never added to the headless harness.
//!
//! Caveat: `audio::mute_when_background` forces `GlobalVolume` to 0 when the window is unfocused/headless,
//! so the snap is audible only when the window is focused at capture time — which is exactly the
//! interactive case here (the headless `devshot` path stays silent, as intended).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};

use crate::audio::Sfx;
use crate::camera::CameraView;
use crate::crab::Crab;
use crate::dungeon::Dungeon;
use crate::enemy::Enemy;
use crate::nest::Nest;
use crate::parasite::Manca;
use crate::placement::PlacedIn;
use crate::squad::Unit;
use crate::ui::state::{AppState, MenuState};
use crate::ui::theme::{FontAssets, UiTheme, Z_MENU};
use crate::ui::widgets::{panel, text, text_colored};
use crate::{DebugCaptureActive, NoteInputActive};

/// Where captures land (project root, sibling of `devshot`'s `screenshot.png`). PNGs are gitignored; the
/// committed `README.md` inside it is the instructions for future sessions.
const OUT_DIR: &str = "debug_screenshots";

/// Drags smaller than this (logical px, either axis) are treated as an accidental click, not a capture.
const MIN_DRAG_PX: f32 = 8.0;

/// Marquee border thickness (logical px).
const MARQUEE_BORDER: f32 = 2.0;

/// Cyan (#00E5FF) marquee border — high contrast against the game's earthy palette, unmistakably a tool.
fn marquee_border() -> Color {
    Color::srgb(0.0, 0.898, 1.0)
}
/// Faint translucent cyan fill, so the boxed region is tinted but still readable underneath.
fn marquee_fill() -> Color {
    Color::srgba(0.0, 0.9, 1.0, 0.12)
}

pub struct RegionCapturePlugin;

impl Plugin for RegionCapturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RegionCapture>()
            .add_systems(
                Update,
                // Ordered: read input & advance the state machine, sync the on-screen marquee to match,
                // then (a frame later) fire the deferred capture. Chaining keeps the phase read consistent
                // per frame. `drive` stands down while the note box owns the keyboard, so Ctrl+P/Escape
                // there edit the note instead of re-arming/cancelling a capture.
                (
                    drive.run_if(not(resource_exists::<NoteInputActive>)),
                    sync_marquee,
                    run_pending_capture,
                )
                    .chain(),
            )
            // The note box only lives while a `NoteDraft` exists (opened by `run_pending_capture`).
            .add_systems(Update, note_input.run_if(resource_exists::<NoteDraft>))
            // While the note box owns the keyboard, swallow ALL gameplay input at the source so no shortcut
            // (camera pan/rotate/zoom, speed digits, overlay toggles, …) fires while the player types. Runs
            // in `PreUpdate` right after Bevy populates the input resources, so every `Update` reader sees
            // them empty. `note_input` reads raw `KeyboardInput` *events* (not `ButtonInput`), so text entry
            // is unaffected. This is the one place that guarantees coverage — no per-system gate to forget.
            .add_systems(
                PreUpdate,
                swallow_input_during_note
                    .after(InputSystems)
                    .run_if(resource_exists::<NoteInputActive>),
            )
            // One-line boot confirmation: if this line is absent from the log, the tool wasn't compiled in
            // (a release build strips the whole `debug_assertions`-only module), which is the first thing to
            // rule out when Ctrl/Super+P appears to do nothing.
            .add_systems(Startup, || {
                info!("region_capture: ready — Ctrl+P (or Super+P) arms a debug region capture");
            });
    }
}

/// The capture state machine. `Dragging` remembers where the drag began; `Pending` carries the finalized
/// rectangle (logical px) plus the window scale factor, and a one-frame `settle` so the marquee is despawned
/// and a *clean* frame has rendered before we grab it (otherwise the cyan box bakes into the crop).
#[derive(Default, Clone, Copy)]
enum Phase {
    #[default]
    Idle,
    Armed,
    Dragging {
        start: Vec2,
    },
    Pending {
        min: Vec2,
        max: Vec2,
        scale: f32,
        settle: u8,
    },
}

#[derive(Resource, Default)]
struct RegionCapture {
    phase: Phase,
}

/// Marker for the marquee UI rectangle node (one at a time; despawned whenever we leave `Dragging`).
#[derive(Component)]
struct Marquee;

/// Read the keyboard/mouse and advance the state machine. Plays the snap on release (immediately, per
/// Kaaresoja 2015's 20–70 ms feedback window) and defers the actual capture to `run_pending_capture`.
fn drive(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    mut state: ResMut<RegionCapture>,
    mut active: ResMut<DebugCaptureActive>,
    mut sfx: MessageWriter<Sfx>,
    mut commands: Commands,
) {
    let (win_entity, win) = *window;
    // Accept Ctrl OR Super/Meta as the modifier: a Ctrl<->Meta remap (or a Cmd-key habit) still arms it.
    let modifier = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight);
    let toggle = modifier && keys.just_pressed(KeyCode::KeyP);
    let cancel = keys.just_pressed(KeyCode::Escape);
    let cursor = win.cursor_position(); // logical px, `None` when off-window

    match state.phase {
        Phase::Idle => {
            if toggle {
                state.phase = Phase::Armed;
                set_cursor(&mut commands, win_entity, SystemCursorIcon::Crosshair);
                info!("region_capture: armed — drag a box, release to capture (Esc or Ctrl/Super+P to cancel)");
            } else if keys.just_pressed(KeyCode::KeyP) {
                // 'P' reached the app but no Ctrl/Super was held — surface it, since a Ctrl<->Meta remap
                // (this box's keys were reported flipped) is the usual cause. No log at all ⇒ either a
                // release build (this whole module is `debug_assertions`-only) or the window isn't focused.
                info!(
                    "region_capture: 'P' pressed with no Ctrl/Super modifier registered — hold Ctrl or \
                     Super (Meta) and press P to arm"
                );
            }
        }
        Phase::Armed => {
            if toggle || cancel {
                state.phase = Phase::Idle;
                set_cursor(&mut commands, win_entity, SystemCursorIcon::Move);
            } else if mouse.just_pressed(MouseButton::Left) {
                if let Some(p) = cursor {
                    state.phase = Phase::Dragging { start: p };
                }
            }
        }
        Phase::Dragging { start } => {
            if cancel {
                // Bail the drag but stay in capture mode so the player can immediately try again.
                state.phase = Phase::Armed;
            } else if mouse.just_released(MouseButton::Left) {
                let end = cursor.unwrap_or(start);
                let (min, max) = (start.min(end), start.max(end));
                if (max - min).cmpge(Vec2::splat(MIN_DRAG_PX)).all() {
                    // Valid box: confirm the gesture NOW (0 ms), capture a clean frame shortly after.
                    sfx.write(Sfx::Screenshot);
                    state.phase = Phase::Pending {
                        min,
                        max,
                        scale: win.scale_factor(),
                        settle: 1,
                    };
                    set_cursor(&mut commands, win_entity, SystemCursorIcon::Move);
                } else {
                    // Too small — a mis-click, not a capture. Stay armed.
                    state.phase = Phase::Armed;
                }
            }
        }
        Phase::Pending { .. } => { /* handed off to `run_pending_capture` */ }
    }

    // Own the mouse for every non-idle phase so `selection::command_input` stands down (no stray move order).
    active.0 = !matches!(state.phase, Phase::Idle);
}

/// Keep the marquee node matching the live drag; despawn it the instant we leave `Dragging` (including the
/// release → `Pending` transition, so the box is gone before the capture frame renders).
fn sync_marquee(
    state: Res<RegionCapture>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut commands: Commands,
    existing: Query<Entity, With<Marquee>>,
    mut nodes: Query<&mut Node, With<Marquee>>,
) {
    match state.phase {
        Phase::Dragging { start } => {
            let cur = window.cursor_position().unwrap_or(start);
            let min = start.min(cur);
            let size = (start.max(cur) - min).max(Vec2::ZERO);
            if let Ok(mut node) = nodes.single_mut() {
                node.left = Val::Px(min.x);
                node.top = Val::Px(min.y);
                node.width = Val::Px(size.x);
                node.height = Val::Px(size.y);
            } else {
                spawn_marquee(&mut commands, min, size);
            }
        }
        _ => {
            for e in &existing {
                commands.entity(e).despawn();
            }
        }
    }
}

/// One frame after release (marquee gone, clean frame rendered): capture the full window and crop to the
/// selected region on the CPU (Bevy 0.19's `Screenshot` only grabs the whole window, so the sub-region is a
/// `crop_imm`), snapshot the scene metadata for the sidecar, and open the note box for the player's
/// description. The metadata is gathered *now* so it stays consistent with the PNG even though the player
/// may type for a while (and the sim is frozen while they do — see `NoteInputActive`).
///
/// Region focusing done by hand: the player pre-crops the bug-relevant region, cutting the "large expanses
/// of uninformative, bug-irrelevant regions" that hurt multimodal localization (Xiao et al. 2026,
/// "VisualRepair"; Ma et al. 2026, "FailureMem"). The structured entity/position block preserves the spatial
/// relationships a plain-text screenshot would discard (Liu et al. 2026, "GALA"), and pairing the image crop
/// with the textual note follows Saha et al. (2024) — visual localizes screens, text localizes components.
#[allow(clippy::too_many_arguments)]
fn run_pending_capture(
    mut state: ResMut<RegionCapture>,
    mut commands: Commands,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    dungeon: Option<Res<Dungeon>>,
    cam_view: Option<Res<CameraView>>,
    app_state: Res<State<AppState>>,
    menu: Option<Res<State<MenuState>>>,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    ents: Query<(
        Entity,
        &GlobalTransform,
        Option<&Name>,
        Has<Unit>,
        Has<Enemy>,
        Has<Crab>,
        Has<Manca>,
        Has<Nest>,
        Has<PlacedIn>,
    )>,
) {
    let Phase::Pending {
        min,
        max,
        scale,
        settle,
    } = state.phase
    else {
        return;
    };
    if settle > 0 {
        // Let the marquee despawn + a clean frame render before we grab the framebuffer.
        state.phase = Phase::Pending {
            min,
            max,
            scale,
            settle: settle - 1,
        };
        return;
    }
    state.phase = Phase::Idle;

    // Logical px → physical px (the swapchain image is physical). Clamp to image bounds inside the observer.
    let scale = scale.max(f32::EPSILON);
    let px = (min.x * scale).floor().max(0.0) as u32;
    let py = (min.y * scale).floor().max(0.0) as u32;
    let pw = ((max.x - min.x) * scale).ceil().max(1.0) as u32;
    let ph = ((max.y - min.y) * scale).ceil().max(1.0) as u32;

    let stem = match capture_stem() {
        Ok(s) => s,
        Err(e) => {
            error!("region_capture: {e}");
            return;
        }
    };
    if let Err(e) = std::fs::create_dir_all(OUT_DIR) {
        error!("region_capture: could not create {OUT_DIR}/: {e}");
        return;
    }
    let png_path = Path::new(OUT_DIR).join(format!("{stem}.png"));
    let md_path = Path::new(OUT_DIR).join(format!("{stem}.md"));

    // The observer runs once when this window's screenshot is ready; it owns `png_path` + the crop rect.
    commands
        .spawn(Screenshot::primary_window())
        .observe(move |captured: On<ScreenshotCaptured>| {
            let img = captured.image.clone();
            match img.try_into_dynamic() {
                Ok(dynimg) => {
                    let (iw, ih) = (dynimg.width(), dynimg.height());
                    // Clamp so a drag that ran off the window edge still yields an in-bounds crop.
                    let x = px.min(iw.saturating_sub(1));
                    let y = py.min(ih.saturating_sub(1));
                    let w = pw.min(iw - x).max(1);
                    let h = ph.min(ih - y).max(1);
                    // `to_rgb8` drops the alpha channel, which carries brightness under HDR (mirrors
                    // Bevy's own `save_to_disk`), so the PNG looks right.
                    let cropped = dynimg.crop_imm(x, y, w, h).to_rgb8();
                    match cropped.save(&png_path) {
                        Ok(()) => info!("region_capture: wrote {} ({w}x{h}px)", png_path.display()),
                        Err(e) => error!("region_capture: cannot save {}: {e}", png_path.display()),
                    }
                }
                Err(e) => error!("region_capture: screen format not understood: {e}"),
            }
        });

    // Snapshot the scene metadata now (consistent with the just-captured frame) and open the note box.
    let (camera, cam_tf) = *camera;
    let metadata_md = build_metadata(
        &stem,
        (min, max),
        (px, py, pw, ph),
        &window,
        scale,
        camera,
        cam_tf,
        dungeon.as_deref(),
        cam_view.as_deref(),
        &app_state,
        menu.as_deref(),
        &ents,
    );

    spawn_note_box(&mut commands, &theme, &fonts);
    commands.insert_resource(NoteInputActive);
    commands.insert_resource(NoteDraft {
        stem,
        md_path,
        metadata_md,
        note: String::new(),
    });
}

/// `region_YYYY-MM-DD_HH-MM-SS-mmm` (no extension) from the wall clock. Milliseconds keep rapid captures
/// unique; dashes (not colons) keep it valid on every filesystem. The `.png` and `.md` sidecar share this
/// stem. Shares the calendar math with the bake ledger via [`crate::util::civil_from_days`] — no date crate.
fn capture_stem() -> Result<String, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system clock is before the unix epoch: {e}"))?;
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, m, d) = crate::util::civil_from_days(days);
    Ok(format!(
        "region_{y:04}-{m:02}-{d:02}_{hh:02}-{mm:02}-{ss:02}-{millis:03}"
    ))
}

fn set_cursor(commands: &mut Commands, window: Entity, icon: SystemCursorIcon) {
    commands.entity(window).insert(CursorIcon::from(icon));
}

/// Spawn the cyan marquee overlay (absolute-positioned UI node, above the HUD, click-through).
fn spawn_marquee(commands: &mut Commands, min: Vec2, size: Vec2) {
    let border = marquee_border();
    commands.spawn((
        Marquee,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(min.x),
            top: Val::Px(min.y),
            width: Val::Px(size.x),
            height: Val::Px(size.y),
            border: UiRect::all(Val::Px(MARQUEE_BORDER)),
            ..default()
        },
        BorderColor {
            top: border,
            right: border,
            bottom: border,
            left: border,
        },
        BackgroundColor(marquee_fill()),
        GlobalZIndex(2000), // above perf_hud (1000), HUD (10), menus (100)
        Pickable::IGNORE,
    ));
}

// ---------------------------------------------------------------------------------------------------------
// Note box + metadata sidecar
// ---------------------------------------------------------------------------------------------------------

/// The open note box's data: where to write, the pre-built metadata block (snapshotted at capture time),
/// and the note the player is typing. Present exactly while the box is open (mirrors [`NoteInputActive`]).
#[derive(Resource)]
struct NoteDraft {
    /// `region_<timestamp>` — shared by the `.png` and this `.md`.
    stem: String,
    md_path: PathBuf,
    /// The `## Capture` / `## Scene` / `## Entities` sections, ready to write under the note.
    metadata_md: String,
    note: String,
}

/// Root of the note-box modal (despawned on submit).
#[derive(Component)]
struct NoteBoxRoot;

/// The live text line inside the note box, rewritten each frame with the current buffer + cursor.
#[derive(Component)]
struct NoteBoxText;

/// Type + source-asset label for a tracked gameplay entity, or `None` for everything else (child meshes,
/// UI, gizmos) so the "entities in region" list stays signal, not noise. The asset strings mirror the
/// module-private `*_GLB` path constants in `squad`/`crab`/`parasite` (kept private there; duplicated here
/// so this dev tool doesn't widen their visibility).
fn classify(
    unit: bool,
    enemy: bool,
    crab: bool,
    manca: bool,
    nest: bool,
    prop: bool,
) -> Option<(&'static str, &'static str)> {
    if enemy {
        Some(("smiley-boss", "procedural mesh (enemy.rs)"))
    } else if manca {
        Some(("manca/parasite", "scp150/scp-150.glb"))
    } else if crab {
        Some(("dimensional-crab", "dimensional_crab/dimensional_crab.glb"))
    } else if nest {
        Some(("nest", "procedural mesh (nest.rs)"))
    } else if unit {
        Some(("squad-unit", "characters/valkyrie.glb"))
    } else if prop {
        Some(("prop/furniture", "glTF scene (glb path not on entity)"))
    } else {
        None
    }
}

/// Ground-plane (`y=0`) world point under a screen pixel — the same ray-cast `selection::cursor_ground_point`
/// does, but for an arbitrary pixel (here the selection centre).
fn ground_under(camera: &Camera, cam_tf: &GlobalTransform, pixel: Vec2) -> Option<Vec3> {
    let ray = camera.viewport_to_world(cam_tf, pixel).ok()?;
    let dist = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
    Some(ray.get_point(dist))
}

/// Cap on the per-region entity list so a box over a swarm can't produce a wall of text.
const MAX_ENTITIES_LISTED: usize = 40;

/// Build the `## Capture` / `## Scene` / `## Entities in region` markdown (everything but the player's note),
/// snapshotting the scene as it was in the captured frame.
#[allow(clippy::too_many_arguments)]
fn build_metadata(
    stem: &str,
    (min, max): (Vec2, Vec2),
    (px, py, pw, ph): (u32, u32, u32, u32),
    window: &Window,
    scale: f32,
    camera: &Camera,
    cam_tf: &GlobalTransform,
    dungeon: Option<&Dungeon>,
    cam_view: Option<&CameraView>,
    app_state: &State<AppState>,
    menu: Option<&State<MenuState>>,
    ents: &Query<(
        Entity,
        &GlobalTransform,
        Option<&Name>,
        Has<Unit>,
        Has<Enemy>,
        Has<Crab>,
        Has<Manca>,
        Has<Nest>,
        Has<PlacedIn>,
    )>,
) -> String {
    let campos = cam_tf.translation();

    // One pass: tally scene-wide counts and collect the tracked entities that project inside the box.
    let mut counts = [0usize; 5]; // unit, enemy, crab, manca, nest
    let mut inbox: Vec<(f32, Entity, &'static str, String, Vec3, Vec2, &'static str)> = Vec::new();
    for (ent, gt, name, unit, enemy, crab, manca, nest, prop) in ents {
        counts[0] += unit as usize;
        counts[1] += enemy as usize;
        counts[2] += crab as usize;
        counts[3] += manca as usize;
        counts[4] += nest as usize;

        let Some((kind, asset)) = classify(unit, enemy, crab, manca, nest, prop) else {
            continue;
        };
        let world = gt.translation();
        let Ok(screen) = camera.world_to_viewport(cam_tf, world) else {
            continue; // behind the camera or otherwise unprojectable
        };
        if screen.x < min.x || screen.x > max.x || screen.y < min.y || screen.y > max.y {
            continue;
        }
        let name = name.map(|n| format!(" \"{}\"", n.as_str())).unwrap_or_default();
        inbox.push((campos.distance(world), ent, kind, name, world, screen, asset));
    }
    // Nearest first; entity id breaks ties into a total order.
    // SORT-OK: dev-only debug-report ordering, never touches pinned sim state.
    inbox.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let total_in = inbox.len();
    inbox.truncate(MAX_ENTITIES_LISTED);

    let center = (min + max) * 0.5;
    let ground = ground_under(camera, cam_tf, center);
    let menu_str = menu
        .map(|m| format!("{:?}", m.get()))
        .unwrap_or_else(|| "(not in-game)".to_string());

    let mut md = String::new();
    let _ = writeln!(md, "## Capture\n");
    let _ = writeln!(md, "- Timestamp: `{stem}`");
    let _ = writeln!(
        md,
        "- Selection (logical px): ({:.0}, {:.0}) – ({:.0}, {:.0})  ·  {:.0}×{:.0}",
        min.x,
        min.y,
        max.x,
        max.y,
        max.x - min.x,
        max.y - min.y
    );
    let _ = writeln!(md, "- Selection (physical px): x={px} y={py} w={pw} h={ph}");
    let _ = writeln!(
        md,
        "- Window: {}×{} physical px  ·  scale {scale:.2}",
        window.resolution.physical_width(),
        window.resolution.physical_height()
    );
    let _ = writeln!(md, "\n## Scene\n");
    let _ = writeln!(md, "- App state: {:?}", app_state.get());
    let _ = writeln!(md, "- Menu/overlay: {menu_str}");
    let _ = writeln!(md, "- Sim: frozen while this note was written");
    let _ = writeln!(
        md,
        "- Camera position: ({:.2}, {:.2}, {:.2})",
        campos.x, campos.y, campos.z
    );
    if let Some(cv) = cam_view {
        let _ = writeln!(
            md,
            "- Camera viewport height (zoom, world units): {:.2}",
            cv.viewport_height
        );
    }
    match ground {
        Some(p) => {
            let cell = dungeon
                .map(|d| {
                    let c = d.world_to_cell(p);
                    format!("  ·  dungeon cell ({}, {}), floor={}", c.x, c.y, d.is_floor(c))
                })
                .unwrap_or_default();
            let _ = writeln!(
                md,
                "- Ground point under selection centre: ({:.2}, {:.2}, {:.2}){cell}",
                p.x, p.y, p.z
            );
        }
        None => {
            let _ = writeln!(md, "- Ground point under selection centre: (no ground-plane hit)");
        }
    }
    let _ = writeln!(
        md,
        "- Scene entity counts: units={}, enemies={}, crabs={}, mancae={}, nests={}",
        counts[0], counts[1], counts[2], counts[3], counts[4]
    );

    let _ = writeln!(md, "\n## Entities in region ({total_in})\n");
    if inbox.is_empty() {
        let _ = writeln!(md, "_(no tracked gameplay entity projects inside the box)_");
    } else {
        for (dist, _idx, kind, name, world, screen, asset) in &inbox {
            let _ = writeln!(
                md,
                "- **{kind}**{name} · asset `{asset}` · world=({:.2}, {:.2}, {:.2}) · screen=({:.0}, {:.0}) · dist={dist:.1}",
                world.x, world.y, world.z, screen.x, screen.y
            );
        }
        if total_in > MAX_ENTITIES_LISTED {
            let _ = writeln!(md, "- … and {} more (list capped)", total_in - MAX_ENTITIES_LISTED);
        }
    }
    md
}

/// Spawn the note-box modal: a bottom-centre panel with a prompt, the live note line, and the key hints.
fn spawn_note_box(commands: &mut Commands, theme: &UiTheme, fonts: &FontAssets) {
    commands
        .spawn((
            NoteBoxRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexEnd,
                padding: UiRect::bottom(Val::Px(64.0)),
                ..default()
            },
            GlobalZIndex(Z_MENU),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn(panel(
                theme,
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(theme.space_md)),
                    row_gap: Val::Px(theme.space_sm),
                    min_width: Val::Px(560.0),
                    max_width: Val::Px(900.0),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
            ))
            .with_children(|p| {
                p.spawn(text_colored(
                    theme,
                    fonts,
                    "\u{25a3} region captured \u{2014} describe the issue",
                    theme.font_body,
                    theme.accent,
                ));
                // Starts with just the cursor; `note_input` rewrites this each frame.
                p.spawn((NoteBoxText, text(theme, fonts, "\u{2588}", theme.font_body)));
                p.spawn(text_colored(
                    theme,
                    fonts,
                    "[Enter] save   \u{00b7}   [Esc] save without note   \u{00b7}   [Backspace] delete",
                    theme.font_body * 0.8,
                    theme.text_muted,
                ));
            });
        });
}

/// Drive the note box: accumulate typed characters, and on Enter or Escape write the `.md` sidecar (note +
/// metadata) and close. Both keys save — the note is optional (Esc/empty still keeps the PNG + metadata).
fn note_input(
    mut key_events: MessageReader<KeyboardInput>,
    mut draft: ResMut<NoteDraft>,
    mut commands: Commands,
    roots: Query<Entity, With<NoteBoxRoot>>,
    mut line: Query<&mut Text, With<NoteBoxText>>,
) {
    let mut finalize = false;
    for ev in key_events.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        match &ev.logical_key {
            Key::Character(s) => {
                for c in s.chars().filter(|c| !c.is_control()) {
                    draft.note.push(c);
                }
            }
            Key::Space => draft.note.push(' '),
            Key::Backspace => {
                draft.note.pop();
            }
            Key::Enter | Key::Escape => finalize = true,
            _ => {}
        }
    }

    if finalize {
        write_note_sidecar(&draft);
        for e in &roots {
            commands.entity(e).despawn();
        }
        commands.remove_resource::<NoteDraft>();
        commands.remove_resource::<NoteInputActive>(); // unfreezes the sim next frame
        return;
    }

    if let Ok(mut t) = line.single_mut() {
        t.0 = format!("{}\u{2588}", draft.note);
    }
}

/// Swallow every gameplay input while the note box is open, so typing a note can't drive the camera, change
/// game speed, toggle overlays, etc. Clears the derived input resources (keyboard/mouse buttons + the
/// accumulated scroll/motion) after Bevy fills them and before any `Update` reader runs. `note_input` reads
/// raw `KeyboardInput` events, which this does not touch, so text entry keeps working.
fn swallow_input_during_note(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut scroll: ResMut<AccumulatedMouseScroll>,
    mut motion: ResMut<AccumulatedMouseMotion>,
) {
    keys.reset_all();
    mouse.reset_all();
    scroll.delta = Vec2::ZERO;
    motion.delta = Vec2::ZERO;
}

/// Write the `.md` sidecar: the player's note (or a placeholder) above the snapshotted metadata block.
fn write_note_sidecar(draft: &NoteDraft) {
    let mut s = String::new();
    let _ = writeln!(s, "# {}\n", draft.stem);
    let _ = writeln!(s, "## Note\n");
    if draft.note.trim().is_empty() {
        let _ = writeln!(s, "_(none provided)_\n");
    } else {
        let _ = writeln!(s, "{}\n", draft.note.trim());
    }
    s.push_str(&draft.metadata_md);
    match std::fs::write(&draft.md_path, s) {
        Ok(()) => info!("region_capture: wrote note sidecar {}", draft.md_path.display()),
        Err(e) => error!(
            "region_capture: cannot write note sidecar {}: {e}",
            draft.md_path.display()
        ),
    }
}
