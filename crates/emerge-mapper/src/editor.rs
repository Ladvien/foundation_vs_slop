//! **The editing loop** — a palette, a ghost, a click, a file.
//!
//! # The ghost is the contract
//!
//! Everything an author does here goes through a preview that stands where the thing will actually
//! be. That is not polish; it is the rule `containment::cordon` states and the Site editor had to
//! learn twice: a preview drawn somewhere the piece will not end up *"is worse than no preview,
//! because it is a promise the game then breaks."* So the ghost is snapped, aimed, and lifted onto
//! its host exactly as the real placement will be.
//!
//! # Aiming happens before placing
//!
//! `[` and `]` turn the **brush**, never the last thing placed. The Site editor bound them to the
//! selection and it made rotation feel broken: placing selects, so the next `]` turned the piece you
//! had just put down while the ghost — the only thing on screen showing a facing — sat still.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton, ScrollArea};
use emerge_core::descriptor::Mount;
use emerge_core::map::Placed;

use crate::project::Project;
use crate::view::{cursor_ground, MainCamera};

/// Translation snap, metres. Half a metre is the unit the kits are authored on.
const SNAP: f32 = 0.5;
/// Yaw snap, degrees.
const YAW_STEP: f32 = 15.0;

const PANEL_BG: Color = Color::srgb(0.058, 0.054, 0.047);
const ROW_BG: Color = Color::srgb(0.098, 0.092, 0.082);
const ROW_ARMED: Color = Color::srgb(0.30, 0.28, 0.24);
const TEXT: Color = Color::srgb(0.86, 0.84, 0.80);
const TEXT_DIM: Color = Color::srgb(0.58, 0.56, 0.53);
const ACCENT: Color = Color::srgb(0.90, 0.66, 0.24);
/// The key column. Brighter than the description beside it, because the key is what you scan for.
const KEY: Color = Color::srgb(0.74, 0.71, 0.66);
/// The readout's label column — quieter than its value, which is the thing that changes.
const LABEL: Color = Color::srgb(0.46, 0.44, 0.42);
const DANGER: Color = Color::srgb(0.86, 0.36, 0.30);
/// Empty preview tile, so an un-baked row reads as "not yet" rather than as a hole in the panel.
const SLOT_BG: Color = Color::srgb(0.14, 0.135, 0.125);
/// The map's edge. Dim enough not to compete with the grid, bright enough to find.
const BOUNDS_LINE: Color = Color::srgb(0.42, 0.38, 0.30);

/// Edge of a palette row's preview box, logical px.
const THUMB_SLOT: f32 = 30.0;

#[derive(Resource)]
pub struct EditorState {
    /// Index into the library — what a click would place.
    pub brush: usize,
    pub brush_yaw: f32,
    pub status: String,
    /// Monotonic counter behind generated placement ids, so two crates never share a name.
    next_id: u32,
}

impl Default for EditorState {
    fn default() -> Self {
        EditorState {
            brush: 0,
            brush_yaw: 0.0,
            status: String::new(),
            next_id: 0,
        }
    }
}

/// A map-size nudge button: which axis, and by how much.
#[derive(Component, Clone, Copy)]
struct SizeNudge {
    axis: Axis,
    delta: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn label(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::Y => "Y",
            Axis::Z => "Z",
        }
    }
    fn get(self, b: (f32, f32, f32)) -> f32 {
        match self {
            Axis::X => b.0,
            Axis::Y => b.1,
            Axis::Z => b.2,
        }
    }
    fn set(self, b: &mut (f32, f32, f32), v: f32) {
        match self {
            Axis::X => b.0 = v,
            Axis::Y => b.1 = v,
            Axis::Z => b.2 = v,
        }
    }
    /// One click of this axis. X and Z move in whole rooms; Y moves in half a storey, because a
    /// ceiling is the one dimension an author tunes rather than lays out.
    fn step(self) -> f32 {
        match self {
            Axis::X | Axis::Z => 4.0,
            Axis::Y => 0.5,
        }
    }
}

/// The value text of one axis row, refreshed as the size changes.
#[derive(Component, Clone, Copy)]
struct SizeReadout(Axis);

