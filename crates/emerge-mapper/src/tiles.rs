//! **Tile configuration** — bringing meshes in, and saying what they are.
//!
//! The editor's second tab. `emerge_core::import` does the measuring; this is where an author reads
//! it, gives a mesh an id, decides which layer it goes on and what it is tagged as, and accepts it
//! into the library. Separate from the map tab because they are different jobs with different
//! controls, and one panel trying to hold both would be a panel that does neither well.
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

use crate::keys::{self, Action, Context};
use crate::project::Project;

/// Which job the editor is doing.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Place pieces and build the level.
    #[default]
    Map,
    /// Bring meshes in and say what they are.
    Tiles,
}

impl Mode {
    /// The tabs, in the order they are shown. Map first: it is the job, and configuring tiles is
    /// what you do in order to do it.
    pub const ALL: [Mode; 2] = [Mode::Map, Mode::Tiles];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Map => "MAP",
            Mode::Tiles => "TILES",
        }
    }

    /// The number key that jumps straight here.
    ///
    /// A direct key per tab as well as `Tab` to cycle, because cycling is fine for two and useless
    /// for four — and `docs/ui.md` §4.2 wants everything reachable by mouse reachable by keyboard.
    pub fn action(self) -> crate::keys::Action {
        match self {
            Mode::Map => crate::keys::Action::MapTab,
            Mode::Tiles => crate::keys::Action::TilesTab,
        }
    }
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
    /// The library entry selected for removal, if one is. Separate from [`Self::selected`], which
    /// indexes candidates — the two lists are different things and one index into both would be a
    /// bug waiting for the first time their lengths differ.
    pub selected_library_id: Option<String>,
    /// Packs the author has folded away.
    pub folded_packs: std::collections::HashSet<String>,
}

/// Candidates grouped by the directory they came from, in scan order.
///
/// Scan order is sorted path order, so the groups come out stable across machines and never reorder
/// — the same rule the palette follows and for the same reason (Samp 2011, via `docs/ui.md` §3.5).
fn packs(candidates: &[Candidate]) -> Vec<(String, Vec<usize>)> {
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    for (ix, c) in candidates.iter().enumerate() {
        let dir = c.mesh.rsplit_once('/').map_or(".", |(d, _)| d).to_owned();
        match out.iter_mut().find(|(name, _)| *name == dir) {
            Some((_, members)) => members.push(ix),
            None => out.push((dir, vec![ix])),
        }
    }
    out
}

/// The file name out of a path.
fn leaf(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

impl ImportState {
    pub fn current(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected)
    }
}

/// One tab in the strip, carrying the mode it selects.
#[derive(Component, Clone, Copy)]
struct Tab(Mode);

/// The tab's name, so the active one can be lit without touching its key.
#[derive(Component)]
struct TabLabel;

/// The tab's shortcut, styled a step quieter than the name.
#[derive(Component)]
struct TabKey;

/// Root of the tiles panel, shown and hidden with the mode.
#[derive(Component)]
struct TilesRoot;

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

/// A pack heading in the candidate list. Clicking it folds the pack away.
#[derive(Component, Clone)]
struct PackHeader(String);

/// One row for a tile already in the library, carrying its id.
#[derive(Component, Clone)]
struct LibraryRow(String);

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

/// Where the panels start, below the tab strip. One number, so the two panels cannot disagree about
/// it and leave a tab half-covered.
pub const TAB_STRIP_BOTTOM: f32 = 46.0;

const PANEL_BG: Color = Color::srgb(0.058, 0.054, 0.047);
const ROW_BG: Color = Color::srgb(0.098, 0.092, 0.082);
const ROW_SELECTED: Color = Color::srgb(0.30, 0.28, 0.24);
const TEXT: Color = Color::srgb(0.86, 0.84, 0.80);
const DIM: Color = Color::srgb(0.58, 0.56, 0.53);
const LABEL: Color = Color::srgb(0.46, 0.44, 0.42);
const ACCENT: Color = Color::srgb(0.90, 0.66, 0.24);
const DANGER: Color = Color::srgb(0.86, 0.36, 0.30);
/// A group heading — quieter than a row, because it is a signpost.
const HEADER_BG: Color = Color::srgb(0.075, 0.070, 0.063);
/// The measured footprint — what the placement rules reserve.
const FOOTPRINT: Color = Color::srgb(0.35, 0.72, 0.85);
/// The grid cells it occupies. Where this and the footprint differ is the tiling slack.
const CELLS: Color = Color::srgb(0.42, 0.38, 0.30);
/// The volume, so a height is seen rather than only read.
const EXTENT: Color = Color::srgb(0.24, 0.42, 0.50);

