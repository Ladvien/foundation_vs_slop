//! **The editor's shared furniture** — one palette, one panel, one key list.
//!
//! Every tab is the same two shapes: a controls panel down one side, and a list down the other. They
//! were written twice and were already drifting. The census key-row block appeared in `editor.rs` and
//! `tiles.rs` byte-for-byte, *including its five-line comment about `min_width`*, and rendered in two
//! different pairs of colours — the same list, a shade darker in one tab, which nobody chose. The
//! colour table itself was declared twice, with two of its entries carrying the same value under two
//! names (`TEXT_DIM`/`DIM`, `ROW_ARMED`/`ROW_SELECTED`), so a change to one tab's greys silently was
//! not a change to the other's.
//!
//! That is the failure `docs/ui.md` §3.5 records for key bindings, in a second place: a fact stated
//! more than once drifts. Ousterhout's *A Philosophy of Software Design* ch. 17 is the general form —
//! *"once you have learned how something is done in one place, you can use that knowledge to
//! immediately understand other places that use the same approach."* Here that is literal: a third tab
//! costs ~30 lines of panel code instead of ~110, and it cannot come out looking like a different
//! program.
//!
//! Nothing here knows what a map or a tile is. It knows what a panel looks like.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::ScrollArea;

use crate::keys::{self, Context};

// ── the palette ──────────────────────────────────────────────────────────────────────────────────

/// The panel's own ground. Opaque, not the game's translucent HUD panel: an editor panel is a work
/// surface, and a researcher in a white coat behind a translucent one is unreadable — measured.
pub const PANEL_BG: Color = Color::srgb(0.058, 0.054, 0.047);
/// A row at rest.
pub const ROW_BG: Color = Color::srgb(0.098, 0.092, 0.082);
/// A row that is armed, selected, or otherwise the one being acted on. **One name.** This was
/// `ROW_ARMED` in the map tab and `ROW_SELECTED` in the tiles tab, at the same value — two names for
/// one idea is two things to keep in step.
pub const ROW_SELECTED: Color = Color::srgb(0.30, 0.28, 0.24);
/// Body text: a value, an id, the thing that changes.
pub const TEXT: Color = Color::srgb(0.86, 0.84, 0.80);
/// Quieter body text. Was `TEXT_DIM` in one tab and `DIM` in the other, same value.
pub const DIM: Color = Color::srgb(0.58, 0.56, 0.53);
/// The heading and the live-edit colour.
pub const ACCENT: Color = Color::srgb(0.90, 0.66, 0.24);
/// The key column. Brighter than the description beside it, because the key is what you scan for.
pub const KEY: Color = Color::srgb(0.74, 0.71, 0.66);
/// A label column — quieter than its value, which is the thing that changes.
pub const LABEL: Color = Color::srgb(0.46, 0.44, 0.42);
/// A refusal, a blocking finding, an expensive number.
pub const DANGER: Color = Color::srgb(0.86, 0.36, 0.30);
/// Empty preview tile, so an un-baked row reads as "not yet" rather than as a hole in the panel.
/// `thumbs.rs` carries a third copy of this value as `BACKDROP`, for the booth's own background.
pub const SLOT_BG: Color = Color::srgb(0.14, 0.135, 0.125);
/// A group heading — quieter than a row, because it is a signpost rather than a thing to click on
/// most of the time.
pub const HEADER_BG: Color = Color::srgb(0.075, 0.070, 0.063);

// ── layout ───────────────────────────────────────────────────────────────────────────────────────

/// Where the panels start, below the tab strip. One number, so no two panels can disagree about it
/// and leave a tab half-covered.
pub const TAB_STRIP_BOTTOM: f32 = 46.0;
/// The gap every panel keeps from the window edge.
pub const MARGIN: f32 = 12.0;
/// Inside a panel.
pub const PAD: f32 = 12.0;

/// Width of a controls panel — two aligned text columns, so it is set by the widest chord plus its
/// label.
pub const CONTROLS_W: f32 = 300.0;
/// The tiles tab's controls panel is wider, and earns it: below the key list it carries four rows of
/// tag chips and a findings paragraph, neither of which is a two-column table. At `CONTROLS_W` the
/// chips wrapped to twice the height and pushed the findings off the bottom of the screen.
pub const TILES_CONTROLS_W: f32 = 380.0;
/// Width of a list panel. Narrower than the controls: a row here is a thumbnail, an id and a number
/// rather than two text columns. Both panels together leave a little under half the screen for the
/// map at `UiScale(1.2)`, which is the thing the right edge was chosen to protect.
pub const LIST_W: f32 = 264.0;

