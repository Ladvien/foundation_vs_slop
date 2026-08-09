//! **The Compose tab** — reusable groups, what they present, and what has gone stale under them.
//!
//! A [`Composition`] is a named set of placements a map holds a *reference* to rather than a copy of.
//! This is where an author reads one: its members, the edge tokens its boundary derives, the places
//! its members contradict each other about that boundary, and which of its members were built against
//! a descriptor that has since changed. Arming one here is what makes the Map tab's next click a
//! stamp instead of a placement.
//!
//! # It reads; the file writes
//!
//! This tab does not yet author a composition — `compositions.ron` is hand-written, and that is
//! deliberate rather than unfinished. Building the group-from-selection verb before anything could
//! *read* a group would have meant an authoring surface whose output nothing validated; the order
//! here is the same one `emerge-core` took, where the schema and its expander shipped before the file
//! did. What this tab does is make the file's consequences visible, which is the thing that was
//! impossible before it.
//!
//! # Explaining itself is the job
//!
//! The PCG book's mixed-initiative chapter puts the failure plainly: a designer *"may become
//! frustrated or confused if the computer consistently acts as though it is not following the model
//! that the human designer has in her head"*, and lists *"can the computer explain itself?"* among the
//! open questions any such tool has to answer. Three of this panel's four blocks exist for that
//! reason — the derived interface says what a group presents, the faults say where its own members
//! disagree, and STALE says which member changed underneath it and by how much. A badge that says
//! only "something is wrong" would be the version of this tool that gets ignored.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton};

use emerge_core::composition::{self, Band, Composition, Envelope};

use crate::chrome::{ACCENT, DANGER, DIM, LABEL, TEXT};
use crate::keys::{self, Action};
use crate::project::Project;
use crate::tiles::{ComposeRoot, Mode};

/// **Where the Compose tab stands the group it is editing.**
///
/// Far from the map, like [`crate::tiles::STAGE`] and for the same reason: a group drawn over the map
/// is indistinguishable from what is already placed there, and seating a member means watching *that*
/// member move.
pub const COMPOSE_STAGE: Vec3 = crate::stages::COMPOSE;

/// The three lists on this tab, and the order `left`/`right` cycles them.
///
/// Groups on the left, then that group's members under it, then the library on the right — reading
/// order, so cycling forward walks the screen left to right rather than in an order only the code
/// knows.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    #[default]
    Groups,
    Members,
    Meshes,
}

impl Pane {
    pub const ALL: [Pane; 3] = [Pane::Groups, Pane::Members, Pane::Meshes];

    pub fn label(self) -> &'static str {
        match self {
            Pane::Groups => "COMPOSITIONS",
            Pane::Members => "MEMBERS",
            Pane::Meshes => "PLACE",
        }
    }

    fn step(self, by: i32) -> Pane {
        let at = Pane::ALL.iter().position(|p| *p == self).unwrap_or(0) as i32;
        let n = Pane::ALL.len() as i32;
        Pane::ALL[(at + by).rem_euclid(n) as usize]
    }
}

/// Which group the tab is looking at, and which one the map will stamp.
#[derive(Resource)]
pub struct ComposeState {
    /// Index into `project.compositions.compositions`. Clamped on rebuild rather than stored as an
    /// `Option`, because the list is never empty while a selection exists.
    pub selected: usize,
    /// Index into the selected group's members — the second cursor, under the one above. Clamped the
    /// same way and for the same reason.
    pub member: usize,
    /// **Previous member lists, most recent last.** Compose's own undo, because Map, Tiles and Anim
    /// each keep one and an editing surface without one would be the odd tab out.
    ///
    /// Whole values rather than an operation enum. The Map's stack argues the shape it needs —
    /// *"Every variant's inverse is another variant of this same enum"* — and for a file this size
    /// the simplest thing with that property is the value itself, which cannot disagree with what it
    /// is the inverse of.
    /// Written only by [`commit`] and [`step_history`]; public because this is a `Resource` whose
    /// other fields are, and because a functional-update literal outside the crate needs every field
    /// visible — `tests/headless.rs` builds one.
    pub undo: Vec<Vec<Composition>>,
    pub redo: Vec<Vec<Composition>>,
    /// **The name being typed for a new group.** `Some("")` the moment `N` is pressed, so the panel
    /// can show the field before a character exists.
    pub naming: Option<String>,
    /// **Which of the three lists the walk keys move.**
    ///
    /// Replaced a modal picker that took the keyboard while open. A mode you cannot see is the
    /// defect this tab was rebuilt for, so this is drawn — see `rebuild`, which tints the focused
    /// list's header — and the lists are all on screen at once rather than one at a time.
    pub focus: Pane,
    /// Index into the library, for the mesh list on the right.
    pub mesh: usize,
    /// **The armed group** — by id, never by index.
    ///
    /// An index would silently re-point the moment `compositions.ron` gained a row above it, which is
    /// the same argument [`emerge_core::composition::Override`] makes about naming a member. The id
    /// costs one lookup at stamp time and cannot go stale.
    pub armed: Option<String>,
    /// **What was last stood up**, so nothing is respawned unless the picture actually changed.
    ///
    /// This exists for a defect rather than for speed. [`restage_group`] takes `ResMut<ComposeState>`
    /// and writes `status.problem` when a group will not resolve — which re-marks the resource
    /// changed, which re-triggers its own gate, which writes again. That is an unbounded
    /// despawn/respawn every frame for as long as the error is on screen, and the strip multiplies it
    /// by five. Comparing what was staged closes the loop at its source.
    ///
    /// The focal index is part of the key because stepping the carousel restages everything: the
    /// neighbours change and the scales move.
    pub staged: Option<(usize, Vec<Composition>)>,
    /// What this tab has to say — see [`crate::chrome::Status`] for why a refusal and a receipt are
    /// two slots rather than one string. This panel used to paint every message [`ACCENT`], so
    /// `cannot record` and `recorded 3 member(s)` were the same colour and the first was gone as soon
    /// as anything else happened.
    pub status: crate::chrome::Status,
}

/// Written out rather than derived, so adding a field costs a compile error here — which is the right
/// place to be asked whether its zero value is the one this tab should open in.
impl Default for ComposeState {
    fn default() -> Self {
        ComposeState {
            selected: 0,
            member: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            naming: None,
            focus: Pane::default(),
            mesh: 0,
            armed: None,
            staged: None,
            status: crate::chrome::Status::default(),
        }
    }
}

/// One line of the panel. Rebuilt wholesale when anything it reads changes.
///
/// Carries nothing: the whole block is despawned and respawned together, so a line has no identity
/// worth keeping. An index field would be a hook for a click handler that does not exist yet, which
/// is a stub.
#[derive(Component)]
struct ComposeLine;

/// The block the list and the detail are written into.
#[derive(Component)]
struct ComposeBody;

/// The right-hand mesh list's scroll area, and the header above it that shows the focus.
#[derive(Component)]
struct MeshList;

#[derive(Component)]
struct MeshHeader;

/// One row of the mesh list. Carries its library index so a click needs no second lookup.
#[derive(Component)]
struct MeshRow(usize);

/// The **NEW** button, top right. The `N` key does the same thing through the same call.
///
/// `pub` for the same reason [`StagedMember`] is: whether an observer fires for the *right* entity is
/// a question about the schedule, and only `tests/headless.rs` can ask it.
#[derive(Component)]
pub struct NewGroupButton;

pub struct ComposePlugin;

impl Plugin for ComposePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComposeState>()
            .init_resource::<StagedCarousel>()
            .add_systems(Startup, spawn_compose_panel)
            .add_systems(
                Update,
                (
                    start_new,
                    walk,
                    arm,
                    record,
                    cycle_focus,
                    seat_member,
                    flush_member,
                    turn_member,
                    drop_member,
                    paint_member,
                    step_history,
                    step_carousel,
                    pick_slot,
                )
                    .in_set(keys::Phase::Act)
                    .run_if(in_compose_mode),
            )
            // Not gated on the mode, for the same reason `rebuild` is not: the staged group is
            // despawned when the tab is left, and a system that stops running cannot despawn it.
            .add_systems(Update, new_group_keys.in_set(keys::Phase::Text))
            .add_systems(Update, restage_group.after(keys::Phase::Act))
            // After the strip is published, so nothing is drawn against last frame's layout.
            .add_systems(
                Update,
                draw_stage.after(restage_group).run_if(in_compose_mode),
            )
            // Labels are NOT gated on the mode: `place_labels` owns their visibility and hides them
            // off-tab, which a system that has stopped running cannot do.
            .add_systems(
                Update,
                (rebuild_labels, place_labels).chain().after(restage_group),
            )
            // Not gated on the mode: the armed group is shown on the Map tab too, and a panel that
            // stops updating when you leave it is a panel that lies the moment you come back.
            .add_systems(Update, rebuild.after(keys::Phase::Act))
            .add_systems(Update, rebuild_meshes.after(keys::Phase::Act))
            .add_observer(on_new_group_click)
            .add_observer(on_mesh_click);
    }
}

fn in_compose_mode(mode: Res<Mode>) -> bool {
    *mode == Mode::Compose
}

fn spawn_compose_panel(mut commands: Commands) {
    crate::chrome::panel_root(
        &mut commands,
        crate::chrome::Side::Left,
        // The wider of the two panel widths. Measured, not guessed: at `CONTROLS_W` a member line
        // (`chair_north: dining_chair (0.0, -1.0) yaw 180`) wrapped, and a wrapped continuation
        // starts at column zero, which breaks the indentation the list reads by.
        crate::chrome::TILES_CONTROLS_W,
        true,
        // Starts hidden: the editor opens in map mode.
        true,
    )
    .insert(ComposeRoot)
    .with_children(|p| {
        crate::chrome::title(p, "COMPOSE");
        // Directly under the title, above everything a working author reads — a problem that has to
        // be scrolled to is a problem that gets missed.
        crate::chrome::problem_banner(p, Mode::Compose);
        crate::chrome::shortcut_hint(p);
        // **No inline key census here, and its absence is the fix.**
        //
        // This tab was the only one still calling `chrome::key_census` in its own panel. The other
        // three dropped it when the hold-`K` overlay arrived — `drive_shortcuts_overlay` builds the
        // same rows from the same function, and its own note says "the same order the panels *used*".
        // Compose kept the old call, so it drew twenty rows of key list permanently AND the same
        // twenty again on `K`: two paths to one answer, and the panel paying for both.
        //
        // Harmless while this context had three rows; step 3 took it to eight, and with Global's
        // twelve the group list was pushed off the bottom of the screen. Found by an author trying to
        // read it, which is the only way a layout bug is ever found.
        crate::chrome::section(p, "COMPOSITIONS");
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ComposeBody,
            crate::notice::CopyPane(Mode::Compose),
        ));
        // **Last, and it must be.** `margin-top: auto` is what pins it to the bottom of
        // the panel, and an auto margin in a column absorbs the free space above it — so
        // placed any earlier it pushes every sibling after it down with it.
        crate::chrome::problem_log(p, Mode::Compose);
    });

    // **The mesh list, on the right where the Map tab keeps its palette.**
    //
    // Tagged `ComposeRoot` like the panel above, so `tiles::apply_mode` shows and hides it with the
    // tab and needs no change — it iterates every marker it knows and this is one of them.
    //
    // This is the thing the tab was missing: you could not see what you were choosing between. A
    // modal picker over the same list was tried first and rejected for the reason the whole rebuild
    // exists — a mode you cannot see is a mode you cannot use.
    crate::chrome::panel_root(
        &mut commands,
        crate::chrome::Side::Right,
        crate::chrome::LIST_W,
        true,
        true,
    )
    .insert(ComposeRoot)
    .with_children(|p| {
        // **The verb, where it can be seen.** `N` has made a group since this tab could make one at
        // all, and an author looked straight at the tab and reported that it could not — a key with
        // no visible affordance is a key nobody finds. Cockburn et al. is the same finding the key
        // census is built on: a fast path offered beside no slow path is not offered.
        p.spawn((
            UiButton,
            Hovered::default(),
            NewGroupButton,
            Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                min_height: Val::Px(22.0),
                margin: UiRect::bottom(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(crate::chrome::HEADER_BG),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(format!("+ NEW COMPOSITION   {}", keys::chord(Action::NewGroup))),
                TextColor(ACCENT),
                TextFont::from_font_size(11.0),
            ));
        });
        p.spawn((Text::new("PLACE"), TextColor(LABEL), TextFont::from_font_size(11.0), MeshHeader));
        crate::chrome::scroll_list(p, MeshList);
    });
}

/// Walk the list. Shift steps five, matching every other list in this editor.
/// **`left`/`right` choose which list the arrows walk.**
///
/// The Tiles tab's own idiom — two keys for three lists, which is what keeps this context inside the
/// twelve-row census ceiling with everything else it has to say. The focused list is drawn
/// differently; see `rebuild`.
fn cycle_focus(
    mut state: ResMut<ComposeState>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let by = if keys::just_pressed(&input, keys.0, Action::ComposeMemberNext) {
        1
    } else if keys::just_pressed(&input, keys.0, Action::ComposeMemberPrev) {
        -1
    } else {
        return;
    };
    state.focus = state.focus.step(by);
    let focus = state.focus;
    let said = match focus {
        Pane::Meshes => format!("{} — Enter drops one into this tile", focus.label()),
        _ => focus.label().to_owned(),
    };
    state.status.note(said);
}

/// **`up`/`down` walk whichever list has focus.** Shift strides five, as every list here does.
fn walk(
    mut state: ResMut<ComposeState>,
    project: Res<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let step = if input.pressed(KeyCode::ShiftLeft) || input.pressed(KeyCode::ShiftRight) { 5 } else { 1 };
    let by = if keys::just_pressed(&input, keys.0, Action::ComposeNext) {
        step
    } else if keys::just_pressed(&input, keys.0, Action::ComposePrev) {
        -step
    } else {
        return;
    };
    let n = match state.focus {
        Pane::Groups => project.compositions.compositions.len(),
        Pane::Members => project
            .compositions
            .compositions
            .get(state.selected)
            .map_or(0, |c| c.members.len()),
        Pane::Meshes => project.library.descriptors.len(),
    };
    if n == 0 {
        return;
    }
    let wrap = |at: usize| ((at as i64 + by).rem_euclid(n as i64)) as usize;
    match state.focus {
        Pane::Groups => {
            state.selected = wrap(state.selected);
            // A different group has different members; leaving the old index would point the seat
            // verbs at whatever happened to sit there.
            state.member = 0;
        }
        Pane::Members => state.member = wrap(state.member.min(n - 1)),
        Pane::Meshes => state.mesh = wrap(state.mesh.min(n - 1)),
    }
}