pub struct TilesPlugin;

impl Plugin for TilesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mode>()
            .init_resource::<ImportState>()
            .add_systems(Startup, (spawn_tab_strip, spawn_tiles_panel))
            .add_systems(
                Update,
                (
                    toggle_mode,
                    rename_candidate,
                    move_selection.run_if(not_renaming_candidate),
                    cycle_mount.run_if(not_renaming_candidate),
                    commit_candidate.run_if(not_renaming_candidate),
                    remove_tile.run_if(not_renaming_candidate),
                    apply_mode,
                    style_tabs,
                    tab_shortcuts,
                    rebuild_candidates.run_if(resource_changed::<ImportState>),
                    rebuild_detail.run_if(resource_changed::<ImportState>),
                    refresh_lines,
                    drive_preview,
                    draw_preview_footprint.run_if(in_tiles_mode),
                ),
            )
            .add_observer(on_tab_click)
            .add_observer(on_candidate_click)
            .add_observer(on_library_click)
            .add_observer(on_pack_click)
            .add_observer(on_tag_chip);
    }
}

/// The tab strip. Always visible, above whichever panel is showing.
///
/// A key alone was not enough. `Tab` cycles the mode and always did, but a mode you can only reach by
/// pressing something is a mode you have to be told about — and an editor that has to be explained
/// has a bug in its front page. The strip says both things at once: which jobs exist, and which one
/// you are doing.
fn spawn_tab_strip(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(12.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(2.0),
                ..default()
            },
            GlobalZIndex(101),
        ))
        .with_children(|p| {
            for mode in Mode::ALL {
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    Tab(mode),
                    Node {
                        padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                        align_items: AlignItems::Center,
                        // A thick bottom border is the active marker: it reads at a glance and does
                        // not depend on telling two dark greys apart, which `docs/ui.md` §1.3 rules
                        // out as an encoding on its own.
                        border: UiRect::bottom(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(Color::NONE),
                    BackgroundColor(ROW_BG),
                ))
                .with_children(|tab| {
                    // **The tab states its key.** Cockburn et al. 2014 on the intermodal-transition
                    // failure: offering a fast path beside a slow one does not work on its own, and
                    // users plateau on the slow one. The key has to be visible at the moment of use,
                    // which is `docs/ui.md` §4.2's "each chip states its key".
                    tab.spawn((
                        Text::new(crate::keys::chord(mode.action())),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                        Node {
                            margin: UiRect::right(Val::Px(7.0)),
                            ..default()
                        },
                        TabKey,
                    ));
                    tab.spawn((
                        Text::new(mode.label()),
                        TextColor(LABEL),
                        TextFont::from_font_size(13.0),
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                        TabLabel,
                    ));
                });
            }
        });
}

/// Clicking a tab selects it — and scans on the first visit to the tiles tab, exactly as the key
/// does, so the two ways in behave the same.
fn on_tab_click(
    activate: On<Activate>,
    tabs: Query<&Tab>,
    project: Res<Project>,
    mut mode: ResMut<Mode>,
    mut state: ResMut<ImportState>,
) {
    let Ok(tab) = tabs.get(activate.entity) else {
        return;
    };
    if *mode == tab.0 {
        return;
    }
    *mode = tab.0;
    if *mode == Mode::Tiles && !state.scanned {
        scan(&project, &mut state);
    }
}

