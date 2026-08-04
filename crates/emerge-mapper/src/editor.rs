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
use bevy::ui_widgets::{Activate, Button as UiButton};
use emerge_core::map::Placed;

use crate::chrome::{
    ACCENT, DANGER, DIM, HEADER_BG, LABEL, LIST_W, PANEL_BG, ROW_BG, ROW_SELECTED, SLOT_BG,
    TEXT,
};
use crate::keys::{self, Action};
use crate::project::Project;
use crate::view::{cursor_ground, MainCamera};

/// Translation snap, metres. Half a metre is the unit the kits are authored on.
const SNAP: f32 = 0.5;
/// Yaw snap, degrees.
const YAW_STEP: f32 = 15.0;

/// Where a descriptor with no `kind` goes. Named rather than hidden: an untagged piece is work to do,
/// and a palette that quietly omitted it would be a palette missing pieces.
const UNSORTED: &str = "unsorted";
/// The map's edge. Dim enough not to compete with the grid, bright enough to find.
const BOUNDS_LINE: Color = Color::srgb(0.42, 0.38, 0.30);

/// The removal marker. Red because it is the one destructive tool here, and translucent because the
/// thing it covers is the thing being asked about — an opaque marker would hide the answer.
const REMOVE_TINT: Color = Color::srgba(0.86, 0.20, 0.16, 0.38);
/// How far the marker floats above the floor, metres. Enough to beat z-fighting against a floor tile
/// it is lying exactly on top of, small enough to still read as flat on the ground.
const MARKER_LIFT: f32 = 0.02;
/// A press-and-release inside this many metres is a click, not a drag — so "click one piece" and
/// "drag a box" are the same gesture told apart by distance rather than by a second modifier.
const CLICK_EPS: f32 = 0.2;

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
    /// The ghost's running commentary. Written every frame; never mixed with [`Self::status`].
    pub hint: String,
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
    /// Is the removal tool armed?
    ///
    /// A **bool here and the drag in [`RemovalDrag`]**, not one struct: `rebuild_palette` runs on
    /// `resource_changed::<EditorState>`, and a drag corner written every frame would tear the whole
    /// palette down and rebuild it at frame rate. This flips twice per use; that one lives elsewhere.
    pub removing: bool,
}

/// The rectangle being dragged out, in map space. Deliberately its own resource — see
/// [`EditorState::removing`] for why it is not a field on the state everything else watches.
#[derive(Resource, Default)]
pub struct RemovalDrag {
    /// Where the button went down, or `None` while only hovering.
    from: Option<(f32, f32)>,
}

/// The translucent red marker: the hovered piece's footprint, or the dragged rectangle.
#[derive(Component)]
struct RemovalTile;

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
    /// Put back everything one drag took out. **Ascending by index**, which is what lets them go
    /// back in that order and each land where it came from — an earlier row returning first shifts
    /// the later ones into place. One entry for the whole rectangle, on the same argument the fill
    /// makes: a box the author drew once is one act to undo once.
    RemovedMany { items: Vec<(usize, Box<Placed>)> },
}

impl Default for EditorState {
    fn default() -> Self {
        EditorState {
            brush: 0,
            brush_yaw: 0.0,
            status: String::new(),
            hint: String::new(),
            next_id: 0,
            seed: 1,
            collapsed: std::collections::HashSet::new(),
            pinning: None,
            renaming: None,
            undo: Vec::new(),
            removing: false,
        }
    }
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
}

/// A map may not be smaller than this on any axis, metres. One, not zero: a zero-width map has no
/// floor to point at, and `fill` would refuse every cell in it for a reason that reads like a bug.
const MIN_BOUND: u32 = 1;
/// And no larger. 512 m at 0.5 m cells is past what one fill can cover and far past what an author
/// can see; a typo of one extra digit should be refused rather than resolved into a map nobody meant.
const MAX_BOUND: u32 = 512;

/// One clickable size field.
#[derive(Component, Clone, Copy)]
struct SizeField(Axis);

/// Which size field is being typed into, and the digits so far.
///
/// Its own resource for the reason [`RemovalDrag`] is: `rebuild_palette` watches `EditorState`, and a
/// keystroke landing there would tear down and rebuild all forty-one palette rows per character.
#[derive(Resource, Default)]
pub struct SizeEdit {
    active: Option<(Axis, String)>,
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

/// **The grid step the map is read on**, metres.
///
/// Shared by `generate` and `check_edges` on purpose: the learned grammar and the edge check must
/// agree about which two placements are neighbours, or the tool would report a fault between pieces
/// the solver does not think touch.
const CELL: f32 = 1.0;

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
    /// **What the cursor is telling you right now**, as opposed to what just happened.
    ///
    /// Its own line because the ghost writes it EVERY FRAME while a surface piece hovers over bare
    /// floor, and sharing [`Self::Last`] meant every message an action produced was erased before it
    /// could be read. Two different questions — "what did I just do" and "why can't I place this" —
    /// so two lines.
    Hint,
    /// **Where the map disagrees with the tokens the tiles declare.**
    ///
    /// See `emerge_core::adjacency`. A standing readout rather than a message, because a fault is a
    /// state the map is in — it does not happen once and stop being true, so putting it on
    /// [`Self::Last`] would let the next action erase it.
    Edges,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Field::Edges => "EDGES",
            Field::Name => "NAME",
            Field::Brush => "BRUSH",
            Field::Yaw => "YAW",
            Field::Map => "MAP",
            Field::Last => "",
            Field::Hint => "",
        }
    }
}

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
            .init_resource::<RemovalDrag>()
            .init_resource::<SizeEdit>()
            .init_resource::<EdgeFaults>()
            // Shared by both tabs' lists, so it is registered once here rather than by whichever
            // plugin happens to build first.
            .init_resource::<crate::filter::Filters>()
            .add_systems(
                Update,
                (
                    crate::filter::keys.in_set(keys::Phase::Text),
                    crate::filter::refresh,
                ),
            )
            .add_observer(crate::filter::on_click)
            .add_systems(
                Startup,
                (
                    crate::thumbs::setup,
                    spawn_panel,
                    spawn_palette_panel,
                    spawn_cost_readout,
                    spawn_removal_tile,
                    spawn_existing,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    // **The fields run last and the actions run first**, so no census action can
                    // fire on a keystroke a field has already swallowed. See `keys::Phase`.
                    rename_keys.in_set(keys::Phase::Text),
                    pin_reason_keys.in_set(keys::Phase::Text),
                    size_edit_keys.in_set(keys::Phase::Text),
                    // No `not_typing`, no `in_map_mode`: `keys::just_pressed` now refuses on both
                    // counts, and a run condition repeating it would be a second census. Dropping
                    // `in_map_mode` also makes `Cmd+S` and `Cmd+Z` work from every tab, which is what
                    // the census has always said they do (`Context::Global`) and what the guard
                    // quietly contradicted.
                    keys.in_set(keys::Phase::Act),
                    // These two stay gated: they read the MOUSE, which the census does not model, so
                    // context is not their guard. Mode is.
                    place_on_click.run_if(not_typing).run_if(in_map_mode),
                    drive_removal.run_if(not_typing).run_if(in_map_mode),
                    drive_ghost.run_if(in_map_mode),
                    fade_ghost,
                    style_rows,
                    refresh_status,
                    rebuild_palette.run_if(
                        // `or_else`, not the deprecated `or`: 0.19 spells the lazy form this way,
                        // and this project has already paid for the eager one — every run condition
                        // being evaluated is what made a bare `Res<T>` behind an earlier `false`
                        // panic on launch.
                        resource_changed::<Project>
                            .or_else(resource_changed::<EditorState>)
                            .or_else(resource_changed::<crate::filter::Filters>)
                            .or_else(run_once),
                    ),
                    refresh_size,
                    refresh_triangle_total,
                    draw_bounds,
                    check_edges.run_if(resource_changed::<Project>.or_else(run_once)),
                    draw_edge_faults.run_if(in_map_mode),
                ),
            )
            .add_observer(on_row_click)
            .add_observer(on_category_click)
            .add_observer(on_size_field_click);
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
                TextColor(DIM),
                TextFont::from_font_size(11.0),
                TriangleTotal,
            ));
        });
}