/// A palette row, carrying its library index so one observer can serve all of them.
#[derive(Component, Clone, Copy)]
struct PaletteRow(usize);

/// A spawned instance of a map placement, tagged with the id it came from.
#[derive(Component)]
pub struct Placement(pub String);

/// The see-through preview of the armed brush.
#[derive(Component)]
struct Ghost;

/// Which descriptor the live ghost is showing, so it is rebuilt only when the brush changes —
/// respawning a GLB every frame would thrash the asset server and never finish loading.
#[derive(Component)]
struct GhostOf(usize);

#[derive(Component)]
struct Ghosted;

/// The readout block, so the whole thing can be found in one query.
#[derive(Component)]
struct StatusBlock;

/// One labelled row of the readout. A field per line, rather than one string with separators in it,
/// because a separator is a thing the reader has to parse and a column is not.
#[derive(Component, Clone, Copy, PartialEq)]
enum Field {
    Brush,
    Yaw,
    Map,
    /// The last thing that happened — the only line that is prose.
    Last,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Field::Brush => "BRUSH",
            Field::Yaw => "YAW",
            Field::Map => "MAP",
            Field::Last => "",
        }
    }
}

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
            .add_systems(
                Startup,
                (crate::thumbs::setup, spawn_panel, spawn_existing).chain(),
            )
            .add_systems(
                Update,
                (
                    keys,
                    place_on_click,
                    drive_ghost,
                    fade_ghost,
                    style_rows,
                    refresh_status,
                    refresh_size,
                    draw_bounds,
                ),
            )
            .add_observer(on_row_click)
            .add_observer(on_size_nudge);
    }
}

// ── chrome ───────────────────────────────────────────────────────────────────────────────────────

