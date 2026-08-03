//! **Import mode** — what the measuring half has to say, where a person can read it.
//!
//! `emerge_core::import` does the work; this is the panel. Tab switches between placing pieces and
//! bringing new ones in, because they are different jobs with different controls and one panel trying
//! to hold both would be a panel that does neither well.
//!
//! # The scan is lazy and says how big it was
//!
//! This project ships 360 meshes and 41 are in the library, so the candidate list is around 319. That
//! is a second of file reading, and doing it at launch would make every session pay for a mode most
//! of them never open. It happens on the first Tab, and the panel reports what it found — a list of
//! 319 with no count is a list nobody trusts they have seen the end of.
//!
//! # Findings are shown with their fix
//!
//! A warning that does not say what to do about it is a warning that gets read once. Every
//! [`emerge_core::import::Finding`] that has an obvious remedy carries it, and the panel shows both.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton, ScrollArea};
use emerge_core::descriptor::{mount_label, mount_options};
use emerge_core::import::{self, Candidate, Severity};

use crate::project::Project;

/// Which job the editor is doing.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Map,
    Import,
}

#[derive(Resource, Default)]
pub struct ImportState {
    pub candidates: Vec<Candidate>,
    pub selected: usize,
    /// Whether the directory has been walked yet. Separate from `candidates.is_empty()`, which is
    /// also true of a directory with nothing new in it — and those two states want different words.
    pub scanned: bool,
    /// What the last scan found. **Persistent**, and separate from [`Self::status`] on purpose: the
    /// first version had one field, so "319 mesh(es) not in the library" was replaced by "layer: on
    /// support" the moment anyone did anything, and the one number that says whether you have seen
    /// the whole list was gone for the rest of the session.
    pub summary: String,
    /// The last thing that happened. Transient, and lives at the bottom where a changing line belongs.
    pub status: String,
    /// The raw text being typed into the candidate's id, or `None` when not renaming. Snake case is
    /// applied for display and on commit, exactly as the map's name is — one rule, one behaviour.
    pub renaming: Option<String>,
}

impl ImportState {
    pub fn current(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected)
    }
}

/// Root of the import panel, shown and hidden with the mode.
#[derive(Component)]
struct ImportRoot;

/// Root of the map panel, so the mode can hide it.
#[derive(Component)]
pub struct MapRoot;

/// One tag chip: which axis, and which token.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
struct TagChip {
    axis: Axis,
    /// Index into that axis's token table. The token itself lives in the vocabulary; carrying an
    /// index rather than a `String` keeps the component `Copy` and cannot drift from the table.
    token: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Kind,
    Effects,
    Look,
    /// What this piece OFFERS a top for — the two-sided axis.
    Surfaces,
}

impl Axis {
    fn label(self) -> &'static str {
        match self {
            Axis::Kind => "KIND",
            Axis::Effects => "DOES",
            Axis::Look => "LOOKS",
            Axis::Surfaces => "OFFERS",
        }
    }
    fn tokens<'a>(self, v: &'a emerge_core::vocab::Vocabularies) -> &'a emerge_core::vocab::Vocabulary {
        match self {
            Axis::Kind => &v.kind,
            Axis::Effects => &v.effects,
            Axis::Look => &v.look,
            Axis::Surfaces => &v.surfaces,
        }
    }
    fn list<'a>(self, d: &'a mut emerge_core::descriptor::Descriptor) -> &'a mut Vec<String> {
        match self {
            Axis::Kind => &mut d.kind,
            Axis::Effects => &mut d.effects,
            Axis::Look => &mut d.look,
            Axis::Surfaces => &mut d.offers.surfaces,
        }
    }
}

/// One candidate row, carrying its index.
#[derive(Component, Clone, Copy)]
struct CandidateRow(usize);

/// The node the candidate list is rebuilt into.
#[derive(Component)]
struct CandidateList;

/// The node the selected candidate's detail is rebuilt into.
#[derive(Component)]
struct DetailPane;

/// The candidate standing on the grid, so an author can see what they are about to accept.
#[derive(Component)]
struct Preview;

/// Which candidate the live preview shows, so it is rebuilt only when the selection actually moves —
/// respawning a GLB every frame would thrash the asset server and never finish loading.
#[derive(Component)]
struct PreviewOf(usize);

