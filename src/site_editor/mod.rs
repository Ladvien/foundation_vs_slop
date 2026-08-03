//! **The Site-67 editor** — a dev-only, F7-toggled tool for authoring the hub's dressing in the live
//! isometric view, with the placement rules checked as you drag rather than at next boot.
//!
//! Launch with `FVS_SITE_EDITOR=1 cargo run`, walk to the Site, press **F7**.
//!
//! # What it owns, and what it must not touch
//!
//! `assets/site/site67.ron` has two halves. `areas`, `floor`, `walls` and `doorways` are **generated**
//! by `scripts/gen_site67.py` from its own `ROOMS` table; that script exists because an earlier one
//! kept a hand-duplicated copy of the floor list and the two drifted, and its docstring says so. An
//! in-game tool that wrote those lists would rebuild exactly that trap. So this editor owns only the
//! hand-authored half — `props`, `cells`, `spawns` — and structure stays with the generator.
//!
//! Making structure editable is a later, deliberate move with a shape that keeps one source of truth:
//! port the generator's boundary/corner/connectivity derivation to Rust and delete the Python. It is
//! not a matter of widening [`source_map::Section`].
//!
//! # Why it exists
//!
//! `site::layout::check_prop_placements` already enforces six real rules — footprint inside its area,
//! no overlap, a resting prop has a host, a seat faces the surface it is pulled up to, thresholds stay
//! clear, nothing fronted faces a wall — and it even suggests a corrected yaw. But it runs **at load**,
//! as a wall of text, so dressing is authored blind in a text file and graded at startup. When the
//! Site was enlarged, the 86 props had to be moved by a throwaway script
//! (`scripts/migrate_site67_props.py`) whose own docstring names the failure it was avoiding: *"which
//! is how a chair ends up facing a wall it used to face away from."*
//!
//! This is that checker moved to per-edit, beside the thing being edited — the real-time evaluation
//! of Liapis, Yannakakis & Togelius, *Sentient Sketchbook* (FDG 2013) and Liapis, Smith & Shaker,
//! *Mixed-initiative content creation* (PCG Book ch. 11), which is also what `research_room::editor`
//! cites. Tanagra (Smith, Whitehead & Mateas, same chapter) is the other half of the idea: because the
//! validator guarantees the hub stays connected and legal, authoring attention moves from "is this
//! reachable" to "is this any good". The rules being checked descend from Merrell, Schkufza, Li,
//! Agrawala & Koltun, *Interactive Furniture Layout Using Interior Design Guidelines* (SIGGRAPH 2011)
//! — already implemented in `placement::solvers::metropolis` — and Tutenel et al. 2010's
//! surfaces-vs-affordances split, which the site kit encodes as `surfaces` / `rests_on`.
//!
//! # Determinism — a deliberate exemption
//!
//! This module is **exempt** from the repo rule that every feature is wired into the RL/QD search, and
//! the reason is the same one `docs/ui.md` §4.4 gives for UI and `docs/animation.md` gives for the
//! animation layer. `site::layout`'s header states that `site67.ron` is kept out of `config.ron`
//! precisely so the offline search can never evolve it — *"a search free to evolve the hub's layout
//! would destroy the one property the hub exists to have"* — so a tool for editing that file is
//! outside the search by the same argument. A genome gene pointed here could never move fitness.
//!
//! Mechanically it stays invisible: `#[cfg(debug_assertions)]`, gated on [`crate::SiteEditorActive`],
//! every system on `Update`, and never registered in `sim_harness`.

pub mod edit;
pub mod ghost;
pub mod overlay;
pub mod panel;
pub mod pick;
pub mod source_map;
pub mod thumbs;

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::input::{Action, Actions};
use crate::site::kit::SiteKit;
use crate::site::layout::PropPlacement;
use crate::site::pieces::SitePiece;
use crate::site::visuals::{PropIndex, SiteLayoutRes};
use crate::site::SiteKitRes;
use crate::ui::state::AppState;
use crate::ui::theme::{FontAssets, UiTheme};

use edit::{Change, EditorDoc};

/// Pixels the cursor must travel before a press becomes a drag rather than a click. Same value and
/// same reason as `region_capture::MIN_DRAG_PX`: a mis-click must not nudge a prop.
const MIN_DRAG_PX: f32 = 6.0;