// ── spacing ──────────────────────────────────────────────────────────────────────────────────────
//
// **A scale, used as a set.** van den Berg, Cornelissen & Roerdink 2009 (`10.1167/9.4.24`) find that
// clutter is *crowding*, not element count: what makes a group read as a group is that its members
// sit closer to each other than to anything else. So what matters here is the RATIO between these,
// not their absolute values — a panel where every gap is 4 px has no groups, however few things are
// in it, and that is what the tiles detail block was.
//
// `docs/ui.md` §1.2 supplies the other half, and it cuts against reflexive minimalism: the test is
// Vicente & Rasmussen's *"does this force interpretation?"*, not "how many things are on screen",
// and Yang et al. 2017 measured more information **improving** performance. So the answer to a
// crowded panel is spacing and grouping, not deletion.

/// Between items that belong to each other — chips in one axis, cells in one row.
pub const GAP_TIGHT: f32 = 3.0;
/// Between the rows of one block.
pub const GAP_ROW: f32 = 5.0;
/// Between blocks. Several times [`GAP_TIGHT`], which is the whole point.
pub const GAP_GROUP: f32 = 16.0;

/// A block heading: quiet, and separated from what came before it.
///
/// The separation is the work — a heading with the same gap above it as below is a label that could
/// belong to either side.
pub fn section(parent: &mut ChildSpawnerCommands, label: &str) {
    parent.spawn((
        Text::new(label.to_owned()),
        TextColor(LABEL),
        TextFont::from_font_size(9.0),
        Node {
            margin: UiRect::top(Val::Px(GAP_GROUP)).with_bottom(Val::Px(GAP_TIGHT)),
            ..default()
        },
    ));
}

/// Which edge a panel is pinned to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// **A panel.** Absolutely positioned, below the tab strip, opaque, above the world.
///
/// `full_height` pins `bottom` as well as `top`, which is what gives a list inside it a real height to
/// scroll within: a `max_height` inside an unpinned panel is never reached and does nothing — the map
/// palette shipped that way and rendered two rows.
///
/// **Every panel carries `Hovered`.** `view::drive` and `place_on_click` both ask "is the pointer over
/// UI" by looking for any true `Hovered`, and when only the *rows* carried one the gaps between them
/// counted as open map — a wheel turn over a list zoomed the world, and a click that missed a row by a
/// pixel dropped a piece behind the panel. `Hovered` is true for an entity **or any descendant**
/// (`bevy_picking-0.19.0/src/hover.rs:322`), so one on the root answers for the whole surface.
pub fn panel_root<'a>(
    commands: &'a mut Commands,
    side: Side,
    width: f32,
    full_height: bool,
    hidden: bool,
) -> EntityCommands<'a> {
    let mut node = Node {
        position_type: PositionType::Absolute,
        top: Val::Px(TAB_STRIP_BOTTOM),
        width: Val::Px(width),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(6.0),
        padding: UiRect::all(Val::Px(PAD)),
        ..default()
    };
    match side {
        Side::Left => node.left = Val::Px(MARGIN),
        Side::Right => node.right = Val::Px(MARGIN),
    }
    if full_height {
        node.bottom = Val::Px(MARGIN);
    }
    if hidden {
        // `Display::None`, never `Visibility`: a visibility-hidden UI node still occupies layout and
        // still answers hover, which would leave a hidden tab's rows eating clicks aimed at the world.
        node.display = Display::None;
    }
    commands.spawn((
        node,
        BackgroundColor(PANEL_BG),
        GlobalZIndex(100),
        Hovered::default(),
    ))
}

/// A panel's heading.
pub fn title(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        Text::new(text.to_owned()),
        TextColor(ACCENT),
        TextFont::from_font_size(15.0),
    ));
}

/// **The key list, read from the census and never retyped.**
///
/// `docs/ui.md` §3.5 records what happens otherwise: key allocation lived in five prose censuses and
/// all five drifted to the same wrong answer. A panel that types its own key list is a sixth. This
/// renders `keys::rows`, so a binding that changes changes here — in every tab at once.
///
/// Two aligned columns rather than a run-on line: a run-on wraps unpredictably at any width, and the
/// eye finds a row in a table without reading the others.
pub fn key_census(parent: &mut ChildSpawnerCommands, contexts: &[Context]) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(1.0),
            margin: UiRect::top(Val::Px(2.0)),
            ..default()
        })
        .with_children(|list| {
            for row_def in contexts.iter().flat_map(|c| keys::rows(*c)) {
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
                        TextColor(DIM),
                        TextFont::from_font_size(11.0),
                    ));
                });
            }
        });
}

