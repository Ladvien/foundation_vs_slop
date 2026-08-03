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

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::camera::visibility::ViewVisibility;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton, ScrollArea};
use emerge_core::map::Placed;

use crate::keys::{self, Action, Context};
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
/// A category heading — quieter than a row, because it is a signpost rather than a thing to click on
/// most of the time.
const HEADER_BG: Color = Color::srgb(0.075, 0.070, 0.063);

/// Where a descriptor with no `kind` goes. Named rather than hidden: an untagged piece is work to do,
/// and a palette that quietly omitted it would be a palette missing pieces.
const UNSORTED: &str = "unsorted";
/// The map's edge. Dim enough not to compete with the grid, bright enough to find.
const BOUNDS_LINE: Color = Color::srgb(0.42, 0.38, 0.30);

/// Edge of a palette row's preview box, logical px.
const THUMB_SLOT: f32 = 30.0;

/// Scene triangle counts worth colouring. Not limits — this machine draws far more than either — but
/// the points at which an author should know what they have built.
const BUSY_SCENE: usize = 1_000_000;
const HEAVY_SCENE: usize = 5_000_000;

#[derive(Resource)]
pub struct EditorState {
    /// Index into the library — what a click would place.
    pub brush: usize,
    pub brush_yaw: f32,
    pub status: String,
    /// Monotonic counter behind generated placement ids, so two crates never share a name.
    next_id: u32,
    /// Advanced on every solve, so pressing `G` twice offers a different arrangement rather than the
    /// same one — a generator that cannot be asked again is one you have to undo to disagree with.
    seed: u64,
    /// Categories the author has folded away.
    ///
    /// A set of names rather than a per-row flag: the grouping is derived from the library every
    /// rebuild, so a flag stored on a row would be lost the moment the library changed.
    collapsed: std::collections::HashSet<String>,
    /// A pin waiting for its reason: the placement index, and what has been typed so far.
    ///
    /// `Placed::owned_because` is *a reason, never a bool*, on the schema's own argument: a bool lets
    /// "I could not be bothered" and "this is the cell block's only entrance" look identical in a
    /// diff. A canned reason supplied by the editor would be that bool wearing a sentence, so pinning
    /// asks.
    pinning: Option<(usize, String)>,
    /// The raw text being typed into the name, or `None` when not renaming.
    ///
    /// Raw, with the snake_case spelling applied for display and on commit — so a backspace undoes a
    /// keystroke rather than an underscore the transform inserted, and "Site 67" reads back as
    /// `site_67` while it is still being typed.
    renaming: Option<String>,
    /// What can be undone, most recent last. See [`Undo`].
    undo: Vec<Undo>,
}

/// One reversible edit.
///
/// Only placements, and deliberately: the map's *size* and *name* are settings rather than edits, and
/// folding them into the same stack would mean Ctrl+Z sometimes resized the map when an author meant
/// to take back a crate. One undo stack, one kind of thing in it.
enum Undo {
    /// Remove the last `count` placements — the inverse of a place or a fill.
    Added { count: usize },
    /// Put a removed placement back where it was, at its old index.
    Removed { index: usize, placed: Box<Placed> },
}