/// Insert the [`crate::SiteEditorActive`] marker when `FVS_SITE_EDITOR=1`.
///
/// Same shape as `research_room::install_if_requested`: the env var is read once, at startup, and
/// every system in this module is gated on the marker's presence rather than on the variable — so a
/// build with the module compiled out has nothing to check, and a debug build without the variable
/// pays one `resource_exists` per frame.
pub fn install_if_requested(app: &mut App) {
    if std::env::var("FVS_SITE_EDITOR").as_deref() == Ok("1") {
        app.insert_resource(crate::SiteEditorActive);
        info!("site_editor: FVS_SITE_EDITOR=1 — F7 opens the dressing palette at Site-67");
    }
}

/// What the cursor is doing to a prop.
///
/// A flat state machine like `region_capture::Phase` rather than a pair of bools: "pressed but not
/// yet dragging" and "dragging" are genuinely different states and the deadzone lives between them.
#[derive(Default, Clone, Copy)]
pub enum Drag {
    #[default]
    Idle,
    /// Button down on a prop, not yet past the deadzone.
    Pressed {
        index: usize,
        /// Cursor offset from the prop's centre at grab time, so a drag does not snap the prop's
        /// centre to the cursor and jump it.
        grab: (f32, f32),
        start_px: Vec2,
    },
    /// Past the deadzone; the ghost is live and the record commits on release.
    Moving { index: usize, grab: (f32, f32) },
}

/// Everything the editor is holding.
#[derive(Resource)]
pub struct EditorState {
    pub open: bool,
    /// The document under edit. `None` until the panel is first opened.
    pub doc: Option<EditorDoc>,
    pub selected: Option<usize>,
    pub hovered: Option<usize>,
    pub drag: Drag,
    /// The piece a click on empty floor will place.
    pub brush: SitePiece,
    /// The yaw that piece will be placed at. `[` / `]` turn it while nothing is selected, and the
    /// ghost shows it — so a chair can be aimed *before* it exists rather than placed wrong and fixed.
    pub brush_yaw: f32,
    /// The music bus gain from before the editor opened, restored on close.
    pub music_was: Option<f32>,
    /// Last thing that happened, shown in the panel.
    pub status: String,
    /// Set by anything that changes the document; cleared by `panel::refresh_faults`.
    pub panel_dirty: bool,
    /// Whether the panel has been opened once this launch. Drives the open-on-arrival above.
    pub bootstrapped: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        EditorState {
            open: false,
            doc: None,
            selected: None,
            hovered: None,
            drag: Drag::default(),
            brush: SitePiece::Crate,
            brush_yaw: 0.0,
            music_was: None,
            status: String::new(),
            panel_dirty: false,
            bootstrapped: false,
        }
    }
}

/// The editor's systems. Registered from `lib::run` under `#[cfg(debug_assertions)]`.
///
/// Everything is on `Update` and gated on both [`crate::SiteEditorActive`] and
/// `AppState::Site` — the tool edits the hub, and the hub is the only place its overlay would mean
/// anything.
pub struct SiteEditorPlugin;

impl Plugin for SiteEditorPlugin {
    fn build(&self, app: &mut App) {
        let armed = resource_exists::<crate::SiteEditorActive>;
        app.init_resource::<EditorState>()
            // The gizmo's own systems must not run while the editor is closed, or its handles would
            // hang over the world in a normal launch. Its doc comment names this exact pattern.
            .add_observer(panel::on_palette_click)
            .add_systems(Startup, setup_thumbnails.run_if(armed))
            // Runs whether or not the panel is open: the gizmo overlay camera exists from the moment
            // `TransformGizmoPlugin` is registered, and it blanks the window until it is told not to.
            // The baker is deliberately NOT gated on the panel being open: it walks the kit once at
            // launch and then despawns its camera, so the previews are ready before they are wanted.
            .add_systems(
                Update,
                thumbs::bake
                    .run_if(armed)
                    // **`Option<Res<_>>`, never a bare `Res<_>`.** Bevy 0.19 evaluates EVERY run
                    // condition — it does not stop once `armed` returns false — and a missing
                    // `Res<T>` is a *panic* at parameter validation, not a skip (docs/ui.md §5
                    // trap 2). `Thumbnails` is only created when the editor is armed, so a bare
                    // `Res` here aborted the normal game launch on the very first frame, for
                    // everyone, with the editor switched off. It shipped that way; do not undo this.
                    //
                    // `finished`, not `done()` — see the field's docs; gating on `done()` would stop
                    // the system before it could dismantle its own booth.
                    .run_if(|t: Option<Res<thumbs::Thumbnails>>| {
                        t.is_some_and(|t| !t.finished())
                    }),
            )
            .add_systems(
                Update,
                enter_site_state
                    .run_if(armed)
                    .run_if(in_state(AppState::Title)),
            )
            .add_systems(
                Update,
                (
                    toggle_editor,
                    (
                        track_hover,
                        drag_props,
                        keyboard_edits,
                        panel::refresh_labels,
                        panel::refresh_faults,
                        panel::style_palette,
                        ghost::drive_ghost,
                        ghost::fade_ghost,
                        draw_overlay,
                    )
                        // Chained: several of these take `ResMut<EditorState>`, so Bevy would
                        // serialise them anyway — but on an order it picks, and "did the keyboard
                        // delete land before or after the drag read the selection" is not something
                        // to leave to the scheduler.
                        .chain()
                        .run_if(|s: Res<EditorState>| s.open),
                )
                    .chain()
                    .run_if(armed)
                    .run_if(in_state(AppState::Site)),
            );
    }
}