/// Build the panel's fixed furniture. The palette itself is `rebuild_palette`'s, which is why neither
/// the project nor the thumbnails are read here — they were parameters that had stopped being used.
fn spawn_panel(mut commands: Commands) {
    let root = crate::chrome::panel_root(
        &mut commands,
        crate::chrome::Side::Left,
        crate::chrome::CONTROLS_W,
        false,
        false,
    )
    .insert(crate::tiles::MapRoot)
    .id();

    commands.entity(root).with_children(|p| {
        crate::chrome::title(p, "EMERGE MAPPER");
        crate::chrome::shortcut_hint(p);

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
            for field in [
                Field::Name,
                Field::Brush,
                Field::Yaw,
                Field::Map,
                Field::Last,
                Field::Hint,
                Field::Edges,
            ] {
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
                    // **A field, not a pair of nudges.** Stepping to 48 m from 32 was four clicks
                    // and no way to say the number; the author knows the size they want, so the
                    // control should let them state it. Click to focus, type digits, Enter to keep.
                    row.spawn((
                        UiButton,
                        Hovered::default(),
                        SizeField(axis),
                        Node {
                            width: Val::Px(56.0),
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(ROW_BG),
                    ))
                    .with_children(|f| {
                        f.spawn((
                            Text::new(""),
                            TextColor(TEXT),
                            TextFont::from_font_size(11.0),
                            SizeReadout(axis),
                        ));
                    });
                });
            }
        });

    });
}

/// **The palette, in its own panel against the right edge.**
///
/// It used to be the last child of the panel above, which meant it was handed whatever vertical space
/// the keys, the status block and the map-size rows had not already taken. At `UiScale(1.2)` on a
/// 1280×802 logical screen that is 668 UI px of budget against ~447 px of everything else — so the
/// list rendered *two* rows and the rest ran off the bottom of the screen. The `max_height` it carried
/// was never reached and never did anything.
///
/// A panel pinned to BOTH insets cannot have that failure: `top` and `bottom` together give the node a
/// height taken from the viewport rather than from its content, so the list has a bottom to stop at
/// and the scroll area is bounded by construction. Nothing above it can push it off-screen, because
/// there is no longer anything above it.
///
/// Right edge rather than a second column beside the keys: the left panel keeps its width, and the map
/// stays visible in the band between them, which is the thing being authored.
fn spawn_palette_panel(mut commands: Commands) {
    crate::chrome::panel_root(
        &mut commands,
        crate::chrome::Side::Right,
        LIST_W,
        // Pinned top AND bottom — the whole point of this panel.
        true,
        false,
    )
    // Tagged like the panel above so `tiles::apply_mode` hides it with the rest of the map tab — it
    // iterates every `MapRoot`, so a second one needs no change there. Without this the palette would
    // sit over the tiles tab offering pieces that tab cannot place.
    .insert(crate::tiles::MapRoot)
    .with_children(|p| {
        p.spawn((
            Text::new("PLACE"),
            TextColor(LABEL),
            TextFont::from_font_size(10.0),
        ));
        crate::filter::spawn(p, crate::filter::Pane::Palette);
        crate::chrome::scroll_list(p, PaletteList);
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
    filters: Res<crate::filter::Filters>,
    lists: Query<Entity, With<PaletteList>>,
) {
    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|p| {
            for (category, mut members) in categories(&project) {
                // **The filter narrows; it never reorders.** Rows that survive keep the positions
                // they had, so what an author learned about where a piece sits is still true —
                // Samp 2011, via `docs/ui.md` §3.5.
                members.retain(|ix| {
                    project
                        .library
                        .descriptors
                        .get(*ix)
                        .is_some_and(|d| filters.keeps(crate::filter::Pane::Palette, &d.id))
                });
                // A heading with nothing under it is a heading about nothing.
                if members.is_empty() {
                    continue;
                }
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
fn on_size_field_click(
    activate: On<Activate>,
    fields: Query<&SizeField>,
    mut edit: ResMut<SizeEdit>,
    mut state: ResMut<EditorState>,
) {
    let Ok(field) = fields.get(activate.entity) else {
        return;
    };
    // **Starts empty, and says so.** Seeding it with the current number meant the first digit typed
    // appended to it — 32 became 328 — and it looked like it had worked. `rename_keys` records the
    // same trap and the same answer: a field with no selection model starts blank, and the value it
    // is replacing is still on screen until Enter.
    edit.active = Some((field.0, String::new()));
    state.status = format!(
        "{} size: type a whole number of metres, Enter to keep it, Esc to leave it alone",
        field.0.label()
    );
}

/// Digits, and nothing else.
///
/// Filtered at the keystroke rather than validated at commit: a field that accepts `4.5` and then
/// refuses it has taught the author it was allowed. This one simply never shows a character that
/// cannot be part of the answer.
fn size_edit_keys(
    mut events: MessageReader<KeyboardInput>,
    mut edit: ResMut<SizeEdit>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    if edit.active.is_none() {
        return;
    }
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let Some((axis, raw)) = edit.active.take() else {
                    return;
                };
                if raw.is_empty() {
                    state.status = "nothing typed; the size is unchanged".to_owned();
                    return;
                }
                let Ok(want) = raw.parse::<u32>() else {
                    // Unreachable while the filter below holds, and stated rather than assumed: an
                    // overlong run of digits overflows `u32` and that is a refusal, not a panic.
                    state.status = format!("`{raw}` is not a whole number of metres");
                    return;
                };
                if !(MIN_BOUND..=MAX_BOUND).contains(&want) {
                    state.status =
                        format!("a map axis runs {MIN_BOUND}..{MAX_BOUND} m; `{want}` is outside it");
                    return;
                }
                let mut bounds = project.map.bounds;
                axis.set(&mut bounds, want as f32);
                if bounds != project.map.bounds {
                    project.map.bounds = bounds;
                    project.dirty = true;
                }
                state.status = format!(
                    "map is {:.0} x {:.0} x {:.0} m",
                    bounds.0, bounds.1, bounds.2
                );
            }
            Key::Escape => {
                edit.active = None;
                state.status = "size unchanged".to_owned();
            }
            Key::Backspace => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    raw.pop();
                }
            }
            Key::Character(s) => {
                if let Some((_, raw)) = edit.active.as_mut() {
                    // Room for `MAX_BOUND`'s three digits and no more, so the buffer cannot grow
                    // into something `u32` has to refuse later.
                    if s.chars().all(|c| c.is_ascii_digit()) && raw.len() < 3 {
                        raw.push_str(s);
                    }
                }
            }
            _ => {}
        }
    }
}