/// Arm the selected group, or disarm it if it was already armed.
///
/// Toggling rather than a separate disarm verb, for the reason `EditorState::brush` is an `Option`:
/// **nothing armed has to be a reachable state**, or an author cannot put the cursor over the map
/// without something following it.
/// `Enter` — **adds the highlighted mesh when the library has focus, arms the group otherwise.**
///
/// One key, two meanings decided by something visible on screen, rather than a modal that takes the
/// keyboard and says so only in a status line.
fn arm(
    mut state: ResMut<ComposeState>,
    mut project: ResMut<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if !keys::just_pressed(&input, keys.0, Action::ComposeArm) {
        return;
    }
    if state.focus == Pane::Meshes {
        let Some(d) = project.library.descriptors.get(state.mesh) else {
            return state.status.problem("the library has no piece there");
        };
        let descriptor = d.id.clone();
        return add_member(&mut state, &mut project, &descriptor);
    }
    toggle_arm(&mut state, &project);
}

/// **Arm the selected group, or disarm it if it was already armed.**
///
/// `pub` because the sentinel driver calls it too. It re-implemented this inline for one session and
/// the two immediately disagreed — the keyboard toggled and the sentinel did not — so a captured
/// frame was evidence about a path no author could take, which is the one thing that driver exists to
/// avoid.
pub fn toggle_arm(state: &mut ComposeState, project: &Project) {
    let Some(c) = project.compositions.compositions.get(state.selected) else {
        state.status.note("no composition to arm");
        return;
    };
    if state.armed.as_deref() == Some(c.id.as_str()) {
        state.armed = None;
        state.status.note(format!("`{}` disarmed", c.id));
    } else {
        state.armed = Some(c.id.clone());
        state.status.note(format!("`{}` armed — the map tab stamps it", c.id));
    }
}

/// **Write down what every member currently presents, and save it.**
///
/// The verb the STALE badge is about. Without it nothing in a shipped path ever called
/// `record_fingerprints`, so every group stayed permanently `Unrecorded` and the whole verifying
/// trace was inert — a mechanism that could report drift and would never be given a baseline to
/// report it against.
///
/// It writes `compositions.ron` through the same atomic save the library uses, and **only** when
/// something changed: a keypress that rewrites a file to identical bytes is a keypress that makes
/// every group look edited in a diff.
fn record(
    mut state: ResMut<ComposeState>,
    mut project: ResMut<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if !keys::just_pressed(&input, keys.0, Action::ComposeRecord) {
        return;
    }
    record_selected(&mut state, &mut project);
}

/// The body, `pub` so the sentinel driver calls the same one the key does — never a second copy.
pub fn record_selected(state: &mut ComposeState, project: &mut Project) {
    let at = state.selected;
    let snapshot = project.compositions.compositions.clone();
    let library = project.library.clone();
    let Some(target) = project.compositions.compositions.get_mut(at) else {
        state.status.note("no composition to record");
        return;
    };
    let id = target.id.clone();
    let changed = match composition::record_fingerprints(target, &snapshot, &library) {
        Ok(n) => n,
        Err(e) => {
            state.status.problem(format!("cannot record `{id}`: {e}"));
            return;
        }
    };
    if changed == 0 {
        state.status.note(format!("`{id}` was already up to date — nothing written"));
        return;
    }
    let path = project
        .emerge_dir
        .join(emerge_core::composition::Compositions::FILE);
    let text = match project.compositions.to_ron() {
        Ok(t) => t,
        Err(e) => {
            state.status.problem(format!("NOT WRITTEN: {e}"));
            return;
        }
    };
    // A refusal must not read like a receipt — which is now structural rather than a matter of
    // spelling, because the two go to different slots and only one of them is a red block.
    state.status.say(
        emerge_core::ron_surgery::save_atomic(&path, &text)
            .map(|()| format!("recorded {changed} member(s) of `{id}`"))
            .map_err(|e| format!("NOT WRITTEN: {e}")),
    );
}

// ------------------------------------------------------------------------------------------------
// Seating — the door, and the verbs that go through it
// ------------------------------------------------------------------------------------------------

/// How deep the undo stack goes. Deep enough for a seating session, bounded so a long one cannot
/// grow without limit.
const HISTORY: usize = 64;

/// **Edit the composition set, and keep the result only if it is a valid set.**
///
/// The same door [`crate::editor::keep_as_group`] and [`record_selected`] go through, and the reason
/// there is one: a group that fails validation must leave both the file and the in-memory set exactly
/// as they were, or the editor is showing something the game will refuse to load.
///
/// Writes immediately rather than staging, because **that is the model `compositions.ron` already
/// has** — both existing writers `save_atomic` on the keypress. A staging buffer would be a second
/// write model for one file. It is safe here for a reason specific to this file: it carries no `//`
/// comments on purpose, recorded in its own `note`, precisely because `to_ron` reserializes. There is
/// nothing for a rewrite to lose.
fn commit(
    state: &mut ComposeState,
    project: &mut Project,
    edit: impl FnOnce(&mut Vec<Composition>) -> Result<String, String>,
) {
    let was = project.compositions.compositions.clone();
    let mut proposed = project.compositions.clone();
    let receipt = match edit(&mut proposed.compositions) {
        Ok(r) => r,
        Err(e) => return state.status.problem(e),
    };
    if let Err(e) = composition::validate(&proposed.compositions, &project.library) {
        return state.status.problem(e);
    }
    let path = project
        .emerge_dir
        .join(emerge_core::composition::Compositions::FILE);
    let text = match proposed.to_ron() {
        Ok(t) => t,
        Err(e) => return state.status.problem(format!("NOT WRITTEN: {e}")),
    };
    if let Err(e) = emerge_core::ron_surgery::save_atomic(&path, &text) {
        return state.status.problem(format!("NOT WRITTEN: {e}"));
    }
    project.compositions = proposed;
    state.undo.push(was);
    if state.undo.len() > HISTORY {
        state.undo.remove(0);
    }
    // A new edit forks the history. Keeping the redo stack would let a later redo reinstate a set
    // that never followed from what is now on disk.
    state.redo.clear();
    state.status.note(receipt);
}

// ------------------------------------------------------------------------------------------------
// Making a group here, rather than capturing one on the Map
// ------------------------------------------------------------------------------------------------

/// The tile a new group claims until somebody says otherwise.
///
/// **Bounded, not anchored**, because a group that claims a tile is the thing this whole effort is
/// for: `grammar::learn` refuses a piece that is not the cell's size, and not one wall piece in the
/// Site kit is. One metre square by the height the kit's walls stand at.
pub(crate) const NEW_TILE: (f32, f32, f32) = (1.0, 2.4, 1.0);

/// `N` — start naming a new group.
fn start_new(
    mut state: ResMut<ComposeState>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if !keys::just_pressed(&input, keys.0, Action::NewGroup) {
        return;
    }
    state.naming = Some(String::new());
    state.status.note("name the new composition, then Enter — Esc to abandon it");
}

/// The name field. Same shape as every other field here: drain on the frame it opens, or the
/// keystroke that opened it is read as its first character.
fn new_group_keys(
    mut events: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut project: ResMut<Project>,
    mut state: ResMut<ComposeState>,
) {
    use bevy::input::keyboard::Key;
    if state.naming.is_none() {
        events.clear();
        return;
    }
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                let Some(raw) = state.naming.take() else { return };
                new_group(&mut state, &mut project, &raw);
                return;
            }
            Key::Escape => {
                state.naming = None;
                state.status.note("no new composition");
                return;
            }
            Key::Backspace => {
                if let Some(raw) = state.naming.as_mut() {
                    raw.pop();
                }
            }
            Key::Character(text) => {
                if let Some(raw) = state.naming.as_mut() {
                    raw.push_str(text);
                }
            }
            Key::Space => {
                if let Some(raw) = state.naming.as_mut() {
                    raw.push(' ');
                }
            }
            _ => {}
        }
    }
}

/// **Make an empty bounded group and select it**, through the same door everything else goes through.
///
/// `pub` for the same reason [`seat_selected`] is: a test drives the verb the key drives.
pub fn new_group(state: &mut ComposeState, project: &mut Project, raw: &str) {
    let id = emerge_core::naming::to_snake_case(raw);
    if id.is_empty() {
        return state
            .status
            .problem(format!("`{raw}` leaves nothing usable as a name"));
    }
    if project.compositions.compositions.iter().any(|c| c.id == id) {
        return state.status.problem(format!(
            "`{id}` is already a composition. Pick another name — renaming one would strand every map \
             that stamped it."
        ));
    }
    let made = id.clone();
    commit(state, project, move |set| {
        set.push(Composition {
            id: made.clone(),
            envelope: Envelope::Bounded { size: NEW_TILE },
            members: Vec::new(),
            locations: Vec::new(),
            note: None,
        });
        // Sorted, because the file has one encoding — the same rule members follow.
        set.sort_by(|a, b| a.id.cmp(&b.id));
        // **Built from the census, not written out.** This said "press A to add a piece", and there
        // is no `A` verb on this tab — `A` is `PanLeft`, so an author following the receipt slid the
        // camera and concluded pieces could not be placed at all. A chord in a message has to come
        // from the same table the key handler reads, or it is a claim nothing checks.
        Ok(format!(
            "`{made}` — empty. Click a piece in PLACE (or {} to walk to that list), then {} to add it.",
            keys::chord(Action::ComposeMemberNext),
            keys::chord(Action::ComposeArm),
        ))
    });
    // Select what was just made, so the next verb acts on it rather than on whatever was selected
    // before. Looked up by id rather than remembered as an index, because the sort above moved things.
    if let Some(at) = project
        .compositions
        .compositions
        .iter()
        .position(|c| c.id == id)
    {
        state.selected = at;
        state.member = 0;
    }
}



/// **Put a library piece into the selected group**, at its centre, through the commit door.
///
/// It lands at the middle of the tile rather than anywhere clever: the seat and flush verbs are how
/// it gets where it belongs, and guessing a position would be the tool acting on a model other than
/// the author's.
pub fn add_member(state: &mut ComposeState, project: &mut Project, descriptor: &str) {
    let selected = state.selected;
    let what = descriptor.to_owned();
    commit(state, project, move |set| {
        let c = set
            .get_mut(selected)
            .ok_or_else(|| "that composition is no longer there".to_owned())?;
        // A member id from the piece's own name, numbered from the second — the rule capture uses.
        let short = what.rsplit('/').next().unwrap_or(&what).to_owned();
        let mut id = short.clone();
        let mut n = 2;
        while c.members.iter().any(|m| m.id == id) {
            id = format!("{short}_{n}");
            n += 1;
        }
        c.members.push(composition::Member {
            paint: 0,
            id: id.clone(),
            body: composition::Body::Descriptor {
                id: what.clone(),
                tip: (0, 0),
                on: None,
                patch: None,
            },
            at: (0.0, 0.0),
            yaw: 0.0,
            lift: 0.0,
            // Never recorded is a different fact from stale; `R` is what records it.
            of_fingerprint: None,
            note: None,
        });
        c.members.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(format!("added `{id}` — T F G H to seat it, Shift for flush"))
    });
}

/// The selected member of the selected group, or a note saying there is none.
fn selected_member<'a>(
    state: &ComposeState,
    comps: &'a [Composition],
) -> Option<(&'a Composition, usize)> {
    let c = comps.get(state.selected)?;
    let i = state.member.min(c.members.len().checked_sub(1)?);
    Some((c, i))
}


/// **Seat the selected member** — one lattice step per press, written through the door.
fn seat_member(
    mut state: ResMut<ComposeState>,
    mut project: ResMut<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let nudge = [
        (Action::SeatForward, Nudge::Forward),
        (Action::SeatBack, Nudge::Back),
        (Action::SeatLeft, Nudge::Left),
        (Action::SeatRight, Nudge::Right),
        (Action::SeatUp, Nudge::Up),
        (Action::SeatDown, Nudge::Down),
    ]
    .into_iter()
    .find(|(a, _)| keys::just_pressed(&input, keys.0, *a));
    let Some((_, nudge)) = nudge else { return };
    seat_selected(&mut state, &mut project, nudge);
}

/// **The body, `pub` so a test and a driver call the same one the key does** — never a second copy.
///
/// The same reason [`toggle_arm`] and [`record_selected`] are public: the one time a caller
/// re-implemented a verb inline, the two immediately disagreed.
pub fn seat_selected(state: &mut ComposeState, project: &mut Project, nudge: Nudge) {
    let comps = &project.compositions.compositions;
    let Some((c, i)) = selected_member(&state, comps) else {
        state.status.note("no member to seat");
        return;
    };
    let (envelope, member_id) = (c.envelope, c.members[i].id.clone());
    // Measured against the set as it stands, before the edit — a member's footprint does not depend
    // on where it is being moved to.
    let footprint = match member_footprint(&c.members[i], comps, &project.library) {
        Ok(f) => f,
        Err(e) => return state.status.problem(e),
    };
    let step = seat_step(&project);
    let (at, lift) = (c.members[i].at, c.members[i].lift);
    let (next_at, next_lift) = match seated(envelope, at, lift, footprint, nudge, step) {
        Ok(v) => v,
        Err(e) => return state.status.problem(format!("`{member_id}`: {e}")),
    };
    if (next_at, next_lift) == (at, lift) {
        return;
    }
    let selected = state.selected;
    commit(state, project, |set| {
        let m = set
            .get_mut(selected)
            .and_then(|c| c.members.get_mut(i))
            .ok_or_else(|| format!("`{member_id}` is no longer there to seat"))?;
        m.at = next_at;
        m.lift = next_lift;
        // `of_fingerprint` is deliberately untouched: it records what this member's *body* was built
        // against, and moving one changes no body. Writing it here would make every seat look like a
        // re-record and would silence a real STALE badge.
        Ok(format!(
            "`{member_id}` seated at ({:.2}, {:.2}) lift {:.2}",
            next_at.0, next_at.1, next_lift
        ))
    });
}

/// Flush the selected member against a face — `Shift` + a seat key.
fn flush_member(
    mut state: ResMut<ComposeState>,
    mut project: ResMut<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let to = [
        (Action::FlushForward, Nudge::Forward),
        (Action::FlushBack, Nudge::Back),
        (Action::FlushLeft, Nudge::Left),
        (Action::FlushRight, Nudge::Right),
    ]
    .into_iter()
    .find(|(a, _)| keys::just_pressed(&input, keys.0, *a));
    let Some((_, to)) = to else { return };
    flush_selected(&mut state, &mut project, to);
}