/// F7 opens and closes the panel.
///
/// Opening re-reads `site67.ron` from disk and refuses if it no longer describes the Site that is
/// standing (see [`EditorDoc::open`]) — a stale mapping would write edits to the wrong record.
fn toggle_editor(
    actions: Actions,
    mut state: ResMut<EditorState>,
    mut commands: Commands,
    roots: Query<Entity, With<panel::EditorRoot>>,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    layout: Option<Res<SiteLayoutRes>>,
    kit: Option<Res<SiteKitRes>>,
    rig: Option<ResMut<crate::camera::CameraRig>>,
    cams: Option<Query<&mut Transform, With<crate::MainCamera>>>,
    // Named `baked` rather than `thumbs` so it cannot shadow the module of that name.
    baked: Option<Res<thumbs::Thumbnails>>,
    bus: Option<ResMut<crate::audio::AudioBus>>,
) {
    // Opening on arrival makes `FVS_SITE_EDITOR=1` a scriptable one-shot — launch and you are editing
    // — the same reason `research_room::enter_room_state` auto-clicks "NEW RUN". Unlike the Research
    // Room's palette, which is an optional overlay on normal play, this panel *is* what the flag was
    // set for. `bootstrapped` makes it happen once, so F7 still closes it for an unobstructed look at
    // the room.
    let arriving = !state.bootstrapped;
    if !arriving && !actions.just_pressed(Action::DevSiteEditor) {
        return;
    }
    state.bootstrapped = true;

    if state.open {
        state.open = false;
        state.drag = Drag::Idle;
        // Put the music back exactly as it was, rather than assuming it was at unity.
        if let (Some(was), Some(mut bus)) = (state.music_was.take(), bus) {
            bus.music = was;
        }
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }

    let (Some(layout), Some(kit)) = (layout, kit) else {
        warn!("site editor: the Site is not loaded, nothing to edit");
        return;
    };
    if state.doc.is_none() {
        match EditorDoc::open(&layout.0, &kit.0) {
            Ok(doc) => {
                state.status = format!("{} props loaded", doc.layout.props.len());
                state.doc = Some(doc);
            }
            Err(e) => {
                // One path: refuse to open rather than opening a tool that would write to the wrong
                // line. The message names the cause and the fix.
                error!("{e}");
                return;
            }
        }
    }
    // Silence the adaptive music score while authoring. It is written to swell at threat and it has
    // nothing to say about a room being dressed — a loop under an hour of furniture-nudging is a
    // distraction, and the room tone is the part worth hearing anyway. Snapshotted, not zeroed-and-
    // assumed, so closing the panel restores whatever it was.
    if let Some(mut bus) = bus {
        state.music_was = Some(bus.music);
        bus.music = 0.0;
    }
    state.open = true;
    state.panel_dirty = true;
    panel::spawn(&mut commands, &theme, &fonts, baked.as_deref());

    // Frame the hub. `focus_camera_on_site` lands on the spine at the game's default zoom, which is a
    // 12 m-tall slice — the right framing for *standing* in the Site and much too tight for laying it
    // out, where the question is always "where is this relative to the room". Pull out to the wheel's
    // limit and centre on the floor's own bounding box.
    if let (Some(mut rig), Some(mut cams)) = (rig, cams) {
        let l = &layout.0;
        let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for r in &l.floor {
            lo = (lo.0.min(r.x as f32), lo.1.min(r.z as f32));
            hi = (hi.0.max((r.x + r.w) as f32), hi.1.max((r.z + r.h) as f32));
        }
        if lo.0 <= hi.0 {
            rig.set_zoom(crate::camera::MAX_ZOOM);
            let centre = l.point(((lo.0 + hi.0) * 0.5, (lo.1 + hi.1) * 0.5));
            crate::camera::snap_camera_to(centre, &mut rig, &mut cams);
        }
    }
}