fn spawn_panel(
    mut commands: Commands,
    project: Res<Project>,
    thumbs: Option<Res<crate::thumbs::Thumbnails>>,
) {
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(12.0),
                width: Val::Px(300.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(12.0)),
                ..default()
            },
            // Opaque, not the game's translucent HUD panel. An editor panel is a work surface, and a
            // researcher in a white coat behind a translucent one is unreadable — measured.
            BackgroundColor(PANEL_BG),
            GlobalZIndex(100),
        ))
        .id();

    commands.entity(root).with_children(|p| {
        p.spawn((
            Text::new("EMERGE MAPPER"),
            TextColor(ACCENT),
            TextFont::from_font_size(15.0),
        ));
        // **The keys, inline and in a column.** A run-on line of `a | b | c` is one long thing to
        // read and it wraps unpredictably at any panel width; two aligned columns are a table, and
        // the eye finds a row in it without reading the others. Marschner §27.7 on clutter — the
        // first remedy is showing *less detail per item*, and a key needs exactly two facts.
        p.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(1.0),
            margin: UiRect::top(Val::Px(2.0)),
            ..default()
        })
        .with_children(|keys| {
            for (chord, what) in [
                ("click", "place"),
                ("[ ]", "aim"),
                ("F", "flood fill"),
                ("Del", "remove"),
                ("Q E", "turn view"),
                ("WASD", "pan"),
                ("wheel", "zoom"),
                ("Ctrl+S", "save"),
            ] {
                keys.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        // A fixed key column is what makes it a table rather than two ragged lists.
                        Node {
                            width: Val::Px(58.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(chord),
                        TextColor(KEY),
                        TextFont::from_font_size(11.0),
                        // **No wrap.** A chord containing a space (`Q E`, `[ ]`) is one token to a
                        // reader and two to a line-breaker, and letting it wrap turned three of these
                        // rows into two-line entries with the description stranded underneath.
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    ));
                    row.spawn((
                        Text::new(what),
                        TextColor(TEXT_DIM),
                        TextFont::from_font_size(11.0),
                    ));
                });
            }
        });

        // The readout, in the same two columns as the keys so the whole panel shares one left edge.
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(1.0),
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            StatusBlock,
        ))
        .with_children(|s| {
            for field in [Field::Brush, Field::Yaw, Field::Map, Field::Last] {
                s.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Node {
                            width: Val::Px(62.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(field.label()),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                    ));
                    row.spawn((
                        Text::new(""),
                        TextColor(TEXT),
                        TextFont::from_font_size(11.0),
                        field,
                    ));
                });
            }
        });

        // **Map size.** Stated, adjustable, and drawn in the world — an edge nothing shows is an
        // edge nobody believes. It is also what gives the flood fill somewhere to stop.
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            },
        ))
        .with_children(|s| {
            s.spawn((
                Text::new("MAP SIZE  (m)"),
                TextColor(LABEL),
                TextFont::from_font_size(10.0),
            ));
            for axis in [Axis::X, Axis::Y, Axis::Z] {
                s.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Node {
                            width: Val::Px(14.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(axis.label()),
                        TextColor(LABEL),
                        TextFont::from_font_size(11.0),
                    ));
                    for (glyph, delta) in [("-", -axis.step()), ("+", axis.step())] {
                        row.spawn((
                            UiButton,
                            Hovered::default(),
                            SizeNudge { axis, delta },
                            Node {
                                width: Val::Px(18.0),
                                justify_content: JustifyContent::Center,
                                flex_shrink: 0.0,
                                ..default()
                            },
                            BackgroundColor(ROW_BG),
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new(glyph),
                                TextColor(KEY),
                                TextFont::from_font_size(11.0),
                            ));
                        });
                    }
                    row.spawn((
                        Text::new(""),
                        TextColor(TEXT),
                        TextFont::from_font_size(11.0),
                        SizeReadout(axis),
                    ));
                });
            }
        });

        p.spawn((
            Text::new("PLACE"),
            TextColor(LABEL),
            TextFont::from_font_size(10.0),
            Node {
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            },
        ));
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                max_height: Val::Px(520.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollArea::default(),
        ))
        .with_children(|list| {
            for (ix, d) in project.library.descriptors.iter().enumerate() {
                list.spawn((
                    UiButton,
                    Hovered::default(),
                    PaletteRow(ix),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(ROW_BG),
                ))
                .with_children(|row| {
                    // A fixed slot whether or not the bake has reached this piece, so arriving
                    // thumbnails never reflow the list under the cursor.
                    let mut slot = row.spawn((
                        Node {
                            width: Val::Px(THUMB_SLOT),
                            height: Val::Px(THUMB_SLOT),
                            margin: UiRect::right(Val::Px(8.0)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(SLOT_BG),
                    ));
                    if let Some(image) = thumbs.as_ref().and_then(|t| t.image(ix)) {
                        // `ImageNode::new`, never `default()` — the default is an invisible 1x1
                        // transparent texture.
                        slot.insert(ImageNode::new(image));
                    }
                    row.spawn((
                        Text::new(d.id.clone()),
                        TextColor(TEXT),
                        TextFont::from_font_size(11.0),
                    ));
                });
            }
        });
    });
}

/// One observer for the whole palette. `Activate` carries the entity, so the index lives on the row
/// as a component and there is no macro-unrolled observer per row.
fn on_row_click(
    activate: On<Activate>,
    rows: Query<&PaletteRow>,
    project: Res<Project>,
    mut state: ResMut<EditorState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    state.brush = row.0;
    if let Some(d) = project.library.descriptors.get(row.0) {
        state.status = format!("{} armed", d.id);
    }
}

/// Grow or shrink one axis. Clamped at one cell rather than at zero: a map has to enclose something,
/// and `Map::validate` refuses a non-positive extent — better to stop at the floor than to write a
/// map the save will then reject.
fn on_size_nudge(
    activate: On<Activate>,
    nudges: Query<&SizeNudge>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    let Ok(nudge) = nudges.get(activate.entity) else {
        return;
    };
    let mut bounds = project.map.bounds;
    let want = (nudge.axis.get(bounds) + nudge.delta).max(nudge.axis.step());
    nudge.axis.set(&mut bounds, want);
    if bounds != project.map.bounds {
        project.map.bounds = bounds;
        project.dirty = true;
        state.status = format!(
            "map is {:.0} x {:.1} x {:.0} m",
            bounds.0, bounds.1, bounds.2
        );
    }
}

