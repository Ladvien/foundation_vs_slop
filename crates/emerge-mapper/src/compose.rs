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

use emerge_core::composition::{self, Band, Composition, Envelope};

use crate::chrome::{ACCENT, DANGER, DIM, TEXT};
use crate::keys::{self, Action};
use crate::project::Project;
use crate::tiles::{ComposeRoot, Mode};

/// **Where the Compose tab stands the group it is editing.**
///
/// Far from the map, like [`crate::tiles::STAGE`] and for the same reason: a group drawn over the map
/// is indistinguishable from what is already placed there, and seating a member means watching *that*
/// member move.
pub const COMPOSE_STAGE: Vec3 = crate::stages::COMPOSE;

/// The two lists on this tab, and the order `left`/`right` cycles them.
///
/// Compositions, then the members of the selected one — reading order, so cycling forward walks the
/// screen top to bottom rather than in an order only the code knows. There was a third, the library
/// to place from; it went when authoring moved to the Map.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    #[default]
    Groups,
    Members,
}

impl Pane {
    pub const ALL: [Pane; 2] = [Pane::Groups, Pane::Members];

    pub fn label(self) -> &'static str {
        match self {
            Pane::Groups => "COMPOSITIONS",
            Pane::Members => "MEMBERS",
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
    /// **Which of the three lists the walk keys move.**
    ///
    /// Replaced a modal picker that took the keyboard while open. A mode you cannot see is the
    /// defect this tab was rebuilt for, so this is drawn — see `rebuild`, which tints the focused
    /// list's header — and the lists are all on screen at once rather than one at a time.
    pub focus: Pane,
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
            focus: Pane::default(),
            armed: None,
            staged: None,
            status: crate::chrome::Status::default(),
        }
    }
}

/// One row of the panel, typed — `docs/ui.md` §3.1's rule, applied to the last tab that rendered
/// strings: the builders say WHAT a row is and the renderer owns how each kind looks, so a heading,
/// a selectable row, prose and a railed refusal can never again differ only by wording and indent.
enum Line {
    /// A block heading — `chrome::section`, margins and all.
    Section(String),
    /// Prose; leading spaces become layout in [`spawn_line`].
    Prose(String, Color),
    /// A composition row: filled when selected, starred when armed, clickable.
    Comp { ix: usize, text: String, selected: bool, armed: bool },
    /// A member row: filled when the seating cursor is on it, clickable.
    Member { ix: usize, text: String, at: bool },
    /// A severity-railed block — the first line reads first, in the tint.
    Rail { tint: Color, lines: Vec<(String, Color)> },
}

/// A clickable composition row, carrying its index. The panel's rows are rebuilt wholesale, so the
/// index is re-derived every rebuild and cannot go stale between clicks.
#[derive(Component, Clone, Copy)]
struct CompRow(usize);

/// A clickable member row, carrying its index into the selected composition's members.
#[derive(Component, Clone, Copy)]
struct MemberRow(usize);

/// The despawn marker every rebuilt row carries. Rebuilt wholesale when anything it reads changes.
#[derive(Component)]
struct ComposeLine;

/// The block the list and the detail are written into.
#[derive(Component)]
struct ComposeBody;

pub struct ComposePlugin;