/// Stand up the preview booth and its image handles at startup.
///
/// At `Startup` rather than on first open, so the palette's rows are already baked by the time anyone
/// looks at them — the walk takes about a second and doing it lazily would show 45 empty squares at
/// exactly the moment the author wants to pick something. Gated on the editor being armed, so a normal
/// launch never builds it.
fn setup_thumbnails(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let baked = thumbs::ensure(&mut commands, &mut images);
    info!("site_editor: preview booth up, baking palette thumbnails");
    commands.insert_resource(baked);
}

/// Walk straight to Site-67 when the editor is armed.
///
/// Same shape and same reason as `research_room::enter_room_state`: the env var should not mean
/// "launch, then click through the title card". Self-limiting via its `in_state(Title)` run condition.
fn enter_site_state(
    mut next: ResMut<NextState<AppState>>,
    title: Query<Entity, With<crate::ui::title::TitleRoot>>,
) {
    // **Do not leave `Title` until its screen actually exists.**
    //
    // Leaving on the first frame races the title screen's own `OnEnter` spawn: the state exits before
    // those entities are there, so `OnExit(Title)`'s `despawn_scoped::<TitleRoot>` finds nothing and
    // the menu is left hanging over the Site for the rest of the session. `TitleRoot` is a
    // **full-screen node with an opaque background** at `Z_MENU` and no `Pickable::IGNORE`, so a leaked
    // one does exactly two things: it hides the entire hub, and it swallows every world click.
    //
    // That is the whole of "the Site is black and I cannot place anything". Waiting on the entity —
    // rather than on a frame count, which is a guess about scheduling — is what makes the handoff
    // ordered instead of lucky.
    if title.is_empty() {
        return;
    }
    next.set(AppState::Site);
}

/// Which prop the cursor is over, for the hover outline.
fn track_hover(
    mut state: ResMut<EditorState>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>>,
    kit: Option<Res<SiteKitRes>>,
    ui_hover: Query<&Hovered>,
) {
    let (Some(window), Some(camera), Some(kit)) = (window, camera, kit) else {
        return;
    };
    // The cursor is over a control — every interactive widget carries `Hovered` and every readout is
    // `Pickable::IGNORE`, so this one query answers it for the whole UI (`selection.rs:410`).
    if ui_hover.iter().any(|h| h.0) {
        state.hovered = None;
        return;
    }
    let (cam, cam_tf) = *camera;
    let Some(doc) = state.doc.as_ref() else { return };
    let at = pick::cursor_layout_point(&doc.layout, &window, cam, cam_tf);
    state.hovered = at.and_then(|at| pick::prop_at(&doc.layout, &kit.0, at));
}