fn refresh_size(project: Res<Project>, mut readouts: Query<(&SizeReadout, &mut Text)>) {
    for (readout, mut text) in &mut readouts {
        let v = readout.0.get(project.map.bounds);
        // One decimal only where it earns it — "32" reads faster than "32.0", and Y is the axis that
        // actually lands on halves.
        let want = if (v - v.round()).abs() < 1e-3 {
            format!("{v:.0}")
        } else {
            format!("{v:.1}")
        };
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// Draw the map's extent as a wireframe box.
///
/// Gizmos rather than a mesh: the bounds are a statement about the map, not a thing in it, and a real
/// box would be pickable, shadow-casting, and something a click could land on.
fn draw_bounds(project: Res<Project>, mut gizmos: Gizmos) {
    let (min_x, min_z, max_x, max_z) = project.map.floor_rect();
    let (floor, ceiling) = project.map.height_span();
    let (w, h, d) = project.map.bounds;
    let centre = Vec3::new(
        (min_x + max_x) * 0.5,
        (floor + ceiling) * 0.5,
        (min_z + max_z) * 0.5,
    );
    // `cube`, not `cuboid` — 0.19 spells it `Gizmos::cube` and takes a transform whose SCALE is
    // the box's size (`bevy_gizmos-0.19.0/src/gizmos.rs:637`).
    gizmos.cube(
        Transform::from_translation(centre).with_scale(Vec3::new(w, h, d)),
        BOUNDS_LINE,
    );
}

fn style_rows(
    state: Res<EditorState>,
    mut rows: Query<(&PaletteRow, &Hovered, &mut BackgroundColor)>,
) {
    for (row, hovered, mut bg) in &mut rows {
        let want = if row.0 == state.brush {
            ROW_ARMED
        } else if hovered.0 {
            Color::srgb(0.16, 0.15, 0.14)
        } else {
            ROW_BG
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

fn refresh_status(
    project: Res<Project>,
    state: Res<EditorState>,
    mut fields: Query<(&Field, &mut Text, &mut TextColor)>,
) {
    let brush = project
        .library
        .descriptors
        .get(state.brush)
        .map(|d| d.id.as_str())
        .unwrap_or("none");

    for (field, mut text, mut colour) in &mut fields {
        let (want, tint) = match field {
            Field::Brush => (brush.to_owned(), TEXT),
            Field::Yaw => (format!("{} deg", state.brush_yaw), TEXT),
            Field::Map => (
                if project.dirty {
                    format!("{} placed, unsaved", project.map.placements.len())
                } else {
                    format!("{} placed", project.map.placements.len())
                },
                if project.dirty { ACCENT } else { TEXT },
            ),
            // A refusal has to read differently from a success, or "NOT SAVED" scrolls past as if it
            // were a receipt.
            Field::Last => (
                state.status.clone(),
                if state.status.starts_with("NOT SAVED") {
                    DANGER
                } else {
                    TEXT_DIM
                },
            ),
        };
        if text.0 != want {
            text.0 = want;
        }
        if colour.0 != tint {
            colour.0 = tint;
        }
    }
}

// ── placing ──────────────────────────────────────────────────────────────────────────────────────

fn snap(v: f32) -> f32 {
    (v / SNAP).round() * SNAP
}

/// Where a descriptor's origin goes for a given map position — the arithmetic the ghost and the real
/// placement both use, so they cannot disagree.
fn origin_of(d: &emerge_core::descriptor::Descriptor, at: (f32, f32)) -> Vec3 {
    let lift = match &d.mount {
        Some(Mount::OnWall { height }) => *height,
        Some(Mount::OnCeiling) => 2.4,
        _ => 0.0,
    };
    Vec3::new(at.0, lift + d.align.y_offset.unwrap_or(0.0), at.1)
}

fn spawn_piece(
    commands: &mut Commands,
    assets: &AssetServer,
    d: &emerge_core::descriptor::Descriptor,
    at: (f32, f32),
    yaw_deg: f32,
) -> Option<Entity> {
    let mesh = d.mesh.as_ref()?;
    let scene: Handle<WorldAsset> =
        assets.load(GltfAssetLabel::Scene(0).from_asset(mesh.clone()));
    let scale = d.align.scale.unwrap_or(1.0);
    // `front` is the mesh's own facing offset — the kit records it precisely so an authored yaw means
    // the same thing for every piece. Adding it here is what stops chairs being placed sideways.
    let yaw = yaw_deg + d.align.front.unwrap_or(0.0);
    Some(
        commands
            .spawn((
                Transform::from_translation(origin_of(d, at))
                    .with_rotation(Quat::from_rotation_y(yaw.to_radians()))
                    .with_scale(Vec3::splat(scale)),
                Visibility::Inherited,
            ))
            .with_child((WorldAssetRoot(scene), Transform::default()))
            .id(),
    )
}

/// Bring up whatever the map already holds.
fn spawn_existing(mut commands: Commands, assets: Res<AssetServer>, project: Res<Project>) {
    for p in &project.map.placements {
        let Some(d) = project.library.get(&p.descriptor) else {
            // Loud, not silent: a placement naming a descriptor the library does not have is a hole
            // in the map, and an author must be told which one rather than counting missing crates.
            warn!(
                "placement `{}` names descriptor `{}`, which this library does not have",
                p.id, p.descriptor
            );
            continue;
        };
        if let Some(e) = spawn_piece(&mut commands, &assets, d, p.at, p.yaw) {
            commands.entity(e).insert(Placement(p.id.clone()));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn place_on_click(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    assets: Res<AssetServer>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // A click on a control is not a click on the world. Without this, arming a piece from the palette
    // also dropped one wherever the panel happened to be over.
    if hovered_ui.iter().any(|h| h.0) {
        return;
    }
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        return;
    };

    let Some(d) = project.library.descriptors.get(state.brush).cloned() else {
        return;
    };
    let at = (snap(hit.x), snap(hit.z));
    state.next_id += 1;
    let id = format!("{}@{}", short_id(&d.id), state.next_id);

    let placed = Placed {
        id: id.clone(),
        descriptor: d.id.clone(),
        at,
        yaw: state.brush_yaw,
        ..Placed::default()
    };
    project.map.placements.push(placed);
    project.dirty = true;

    if let Some(e) = spawn_piece(&mut commands, &assets, &d, at, state.brush_yaw) {
        commands.entity(e).insert(Placement(id.clone()));
    }
    state.status = format!("placed {id} at ({}, {})", at.0, at.1);
}

/// The tail of a descriptor id, so a generated placement id reads as `crate@7` rather than
/// `kenney_prototype-kit/crate@7`.
fn short_id(descriptor_id: &str) -> &str {
    descriptor_id.rsplit('/').next().unwrap_or(descriptor_id)
}

#[allow(clippy::too_many_arguments)]
fn keys(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    assets: Res<AssetServer>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);

    if ctrl && keyboard.just_pressed(KeyCode::KeyS) {
        match project.save() {
            Ok(()) => {
                let path = project.map_path.display().to_string();
                state.status = format!("saved {path}");
                info!("saved {path}");
            }
            // The save refuses on an invalid map rather than writing one, and says which rule.
            Err(e) => {
                state.status = format!("NOT SAVED: {e}");
                error!("{e}");
            }
        }
        return;
    }

    // **F floods.** From the cell under the cursor outward, stopping at anything already placed and
    // at the map's edge — see `crate::fill`.
    if keyboard.just_pressed(KeyCode::KeyF) && !hovered_ui.iter().any(|h| h.0) {
        flood_from_cursor(
            &mut commands,
            &assets,
            window,
            camera,
            &mut project,
            &mut state,
        );
        return;
    }

    let step = if keyboard.just_pressed(KeyCode::BracketRight) {
        YAW_STEP
    } else if keyboard.just_pressed(KeyCode::BracketLeft) {
        -YAW_STEP
    } else {
        0.0
    };
    if step != 0.0 {
        state.brush_yaw = (state.brush_yaw + step).rem_euclid(360.0);
    }
}

/// The `F` handler, split out so `keys` stays readable.
fn flood_from_cursor(
    commands: &mut Commands,
    assets: &AssetServer,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &mut Project,
    state: &mut EditorState,
) {
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        return;
    };
    let Some(brush) = project.library.descriptors.get(state.brush).cloned() else {
        return;
    };

    let start_id = state.next_id;
    let mut n = start_id;
    let short = short_id(&brush.id).to_owned();
    let filled = match crate::fill::flood(
        &project.map,
        &brush,
        (hit.x, hit.z),
        state.brush_yaw,
        || {
            n += 1;
            format!("{short}@{n}")
        },
    ) {
        Ok(f) => f,
        // A refusal is the answer, not a failure: "outside the map" and "something is already here"
        // are both things the author needs told rather than worked around.
        Err(e) => {
            state.status = e;
            return;
        }
    };
    state.next_id = n;

    let count = filled.placements.len();
    for p in filled.placements {
        if let Some(e) = spawn_piece(commands, assets, &brush, p.at, p.yaw) {
            commands.entity(e).insert(Placement(p.id.clone()));
        }
        project.map.placements.push(p);
    }
    project.dirty = true;
    // A cap that stopped the fill has to say so — a truncated fill looks exactly like a finished one.
    state.status = if filled.truncated {
        format!(
            "filled {count} and STOPPED at the {} cell cap — fill again to continue",
            crate::fill::MAX_CELLS
        )
    } else {
        format!("filled {count} with {}", brush.id)
    };
}

// ── the ghost ────────────────────────────────────────────────────────────────────────────────────

/// How much of its opacity a ghost keeps.
const GHOST_ALPHA: f32 = 0.45;

#[allow(clippy::too_many_arguments)]
fn drive_ghost(
    mut commands: Commands,
    assets: Res<AssetServer>,
    project: Res<Project>,
    state: Res<EditorState>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    ghosts: Query<(Entity, &GhostOf), With<Ghost>>,
    mut transforms: Query<&mut Transform, With<Ghost>>,
) {
    let clear = |commands: &mut Commands| {
        for (e, _) in &ghosts {
            commands.entity(e).despawn();
        }
    };

    let (Some(window), Some(camera)) = (window, camera) else {
        clear(&mut commands);
        return;
    };
    // No ghost while the cursor is over the panel: the piece is not going there, and a preview
    // hovering under the palette is a promise about a click that will never reach the world.
    if hovered_ui.iter().any(|h| h.0) {
        clear(&mut commands);
        return;
    }
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        clear(&mut commands);
        return;
    };
    let Some(d) = project.library.descriptors.get(state.brush) else {
        clear(&mut commands);
        return;
    };

    let at = (snap(hit.x), snap(hit.z));
    let yaw = state.brush_yaw + d.align.front.unwrap_or(0.0);

    let existing = ghosts.iter().find(|(_, g)| g.0 == state.brush).map(|(e, _)| e);
    for (e, g) in &ghosts {
        if g.0 != state.brush {
            commands.entity(e).despawn();
        }
    }

    match existing {
        Some(e) => {
            if let Ok(mut tf) = transforms.get_mut(e) {
                tf.translation = origin_of(d, at);
                tf.rotation = Quat::from_rotation_y(yaw.to_radians());
            }
        }
        None => {
            if let Some(e) = spawn_piece(&mut commands, &assets, d, at, state.brush_yaw) {
                commands.entity(e).insert((Ghost, GhostOf(state.brush)));
            }
        }
    }
}

/// Fade the ghost once its GLB has instantiated materials.
///
/// The materials come from the asset, so they are the *shared* handles every real instance uses —
/// writing alpha into them would turn every crate in the map see-through. Each descendant gets its
/// own clone, marked so the walk settles to a no-op.
fn fade_ghost(
    mut commands: Commands,
    ghosts: Query<Entity, With<Ghost>>,
    children: Query<&Children>,
    painted: Query<&MeshMaterial3d<StandardMaterial>, Without<Ghosted>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for root in &ghosts {
        let mut queue = vec![root];
        while let Some(e) = queue.pop() {
            if let Ok(handle) = painted.get(e) {
                if let Some(base) = materials.get(&handle.0) {
                    let mut faded = base.clone();
                    faded.alpha_mode = AlphaMode::Blend;
                    let a = faded.base_color.alpha() * GHOST_ALPHA;
                    faded.base_color.set_alpha(a);
                    let handle = materials.add(faded);
                    commands.entity(e).insert((
                        MeshMaterial3d(handle),
                        Ghosted,
                        bevy::light::NotShadowCaster,
                    ));
                }
            }
            if let Ok(kids) = children.get(e) {
                queue.extend(kids.iter());
            }
        }
    }
}