/// The body, `pub` for the same reason [`seat_selected`] is.
pub fn flush_selected(state: &mut ComposeState, project: &mut Project, to: Nudge) {
    let comps = &project.compositions.compositions;
    let Some((c, i)) = selected_member(state, comps) else {
        state.status.note("no member to flush");
        return;
    };
    let (envelope, member_id) = (c.envelope, c.members[i].id.clone());
    let footprint = match member_footprint(&c.members[i], comps, &project.library) {
        Ok(f) => f,
        Err(e) => return state.status.problem(e),
    };
    let at = c.members[i].at;
    let next = match flushed(envelope, at, footprint, to) {
        Ok(v) => v,
        Err(e) => return state.status.problem(format!("`{member_id}`: {e}")),
    };
    if next == at {
        return;
    }
    let selected = state.selected;
    commit(state, project, |set| {
        let m = set
            .get_mut(selected)
            .and_then(|c| c.members.get_mut(i))
            .ok_or_else(|| format!("`{member_id}` is no longer there to flush"))?;
        m.at = next;
        Ok(format!("`{member_id}` flush at ({:.2}, {:.2})", next.0, next.1))
    });
}

/// Turn the selected member — a quarter bare, 15° on Shift.
fn turn_member(
    mut state: ResMut<ComposeState>,
    mut project: ResMut<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let turn = [
        (Action::TurnMemberLeft, -1.0, 90.0),
        (Action::TurnMemberRight, 1.0, 90.0),
        (Action::TurnMemberLeftFine, -1.0, 15.0),
        (Action::TurnMemberRightFine, 1.0, 15.0),
    ]
    .into_iter()
    .find(|(a, _, _)| keys::just_pressed(&input, keys.0, *a));
    let Some((_, dir, step)) = turn else { return };

    let comps = &project.compositions.compositions;
    let Some((c, i)) = selected_member(&state, comps) else {
        state.status.note("no member to turn");
        return;
    };
    let member_id = c.members[i].id.clone();
    let next = turned(c.members[i].yaw, step, dir);
    let selected = state.selected;
    commit(&mut state, &mut project, |set| {
        let m = set
            .get_mut(selected)
            .and_then(|c| c.members.get_mut(i))
            .ok_or_else(|| format!("`{member_id}` is no longer there to turn"))?;
        m.yaw = next;
        Ok(format!("`{member_id}` turned to {next:.0}"))
    });
}

/// `,` / `.` — move the selected member back or forward in paint order.
fn paint_member(
    mut state: ResMut<ComposeState>,
    mut project: ResMut<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let by: i8 = if keys::just_pressed(&input, keys.0, Action::PaintUp) {
        1
    } else if keys::just_pressed(&input, keys.0, Action::PaintDown) {
        -1
    } else {
        return;
    };
    let comps = &project.compositions.compositions;
    let Some((c, i)) = selected_member(&state, comps) else {
        return state.status.note("no member to reorder");
    };
    let member_id = c.members[i].id.clone();
    // Saturating: `i8` is deliberately narrow, and a wrap would send the front-most member behind
    // everything — the one outcome nobody presses this key for.
    let next = c.members[i].paint.saturating_add(by);
    if next == c.members[i].paint {
        return state.status.note(format!("`{member_id}` is already as far {} as it goes",
            if by > 0 { "front" } else { "back" }));
    }
    let selected = state.selected;
    commit(&mut state, &mut project, move |set| {
        let m = set
            .get_mut(selected)
            .and_then(|c| c.members.get_mut(i))
            .ok_or_else(|| format!("`{member_id}` is no longer there"))?;
        m.paint = next;
        Ok(format!("`{member_id}` paint {next}"))
    });
}

/// Take the selected member out of the composition.
///
/// The pair to the Map's capture verb: a box drag takes whatever was inside it, and this is how the
/// one piece that should not have come along leaves again.
fn drop_member(
    mut state: ResMut<ComposeState>,
    mut project: ResMut<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if !keys::just_pressed(&input, keys.0, Action::DropMember) {
        return;
    }
    let comps = &project.compositions.compositions;
    let Some((c, i)) = selected_member(&state, comps) else {
        state.status.note("no member to drop");
        return;
    };
    let member_id = c.members[i].id.clone();
    let selected = state.selected;
    commit(&mut state, &mut project, |set| {
        let c = set
            .get_mut(selected)
            .ok_or_else(|| format!("`{member_id}`'s composition is no longer there"))?;
        if i >= c.members.len() {
            return Err(format!("`{member_id}` is no longer there to drop"));
        }
        c.members.remove(i);
        // A `Location` names member ids in `props`; leaving one pointing at a member that is gone is
        // exactly the dangling reference `validate` refuses, so the refusal below would fire and the
        // write would be abandoned. Dropping the prop with the member keeps the composition loadable, and
        // an affordance left with no props at all is reported rather than silently kept.
        for l in &mut c.locations {
            l.props.retain(|p| *p != member_id);
        }
        Ok(format!("dropped `{member_id}`"))
    });
    state.member = 0;
}

/// Undo and redo, over whole member lists.
fn step_history(
    mut state: ResMut<ComposeState>,
    mut project: ResMut<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let back = keys::just_pressed(&input, keys.0, Action::UndoCompose);
    let forward = keys::just_pressed(&input, keys.0, Action::RedoCompose);
    if !(back || forward) {
        return;
    }
    let taken = if back { state.undo.pop() } else { state.redo.pop() };
    let Some(want) = taken else {
        state.status.note(if back { "nothing to undo" } else { "nothing to redo" });
        return;
    };
    let was = project.compositions.compositions.clone();
    let mut proposed = project.compositions.clone();
    proposed.compositions = want;
    let path = project
        .emerge_dir
        .join(emerge_core::composition::Compositions::FILE);
    let text = match proposed.to_ron() {
        Ok(t) => t,
        Err(e) => return state.status.problem(format!("NOT WRITTEN: {e}")),
    };
    if let Err(e) = emerge_core::ron_surgery::save_atomic(&path, &text) {
        return state.status.problem(format!("NOT WRITTEN: {e}"));
    }
    project.compositions = proposed;
    // The inverse goes on the other stack, so undo and redo are the same walk in opposite
    // directions — the shape the Map's own stack argues for.
    if back {
        state.redo.push(was);
    } else {
        state.undo.push(was);
    }
    state.status.note(if back { "undone" } else { "redone" });
}

// ------------------------------------------------------------------------------------------------
// The stage — seeing what you are seating
// ------------------------------------------------------------------------------------------------

/// A mesh standing on the Compose stage. Despawned wholesale, never diffed.
///
/// `pub` for the same reason [`ComposeState`]'s fields are: whether every group actually stood up is
/// a question about the schedule, which only `tests/headless.rs` can ask.
#[derive(Component)]
pub struct StagedMember;

/// One whole group on the strip — the parent carrying its slot's position and miniature scale.
///
/// Despawning this takes its members with it: `ChildOf` is `linked_spawn` in Bevy 0.19, so the
/// hierarchy owns the lifetime and there is no second list of what to clean up.
#[derive(Component)]
pub struct StagedGroup;

// ------------------------------------------------------------------------------------------------
// The carousel — one group at a time, with its neighbours either side
// ------------------------------------------------------------------------------------------------

/// How many groups stand either side of the focal one.
///
/// **The wings do not wrap.** With fewer groups than the carousel could show, the strip is simply
/// shorter — so running out of miniatures on one side is how the stage says "this is the end of the
/// list", which a wrapping strip would hide by showing the same group twice.
pub const WINGS: i32 = 2;

/// What each remove from the focal group multiplies its scale by.
///
/// Geometric rather than a table, so adding a wing needs no new number: the focal group stands at 1,
/// its neighbours at this, theirs at this squared.
pub(crate) const MINIATURE: f32 = 0.55;

/// **The strip's direction across the ground**, as a unit vector in XZ.
///
/// The rig looks along the XZ diagonal (`view::ISO_OFFSET` is `(12, 12, 12)`), so this is the ground
/// direction that reads as *horizontal on screen* at the default yaw — which is what makes a filmstrip
/// look like a filmstrip rather than a staircase. It is a constant and not a camera read on purpose:
/// the layout stays pure and testable, and turning the view with `Q`/`E` tilts the strip the same way
/// it tilts everything else on the stage.
const STRIP: (f32, f32) = (
    std::f32::consts::FRAC_1_SQRT_2,
    -std::f32::consts::FRAC_1_SQRT_2,
);

/// Air between one slot and the next, in metres. `SNAP`, so the strip's own pitch is on the lattice
/// everything else here already uses rather than being a second quantum.
const SLOT_GAP: f32 = emerge_core::grid::SNAP;

/// Where one composition stands on the carousel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// Index into `project.compositions.compositions`.
    pub index: usize,
    /// Remove from the focal group: `0` is the one being edited, negative is earlier in the list.
    pub offset: i32,
    /// Centre on the ground, relative to the stage origin, in metres.
    pub at: (f32, f32),
    /// Uniform scale the whole group is drawn at. `1.0` for the focal one.
    pub scale: f32,
    /// Footprint **at that scale** — what it occupies on the strip and what a click tests against.
    pub size: (f32, f32),
    /// How tall it stands at that scale. The camera has to fit this, not just the floor plan.
    pub height: f32,
}

/// The focal group and its neighbours, laid out along the strip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Carousel {
    /// Ascending by `offset`, so the strip reads in list order.
    pub slots: Vec<Slot>,
    /// Ground bounding box of everything standing, in metres.
    pub extent: (f32, f32),
    /// The tallest thing standing, at its own scale.
    pub tallest: f32,
}

impl Carousel {
    /// The group being edited — the only one that gets a lattice, a ring and full-strength colour.
    pub fn focal(&self) -> Option<&Slot> {
        self.slots.iter().find(|s| s.offset == 0)
    }
}

/// **How much floor one group takes**, or why that cannot be answered.
///
/// The two-variant match the schema already forces, and the same one [`member_footprint`] makes:
///
/// - `Bounded` answers with its **declared claim, unmodified**. Not a measurement — the declared size
///   is the tile it says it fills, and rounding or padding it would make the drawn envelope disagree
///   with the one `interface` reads faces off.
/// - `Anchored` claims no tile, so it is measured: the union of its members' own footprints. A member
///   that cannot be measured is a refusal naming the group, never a guessed box — the rule
///   [`member_footprint`] and [`crate::editor::composition_from_set`] already apply.
///
/// The measured branch is floored at one `SNAP`. A group with no members yet is the ordinary state
/// right after `N`, and a cell smaller than the editor's own quantum can be neither seen nor clicked.
pub fn footprint(
    c: &Composition,
    comps: &[Composition],
    library: &emerge_core::library::Library,
) -> Result<(f32, f32), String> {
    match c.envelope {
        Envelope::Bounded { size } => Ok((size.0, size.2)),
        Envelope::Anchored => {
            let mut span: Option<(f32, f32, f32, f32)> = None;
            for m in &c.members {
                let (w, d) = member_footprint(m, comps, library)
                    .map_err(|e| format!("`{}` cannot be sized for the stage: {e}", c.id))?;
                let (x0, x1) = (m.at.0 - w * 0.5, m.at.0 + w * 0.5);
                let (z0, z1) = (m.at.1 - d * 0.5, m.at.1 + d * 0.5);
                span = Some(match span {
                    None => (x0, x1, z0, z1),
                    Some((lo_x, hi_x, lo_z, hi_z)) => {
                        (lo_x.min(x0), hi_x.max(x1), lo_z.min(z0), hi_z.max(z1))
                    }
                });
            }
            let (w, d) = span.map_or((0.0, 0.0), |(x0, x1, z0, z1)| (x1 - x0, z1 - z0));
            let floor = emerge_core::grid::SNAP;
            Ok((w.max(floor), d.max(floor)))
        }
    }
}

/// **How tall one group stands**, which the camera needs and the floor plan does not say.
///
/// Framing on the footprint alone was measured to be wrong: four 1 × 1 tiles are 2.4 m tall, and a
/// view sized to their floor plan cut their tops off. Same two-variant match as [`footprint`] — a
/// `Bounded` group declares its height, an `Anchored` one is measured from what stands in it.
pub fn height_of(
    c: &Composition,
    comps: &[Composition],
    library: &emerge_core::library::Library,
) -> Result<f32, String> {
    match c.envelope {
        Envelope::Bounded { size } => Ok(size.1),
        Envelope::Anchored => {
            let mut tallest = 0.0f32;
            for m in &c.members {
                let top = match &m.body {
                    composition::Body::Descriptor { id, patch, .. } => {
                        let base = library.get(id).ok_or_else(|| {
                            format!(
                                "`{}` places descriptor `{id}`, which the library does not define",
                                c.id
                            )
                        })?;
                        let d = match patch {
                            Some(p) => base.patched_with(p),
                            None => base.clone(),
                        };
                        emerge_core::descriptor::placed_height(&d).ok_or_else(|| {
                            format!("`{}` holds `{}`, which is unmeasured", c.id, m.id)
                        })?
                    }
                    composition::Body::Composition { id } => {
                        let child = comps.iter().find(|k| k.id == *id).ok_or_else(|| {
                            format!("`{}` nests `{id}`, which is not a composition here", c.id)
                        })?;
                        match child.envelope {
                            Envelope::Bounded { size } => size.1,
                            // The same refusal `member_footprint` makes, for the same reason: an
                            // anchored child declares no box, so a parent cannot bound it.
                            Envelope::Anchored => {
                                return Err(format!(
                                    "`{}` nests `{id}`, which is anchored and so declares no height",
                                    c.id
                                ))
                            }
                        }
                    }
                };
                tallest = tallest.max(m.lift + top);
            }
            Ok(tallest.max(emerge_core::grid::SNAP))
        }
    }
}

