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
        // Directly under the title, above everything a working author reads — a problem that has to
        // be scrolled to is a problem that gets missed.
        crate::chrome::problem_banner(p, Mode::Compose);
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