/// Light the active tab. The inactive one stays legible rather than greyed to nothing — a tab you
/// cannot read is a tab you do not know is there.
fn style_tabs(
    mode: Res<Mode>,
    mut tabs: Query<(&Tab, &Hovered, &mut BackgroundColor, &mut BorderColor, &Children)>,
    mut names: Query<&mut TextColor, (With<TabLabel>, Without<TabKey>)>,
    mut chords: Query<&mut TextColor, (With<TabKey>, Without<TabLabel>)>,
) {
    for (tab, hovered, mut bg, mut border, children) in &mut tabs {
        let active = tab.0 == *mode;
        // The active tab continues the panel beneath it, so the two read as one surface rather than
        // as a button sitting on top of a box.
        let want_bg = if active {
            PANEL_BG
        } else if hovered.0 {
            Color::srgb(0.16, 0.15, 0.14)
        } else {
            ROW_BG
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want_border = if active { ACCENT } else { Color::NONE };
        *border = BorderColor::all(want_border);

        for child in children.iter() {
            if let Ok(mut colour) = names.get_mut(child) {
                let want = if active { TEXT } else { DIM };
                if colour.0 != want {
                    colour.0 = want;
                }
            }
            if let Ok(mut colour) = chords.get_mut(child) {
                let want = if active { ACCENT } else { LABEL };
                if colour.0 != want {
                    colour.0 = want;
                }
            }
        }
    }
}

/// The number keys jump straight to a tab, and scan on first arrival exactly as `Tab` and a click do.
///
/// Three ways in, one behaviour — `docs/ui.md` §4.2: everything reachable by mouse is reachable by
/// keyboard and vice versa.
fn tab_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    project: Res<Project>,
    mut mode: ResMut<Mode>,
    mut state: ResMut<ImportState>,
) {
    for want in Mode::ALL {
        if keys::just_pressed(&keyboard, want.action()) && *mode != want {
            *mode = want;
            if want == Mode::Tiles && !state.scanned {
                scan(&project, &mut state);
            }
            return;
        }
    }
}