/// **Lay the focal group out with its neighbours either side.**
///
/// Walks outward from the focal group, so its position never depends on how many neighbours happen to
/// exist — stepping to the next composition slides the strip past a fixed centre rather than
/// re-centring a block that changed width.
pub fn lay_out(
    comps: &[Composition],
    library: &emerge_core::library::Library,
    selected: usize,
) -> Result<Carousel, String> {
    if comps.is_empty() {
        return Ok(Carousel::default());
    }
    let selected = selected.min(comps.len() - 1);

    // Measure first, so a group that cannot be sized refuses before anything is positioned.
    let mut measured: Vec<(usize, i32, f32, (f32, f32), f32)> = Vec::new();
    for offset in -WINGS..=WINGS {
        let Some(index) = selected.checked_add_signed(offset as isize) else {
            continue;
        };
        let Some(c) = comps.get(index) else { continue };
        let scale = MINIATURE.powi(offset.abs());
        let (w, d) = footprint(c, comps, library)?;
        let h = height_of(c, comps, library)?;
        measured.push((index, offset, scale, (w * scale, d * scale), h * scale));
    }

    /// A box's reach from its own centre along the strip — half the projection of an axis-aligned
    /// `w × d` onto [`STRIP`].
    fn reach(size: (f32, f32)) -> f32 {
        (size.0 * STRIP.0.abs() + size.1 * STRIP.1.abs()) * 0.5
    }

    let Some(&focal) = measured.iter().find(|m| m.1 == 0) else {
        return Ok(Carousel::default());
    };
    let slot = |(index, offset, scale, size, height): (usize, i32, f32, (f32, f32), f32),
                distance: f32| Slot {
        index,
        offset,
        at: (distance * STRIP.0, distance * STRIP.1),
        scale,
        size,
        height,
    };

    // The focal group sits at zero whatever its neighbours do, so stepping to the next composition
    // slides the strip past a fixed centre instead of re-centring a block that changed width.
    let mut slots = vec![slot(focal, 0.0)];
    for side in [1i32, -1] {
        let mut edge = reach(focal.3);
        for step in 1..=WINGS {
            let Some(&m) = measured.iter().find(|m| m.1 == side * step) else {
                continue;
            };
            let distance = edge + SLOT_GAP + reach(m.3);
            edge = distance + reach(m.3);
            slots.push(slot(m, distance * side as f32));
        }
    }
    slots.sort_by_key(|s| s.offset);

    // **Symmetric about the focal group, deliberately.** The camera is pinned to the stage origin so
    // that the group being edited never moves, so what it has to cover is the box centred there that
    // contains every slot — not the strip's own bounding box, which drifts when one wing is short.
    let mut extent = (0.0f32, 0.0f32);
    let mut tallest = 0.0f32;
    for s in &slots {
        extent.0 = extent.0.max((s.at.0.abs() + s.size.0 * 0.5) * 2.0);
        extent.1 = extent.1.max((s.at.1.abs() + s.size.1 * 0.5) * 2.0);
        tallest = tallest.max(s.height);
    }
    Ok(Carousel { slots, extent, tallest })
}

/// **Which slot a point on the ground belongs to** — the inverse of [`lay_out`], for clicks.
///
/// Returns the *composition* index, which is what a click is for: putting the miniature you pointed
/// at into the focal position. Slots never overlap, so `position` picking the first match is
/// deterministic rather than incidental.
pub fn slot_at(carousel: &Carousel, at: (f32, f32)) -> Option<usize> {
    carousel
        .slots
        .iter()
        .find(|s| {
            (at.0 - s.at.0).abs() <= s.size.0 * 0.5 && (at.1 - s.at.1).abs() <= s.size.1 * 0.5
        })
        .map(|s| s.index)
}

/// **The orthographic viewport height that shows the whole strip.**
///
/// Two extents matter and the first draft only had one. The rig looks along the XZ diagonal, so a
/// `w × d` ground rectangle spreads `(w + d) / √2` across the screen; but the groups also *stand up*,
/// and a vertical metre projects to `cos(elevation)` of one. Framing on the floor plan alone cut the
/// tops off four 2.4 m tiles — measured in a captured frame, not predicted.
///
/// The horizontal fit assumes a square viewport, which is exact for the debugger's mirror camera and
/// conservative for a window with two panels eating its width.
///
/// Returned unclamped: the caller decides what to do when it exceeds [`crate::view::MAX_ZOOM`],
/// because that is a thing to say out loud rather than to silently crop.
pub fn framing_height(extent: (f32, f32), tallest: f32) -> f32 {
    /// A little air around the strip, so the outermost miniature is not flush with the window edge.
    const MARGIN: f32 = 1.15;
    let spread = (extent.0 + extent.1) * std::f32::consts::FRAC_1_SQRT_2;
    let elevation = crate::view::ISO_ELEVATION;
    let vertical = spread * elevation.sin() + tallest * elevation.cos();
    (vertical.max(spread) * MARGIN).max(crate::tiles::TILE_VIEW_HEIGHT)
}

/// **The carousel as it currently stands**, written by [`restage_group`] and read by everything that
/// has to agree with it — the gizmos, the labels, the click, the camera.
///
/// A resource rather than four calls to [`lay_out`], because four callers computing the same layout is
/// four chances for them to disagree about where a group is. One writer, one answer.
#[derive(Resource, Default)]
pub struct StagedCarousel(pub Carousel);

/// The envelope of a group that is not the focal one.
///
/// [`ACCENT`] at half luminance: the same hue, so a reader sees one kind of thing at two weights,
/// rather than a second colour to learn.
const ENVELOPE_IDLE: Color = Color::srgb(0.45, 0.33, 0.12);

/// One group's id, floating over its slot. Carries the slot's index into the carousel.
#[derive(Component)]
struct SlotLabel(usize);

/// How far below the slot's projected centre the label sits, in logical pixels.
const LABEL_DROP: f32 = 4.0;

/// Label type sizes: the focal group's, then a miniature's.
///
/// Two constants rather than one with a factor, because [`place_labels`] has to centre a string it
/// did not set the size of — deriving the advance from the same pair keeps the two systems from
/// disagreeing about how wide a label is. `TextFont::font_size` is a `FontSize` in Bevy 0.19 and does
/// not divide, so reading it back is not the shortcut it looks like.
const LABEL_PX: (f32, f32) = (12.0, 9.0);

/// Advance of one glyph at `LABEL_PX.0`, in logical pixels.
///
/// The shipped face is `FiraMono-Regular.ttf` — **monospace** — so a string's width is its length
/// times this, and centring needs no text-layout round trip. If the face is ever changed to a
/// proportional one, labels drift off-centre; they do not break.
const LABEL_CHAR_W: f32 = 6.6;

/// **Stand the focal group up, with its neighbours either side, each through `composition::expand`.**
///
/// The same three calls `editor::redraw_stamps` makes — build a scratch map, expand a stamp of the
/// group against it, `stack::resolve_y` over a map carrying both — so **what this tab shows is what a
/// stamp produces**. A second interpretation of a composition would be a preview that lies, which is
/// the argument `spawn_piece` itself was written for.
///
/// # One scratch map per group, not one for the strip
///
/// `expand` takes a slice, so staging all of them in a single map was the obvious shape and it is the
/// wrong one. [`composition::interface`] derives a group's faces against **its own envelope as a
/// scratch map** — origin at zero, `bounds` the declared size. A shared map would give every group the
/// strip's bounds instead, so a ceiling-mounted member would hang from a ceiling belonging to whatever
/// else happened to be on screen, and the stage would be showing something other than what the
/// interface beside it was derived from. Per-group maps keep the picture and the tokens answering the
/// same question, and they make a failure local: a group that will not resolve says so by name and the
/// others still stand.
///
/// # The miniatures are scaled by a parent, and that is why the origin is zero
///
/// Each group is spawned under one parent carrying its slot's translation *and* scale, with the pieces
/// themselves built at a local origin. Scaling a group any other way would mean scaling each piece
/// about the stage rather than about its own group, which shears the arrangement rather than shrinking
/// it. Despawning the parent takes the pieces with it — `ChildOf` is `linked_spawn` in Bevy 0.19.
fn restage_group(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mode: Res<Mode>,
    mut state: ResMut<ComposeState>,
    mut carousel_out: ResMut<StagedCarousel>,
    project: Res<Project>,
    drawn: Query<Entity, With<StagedGroup>>,
) {
    if !(project.is_changed() || state.is_changed() || mode.is_changed()) {
        return;
    }
    if *mode != Mode::Compose {
        // Left the tab. A system that stops running cannot despawn what it drew, so this is not
        // gated on the mode — but it also must not re-despawn an empty stage every frame.
        if state.staged.is_some() {
            for e in &drawn {
                commands.entity(e).despawn();
            }
            state.staged = None;
        }
        return;
    }
    let comps = &project.compositions.compositions;
    // **The loop-breaker.** See `ComposeState::staged`: writing a problem below re-marks this
    // resource changed, which re-triggers the gate above. Claiming the key before staging means a
    // group that cannot resolve is reported once rather than respawning the strip every frame.
    if state
        .staged
        .as_ref()
        .is_some_and(|(sel, c)| *sel == state.selected && c == comps)
    {
        return;
    }
    for e in &drawn {
        commands.entity(e).despawn();
    }
    state.staged = Some((state.selected, comps.clone()));

    let carousel = match lay_out(comps, &project.library, state.selected) {
        Ok(c) => c,
        Err(e) => {
            // Nothing can be laid out, so nothing can be drawn over it either.
            carousel_out.0 = Carousel::default();
            return state.status.problem(e);
        }
    };
    for slot in &carousel.slots {
        let Some(c) = comps.get(slot.index) else { continue };
        let size = match c.envelope {
            Envelope::Bounded { size } => size,
            // An anchored group claims no tile. It still stands up — it is furniture somewhere — and
            // the scratch map just needs to be big enough not to clip it.
            Envelope::Anchored => (64.0, 8.0, 64.0),
        };
        // **Origin zero**: the pieces come out in the group's own space, and the parent below puts
        // that space on the strip at the slot's scale.
        let scratch = emerge_core::map::Map {
            version: emerge_core::map::MAP_VERSION,
            name: "compose_stage".to_owned(),
            origin: (0.0, 0.0, 0.0),
            bounds: size,
            placements: Vec::new(),
            stamps: Vec::new(),
            locations: Vec::new(),
            note: None,
        };
        // **Nothing to expand is not an error here.** A composition with no members yet is the
        // ordinary state right after `N`; `expand` refuses to *stamp* one, which is right for a map
        // and wrong for the stage that is showing you the one you are filling. Its envelope still
        // draws — `draw_stage` reads that off the schema, not off the members.
        if c.members.is_empty() {
            continue;
        }
        let stamp = emerge_core::composition::Stamped {
            id: "staged".to_owned(),
            of: c.id.clone(),
            at: (0.0, 0.0),
            ..Default::default()
        };
        let expanded = match composition::expand(&scratch, &[stamp], comps, &project.library) {
            Ok(e) => e,
            // Loud. An empty patch of floor where a composition should be, with nothing saying why,
            // is the failure this editor's own notes call the worst it had.
            Err(e) => {
                state.status.problem(format!("`{}` does not resolve: {e}", c.id));
                continue;
            }
        };
        let mut with_rows = scratch.clone();
        with_rows.placements.extend(expanded.placements.iter().cloned());
        let ys = match emerge_core::stack::resolve_y(&with_rows, &project.library) {
            Ok(ys) => ys,
            Err(e) => {
                state.status.problem(format!("`{}` has no height: {e}", c.id));
                continue;
            }
        };
        let parent = commands
            .spawn((
                Name::new(format!("staged {}", c.id)),
                StagedGroup,
                Transform::from_translation(
                    COMPOSE_STAGE + Vec3::new(slot.at.0, 0.0, slot.at.1),
                )
                .with_scale(Vec3::splat(slot.scale)),
                Visibility::Inherited,
            ))
            .id();
        for (k, p) in expanded.placements.iter().enumerate() {
            let Some(base) = project.library.get(&p.descriptor) else {
                continue;
            };
            let d = match &p.patch {
                Some(patch) => base.patched_with(patch),
                None => base.clone(),
            };
            let Some(&y) = ys.get(k) else { continue };
            if let Some(e) = crate::editor::spawn_piece(
                &mut commands,
                &assets,
                &d,
                p.at,
                p.yaw,
                p.tip,
                scratch.origin,
                y,
            ) {
                commands.entity(e).insert(StagedMember);
                // The stage must show what the map will: paint decides what is seen where two members
                // coincide, and a preview that ignores it is a preview that lies.
                if p.paint != 0 {
                    commands.entity(e).insert(emerge_bevy::Paint(p.paint));
                }
                commands.entity(parent).add_child(e);
            }
        }
    }
    // **A strip too big to be seen whole says so.** The rig stops at `MAX_ZOOM`, so past that the
    // outer miniatures are cropped — and cropped read as complete is exactly the silent truncation
    // `fill::box_fill` grew its `truncated` flag to avoid.
    let want = framing_height(carousel.extent, carousel.tallest);
    if want > crate::view::MAX_ZOOM {
        state.status.problem(format!(
            "this composition and its neighbours need a {want:.0} m view and the camera stops at \
             {:.0} m \
             — the outer miniatures are cropped.",
            crate::view::MAX_ZOOM,
        ));
    }
    // Published last, so everything drawn over the strip is drawn over the one that just went up.
    carousel_out.0 = carousel;
}

/// **Every visible group's envelope, and the lattice the focal one seats on.**
///
/// Drawn rather than spawned: these are not things in the world, they are the tiles the compositions
/// claim. An anchored group gets neither box nor lattice, because it claims none — an invented box
/// would be exactly the guess `seated` refuses to make.
///
/// The miniatures draw their boxes at [`ENVELOPE_IDLE`] so the strip reads as *claimed tiles* rather
/// than floating meshes; only the focal group draws at full [`ACCENT`], and only it gets the lattice
/// and the member ring, because those answer "what am I seating, and where" about one group.
fn draw_stage(
    state: Res<ComposeState>,
    project: Res<Project>,
    carousel: Res<StagedCarousel>,
    mut gizmos: Gizmos,
) {
    let step = emerge_core::grid::SNAP;
    for slot in &carousel.0.slots {
        let Some(c) = project.compositions.compositions.get(slot.index) else {
            continue;
        };
        let Envelope::Bounded { size } = c.envelope else {
            continue;
        };
        let base = COMPOSE_STAGE + Vec3::new(slot.at.0, 0.0, slot.at.1);
        let focal = slot.offset == 0;
        // Scaled with its group, so the box stays the tile the miniature is actually drawing.
        let size = (size.0 * slot.scale, size.1 * slot.scale, size.2 * slot.scale);
        gizmos.cube(
            Transform::from_translation(base + Vec3::new(0.0, size.1 * 0.5, 0.0))
                .with_scale(Vec3::new(size.0, size.1, size.2)),
            if focal { crate::chrome::ACCENT } else { ENVELOPE_IDLE },
        );
        if !focal {
            continue;
        }
        // The seating lattice on its floor — `grid::SNAP`, the same quantum `seated` steps by and the
        // Map snaps to. Drawing anything else here would be the drawn-grid-disagrees-with-the-snap bug
        // that `GridSpacing`'s note records. The focal group is always at scale 1, so this is the
        // lattice a seat step actually lands on rather than a scaled picture of one.
        let cells = (
            (size.0 / step).round().max(1.0) as u32,
            (size.2 / step).round().max(1.0) as u32,
        );
        gizmos
            .grid(
                Isometry3d::new(
                    base + Vec3::Y * 0.002,
                    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                ),
                UVec2::new(cells.0, cells.1),
                Vec2::splat(step),
                crate::editor::GRID_LINE,
            )
            .outer_edges();
        // The member being seated, ringed where it stands.
        if let Some(m) = c.members.get(state.member.min(c.members.len().saturating_sub(1))) {
            gizmos.circle(
                Isometry3d::new(
                    base + Vec3::new(m.at.0, 0.01, m.at.1),
                    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                ),
                step * 0.75,
                crate::chrome::ACCENT,
            );
        }
    }
}