/// Press, drag, release — move a prop, or place a new one on empty floor.
#[allow(clippy::too_many_arguments)]
fn drag_props(
    mut state: ResMut<EditorState>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>>,
    kit: Option<Res<SiteKitRes>>,
    capture: Res<crate::DebugCaptureActive>,
    ui_hover: Query<&Hovered>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut props: Query<(Entity, &mut PropIndex, &mut Transform)>,
) {
    // Every early return below logs when it swallows a click. This is not scaffolding: "I clicked and
    // nothing happened" is the report this tool will keep generating, and a silent guard turns that
    // into an afternoon of guessing. It costs one bool per frame and prints only on a press.
    let click = mouse.just_pressed(MouseButton::Left);
    // The region-capture tool owns the mouse while it is armed, exactly as every consumer in
    // `selection.rs` respects.
    if capture.0 {
        if click {
            info!("site editor: click ignored — the region-capture tool owns the mouse");
        }
        return;
    }
    // Report before destructuring — the `Single`s are not `Copy`, so the match moves them.
    let (have_window, have_camera, have_kit) = (window.is_some(), camera.is_some(), kit.is_some());
    let (Some(window), Some(camera), Some(kit)) = (window, camera, kit) else {
        if click {
            info!(
                "site editor: click ignored — window={have_window} main-camera={have_camera} \
                 kit={have_kit}. A `Single` matching 0 or 2+ entities silently skips its system, \
                 which is what a second camera in the world does to this."
            );
        }
        return;
    };
    let (cam, cam_tf) = *camera;
    let cursor_px = window.cursor_position().unwrap_or_default();
    let over_ui = ui_hover.iter().any(|h| h.0);

    // Read everything needed out of the document first, as owned values, so the arms below are free
    // to mutate `state`. The alternative — holding `&doc` across the match — is what the borrow
    // checker refuses, and rightly: `commit` replaces the document wholesale.
    let (at, hit, dragged_yaw) = {
        let Some(doc) = state.doc.as_ref() else {
            if click {
                info!("site editor: click ignored — no document open");
            }
            return;
        };
        let Some(at) = pick::cursor_layout_point(&doc.layout, &window, cam, cam_tf) else {
            if click {
                info!("site editor: click ignored — the cursor ray never met the ground plane");
            }
            return;
        };
        let hit = pick::prop_at(&doc.layout, &kit.0, at)
            .and_then(|i| doc.layout.props.get(i).map(|p| (i, p.piece, p.pos)));
        let dragged_yaw = match state.drag {
            Drag::Moving { index, .. } => doc.layout.props.get(index).map(|p| p.yaw),
            _ => None,
        };
        (at, hit, dragged_yaw)
    };

    match state.drag {
        Drag::Idle => {
            if !click {
                return;
            }
            if over_ui {
                info!("site editor: click ignored — the cursor is over a UI control");
                return;
            }
            info!(
                "site editor: click at layout {at:?} — over {:?}, brush {:?}",
                hit.map(|(i, p, _)| (i, p)),
                state.brush
            );
            match hit {
                Some((index, piece, pos)) => {
                    // Grab offset, so the prop keeps its position under the cursor instead of
                    // snapping its centre there and jumping on the first pixel of movement.
                    let grab = (at.0 - pos.0, at.1 - pos.1);
                    state.selected = Some(index);
                    state.drag = Drag::Pressed {
                        index,
                        grab,
                        start_px: cursor_px,
                    };
                    state.status = format!("#{index} {piece:?} selected");
                }
                None => {
                    // Empty floor: place the armed brush there.
                    let pos = pick::snap(at);
                    let piece = state.brush;
                    place_new(
                        &mut state,
                        &kit.0,
                        &mut commands,
                        &assets,
                        piece,
                        pos,
                        &mut props,
                    );
                }
            }
        }
        Drag::Pressed {
            index,
            grab,
            start_px,
        } => {
            if mouse.just_released(MouseButton::Left) {
                state.drag = Drag::Idle;
                return;
            }
            if cursor_px.distance(start_px) > MIN_DRAG_PX {
                state.drag = Drag::Moving { index, grab };
            }
        }
        Drag::Moving { index, grab } => {
            if !mouse.just_released(MouseButton::Left) {
                return;
            }
            state.drag = Drag::Idle;
            let Some(yaw) = dragged_yaw else { return };
            let target = pick::snap((at.0 - grab.0, at.1 - grab.1));
            commit(&mut state, &kit.0, |doc, kit| {
                doc.move_prop(index, target, yaw, kit)
            });
            sync_world(&mut state, &kit.0, &mut commands, &assets, &mut props);
        }
    }
}