/// The persistent line saying what the scan found.
#[derive(Component)]
struct ScanSummary;

/// The transient line saying what just happened.
#[derive(Component)]
struct ActionLine;

const PANEL_BG: Color = Color::srgb(0.058, 0.054, 0.047);
const ROW_BG: Color = Color::srgb(0.098, 0.092, 0.082);
const ROW_SELECTED: Color = Color::srgb(0.30, 0.28, 0.24);
const TEXT: Color = Color::srgb(0.86, 0.84, 0.80);
const DIM: Color = Color::srgb(0.58, 0.56, 0.53);
const LABEL: Color = Color::srgb(0.46, 0.44, 0.42);
const ACCENT: Color = Color::srgb(0.90, 0.66, 0.24);
const DANGER: Color = Color::srgb(0.86, 0.36, 0.30);
/// The measured footprint — what the placement rules reserve.
const FOOTPRINT: Color = Color::srgb(0.35, 0.72, 0.85);
/// The grid cells it occupies. Where this and the footprint differ is the tiling slack.
const CELLS: Color = Color::srgb(0.42, 0.38, 0.30);
/// The volume, so a height is seen rather than only read.
const EXTENT: Color = Color::srgb(0.24, 0.42, 0.50);

pub struct ImportUiPlugin;

impl Plugin for ImportUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mode>()
            .init_resource::<ImportState>()
            .add_systems(Startup, spawn_import_panel)
            .add_systems(
                Update,
                (
                    toggle_mode,
                    rename_candidate,
                    move_selection.run_if(not_renaming_candidate),
                    cycle_mount.run_if(not_renaming_candidate),
                    apply_mode,
                    rebuild_candidates.run_if(resource_changed::<ImportState>),
                    rebuild_detail.run_if(resource_changed::<ImportState>),
                    refresh_lines,
                    drive_preview,
                    draw_preview_footprint.run_if(in_import_mode),
                ),
            )
            .add_observer(on_candidate_click)
            .add_observer(on_tag_chip);
    }
}

fn spawn_import_panel(mut commands: Commands) {
    commands
        .spawn((
            ImportRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(12.0),
                width: Val::Px(380.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(12.0)),
                // Starts hidden: the editor opens in map mode.
                display: Display::None,
                ..default()
            },
            BackgroundColor(PANEL_BG),
            GlobalZIndex(100),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("IMPORT"),
                TextColor(ACCENT),
                TextFont::from_font_size(15.0),
            ));
            for (chord, what) in [
                ("Tab", "back to the map"),
                ("up down", "choose"),
                ("I", "type an id"),
                ("M", "layer"),
                ("R", "rescan"),
            ] {
                p.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Node {
                            width: Val::Px(64.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(chord),
                        TextColor(DIM),
                        TextFont::from_font_size(11.0),
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    ));
                    row.spawn((
                        Text::new(what),
                        TextColor(LABEL),
                        TextFont::from_font_size(11.0),
                    ));
                });
            }

            p.spawn((
                Text::new(""),
                TextColor(DIM),
                TextFont::from_font_size(10.0),
                Node {
                    margin: UiRect::top(Val::Px(6.0)),
                    ..default()
                },
                ScanSummary,
            ));

            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    max_height: Val::Px(300.0),
                    overflow: Overflow::scroll_y(),
                    margin: UiRect::top(Val::Px(4.0)),
                    ..default()
                },
                ScrollArea::default(),
                CandidateList,
            ));

            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    margin: UiRect::top(Val::Px(8.0)),
                    ..default()
                },
                DetailPane,
            ));

            p.spawn((
                Text::new(""),
                TextColor(ACCENT),
                TextFont::from_font_size(10.0),
                Node {
                    margin: UiRect::top(Val::Px(8.0)),
                    ..default()
                },
                ActionLine,
            ));
        });
}

/// Tab swaps the job. `R` rescans, because meshes arrive while the editor is open — an importer that
/// only sees what was on disk at launch is one you have to restart to use.
fn toggle_mode(
    keys: Res<ButtonInput<KeyCode>>,
    project: Res<Project>,
    mut mode: ResMut<Mode>,
    mut state: ResMut<ImportState>,
) {
    let want_scan = if keys.just_pressed(KeyCode::Tab) {
        *mode = match *mode {
            Mode::Map => Mode::Import,
            Mode::Import => Mode::Map,
        };
        *mode == Mode::Import && !state.scanned
    } else {
        *mode == Mode::Import && keys.just_pressed(KeyCode::KeyR)
    };

    if want_scan {
        scan(&project, &mut state);
    }
}