/// **Which group is which, said in the viewport rather than only in the panel.**
///
/// Rebuilt whenever the strip or the selection changes; positioned every frame by [`place_labels`].
///
/// The label carries the group's **state**, not just its id, and that is a lesson someone else paid
/// for. `pcgbook_chapter11` on Tanagra: *"Later versions of Tanagra altered the UI to make it clearer
/// what geometry was 'pinned' and what was not."* This tab has the same latent gap — `armed` and the
/// focal group are distinguished in the panel by a `>*` marker and nowhere on the stage — so the tint
/// says focal and the trailing `*` says armed, matching the marks the list already uses.
fn rebuild_labels(
    mut commands: Commands,
    carousel: Res<StagedCarousel>,
    state: Res<ComposeState>,
    project: Res<Project>,
    existing: Query<Entity, With<SlotLabel>>,
) {
    if !(carousel.is_changed() || state.is_changed() || project.is_changed()) {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    for (k, slot) in carousel.0.slots.iter().enumerate() {
        let Some(c) = project.compositions.compositions.get(slot.index) else {
            continue;
        };
        let armed = state.armed.as_deref() == Some(c.id.as_str());
        commands.spawn((
            SlotLabel(k),
            Text::new(if armed { format!("{} *", c.id) } else { c.id.clone() }),
            // The miniatures name themselves in smaller type, so the strip reads as one focal group
            // among neighbours rather than as five equal things.
            TextFont::from_font_size(if slot.offset == 0 { LABEL_PX.0 } else { LABEL_PX.1 }),
            TextColor(if slot.offset == 0 { ACCENT } else { DIM }),
            Node {
                position_type: PositionType::Absolute,
                // Hidden until `place_labels` has a projection to put it at — a label at (0,0) for
                // one frame is a label in the wrong place.
                display: Display::None,
                ..default()
            },
            // **Never steal the wheel.** `view::drive` gives scroll to whatever is under the cursor by
            // testing `Hovered`, so a pickable node floating over the stage would silently kill zoom.
            bevy::picking::Pickable::IGNORE,
        ));
    }
}

/// **Put each label under its slot**, in logical window pixels.
///
/// The single owner of a label's `display`, so there is one rule for when a label is on screen rather
/// than two systems disagreeing about it. Hidden when the tab is not live and when the projection has
/// no answer.
fn place_labels(
    mode: Res<Mode>,
    carousel: Res<StagedCarousel>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<crate::view::MainCamera>>>,
    mut labels: Query<(&SlotLabel, &Text, &mut Node)>,
) {
    let Some(camera) = camera.filter(|_| *mode == Mode::Compose) else {
        for (_, _, mut node) in &mut labels {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
        return;
    };
    let (cam, cam_tf) = *camera;
    for (label, text, mut node) in &mut labels {
        let placed = carousel.0.slots.get(label.0).and_then(|slot| {
            let advance = LABEL_CHAR_W
                * if slot.offset == 0 { 1.0 } else { LABEL_PX.1 / LABEL_PX.0 };
            cam.world_to_viewport(cam_tf, COMPOSE_STAGE + Vec3::new(slot.at.0, 0.0, slot.at.1))
                .ok()
                .map(|p| (p, advance))
        });
        match placed {
            // Centred by measuring the string, which works because the shipped face is monospace —
            // see `LABEL_CHAR_W`.
            Some((p, advance)) => {
                let half = text.0.chars().count() as f32 * advance * 0.5;
                let (left, top) = (Val::Px(p.x - half), Val::Px(p.y + LABEL_DROP));
                // **Guarded against the no-op write**, the way `editor::refresh_status` is: `Node` is
                // change-detected, and touching it every frame makes Bevy re-lay the whole UI tree
                // on every frame the camera happens to be still.
                if node.display != Display::Flex || node.left != left || node.top != top {
                    node.display = Display::Flex;
                    node.left = left;
                    node.top = top;
                }
            }
            None => {
                if node.display != Display::None {
                    node.display = Display::None;
                }
            }
        }
    }
}

/// **Step the carousel — the previous or next composition becomes the focal one.**
///
/// Takes the twelfth and last row of this tab's key census. Its own keys rather than the arrows
/// because the arrows belong to whichever of the three lists has focus: stepping the strip while
/// editing a member would otherwise cost `left left up right right`, and the whole point of the
/// carousel is that moving between groups is one keypress from wherever you are.
///
/// Wraps, like [`walk`] does, so the list is a ring to move through even though the strip itself does
/// not wrap — running out of miniatures is how the stage says which end you are at.
fn step_carousel(
    keys: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    project: Res<Project>,
    mut state: ResMut<ComposeState>,
) {
    let by = match (
        keys::just_pressed(&keys, live.0, Action::CarouselNext),
        keys::just_pressed(&keys, live.0, Action::CarouselPrev),
    ) {
        (true, false) => 1i64,
        (false, true) => -1,
        _ => return,
    };
    let n = project.compositions.compositions.len();
    if n == 0 {
        return;
    }
    state.selected = ((state.selected as i64 + by).rem_euclid(n as i64)) as usize;
    // A different group has different members; leaving the old index would point the seat verbs at
    // whatever happened to sit there — the same reason `walk` resets it.
    state.member = 0;
}

/// **Click a miniature to bring it to the middle** — the strip lies on the ground plane, so this is
/// arithmetic rather than a raycast.
///
/// Reads [`crate::view::Pointer`] and never the `Window`: a system that asks the window directly is
/// undrivable by an agent and is a second opinion about where the cursor is.
fn pick_slot(
    buttons: Res<ButtonInput<MouseButton>>,
    pointer: Res<crate::view::Pointer>,
    carousel: Res<StagedCarousel>,
    hovered_ui: Query<&Hovered>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<crate::view::MainCamera>>>,
    mut state: ResMut<ComposeState>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    // A click that lands on a panel belongs to the panel, the same test `view::drive` uses for scroll.
    if hovered_ui.iter().any(|h| h.0) {
        return;
    }
    let Some(camera) = camera else { return };
    let (cam, cam_tf) = *camera;
    let Some(ground) = crate::view::cursor_ground(pointer.0, cam, cam_tf) else {
        return;
    };
    let at = (ground.x - COMPOSE_STAGE.x, ground.z - COMPOSE_STAGE.z);
    let Some(i) = slot_at(&carousel.0, at) else { return };
    if state.selected != i {
        state.selected = i;
        state.member = 0;
    }
}

/// Clicking **NEW** opens the same name field `N` opens — never a second copy of the verb.
///
/// # It has to ask *which* entity was activated
///
/// This took `On<Activate>` and then tested `buttons.is_empty()` — which asks whether a
/// `NewGroupButton` exists **anywhere in the world**, and one always does. An observer fires for every
/// `Activate` regardless of target, so clicking any row of the PLACE list on the right opened the
/// name field: an author picking a mesh was asked to name a new composition instead. `on_mesh_click`
/// directly below has always done it right, which is the shape to copy — `get(activate.entity)`
/// answers "was it this one", and an empty result is the answer "no", not an error.
fn on_new_group_click(
    activate: On<Activate>,
    buttons: Query<&NewGroupButton>,
    mut state: ResMut<ComposeState>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    state.naming = Some(String::new());
    state.status.note("name the new composition, then Enter — Esc to abandon it");
}

/// Clicking a mesh row selects it **and takes the focus**, so `Enter` then means "add this one".
///
/// Taking the focus is the point: picking something to place is an unambiguous statement about which
/// list you are working in, and the Map palette's own click handler makes the same argument about
/// arming a piece returning you to placing.
fn on_mesh_click(
    activate: On<Activate>,
    rows: Query<&MeshRow>,
    mut state: ResMut<ComposeState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    state.mesh = row.0;
    state.focus = Pane::Meshes;
}

/// **The mesh list, and the header that says whether it has the arrows.**
///
/// Rebuilt wholesale when the selection or the project moves, like every other list here — a diffing
/// version would be a second opinion about what is on screen.
fn rebuild_meshes(
    mut commands: Commands,
    state: Res<ComposeState>,
    project: Res<Project>,
    thumbs: Option<Res<crate::thumbs::Thumbnails>>,
    list: Query<Entity, With<MeshList>>,
    mut header: Query<(&mut Text, &mut TextColor), With<MeshHeader>>,
    rows: Query<Entity, With<MeshRow>>,
) {
    if !(state.is_changed() || project.is_changed()) {
        return;
    }
    // **The focus, drawn.** Two lists cannot both look live, and an author who cannot tell which one
    // the arrows move is back to the modal picker this replaced.
    let focused = state.focus == Pane::Meshes;
    for (mut text, mut colour) in &mut header {
        let want = if focused { "PLACE  <- arrows" } else { "PLACE" };
        if text.0 != want {
            text.0 = want.to_owned();
        }
        colour.0 = if focused { ACCENT } else { LABEL };
    }
    let Ok(root) = list.single() else { return };
    for e in &rows {
        commands.entity(e).despawn();
    }
    let at = state.mesh.min(project.library.descriptors.len().saturating_sub(1));
    commands.entity(root).with_children(|p| {
        for (i, d) in project.library.descriptors.iter().enumerate() {
            let picked = i == at;
            let mut row = p.spawn((
                UiButton,
                Hovered::default(),
                MeshRow(i),
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    ..default()
                },
                BackgroundColor(if picked { crate::chrome::ROW_SELECTED } else { crate::chrome::ROW_BG }),
            ));
            row.with_children(|r| {
                // The same thumbnails the Map palette uses — baked once at startup, keyed by id.
                if let Some(image) = thumbs.as_ref().and_then(|t| t.image(&d.id)) {
                    r.spawn((ImageNode::new(image), Node { width: Val::Px(28.0), height: Val::Px(28.0), ..default() }));
                }
                r.spawn((
                    Text::new(d.id.clone()),
                    TextColor(if picked { ACCENT } else { TEXT }),
                    TextFont::from_font_size(11.0),
                ));
            });
        }
    });
}

/// **Everything the panel says, derived on the spot.**
///
/// No count and no badge is stored: staleness and the interface are recomputed from the library and
/// the composition set each time this runs, so a panel cannot disagree with the files it is about.
/// That is the same discipline `emerge_core::census` applies to counts, for the same reason.
fn rebuild(
    mut commands: Commands,
    state: Res<ComposeState>,
    project: Res<Project>,
    body: Query<Entity, With<ComposeBody>>,
    lines: Query<Entity, With<ComposeLine>>,
) {
    if !(state.is_changed() || project.is_changed()) {
        return;
    }
    let Ok(root) = body.single() else {
        return;
    };
    for e in &lines {
        commands.entity(e).despawn();
    }

    let comps = &project.compositions.compositions;
    let mut rows: Vec<(String, Color)> = Vec::new();

    // **The two modal blocks first**, because while one is open it is the only thing the keyboard
    // reaches, and a field you cannot see is a field you cannot finish.
    // The name field is not drawn here any more — it is `chrome::NameBox`, centred over the
    // viewport, shared with the Map's own capture verb. Two places asking for the same thing made one
    // act look like two.
    if state.focus == Pane::Groups {
        rows.push((format!("{}  <- arrows", Pane::Groups.label()), ACCENT));
    }
    // **Where you are in the list, and how to move.** The stage shows the focal group among its
    // neighbours, but not how many there are or how far in you have got — and `status.note` says it
    // on the keypress and is then replaced by the next message, which is the wrong lifetime for it.
    if !comps.is_empty() {
        rows.push((
            format!(
                "  {} of {}   {} / {}",
                state.selected.min(comps.len() - 1) + 1,
                comps.len(),
                keys::chord(Action::CarouselPrev),
                keys::chord(Action::CarouselNext),
            ),
            DIM,
        ));
    }
    if comps.is_empty() {
        rows.push((
            "No groups. `compositions.ron` beside library.ron defines them; a project with none is \
             a project that stamps nothing, not a broken one."
                .to_owned(),
            DIM,
        ));
    }

    for (i, c) in comps.iter().enumerate() {
        let armed = state.armed.as_deref() == Some(c.id.as_str());
        let marker = match (i == state.selected, armed) {
            (true, true) => ">*",
            (true, false) => "> ",
            (false, true) => " *",
            (false, false) => "  ",
        };
        let envelope = match c.envelope {
            Envelope::Anchored => "anchored".to_owned(),
            Envelope::Bounded { size } => format!("bounded {:.1}x{:.1}x{:.1}", size.0, size.1, size.2),
        };
        rows.push((
            format!("{marker}{}  —  {} member(s), {envelope}", c.id, c.members.len()),
            if i == state.selected { ACCENT } else { TEXT },
        ));
    }

    if let Some(c) = comps.get(state.selected) {
        rows.push((String::new(), TEXT));
        rows.push((
            format!(
                "MEMBERS OF `{}`{}",
                c.id,
                if state.focus == Pane::Members { "  <- arrows" } else { "" }
            ),
            if state.focus == Pane::Members { ACCENT } else { LABEL },
        ));
        let at = state.member.min(c.members.len().saturating_sub(1));
        for (i, m) in c.members.iter().enumerate() {
            // The seating cursor, in the same `> ` shape the composition list above uses — one marker
            // shape for "the thing a verb will act on", read the same way in both lists.
            let marker = if i == at { "> " } else { "  " };
            rows.push((
                format!("{marker}{}", describe_member(m)),
                if i == at { ACCENT } else { TEXT },
            ));
        }
        affordances(&mut rows, c);
        detail(&mut rows, c, comps, &project);
    }

    // The receipt only. The refusal is the block under the title — see `chrome::Status`.
    if !state.status.note_text().is_empty() {
        rows.push((String::new(), TEXT));
        rows.push((state.status.note_text().to_owned(), ACCENT));
    }

    commands.entity(root).with_children(|p| {
        for (text, colour) in rows {
            p.spawn((
                Text::new(text),
                TextColor(colour),
                TextFont::from_font_size(11.0),
                ComposeLine,
            ));
        }
    });
}

// ------------------------------------------------------------------------------------------------
// Seating — the pure half
// ------------------------------------------------------------------------------------------------

/// One lattice step, in the frame [`composition::Member::at`] is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nudge {
    /// −Z, matching [`emerge_core::wfc`]'s `N`.
    Forward,
    Back,
    Left,
    Right,
    Up,
    Down,
}

/// **The axis-aligned box that contains a `w × d` footprint turned `yaw` degrees.**
///
/// Exact at every angle rather than only at the quarters, so a chair drawn up to a table at 15° is
/// bounded by the box it actually occupies. At yaw 0 this is `(w, d)`; at 90 it is `(d, w)`.
pub fn turned_footprint((w, d): (f32, f32), yaw_deg: f32) -> (f32, f32) {
    let (s, c) = yaw_deg.to_radians().sin_cos();
    (w * c.abs() + d * s.abs(), w * s.abs() + d * c.abs())
}