/// Delete, rotate, undo/redo, save.
#[allow(clippy::too_many_arguments)]
fn keyboard_edits(
    mut state: ResMut<EditorState>,
    keys: Res<ButtonInput<KeyCode>>,
    kit: Option<Res<SiteKitRes>>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut props: Query<(Entity, &mut PropIndex, &mut Transform)>,
) {
    let Some(kit) = kit else { return };
    let ctrl = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);

    if ctrl && keys.just_pressed(KeyCode::KeyS) {
        match state.doc.as_mut().map(EditorDoc::save) {
            Some(Ok(())) => {
                state.status = "saved to site67.ron".to_owned();
                info!("site editor: saved {}", crate::site::layout::SITE_LAYOUT_PATH);
            }
            Some(Err(e)) => {
                state.status = e.clone();
                error!("{e}");
            }
            None => {}
        }
        state.panel_dirty = true;
        return;
    }

    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        let outcome = state.doc.as_mut().and_then(|d| d.undo(&kit.0));
        after_history(&mut state, outcome, "nothing to undo");
        sync_world(&mut state, &kit.0, &mut commands, &assets, &mut props);
        return;
    }
    if ctrl && keys.just_pressed(KeyCode::KeyY) {
        let outcome = state.doc.as_mut().and_then(|d| d.redo(&kit.0));
        after_history(&mut state, outcome, "nothing to redo");
        sync_world(&mut state, &kit.0, &mut commands, &assets, &mut props);
        return;
    }

    // `[` / `]` turn the SELECTION if there is one, otherwise the brush — so the key means "rotate
    // the thing I am working on" either way, and a piece can be aimed before it is placed.
    let turn = if keys.just_pressed(KeyCode::BracketRight) {
        pick::YAW_STEP_DEG
    } else if keys.just_pressed(KeyCode::BracketLeft) {
        -pick::YAW_STEP_DEG
    } else {
        0.0
    };

    let Some(index) = state.selected else {
        if turn != 0.0 {
            state.brush_yaw = pick::snap_yaw(state.brush_yaw + turn);
            state.status = format!("{:?} facing {}\u{00b0}", state.brush, state.brush_yaw);
        }
        return;
    };

    if keys.just_pressed(KeyCode::Delete) {
        // Deleting a line that carries a comment destroys somebody's note, so say so rather than
        // letting it vanish into the diff.
        let note = state
            .doc
            .as_ref()
            .and_then(|d| d.prop_line(index).ok())
            .and_then(source_map::trailing_comment)
            .map(|c| format!(" (dropped comment: {c})"))
            .unwrap_or_default();
        commit(&mut state, &kit.0, |doc, kit| doc.delete_prop(index, kit));
        state.selected = None;
        state.status = format!("deleted #{index}{note}");
        sync_world(&mut state, &kit.0, &mut commands, &assets, &mut props);
        return;
    }

    if turn != 0.0 {
        let Some(doc) = state.doc.as_ref() else { return };
        let Some(p) = doc.layout.props.get(index) else {
            return;
        };
        let (pos, yaw) = (p.pos, pick::snap_yaw(p.yaw + turn));
        commit(&mut state, &kit.0, |doc, kit| {
            doc.move_prop(index, pos, yaw, kit)
        });
        sync_world(&mut state, &kit.0, &mut commands, &assets, &mut props);
    }
}

/// Run a document mutation and fold its outcome into the panel. Every edit goes through here so the
/// failure message reaches the author rather than only the log.
fn commit(
    state: &mut EditorState,
    kit: &SiteKit,
    f: impl FnOnce(&mut EditorDoc, &SiteKit) -> Result<Change, String>,
) {
    let Some(doc) = state.doc.as_mut() else { return };
    match f(doc, kit) {
        Ok(_) => state.panel_dirty = true,
        Err(e) => {
            state.status = e.clone();
            warn!("{e}");
        }
    }
}

fn after_history(
    state: &mut EditorState,
    outcome: Option<Result<Change, String>>,
    empty: &str,
) {
    match outcome {
        Some(Ok(_)) => {
            state.status = "ok".to_owned();
            state.panel_dirty = true;
        }
        Some(Err(e)) => {
            state.status = e.clone();
            warn!("{e}");
        }
        None => state.status = empty.to_owned(),
    }
    // An index the history moved may no longer exist.
    if let Some(doc) = state.doc.as_ref() {
        if state.selected.is_some_and(|i| i >= doc.layout.props.len()) {
            state.selected = None;
        }
    }
}