fn spawn_tiles_panel(mut commands: Commands) {
    commands
        .spawn((
            TilesRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(TAB_STRIP_BOTTOM),
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
                Text::new("TILE CONFIGURATION"),
                TextColor(ACCENT),
                TextFont::from_font_size(15.0),
            ));
            // From the census, like the map panel's — see `crate::keys`.
            for row_def in keys::rows(Context::Tiles)
                .into_iter()
                .chain(keys::rows(Context::Global))
            {
                p.spawn(Node {
                    flex_direction: FlexDirection::Row,
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
                        TextColor(DIM),
                        TextFont::from_font_size(11.0),
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    ));
                    row.spawn((
                        Text::new(row_def.does),
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
    keyboard: Res<ButtonInput<KeyCode>>,
    project: Res<Project>,
    mut mode: ResMut<Mode>,
    mut state: ResMut<ImportState>,
) {
    let want_scan = if keys::just_pressed(&keyboard, Action::NextTab) {
        // Cycle, not toggle: a third tab then costs a row in `Mode::ALL` and nothing else.
        let at = Mode::ALL.iter().position(|m| m == &*mode).unwrap_or(0);
        *mode = Mode::ALL[(at + 1) % Mode::ALL.len()];
        *mode == Mode::Tiles && !state.scanned
    } else {
        *mode == Mode::Tiles && keys::just_pressed(&keyboard, Action::Rescan)
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
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<Mode>,
    mut state: ResMut<ImportState>,
) {
    if *mode != Mode::Tiles {
        return;
    }
    if state.renaming.is_none() {
        if keys::just_pressed(&keyboard, Action::TypeId) && state.current().is_some() {
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
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<Mode>,
    project: Res<Project>,
    mut state: ResMut<ImportState>,
) {
    if *mode != Mode::Tiles || !keys::just_pressed(&keyboard, Action::CycleLayer) {
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

/// **Accept a candidate into the library.**
///
/// Validated first, and refused rather than repaired: a descriptor that fails the vocabulary is one
/// an author has not finished, and writing a broken entry would make the next `Library::parse` fail
/// for everyone rather than for the person who caused it.
///
/// The library is written immediately. An importer that batches its additions until some later save
/// is one where a crash loses work an author believes they did — and the file is generated from the
/// manifests today, so an unwritten addition would simply be regenerated away.
fn commit_candidate(
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<Mode>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    if *mode != Mode::Tiles || !keys::just_pressed(&keyboard, Action::Accept) {
        return;
    }
    let Some(candidate) = state.current().cloned() else {
        return;
    };
    if candidate.blocked() {
        state.status = "this mesh cannot be measured, so there is nothing to add".to_owned();
        return;
    }
    let descriptor = candidate.proposed.clone();
    if descriptor.id.trim().is_empty() {
        state.status = "give it an id first (I)".to_owned();
        return;
    }
    if project.library.get(&descriptor.id).is_some() {
        state.status = format!("`{}` is already in the library — rename it (I)", descriptor.id);
        return;
    }

    // Validate against a library that ALREADY CONTAINS it, because the two-sided surface check is
    // about the finished set: a piece that offers `worktop` makes another piece's `on worktop` legal,
    // and checking it in isolation would reject the pair that fixes each other.
    let mut trial = project.library.clone();
    trial.descriptors.push(descriptor.clone());
    if let Err(e) = trial.resolve(&project.vocab) {
        state.status = format!("not added: {e}");
        return;
    }

    project.library = trial;
    // Masks and triangle counts are derived from the library, so they move with it.
    match project.library.resolve(&project.vocab) {
        Ok(masks) => project.masks = masks,
        Err(e) => {
            state.status = format!("not added: {e}");
            return;
        }
    }
    project.remeasure_triangles();
    let path = project.root.join("assets/emerge/library.ron");
    match project
        .library
        .to_ron()
        .and_then(|text| emerge_core::ron_surgery::save_atomic(&path, &text))
    {
        Ok(()) => {
            // Drop it from the candidate list: it is in the library now, and an importer that keeps
            // offering what you have already taken is one you cannot tell your progress from.
            let at = state.selected;
            state.candidates.remove(at);
            state.selected = at.min(state.candidates.len().saturating_sub(1));
            state.summary = format!("{} mesh(es) left to import", state.candidates.len());
            state.status = format!(
                "added `{}` — it is in the palette now",
                descriptor.id
            );
            info!("added `{}` to {}", descriptor.id, path.display());
        }
        Err(e) => {
            state.status = format!("NOT WRITTEN: {e}");
            error!("{e}");
        }
    }
}

/// **Take a tile back out of the library.**
///
/// The tiles tab lists what is IN the library above what could be added to it, because "configure the
/// tiles" is both halves of that and an editor with an add and no remove is one where a mistyped
/// import is permanent.
///
/// It refuses to remove a descriptor the open map is using. An orphaned placement is not an error the
/// map can carry — it names a descriptor nothing defines, so the piece silently fails to appear and
/// the author finds out by counting crates. Saying "12 placements use this" is the answer; deleting
/// them on their behalf is not.
fn remove_tile(
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<Mode>,
    mut project: ResMut<Project>,
    mut state: ResMut<ImportState>,
) {
    if *mode != Mode::Tiles || !keys::just_pressed(&keyboard, Action::RemoveTile) {
        return;
    }
    let Some(id) = state.selected_library_id.clone() else {
        state.status = "select a library tile to remove it".to_owned();
        return;
    };

    let used = project
        .map
        .placements
        .iter()
        .filter(|p| p.descriptor == id)
        .count();
    if used > 0 {
        state.status = format!(
            "`{id}` is used by {used} placement(s) in this map — remove those first"
        );
        return;
    }

    let Some(at) = project.library.descriptors.iter().position(|d| d.id == id) else {
        return;
    };
    let mut trial = project.library.clone();
    trial.descriptors.remove(at);
    // Re-validate: removing a piece can strand another that rested on the surface it offered, and
    // that is exactly the two-sided check's job.
    match trial.resolve(&project.vocab) {
        Ok(masks) => {
            project.library = trial;
            project.masks = masks;
        }
        Err(e) => {
            state.status = format!("not removed: {e}");
            return;
        }
    }
    project.remeasure_triangles();

    let path = project.root.join("assets/emerge/library.ron");
    match project
        .library
        .to_ron()
        .and_then(|text| emerge_core::ron_surgery::save_atomic(&path, &text))
    {
        Ok(()) => {
            state.selected_library_id = None;
            state.status = format!("removed `{id}` from the library");
            info!("removed `{id}` from {}", path.display());
        }
        Err(e) => state.status = format!("NOT WRITTEN: {e}"),
    }
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
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<Mode>,
    mut state: ResMut<ImportState>,
) {
    if *mode != Mode::Tiles || state.candidates.is_empty() {
        return;
    }
    let last = state.candidates.len() - 1;
    if keys::just_pressed(&keyboard, Action::NextCandidate) && state.selected < last {
        state.selected += 1;
    }
    if keys::just_pressed(&keyboard, Action::PrevCandidate) && state.selected > 0 {
        state.selected -= 1;
    }
}

fn on_pack_click(
    activate: On<Activate>,
    headers: Query<&PackHeader>,
    mut state: ResMut<ImportState>,
) {
    let Ok(header) = headers.get(activate.entity) else {
        return;
    };
    if !state.folded_packs.remove(&header.0) {
        state.folded_packs.insert(header.0.clone());
    }
}

fn on_library_click(
    activate: On<Activate>,
    rows: Query<&LibraryRow>,
    mut state: ResMut<ImportState>,
) {
    if let Ok(row) = rows.get(activate.entity) {
        state.selected_library_id = Some(row.0.clone());
        state.status = format!("`{}` selected — Del removes it", row.0);
    }
}

fn on_candidate_click(
    activate: On<Activate>,
    rows: Query<&CandidateRow>,
    mut state: ResMut<ImportState>,
) {
    if let Ok(row) = rows.get(activate.entity) {
        state.selected = row.0;
        // One selection at a time, or `Del` would have to guess which list it meant.
        state.selected_library_id = None;
    }
}

/// Show one panel and hide the other. `Display::None` rather than `Visibility`, because a hidden-by-
/// visibility UI node still occupies layout and still answers hover — which would leave the map
/// panel's rows eating clicks aimed at the world.
fn apply_mode(
    mode: Res<Mode>,
    mut import_root: Query<&mut Node, (With<TilesRoot>, Without<MapRoot>)>,
    mut map_root: Query<&mut Node, (With<MapRoot>, Without<TilesRoot>)>,
) {
    if !mode.is_changed() {
        return;
    }
    let (import_shown, map_shown) = match *mode {
        Mode::Map => (Display::None, Display::Flex),
        Mode::Tiles => (Display::Flex, Display::None),
    };
    for mut node in &mut import_root {
        node.display = import_shown;
    }
    for mut node in &mut map_root {
        node.display = map_shown;
    }
}

fn in_tiles_mode(mode: Res<Mode>) -> bool {
    *mode == Mode::Tiles
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
    if *mode != Mode::Tiles {
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
    project: Res<Project>,
    lists: Query<Entity, With<CandidateList>>,
) {
    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|p| {
            // **What is already a tile**, above what could become one. Both halves are "configuring
            // the tiles", and an editor that can add but not remove makes a mistyped import permanent.
            p.spawn((
                Text::new(format!("IN LIBRARY  ({})", project.library.descriptors.len())),
                TextColor(LABEL),
                TextFont::from_font_size(9.0),
            ));
            for d in &project.library.descriptors {
                let selected = state.selected_library_id.as_deref() == Some(d.id.as_str());
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    LibraryRow(d.id.clone()),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(if selected { ROW_SELECTED } else { ROW_BG }),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new(d.id.clone()),
                        TextColor(TEXT),
                        TextFont::from_font_size(10.0),
                    ));
                });
            }

            p.spawn((
                Text::new(format!("NOT YET IMPORTED  ({})", state.candidates.len())),
                TextColor(LABEL),
                TextFont::from_font_size(9.0),
                Node {
                    margin: UiRect::top(Val::Px(6.0)),
                    ..default()
                },
            ));
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
            // **Grouped by pack.** 319 rows is a list you scroll past; grouped by where they came
            // from it is a dozen headings, and an author importing a kit wants that kit rather than
            // an alphabet.
            //
            // The directory, not `kind` — a candidate has no `kind` yet, that being the thing import
            // is FOR. The folder an artist put it in is the only categorisation that exists before
            // anyone has looked at it, and it is usually the right one.
            for (pack, members) in packs(&state.candidates) {
                let folded = state.folded_packs.contains(&pack);
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    PackHeader(pack.clone()),
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
                        Text::new(pack.clone()),
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
                let Some(c) = state.candidates.get(ix) else {
                    continue;
                };
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
                        // The file's own name, not the full path — the pack heading already said
                        // where it came from, and repeating it on 145 rows is the same word 145 times.
                        Text::new(leaf(&c.mesh)),
                        TextColor(TEXT),
                        TextFont::from_font_size(10.0),
                    ));
                });
                }
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