/// **How much floor a member takes**, or why that cannot be answered.
///
/// A nested group's footprint is its own envelope, which is the whole point of `Bounded`: it declares
/// the tile it claims. An `Anchored` child declares none, so a parent cannot bound it — that is a
/// refusal naming the child rather than a made-up box, the same rule
/// [`crate::editor::composition_from_set`] applies to an unmeasured piece.
pub fn member_footprint(
    m: &composition::Member,
    comps: &[Composition],
    library: &emerge_core::library::Library,
) -> Result<(f32, f32), String> {
    match &m.body {
        composition::Body::Descriptor { id, tip, patch, .. } => {
            let base = library.get(id).ok_or_else(|| {
                format!("`{}` places descriptor `{id}`, which the library does not define", m.id)
            })?;
            let d = match patch {
                Some(p) => base.patched_with(p),
                None => base.clone(),
            };
            let (w, _, dep) = emerge_core::descriptor::tipped_extents(&d, *tip)
                .ok_or_else(|| format!("`{}` is unmeasured, so it cannot be seated", m.id))?;
            Ok(turned_footprint((w, dep), m.yaw))
        }
        composition::Body::Composition { id } => {
            let child = comps
                .iter()
                .find(|c| c.id == *id)
                .ok_or_else(|| format!("`{}` nests `{id}`, which is not a composition here", m.id))?;
            match child.envelope {
                Envelope::Bounded { size } => Ok(turned_footprint((size.0, size.2), m.yaw)),
                Envelope::Anchored => Err(format!(
                    "`{}` nests `{id}`, which is anchored and so claims no tile — there is no box to \
                     seat it inside. Give `{id}` a bounded envelope first.",
                    m.id
                )),
            }
        }
    }
}

/// **Seat a member one step, or refuse and say why.** Returns the new `(at, lift)`.
///
/// # The lattice is not defined here
///
/// Horizontal steps are [`emerge_core::grid::SNAP`] and vertical ones are `SNAP / divisions` — the
/// exact quanta [`crate::editor`]'s `snap` and `lift_step` already apply to every `Placed`. A second
/// quantum for the same act is the mistake `GridSpacing`'s note records: the drawn grid said 1.0 m
/// while the snap was 0.5 m, and an author cannot see which one a piece obeyed.
///
/// # It refuses rather than clamping
///
/// A step that would put any part of the member outside the envelope is an error, not a silently
/// shortened move. Clamping would answer a key with a smaller version of what was asked, which is the
/// one thing pcgbook ch.11 says a mixed-initiative tool must not do — the computer acting on a model
/// other than the one in the author's head. The bound is the member's **footprint**, not its centre:
/// a 1 m wall centred on the edge of a 1 m envelope is half outside it.
///
/// `lift` is deliberately unbounded. It is a nudge on top of whatever the mount resolves to, not an
/// absolute height, so there is no honest bound to give it — a ceiling-mounted member's useful lifts
/// are negative. A lift that carries a member out of its envelope shows up where it should, as a face
/// that presents nothing.
pub fn seated(
    envelope: Envelope,
    at: (f32, f32),
    lift: f32,
    footprint: (f32, f32),
    nudge: Nudge,
    lift_step: f32,
) -> Result<((f32, f32), f32), String> {
    use emerge_core::grid::SNAP;
    let (dx, dz, dy) = match nudge {
        Nudge::Forward => (0.0, -SNAP, 0.0),
        Nudge::Back => (0.0, SNAP, 0.0),
        Nudge::Left => (-SNAP, 0.0, 0.0),
        Nudge::Right => (SNAP, 0.0, 0.0),
        Nudge::Up => (0.0, 0.0, lift_step),
        Nudge::Down => (0.0, 0.0, -lift_step),
    };
    let next = (snap_to(at.0 + dx), snap_to(at.1 + dz));
    if dy != 0.0 {
        return Ok((at, lift + dy));
    }
    let Envelope::Bounded { size } = envelope else {
        return Err(
            "this composition is anchored, so it claims no tile and has no lattice to seat inside. \
             Anchored groups are furniture standing somewhere; a bounded one is a module that has to \
             meet a wall."
                .to_owned(),
        );
    };
    let (half_x, half_z) = (size.0 * 0.5, size.2 * 0.5);
    let (fw, fd) = (footprint.0 * 0.5, footprint.1 * 0.5);
    // A hair of tolerance, for the same reason `adjacency::EDGE_EPSILON` has one: a piece measured
    // 1.0000001 m flush against a 1 m envelope is flush, and refusing it would be arithmetic showing
    // through as a rule the author cannot see the cause of.
    const SLOP: f32 = 1e-3;
    if next.0 - fw < -half_x - SLOP || next.0 + fw > half_x + SLOP {
        return Err(format!(
            "that would put it {:.2} m outside the group's {:.1} m width — a member has to stay \
             inside the tile the group claims",
            ((next.0.abs() + fw) - half_x).max(0.0),
            size.0
        ));
    }
    if next.1 - fd < -half_z - SLOP || next.1 + fd > half_z + SLOP {
        return Err(format!(
            "that would put it {:.2} m outside the group's {:.1} m depth — a member has to stay \
             inside the tile the group claims",
            ((next.1.abs() + fd) - half_z).max(0.0),
            size.2
        ));
    }
    Ok((next, lift))
}

/// **Put a member flush against one face of the envelope**, or refuse and say why.
///
/// # The verb a wall wants, and why the lattice is not it
///
/// Measured on the shipped site kit while authoring real tiles: `site/wall` is **0.1 m** thick, so
/// sitting it flush inside a 1 m tile puts its centre at **0.45** — not a multiple of
/// [`emerge_core::grid::SNAP`], and therefore unreachable by [`seated`] however many times you press.
/// A uniform grid is the right primitive for furniture and the wrong one for architecture.
///
/// So the offset comes from the member's **own measured thickness** rather than from a step: this is
/// the *relative* split value to [`seated`]'s absolute one (Müller et al., *Procedural Modeling of
/// Buildings*, `10.1145/1179352.1141931`, which types split values absolute or relative for exactly
/// this reason), and it is Tutenel et al.'s *"snapping to the nearest valid location"*
/// (`10.1609/aiide.v6i1.12398`).
///
/// # It is what makes a composition a tile
///
/// [`composition::interface`] reads a member on a face when its box edge is within
/// `adjacency::EDGE_EPSILON` of the envelope's. Flush puts it exactly there, so the composition presents
/// what it is made of — and the envelope stays exactly the tile, which is what lets
/// `grammar::learn` accept a composition of floor-plus-wall where the 0.1 × 1.0 wall alone is refused as
/// the wrong size for the cell.
///
/// Only the axis being flushed to moves; the other is left alone, so flushing north then west is a
/// corner rather than two competing answers.
pub fn flushed(
    envelope: Envelope,
    at: (f32, f32),
    footprint: (f32, f32),
    to: Nudge,
) -> Result<(f32, f32), String> {
    let Envelope::Bounded { size } = envelope else {
        return Err(
            "this composition is anchored, so it claims no tile and has no face to flush against"
                .to_owned(),
        );
    };
    let (half_x, half_z) = (size.0 * 0.5, size.2 * 0.5);
    let (fw, fd) = (footprint.0 * 0.5, footprint.1 * 0.5);
    match to {
        Nudge::Forward => Ok((at.0, -half_z + fd)),
        Nudge::Back => Ok((at.0, half_z - fd)),
        Nudge::Left => Ok((-half_x + fw, at.1)),
        Nudge::Right => Ok((half_x - fw, at.1)),
        // Deliberately not answerable. `lift` is a nudge on top of whatever the mount resolves to
        // rather than an absolute height, so "flush to the floor" would have to invent which datum it
        // meant — and a ceiling-mounted member's useful lifts are negative.
        Nudge::Up | Nudge::Down => Err(
            "there is no face to flush against vertically — lift is a nudge on the mount, not a \
             height, so the floor and the ceiling are not where a member's `lift` is measured from"
                .to_owned(),
        ),
    }
}

/// **One seat step, metres** — `grid::SNAP / seating_divisions`, 125 mm at the default 4.
///
/// The **seating** number, never the face one. They were a single `policy.divisions` until the split:
/// edge tokens belong to a face and seating belongs to a volume, and one number serving both meant a
/// finer seat cost a re-index of every authored token. [`emerge_core::policy::Policy`] carries the
/// argument.
///
/// Seats are the multiples of this measured from the envelope's centre in X/Z and its floor in Y, so
/// the centre is always a seat and nudging out and back returns exactly.
pub fn seat_step(project: &Project) -> f32 {
    emerge_core::grid::SNAP / project.policy.seating_divisions.max(1) as f32
}

/// The authoring grid, rounded the way [`crate::editor`] rounds it. One rule, two callers.
fn snap_to(v: f32) -> f32 {
    (v / emerge_core::grid::SNAP).round() * emerge_core::grid::SNAP
}

/// **Turn a member, landing on a multiple of `step`.**
///
/// Rounds the *result* rather than adding to whatever was there, so a member that arrived at 47°
/// from somewhere else comes back onto the lattice with one press instead of staying 2° off forever.
/// That matters because [`emerge_core::adjacency::quarter_turns`] refuses a yaw off the quarters and
/// names the piece — a tokened member stuck at 47° makes the whole group's interface underivable.
pub fn turned(yaw: f32, step: f32, dir: f32) -> f32 {
    if !(step.is_finite() && step > 0.0) {
        return yaw;
    }
    let next = (yaw / step).round() * step + dir * step;
    next.rem_euclid(360.0)
}

/// One member as a line: what it is, where it sits, and what it rests on.
fn describe_member(m: &composition::Member) -> String {
    let (what, extra) = match &m.body {
        composition::Body::Descriptor { id, tip, on, patch } => {
            let mut notes: Vec<String> = Vec::new();
            if *tip != (0, 0) {
                notes.push(format!("tip {tip:?}"));
            }
            if let Some(host) = on {
                notes.push(format!("on `{host}`"));
            }
            if patch.is_some() {
                notes.push("patched".to_owned());
            }
            (id.clone(), notes.join(", "))
        }
        composition::Body::Composition { id } => (format!("[{id}]"), "nested".to_owned()),
    };
    let where_ = format!("({:.1}, {:.1})", m.at.0, m.at.1);
    let yaw = if m.yaw == 0.0 { String::new() } else { format!(" yaw {:.0}", m.yaw) };
    let paint = if m.paint == 0 { String::new() } else { format!(" paint {}", m.paint) };
    if extra.is_empty() {
        format!("{}: {what} {where_}{yaw}{paint}", m.id)
    } else {
        format!("{}: {what} {where_}{yaw}{paint} — {extra}", m.id)
    }
}

/// **What the composition offers an actor**, and who may take part.
///
/// The half a geometry-only view hides. A `Location` travels with the composition and its `props` are
/// repointed at stamp time, so two stamps of one group are two independent affordances — and that is
/// exactly the distinction `map::Location`'s own note is about: *a table plus four chairs is one
/// affordance with four seats, not four affordances*. Without this block an author reading a composition
/// sees three meshes and no reason they belong together.
fn affordances(rows: &mut Vec<(String, Color)>, c: &Composition) {
    if c.locations.is_empty() {
        return;
    }
    rows.push((String::new(), TEXT));
    rows.push(("OFFERS".to_owned(), LABEL));
    for l in &c.locations {
        rows.push((format!("    {} over {}", l.id, l.props.join(", ")), TEXT));
        for i in &l.interactions {
            let roles: Vec<String> = i
                .roles
                .iter()
                .map(|r| {
                    let seat = match &r.socket_role {
                        Some(s) => format!(" at `{s}`"),
                        None => String::new(),
                    };
                    let needs = if r.requires.is_empty() {
                        "anybody".to_owned()
                    } else {
                        r.requires.join("+")
                    };
                    format!("{} {:?} {}-{}{seat} needs {needs}", r.name, r.kind, r.min, r.max)
                })
                .collect();
            rows.push((format!("        {}: {}", i.verb, roles.join("; ")), DIM));
        }
    }
}

/// The three blocks that exist so the tool can explain itself: stale, interface, faults.
fn detail(rows: &mut Vec<(String, Color)>, c: &Composition, comps: &[Composition], project: &Project) {
    rows.push((String::new(), TEXT));

    // **STALE, with the numbers.** Naming the member and both fingerprints is the difference between
    // a badge that sends someone to the right file and one that only says something is wrong.
    match composition::stale_members(c, comps, &project.library) {
        Ok(report) if report.is_empty() => {
            rows.push(("UP TO DATE — every member matches what it was built against".to_owned(), DIM));
        }
        Ok(report) => {
            // **Two different facts, said differently.** A member nobody has measured has nothing to
            // have drifted from; calling that STALE is a sentence about a change that never happened,
            // and it is what every hand-written group said until `of_fingerprint` became an `Option`.
            let drifted: Vec<_> = report
                .iter()
                .filter(|s| s.freshness == composition::Freshness::Stale)
                .collect();
            let unrecorded: Vec<_> = report
                .iter()
                .filter(|s| s.freshness == composition::Freshness::Unrecorded)
                .collect();
            if !drifted.is_empty() {
                rows.push((
                    format!("STALE — {} member(s) changed underneath this composition", drifted.len()),
                    DANGER,
                ));
                for s in drifted {
                    rows.push((
                        format!(
                            "    `{}` was {:#018x}, is now {:#018x}",
                            s.member,
                            s.recorded.unwrap_or_default(),
                            s.measured
                        ),
                        DANGER,
                    ));
                }
            }
            if !unrecorded.is_empty() {
                rows.push((
                    format!(
                        "UNRECORDED — {} member(s) have never been measured, so nothing can be said \
                         about drift yet",
                        unrecorded.len()
                    ),
                    DIM,
                ));
            }
        }
        Err(e) => rows.push((format!("cannot check staleness: {e}"), DANGER)),
    }

    rows.push((String::new(), TEXT));
    match composition::interface(c, comps, &project.library, project.policy.face_bands) {
        Ok(None) => rows.push((
            "ANCHORED — claims no tile, so it has no boundary for anything to abut".to_owned(),
            DIM,
        )),
        Ok(Some(iface)) => {
            rows.push(("DERIVED INTERFACE — read off the members, never authored".to_owned(), LABEL));
            for (dir, name) in [
                (emerge_core::wfc::N, "north"),
                (emerge_core::wfc::E, "east"),
                (emerge_core::wfc::S, "south"),
                (emerge_core::wfc::W, "west"),
            ] {
                for (i, line) in face_rows(&iface.faces[dir]).into_iter().enumerate() {
                    // The face is named once and its bands hang under it, so a doorway reads as one
                    // side that speaks three ways rather than three sides.
                    let label =
                        if i == 0 { format!("{name:>5}: ") } else { " ".repeat(7) };
                    rows.push((format!("    {label}{line}"), TEXT));
                }
            }
            if iface.is_clean() {
                rows.push((
                    "    clean — this composition can constrain a neighbour".to_owned(),
                    DIM,
                ));
            } else {
                rows.push((
                    format!("{} FAULT(S) — members disagree about a face", iface.faults.len()),
                    DANGER,
                ));
                for f in &iface.faults {
                    rows.push((format!("    {}", f.message), DANGER));
                }
            }
        }
        Err(e) => rows.push((format!("cannot derive an interface: {e}"), DANGER)),
    }
}