impl Default for EditorState {
    fn default() -> Self {
        EditorState {
            brush: 0,
            brush_yaw: 0.0,
            status: String::new(),
            next_id: 0,
            seed: 1,
            collapsed: std::collections::HashSet::new(),
            pinning: None,
            renaming: None,
            undo: Vec::new(),
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

/// The node the palette is rebuilt into, so collapsing a category can redraw it.
#[derive(Component)]
struct PaletteList;

/// A category header. Clicking it folds the group away.
#[derive(Component, Clone)]
struct CategoryHeader(String);

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

/// The live cost readout, bottom right.
#[derive(Component)]
struct TriangleTotal;

/// One labelled row of the readout. A field per line, rather than one string with separators in it,
/// because a separator is a thing the reader has to parse and a column is not.
#[derive(Component, Clone, Copy, PartialEq)]
enum Field {
    Name,
    Brush,
    Yaw,
    Map,
    /// The last thing that happened — the only line that is prose.
    Last,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Field::Name => "NAME",
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
                (
                    crate::thumbs::setup,
                    spawn_panel,
                    spawn_cost_readout,
                    spawn_existing,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    rename_keys,
                    pin_reason_keys,
                    keys.run_if(not_renaming).run_if(in_map_mode),
                    place_on_click.run_if(not_renaming).run_if(in_map_mode),
                    drive_ghost.run_if(in_map_mode),
                    fade_ghost,
                    style_rows,
                    refresh_status,
                    rebuild_palette.run_if(
                        resource_changed::<Project>
                            .or(resource_changed::<EditorState>)
                            .or(run_once),
                    ),
                    refresh_size,
                    refresh_triangle_total,
                    draw_bounds,
                ),
            )
            .add_observer(on_row_click)
            .add_observer(on_category_click)
            .add_observer(on_size_nudge);
    }
}

// ── chrome ───────────────────────────────────────────────────────────────────────────────────────

/// The cost readout, in its own root anchored bottom right.
///
/// Separate from the panel rather than a row in it: it is about the *scene*, not about the tool, and
/// it belongs where the eye goes last rather than in the middle of the controls.
fn spawn_cost_readout(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(12.0),
                bottom: Val::Px(10.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            GlobalZIndex(100),
            // Nothing here is clickable, and a readout that eats clicks is a readout that steals the
            // corner of the map underneath it.
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                TextColor(TEXT_DIM),
                TextFont::from_font_size(11.0),
                TriangleTotal,
            ));
        });
}

fn spawn_panel(
    mut commands: Commands,
    project: Res<Project>,
    thumbs: Option<Res<crate::thumbs::Thumbnails>>,
) {
    let root = commands
        .spawn((
            crate::tiles::MapRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(crate::tiles::TAB_STRIP_BOTTOM),
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
        // **The keys, inline and in a column — read from the census, never retyped.**
        //
        // `docs/ui.md` §3.5 records what happens otherwise: key allocation lived in five prose
        // censuses and all five drifted to the same wrong answer. A panel that types its own key list
        // is a sixth. This renders `keys::in_context`, so a binding that changes changes here.
        //
        // Two aligned columns rather than a run-on line: a run-on wraps unpredictably at any width,
        // and the eye finds a row in a table without reading the others.
        p.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(1.0),
            margin: UiRect::top(Val::Px(2.0)),
            ..default()
        })
        .with_children(|list| {
            for row_def in keys::rows(Context::Map)
                .into_iter()
                .chain(keys::rows(Context::Global))
            {
                list.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    // A guaranteed gutter, so the widest chord still has air before its label.
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Node {
                            // **`min_width`, not `width`.** A fixed width does not clip or shrink its
                            // text — an over-long chord simply draws past the column and lands on top
                            // of the label beside it, which is exactly what "W, A, S, D" did to
                            // "pan". `min_width` keeps the column aligned for every row that fits and
                            // lets the one that does not push its label right instead of through it.
                            min_width: Val::Px(78.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(row_def.chord.clone()),
                        TextColor(KEY),
                        TextFont::from_font_size(11.0),
                        // No wrap: a chord with a space in it is one token to a reader and two to a
                        // line-breaker.
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    ));
                    row.spawn((
                        Text::new(row_def.does),
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
            for field in [Field::Name, Field::Brush, Field::Yaw, Field::Map, Field::Last] {
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
            PaletteList,
        ));
    });
}

/// **The palette, grouped.**
///
/// Forty-one rows is a list you scroll past; grouped by what a thing IS it becomes a handful of
/// headings you can skip. The grouping is the `kind` axis of the project's own vocabulary rather than
/// a second taxonomy invented for the panel — `docs/ui.md` §1.2 names over-informing as the failure
/// mode, and two competing category systems is the version of that which also makes people wrong.
///
/// Categories appear in VOCABULARY order and never in frequency or recency order. Samp 2011, via
/// §3.5: a menu's cost is paid at first sight, so fix positions permanently — a palette that
/// reshuffles as a map fills is one nobody builds a memory of.
fn rebuild_palette(
    mut commands: Commands,
    project: Res<Project>,
    state: Res<EditorState>,
    thumbs: Option<Res<crate::thumbs::Thumbnails>>,
    lists: Query<Entity, With<PaletteList>>,
) {
    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|p| {
            for (category, members) in categories(&project) {
                let folded = state.collapsed.contains(&category);
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    CategoryHeader(category.clone()),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(6.0),
                        margin: UiRect::top(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(HEADER_BG),
                ))
                .with_children(|row| {
                    // A caret, so folded and unfolded differ by shape and not only by what is below
                    // them — an encoding that survives being glanced at.
                    row.spawn((
                        Node {
                            width: Val::Px(10.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(if folded { ">" } else { "v" }),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                    ));
                    row.spawn((
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        Text::new(category.to_uppercase()),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                    ));
                    row.spawn((
                        Text::new(format!("{}", members.len())),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                    ));
                });

                if folded {
                    continue;
                }
                for ix in members {
                    let Some(d) = project.library.descriptors.get(ix) else {
                        continue;
                    };
                    p.spawn((
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
                            slot.insert(ImageNode::new(image));
                        }
                        row.spawn((
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                            Text::new(d.id.clone()),
                            TextColor(TEXT),
                            TextFont::from_font_size(11.0),
                        ));
                        let tris = project.triangles.get(ix).copied().unwrap_or(0);
                        row.spawn((
                            Text::new(brief_count(tris)),
                            TextColor(cost_tint(tris)),
                            TextFont::from_font_size(10.0),
                            TextLayout::new(Justify::Right, LineBreak::NoWrap),
                        ));
                    });
                }
            }
        });
    }
}

