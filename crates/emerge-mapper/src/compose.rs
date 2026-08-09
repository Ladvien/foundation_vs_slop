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

use emerge_core::composition::{self, Composition, Envelope};

use crate::chrome::{ACCENT, DANGER, DIM, LABEL, TEXT};
use crate::keys::{self, Action};
use crate::project::Project;
use crate::tiles::{ComposeRoot, Mode};

/// Which group the tab is looking at, and which one the map will stamp.
#[derive(Resource, Default)]
pub struct ComposeState {
    /// Index into `project.compositions.compositions`. Clamped on rebuild rather than stored as an
    /// `Option`, because the list is never empty while a selection exists.
    pub selected: usize,
    /// **The armed group** — by id, never by index.
    ///
    /// An index would silently re-point the moment `compositions.ron` gained a row above it, which is
    /// the same argument [`emerge_core::composition::Override`] makes about naming a member. The id
    /// costs one lookup at stamp time and cannot go stale.
    pub armed: Option<String>,
    pub status: String,
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
                (walk, arm, record)
                    .in_set(keys::Phase::Act)
                    .run_if(in_compose_mode),
            )
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
        crate::chrome::shortcut_hint(p);
        crate::chrome::key_census(p, &[keys::Context::Global, keys::Context::Compose]);
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
        ));
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
        state.status = "no group to arm".to_owned();
        return;
    };
    if state.armed.as_deref() == Some(c.id.as_str()) {
        state.armed = None;
        state.status = format!("`{}` disarmed", c.id);
    } else {
        state.armed = Some(c.id.clone());
        state.status = format!("`{}` armed — the map tab stamps it", c.id);
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
        state.status = "no group to record".to_owned();
        return;
    };
    let id = target.id.clone();
    let changed = match composition::record_fingerprints(target, &snapshot, &library) {
        Ok(n) => n,
        Err(e) => {
            state.status = format!("cannot record `{id}`: {e}");
            return;
        }
    };
    if changed == 0 {
        state.status = format!("`{id}` was already up to date — nothing written");
        return;
    }
    let path = project
        .emerge_dir
        .join(emerge_core::composition::Compositions::FILE);
    let text = match project.compositions.to_ron() {
        Ok(t) => t,
        Err(e) => {
            state.status = format!("NOT WRITTEN: {e}");
            return;
        }
    };
    match emerge_core::ron_surgery::save_atomic(&path, &text) {
        Ok(()) => state.status = format!("recorded {changed} member(s) of `{id}`"),
        // Replaces the message rather than appending — a refusal must not read like a receipt.
        Err(e) => state.status = format!("NOT WRITTEN: {e}"),
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
        for m in &c.members {
            rows.push((format!("    {}", describe_member(m)), TEXT));
        }
        affordances(&mut rows, c);
        detail(&mut rows, c, comps, &project);
    }

    if !state.status.is_empty() {
        rows.push((String::new(), TEXT));
        rows.push((state.status.clone(), ACCENT));
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
                rows.push((format!("    {name:>5}: {}", summarise_face(&iface.faces[dir])), TEXT));
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

/// A face as one line: the distinct tokens on it, or that it presents nothing.
///
/// Summarised rather than listed cell by cell, because a 2.4 m wall at the shipped divisions is ten
/// cells and every one of them says the same word. What an author needs from this line is whether the
/// face speaks, and with which words.
fn summarise_face(face: &[Option<String>]) -> String {
    if face.is_empty() {
        return "no cells".to_owned();
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut silent = 0usize;
    for cell in face {
        match cell.as_deref() {
            Some(t) if !seen.contains(&t) => seen.push(t),
            Some(_) => {}
            None => silent += 1,
        }
    }
    if seen.is_empty() {
        // Not a wildcard — `None` matches only `None`, which is `adjacency`'s rule and the reason the
        // whole feature stays inert until somebody authors a token.
        return format!("nothing ({silent} cells unlabelled — matches only unlabelled)");
    }
    let tokens = seen.join(", ");
    if silent == 0 {
        format!("{tokens} ({} cells)", face.len())
    } else {
        format!("{tokens} ({} of {} cells; the rest unlabelled)", face.len() - silent, face.len())
    }
}
