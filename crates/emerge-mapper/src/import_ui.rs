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

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton, ScrollArea};
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
    pub status: String,
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

/// One candidate row, carrying its index.
#[derive(Component, Clone, Copy)]
struct CandidateRow(usize);

/// The node the candidate list is rebuilt into.
#[derive(Component)]
struct CandidateList;

/// The node the selected candidate's detail is rebuilt into.
#[derive(Component)]
struct DetailPane;

const PANEL_BG: Color = Color::srgb(0.058, 0.054, 0.047);
const ROW_BG: Color = Color::srgb(0.098, 0.092, 0.082);
const ROW_SELECTED: Color = Color::srgb(0.30, 0.28, 0.24);
const TEXT: Color = Color::srgb(0.86, 0.84, 0.80);
const DIM: Color = Color::srgb(0.58, 0.56, 0.53);
const LABEL: Color = Color::srgb(0.46, 0.44, 0.42);
const ACCENT: Color = Color::srgb(0.90, 0.66, 0.24);
const DANGER: Color = Color::srgb(0.86, 0.36, 0.30);

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
                    move_selection,
                    apply_mode,
                    rebuild_candidates.run_if(resource_changed::<ImportState>),
                    rebuild_detail.run_if(resource_changed::<ImportState>),
                ),
            )
            .add_observer(on_candidate_click);
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
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    max_height: Val::Px(300.0),
                    overflow: Overflow::scroll_y(),
                    margin: UiRect::top(Val::Px(6.0)),
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
            state.status = format!(
                "{} mesh(es) not in the library — {warned} with warnings, {blocked} unmeasurable",
                found.len()
            );
            state.candidates = found;
            state.selected = 0;
            state.scanned = true;
        }
        Err(e) => {
            state.status = e;
            state.scanned = true;
        }
    }
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
    panes: Query<Entity, With<DetailPane>>,
) {
    for pane in &panes {
        commands.entity(pane).despawn_related::<Children>();
        commands.entity(pane).with_children(|p| {
            p.spawn((
                Text::new(state.status.clone()),
                TextColor(DIM),
                TextFont::from_font_size(10.0),
            ));

            let Some(c) = state.current() else {
                return;
            };

            p.spawn((
                Text::new(format!("id  {}", c.proposed.id)),
                TextColor(TEXT),
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