/// The library grouped by its primary `kind`, in vocabulary order.
///
/// A descriptor's FIRST kind token is its category. Most carry exactly one — the converter maps the
/// old `category` field straight across — and for the few that carry several, the first is the one
/// the author wrote first, which is a better guess at "what this mainly is" than any rule this
/// function could invent.
fn categories(project: &Project) -> Vec<(String, Vec<usize>)> {
    let mut out: Vec<(String, Vec<usize>)> = project
        .vocab
        .kind
        .names()
        .map(|n| (n.to_owned(), Vec::new()))
        .collect();
    // A home for anything untagged, LAST — it is a to-do list, not a category, and putting it first
    // would give the least useful group the most valuable position.
    out.push((UNSORTED.to_owned(), Vec::new()));

    for (ix, d) in project.library.descriptors.iter().enumerate() {
        let key = d.kind.first().map(String::as_str).unwrap_or(UNSORTED);
        match out.iter_mut().find(|(name, _)| name == key) {
            Some((_, members)) => members.push(ix),
            // A token the vocabulary does not have cannot reach here — `Library::resolve` refuses it
            // at load — but grouping it visibly beats dropping the row from the palette entirely.
            None => {
                if let Some((_, members)) = out.iter_mut().find(|(name, _)| name == UNSORTED) {
                    members.push(ix);
                }
            }
        }
    }
    // Empty categories are noise: the vocabulary has sixteen and a project may use six.
    out.retain(|(_, members)| !members.is_empty());
    out
}

