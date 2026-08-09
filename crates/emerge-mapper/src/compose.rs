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

use bevy::prelude::*;

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

/// Which group the tab is looking at, and which one the map will stamp.
#[derive(Resource, Default)]
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
    /// **The armed group** — by id, never by index.
    ///
    /// An index would silently re-point the moment `compositions.ron` gained a row above it, which is
    /// the same argument [`emerge_core::composition::Override`] makes about naming a member. The id
    /// costs one lookup at stamp time and cannot go stale.
    pub armed: Option<String>,
    /// What this tab has to say — see [`crate::chrome::Status`] for why a refusal and a receipt are
    /// two slots rather than one string. This panel used to paint every message [`ACCENT`], so
    /// `cannot record` and `recorded 3 member(s)` were the same colour and the first was gone as soon
    /// as anything else happened.
    pub status: crate::chrome::Status,
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

pub struct ComposePlugin;

impl Plugin for ComposePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComposeState>()
            .add_systems(Startup, spawn_compose_panel)
            .add_systems(
                Update,
                (
                    walk,
                    arm,
                    record,
                    walk_members,
                    seat_member,
                    flush_member,
                    turn_member,
                    drop_member,
                    step_history,
                )
                    .in_set(keys::Phase::Act)
                    .run_if(in_compose_mode),
            )
            // Not gated on the mode, for the same reason `rebuild` is not: the staged group is
            // despawned when the tab is left, and a system that stops running cannot despawn it.
            .add_systems(Update, restage_group.after(keys::Phase::Act))
            .add_systems(Update, draw_stage.run_if(in_compose_mode))
            // Not gated on the mode: the armed group is shown on the Map tab too, and a panel that
            // stops updating when you leave it is a panel that lies the moment you come back.
            .add_systems(Update, rebuild.after(keys::Phase::Act));
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
        crate::chrome::section(p, "GROUPS");
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
}

/// Walk the list. Shift steps five, matching every other list in this editor.
fn walk(mut state: ResMut<ComposeState>, project: Res<Project>, keys: Res<keys::Live>, input: Res<ButtonInput<KeyCode>>) {
    let n = project.compositions.compositions.len();
    if n == 0 {
        return;
    }
    let step = if input.pressed(KeyCode::ShiftLeft) || input.pressed(KeyCode::ShiftRight) { 5 } else { 1 };
    let mut at = state.selected as i64;
    if keys::just_pressed(&input, keys.0, Action::ComposePrev) {
        at -= step;
    } else if keys::just_pressed(&input, keys.0, Action::ComposeNext) {
        at += step;
    } else {
        return;
    }
    state.selected = at.rem_euclid(n as i64) as usize;
}

/// Arm the selected group, or disarm it if it was already armed.
///
/// Toggling rather than a separate disarm verb, for the reason `EditorState::brush` is an `Option`:
/// **nothing armed has to be a reachable state**, or an author cannot put the cursor over the map
/// without something following it.
fn arm(mut state: ResMut<ComposeState>, project: Res<Project>, keys: Res<keys::Live>, input: Res<ButtonInput<KeyCode>>) {
    if !keys::just_pressed(&input, keys.0, Action::ComposeArm) {
        return;
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
        state.status.note("no group to arm");
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
        state.status.note("no group to record");
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

/// The selected member of the selected group, or a note saying there is none.
fn selected_member<'a>(
    state: &ComposeState,
    comps: &'a [Composition],
) -> Option<(&'a Composition, usize)> {
    let c = comps.get(state.selected)?;
    let i = state.member.min(c.members.len().checked_sub(1)?);
    Some((c, i))
}

/// Walk the members of the selected group. Wraps, like every other list in this editor.
fn walk_members(
    mut state: ResMut<ComposeState>,
    project: Res<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    let Some(c) = project.compositions.compositions.get(state.selected) else {
        return;
    };
    let n = c.members.len();
    if n == 0 {
        return;
    }
    let mut at = state.member as i64;
    if keys::just_pressed(&input, keys.0, Action::ComposeMemberPrev) {
        at -= 1;
    } else if keys::just_pressed(&input, keys.0, Action::ComposeMemberNext) {
        at += 1;
    } else {
        return;
    }
    state.member = at.rem_euclid(n as i64) as usize;
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
    let step = emerge_core::grid::SNAP / project.policy.divisions.max(1) as f32;
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

/// Take the selected member out of the group.
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
            .ok_or_else(|| format!("`{member_id}`'s group is no longer there"))?;
        if i >= c.members.len() {
            return Err(format!("`{member_id}` is no longer there to drop"));
        }
        c.members.remove(i);
        // A `Location` names member ids in `props`; leaving one pointing at a member that is gone is
        // exactly the dangling reference `validate` refuses, so the refusal below would fire and the
        // write would be abandoned. Dropping the prop with the member keeps the group loadable, and
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
#[derive(Component)]
struct StagedMember;

/// **Stand the selected group up, through `composition::expand`.**
///
/// The same three calls `editor::redraw_stamps` makes — build a scratch map, expand a stamp of the
/// group against it, `stack::resolve_y` over a map carrying both — so **what this tab shows is what a
/// stamp produces**. A second interpretation of a composition would be a preview that lies, which is
/// the argument `spawn_piece` itself was written for.
///
/// Rebuilt wholesale whenever the project or the selection changes. A diffing version would be faster
/// and would be a second opinion about what is on screen.
fn restage_group(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mode: Res<Mode>,
    mut state: ResMut<ComposeState>,
    project: Res<Project>,
    drawn: Query<Entity, With<StagedMember>>,
) {
    if !(project.is_changed() || state.is_changed() || mode.is_changed()) {
        return;
    }
    for e in &drawn {
        commands.entity(e).despawn();
    }
    if *mode != Mode::Compose {
        return;
    }
    let Some(c) = project.compositions.compositions.get(state.selected) else {
        return;
    };
    let size = match c.envelope {
        Envelope::Bounded { size } => size,
        // An anchored group claims no tile. It still stands up — it is furniture somewhere — and the
        // scratch map just needs to be big enough not to clip it.
        Envelope::Anchored => (64.0, 8.0, 64.0),
    };
    let scratch = emerge_core::map::Map {
        version: emerge_core::map::MAP_VERSION,
        name: "compose_stage".to_owned(),
        origin: (COMPOSE_STAGE.x, COMPOSE_STAGE.y, COMPOSE_STAGE.z),
        bounds: size,
        placements: Vec::new(),
        stamps: Vec::new(),
        locations: Vec::new(),
        note: None,
    };
    let stamp = emerge_core::composition::Stamped {
        id: "staged".to_owned(),
        of: c.id.clone(),
        at: (0.0, 0.0),
        ..Default::default()
    };
    let expanded = match composition::expand(
        &scratch,
        &[stamp],
        &project.compositions.compositions,
        &project.library,
    ) {
        Ok(e) => e,
        // Loud. An empty patch of floor where a group should be, with nothing saying why, is the
        // failure this editor's own notes call the worst it had.
        Err(e) => return state.status.problem(format!("`{}` does not resolve: {e}", c.id)),
    };
    let mut with_rows = scratch.clone();
    with_rows.placements.extend(expanded.placements.iter().cloned());
    let ys = match emerge_core::stack::resolve_y(&with_rows, &project.library) {
        Ok(ys) => ys,
        Err(e) => return state.status.problem(format!("`{}` has no height: {e}", c.id)),
    };
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
        }
    }
}