/// Place the armed brush at a snapped position and spawn its body.
#[allow(clippy::too_many_arguments)]
fn place_new(
    state: &mut EditorState,
    kit: &SiteKit,
    commands: &mut Commands,
    assets: &AssetServer,
    piece: SitePiece,
    pos: (f32, f32),
    props: &mut Query<(Entity, &mut PropIndex, &mut Transform)>,
) {
    let prop = PropPlacement {
        piece,
        pos,
        yaw: state.brush_yaw,
        waive: None,
    };
    commit(state, kit, |doc, kit| doc.insert_prop(prop, kit));
    if let Some(doc) = state.doc.as_ref() {
        state.selected = Some(doc.layout.props.len().saturating_sub(1));
        state.status = format!("placed {piece:?} at {pos:?}");
    }
    sync_world(state, kit, commands, assets, props);
}

/// Bring the spawned bodies into line with the document.
///
/// Rebuilds nothing: `site::visuals`' header is explicit that the Site is spawned once at `Startup`
/// precisely to avoid re-instantiating ~150 GLB scenes, so a full rebuild per drag would be unusable.
/// Instead every body is re-indexed and re-posed from the record it now corresponds to, and bodies
/// with no record left are despawned.
///
/// Re-indexing is unconditional rather than a delta, because an insert or a delete in the middle
/// shifts every later record and getting that wrong points the editor at the wrong line — the failure
/// this whole module is built to prevent.
fn sync_world(
    state: &mut EditorState,
    kit: &SiteKit,
    commands: &mut Commands,
    assets: &AssetServer,
    props: &mut Query<(Entity, &mut PropIndex, &mut Transform)>,
) {
    let Some(doc) = state.doc.as_ref() else { return };
    let layout = &doc.layout;

    // Collect and sort so the pass is independent of ECS query order, which is not stable across
    // `App` instances. Nothing here feeds the sim, but an editor whose behaviour depended on
    // iteration order would be maddening to debug.
    let mut bodies: Vec<(usize, Entity)> = props.iter().map(|(e, ix, _)| (ix.0, e)).collect();
    // SORT-OK: total key — `PropIndex` is unique per body (assigned from `enumerate` at spawn and
    // maintained here), so no two entries can tie. Input is a Vec built from a query, and the sort is
    // what removes that order dependence.
    bodies.sort_unstable_by_key(|(ix, _)| *ix);

    for (slot, &(_, entity)) in bodies.iter().enumerate() {
        match layout.props.get(slot) {
            Some(p) => {
                if let Ok((_, mut ix, mut tf)) = props.get_mut(entity) {
                    ix.0 = slot;
                    let mut at = layout.point(p.pos);
                    if let Some(Ok((top, _))) = crate::site::layout::resting_on(layout, kit, p) {
                        at.y += top;
                    }
                    tf.translation = at + Vec3::Y * kit.y_offset(p.piece);
                    tf.rotation = Quat::from_rotation_y(p.yaw.to_radians());
                }
            }
            // More bodies than records — a delete happened.
            None => commands.entity(entity).despawn(),
        }
    }

    // More records than bodies — an insert happened. Spawn through the real path so a placed prop is
    // built exactly like an authored one.
    for slot in bodies.len()..layout.props.len() {
        let Some(p) = layout.props.get(slot) else { break };
        let mut at = layout.point(p.pos);
        if let Some(Ok((top, _))) = crate::site::layout::resting_on(layout, kit, p) {
            at.y += top;
        }
        let e = crate::site::visuals::place(commands, assets, kit, p.piece, at, p.yaw);
        commands.entity(e).insert(PropIndex(slot));
    }
}

/// Draw the overlay.
fn draw_overlay(
    mut gizmos: Gizmos,
    state: Res<EditorState>,
    theme: Res<UiTheme>,
    kit: Option<Res<SiteKitRes>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>>,
) {
    let (Some(kit), Some(doc)) = (kit, state.doc.as_ref()) else {
        return;
    };

    // The ghost, when a drag is live: the snapped position the prop will actually take.
    let ghost = match (state.drag, window, camera) {
        (Drag::Moving { index, grab }, Some(window), Some(camera)) => {
            let (cam, cam_tf) = *camera;
            pick::cursor_layout_point(&doc.layout, &window, cam, cam_tf).and_then(|at| {
                doc.layout
                    .props
                    .get(index)
                    .map(|p| (p.piece, pick::snap((at.0 - grab.0, at.1 - grab.1)), p.yaw))
            })
        }
        _ => None,
    };

    overlay::draw(
        &mut gizmos,
        &theme,
        &doc.layout,
        &kit.0,
        &doc.faults,
        state.selected,
        state.hovered,
        ghost,
    );
}