/// Fold or unfold a category.
fn on_category_click(
    activate: On<Activate>,
    headers: Query<&CategoryHeader>,
    mut state: ResMut<EditorState>,
) {
    let Ok(header) = headers.get(activate.entity) else {
        return;
    };
    if !state.collapsed.remove(&header.0) {
        state.collapsed.insert(header.0.clone());
    }
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
            // While renaming, the field shows what the name WILL be, not what was typed — that is
            // what "forced" means, and seeing `site_67` appear as you type "Site 67" teaches the rule
            // without anyone having to read it.
            Field::Name => match &state.renaming {
                // A caret, so an empty field reads as "waiting for you" rather than as the name
                // having been wiped.
                Some(raw) => (
                    format!("{}_", emerge_core::naming::to_snake_case(raw)),
                    ACCENT,
                ),
                None => (project.map.name.clone(), TEXT),
            },
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

/// While a name is being typed, every other key belongs to the name.
///
/// `Option<Res<_>>` is not needed here because `EditorState` is `init_resource`d by this same plugin
/// — but every run condition IS evaluated in Bevy 0.19, with no short-circuit, so a condition reading
/// a resource some *other* plugin owns must take the option. Worth stating next to the one place that
/// legitimately does not.
fn not_renaming(state: Res<EditorState>) -> bool {
    state.renaming.is_none() && state.pinning.is_none()
}

/// Placing belongs to map mode. Without this, `F` in import mode would flood the map with whatever
/// the palette last had armed, which is a surprising amount of work to undo.
fn in_map_mode(mode: Res<crate::tiles::Mode>) -> bool {
    *mode == crate::tiles::Mode::Map
}

/// Type a name. Snake case is applied to what is shown and to what is committed, so the illegal state
/// is never reachable rather than merely being rejected at the end.
fn rename_keys(
    mut events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    if state.renaming.is_none() {
        // `N` starts a rename. Read from the buffered events like everything else here, so a keypress
        // cannot both start the rename and be typed into it.
        if keys::just_pressed(&keyboard, Action::RenameMap) {
            // **Empty, not seeded with the current name.** Seeding it meant the first keystroke
            // appended, so renaming `site_67_hub` to `galley_deck` produced
            // `site_67_hubgalley_deck` — and it looked like it had worked, because the panel showed a
            // name growing as expected. A real rename field starts with the old name SELECTED so
            // typing replaces it; there is no selection model here, so the honest equivalent is to
            // start blank. The old name is still on screen the moment you press Esc.
            state.renaming = Some(String::new());
            state.status = format!(
                "type a new name for `{}` — Enter to keep it, Esc to leave it alone",
                project.map.name
            );
        }
        return;
    }

    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let raw = state.renaming.take().unwrap_or_default();
                let name = emerge_core::naming::to_snake_case(&raw);
                if name.is_empty() {
                    state.status = "a map needs a name; nothing was changed".to_owned();
                } else if name != project.map.name {
                    let was = std::mem::replace(&mut project.map.name, name.clone());
                    project.dirty = true;
                    // The file follows the name on the next save, and the old one stays where it is
                    // — deleting it here would destroy a file on a keystroke.
                    state.status = format!("renamed `{was}` to `{name}` (Ctrl+S writes the new file)");
                }
            }
            Key::Escape => {
                state.renaming = None;
                state.status = "name unchanged".to_owned();
            }
            Key::Backspace => {
                if let Some(raw) = state.renaming.as_mut() {
                    raw.pop();
                }
            }
            Key::Space => {
                if let Some(raw) = state.renaming.as_mut() {
                    raw.push(' ');
                }
            }
            Key::Character(s) => {
                if let Some(raw) = state.renaming.as_mut() {
                    raw.push_str(s);
                }
            }
            _ => {}
        }
    }
}