/// **A face as its bands** — one row each, or a single word when the whole side says one thing.
///
/// This summarised a cell vector until 2026-08-09, and had to: a face was a `Vec` whose length was
/// the project's division setting, so the honest line quoted counts — `wall (8 of 16 cells; the rest
/// unlabelled)`. Those counts were noise. The same wall divided five ways and fifty presents the same
/// thing and read two different ways, which is exactly the leak the band form closes. There is
/// nothing left to summarise here, so this no longer does.
///
/// Only the axis that actually varies is quoted. A plain wall is one word; a doorway varies across
/// and not up, so its rows carry a span and no height; a composition mixing a low piece with a tall one
/// varies up and carries the height instead.
fn face_rows(bands: &[Band]) -> Vec<String> {
    let Some(first) = bands.first() else {
        // Only reachable for a zero-extent envelope, which `interface` derives nothing from.
        return vec!["no face".to_owned()];
    };
    if bands.len() == 1 {
        return match &first.token {
            Some(t) => vec![t.clone()],
            // Not a wildcard — `None` matches only `None`, which is `adjacency`'s rule and the reason
            // the whole feature stays inert until somebody authors a token.
            None => vec!["nothing (matches only unlabelled)".to_owned()],
        };
    }
    let varies_across = bands.iter().any(|b| b.lat != first.lat);
    let varies_up = bands.iter().any(|b| b.y != first.y);
    bands
        .iter()
        .map(|b| {
            let mut s = format!("{:<10}", b.token.as_deref().unwrap_or("nothing"));
            if varies_across {
                s.push_str(&format!(" across {:>6.2} to {:>6.2} m", b.lat.0, b.lat.1));
            }
            if varies_up {
                s.push_str(&format!(" at {:>5.2} to {:>5.2} m up", b.y.0, b.y.1));
            }
            s
        })
        .collect()
}

#[cfg(test)]
mod face_row_tests {
    use super::face_rows;
    use emerge_core::composition::Band;

    fn band(y: (f32, f32), lat: (f32, f32), token: Option<&str>) -> Band {
        Band { y, lat, token: token.map(str::to_owned) }
    }

    /// A side that says one thing is one word — no counts, because there is nothing left to count.
    #[test]
    fn a_face_that_says_one_thing_reads_as_one_word() {
        let rows = face_rows(&[band((0.0, 2.4), (-0.5, 0.5), Some("wall"))]);
        assert_eq!(rows, vec!["wall".to_owned()]);
    }

    /// `None` is a token, not a wildcard, and the line has to say so — otherwise "nothing" reads as
    /// "anything" and an author expects a match that `adjacency` will not make.
    #[test]
    fn an_unlabelled_face_says_that_it_matches_only_unlabelled() {
        let rows = face_rows(&[band((0.0, 2.4), (-0.5, 0.5), None)]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("matches only unlabelled"), "{}", rows[0]);
    }

    /// A doorway varies across and not up, so its rows carry a span and no height.
    #[test]
    fn a_doorway_reads_across_and_does_not_quote_a_height() {
        let rows = face_rows(&[
            band((0.0, 2.0), (-1.5, -0.5), Some("wall")),
            band((0.0, 2.0), (-0.5, 0.5), None),
            band((0.0, 2.0), (0.5, 1.5), Some("wall")),
        ]);
        assert_eq!(rows.len(), 3);
        assert!(rows[0].contains("across") && rows[0].contains("-1.50"), "{}", rows[0]);
        assert!(rows[1].trim_start().starts_with("nothing"), "{}", rows[1]);
        assert!(rows.iter().all(|r| !r.contains("up")), "height quoted where it does not vary: {rows:?}");
    }

    /// A group mixing a low piece with a tall one varies up and not across, so it quotes the height
    /// instead. Both axes are quoted only when both actually move.
    #[test]
    fn a_parapet_reads_up_and_does_not_quote_a_span() {
        let rows = face_rows(&[
            band((0.0, 1.0), (-0.5, 0.5), Some("wall")),
            band((1.0, 2.0), (-0.5, 0.5), None),
        ]);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].contains("1.00 m up"), "{}", rows[0]);
        assert!(rows.iter().all(|r| !r.contains("across")), "span quoted where it does not vary: {rows:?}");
    }
}

#[cfg(test)]
mod seating_tests {
    use super::{member_footprint, seated, turned, turned_footprint, Nudge};
    use emerge_core::composition::{Body, Composition, Envelope, Member};

    fn bay() -> Envelope {
        Envelope::Bounded { size: (3.0, 2.0, 3.0) }
    }

    fn member(id: &str, at: (f32, f32), yaw: f32) -> Member {
        Member {
            id: id.to_owned(),
            body: Body::Descriptor { id: "wall".to_owned(), tip: (0, 0), on: None, patch: None },
            at,
            paint: 0,
            yaw,
            lift: 0.0,
            of_fingerprint: Some(7),
            note: None,
        }
    }

    /// **The lattice is the editor's own**, not a new one: half a metre across, `SNAP / divisions` up.
    #[test]
    fn a_step_is_the_grid_the_map_already_snaps_to() {
        let (at, lift) =
            seated(bay(), (0.0, 0.0), 0.0, (1.0, 1.0), Nudge::Right, 0.25).expect("seats");
        assert_eq!(at, (0.5, 0.0));
        assert_eq!(lift, 0.0);

        let (at, lift) =
            seated(bay(), (0.0, 0.0), 0.0, (1.0, 1.0), Nudge::Up, 0.25).expect("seats");
        assert_eq!(at, (0.0, 0.0), "a lift moves nothing on the floor");
        assert_eq!(lift, 0.25);
    }

    /// North is −Z, matching `wfc`'s `N` and the rest of this project.
    #[test]
    fn forward_is_negative_z() {
        let (at, _) = seated(bay(), (0.0, 0.0), 0.0, (1.0, 1.0), Nudge::Forward, 0.25).expect("seats");
        assert_eq!(at, (0.0, -0.5));
    }

    /// **It refuses rather than clamping**, and the bound is the footprint, not the centre.
    ///
    /// A 1 m piece in a 3 m bay can reach ±1.0 and no further: at 1.5 its centre is still inside the
    /// envelope while half of it hangs out.
    #[test]
    fn a_step_out_of_the_envelope_is_refused_and_says_how_far() {
        let ok = seated(bay(), (0.5, 0.0), 0.0, (1.0, 1.0), Nudge::Right, 0.25).expect("seats");
        assert_eq!(ok.0, (1.0, 0.0), "flush against the wall is still inside");

        let err = seated(bay(), (1.0, 0.0), 0.0, (1.0, 1.0), Nudge::Right, 0.25)
            .expect_err("refuses");
        assert!(err.contains("outside"), "{err}");
        assert!(err.contains("0.50"), "it should say how far out: {err}");
    }

    /// An anchored group claims no tile, so there is nothing to seat inside — named, not guessed.
    #[test]
    fn an_anchored_group_has_no_lattice_and_says_so() {
        let err = seated(Envelope::Anchored, (0.0, 0.0), 0.0, (1.0, 1.0), Nudge::Left, 0.25)
            .expect_err("refuses");
        assert!(err.contains("anchored"), "{err}");
        assert!(err.contains("claims no tile"), "{err}");
    }

    /// **Lift is deliberately unbounded** — it is a nudge on a mount, not an absolute height, and a
    /// ceiling-mounted member's useful lifts are negative.
    #[test]
    fn lift_is_not_bounded_by_the_envelope() {
        let (_, lift) = seated(Envelope::Anchored, (0.0, 0.0), 0.0, (1.0, 1.0), Nudge::Down, 0.25)
            .expect("an anchored group can still be lifted");
        assert_eq!(lift, -0.25);
    }

    /// **A turn lands on the step from any start**, so a member that arrived off-square comes back
    /// onto the lattice with one press — `adjacency::quarter_turns` refuses anything else.
    #[test]
    fn a_quarter_turn_lands_on_a_multiple_of_ninety() {
        assert_eq!(turned(0.0, 90.0, 1.0), 90.0);
        assert_eq!(turned(47.0, 90.0, 1.0), 180.0, "47 rounds to 90, then steps");
        assert_eq!(turned(270.0, 90.0, 1.0), 0.0, "and it wraps");
        assert_eq!(turned(0.0, 90.0, -1.0), 270.0);
        // The fine step is still a step, for a chair drawn up to a table.
        assert_eq!(turned(0.0, 15.0, 1.0), 15.0);
    }

    /// The bounding box of a turned rectangle, exact at every angle and not only the quarters.
    #[test]
    fn a_turned_footprint_is_the_box_that_contains_it() {
        let (w, d) = turned_footprint((2.0, 1.0), 0.0);
        assert!((w - 2.0).abs() < 1e-5 && (d - 1.0).abs() < 1e-5);
        let (w, d) = turned_footprint((2.0, 1.0), 90.0);
        assert!((w - 1.0).abs() < 1e-5 && (d - 2.0).abs() < 1e-5, "{w} x {d}");
        // At 45 it needs more room on both axes than it does square-on, which is the whole point.
        let (w, d) = turned_footprint((2.0, 1.0), 45.0);
        assert!(w > 2.0 && d > 1.4, "{w} x {d}");
    }

    /// A nested group's footprint is its own envelope; an anchored child is refused by name.
    #[test]
    fn a_nested_group_is_measured_by_its_envelope_and_an_anchored_one_refuses() {
        let lib = emerge_core::library::Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: None,
            descriptors: Vec::new(),
        };
        let bounded = Composition {
            id: "inner".to_owned(),
            envelope: Envelope::Bounded { size: (2.0, 1.0, 4.0) },
            members: Vec::new(),
            locations: Vec::new(),
            note: None,
        };
        let loose = Composition { id: "loose".to_owned(), envelope: Envelope::Anchored, ..bounded.clone() };
        let comps = vec![bounded, loose];

        let nested = Member {
            body: Body::Composition { id: "inner".to_owned() },
            ..member("part", (0.0, 0.0), 0.0)
        };
        assert_eq!(member_footprint(&nested, &comps, &lib).expect("measures"), (2.0, 4.0));

        let bad = Member {
            body: Body::Composition { id: "loose".to_owned() },
            ..member("part", (0.0, 0.0), 0.0)
        };
        let err = member_footprint(&bad, &comps, &lib).expect_err("refuses");
        assert!(err.contains("anchored"), "{err}");
        assert!(err.contains("claims no tile"), "{err}");
    }
}

#[cfg(test)]
mod flush_tests {
    use super::{flushed, Nudge};
    use emerge_core::composition::Envelope;

    /// **The case step 4 was written to find.** `site/wall` is 0.1 m thick; flush inside a 1 m tile
    /// puts it at 0.45, which is not a multiple of `grid::SNAP` and so is unreachable by seating.
    #[test]
    fn a_thin_wall_lands_where_the_lattice_cannot_put_it() {
        let tile = Envelope::Bounded { size: (1.0, 2.4, 1.0) };
        let wall = (0.1, 1.0);
        let (x, z) = flushed(tile, (0.0, 0.0), wall, Nudge::Left).expect("flushes");
        assert_eq!((x, z), (-0.45, 0.0));
        // Its west face is exactly the envelope's, which is what `interface` reads a member on.
        assert!((x - wall.0 * 0.5 + 0.5).abs() < 1e-6, "west face at {}", x - wall.0 * 0.5);
        // And it is not on the seating lattice, which is the whole point.
        assert!(
            (x / emerge_core::grid::SNAP).fract().abs() > 1e-6,
            "if this were on the lattice the flush verb would be unnecessary"
        );
    }

    /// **A wall has to be turned before it is flushed**, and the footprint carries that.
    ///
    /// `site/wall` is 0.1 wide and 1.0 deep, so it runs along Z: flushing it *north* while it still
    /// points that way just centres it, because it already fills the tile in Z. Turned a quarter its
    /// footprint is (1.0, 0.1) and the same verb puts it on the north face. `member_footprint`
    /// applies the member's yaw for exactly this reason — the first draft of this test did not, and
    /// asserted a wall could be flush across the axis it spans.
    #[test]
    fn a_wall_must_be_turned_before_flushing_and_the_footprint_says_so() {
        let tile = Envelope::Bounded { size: (1.0, 2.4, 1.0) };
        let along_z = (0.1, 1.0);
        assert_eq!(
            flushed(tile, (0.0, 0.0), along_z, Nudge::Forward).expect("flushes"),
            (0.0, 0.0),
            "it already spans Z, so north is where it is"
        );
        let turned = (1.0, 0.1);
        assert_eq!(
            flushed(tile, (0.0, 0.0), turned, Nudge::Forward).expect("flushes"),
            (0.0, -0.45)
        );
    }

    /// Flushing one axis leaves the other alone, so north-then-west is a corner.
    #[test]
    fn two_flushes_make_a_corner_rather_than_competing() {
        let tile = Envelope::Bounded { size: (1.0, 2.4, 1.0) };
        let north = flushed(tile, (0.0, 0.0), (1.0, 0.1), Nudge::Forward).expect("flushes");
        assert_eq!(north, (0.0, -0.45));
        // The west wall of the same tile, still pointing along Z.
        let corner = flushed(tile, north, (0.1, 1.0), Nudge::Left).expect("flushes");
        assert_eq!(corner, (-0.45, -0.45), "the first flush survives the second");
    }

    /// A piece as wide as its tile is already flush on both sides — the answer is the centre.
    #[test]
    fn a_full_width_member_is_flush_where_it_stands() {
        let tile = Envelope::Bounded { size: (1.0, 2.4, 1.0) };
        assert_eq!(flushed(tile, (0.0, 0.0), (1.0, 1.0), Nudge::Left).expect("flushes"), (0.0, 0.0));
        assert_eq!(flushed(tile, (0.0, 0.0), (1.0, 1.0), Nudge::Right).expect("flushes"), (0.0, 0.0));
    }