/// **A scrolling list that fills what is left of its panel.**
///
/// `flex_grow` with `min_height: 0`, not `max_height`: a flex item's automatic minimum size is its
/// content, which would grow the node to fit every row and leave `overflow` with nothing to clip. The
/// panel must be `full_height` for this to bound anything.
pub fn scroll_list(parent: &mut ChildSpawnerCommands, marker: impl Bundle) {
    parent.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        ScrollArea::default(),
        marker,
    ));
}

// A `LABEL  value` row builder belongs here too — the shape is repeated four times across the two
// tabs, differing only in the label column's width. It is not written yet because nothing has been
// moved onto it, and a builder with no caller is a stub.

/// The held-key overlay's root. `Display::None` when it is not being asked for.
#[derive(Component)]
pub struct ShortcutsOverlay;

/// Where the rows are rebuilt, so the frame around them is spawned once.
#[derive(Component)]
struct ShortcutsBody;

/// Which tab's list the overlay is currently showing, so it is rebuilt on a tab change and not on
/// every frame the key is held.
#[derive(Resource, Default)]
struct ShowingFor(Option<crate::tiles::Mode>);

/// **The key list, on demand.**
///
/// It used to be printed down the side of every panel: eighteen rows in the Tiles tab, above the
/// controls, which is what pushed the subgrid's own cell grid below the fold (`FVS-Q-11`). A key
/// list is *reference*, consulted rarely and never while acting — and `docs/ui.md` §1.2's test is
/// "does this force interpretation?", which a permanent eighteen-row table beside the thing you are
/// trying to use does.
///
/// So it is held rather than printed, and the panels carry one line saying so. That line is not
/// optional: Cockburn, Gutwin, Scarr & Malacria 2014 (`10.1145/2659796`, already cited by
/// `crate::keys`) document the intermodal-transition failure — a fast path offered *beside* a slow
/// one does not get adopted on its own. A hidden list nobody is told about is that failure exactly.
pub struct ChromePlugin;

impl Plugin for ChromePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowingFor>()
            .add_systems(Startup, spawn_shortcuts_overlay)
            .add_systems(Update, drive_shortcuts_overlay.in_set(keys::Phase::Act));
    }
}

fn spawn_shortcuts_overlay(mut commands: Commands) {
    commands
        .spawn((
            ShortcutsOverlay,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            // **Per `docs/ui.md` §5 trap 7.** A full-screen container without this eats every world
            // click; its children keep their own hit targets, which this one does not need anyway.
            bevy::picking::Pickable::IGNORE,
            // Above the panels and the tab strip (101), which is the whole point of an overlay.
            GlobalZIndex(400),
            // A scrim, so the list reads against whatever is behind it without hiding the map.
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(PAD * 1.5)),
                    row_gap: Val::Px(GAP_ROW),
                    ..default()
                },
                BackgroundColor(PANEL_BG),
                ShortcutsBody,
            ));
        });
}

/// Show the list while the key is down, and rebuild it when the tab under it changes.
fn drive_shortcuts_overlay(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mode: Res<crate::tiles::Mode>,
    mut showing: ResMut<ShowingFor>,
    mut roots: Query<&mut Node, With<ShortcutsOverlay>>,
    bodies: Query<Entity, With<ShortcutsBody>>,
) {
    let want = keys::pressed(&keyboard, live.0, keys::Action::Shortcuts);
    for mut node in &mut roots {
        let display = if want { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    // Rebuilt on the transition and on a tab change, never per frame: the rows are static text and
    // respawning them sixty times a second would be sixty times the work for one picture.
    let key = want.then_some(*mode);
    if showing.0 == key {
        return;
    }
    showing.0 = key;
    let Some(tab) = key else { return };
    for body in &bodies {
        commands.entity(body).despawn_related::<Children>();
        commands.entity(body).with_children(|p| {
            title(p, &format!("{} KEYS", tab.label()));
            // This tab first, then the frame around it — the same order the panels used, so anyone
            // who learned the old list finds the rows where they were.
            key_census(p, &[tab.context(), Context::Global]);
        });
    }
}

/// **One line where eighteen rows used to be.** See [`ChromePlugin`] for why it is not optional.
pub fn shortcut_hint(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Text::new(format!(
            "Hold {} for shortcuts",
            keys::chord(keys::Action::Shortcuts)
        )),
        TextColor(LABEL),
        TextFont::from_font_size(10.0),
        Node {
            margin: UiRect::top(Val::Px(2.0)),
            ..default()
        },
    ));
}