/// **Triangles actually drawn this frame**, bottom right.
///
/// Visible entities, not resident ones — the same distinction `perf_probe` draws in the game, and for
/// the same reason: a culled mesh costs memory, not frame time, and mixing them is how you end up
/// optimising the wrong thing. It is here because a flood fill can lay down 1,400 pieces in one
/// keystroke, and the moment to notice that costs 40 million triangles is while it is happening.
fn refresh_triangle_total(
    drawn: Query<(&ViewVisibility, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
    mut readout: Query<(&mut Text, &mut TextColor), With<TriangleTotal>>,
) {
    let total: usize = drawn
        .iter()
        .filter(|(v, _)| v.get())
        .filter_map(|(_, m)| meshes.get(&m.0))
        .map(|m| match m.indices() {
            Some(i) => i.len() / 3,
            None => m.count_vertices() / 3,
        })
        .sum();

    let want = format!("{} tris drawn", with_thousands(total));
    for (mut text, mut colour) in &mut readout {
        if text.0 != want {
            text.0 = want.clone();
        }
        let tint = if total > HEAVY_SCENE {
            DANGER
        } else if total > BUSY_SCENE {
            ACCENT
        } else {
            TEXT_DIM
        };
        if colour.0 != tint {
            colour.0 = tint;
        }
    }
}

/// A triangle count at a glance: `1.5k`, `70k`, `4.2M`. Exact numbers belong in the total at the
/// bottom; a palette row needs an order of magnitude.
fn brief_count(n: usize) -> String {
    match n {
        0 => String::new(),
        n if n < 1_000 => format!("{n}"),
        n if n < 100_000 => format!("{:.1}k", n as f32 / 1_000.0),
        n if n < 1_000_000 => format!("{}k", n / 1_000),
        n => format!("{:.1}M", n as f32 / 1_000_000.0),
    }
}

/// Group a number for reading. The total is meant to be compared against itself over time, so the
/// digits matter.
fn with_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A piece worth a second look before filling a room with it, and one worth a first look.
///
/// Calibrated on the shipped kit rather than picked: its median is 1,526 triangles and its
/// second-densest piece is 7,996, so 20k is well clear of everything except the one genuine outlier.
fn cost_tint(triangles: usize) -> Color {
    if triangles > emerge_core::import::BUSY_TRIANGLES {
        DANGER
    } else if triangles > 5_000 {
        ACCENT
    } else {
        LABEL
    }
}

// ── placing ──────────────────────────────────────────────────────────────────────────────────────

fn snap(v: f32) -> f32 {
    (v / SNAP).round() * SNAP
}

/// Put a piece in the world — **through `emerge-bevy`**, which is also what the game uses.
///
/// The editor used to own this arithmetic. It cannot: a preview that disagrees with the runtime by
/// half a degree of `front` or a centimetre of `y_offset` is a preview that lies, and the only way to
/// be sure two places agree is for there to be one place. `emerge_bevy::spawn_descriptor` is it.
fn spawn_piece(
    commands: &mut Commands,
    assets: &AssetServer,
    d: &emerge_core::descriptor::Descriptor,
    at: (f32, f32),
    yaw: f32,
    origin: (f32, f32, f32),
) -> Option<Entity> {
    emerge_bevy::spawn_descriptor(
        commands,
        assets,
        d,
        // The editor draws before tags matter; the runtime resolves them from the library. Defaulting
        // here keeps the signature honest rather than pretending the editor knows.
        emerge_core::vocab::Masks::default(),
        at,
        yaw,
        // The editor authors in the map's own space, so the origin it draws at is the map's own.
        origin,
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
        if let Some(e) = spawn_piece(&mut commands, &assets, d, p.at, p.yaw, project.map.origin) {
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
    state.undo.push(Undo::Added { count: 1 });

    if let Some(e) = spawn_piece(&mut commands, &assets, &d, at, state.brush_yaw, project.map.origin) {
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
    placed: Query<(Entity, &Placement)>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {

    if keys::just_pressed(&keyboard, Action::Undo) {
        undo(&mut commands, &assets, &mut project, &mut state, &placed);
        return;
    }

    if keys::just_pressed(&keyboard, Action::Remove) && !hovered_ui.iter().any(|h| h.0) {
        delete_under_cursor(
            &mut commands,
            window,
            camera,
            &mut project,
            &mut state,
            &placed,
        );
        return;
    }

    if keys::just_pressed(&keyboard, Action::Save) {
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

    // **O pins or unpins the piece under the cursor.** A pin is what the solver routes around.
    if keys::just_pressed(&keyboard, Action::OwnToggle) && !hovered_ui.iter().any(|h| h.0) {
        toggle_pin(window, camera, &mut project, &mut state);
        return;
    }

    // **G continues the layout.** Learn the grammar from what is already placed, then fill the free
    // cells with more of it — see `emerge_core::grammar`.
    if keys::just_pressed(&keyboard, Action::Generate) {
        generate(&mut commands, &assets, &mut project, &mut state, &placed);
        return;
    }

    // **F floods.** From the cell under the cursor outward, stopping at anything already placed and
    // at the map's edge — see `crate::fill`.
    if keys::just_pressed(&keyboard, Action::Fill) && !hovered_ui.iter().any(|h| h.0) {
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

    let step = if keys::just_pressed(&keyboard, Action::AimRight) {
        YAW_STEP
    } else if keys::just_pressed(&keyboard, Action::AimLeft) {
        -YAW_STEP
    } else {
        0.0
    };
    if step != 0.0 {
        state.brush_yaw = (state.brush_yaw + step).rem_euclid(360.0);
    }
}

/// Remove the placement nearest the cursor, within a piece's own reach.
///
/// Nearest-within-a-radius rather than a picking ray: the pieces are GLB scenes with no colliders, and
/// a ray would need every one of them to be pickable — which would also make every one of them eat
/// the click that places the next piece. The radius is the brush cell, so "delete what I am pointing
/// at" means the same distance as "place one here".
fn delete_under_cursor(
    commands: &mut Commands,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
) {
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        return;
    };

    let reach = project
        .library
        .descriptors
        .get(state.brush)
        .map(|d| crate::fill::cell_extents(d, state.brush_yaw))
        .map(|(x, z)| x.max(z))
        .unwrap_or(crate::fill::MIN_CELL);

    // `sort`-free: one pass for the minimum, and ties broken by index so two pieces stacked exactly
    // cannot make the choice depend on iteration order.
    let mut best: Option<(usize, f32)> = None;
    for (i, p) in project.map.placements.iter().enumerate() {
        let d2 = (p.at.0 - hit.x).powi(2) + (p.at.1 - hit.z).powi(2);
        if d2 <= reach * reach && best.is_none_or(|(_, b)| d2 < b) {
            best = Some((i, d2));
        }
    }
    let Some((index, _)) = best else {
        state.status = "nothing here to remove".to_owned();
        return;
    };

    let removed = project.map.placements.remove(index);
    for (entity, marker) in placed {
        if marker.0 == removed.id {
            commands.entity(entity).despawn();
        }
    }
    project.dirty = true;
    state.status = format!("removed {}", removed.id);
    state.undo.push(Undo::Removed {
        index,
        placed: Box::new(removed),
    });
}

/// Take back the last edit.
fn undo(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
) {
    let Some(op) = state.undo.pop() else {
        state.status = "nothing to undo".to_owned();
        return;
    };
    match op {
        Undo::Added { count } => {
            let keep = project.map.placements.len().saturating_sub(count);
            let gone: Vec<String> = project.map.placements.drain(keep..).map(|p| p.id).collect();
            for (entity, marker) in placed {
                if gone.contains(&marker.0) {
                    commands.entity(entity).despawn();
                }
            }
            state.status = format!("undid {} placement(s)", gone.len());
        }
        Undo::Removed { index, placed: p } => {
            if let Some(d) = project.library.get(&p.descriptor).cloned() {
                if let Some(e) = spawn_piece(commands, assets, &d, p.at, p.yaw, project.map.origin) {
                    commands.entity(e).insert(Placement(p.id.clone()));
                }
            }
            state.status = format!("restored {}", p.id);
            // Back at its old index, so a location referring to it by position in the list is not
            // quietly re-pointed at its neighbour.
            let at = index.min(project.map.placements.len());
            project.map.placements.insert(at, *p);
        }
    }
    project.dirty = true;
}

/// Pin or unpin the placement nearest the cursor.
///
/// Unpinning is immediate; pinning asks for a reason first, because that is what the field is for.
fn toggle_pin(
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &mut Project,
    state: &mut EditorState,
) {
    let Some(index) = nearest_placement(window, camera, project, state) else {
        state.status = "nothing here to pin".to_owned();
        return;
    };
    let Some(p) = project.map.placements.get_mut(index) else {
        return;
    };
    if p.owned {
        p.owned = false;
        p.owned_because = None;
        project.dirty = true;
        state.status = format!("unpinned {}", p.id);
    } else {
        let id = p.id.clone();
        state.pinning = Some((index, String::new()));
        state.status = format!("why is {id} pinned? Enter to keep it, Esc to cancel");
    }
}

/// The placement nearest the cursor, within a brush cell — shared by pin and delete so "the thing I
/// am pointing at" means one distance rather than two.
fn nearest_placement(
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &Project,
    state: &EditorState,
) -> Option<usize> {
    let (window, camera) = (window?, camera?);
    let (cam, cam_tf) = *camera;
    let hit = cursor_ground(&window, cam, cam_tf)?;
    let reach = project
        .library
        .descriptors
        .get(state.brush)
        .map(|d| crate::fill::cell_extents(d, state.brush_yaw))
        .map(|(x, z)| x.max(z))
        .unwrap_or(crate::fill::MIN_CELL);

    let mut best: Option<(usize, f32)> = None;
    for (i, p) in project.map.placements.iter().enumerate() {
        let d2 = (p.at.0 - hit.x).powi(2) + (p.at.1 - hit.z).powi(2);
        if d2 <= reach * reach && best.is_none_or(|(_, b)| d2 < b) {
            best = Some((i, d2));
        }
    }
    best.map(|(i, _)| i)
}

/// Type the reason a cell is pinned.
fn pin_reason_keys(
    mut events: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    if state.pinning.is_none() {
        return;
    }
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let Some((index, reason)) = state.pinning.take() else {
                    return;
                };
                let reason = reason.trim().to_owned();
                if reason.is_empty() {
                    state.status = "a pin needs a reason; nothing was pinned".to_owned();
                    return;
                }
                let pinned_id = project.map.placements.get_mut(index).map(|p| {
                    p.owned = true;
                    p.owned_because = Some(reason.clone());
                    p.id.clone()
                });
                if let Some(id) = pinned_id {
                    project.dirty = true;
                    state.status = format!("pinned {id}: {reason}");
                }
            }
            Key::Escape => {
                state.pinning = None;
                state.status = "nothing pinned".to_owned();
            }
            Key::Backspace => {
                if let Some((_, r)) = state.pinning.as_mut() {
                    r.pop();
                }
            }
            Key::Space => {
                if let Some((_, r)) = state.pinning.as_mut() {
                    r.push(' ');
                }
            }
            Key::Character(c) => {
                if let Some((_, r)) = state.pinning.as_mut() {
                    r.push_str(c);
                }
            }
            _ => {}
        }
    }
}

/// **Continue the author's arrangement.** Learn, solve, replace the unpinned pieces.
fn generate(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
) {
    // One metre: the tile the kits are authored on, and coarse enough that a 32 m map is a grid the
    // solver finishes rather than 4,096 cells of half-metre noise.
    const CELL: f32 = 1.0;

    let grammar = match emerge_core::grammar::learn(&project.map, CELL) {
        Ok(g) => g,
        Err(e) => {
            state.status = e;
            return;
        }
    };
    state.seed = state.seed.wrapping_add(1);
    let mut n = state.next_id;
    let solved = match emerge_core::grammar::solve(&project.map, &grammar, CELL, state.seed, || {
        n += 1;
        format!("gen@{n}")
    }) {
        Ok(s) => s,
        Err(e) => {
            state.status = e;
            return;
        }
    };
    state.next_id = n;

    // Everything unpinned is the sketch; the solve is the drawing. Despawn it and rebuild.
    let doomed: Vec<String> = project
        .map
        .placements
        .iter()
        .filter(|p| !p.owned)
        .map(|p| p.id.clone())
        .collect();
    for (entity, marker) in placed {
        if doomed.contains(&marker.0) {
            commands.entity(entity).despawn();
        }
    }
    project.map.placements.retain(|p| p.owned);

    let count = solved.placements.len();
    for p in solved.placements {
        if let Some(d) = project.library.get(&p.descriptor).cloned() {
            if let Some(e) = spawn_piece(commands, assets, &d, p.at, p.yaw, project.map.origin) {
                commands.entity(e).insert(Placement(p.id.clone()));
            }
        }
        project.map.placements.push(p);
    }
    project.dirty = true;
    state.undo.push(Undo::Added { count });
    state.status = format!(
        "continued the layout: {count} placed around {} pinned cell(s), from {} prototype(s)",
        solved.owned_cells,
        grammar.len() - 1
    );
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
        if let Some(e) = spawn_piece(commands, assets, &brush, p.at, p.yaw, project.map.origin) {
            commands.entity(e).insert(Placement(p.id.clone()));
        }
        project.map.placements.push(p);
    }
    project.dirty = true;
    // **One undo entry for the whole fill.** A fill is one act to the person who performed it, and an
    // undo stack that made them press Ctrl+Z 1,408 times would be a stack that models the code rather
    // than the work.
    if count > 0 {
        state.undo.push(Undo::Added { count });
    }
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
    let yaw = emerge_bevy::draw_yaw(d, state.brush_yaw);

    let existing = ghosts.iter().find(|(_, g)| g.0 == state.brush).map(|(e, _)| e);
    for (e, g) in &ghosts {
        if g.0 != state.brush {
            commands.entity(e).despawn();
        }
    }

    match existing {
        Some(e) => {
            if let Ok(mut tf) = transforms.get_mut(e) {
                tf.translation = emerge_bevy::origin_of(d, at, project.map.origin);
                tf.rotation = Quat::from_rotation_y(yaw.to_radians());
            }
        }
        None => {
            if let Some(e) = spawn_piece(&mut commands, &assets, d, at, state.brush_yaw, project.map.origin) {
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