fn refresh_size(
    project: Res<Project>,
    edit: Res<SizeEdit>,
    mut readouts: Query<(&SizeReadout, &mut Text, &mut TextColor)>,
    mut fields: Query<(&SizeField, &mut BackgroundColor)>,
) {
    for (readout, mut text, mut colour) in &mut readouts {
        // While a field is being typed into it shows what has been typed, with a caret — so the
        // number on screen is always the number Enter would commit, never the one being replaced.
        let editing = match &edit.active {
            Some((axis, raw)) if *axis == readout.0 => Some(raw),
            _ => None,
        };
        let (want, want_colour) = match editing {
            Some(raw) => (format!("{raw}_"), ACCENT),
            None => {
                let v = readout.0.get(project.map.bounds);
                // Whole metres now that the field only accepts them. A map loaded with a fractional
                // bound still reads truthfully rather than being silently rounded on screen.
                let text = if (v - v.round()).abs() < 1e-3 {
                    format!("{v:.0}")
                } else {
                    format!("{v:.1}")
                };
                (text, TEXT)
            }
        };
        if text.0 != want {
            text.0 = want;
        }
        if colour.0 != want_colour {
            colour.0 = want_colour;
        }
    }

    for (field, mut bg) in &mut fields {
        let focused = matches!(&edit.active, Some((axis, _)) if *axis == field.0);
        let want = if focused { SLOT_BG } else { ROW_BG };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

/// Draw the map's extent as a wireframe box.
///
/// Gizmos rather than a mesh: the bounds are a statement about the map, not a thing in it, and a real
/// box would be pickable, shadow-casting, and something a click could land on.
/// **Every place the map disagrees with the tiles' declared edge tokens.**
///
/// Recomputed only when the project changes — it is O(placements^2) in the worst case and the answer
/// cannot move while nothing has been placed, turned or removed.
#[derive(Resource, Default)]
pub struct EdgeFaults(pub Vec<emerge_core::adjacency::Fault>);

fn check_edges(project: Res<Project>, mut faults: ResMut<EdgeFaults>) {
    faults.0 = emerge_core::adjacency::faults(&project.map, &project.library, CELL);
}

/// Outline both halves of every fault, so the sentence in the panel has something to point at.
///
/// A gizmo rather than a spawned marker: it is derived from a resource that is already recomputed on
/// change, so there is nothing to keep in step and nothing to clean up.
fn draw_edge_faults(
    faults: Res<EdgeFaults>,
    project: Res<Project>,
    heights: Query<(&Placement, &Transform)>,
    mut gizmos: Gizmos,
) {
    if faults.0.is_empty() {
        return;
    }
    for fault in &faults.0 {
        for id in [fault.a.as_str(), fault.b.as_str()] {
            if id.is_empty() {
                continue;
            }
            let Some((_, tf)) = heights.iter().find(|(p, _)| p.0 == id) else {
                continue;
            };
            let footprint = project
                .map
                .placements
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| project.library.get(&p.descriptor))
                .and_then(|d| d.extent.footprint)
                .unwrap_or((CELL, CELL));
            gizmos.rect(
                Isometry3d::new(
                    tf.translation + Vec3::Y * 0.02,
                    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                ),
                Vec2::new(footprint.0, footprint.1),
                FAULT_LINE,
            );
        }
    }
}

/// The colour a disagreeing face is outlined in — the palette's refusal colour, because a fault is
/// the map and the tokens contradicting each other and that is a thing to fix, not a warning.
const FAULT_LINE: Color = DANGER;

fn draw_bounds(project: Res<Project>, mut gizmos: Gizmos) {
    // `floor_rect` is the floor PLAN — map space, centred on zero. Drawing happens in the world, so
    // the origin goes back on here.
    let (min_x, min_z, max_x, max_z) = project.map.floor_rect();
    let (floor, ceiling) = project.map.height_span();
    let (w, h, d) = project.map.bounds;
    let centre = Vec3::new(
        project.map.origin.0 + (min_x + max_x) * 0.5,
        (floor + ceiling) * 0.5,
        project.map.origin.2 + (min_z + max_z) * 0.5,
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
            ROW_SELECTED
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
    faults: Res<EdgeFaults>,
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
            // Silent when the map agrees with itself, which is the common case and the one a standing
            // line must not shout about. `docs/ui.md` §1.2: more information helps, noise does not.
            Field::Edges => match faults.0.len() {
                0 => ("ok".to_owned(), DIM),
                1 => (faults.0[0].message.clone(), DANGER),
                n => (
                    format!("{n} — {}", faults.0[0].message),
                    DANGER,
                ),
            },
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
            Field::Hint => (state.hint.clone(), ACCENT),
            Field::Last => (
                state.status.clone(),
                if state.status.starts_with("NOT SAVED") {
                    DANGER
                } else {
                    DIM
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
/// **Nothing that reads a tab's keys may fire while a field is taking them.**
///
/// This is [`crate::keys::Context::Typing`] in the census's terms — the context that overlaps every
/// other one and suppresses all of them.
///
/// **One condition, every field.** There were two (`not_typing` here and `not_renaming_candidate` in
/// `tiles.rs`), each knowing about the fields of its own tab, and a filter box would have made three:
/// a system gated on the wrong one fires while you type, which is how `2` in a text box lands you on
/// another tab. Every field is listed here and nowhere else, so adding one is adding a line.
///
/// The buffers stay with their fields — a name, a pin's reason, an axis, a candidate id and a filter
/// hold different things and commit differently. That is five kinds of state, not one fact written
/// five times, so this reads them rather than owning them.
pub fn not_typing(
    state: Res<EditorState>,
    edit: Res<SizeEdit>,
    import: Res<crate::tiles::ImportState>,
    filters: Res<crate::filter::Filters>,
    div: Res<crate::tiles::DivEdit>,
    cell: Res<crate::tiles::CellEdit>,
    note: Res<crate::tiles::NoteEdit>,
) -> bool {
    state.renaming.is_none()
        && state.pinning.is_none()
        && edit.active.is_none()
        && import.renaming.is_none()
        && !filters.typing()
        && !div.typing()
        && !cell.typing()
        && !note.typing()
}

/// **Decide who owns the keyboard, once, before anything reads a key.**
///
/// Runs in [`keys::Phase::Sense`], which is ordered before both the action systems and the text
/// fields. That ordering is not tidiness — see [`keys::Phase`] for the defect it fixes, in which a
/// text field cleared its own flag mid-frame and the run condition guarding `Enter` re-evaluated to
/// true behind it, importing six descriptors into `library.ron` by accident.
///
/// This reads exactly the fields [`not_typing`] reads, and for the same reason: the buffers stay with
/// their fields, so adding a field is adding a line *here* — the one list — rather than a new guard
/// somebody else has to remember to consult.
pub fn sense_context(
    mode: Res<crate::tiles::Mode>,
    state: Res<EditorState>,
    edit: Res<SizeEdit>,
    import: Res<crate::tiles::ImportState>,
    filters: Res<crate::filter::Filters>,
    div: Res<crate::tiles::DivEdit>,
    cell: Res<crate::tiles::CellEdit>,
    note: Res<crate::tiles::NoteEdit>,
    mut live: ResMut<keys::Live>,
) {
    let typing = state.renaming.is_some()
        || state.pinning.is_some()
        || edit.active.is_some()
        || import.renaming.is_some()
        || filters.typing()
        || div.typing()
        || cell.typing()
        || note.typing();
    let want = keys::Live(keys::live(mode.context(), typing));
    // Written through the change detector only when it actually moves, so `Live` staying put does not
    // wake every `resource_changed` reader in the editor every frame.
    if *live != want {
        *live = want;
    }
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
    live: Res<keys::Live>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    if state.renaming.is_none() {
        // `N` starts a rename. Read from the buffered events like everything else here, so a keypress
        // cannot both start the rename and be typed into it.
        if keys::just_pressed(&keyboard, live.0, Action::RenameMap) {
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
                    // The modifier from the census, not typed: this sentence naming a key the build
                    // does not read is the same failure as the panel doing it.
                    state.status = format!(
                        "renamed `{was}` to `{name}` ({}+S writes the new file)",
                        keys::MOD_NAME
                    );
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
            DIM
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

/// A world ground point as a snapped **map-space** `at`.
///
/// The conversion has to happen somewhere, and here is the only somewhere: `cursor_ground` answers in
/// world metres and `Placed::at` is in map space. They agree only for a map at the origin — which is
/// every map the editor has ever authored, so writing world coordinates straight into `at` looked
/// right for as long as nobody moved a map.
fn map_at(project: &Project, hit: Vec3) -> (f32, f32) {
    let (x, z) = project.map.to_map_space((hit.x, hit.z));
    (snap(x), snap(z))
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
    y: f32,
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
        y,
    )
}

/// **Where every piece in the map stands.** Resolved together, because a lamp's height is its table's.
///
/// Returns the reason rather than a partial answer. A map whose stacking will not resolve is one an
/// author has to be told about — drawing it half-right is how a lamp ends up looking badly authored
/// when the real problem is the shelf it names.
fn heights(project: &Project) -> Result<Vec<f32>, String> {
    emerge_core::stack::resolve_y(&project.map, &project.library)
}

/// Draw the placements from `first` onward — what a fill or a generate just added.
///
/// The map is already complete when this runs, which is the point: heights are resolved once, over the
/// finished map, so a piece added by the solver stands wherever the finished map says it does.
fn spawn_range(
    commands: &mut Commands,
    assets: &AssetServer,
    project: &Project,
    state: &mut EditorState,
    first: usize,
) {
    let ys = match heights(project) {
        Ok(ys) => ys,
        Err(e) => {
            state.status = e.clone();
            error!("{e}");
            return;
        }
    };
    for (i, p) in project.map.placements.iter().enumerate().skip(first) {
        let (Some(d), Some(&y)) = (project.library.get(&p.descriptor), ys.get(i)) else {
            continue;
        };
        if let Some(e) = spawn_piece(commands, assets, d, p.at, p.yaw, project.map.origin, y) {
            commands.entity(e).insert(Placement(p.id.clone()));
        }
    }
}

/// The removal marker, spawned once and then only moved.
///
/// Once, because a mesh and a material built per frame are a new asset per frame — the editor would
/// grow a handle a tick until it ran out of memory, which is the shape of bug that looks like "it
/// gets slow after a while" rather than like a leak.
fn spawn_removal_tile(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        RemovalTile,
        // A unit rectangle, scaled to whatever it has to cover. `Rectangle` is authored in the XY
        // plane, so it is laid flat by a quarter turn about X in the system below.
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: REMOVE_TINT,
            // **Unlit and blended.** Lit, the marker's brightness would report where the key light
            // happens to be rather than what it means; opaque, it would hide the piece it is
            // pointing at, which is the one thing the author is trying to look at.
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// **The removal tool**: show what would go, and take it when the button comes up.
///
/// Preview and commit in one system because they are one question asked twice — the rectangle the
/// marker draws IS the rectangle the release removes, and computing it in two places is how a
/// selection box comes to disagree with what it deletes.
#[allow(clippy::too_many_arguments)]
fn drive_removal(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    placed: Query<(Entity, &Placement)>,
    mut marker: Query<(&mut Transform, &mut Visibility), With<RemovalTile>>,
    mut drag: ResMut<RemovalDrag>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    // Read, never write, unless something actually happened: `state` is watched by
    // `rebuild_palette`, so an unconditional deref here rebuilds the palette every frame.
    if !state.removing {
        if drag.from.is_some() {
            drag.from = None;
        }
        for (_, mut vis) in &mut marker {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        }
        return;
    }

    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    // A cursor over the panel is not a cursor over the map, and a marker under the palette is a
    // promise about a click that will never reach the world — the same rule the ghost follows.
    let hit = cursor_ground(&window, cam, cam_tf).filter(|_| !hovered_ui.iter().any(|h| h.0));
    let Some(hit) = hit else {
        for (_, mut vis) in &mut marker {
            *vis = Visibility::Hidden;
        }
        return;
    };
    let at = project.map.to_map_space((hit.x, hit.z));

    if mouse.just_pressed(MouseButton::Left) {
        drag.from = Some(at);
    }

    // What the marker covers: the box being dragged, or — before a drag starts — the footprint of
    // the piece that a click would take, which is what makes "this one" a claim rather than a guess.
    let rect = match drag.from {
        Some(from) => Some((
            from.0.min(at.0),
            from.1.min(at.1),
            from.0.max(at.0),
            from.1.max(at.1),
        )),
        None => pick_at(&project, at).and_then(|i| {
            let p = project.map.placements.get(i)?;
            let d = project.library.get(&p.descriptor)?;
            let (w, depth) = crate::fill::cell_extents(d, p.yaw);
            Some((
                p.at.0 - w * 0.5,
                p.at.1 - depth * 0.5,
                p.at.0 + w * 0.5,
                p.at.1 + depth * 0.5,
            ))
        }),
    };

    for (mut tf, mut vis) in &mut marker {
        match rect {
            Some((x0, z0, x1, z1)) => {
                *vis = Visibility::Visible;
                *tf = Transform::from_xyz(
                    project.map.origin.0 + (x0 + x1) * 0.5,
                    project.map.origin.1 + MARKER_LIFT,
                    project.map.origin.2 + (z0 + z1) * 0.5,
                )
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                // A floor of a few centimetres so a zero-area box is still visible as a box.
                .with_scale(Vec3::new((x1 - x0).max(0.05), (z1 - z0).max(0.05), 1.0));
            }
            None => *vis = Visibility::Hidden,
        }
    }

    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some(from) = drag.from.take() else {
        return;
    };

    if (from.0 - at.0).abs() <= CLICK_EPS && (from.1 - at.1).abs() <= CLICK_EPS {
        match pick_at(&project, at) {
            Some(i) => delete_index(&mut commands, i, &mut project, &mut state, &placed),
            None => state.status = "nothing here to remove".to_owned(),
        }
        return;
    }

    let (x0, z0) = (from.0.min(at.0), from.1.min(at.1));
    let (x1, z1) = (from.0.max(at.0), from.1.max(at.1));
    let doomed: Vec<usize> = project
        .map
        .placements
        .iter()
        .enumerate()
        .filter(|(_, p)| p.at.0 >= x0 && p.at.0 <= x1 && p.at.1 >= z0 && p.at.1 <= z1)
        .map(|(i, _)| i)
        .collect();
    if doomed.is_empty() {
        state.status = "nothing inside that box".to_owned();
        return;
    }

    // **Taken back to front.** Removing an earlier row shifts every later one down, so a forward
    // pass would delete the wrong pieces the moment the box held more than one.
    let mut items: Vec<(usize, Box<Placed>)> = Vec::with_capacity(doomed.len());
    for i in doomed.iter().rev() {
        let removed = project.map.placements.remove(*i);
        for (entity, mark) in &placed {
            if mark.0 == removed.id {
                commands.entity(entity).despawn();
            }
        }
        items.push((*i, Box::new(removed)));
    }
    // Ascending again, which is the order `Undo::RemovedMany` puts them back in.
    items.reverse();

    let n = items.len();
    state.undo.push(Undo::RemovedMany { items });
    project.dirty = true;
    // The whole chord, rendered by the census — naming just the modifier told the author to press
    // `Cmd`, which is not a thing anyone can do.
    state.status = format!(
        "removed {n} placement(s) — {} puts them back",
        keys::chord_text(keys::binding(Action::Undo))
    );
}

/// Bring up whatever the map already holds.
fn spawn_existing(mut commands: Commands, assets: Res<AssetServer>, project: Res<Project>) {
    let ys = match heights(&project) {
        Ok(ys) => ys,
        Err(e) => {
            error!("{e}");
            return;
        }
    };
    for (i, p) in project.map.placements.iter().enumerate() {
        let Some(d) = project.library.get(&p.descriptor) else {
            // Loud, not silent: a placement naming a descriptor the library does not have is a hole
            // in the map, and an author must be told which one rather than counting missing crates.
            warn!(
                "placement `{}` names descriptor `{}`, which this library does not have",
                p.id, p.descriptor
            );
            continue;
        };
        let Some(&y) = ys.get(i) else { continue };
        if let Some(e) = spawn_piece(&mut commands, &assets, d, p.at, p.yaw, project.map.origin, y) {
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
    // While the removal tool is armed a click removes; it must not also place, or a box dragged over
    // a crowded corner would delete what was there and leave a new piece behind it.
    if state.removing {
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
    let at = map_at(&project, hit);

    // **What it lands on.** A piece that mounts on a surface must find one under the cursor; the same
    // question the ghost has been answering while the author moved the mouse here, asked once more at
    // the moment it matters.
    let ys = match heights(&project) {
        Ok(ys) => ys,
        Err(e) => {
            state.status = e;
            return;
        }
    };
    let (y, host) =
        match emerge_core::stack::placement_at(&project.map, &project.library, &ys, &d, at) {
            Ok(found) => found,
            // Refused, not floored. Dropping it at floor level is the behaviour this replaced: the
            // piece appears, in the wrong place, and looks like an authoring mistake.
            Err(e) => {
                state.status = e;
                return;
            }
        };
    let on = host.map(|h| h.id.clone());

    state.next_id += 1;
    let id = format!("{}@{}", short_id(&d.id), state.next_id);

    let placed = Placed {
        id: id.clone(),
        descriptor: d.id.clone(),
        at,
        yaw: state.brush_yaw,
        on: on.clone(),
        ..Placed::default()
    };
    project.map.placements.push(placed);
    project.dirty = true;
    state.undo.push(Undo::Added { count: 1 });

    if let Some(e) = spawn_piece(
        &mut commands,
        &assets,
        &d,
        at,
        state.brush_yaw,
        project.map.origin,
        y,
    ) {
        commands.entity(e).insert(Placement(id.clone()));
    }
    state.status = match on {
        Some(host) => format!("placed {id} on {host}"),
        None => format!("placed {id} at ({}, {})", at.0, at.1),
    };
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
    live: Res<keys::Live>,
    assets: Res<AssetServer>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    placed: Query<(Entity, &Placement)>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {

    if keys::just_pressed(&keyboard, live.0, Action::Undo) {
        undo(&mut commands, &assets, &mut project, &mut state, &placed);
        return;
    }

    // **The delete key arms a tool; it does not delete.** Removing on the keypress meant the only
    // preview of what was about to go was the author's memory of where the cursor was. Now the key
    // turns the mode on, the red marker answers "this one", and a click or a dragged box commits.
    if keys::just_pressed(&keyboard, live.0, Action::Remove) {
        state.removing = !state.removing;
        state.status = if state.removing {
            format!(
                "removal mode: click a piece, or drag a box. {} or Esc to stop.",
                keys::REMOVE_NAME
            )
        } else {
            "removal mode off".to_owned()
        };
        return;
    }

    if keys::just_pressed(&keyboard, live.0, Action::Cancel) && state.removing {
        state.removing = false;
        state.status = "removal mode off".to_owned();
        return;
    }

    if keys::just_pressed(&keyboard, live.0, Action::Save) {
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

    // **`,` and `.` turn the piece under the cursor.** The other half of aiming: `[`/`]` set the
    // brush's facing before a piece exists, and these fix one that is already down — three chairs
    // round a table were three chairs facing the same way until this existed.
    for (action, step) in [
        (Action::TurnPieceLeft, -YAW_STEP),
        (Action::TurnPieceRight, YAW_STEP),
    ] {
        if keys::just_pressed(&keyboard, live.0, action) && !hovered_ui.iter().any(|h| h.0) {
            turn_under_cursor(
                &mut commands,
                &assets,
                window,
                camera,
                &mut project,
                &mut state,
                &placed,
                step,
            );
            return;
        }
    }

    // **O pins or unpins the piece under the cursor.** A pin is what the solver routes around.
    if keys::just_pressed(&keyboard, live.0, Action::OwnToggle) && !hovered_ui.iter().any(|h| h.0) {
        toggle_pin(window, camera, &mut project, &mut state);
        return;
    }

    // **G continues the layout.** Learn the grammar from what is already placed, then fill the free
    // cells with more of it — see `emerge_core::grammar`.
    if keys::just_pressed(&keyboard, live.0, Action::Generate) {
        generate(&mut commands, &assets, &mut project, &mut state, &placed);
        return;
    }

    // **F floods.** From the cell under the cursor outward, stopping at anything already placed and
    // at the map's edge — see `crate::fill`.
    if keys::just_pressed(&keyboard, live.0, Action::Fill) && !hovered_ui.iter().any(|h| h.0) {
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

    // **X puts the brush back where it started.** Turning is relative, so a piece three quarters round
    // is one press from straight in one direction and three in the other — and an author who has been
    // tapping `Z` has no reason to be keeping count. This is the only absolute among the aim keys.
    if keys::just_pressed(&keyboard, live.0, Action::AimReset) {
        state.brush_yaw = 0.0;
        state.status = "brush aimed straight again".to_owned();
        return;
    }

    let step = if keys::just_pressed(&keyboard, live.0, Action::AimRight) {
        YAW_STEP
    } else if keys::just_pressed(&keyboard, live.0, Action::AimLeft) {
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
fn delete_index(
    commands: &mut Commands,
    index: usize,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
) {
    if index >= project.map.placements.len() {
        state.status = "nothing here to remove".to_owned();
        return;
    }
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
        Undo::RemovedMany { items } => {
            let n = items.len();
            // Rows back first, all of them, and only THEN drawn: how high a piece sits is a question
            // about the finished map, so asking `heights` mid-restore would answer it against a map
            // that is still missing some of its own contents.
            let mut at_indices = Vec::with_capacity(n);
            for (index, p) in items {
                let at = index.min(project.map.placements.len());
                project.map.placements.insert(at, *p);
                at_indices.push(at);
            }
            match heights(project) {
                Ok(ys) => {
                    for at in at_indices {
                        let Some(p) = project.map.placements.get(at) else {
                            continue;
                        };
                        let (id, pat, pyaw) = (p.id.clone(), p.at, p.yaw);
                        let (Some(d), Some(&y)) =
                            (project.library.get(&p.descriptor).cloned(), ys.get(at))
                        else {
                            continue;
                        };
                        if let Some(e) =
                            spawn_piece(commands, assets, &d, pat, pyaw, project.map.origin, y)
                        {
                            commands.entity(e).insert(Placement(id));
                        }
                    }
                    state.status = format!("restored {n} placement(s)");
                }
                Err(e) => {
                    state.status = format!("restored {n} but cannot draw them: {e}");
                    error!("{e}");
                }
            }
        }
        Undo::Removed { index, placed: p } => {
            state.status = format!("restored {}", p.id);
            let id = p.id.clone();
            let descriptor = p.descriptor.clone();
            // Back at its old index, so a location referring to it by position in the list is not
            // quietly re-pointed at its neighbour. **Into the map before it is drawn**, because what
            // it rests on decides how high it goes and that is a question about the map.
            let at = index.min(project.map.placements.len());
            project.map.placements.insert(at, *p);

            // Drawn from the finished map. A failure here is loud rather than a piece that is in the
            // file and not on the screen — the two disagreeing is exactly the state an author cannot
            // see and would go on editing around.
            match heights(project) {
                Ok(ys) => {
                    let d = project.library.get(&descriptor).cloned();
                    let placed = project.map.placements.get(at).map(|q| (q.at, q.yaw));
                    if let (Some(d), Some((pat, pyaw)), Some(&y)) = (d, placed, ys.get(at)) {
                        if let Some(e) =
                            spawn_piece(commands, assets, &d, pat, pyaw, project.map.origin, y)
                        {
                            commands.entity(e).insert(Placement(id));
                        }
                    }
                }
                Err(e) => {
                    state.status = format!("restored {id} but cannot draw it: {e}");
                    error!("{e}");
                }
            }
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
    let Some(index) = nearest_placement(window, camera, project) else {
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

/// Turn the placement under the cursor by `step` degrees, and redraw it.
///
/// Rewrites the yaw in the map and respawns the entity rather than rotating the transform in place:
/// the file is the truth and the entity is a picture of it, so turning the picture without turning the
/// file is the class of bug where a save loses what you just did.
#[allow(clippy::too_many_arguments)]
fn turn_under_cursor(
    commands: &mut Commands,
    assets: &AssetServer,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &mut Project,
    state: &mut EditorState,
    placed: &Query<(Entity, &Placement)>,
    step: f32,
) {
    let Some(index) = nearest_placement(window, camera, project) else {
        state.status = "nothing here to turn".to_owned();
        return;
    };
    let Some(p) = project.map.placements.get_mut(index) else {
        return;
    };
    p.yaw = (p.yaw + step).rem_euclid(360.0);
    let (id, at, yaw, descriptor) = (p.id.clone(), p.at, p.yaw, p.descriptor.clone());
    project.dirty = true;

    for (entity, marker) in placed {
        if marker.0 == id {
            commands.entity(entity).despawn();
        }
    }
    // Redrawn from the finished map, so a piece standing on this one keeps the height it had.
    match heights(project) {
        Ok(ys) => {
            if let (Some(d), Some(&y)) = (project.library.get(&descriptor).cloned(), ys.get(index)) {
                if let Some(e) = spawn_piece(commands, assets, &d, at, yaw, project.map.origin, y) {
                    commands.entity(e).insert(Placement(id.clone()));
                }
            }
        }
        Err(e) => {
            state.status = format!("turned {id} but cannot draw it: {e}");
            error!("{e}");
            return;
        }
    }
    state.status = format!("{id} now at {yaw:.0} deg");
}

/// **The thing you are pointing at**, given a probe in map space.
///
/// Pure, and separated from the cursor for exactly that reason: the rule below is the whole content
/// of "which piece did I mean", and proving it through a window means aiming a synthetic mouse at a
/// 0.45 m chair — which is how the last two sessions lost an hour each to the harness rather than to
/// the code.
///
/// Two passes, and the first is the fix. It used to be distance-to-`at` alone, which reads as "the
/// nearest centre" and is wrong the moment anything stands on anything: a lamp on a table has almost
/// exactly the table's `at`, so pointing at the lamp could delete the table. That is not
/// hypothetical — it happened while authoring `break_room.map.ron`.
///
/// 1. **Anything whose footprint contains the probe**, smallest first. The piece you can see least of
///    is the one you must have been aiming at.
/// 2. Failing that, the nearest centre within **its own** reach — its own, not the brush's. The reach
///    used to come from whatever piece was armed, so how far you could grab a chair depended on what
///    you happened to be holding. What you are pointing at is not a property of what you are carrying.
///
/// Ties break on the placement id, a total order that does not depend on authoring order.
pub fn pick_at(project: &Project, probe: (f32, f32)) -> Option<usize> {
    let mut covering: Option<(usize, f32, &str)> = None;
    for (i, p) in project.map.placements.iter().enumerate() {
        let Some(d) = project.library.get(&p.descriptor) else {
            continue;
        };
        if !emerge_core::stack::covers(d, p.at, p.yaw, probe) {
            continue;
        }
        let area = d
            .extent
            .footprint
            .map_or(f32::INFINITY, |(w, depth)| w * depth);
        let better = match covering {
            None => true,
            Some((_, best_area, best_id)) => (area, p.id.as_str()) < (best_area, best_id),
        };
        if better {
            covering = Some((i, area, p.id.as_str()));
        }
    }
    if let Some((i, _, _)) = covering {
        return Some(i);
    }

    let mut nearest: Option<(usize, f32, &str)> = None;
    for (i, p) in project.map.placements.iter().enumerate() {
        let reach = project
            .library
            .get(&p.descriptor)
            .map(|d| crate::fill::cell_extents(d, p.yaw))
            .map_or(crate::fill::MIN_CELL, |(x, z)| x.max(z));
        let d2 = (p.at.0 - probe.0).powi(2) + (p.at.1 - probe.1).powi(2);
        if d2 > reach * reach {
            continue;
        }
        let better = match nearest {
            None => true,
            Some((_, best_d2, best_id)) => (d2, p.id.as_str()) < (best_d2, best_id),
        };
        if better {
            nearest = Some((i, d2, p.id.as_str()));
        }
    }
    nearest.map(|(i, _, _)| i)
}

/// [`pick_at`], with the probe taken from the cursor — shared by pin, delete and turn, so "the thing
/// I am pointing at" means one thing.
fn nearest_placement(
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    project: &Project,
) -> Option<usize> {
    let (window, camera) = (window?, camera?);
    let (cam, cam_tf) = *camera;
    let hit = cursor_ground(&window, cam, cam_tf)?;
    // Both sides in map space: `at` is authored there and the cursor answers in world metres.
    pick_at(project, project.map.to_map_space((hit.x, hit.z)))
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

    // **Into the map first, drawn second.** The solver lays pieces on the floor grid; how high each
    // one ends up is a question about the finished map, so the map has to be finished before it is
    // asked.
    let count = solved.placements.len();
    let first = project.map.placements.len();
    project.map.placements.extend(solved.placements);
    spawn_range(commands, assets, project, state, first);
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
        project.map.to_map_space((hit.x, hit.z)),
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
    let first = project.map.placements.len();
    project.map.placements.extend(filled.placements);
    spawn_range(commands, assets, project, state, first);
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
    // Mutable for one reason: the status line is where "there is no worktop here" belongs, and the
    // moment the author needs it is while the cursor is over the spot, not after the click.
    mut state: ResMut<EditorState>,
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
    // hovering under the palette is a promise about a click that will never reach the world. Nor
    // while removing — two previews for two different outcomes under one cursor is a question, not
    // an answer.
    if hovered_ui.iter().any(|h| h.0) || state.removing {
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

    let at = map_at(&project, hit);
    let yaw = emerge_bevy::draw_yaw(d, state.brush_yaw);

    // **The ghost stands where the piece would.** A lamp dragged over a table rises onto it, so the
    // author sees the answer before committing to it rather than placing and then wondering. When
    // there is no surface under a piece that needs one there is nothing truthful to draw — showing it
    // on the floor would be a preview of something that will not happen.
    let ys = match heights(&project) {
        Ok(ys) => ys,
        Err(_) => {
            clear(&mut commands);
            return;
        }
    };
    let (y, _) = match emerge_core::stack::placement_at(&project.map, &project.library, &ys, d, at) {
        Ok(found) => found,
        // **The reason, while the cursor is still there.** `docs/ui.md` §1.4: an unmet condition is an
        // instruction. A ghost that simply vanishes over bare floor reads as the editor being broken;
        // the sentence says which surface the piece wants and leaves the author holding the answer.
        Err(e) => {
            clear(&mut commands);
            // **Only when it changes.** This runs every frame the cursor sits over bare floor with a
            // surface piece armed, and `ResMut` marks the resource changed on every mutable deref —
            // `rebuild_palette` watches `resource_changed::<EditorState>`, so an unconditional write
            // here would tear down and rebuild the whole palette at frame rate.
            if state.hint != e {
                state.hint = e;
            }
            return;
        }
    };
    // The piece can go here, so there is nothing to warn about.
    if !state.hint.is_empty() {
        state.hint.clear();
    }

    let existing = ghosts.iter().find(|(_, g)| g.0 == state.brush).map(|(e, _)| e);
    for (e, g) in &ghosts {
        if g.0 != state.brush {
            commands.entity(e).despawn();
        }
    }

    match existing {
        Some(e) => {
            if let Ok(mut tf) = transforms.get_mut(e) {
                tf.translation = emerge_bevy::origin_of(at, project.map.origin, y);
                tf.rotation = Quat::from_rotation_y(yaw.to_radians());
            }
        }
        None => {
            if let Some(e) = spawn_piece(
                &mut commands,
                &assets,
                d,
                at,
                state.brush_yaw,
                project.map.origin,
                y,
            ) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use emerge_core::descriptor::{Descriptor, Extent};
    use emerge_core::library::{Library, LIBRARY_VERSION};
    use emerge_core::map::{Map, Placed};

    fn piece(id: &str, w: f32, d: f32) -> Descriptor {
        Descriptor {
            id: id.to_owned(),
            mesh: Some(format!("{id}.glb")),
            extent: Extent {
                footprint: Some((w, d)),
                height: Some(1.0),
            },
            ..Descriptor::default()
        }
    }

    fn at(id: &str, descriptor: &str, at: (f32, f32)) -> Placed {
        Placed {
            id: id.to_owned(),
            descriptor: descriptor.to_owned(),
            at,
            ..Placed::default()
        }
    }

    fn project(descriptors: Vec<Descriptor>, placements: Vec<Placed>) -> Project {
        Project {
            root: std::path::PathBuf::from("."),
            emerge_dir: std::path::PathBuf::from("assets/emerge"),
            library_path: std::path::PathBuf::from("assets/emerge/library.ron"),
            vocab: emerge_core::vocab::Vocabularies::default(),
            library: Library {
                version: LIBRARY_VERSION,
                note: None,
                descriptors,
            },
            masks: Vec::new(),
            map: Map {
                name: "test_map".into(),
                placements,
                ..Map::default()
            },
            map_path: std::path::PathBuf::from("test_map.map.ron"),
            dirty: false,
            triangles: Vec::new(),
        }
    }

    /// **The bug this rule was rewritten for.** A lamp on a table shares the table's `at`, so
    /// "nearest centre" could hand back the table when the cursor was squarely on the lamp — and did,
    /// while authoring `break_room.map.ron`.
    #[test]
    fn pointing_at_a_lamp_on_a_table_picks_the_lamp() {
        let p = project(
            vec![piece("table", 1.6, 0.8), piece("lamp", 0.3, 0.3)],
            vec![at("t1", "table", (0.0, 0.0)), at("l1", "lamp", (0.0, 0.0))],
        );
        assert_eq!(pick_at(&p, (0.0, 0.0)), Some(1), "the smaller thing wins");
        // Off the lamp but still on the table, the table is what you are pointing at.
        assert_eq!(pick_at(&p, (0.6, 0.0)), Some(0));
    }

    /// **Reach belongs to the target, not the brush.** Pointing at bare floor beside a piece still
    /// grabs it; how far that reaches must not depend on what happens to be armed.
    #[test]
    fn a_near_miss_still_grabs_the_piece_beside_it() {
        let p = project(
            vec![piece("crate", 1.0, 1.0)],
            vec![at("c1", "crate", (0.0, 0.0))],
        );
        assert_eq!(pick_at(&p, (0.0, 0.0)), Some(0), "dead centre");
        assert_eq!(pick_at(&p, (0.6, 0.0)), Some(0), "just outside, still its cell");
        assert_eq!(pick_at(&p, (9.0, 9.0)), None, "across the room is nothing");
    }

    /// Two identical pieces at the same distance resolve the same way every time — by id, which is a
    /// total order that does not depend on which was authored first.
    #[test]
    fn a_tie_resolves_by_id_rather_than_by_authoring_order() {
        let forwards = project(
            vec![piece("crate", 1.0, 1.0)],
            vec![at("b", "crate", (2.0, 0.0)), at("a", "crate", (-2.0, 0.0))],
        );
        let backwards = project(
            vec![piece("crate", 1.0, 1.0)],
            vec![at("a", "crate", (-2.0, 0.0)), at("b", "crate", (2.0, 0.0))],
        );
        let id = |p: &Project, i: Option<usize>| {
            i.and_then(|i| p.map.placements.get(i)).map(|q| q.id.clone())
        };
        // Equidistant from the origin, so only the id can break it — and it breaks the same way
        // whichever order the file lists them in.
        assert_eq!(
            id(&forwards, pick_at(&forwards, (0.0, 0.0))),
            id(&backwards, pick_at(&backwards, (0.0, 0.0)))
        );
    }

    /// A piece with no measured footprint covers nothing, so it cannot swallow every click.
    #[test]
    fn an_unmeasured_piece_does_not_cover_the_map() {
        let mut vague = piece("mystery", 1.0, 1.0);
        vague.extent.footprint = None;
        let p = project(vec![vague], vec![at("m1", "mystery", (0.0, 0.0))]);
        // Not covering, and its fallback reach is the minimum cell rather than infinity.
        assert_eq!(pick_at(&p, (9.0, 9.0)), None);
    }
}