impl Plugin for ComposePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComposeState>()
            .init_resource::<StagedCarousel>()
            .init_resource::<Budget>()
            .add_systems(Update,
                (measure_budget.before(rebuild))
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            .add_systems(
                OnEnter(crate::screen::Screen::Editor),
                spawn_compose_panel.after(crate::chrome::FrameSystems),
            )
            // **Before anything reads `selected`, and not gated on the mode.** The list can shrink
            // while another tab is live — capturing on the Map rewrites the whole set — and a reader
            // that clamped for itself is how three of them came to disagree. See `clamp_selection`.
            .add_systems(Update,
                (clamp_selection.before(keys::Phase::Act))
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            .add_systems(
                Update,
                ((walk, arm, cycle_focus, step_carousel, pick_slot)
                    .in_set(keys::Phase::Act)
                    .run_if(in_compose_mode),)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            // Not gated on the mode: the staged strip is despawned when the tab is left, and a
            // system that stops running cannot despawn it.
            .add_systems(Update,
                (restage_group.after(keys::Phase::Act))
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            // After the strip is published, so nothing is drawn against last frame's layout.
            .add_systems(
                Update,
                (draw_stage.after(restage_group).run_if(in_compose_mode),)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            // Labels are NOT gated on the mode: `place_labels` owns their visibility and hides them
            // off-tab, which a system that has stopped running cannot do.
            .add_systems(
                Update,
                ((rebuild_labels, place_labels).chain().after(restage_group),)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            // Not gated on the mode: the armed group is shown on the Map tab too, and a panel that
            // stops updating when you leave it is a panel that lies the moment you come back.
            .add_systems(
                Update,
                (rebuild.after(keys::Phase::Act))
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            // **After the rebuild**, so the geometry it reads describes the rows that are actually
            // on screen. Ungated by mode for the same reason `rebuild` is — the panel keeps itself
            // true off-tab, and a follower that stopped would hand it back scrolled to a stale row.
            .add_systems(
                Update,
                (keep_compose_selection_on_screen.after(rebuild))
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            // The pointer as a second way into both lists — same selection the arrows drive.
            // Observers rather than screen-gated systems: they fire on rows that only the Kit
            // door ever spawns, so the screen is already implied by the entity.
            .add_observer(on_comp_row_click)
            .add_observer(on_member_row_click);
    }
}

/// **Keep `selected` inside the list, at the one place it can leave it.**
///
/// The list shrinks when a composition is removed or the project is reloaded, and `selected` is a bare
/// index that knows nothing about either. It used to be clamped inside `lay_out` and nowhere else,
/// which did not fix the problem so much as hide it from one reader: with `selected = 5` over three
/// compositions the stage stood up `comps[2]`, the panel's `i == state.selected` marked nothing at all,
/// and `toggle_arm`'s `.get(5)` answered *"no composition to arm"*. Three readers, three answers, and
/// the only one that looked right was the picture.
///
/// Clamping where it goes stale means every reader can use the value as given. `member` rides along for
/// the same reason `walk` resets it: a different group has different members.
fn clamp_selection(project: Res<Project>, mut state: ResMut<ComposeState>) {
    if !project.is_changed() {
        return;
    }
    let n = project.compositions.compositions.len();
    let want = state.selected.min(n.saturating_sub(1));
    if state.selected != want {
        state.selected = want;
        state.member = 0;
    }
}

/// **The compose body follows whichever list has the arrows.**
///
/// Both lists live in ONE scroll area — `rebuild` builds the compositions and the selected group's
/// members into the same `ComposeBody` — so there is one thing to scroll and the question is only
/// which row to reveal. `Pane` answers it, and it is part of the key: moving focus from a group at
/// the top to a member far below is a move the eye has to be carried through, exactly like moving
/// within one list.
///
/// It had none. The rows became `list_row`s that the pointer and the arrows share, and the body
/// became scrollable, without anything to keep the highlight on screen — the same defect
/// `RigList` had, arriving by a different route. `every_list_follows_its_selection` is what found
/// it; this is the third list that ratchet has now caught.
///
/// Keyed through `chrome::Follow` rather than `is_changed`, for the reason that file records:
/// `ComposeState` carries a status note written most frames, so a change-gated follower would
/// re-arm every frame and never fire.
fn keep_compose_selection_on_screen(
    state: Res<ComposeState>,
    comp_rows: Query<(&CompRow, &ComputedNode, &UiGlobalTransform)>,
    member_rows: Query<(&MemberRow, &ComputedNode, &UiGlobalTransform)>,
    mut lists: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        (With<ComposeBody>, Without<CompRow>, Without<MemberRow>),
    >,
    mut follow: Local<crate::chrome::Follow<(Pane, usize)>>,
) {
    let want = match state.focus {
        Pane::Groups => (Pane::Groups, state.selected),
        Pane::Members => (Pane::Members, state.member),
    };
    if !follow.should_scroll(Some(want)) {
        return;
    }
    // A UI node's transform is its CENTRE, so the edges are the half-size either side. No match
    // means the index is momentarily past the rows — `clamp_selection` fixes that on the next
    // project change, and scrolling to a row that is not drawn is not a thing to invent.
    let row = match state.focus {
        Pane::Groups => comp_rows
            .iter()
            .find(|(r, _, _)| r.0 == state.selected)
            .map(|(_, n, t)| (t.translation.y, n.size.y * 0.5)),
        Pane::Members => member_rows
            .iter()
            .find(|(r, _, _)| r.0 == state.member)
            .map(|(_, n, t)| (t.translation.y, n.size.y * 0.5)),
    };
    let Some((row_mid, row_half)) = row else {
        return;
    };
    for (list, list_tf, mut scroll) in &mut lists {
        // Physical in, logical out — `ComputedNode` and `UiGlobalTransform` are physical pixels,
        // `ScrollPosition` is logical.
        if let Some(want) = crate::chrome::scroll_to_reveal(
            (row_mid, row_half),
            (list_tf.translation.y, list.size.y * 0.5),
            scroll.0.y,
            list.inverse_scale_factor,
        ) {
            scroll.0.y = want;
        }
    }
}

/// **`Option<Res<..>>`, because `Mode` belongs to a door.** See [`crate::editor::in_map_mode`]: every
/// run condition is evaluated, so a bare `Res<Mode>` panics on the menu screen where the door — and
/// its `Mode` — have been dropped.
fn in_compose_mode(mode: Option<Res<Mode>>) -> bool {
    mode.is_some_and(|m| *m == Mode::Compose)
}

fn spawn_compose_panel(mut commands: Commands, frame: Res<crate::chrome::Frame>) {
    crate::chrome::panel_root(
        &mut commands,
        &frame,
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
        // No static heading here: `rebuild` writes the COMPOSITIONS section itself, with the focus
        // affordance riding it — a fixed twin above it was the audit's double-heading finding.
        //
        // The one builder, not a hand copy of it. The copy this replaced had every field of
        // `scroll_list` except `ScrollArea` — and `bevy_ui_widgets` only scrolls `With<ScrollArea>`,
        // so the longest generated pane in the editor clipped its overflow and the wheel did
        // nothing. `tests/headless.rs::every_pane_that_clips_can_scroll` pins the class.
        crate::chrome::scroll_list(
            p,
            (
                ComposeBody,
                crate::notice::CopyPane(&[Mode::Compose]),
                crate::chrome::Control(crate::keys::ControlId::Detail),
            ),
        );
        // **Last, and it must be.** `margin-top: auto` is what pins it to the bottom of
        // the panel, and an auto margin in a column absorbs the free space above it — so
        // placed any earlier it pushes every sibling after it down with it.
    });
}

/// **What the tile set costs the solver**, as a line the panel can draw.
///
/// `grammar::MAX_PROTOTYPES` is 32 *"because `collapse_grid` packs a domain into a `u32`"*, and
/// `constraints::AMO_PAIRWISE_MAX` makes the clause count quadratic in it. Every builder pushes four
/// turns per tile and dedupes by face, so the number an author actually spends is not the number of
/// tiles they wrote — and until this existed the only way to learn it was to ask for a solve and be
/// refused. Códices et al. (`10.1109/access.2022.3168832`) argue the general case: a designer avoids
/// a generator whose limits they cannot see.
///
/// Nie et al. (`10.48550/arXiv.2308.07307`) say what the budget really bounds — a *sub-complete*
/// tileset needs `|T| >= max{|E|²}` per axis pair and is then provably backtrack-free, so 32 over
/// four turns is about **two edge tokens per axis and not three**. That readout is deliberately not
/// here: this codebase's faces are `Band` sequences rather than single tokens, so mapping them onto
/// the paper's edge types is a schema decision, and a wrong `sub-complete: yes` is worse than none.
///
/// **Held rather than derived where it is drawn.** `rebuild` runs on every arrow key, and building
/// the grammar derives an `interface` per tile per quarter turn — work that stopped being bounded by
/// the cap when the count moved to the end of the build. A number that only changes when the tiles
/// change is computed when the tiles change.
#[derive(Resource, Default)]
pub struct Budget {
    /// The row, already worded. Empty when the project has no bounded tile and so no budget to spend.
    pub line: String,
    /// Over the ceiling — drawn in the refusal colour, since that is what a solve will do.
    pub over: bool,
}

/// Recompute [`Budget`], and only when the tiles could have changed.
fn measure_budget(project: Res<Project>, mut budget: ResMut<Budget>) {
    if !project.is_changed() {
        return;
    }
    let comps = &project.compositions.compositions;
    let tiles = comps
        .iter()
        .filter(|c| matches!(c.envelope, Envelope::Bounded { .. }))
        .count();
    if tiles == 0 {
        // Not "0 of 32". An anchored group is not a prototype and never was, so a project of nothing
        // but furniture has no budget to be near — and a zero would read as headroom rather than as
        // a category that does not apply.
        *budget = Budget::default();
        return;
    }
    match emerge_core::grammar::from_compositions(
        comps,
        &project.library,
        project.lattice.face_bands,
        emerge_core::grid::TILE,
        // The same substitutable rule the generate path passes, so the count an author reads is the
        // count a solve will spend rather than a second opinion about it.
        composition::agrees,
    ) {
        Ok(c) => {
            *budget = Budget {
                line: format!(
                    "  {} of {} solver prototypes, from {tiles} bounded tile(s)",
                    c.grammar.len(),
                    emerge_core::grammar::MAX_PROTOTYPES,
                ),
                over: false,
            };
        }
        // The refusal already names the counts and what to do about them, so it is shown verbatim
        // rather than summarised into something shorter and less useful.
        Err(e) => {
            *budget = Budget {
                line: format!("  {e}"),
                over: true,
            };
        }
    }
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
    let by = if keys::just_pressed(&input, *keys, Action::ComposeMemberNext) {
        1
    } else if keys::just_pressed(&input, *keys, Action::ComposeMemberPrev) {
        -1
    } else {
        return;
    };
    state.focus = state.focus.step(by);
    let said = state.focus.label().to_owned();
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
    let by = if keys::just_pressed(&input, *keys, Action::ComposeNext) {
        step
    } else if keys::just_pressed(&input, *keys, Action::ComposePrev) {
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
    }
}

/// Arm the selected group, or disarm it if it was already armed.
///
/// Toggling rather than a separate disarm verb, for the reason `EditorState::brush` is an `Option`:
/// **nothing armed has to be a reachable state**, or an author cannot put the cursor over the map
/// without something following it.
/// `Enter` — **arms this composition for the Map**, and that is now all it does.
///
/// It used to mean two things decided by which list had focus: add the highlighted mesh, or arm the
/// group. The library list went with authoring, so one key means one thing again.
fn arm(
    mut state: ResMut<ComposeState>,
    project: Res<Project>,
    keys: Res<keys::Live>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if !keys::just_pressed(&input, *keys, Action::ComposeArm) {
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
    /// **Where the group's own contents sit relative to its origin**, in metres, unscaled.
    ///
    /// `(0, 0)` for a `Bounded` group, always: its envelope is centred on zero by construction —
    /// `interface` builds its scratch map at the origin and `Map::floor_rect` centres there.
    ///
    /// An `Anchored` group claims no envelope, so its members sit wherever they were authored, and
    /// they are frequently not centred on anything. [`footprint`] used to answer with the *span* and
    /// throw the centre away, which every caller then treated as though it were centred on zero — so
    /// a group whose members sat at `x ∈ [2, 3]` reported a width of 1, was drawn at the origin, and
    /// rendered a metre and a half outside the slot reserved for it, on top of its neighbour.
    ///
    /// [`restage_group`] subtracts this, so what lands in the slot is the *content*.
    pub centre: (f32, f32),
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
    /// **Groups in the window that could not be measured, and why.**
    ///
    /// Reported rather than fatal. `lay_out` used to propagate the first failure with `?`, so one
    /// neighbour holding an unmeasured piece blanked the entire stage — including the group being
    /// edited, its envelope and its lattice. A neighbour that cannot be placed is a neighbour that is
    /// not on the strip; it is not a reason to stop showing the thing you are looking at.
    ///
    /// Only an `Anchored` group can land here: a `Bounded` one answers from its declared envelope and
    /// cannot fail.
    pub unmeasured: Vec<String>,
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
///
/// **It answers with a centre as well as a size**, and the centre is not decoration — see
/// [`Slot::centre`] for the bug that came of discarding it. `Bounded` reports `(0, 0)` because that is
/// true of it by construction, which is what makes the two variants comparable instead of one of them
/// quietly meaning something else.
pub fn footprint(
    c: &Composition,
    comps: &[Composition],
    library: &emerge_core::library::Library,
) -> Result<((f32, f32), (f32, f32)), String> {
    match c.envelope {
        Envelope::Bounded { size } => Ok(((0.0, 0.0), (size.0, size.2))),
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
            let (centre, (w, d)) = span.map_or(((0.0, 0.0), (0.0, 0.0)), |(x0, x1, z0, z1)| {
                (((x0 + x1) * 0.5, (z0 + z1) * 0.5), (x1 - x0, z1 - z0))
            });
            let floor = emerge_core::grid::SNAP;
            Ok((centre, (w.max(floor), d.max(floor))))
        }
    }
}

/// **How tall one group stands**, which the camera needs and the floor plan does not say.
///
/// Framing on the footprint alone was measured to be wrong: four 1 × 1 tiles are 2.4 m tall, and a
/// view sized to their floor plan cut their tops off. Same two-variant match as [`footprint`] — a
/// `Bounded` group declares its height, an `Anchored` one is measured from what stands in it.
///
/// # A member is measured **through its host**, not from the floor
///
/// `stack::resolve_y` seats a member carrying `on: Some(host)` on **top of** that host, so a lamp on a
/// table stands at the table's height plus its own. Taking `lift + own_height` for every member treats
/// them all as floor-standing and reports the lamp alone — the same class of fault the `framing_height`
/// correction already fixed once for `Bounded`, and it cuts the top off the view in exactly the same
/// way.
///
/// Infinigen Indoors (`10.48550/arxiv.2406.11824`) names the relation this implements: *"**SupportedBy**
/// specifies a relation using a child object's planar surface and a parent object's planar surface…
/// the centroid of the child object is contained within the convex hull of the intersection"*. Two
/// things follow, and both are here. Height is defined *through* the host, so resolving host-first is a
/// topological walk over the support graph rather than an optimisation. And the relation is a
/// predicate over a *pair*, so an absent host makes it unsatisfiable — a missing host is a refusal
/// naming the member, never a silent fall back to the floor.
pub fn height_of(
    c: &Composition,
    comps: &[Composition],
    library: &emerge_core::library::Library,
) -> Result<f32, String> {
    match c.envelope {
        Envelope::Bounded { size } => Ok(size.1),
        Envelope::Anchored => {
            // Each member's own height, before anything is stacked on anything.
            let mut own: std::collections::BTreeMap<&str, f32> = std::collections::BTreeMap::new();
            for m in &c.members {
                own.insert(m.id.as_str(), member_height(c, m, comps, library)?);
            }
            let mut tallest = 0.0f32;
            for m in &c.members {
                tallest = tallest.max(m.lift + stacked_height(c, m, &own, 0)?);
            }
            Ok(tallest.max(emerge_core::grid::SNAP))
        }
    }
}

/// **How high the top of one member sits**, following `on` to the floor.
///
/// Depth-bounded rather than cycle-detected: `Composition::validate_shape` already refuses a member
/// resting on itself, and a bound that names the group is a better failure than a stack overflow if it
/// ever does not.
fn stacked_height(
    c: &Composition,
    m: &composition::Member,
    own: &std::collections::BTreeMap<&str, f32>,
    depth: usize,
) -> Result<f32, String> {
    let mine = own.get(m.id.as_str()).copied().unwrap_or(0.0);
    let composition::Body::Descriptor { on: Some(host), .. } = &m.body else {
        return Ok(mine);
    };
    if depth > composition::MAX_RESOLVED_MEMBERS {
        return Err(format!(
            "`{}` has a member stack more than {} deep through `{}`, which cannot be a real \
             arrangement",
            c.id,
            composition::MAX_RESOLVED_MEMBERS,
            m.id
        ));
    }
    let below = c.members.iter().find(|k| k.id == *host).ok_or_else(|| {
        format!(
            "`{}` seats `{}` on `{host}`, which is not a member of it — a member whose host is \
             missing has no height, and guessing the floor would draw it in the wrong place",
            c.id, m.id
        )
    })?;
    Ok(below.lift + stacked_height(c, below, own, depth + 1)? + mine)
}

/// One member's own height, ignoring whatever it rests on.
fn member_height(
    c: &Composition,
    m: &composition::Member,
    comps: &[Composition],
    library: &emerge_core::library::Library,
) -> Result<f32, String> {
    match &m.body {
        composition::Body::Descriptor { id, patch, .. } => {
            let base = library.get(id).ok_or_else(|| {
                format!("`{}` places descriptor `{id}`, which the library does not define", c.id)
            })?;
            let d = match patch {
                Some(p) => base.patched_with(p),
                None => base.clone(),
            };
            emerge_core::descriptor::placed_height(&d)
                .ok_or_else(|| format!("`{}` holds `{}`, which is unmeasured", c.id, m.id))
        }
        composition::Body::Composition { id } => {
            let child = comps
                .iter()
                .find(|k| k.id == *id)
                .ok_or_else(|| format!("`{}` nests `{id}`, which is not a composition here", c.id))?;
            match child.envelope {
                Envelope::Bounded { size } => Ok(size.1),
                // The same refusal `member_footprint` makes, for the same reason: an anchored child
                // declares no box, so a parent cannot bound it.
                Envelope::Anchored => Err(format!(
                    "`{}` nests `{id}`, which is anchored and so declares no height",
                    c.id
                )),
            }
        }
        // **A hole is not tall.** Nothing stands here yet, so there is no height to report and
        // inventing one would put a number on empty air. Zero is the honest answer for a position.
        composition::Body::Slot { .. } => Ok(0.0),
    }
}

/// One group sized for the strip, before its distance along it is known.
///
/// A named struct rather than the 5-tuple this used to be: it grew a sixth field, and
/// `focal.3`/`m.1` were already the kind of thing you have to count on your fingers.
#[derive(Clone, Copy)]
struct Measured {
    index: usize,
    offset: i32,
    scale: f32,
    /// At `scale`.
    size: (f32, f32),
    /// At `scale`.
    height: f32,
    /// Unscaled — see [`Slot::centre`].
    centre: (f32, f32),
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
) -> Carousel {
    if comps.is_empty() {
        return Carousel::default();
    }
    // **Not clamped here.** A stale `selected` used to be clamped locally, which made this the only
    // reader that saw a valid index: the stage stood up `comps[len - 1]`, the panel's `i == selected`
    // marked nothing, and `toggle_arm`'s `.get(selected)` said there was nothing to arm — three
    // readers, three different answers. `clamp_selection` now fixes it at the one place it goes
    // stale, which is the list shrinking, and this reads what everything else reads.
    if selected >= comps.len() {
        return Carousel::default();
    }

    // Measure first. A group that cannot be sized is **left off the strip and named**, not allowed to
    // take the rest of the stage with it.
    let mut measured: Vec<Measured> = Vec::new();
    let mut unmeasured: Vec<String> = Vec::new();
    for offset in -WINGS..=WINGS {
        let Some(index) = selected.checked_add_signed(offset as isize) else {
            continue;
        };
        let Some(c) = comps.get(index) else { continue };
        let scale = MINIATURE.powi(offset.abs());
        match (footprint(c, comps, library), height_of(c, comps, library)) {
            (Ok((centre, (w, d))), Ok(h)) => measured.push(Measured {
                index,
                offset,
                scale,
                size: (w * scale, d * scale),
                height: h * scale,
                centre,
            }),
            (Err(e), _) | (_, Err(e)) => unmeasured.push(e),
        }
    }

    /// A box's reach from its own centre along the strip — half the projection of an axis-aligned
    /// `w × d` onto [`STRIP`].
    fn reach(size: (f32, f32)) -> f32 {
        (size.0 * STRIP.0.abs() + size.1 * STRIP.1.abs()) * 0.5
    }

    let Some(&focal) = measured.iter().find(|m| m.offset == 0) else {
        // The one being edited is the one case where there is genuinely nothing to show.
        return Carousel { unmeasured, ..Default::default() };
    };
    let slot = |m: Measured, distance: f32| Slot {
        index: m.index,
        offset: m.offset,
        at: (distance * STRIP.0, distance * STRIP.1),
        scale: m.scale,
        size: m.size,
        height: m.height,
        centre: m.centre,
    };

    // The focal group sits at zero whatever its neighbours do, so stepping to the next composition
    // slides the strip past a fixed centre instead of re-centring a block that changed width.
    let mut slots = vec![slot(focal, 0.0)];
    for side in [1i32, -1] {
        let mut edge = reach(focal.size);
        for step in 1..=WINGS {
            // A skipped neighbour closes up rather than leaving a hole: the gap would read as a
            // group that is there and empty, which is a different and wronger thing.
            let Some(&m) = measured.iter().find(|m| m.offset == side * step) else {
                continue;
            };
            let distance = edge + SLOT_GAP + reach(m.size);
            edge = distance + reach(m.size);
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
    Carousel { slots, extent, tallest, unmeasured }
}

/// **Where a ray enters an axis-aligned box**, or `None`. The standard slab test.
///
/// Returns the entry distance, so a caller comparing several boxes picks the nearest — which is what
/// makes a tall near group win over a short far one whose ground footprint the ray also crosses.
fn ray_box(origin: Vec3, dir: Vec3, centre: Vec3, half: Vec3) -> Option<f32> {
    let (mut near, mut far) = (f32::NEG_INFINITY, f32::INFINITY);
    for a in 0..3 {
        if dir[a].abs() < 1e-6 {
            // Parallel to this pair of planes: either inside them forever, or never.
            if (origin[a] - centre[a]).abs() > half[a] {
                return None;
            }
            continue;
        }
        let t1 = (centre[a] - half[a] - origin[a]) / dir[a];
        let t2 = (centre[a] + half[a] - origin[a]) / dir[a];
        near = near.max(t1.min(t2));
        far = far.min(t1.max(t2));
    }
    let entry = near.max(0.0);
    (far >= entry).then_some(entry)
}

/// **Which composition the cursor is pointing at** — the inverse of [`lay_out`], for clicks.
///
/// Takes the ray in the stage's own space and tests it against each slot's **box**, nearest first.
///
/// # It used to test the ground point, and that was wrong by about a metre
///
/// The first version intersected the cursor with `y = 0` and asked which slot's floor footprint
/// contained it. Under this rig a screen point over a feature at height `h` lands on the ground
/// roughly `h` metres away from where that feature stands — so for a 2.4 m tile scaled to a 0.55 m
/// miniature, only ground points within 0.275 m of the slot centre were accepted, and clicking the
/// wall you can actually see returned `None`. Most of what was drawn for a slot was dead.
///
/// A ray against the box is exact at any yaw and elevation, which a closed-form inverse of the
/// projection would not be — it would bake in today's `ISO_ELEVATION` and break the moment the anim
/// bench's ground-level presets or a `Q`/`E` detent moved the eye.
///
/// The box is the **envelope** a `Bounded` group claims — the same wireframe `draw_stage` draws — so
/// what is clickable is what is visibly outlined, rather than the meshes' own silhouette.
pub fn slot_at(carousel: &Carousel, origin: Vec3, dir: Vec3) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for s in &carousel.slots {
        let centre = Vec3::new(s.at.0, s.height * 0.5, s.at.1);
        let half = Vec3::new(s.size.0 * 0.5, s.height * 0.5, s.size.1 * 0.5);
        let Some(t) = ray_box(origin, dir, centre, half) else {
            continue;
        };
        if best.is_none_or(|(best_t, _)| t < best_t) {
            best = Some((t, s.index));
        }
    }
    best.map(|(_, index)| index)
}

/// **The orthographic viewport height that shows the whole strip.**
///
/// Two extents matter and the first draft only had one. The rig looks along the XZ diagonal, so a
/// `w × d` ground rectangle spreads `(w + d) / √2` across the screen; but the groups also *stand up*,
/// and a vertical metre projects to `cos(elevation)` of one. Framing on the floor plan alone cut the
/// tops off four 2.4 m tiles — measured in a captured frame, not predicted.
///
/// **`usable_aspect` is the width of the hole the world is drawn through, over its height.** The
/// camera's viewport is `chrome::ViewportSlot`'s rect, not the window (`surface::fit_viewport`), so
/// the panels cannot be drawn over — but a horizontal fit that ignores the ratio will still push a
/// wide subject off the sides of that hole. Pass [`SQUARE`] to keep the old conservative assumption.
///
/// Returned unclamped **at both ends**: the caller decides what to do when it exceeds
/// [`crate::view::MAX_ZOOM`], and the caller supplies its own floor. The floor used to be
/// `TILE_VIEW_HEIGHT` inside here, which was right for a sheet of tiles and wrong the moment the
/// mesh stage wanted to frame a 12 cm mug — it could never zoom closer than three metres.
pub fn framing_height(extent: (f32, f32), tallest: f32, usable_aspect: f32) -> f32 {
    /// A little air around the strip, so the outermost miniature is not flush with the window edge.
    const MARGIN: f32 = 1.15;
    let spread = (extent.0 + extent.1) * std::f32::consts::FRAC_1_SQRT_2;
    let elevation = crate::view::ISO_ELEVATION;
    let vertical = spread * elevation.sin() + tallest * elevation.cos();
    // The viewport is `height` metres tall and `height * usable_aspect` metres wide, so a subject
    // spreading `spread` across it needs `spread / usable_aspect` of height to fit sideways.
    (vertical.max(spread / usable_aspect.max(f32::EPSILON)) * MARGIN).max(0.0)
}

/// **A viewport as wide as it is tall** — the assumption this function used to make for everyone.
///
/// Still what the Compose sheet passes: its own hole is not square either, but its framing has been
/// judged in captured frames at this value and re-fitting it is a separate change with its own
/// before-and-after. Named rather than a bare `1.0` so the assumption is greppable.
pub const SQUARE: f32 = 1.0;

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
/// rather than a second colour to learn. **Derived, not transcribed** — this was a hand-halved
/// copy of `ACCENT`'s channels, which a change to `ACCENT` would have silently desynchronised.
const ENVELOPE_IDLE: Color = crate::chrome::scaled(crate::chrome::ACCENT, 0.5);

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
const LABEL_PX: (f32, f32) = (crate::chrome::text::BODY, crate::chrome::text::HINT);

/// Advance of one glyph at `LABEL_PX.0` — the one measurement, stated beside the size it
/// belongs to. See [`crate::chrome::BODY_CHAR_W`] for why it lives there.
const LABEL_CHAR_W: f32 = crate::chrome::BODY_CHAR_W;

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

    let carousel = lay_out(comps, &project.library, state.selected);
    // **One message, however many groups fail.** `Status::problem` folds only CONSECUTIVE identical
    // texts into a count, so two groups failing with different reasons used to accumulate as separate
    // entries on every restage and push the log past `MAX_PROBLEMS`. Gathered and said once.
    let mut refused: Vec<String> = carousel.unmeasured.clone();
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
            // **No lattice here, because a map no longer carries one.** This scratch map exists
            // to stand the group up exactly as stamping it would; the grid it is read on is the
            // project's, passed to `interface` and `pitch` directly rather than smuggled through
            // a map that would then be a second place to state it.
            bash: None,
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
        let expanded = match composition::expand(&scratch, &[stamp], comps, &project.library) {
            Ok(e) => e,
            // Loud. An empty patch of floor where a composition should be, with nothing saying why,
            // is the failure this editor's own notes call the worst it had.
            Err(e) => {
                refused.push(format!("`{}` does not resolve: {e}", c.id));
                continue;
            }
        };
        let mut with_rows = scratch.clone();
        with_rows.placements.extend(expanded.placements.iter().cloned());
        let ys = match emerge_core::stack::resolve_y(&with_rows, &project.library) {
            Ok(ys) => ys,
            Err(e) => {
                refused.push(format!("`{}` has no height: {e}", c.id));
                continue;
            }
        };
        let parent = commands
            .spawn((
                Name::new(format!("staged {}", c.id)),
                StagedGroup,
                // **The slot's position, less where the group's own contents sit.** Scaled, because
                // the offset is in the group's space and this transform scales that space. Zero for
                // every `Bounded` group, so the tiles are placed exactly as before; it is only an
                // `Anchored` group — whose members sit wherever they were authored — that would
                // otherwise render outside the slot reserved for it. See `Slot::centre`.
                Transform::from_translation(
                    COMPOSE_STAGE
                        + Vec3::new(
                            slot.at.0 - slot.centre.0 * slot.scale,
                            0.0,
                            slot.at.1 - slot.centre.1 * slot.scale,
                        ),
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
    let want = framing_height(carousel.extent, carousel.tallest, SQUARE)
        .max(crate::tiles::TILE_VIEW_HEIGHT);
    if want > crate::view::MAX_ZOOM {
        state.status.problem(format!(
            "this composition and its neighbours need a {want:.0} m view and the camera stops at \
             {:.0} m \
             — the outer miniatures are cropped.",
            crate::view::MAX_ZOOM,
        ));
    }
    if !refused.is_empty() {
        state.status.problem(format!(
            "{} group(s) are not on the stage: {}",
            refused.len(),
            refused.join("; ")
        ));
    }
    // **Written only when it differs.** `ResMut` marks a resource changed on any deref_mut, and
    // `tiles::stage_camera` re-frames on that edge — so an unconditional write threw the author's pan
    // and zoom away on every edit that re-ran this system, which is the very thing `tiles.rs`'s own
    // note warns about. `Carousel` derives `PartialEq` and the layout is a pure function of its
    // inputs, so identical inputs compare equal and the camera stays where it was put.
    if carousel_out.0 != carousel {
        carousel_out.0 = carousel;
    }
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
        // that `editor::Rung`'s note records. The focal group is always at scale 1, so this is the
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
                crate::chrome::GRID_LINE,
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
                // PLACES-ITSELF-OK: a world-space slot label, put where `place_labels` projects it. Flow
                // has no opinion about where a point in the scene lands on screen.
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
/// **`world_to_viewport` answers in logical viewport pixels; `Val::Px` is multiplied by [`UiScale`].**
///
/// Both entry points set `UiScale(1.2)` (`main.rs`, `harness.rs`), so writing the projection straight
/// into a `Val::Px` put every label 20% further from the top-left than the point it names — invisible
/// on the focal group at the centre of the screen, worst on the outermost miniatures, which is exactly
/// the shape that reads as "the labels are a bit off" rather than as a bug.
fn place_labels(
    mode: Res<Mode>,
    carousel: Res<StagedCarousel>,
    ui_scale: Res<UiScale>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<crate::view::MainCamera>>>,
    mut labels: Query<(&SlotLabel, &Text, &mut Node)>,
    ui_nodes: Query<
        (&bevy::ui::ComputedNode, &bevy::ui::UiGlobalTransform),
        With<bevy::picking::hover::Hovered>,
    >,
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
        // **A label the panel covers is taken down, not drawn through.** The projection knows
        // nothing of panel geometry, so a slot near the screen edge landed its label half behind
        // the controls panel — an orphan glyph at the panel's edge (captured, 2026-08-17 audit).
        // The panels' rects are already the one answer to "is this screen point on the interface"
        // — `view::over_ui`, geometry off the same `Hovered` roots every other reader uses — so
        // ask it for the label's two ends and its middle rather than restating a panel width here.
        let placed = placed.filter(|(p, advance)| {
            let half = text.0.chars().count() as f32 * *advance * 0.5;
            let factor = cam.target_scaling_factor().unwrap_or(1.0);
            ![-half, 0.0, half].into_iter().any(|dx| {
                crate::view::over_ui(
                    Some(Vec2::new(p.x + dx, p.y + LABEL_DROP)),
                    factor,
                    ui_nodes.iter(),
                )
            })
        });
        match placed {
            // Centred by measuring the string, which works because the shipped face is monospace —
            // see `LABEL_CHAR_W`.
            Some((p, advance)) => {
                let half = text.0.chars().count() as f32 * advance * 0.5;
                // Into `Val::Px`'s own units, which are scaled ones. A zero or negative scale would be
                // a host misconfiguration rather than a state to render around; guard so it cannot
                // produce a NaN position.
                let s = if ui_scale.0 > 0.0 { ui_scale.0 } else { 1.0 };
                let (left, top) =
                    (Val::Px((p.x - half) / s), Val::Px((p.y + LABEL_DROP) / s));
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
        keys::just_pressed(&keys, *live, Action::CarouselNext),
        keys::just_pressed(&keys, *live, Action::CarouselPrev),
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
    let Some((origin, dir)) = crate::view::cursor_ray(pointer.0, cam, cam_tf) else {
        return;
    };
    pick_along(&carousel.0, origin, dir, &mut state);
}

/// **Bring whichever slot the ray hits to the middle**, and say whether it moved.
///
/// `pub` and separate for the reason `toggle_arm` is: everything above it needs a camera with a real
/// render target, and `MinimalPlugins` has none — `world_to_viewport` and `viewport_to_world` both
/// answer `Err` with no window, so a headless test that goes through them asserts nothing while
/// looking like it asserts something. The projection is the engine's; this is the part that is ours.
pub fn pick_along(
    carousel: &Carousel,
    origin: Vec3,
    dir: Vec3,
    state: &mut ComposeState,
) -> bool {
    // Into the stage's own space, which is where the slots are laid out.
    let Some(i) = slot_at(carousel, origin - COMPOSE_STAGE, dir) else {
        return false;
    };
    if state.selected == i {
        return false;
    }
    state.selected = i;
    // A different group has different members, the same reason `walk` resets it.
    state.member = 0;
    true
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
    budget: Res<Budget>,
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
    let mut lines: Vec<Line> = Vec::new();

    // The one COMPOSITIONS heading, with the focus affordance riding it — the panel used to draw a
    // static section AND a second ACCENT twin of it when the list had focus, the audit's
    // double-heading finding. The name field is not drawn here at all — it is `chrome::NameBox`,
    // centred over the viewport, shared with the Map's own capture verb.
    lines.push(Line::Section(format!(
        "COMPOSITIONS{}",
        if state.focus == Pane::Groups { "  <- arrows" } else { "" }
    )));
    // **Where you are in the list, and how to move.** The stage shows the focal group among its
    // neighbours, but not how many there are or how far in you have got — and `status.note` says it
    // on the keypress and is then replaced by the next message, which is the wrong lifetime for it.
    if !comps.is_empty() {
        lines.push(Line::Prose(
            format!(
                "{} of {}   {} / {}",
                state.selected.min(comps.len() - 1) + 1,
                comps.len(),
                keys::chord(Action::CarouselPrev),
                keys::chord(Action::CarouselNext),
            ),
            DIM,
        ));
    }
    if !budget.line.is_empty() {
        lines.push(Line::Prose(
            budget.line.clone(),
            if budget.over { DANGER } else { DIM },
        ));
    }
    if comps.is_empty() {
        lines.push(Line::Prose(
            "No groups. The project's `compositions.ron` defines them — one collection, so a tile \
             may seat any bound kit's pieces. A project with none stamps nothing, which is not a \
             broken one."
                .to_owned(),
            DIM,
        ));
    }

    for (i, c) in comps.iter().enumerate() {
        let envelope = match c.envelope {
            Envelope::Anchored => "anchored".to_owned(),
            Envelope::Bounded { size } => format!("bounded {:.1}x{:.1}x{:.1}", size.0, size.1, size.2),
        };
        lines.push(Line::Comp {
            ix: i,
            text: format!("{}  —  {} member(s), {envelope}", c.id, c.members.len()),
            selected: i == state.selected,
            armed: state.armed.as_deref() == Some(c.id.as_str()),
        });
    }

    if let Some(c) = comps.get(state.selected) {
        lines.push(Line::Section(format!(
            "MEMBERS OF `{}`{}",
            c.id,
            if state.focus == Pane::Members { "  <- arrows" } else { "" }
        )));
        let at = state.member.min(c.members.len().saturating_sub(1));
        for (i, m) in c.members.iter().enumerate() {
            lines.push(Line::Member { ix: i, text: describe_member(m), at: i == at });
        }
        affordances(&mut lines, c);
        detail(&mut lines, c, comps, &project);
    }

    // The receipt only. The refusal is the block under the title — see `chrome::Status`.
    if !state.status.note_text().is_empty() {
        lines.push(Line::Prose(state.status.note_text().to_owned(), ACCENT));
    }

    commands.entity(root).with_children(|p| {
        for line in lines {
            match line {
                Line::Section(text) => {
                    crate::chrome::section(p, &text).insert(ComposeLine);
                }
                Line::Prose(text, colour) => spawn_line(p, &text, colour),
                Line::Comp { ix, text, selected, armed } => {
                    crate::chrome::list_row(p, selected, (CompRow(ix), ComposeLine)).with_children(
                        |row| {
                            row.spawn((
                                Text::new(text),
                                TextColor(TEXT),
                                TextFont::from_font_size(ROW_PX),
                            ));
                            if armed {
                                // The armed mark in its own ink — the same `*` the stage labels
                                // carry for "this is the one a stamp places".
                                row.spawn((
                                    Text::new("  *"),
                                    TextColor(ACCENT),
                                    TextFont::from_font_size(ROW_PX),
                                ));
                            }
                        },
                    );
                }
                Line::Member { ix, text, at } => {
                    crate::chrome::list_row(p, at, (MemberRow(ix), ComposeLine)).with_children(
                        |row| {
                            row.spawn((
                                Text::new(text),
                                TextColor(TEXT),
                                TextFont::from_font_size(ROW_PX),
                            ));
                        },
                    );
                }
                Line::Rail { tint, lines } => {
                    crate::chrome::severity_rail(p, tint, ComposeLine).with_children(|block| {
                        for (text, colour) in &lines {
                            spawn_line(block, text, *colour);
                        }
                    });
                }
            }
        }
    });
}

/// A click selects the composition, the way a click selects a row in every other list — the arrows
/// keep working exactly as before; this adds the pointer as a second way in, not a second meaning.
fn on_comp_row_click(
    activate: On<bevy::ui_widgets::Activate>,
    rows: Query<&CompRow>,
    mut state: ResMut<ComposeState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    state.selected = row.0;
    state.focus = Pane::Groups;
}

/// A click puts the seating cursor on a member and hands the arrows to the member list.
fn on_member_row_click(
    activate: On<bevy::ui_widgets::Activate>,
    rows: Query<&MemberRow>,
    mut state: ResMut<ComposeState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    state.member = row.0;
    state.focus = Pane::Members;
}

/// The body's one font size. Named because [`spawn_line`] derives an advance from it.
const ROW_PX: f32 = crate::chrome::text::BODY;

/// A row's leading indent, in spaces, and what is left. The pane states its structure as leading
/// spaces (`"    "` under OFFERS, `" ".repeat(7)` in the face table, the hex STALE lines); this is
/// where that convention is read back so the layout can honour it.
fn split_indent(text: &str) -> (usize, &str) {
    let rest = text.trim_start_matches(' ');
    (text.len() - rest.len(), rest)
}

/// **One row: the indent as width, the text beside it, wrapping under itself.**
///
/// A bare `Text` per row wraps its continuation to column zero, which throws away the leading-space
/// indent — a long fault message or hex STALE line restarted at the left margin, mid-scheme
/// (captured, 2026-08-17 audit). `chrome::problem_log_line` is the precedent and states both halves:
/// the gutter must not be in the wrapping column, and `min_width: 0` is load-bearing because a flex
/// item will not shrink below its min-content width by default.
///
/// The indent becomes a **width-only spacer node**, not a `Text` of spaces: `notice::collect_text`
/// harvests every non-blank `Text` under the pane for `Cmd+C`, and a text gutter would split each
/// row into two copied lines. The advance is the same monospace fact [`LABEL_CHAR_W`] states for
/// the world labels, scaled from [`LABEL_PX`] to [`ROW_PX`].
fn spawn_line(p: &mut ChildSpawnerCommands, text: &str, colour: Color) {
    let (indent, content) = split_indent(text);
    // No stated width: the body is a column whose default `align_items: Stretch` already hands
    // every row the pane's full width. `Val::Percent` here is worse than redundant — inside the
    // scroll pane's measure pass a percent can resolve against nothing and collapse the row.
    p.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            ..default()
        },
        ComposeLine,
    ))
    .with_children(|row| {
        if indent > 0 {
            row.spawn(Node {
                min_width: Val::Px(indent as f32 * LABEL_CHAR_W * ROW_PX / LABEL_PX.0),
                flex_shrink: 0.0,
                ..default()
            });
        }
        row.spawn((
            Node {
                // `flex_grow` hands the text the row's remaining width as a DEFINITE size, and
                // `min_width: 0` lets it shrink below its longest line. Measured in the live
                // window (uidump, 2026-08-17): without the grow, the text item's flex base
                // resolved to zero width and every row laid out one glyph per line, clipped —
                // an empty-looking pane over 38 healthy rows.
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            },
            Text::new(content.to_owned()),
            TextColor(colour),
            TextFont::from_font_size(ROW_PX),
            // The default, stated: this is the column that wraps, against the spacer that never
            // does.
            TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
        ));
    });
}

#[cfg(test)]
mod line_tests {
    use super::split_indent;

    /// The split is byte-exact on the shapes the pane actually emits, and the indent is spaces
    /// ONLY — the `>`/`*` cursor markers stay in the wrapping column, because they are content
    /// (the copy harvest keeps them) and two characters of drift on a wrapped member row is not
    /// the failure the spacer exists for.
    #[test]
    fn the_indent_is_leading_spaces_and_nothing_else() {
        assert_eq!(split_indent(""), (0, ""));
        assert_eq!(split_indent("    "), (4, ""));
        assert_eq!(split_indent("no indent"), (0, "no indent"));
        assert_eq!(split_indent("> member"), (0, "> member"));
        assert_eq!(split_indent("  1 of 2"), (2, "1 of 2"));
        assert_eq!(split_indent("    meal over table"), (4, "meal over table"));
        assert_eq!(split_indent("        eat: diner"), (8, "eat: diner"));
        // The face table: 4-space block indent plus `{:>5}` padding, one column either way.
        assert_eq!(split_indent("      eat: n 0.00..1.00"), (6, "eat: n 0.00..1.00"));
    }
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
        // **A hole takes no floor**, so seating it is bounded by the envelope alone. Refusing here
        // instead would make a slot unseatable, and the author positions it with the same keys as
        // everything else; answering a real zero is what lets those keys work on it.
        composition::Body::Slot { .. } => Ok((0.0, 0.0)),
    }
}

/// **Seat a member one step, or refuse and say why.** Returns the new `(at, lift)`.
///
/// # The lattice is not defined here
///
/// Horizontal steps are [`emerge_core::grid::SNAP`] and vertical ones are `SNAP / divisions` — the
/// exact quanta [`crate::editor`]'s `snap` and `lift_step` already apply to every `Placed`. A second
/// quantum for the same act is the mistake `editor::Rung`'s note records: the drawn grid said 1.0 m
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

/// **One seat step, metres** — a rung of the project's ladder, 333 mm at the default divisor of 3.
///
/// The **spatial** number, never the face one. Edge tokens belong to a face and space belongs to a
/// volume; one number serving both meant a finer seat cost a re-index of every authored token.
/// [`emerge_core::policy::Policy::face_bands`] carries that argument.
///
/// # It is the same ladder the Map places on, and that is the change
///
/// This used to be `grid::SNAP / seating_divisions` — a second spatial lattice, dividing the
/// half-metre while the Map's divided the tile. So "divide a tile into four" produced eight seats,
/// and a member seated inside a tile sat on a grid no click on the Map could reach. One ladder at two
/// scales is what lets a tile authored today abut a tile authored last month.
///
/// Seats are multiples of this from the envelope's centre in X/Z and its floor in Y, so the centre is
/// always a seat and nudging out and back returns exactly.
pub fn seat_step(project: &Project, level: emerge_core::grid::SnapLevel) -> f32 {
    level.pitch(project.lattice.snap_divisor)
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
        // Angle brackets rather than square, so a hole never reads as a thing that is there — the
        // list is scanned, and the one property an author needs at a glance is which rows are real.
        composition::Body::Slot { accepts } => (format!("<{accepts}>"), "open".to_owned()),
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
fn affordances(lines: &mut Vec<Line>, c: &Composition) {
    if c.locations.is_empty() {
        return;
    }
    lines.push(Line::Section("OFFERS".to_owned()));
    for l in &c.locations {
        lines.push(Line::Prose(format!("    {} over {}", l.id, l.props.join(", ")), TEXT));
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
            lines.push(Line::Prose(format!("        {}: {}", i.verb, roles.join("; ")), DIM));
        }
    }
}

/// The three blocks that exist so the tool can explain itself: stale, interface, faults.
fn detail(lines: &mut Vec<Line>, c: &Composition, comps: &[Composition], project: &Project) {
    // **STALE, with the numbers.** Naming the member and both fingerprints is the difference between
    // a badge that sends someone to the right file and one that only says something is wrong.
    match composition::stale_members(c, comps, &project.library) {
        Ok(report) if report.is_empty() => {
            lines.push(Line::Prose(
                "UP TO DATE — every member matches what it was built against".to_owned(),
                DIM,
            ));
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
                // A railed block, like every other refusal that stays true — the rail's inset
                // replaces the old hand-indented hex rows.
                let mut rail = vec![(
                    format!("STALE — {} member(s) changed underneath this composition", drifted.len()),
                    DANGER,
                )];
                for s in drifted {
                    rail.push((
                        format!(
                            "`{}` was {:#018x}, is now {:#018x}",
                            s.member,
                            s.recorded.unwrap_or_default(),
                            s.measured
                        ),
                        DANGER,
                    ));
                }
                lines.push(Line::Rail { tint: DANGER, lines: rail });
            }
            if !unrecorded.is_empty() {
                lines.push(Line::Prose(
                    format!(
                        "UNRECORDED — {} member(s) have never been measured, so nothing can be said \
                         about drift yet",
                        unrecorded.len()
                    ),
                    DIM,
                ));
            }
        }
        Err(e) => lines.push(Line::Prose(format!("cannot check staleness: {e}"), DANGER)),
    }

    lines.push(Line::Prose(String::new(), TEXT));
    match composition::interface(c, comps, &project.library, project.lattice.face_bands) {
        Ok(None) => lines.push(Line::Prose(
            "ANCHORED — claims no tile, so it has no boundary for anything to abut".to_owned(),
            DIM,
        )),
        Ok(Some(iface)) => {
            lines.push(Line::Section(
                "DERIVED INTERFACE — read off the members, never authored".to_owned(),
            ));
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
                    lines.push(Line::Prose(format!("    {label}{line}"), TEXT));
                }
            }
            if iface.is_clean() {
                lines.push(Line::Prose(
                    "    clean — this composition can constrain a neighbour".to_owned(),
                    DIM,
                ));
            } else {
                let mut rail = vec![(
                    format!("{} FAULT(S) — members disagree about a face", iface.faults.len()),
                    DANGER,
                )];
                for f in &iface.faults {
                    rail.push((f.message.clone(), DANGER));
                }
                lines.push(Line::Rail { tint: DANGER, lines: rail });
            }
        }
        Err(e) => lines.push(Line::Prose(format!("cannot derive an interface: {e}"), DANGER)),
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
            bash: None,
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
            let c = lay_out(&comps, &lib(), selected);
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
        let c = lay_out(&comps, &lib(), 4);
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
        let first = lay_out(&comps, &lib(), 0);
        assert!(first.slots.iter().all(|s| s.offset >= 0), "nothing stands before the first group");
        assert_eq!(first.slots.len(), 1 + WINGS as usize);

        let last = lay_out(&comps, &lib(), comps.len() - 1);
        assert!(last.slots.iter().all(|s| s.offset <= 0), "nothing stands after the last group");

        // A kit smaller than the strip is simply a shorter strip, not a repeated one.
        let small = kit(2);
        let c = lay_out(&small, &lib(), 0);
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
        let c = lay_out(&comps, &lib(), 4);
        for pair in c.slots.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(b.offset == a.offset + 1, "slots must come out in strip order");
            let apart = ((b.at.0 - a.at.0).powi(2) + (b.at.1 - a.at.1).powi(2)).sqrt();
            let touching = (a.size.0 + a.size.1) * 0.5 * std::f32::consts::FRAC_1_SQRT_2
                + (b.size.0 + b.size.1) * 0.5 * std::f32::consts::FRAC_1_SQRT_2;
            assert!(apart > touching, "offsets {} and {} overlap", a.offset, b.offset);
        }
    }

    /// The view direction of the default rig, normalised — `ISO_OFFSET` is `(12, 12, 12)`, so the
    /// camera looks along `(-1, -1, -1)`.
    fn iso_dir() -> bevy::prelude::Vec3 {
        bevy::prelude::Vec3::new(-1.0, -1.0, -1.0).normalize()
    }

    /// A ray that passes through `target`, coming from far away along the view direction.
    fn ray_through(target: bevy::prelude::Vec3) -> (bevy::prelude::Vec3, bevy::prelude::Vec3) {
        let dir = iso_dir();
        (target - dir * 100.0, dir)
    }

    /// **The bug this test exists for.** Picking used to intersect the cursor with `y = 0` and ask
    /// which slot's FLOOR footprint contained the result. Under this rig a screen point over a
    /// feature at height `h` lands on the ground about `h` metres away from where that feature is —
    /// so clicking the visible wall of a miniature returned nothing, and only the sliver of floor at
    /// its base worked. Most of what was drawn for a slot was dead.
    ///
    /// Aimed at the TOP of each slot, which is the part an author actually clicks and the part the
    /// old ground test could never resolve.
    #[test]
    fn a_click_on_a_miniatures_body_picks_it_rather_than_missing_by_its_own_height() {
        let comps = kit(9);
        let c = lay_out(&comps, &lib(), 4);
        for s in &c.slots {
            // Just under the top face, dead centre — the tallest visible part of the group.
            let top = bevy::prelude::Vec3::new(s.at.0, s.height * 0.95, s.at.1);
            let (origin, dir) = ray_through(top);
            assert_eq!(
                slot_at(&c, origin, dir),
                Some(s.index),
                "a click on the body of offset {} has to pick it; its ground point is {:.2} m away",
                s.offset,
                s.height * 0.95
            );
            // And the base still works, which is all the old test ever checked.
            let base = bevy::prelude::Vec3::new(s.at.0, 0.01, s.at.1);
            let (origin, dir) = ray_through(base);
            assert_eq!(slot_at(&c, origin, dir), Some(s.index), "offset {} at its base", s.offset);
        }
    }

    /// The air between two slots belongs to neither, and off the strip picks nothing.
    #[test]
    fn a_click_that_lands_on_nothing_picks_nothing() {
        let comps = kit(9);
        let c = lay_out(&comps, &lib(), 4);
        let a = c.slots.iter().find(|s| s.offset == 0).unwrap_or_else(|| panic!("no focal"));
        let b = c.slots.iter().find(|s| s.offset == 1).unwrap_or_else(|| panic!("no +1"));
        let between = bevy::prelude::Vec3::new(
            (a.at.0 + b.at.0) * 0.5,
            a.height * 0.5,
            (a.at.1 + b.at.1) * 0.5,
        );
        let (origin, dir) = ray_through(between);
        assert_eq!(slot_at(&c, origin, dir), None, "the gap between two slots picks neither");

        let (origin, dir) = ray_through(bevy::prelude::Vec3::new(1000.0, 1.0, 1000.0));
        assert_eq!(slot_at(&c, origin, dir), None, "and off the strip picks nothing");
    }

    /// **Nearest wins.** Two boxes can both lie on one ray — a tall near group in front of a far one —
    /// and the answer has to be the one you can see, not whichever the loop reached first.
    #[test]
    fn when_a_ray_crosses_two_slots_the_nearer_one_is_picked() {
        let comps = kit(9);
        let c = lay_out(&comps, &lib(), 4);
        let near = c.slots.iter().find(|s| s.offset == 0).unwrap_or_else(|| panic!("no focal"));
        let dir = iso_dir();
        // A ray through the focal group, continued far enough that it would leave the strip.
        let through = bevy::prelude::Vec3::new(near.at.0, near.height * 0.5, near.at.1);
        let origin = through - dir * 100.0;
        let hit = slot_at(&c, origin, dir).unwrap_or_else(|| panic!("the ray must hit something"));
        assert_eq!(hit, near.index, "the nearest box on the ray is the one that was clicked");
    }

    /// A bounded group declares its box; an anchored one is measured from what stands in it.
    #[test]
    fn an_anchored_group_is_measured_and_a_bounded_one_is_taken_at_its_word() {
        let comps = vec![
            tile("bounded", 3.0, 2.0),
            anchored("table", vec![member("l", "wall", (-1.0, 0.0)), member("r", "wall", (1.0, 0.0))]),
        ];
        let l = lib();
        assert_eq!(
            footprint(&comps[0], &comps, &l).unwrap_or_else(|e| panic!("{e}")),
            ((0.0, 0.0), (3.0, 2.0)),
            "a bounded group is centred on zero by construction"
        );
        assert_eq!(height_of(&comps[0], &comps, &l).unwrap_or_else(|e| panic!("{e}")), 2.4);
        // Two 1 × 1 × 1 pieces two metres apart span 3 m in x, 1 m in z, and stand 1 m tall.
        assert_eq!(
            footprint(&comps[1], &comps, &l).unwrap_or_else(|e| panic!("{e}")),
            ((0.0, 0.0), (3.0, 1.0))
        );
        assert_eq!(height_of(&comps[1], &comps, &l).unwrap_or_else(|e| panic!("{e}")), 1.0);
    }

    /// **An anchored group whose members sit off their own origin reports where they are.**
    ///
    /// The span alone is not enough, and the missing half was invisible for as long as every test
    /// used a symmetric group: `footprint` returned a width and every caller drew that width centred
    /// on zero, so a group authored two metres east rendered two metres outside the slot reserved
    /// for it — on top of its neighbour on the strip.
    #[test]
    fn an_off_centre_anchored_group_reports_where_its_contents_are() {
        let comps = vec![anchored(
            "shoved_east",
            vec![member("l", "wall", (2.0, 0.0)), member("r", "wall", (4.0, 0.0))],
        )];
        let (centre, size) =
            footprint(&comps[0], &comps, &lib()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(size, (3.0, 1.0), "the span is unchanged — this is not a resize");
        assert_eq!(centre, (3.0, 0.0), "and the centre says where that span actually sits");

        // And it reaches the stage: the slot subtracts it, so what lands in the slot is the content.
        let carousel = lay_out(&comps, &lib(), 0);
        assert_eq!(carousel.slots[0].centre, (3.0, 0.0));
    }

    /// **A member resting on another is measured through its host, not from the floor.**
    ///
    /// `stack::resolve_y` seats a member with `on: Some(host)` on top of that host, so taking
    /// `lift + own_height` for everything reports the lamp and forgets the table under it — and the
    /// camera then frames a view that cuts the top off.
    #[test]
    fn a_stacked_member_is_measured_through_its_host() {
        // `wall` is 1 × 1 × 1 in the fixture library, so a stack of two stands 2 m tall.
        let mut top = member("top", "wall", (0.0, 0.0));
        if let emerge_core::composition::Body::Descriptor { on, .. } = &mut top.body {
            *on = Some("base".to_owned());
        }
        let comps = vec![anchored("stack", vec![member("base", "wall", (0.0, 0.0)), top])];
        assert_eq!(
            height_of(&comps[0], &comps, &lib()).unwrap_or_else(|e| panic!("{e}")),
            2.0,
            "the stacked member stands on its host, so the group is twice one piece"
        );
    }

    /// A member whose host is missing is **a refusal naming the member**, never a fall back to the
    /// floor — Infinigen's support relation is a predicate over a pair, and an absent parent makes it
    /// unsatisfiable rather than defaulted.
    #[test]
    fn a_member_seated_on_a_missing_host_refuses_and_names_it() {
        let mut orphan = member("lamp", "wall", (0.0, 0.0));
        if let emerge_core::composition::Body::Descriptor { on, .. } = &mut orphan.body {
            *on = Some("table_that_is_not_here".to_owned());
        }
        let comps = vec![anchored("orphaned", vec![orphan])];
        let err = height_of(&comps[0], &comps, &lib()).expect_err("refuses");
        assert!(err.contains("lamp"), "names the member: {err}");
        assert!(err.contains("table_that_is_not_here"), "and the host it wanted: {err}");
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
        // **And `lay_out` reports it rather than blanking the stage.** It used to propagate the first
        // failure with `?`, so one unmeasurable neighbour took the group being edited down with it.
        let c = lay_out(&comps, &lib(), 0);
        assert!(c.slots.is_empty(), "the focal group itself cannot be measured, so nothing stands");
        assert_eq!(c.unmeasured.len(), 1, "and the reason is carried rather than thrown");
        assert!(c.unmeasured[0].contains("mystery"), "{:?}", c.unmeasured);
    }

    /// **A neighbour that cannot be measured is left off the strip, not allowed to take it down.**
    ///
    /// `lay_out` propagated the first measurement failure with `?`, so one unmeasurable group within
    /// two slots of the focal one blanked the WHOLE stage — the group being edited, its envelope, its
    /// lattice and its ring. `compositions.ron` is hand-authored and the schema permits a member whose
    /// descriptor the library does not define, so this is reachable by editing a file, not by a bug.
    #[test]
    fn an_unmeasurable_neighbour_is_left_off_rather_than_blanking_the_stage() {
        let mut comps = kit(5);
        // The neighbour at +1 holds a piece the library does not define.
        comps[3] = anchored("broken", vec![member("m", "absent", (0.0, 0.0))]);
        let c = lay_out(&comps, &lib(), 2);

        assert_eq!(c.focal().map(|s| s.index), Some(2), "the group being edited still stands");
        assert!(
            c.slots.iter().all(|s| s.index != 3),
            "and the one that cannot be measured is simply not on the strip"
        );
        assert_eq!(c.unmeasured.len(), 1, "its reason is carried, not thrown");
        assert!(c.unmeasured[0].contains("broken"), "{:?}", c.unmeasured);
        assert!(c.tallest > 0.0, "the stage is still framed around something");

        // The far neighbour closes up rather than leaving a hole where the broken one was.
        let offsets: Vec<i32> = c.slots.iter().map(|s| s.offset).collect();
        assert!(offsets.contains(&2), "the group beyond it still gets a slot: {offsets:?}");
    }

    /// **The ordinary state right after `N`.** A group with nothing in it yet must still be clickable
    /// and visible, so it floors at the editor's own quantum rather than collapsing to a point.
    #[test]
    fn a_group_with_nothing_in_it_still_gets_a_slot_the_lattice_can_express() {
        let comps = vec![anchored("fresh", Vec::new())];
        let l = lib();
        assert_eq!(
            footprint(&comps[0], &comps, &l).unwrap_or_else(|e| panic!("{e}")),
            ((0.0, 0.0), (SNAP, SNAP))
        );
        assert_eq!(height_of(&comps[0], &comps, &l).unwrap_or_else(|e| panic!("{e}")), SNAP);
        let c = lay_out(&comps, &lib(), 0);
        assert_eq!(c.slots.len(), 1);
        let (origin, dir) = ray_through(bevy::prelude::Vec3::new(0.0, SNAP * 0.5, 0.0));
        assert_eq!(slot_at(&c, origin, dir), Some(0), "a fresh group has to be clickable");
    }

    /// No groups is an empty stage, not a panic.
    ///
    /// **And a selection past the end now stages nothing rather than quietly standing up the last
    /// group** — this test asserted the opposite until the clamp moved, so the change is recorded
    /// here rather than only in the commit.
    ///
    /// `lay_out` clamping for itself was the whole defect: it made this the only reader that saw a
    /// valid index, so with `selected = 99` over three compositions the stage showed `comps[2]`, the
    /// panel's `i == selected` marked nothing, and `toggle_arm`'s `.get(99)` said there was nothing to
    /// arm. Three readers, three answers, and the one that looked right was the picture. An empty
    /// stage for one frame is the honest rendering of an index that is out of range;
    /// `clamp_selection` then fixes the index itself, at the one place it can go stale.
    #[test]
    fn an_empty_set_lays_out_to_nothing_and_a_stale_selection_stages_nothing() {
        let empty = lay_out(&[], &lib(), 3);
        assert!(empty.slots.is_empty());
        assert_eq!(empty.extent, (0.0, 0.0));

        let comps = kit(3);
        let c = lay_out(&comps, &lib(), 99);
        assert_eq!(
            c.focal().map(|s| s.index),
            None,
            "an out-of-range selection is not silently retargeted at another group"
        );
    }

    /// **The bug a captured frame found.** Framing on the floor plan alone cut the tops off four
    /// 2.4 m tiles, because a group that stands up occupies screen the footprint says nothing about.
    #[test]
    fn the_framing_accounts_for_how_tall_the_groups_stand() {
        use super::SQUARE;
        let flat = framing_height((4.0, 4.0), 0.1, SQUARE);
        let tall = framing_height((4.0, 4.0), 6.0, SQUARE);
        assert!(tall > flat, "height has to widen the view: {tall} vs {flat}");
        // **The floor moved to the callers on 2026-08-20** and the reason is the mesh stage: a
        // 12 cm mug has to be framable at 12 cm, and a floor of `TILE_VIEW_HEIGHT` in here made
        // that impossible for every caller at once. Nothing frames tighter than nothing.
        assert_eq!(framing_height((0.0, 0.0), 0.0, SQUARE), 0.0);
        // A narrower hole than it is tall needs MORE height to fit the same spread sideways.
        assert!(
            framing_height((4.0, 4.0), 0.1, 0.5) > flat,
            "the horizontal fit has to answer to the shape of the hole, not to a square"
        );
        // … and a big enough strip outruns the rig, which is the condition `restage_group` reports.
        assert!(
            framing_height((80.0, 50.0), 2.4, SQUARE) > crate::view::MAX_ZOOM,
            "a strip this size is exactly what the crop report is for"
        );
    }

    /// The extent is symmetric about the focal group, because the camera is pinned there — a strip
    /// with one short wing must still be framed without shifting the thing being edited.
    #[test]
    fn the_extent_is_measured_from_the_focal_group_not_from_the_strips_own_middle() {
        let comps = kit(6);
        let c = lay_out(&comps, &lib(), 0);
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