    /// There is no face to flush against vertically, and it says why rather than guessing a datum.
    #[test]
    fn there_is_no_vertical_face_and_it_says_so() {
        let tile = Envelope::Bounded { size: (1.0, 2.4, 1.0) };
        let err = flushed(tile, (0.0, 0.0), (0.1, 1.0), Nudge::Up).expect_err("refuses");
        assert!(err.contains("no face"), "{err}");
        assert!(err.contains("nudge on the mount"), "{err}");
    }

    /// An anchored group claims no tile, so it has no face — named, not invented.
    #[test]
    fn an_anchored_group_has_no_face() {
        let err = flushed(Envelope::Anchored, (0.0, 0.0), (0.1, 1.0), Nudge::Left)
            .expect_err("refuses");
        assert!(err.contains("claims no tile"), "{err}");
    }
}

#[cfg(test)]
mod paint_tests {
    use emerge_core::composition::{Body, Composition, Compositions, Envelope, Member, Stamped};

    fn member(id: &str, paint: i8) -> Member {
        Member {
            id: id.to_owned(),
            body: Body::Descriptor { id: "wall".to_owned(), tip: (0, 0), on: None, patch: None },
            at: (0.0, 0.0),
            yaw: 0.0,
            lift: 0.0,
            paint,
            of_fingerprint: None,
            note: None,
        }
    }

    /// **Zero is not written.** `#[serde(default)]` alone would put `paint: 0` on every member of
    /// every group on the next save, which is a whole-file churn across content diffs that have to
    /// stay readable.
    #[test]
    fn an_unpainted_member_writes_nothing() {
        let set = Compositions {
            version: emerge_core::composition::COMPOSITIONS_VERSION,
            note: None,
            compositions: vec![Composition {
                id: "bay".to_owned(),
                envelope: Envelope::Bounded { size: (1.0, 2.4, 1.0) },
                members: vec![member("a", 0), member("b", 3)],
                locations: Vec::new(),
                note: None,
            }],
        };
        let text = set.to_ron().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(text.matches("paint:").count(), 1, "only the painted member writes it:\n{text}");
        assert!(text.contains("paint: 3"), "{text}");
        let back = Compositions::parse(&text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back.compositions[0].members[0].paint, 0);
        assert_eq!(back.compositions[0].members[1].paint, 3);
    }

    /// Paint reaches the rows the game spawns, or the field decides nothing.
    #[test]
    fn paint_survives_expand_onto_the_placed_row() {
        let lib = emerge_core::library::Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: None,
            descriptors: vec![super::tests_support::wall()],
        };
        let comp = Composition {
            id: "bay".to_owned(),
            envelope: Envelope::Bounded { size: (1.0, 2.4, 1.0) },
            members: vec![member("decal", 2)],
            locations: Vec::new(),
            note: None,
        };
        let map = emerge_core::map::Map {
            version: emerge_core::map::MAP_VERSION,
            name: "m".to_owned(),
            origin: (0.0, 0.0, 0.0),
            bounds: (8.0, 2.4, 8.0),
            placements: Vec::new(),
            stamps: Vec::new(),
            locations: Vec::new(),
            note: None,
        };
        let stamps = vec![Stamped { id: "s".to_owned(), of: "bay".to_owned(), ..Default::default() }];
        let out = emerge_core::composition::expand(&map, &stamps, &[comp], &lib)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.placements.len(), 1);
        assert_eq!(out.placements[0].paint, 2, "paint has to reach the row the game spawns");
    }
}

#[cfg(test)]
mod carousel_tests {
    use super::{footprint, framing_height, height_of, lay_out, slot_at, MINIATURE, WINGS};
    use emerge_core::composition::{Body, Composition, Envelope, Member};
    use emerge_core::grid::SNAP;

    fn lib() -> emerge_core::library::Library {
        emerge_core::library::Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: None,
            descriptors: vec![super::tests_support::wall()],
        }
    }

    /// A 1 × 1 × 1 member of the fixture library, standing at `at`.
    fn member(id: &str, of: &str, at: (f32, f32)) -> Member {
        Member {
            id: id.to_owned(),
            body: Body::Descriptor { id: of.to_owned(), tip: (0, 0), on: None, patch: None },
            at,
            yaw: 0.0,
            lift: 0.0,
            paint: 0,
            of_fingerprint: None,
            note: None,
        }
    }

    fn tile(id: &str, w: f32, d: f32) -> Composition {
        Composition {
            id: id.to_owned(),
            envelope: Envelope::Bounded { size: (w, 2.4, d) },
            members: Vec::new(),
            locations: Vec::new(),
            note: None,
        }
    }

    fn anchored(id: &str, members: Vec<Member>) -> Composition {
        Composition {
            id: id.to_owned(),
            envelope: Envelope::Anchored,
            members,
            locations: Vec::new(),
            note: None,
        }
    }

    fn kit(n: usize) -> Vec<Composition> {
        (0..n).map(|i| tile(&format!("t{i}"), 1.0, 1.0)).collect()
    }

    /// **The property the carousel rests on**: the group being edited never moves. Stepping slides
    /// the strip past a fixed centre, so a seat verb's effect is not confused with the stage shifting.
    #[test]
    fn the_focal_group_stands_at_the_centre_wherever_it_is_in_the_list() {
        let comps = kit(7);
        for selected in 0..comps.len() {
            let c = lay_out(&comps, &lib(), selected).unwrap_or_else(|e| panic!("{e}"));
            let focal = c.focal().unwrap_or_else(|| panic!("no focal slot at {selected}"));
            assert_eq!(focal.index, selected);
            assert_eq!(focal.at, (0.0, 0.0), "the focal group moved at {selected}");
            assert_eq!(focal.scale, 1.0, "the focal group is never a miniature");
        }
    }

    /// The scale ramp is geometric, so adding a wing needs no new number.
    #[test]
    fn each_remove_from_the_focal_group_is_one_step_smaller() {
        let comps = kit(9);
        let c = lay_out(&comps, &lib(), 4).unwrap_or_else(|e| panic!("{e}"));
        for s in &c.slots {
            let want = MINIATURE.powi(s.offset.abs());
            assert!((s.scale - want).abs() < 1e-6, "offset {} scaled {}", s.offset, s.scale);
        }
        assert!(c.slots.iter().all(|s| s.offset.abs() <= WINGS), "a slot beyond the wings");
    }

    /// **The wings do not wrap**, so running out of miniatures is how the stage says which end of the
    /// list you are at. A wrapping strip would show the same group twice and say nothing.
    #[test]
    fn the_ends_of_the_list_are_visible_as_missing_miniatures() {
        let comps = kit(6);
        let first = lay_out(&comps, &lib(), 0).unwrap_or_else(|e| panic!("{e}"));
        assert!(first.slots.iter().all(|s| s.offset >= 0), "nothing stands before the first group");
        assert_eq!(first.slots.len(), 1 + WINGS as usize);

        let last = lay_out(&comps, &lib(), comps.len() - 1).unwrap_or_else(|e| panic!("{e}"));
        assert!(last.slots.iter().all(|s| s.offset <= 0), "nothing stands after the last group");

        // A kit smaller than the strip is simply a shorter strip, not a repeated one.
        let small = kit(2);
        let c = lay_out(&small, &lib(), 0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.slots.len(), 2);
        let mut seen: Vec<usize> = c.slots.iter().map(|s| s.index).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), c.slots.len(), "a group appeared twice on the strip");
    }

    /// Slots are laid end to end with air between them, never overlapping — which is what makes a
    /// click unambiguous and the strip readable.
    #[test]
    fn neighbours_stand_clear_of_one_another_along_the_strip() {
        let comps = kit(9);
        let c = lay_out(&comps, &lib(), 4).unwrap_or_else(|e| panic!("{e}"));
        for pair in c.slots.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(b.offset == a.offset + 1, "slots must come out in strip order");
            let apart = ((b.at.0 - a.at.0).powi(2) + (b.at.1 - a.at.1).powi(2)).sqrt();
            let touching = (a.size.0 + a.size.1) * 0.5 * std::f32::consts::FRAC_1_SQRT_2
                + (b.size.0 + b.size.1) * 0.5 * std::f32::consts::FRAC_1_SQRT_2;
            assert!(apart > touching, "offsets {} and {} overlap", a.offset, b.offset);
        }
    }

    /// `slot_at` inverts `lay_out`, which is what makes a click a lookup rather than a raycast — and
    /// it answers with the *composition* index, because a click's whole job is choosing the next
    /// focal group.
    #[test]
    fn clicking_a_miniature_names_the_group_drawn_there() {
        let comps = kit(9);
        let c = lay_out(&comps, &lib(), 4).unwrap_or_else(|e| panic!("{e}"));
        for s in &c.slots {
            assert_eq!(slot_at(&c, s.at), Some(s.index), "the centre of offset {}", s.offset);
        }
        // The air between two slots belongs to neither.
        let a = c.slots.iter().find(|s| s.offset == 0).unwrap_or_else(|| panic!("no focal"));
        let b = c.slots.iter().find(|s| s.offset == 1).unwrap_or_else(|| panic!("no +1"));
        let between = ((a.at.0 + b.at.0) * 0.5, (a.at.1 + b.at.1) * 0.5);
        assert_eq!(slot_at(&c, between), None, "a click between slots picks neither");
        assert_eq!(slot_at(&c, (1000.0, 1000.0)), None, "and off the strip picks nothing");
    }

    /// A bounded group declares its box; an anchored one is measured from what stands in it.
    #[test]
    fn an_anchored_group_is_measured_and_a_bounded_one_is_taken_at_its_word() {
        let comps = vec![
            tile("bounded", 3.0, 2.0),
            anchored("table", vec![member("l", "wall", (-1.0, 0.0)), member("r", "wall", (1.0, 0.0))]),
        ];
        let l = lib();
        assert_eq!(footprint(&comps[0], &comps, &l).unwrap_or_else(|e| panic!("{e}")), (3.0, 2.0));
        assert_eq!(height_of(&comps[0], &comps, &l).unwrap_or_else(|e| panic!("{e}")), 2.4);
        // Two 1 × 1 × 1 pieces two metres apart span 3 m in x, 1 m in z, and stand 1 m tall.
        assert_eq!(footprint(&comps[1], &comps, &l).unwrap_or_else(|e| panic!("{e}")), (3.0, 1.0));
        assert_eq!(height_of(&comps[1], &comps, &l).unwrap_or_else(|e| panic!("{e}")), 1.0);
    }

    /// A group that cannot be measured **refuses and names itself**, rather than being given a box
    /// nobody authored — the rule `member_footprint` and `composition_from_set` already apply.
    #[test]
    fn a_group_that_cannot_be_measured_refuses_and_names_itself() {
        let comps = vec![anchored("mystery", vec![member("m", "absent", (0.0, 0.0))])];
        let err = footprint(&comps[0], &comps, &lib()).expect_err("refuses");
        assert!(err.contains("mystery"), "{err}");
        let err = height_of(&comps[0], &comps, &lib()).expect_err("refuses");
        assert!(err.contains("mystery"), "{err}");
        // And `lay_out` propagates rather than substituting a slot.
        let err = lay_out(&comps, &lib(), 0).expect_err("refuses");
        assert!(err.contains("mystery"), "{err}");
    }

    /// **The ordinary state right after `N`.** A group with nothing in it yet must still be clickable
    /// and visible, so it floors at the editor's own quantum rather than collapsing to a point.
    #[test]
    fn a_group_with_nothing_in_it_still_gets_a_slot_the_lattice_can_express() {
        let comps = vec![anchored("fresh", Vec::new())];
        let l = lib();
        assert_eq!(footprint(&comps[0], &comps, &l).unwrap_or_else(|e| panic!("{e}")), (SNAP, SNAP));
        assert_eq!(height_of(&comps[0], &comps, &l).unwrap_or_else(|e| panic!("{e}")), SNAP);
        let c = lay_out(&comps, &lib(), 0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.slots.len(), 1);
        assert_eq!(slot_at(&c, (0.0, 0.0)), Some(0), "a fresh group has to be clickable");
    }

    /// No groups is an empty stage, not a panic. A selection past the end clamps rather than dropping
    /// the strip — the panel and the stage must never disagree about what is focal.
    #[test]
    fn an_empty_set_lays_out_to_nothing_and_a_stale_selection_clamps() {
        let empty = lay_out(&[], &lib(), 3).unwrap_or_else(|e| panic!("{e}"));
        assert!(empty.slots.is_empty());
        assert_eq!(empty.extent, (0.0, 0.0));

        let comps = kit(3);
        let c = lay_out(&comps, &lib(), 99).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.focal().map(|s| s.index), Some(2));
    }

    /// **The bug a captured frame found.** Framing on the floor plan alone cut the tops off four
    /// 2.4 m tiles, because a group that stands up occupies screen the footprint says nothing about.
    #[test]
    fn the_framing_accounts_for_how_tall_the_groups_stand() {
        let flat = framing_height((4.0, 4.0), 0.1);
        let tall = framing_height((4.0, 4.0), 6.0);
        assert!(tall > flat, "height has to widen the view: {tall} vs {flat}");
        // It never frames tighter than one tile's worth of view …
        assert_eq!(framing_height((0.0, 0.0), 0.0), crate::tiles::TILE_VIEW_HEIGHT);
        // … and a big enough strip outruns the rig, which is the condition `restage_group` reports.
        assert!(
            framing_height((80.0, 50.0), 2.4) > crate::view::MAX_ZOOM,
            "a strip this size is exactly what the crop report is for"
        );
    }

    /// The extent is symmetric about the focal group, because the camera is pinned there — a strip
    /// with one short wing must still be framed without shifting the thing being edited.
    #[test]
    fn the_extent_is_measured_from_the_focal_group_not_from_the_strips_own_middle() {
        let comps = kit(6);
        let c = lay_out(&comps, &lib(), 0).unwrap_or_else(|e| panic!("{e}"));
        let (hw, hd) = (c.extent.0 * 0.5, c.extent.1 * 0.5);
        for s in &c.slots {
            assert!(s.at.0.abs() + s.size.0 * 0.5 <= hw + 1e-4, "offset {} off the x edge", s.offset);
            assert!(s.at.1.abs() + s.size.1 * 0.5 <= hd + 1e-4, "offset {} off the z edge", s.offset);
        }
        assert!(c.tallest >= 2.4 - 1e-4, "the focal tile stands 2.4 m and the strip has to say so");
    }
}

#[cfg(test)]
mod tests_support {
    use emerge_core::descriptor::{Align, Descriptor, Extent};

    /// A minimal measured piece, so the paint tests need no fixture on disk.
    pub fn wall() -> Descriptor {
        Descriptor {
            id: "wall".to_owned(),
            extent: Extent { footprint: Some((1.0, 1.0)), height: Some(1.0) },
            align: Align { y_offset: Some(0.0), ..Default::default() },
            ..Default::default()
        }
    }
}