/// **The envelope and the lattice a member seats on.**
///
/// Drawn rather than spawned: it is not a thing in the world, it is the tile the group claims. An
/// anchored group gets neither, because it claims none — an invented box would be exactly the guess
/// `seated` refuses to make.
fn draw_stage(state: Res<ComposeState>, project: Res<Project>, mut gizmos: Gizmos) {
    let Some(c) = project.compositions.compositions.get(state.selected) else {
        return;
    };
    let Envelope::Bounded { size } = c.envelope else {
        return;
    };
    let base = COMPOSE_STAGE;
    // The envelope, as the box it is.
    gizmos.cube(
        Transform::from_translation(base + Vec3::new(0.0, size.1 * 0.5, 0.0))
            .with_scale(Vec3::new(size.0, size.1, size.2)),
        crate::chrome::ACCENT,
    );
    // The seating lattice on its floor — `grid::SNAP`, the same quantum `seated` steps by and the
    // Map snaps to. Drawing anything else here would be the drawn-grid-disagrees-with-the-snap bug
    // that `GridSpacing`'s note records.
    let step = emerge_core::grid::SNAP;
    let cells = (
        (size.0 / step).round().max(1.0) as u32,
        (size.2 / step).round().max(1.0) as u32,
    );
    gizmos
        .grid(
            Isometry3d::new(base + Vec3::Y * 0.002, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
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
        rows.push((format!("MEMBERS OF `{}`", c.id), LABEL));
        let at = state.member.min(c.members.len().saturating_sub(1));
        for (i, m) in c.members.iter().enumerate() {
            // The seating cursor, in the same `> ` shape the group list above uses — one marker
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
                .ok_or_else(|| format!("`{}` nests `{id}`, which is not a group here", m.id))?;
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
            "this group is anchored, so it claims no tile and has no lattice to seat inside. \
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
/// # It is what makes a group a tile
///
/// [`composition::interface`] reads a member on a face when its box edge is within
/// `adjacency::EDGE_EPSILON` of the envelope's. Flush puts it exactly there, so the group presents
/// what it is made of — and the envelope stays exactly the tile, which is what lets
/// `grammar::learn` accept a group of floor-plus-wall where the 0.1 × 1.0 wall alone is refused as
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
            "this group is anchored, so it claims no tile and has no face to flush against"
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
    if extra.is_empty() {
        format!("{}: {what} {where_}{yaw}", m.id)
    } else {
        format!("{}: {what} {where_}{yaw} — {extra}", m.id)
    }
}

/// **What the group offers an actor**, and who may take part.
///
/// The half a geometry-only view hides. A `Location` travels with the group and its `props` are
/// repointed at stamp time, so two stamps of one group are two independent affordances — and that is
/// exactly the distinction `map::Location`'s own note is about: *a table plus four chairs is one
/// affordance with four seats, not four affordances*. Without this block an author reading a group
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
                    format!("STALE — {} member(s) changed underneath this group", drifted.len()),
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
    match composition::interface(c, comps, &project.library, project.policy.divisions) {
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
                    "    clean — this group can constrain a neighbour".to_owned(),
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
/// and not up, so its rows carry a span and no height; a group mixing a low piece with a tall one
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