fn scan(project: &Project, state: &mut ImportState) {
    let root = project.root.join("assets");
    match import::scan(&root, &root, &project.library) {
        Ok(found) => {
            let blocked = found.iter().filter(|c| c.blocked()).count();
            let warned = found
                .iter()
                .filter(|c| c.worst() == Some(Severity::Warn))
                .count();
            state.summary = format!(
                "{} mesh(es) not in the library — {warned} with warnings, {blocked} unmeasurable",
                found.len()
            );
            state.candidates = found;
            state.selected = 0;
            state.scanned = true;
        }
        Err(e) => {
            state.summary = e;
            state.scanned = true;
        }
    }
}

fn not_renaming_candidate(state: Res<ImportState>) -> bool {
    state.renaming.is_none()
}

/// Type an id for the selected candidate. Same rule as the map's name and the same behaviour: the
/// spelling is forced as you type, and the field starts EMPTY so the first keystroke replaces rather
/// than appends.
fn rename_candidate(
    mut events: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<Mode>,
    mut state: ResMut<ImportState>,
) {
    if *mode != Mode::Import {
        return;
    }
    if state.renaming.is_none() {
        if keys.just_pressed(KeyCode::KeyI) && state.current().is_some() {
            state.renaming = Some(String::new());
            state.status = "type an id — Enter to keep it, Esc to leave it alone".to_owned();
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
                let id = emerge_core::naming::to_snake_case(&raw);
                if id.is_empty() {
                    state.status = "an id cannot be empty; nothing was changed".to_owned();
                } else {
                    let at = state.selected;
                    if let Some(c) = state.candidates.get_mut(at) {
                        c.proposed.id = id.clone();
                    }
                    state.status = format!("id is `{id}`");
                }
            }
            Key::Escape => {
                state.renaming = None;
                state.status = "id unchanged".to_owned();
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

/// `M` steps the layer this piece goes on.
///
/// A cycle rather than a menu because there are nine of them and the list is short enough to walk;
/// the label says where you are, so nobody has to count presses.
fn cycle_mount(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<Mode>,
    project: Res<Project>,
    mut state: ResMut<ImportState>,
) {
    if *mode != Mode::Import || !keys.just_pressed(KeyCode::KeyM) {
        return;
    }
    let surfaces: Vec<String> = project.vocab.surfaces.names().map(str::to_owned).collect();
    let options = mount_options(&surfaces);
    let at = state.selected;
    let Some(c) = state.candidates.get_mut(at) else {
        return;
    };
    let next = c
        .proposed
        .mount
        .as_ref()
        .and_then(|m| options.iter().position(|o| o == m))
        .map_or(0, |i| (i + 1) % options.len());
    c.proposed.mount = Some(options[next].clone());
    state.status = format!("layer: {}", mount_label(c.proposed.mount.as_ref()));
}

/// Toggle one token on one axis.
fn on_tag_chip(
    activate: On<Activate>,
    chips: Query<&TagChip>,
    project: Res<Project>,
    mut state: ResMut<ImportState>,
) {
    let Ok(chip) = chips.get(activate.entity) else {
        return;
    };
    let Some(token) = chip
        .axis
        .tokens(&project.vocab)
        .names()
        .nth(chip.token)
        .map(str::to_owned)
    else {
        return;
    };
    let at = state.selected;
    let Some(c) = state.candidates.get_mut(at) else {
        return;
    };
    let list = chip.axis.list(&mut c.proposed);
    match list.iter().position(|t| *t == token) {
        Some(i) => {
            list.remove(i);
        }
        // Kept in vocabulary order rather than click order, so two descriptors with the same tags
        // serialize identically and a diff of the library shows real changes only.
        None => {
            list.push(token.clone());
            let order: Vec<String> = chip
                .axis
                .tokens(&project.vocab)
                .names()
                .map(str::to_owned)
                .collect();
            list.sort_by_key(|t| order.iter().position(|o| o == t).unwrap_or(usize::MAX));
        }
    }
    state.status = format!("{} tags updated", chip.axis.label().to_lowercase());
}

fn move_selection(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<Mode>,
    mut state: ResMut<ImportState>,
) {
    if *mode != Mode::Import || state.candidates.is_empty() {
        return;
    }
    let last = state.candidates.len() - 1;
    if keys.just_pressed(KeyCode::ArrowDown) && state.selected < last {
        state.selected += 1;
    }
    if keys.just_pressed(KeyCode::ArrowUp) && state.selected > 0 {
        state.selected -= 1;
    }
}

fn on_candidate_click(
    activate: On<Activate>,
    rows: Query<&CandidateRow>,
    mut state: ResMut<ImportState>,
) {
    if let Ok(row) = rows.get(activate.entity) {
        state.selected = row.0;
    }
}

/// Show one panel and hide the other. `Display::None` rather than `Visibility`, because a hidden-by-
/// visibility UI node still occupies layout and still answers hover — which would leave the map
/// panel's rows eating clicks aimed at the world.
fn apply_mode(
    mode: Res<Mode>,
    mut import_root: Query<&mut Node, (With<ImportRoot>, Without<MapRoot>)>,
    mut map_root: Query<&mut Node, (With<MapRoot>, Without<ImportRoot>)>,
) {
    if !mode.is_changed() {
        return;
    }
    let (import_shown, map_shown) = match *mode {
        Mode::Map => (Display::None, Display::Flex),
        Mode::Import => (Display::Flex, Display::None),
    };
    for mut node in &mut import_root {
        node.display = import_shown;
    }
    for mut node in &mut map_root {
        node.display = map_shown;
    }
}

fn in_import_mode(mode: Res<Mode>) -> bool {
    *mode == Mode::Import
}

/// Keep one preview alive, showing the selected candidate at the origin with its PROPOSED alignment
/// applied.
///
/// Proposed, not raw: the whole question an author is answering here is "will this sit right when the
/// game places it", and showing the mesh as exported answers a different one. A candidate whose origin
/// is 2 m off its base looks wrong in the file and correct here, which is the importer saying "I have
/// a fix for that" in the only language that settles it.
fn drive_preview(
    mut commands: Commands,
    mode: Res<Mode>,
    assets: Res<AssetServer>,
    state: Res<ImportState>,
    previews: Query<(Entity, &PreviewOf), With<Preview>>,
) {
    let clear = |commands: &mut Commands| {
        for (e, _) in &previews {
            commands.entity(e).despawn();
        }
    };
    if *mode != Mode::Import {
        clear(&mut commands);
        return;
    }
    let Some(c) = state.current() else {
        clear(&mut commands);
        return;
    };
    // A blocked candidate has no trustworthy alignment, so a preview of it would be a picture of a
    // guess. The findings say why; an empty grid is the honest illustration.
    if c.blocked() {
        clear(&mut commands);
        return;
    }

    for (e, of) in &previews {
        if of.0 != state.selected {
            commands.entity(e).despawn();
        }
    }
    if previews.iter().any(|(_, of)| of.0 == state.selected) {
        return;
    }

    let Some(mesh) = c.proposed.mesh.as_ref() else {
        return;
    };
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(mesh.clone()));
    let a = &c.proposed.align;
    // The pivot shifts the model so its bounding-box centre lands on the placement point, which is
    // what makes the symmetric footprint an accurate reservation rather than an approximation.
    let pivot = a.pivot.unwrap_or((0.0, 0.0));
    commands
        .spawn((
            Preview,
            PreviewOf(state.selected),
            Transform::from_xyz(-pivot.0, a.y_offset.unwrap_or(0.0), -pivot.1)
                .with_scale(Vec3::splat(a.scale.unwrap_or(1.0))),
            Visibility::Inherited,
        ))
        .with_child((WorldAssetRoot(scene), Transform::default()));
}

/// Draw the footprint the placement rules will reserve, and the grid cells it occupies.
///
/// Two rectangles, deliberately: the measured footprint, and the cells a flood fill would step on.
/// Where they differ is exactly the gap-or-overlap the findings describe in words, and a number in a
/// sentence is much easier to skip than a line that plainly does not meet its neighbour.
fn draw_preview_footprint(state: Res<ImportState>, mut gizmos: Gizmos) {
    let Some(c) = state.current().filter(|c| !c.blocked()) else {
        return;
    };
    let Some((w, d)) = c.proposed.extent.footprint else {
        return;
    };
    let height = c.proposed.extent.height.unwrap_or(0.0);

    // The mesh's own footprint, at the floor.
    gizmos.rect(
        Isometry3d::new(Vec3::new(0.0, 0.005, 0.0), Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        Vec2::new(w, d),
        FOOTPRINT,
    );
    // The cells it will actually occupy.
    let (cx, _) = emerge_core::grid::cells(w);
    let (cz, _) = emerge_core::grid::cells(d);
    gizmos.rect(
        Isometry3d::new(Vec3::new(0.0, 0.01, 0.0), Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        Vec2::new(cx as f32 * emerge_core::grid::SNAP, cz as f32 * emerge_core::grid::SNAP),
        CELLS,
    );
    // And the volume, so height is visible rather than only stated.
    if height > 0.0 {
        gizmos.cube(
            Transform::from_xyz(0.0, height * 0.5, 0.0).with_scale(Vec3::new(w, height, d)),
            EXTENT,
        );
    }
}

/// The two one-line readouts. Cheap enough every frame, and guarded so they only write on change.
fn refresh_lines(
    state: Res<ImportState>,
    mut summaries: Query<&mut Text, (With<ScanSummary>, Without<ActionLine>)>,
    mut actions: Query<&mut Text, (With<ActionLine>, Without<ScanSummary>)>,
) {
    for mut t in &mut summaries {
        if t.0 != state.summary {
            t.0 = state.summary.clone();
        }
    }
    for mut t in &mut actions {
        if t.0 != state.status {
            t.0 = state.status.clone();
        }
    }
}

/// Rebuild the candidate list.
///
/// Wholesale rather than diffed: it changes on a rescan and on nothing else, and a diffing rebuild of
/// a 319-row list would be more code than the thing it saves.
fn rebuild_candidates(
    mut commands: Commands,
    state: Res<ImportState>,
    lists: Query<Entity, With<CandidateList>>,
) {
    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|p| {
            if state.candidates.is_empty() {
                p.spawn((
                    Text::new(if state.scanned {
                        "every mesh under assets/ is already in the library"
                    } else {
                        "press Tab to scan"
                    }),
                    TextColor(DIM),
                    TextFont::from_font_size(11.0),
                ));
                return;
            }
            for (ix, c) in state.candidates.iter().enumerate() {
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    CandidateRow(ix),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        flex_direction: FlexDirection::Row,
                        ..default()
                    },
                    BackgroundColor(if ix == state.selected {
                        ROW_SELECTED
                    } else {
                        ROW_BG
                    }),
                ))
                .with_children(|row| {
                    // The severity mark first, so a list of 300 can be skimmed for the ones that
                    // need attention rather than read.
                    row.spawn((
                        Node {
                            width: Val::Px(14.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(match c.worst() {
                            Some(Severity::Blocking) => "x",
                            Some(Severity::Warn) => "!",
                            _ => "",
                        }),
                        TextColor(match c.worst() {
                            Some(Severity::Blocking) => DANGER,
                            Some(Severity::Warn) => ACCENT,
                            _ => LABEL,
                        }),
                        TextFont::from_font_size(11.0),
                    ));
                    row.spawn((
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        Text::new(c.mesh.clone()),
                        TextColor(TEXT),
                        TextFont::from_font_size(10.0),
                    ));
                });
            }
        });
    }
}

/// Rebuild the detail for whichever candidate is selected.
fn rebuild_detail(
    mut commands: Commands,
    state: Res<ImportState>,
    project: Res<Project>,
    panes: Query<Entity, With<DetailPane>>,
) {
    for pane in &panes {
        commands.entity(pane).despawn_related::<Children>();
        commands.entity(pane).with_children(|p| {
            let Some(c) = state.current() else {
                return;
            };

            // The id, showing what is being typed when it is being typed — with a caret, so an
            // empty field reads as "waiting for you" rather than as the id having been wiped.
            let (id_text, id_tint) = match &state.renaming {
                Some(raw) => (
                    format!("id  {}_", emerge_core::naming::to_snake_case(raw)),
                    ACCENT,
                ),
                None => (format!("id  {}", c.proposed.id), TEXT),
            };
            p.spawn((
                Text::new(id_text),
                TextColor(id_tint),
                TextFont::from_font_size(12.0),
                Node {
                    margin: UiRect::top(Val::Px(6.0)),
                    ..default()
                },
            ));

            if let Some(m) = c.measured {
                let (cells_x, _) = emerge_core::grid::cells(m.footprint.0);
                let (cells_z, _) = emerge_core::grid::cells(m.footprint.1);
                for (label, value) in [
                    (
                        "size",
                        format!(
                            "{:.2} x {:.2} x {:.2} m",
                            m.footprint.0, m.height, m.footprint.1
                        ),
                    ),
                    ("cells", format!("{cells_x} x {cells_z}")),
                    ("tris", format!("{}", c.triangles)),
                    (
                        "front",
                        match c.proposed.align.front {
                            Some(yaw) => format!("{yaw:.0} deg"),
                            None => "none".to_owned(),
                        },
                    ),
                ] {
                    p.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: Val::Px(48.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            Text::new(label),
                            TextColor(LABEL),
                            TextFont::from_font_size(10.0),
                        ));
                        row.spawn((
                            Text::new(value),
                            TextColor(TEXT),
                            TextFont::from_font_size(11.0),
                        ));
                    });
                }
            }

            // **The layer.** `mount` is what replaced `Role`, `rests_on` and the height heuristic that
            // once decided a 10.9 cm mug was a floor decal — so it is the one field worth putting on
            // its own line rather than in a list of tags.
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                margin: UiRect::top(Val::Px(4.0)),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Node {
                        width: Val::Px(48.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Text::new("layer"),
                    TextColor(LABEL),
                    TextFont::from_font_size(10.0),
                ));
                row.spawn((
                    Text::new(mount_label(c.proposed.mount.as_ref())),
                    TextColor(if c.proposed.mount.is_some() { TEXT } else { ACCENT }),
                    TextFont::from_font_size(11.0),
                ));
            });

            // Tag chips, one row per axis. Every token the project has, lit when this piece carries
            // it — so an author sees the whole vocabulary rather than having to remember it, which is
            // the difference between a closed vocabulary being a help and being an obstacle.
            for axis in [Axis::Kind, Axis::Effects, Axis::Look, Axis::Surfaces] {
                let vocab = axis.tokens(&project.vocab);
                if vocab.tokens.is_empty() {
                    continue;
                }
                let held: Vec<String> = match axis {
                    Axis::Kind => c.proposed.kind.clone(),
                    Axis::Effects => c.proposed.effects.clone(),
                    Axis::Look => c.proposed.look.clone(),
                    Axis::Surfaces => c.proposed.offers.surfaces.clone(),
                };
                p.spawn((
                    Text::new(axis.label()),
                    TextColor(LABEL),
                    TextFont::from_font_size(9.0),
                    Node {
                        margin: UiRect::top(Val::Px(5.0)),
                        ..default()
                    },
                ));
                p.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(3.0),
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|chips| {
                    for (ix, name) in vocab.names().enumerate() {
                        let on = held.iter().any(|h| h == name);
                        chips
                            .spawn((
                                UiButton,
                                Hovered::default(),
                                TagChip { axis, token: ix },
                                Node {
                                    padding: UiRect::axes(Val::Px(4.0), Val::Px(1.0)),
                                    ..default()
                                },
                                BackgroundColor(if on { ROW_SELECTED } else { ROW_BG }),
                            ))
                            .with_children(|chip| {
                                chip.spawn((
                                    Text::new(name.to_owned()),
                                    TextColor(if on { TEXT } else { LABEL }),
                                    TextFont::from_font_size(10.0),
                                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                                ));
                            });
                    }
                });
            }

            for f in &c.findings {
                p.spawn((
                    Text::new(f.message.clone()),
                    TextColor(match f.severity {
                        Severity::Blocking => DANGER,
                        Severity::Warn => ACCENT,
                        Severity::Note => DIM,
                    }),
                    TextFont::from_font_size(10.0),
                    Node {
                        margin: UiRect::top(Val::Px(4.0)),
                        ..default()
                    },
                ));
                // The remedy, indented under what it fixes. A warning with no answer is a warning
                // read once.
                if let Some(fix) = &f.fix {
                    p.spawn((
                        Text::new(format!("   {fix}")),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                    ));
                }
            }
        });
    }
}
