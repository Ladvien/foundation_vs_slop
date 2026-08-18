//! **Which half of the editor is live** — the menu, or a door.
//!
//! # One application, two screens
//!
//! These were two processes. The chooser built its own `App`, ran to completion, launched the editor
//! as a child, and a supervisor spawned a fresh menu per lap — because **winit builds at most one
//! event loop per process**, so a second `App::run` in one process dies on `RecreationAttempt`.
//!
//! That shape was chosen for a second reason which is the one that actually mattered: `Project` is
//! inserted before the editor's plugins, and in Bevy 0.19 a **missing `Res<T>` panics its system**
//! rather than skipping it. Around a hundred parameter positions take `Res<Project>`, so a menu
//! living inside the editor's `App` meant gating every one of them — *"Gating is feasible… it is the
//! **cost** that is the argument"* (`chooser.rs`).
//!
//! Asked for at the keyboard, 2026-08-16: *"can we not open a whole another editing window? I'd like
//! to keep the same bevy application running across whether it's the UI or the editor."* So the cost
//! gets paid. This state is what pays it: every editor system runs `in_state(Screen::Editor)`, so in
//! [`Screen::Menu`] none of them run and none of them fetch a `Project` that is not there.
//!
//! # A door change is a full teardown
//!
//! Chosen at the keyboard on the same day, over keeping the project loaded between doors: leaving a
//! door despawns everything it made and drops the project; entering the next one loads it fresh.
//!
//! That is exactly what the two processes did, minus the process. The alternative — keep the library,
//! vocabulary and thumbnails warm and swap only the map — is faster and is the whole point of one
//! window, but it makes every entity, resource and history stack I forget to clear a stale value
//! that survives into the next door. A reload cannot be wrong; a partial teardown can be, silently,
//! and the bug lands weeks later looking like something else.
//!
//! # Doors are still reached through the menu
//!
//! No direct Kit↔Map key. It became affordable once the process boundary went — the design doc had
//! rejected it when it meant a relaunch — and it was declined anyway: with full teardown the saving
//! is the menu round-trip rather than the reload, and a second way to do one thing is the pattern
//! this crate spends its refusals on.

use bevy::prelude::*;

/// The menu, or a door.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Screen {
    /// The chooser: kits, maps, and what each one is.
    #[default]
    Menu,
    /// A door, with a [`crate::project::Project`] loaded and a [`crate::tiles::Door`] chosen.
    Editor,
}

/// **The plugin that owns the two transitions.**
///
/// `OnExit(Menu)` opens the project, because a state transition runs every `OnExit` before any
/// `OnEnter` — so by the time the editor's own spawns run, `Project` is already in the World and the
/// hundred systems that take it are not looking at an empty slot.
///
/// `OnExit(Editor)` puts everything back: entities despawned, resources dropped. See the module note
/// on why that is a full reload rather than a swap.
pub struct ScreenPlugin;

impl Plugin for ScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<Screen>()
            .add_systems(OnExit(Screen::Menu), open_the_door)
            .add_systems(OnExit(Screen::Editor), close_the_door);
    }
}

/// Put what the menu opened into the World. An exclusive system, so the resources exist the instant
/// this returns rather than whenever a command queue is next flushed — `OnEnter(Editor)` is the very
/// next schedule and every system in it assumes a loaded project.
///
/// **This cannot fail, and that is the point.** It used to call `args::open` here and, on an error,
/// write the reason onto the chooser and set `NextState(Menu)`. That never worked: a
/// `StateTransition` runs `ExitSchedules` then `EnterSchedules` in one pass, so the `NextState` was
/// not read until the following pass and `OnEnter(Editor)` ran regardless — against a world with no
/// `Project` and no `Door`, which panics rather than returning. The open moved to
/// `chooser::drive_chooser` and to `main`, both of which can refuse in place; what arrives here is a
/// door already open. See [`crate::chooser::Chosen`].
fn open_the_door(world: &mut World) {
    let Some(chosen) = world.remove_resource::<crate::chooser::Chosen>() else {
        // Reached only when something set the state directly. Loud, because the alternative is an
        // editor with no project and a hundred systems finding out one at a time.
        error!("entering a door with nothing chosen — staying on the menu");
        if let Some(mut next) = world.get_resource_mut::<NextState<Screen>>() {
            next.set(Screen::Menu);
        }
        return;
    };
    chosen.0.insert_into(world);
}

/// Everything the door made, gone.
///
/// The entity sweep is by root — everything the editor spawns hangs off one, and a teardown naming
/// each marker is a list that goes stale the first time somebody adds a camera. The resources are
/// named, because there is no such reachability rule for them and a stale `Project` is precisely the
/// thing that must not survive into the next door.
fn close_the_door(world: &mut World) {
    despawn_scene(world);
    crate::args::Opened::remove_from(world);
}

/// **Despawn every root a screen could have spawned.** The one implementation, called by both
/// screens' teardowns.
///
/// It was two: this and `chooser::tear_down_menu`, which spelled the same reachability rule as its
/// own query — and the copies drifted the first time the rule gained a clause. `crate::surface` was
/// excluded from [`scene_roots`] and swept away by the other one anyway, which left the editor with
/// no camera for its interface, seventy-six UI nodes computing a target of nothing, and a window
/// that still drew the world because the map camera holds the image handle directly. Nothing logged.
///
/// So the rule lives in [`scene_roots`] and nobody writes it a second time.
pub(crate) fn despawn_scene(world: &mut World) {
    for e in scene_roots(world) {
        if let Ok(entity) = world.get_entity_mut(e) {
            entity.despawn();
        }
    }
}

/// **Every root a screen could have spawned** — and nothing else.
///
/// `Or<(With<Transform>, With<Node>)>` rather than "everything without a parent", and the difference
/// is a crash: the broad version despawned Bevy's own bookkeeping entities — observers and the like —
/// and the next command touching one panicked with *"the entity with ID 0v0 is invalid; its index
/// now has generation 1"*. A screen only ever spawns things in the world or things on the screen, so
/// those two components are the whole surface, and anything the engine keeps for itself has neither.
///
/// Windows and monitors are excluded because they outlive both screens; that is the one thing here
/// that is genuinely shared.
pub(crate) fn scene_roots(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<Entity, (
            Or<(With<Transform>, With<Node>)>,
            Without<ChildOf>,
            Without<Window>,
            Without<bevy::window::Monitor>,
            // **And the surface, for the same reason.** It is not something a screen made — it is
            // how this application draws at all, spawned once at `Startup` and shared by both
            // screens. Sweeping it away on a door change left the next screen with no camera to
            // render into and no clue why.
            Without<crate::surface::SurfaceRig>,
        )>()
        .iter(world)
        .collect()
}

/// **What a door change does to a resource** — the answer, for every one of them, in one place.
///
/// # Why this exists
///
/// `screen.rs` used to claim, a few lines up, that *"leaving a door despawns everything it made and
/// drops the project"*, and defend it: *"A reload cannot be wrong; a partial teardown can be,
/// silently, and the bug lands weeks later looking like something else."*
///
/// **That is not what the code does**, measured on 2026-08-17 and unchanged since: entities are
/// swept by reachability and four resources are named, and the other **fifty-six are not touched at
/// all**. Nothing resets them on entry either — every `OnEnter(Editor)` system is a spawn. So the
/// door change is *already* the partial teardown the comment is spent avoiding, unnamed and
/// unchecked, and the bug class it warns about is already open: edit a tile in kit A, leave, open
/// kit B, and A's undo stack is there to be replayed into B.
///
/// So the list is the deliverable. **This changes no behaviour** — it is `docs/2026-08-17-one-application.md`
/// §6 step 1, the step that makes the rest safe and that stands alone if the rest is dropped. What
/// it buys today is that a new resource is a deliberate answer to "what happens to this when the
/// door changes", asked when it is added rather than three weeks later when its stale value
/// surfaces as something else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ownership {
    /// Derived from files on disk. Kept while the kit is unchanged, reloaded when it is not.
    Project,
    /// This door's own working state — selections, drags, edit buffers, undo stacks. **Reset.**
    Door,
    /// True for as long as the application runs: caches, generations, input, the surface. Kept.
    Session,
}

/// **Every resource the editor's own plugins register, and what a door does to it.**
///
/// Keyed by the full type path, because two crates may name a type alike and the ratchet compares
/// against what the world reports. Unsure cases are classified [`Ownership::Door`] on purpose: it is
/// the class that matches what a full teardown would do, so a wrong guess here is conservative
/// rather than a stale value nobody looks for.
pub const OWNERSHIP: &[(&str, Ownership)] = &[
    // ── What was opened, and what was read off disk for it ──────────────────────────────────────
    ("emerge_mapper::project::Project", Ownership::Project),
    ("emerge_mapper::project::OpenMap", Ownership::Project),
    ("emerge_mapper::tiles::Door", Ownership::Project),
    ("emerge_mapper::tiles::Mode", Ownership::Project),
    // Derived from the kit's meshes: a different kit means different thumbnails.
    ("emerge_mapper::thumbs::Thumbnails", Ownership::Project),
    ("emerge_mapper::thumbs::ThumbGeneration", Ownership::Project),
    ("emerge_mapper::labels::Suggestions", Ownership::Project),
    ("emerge_mapper::anim_watch::BenchReports", Ownership::Project),
    ("emerge_mapper::anim_watch::RigWatch", Ownership::Project),

    // ── The door's own working state ────────────────────────────────────────────────────────────
    // The undo stack is the one this list was written for: `docs/2026-08-17-one-application.md` §1
    // names it as the value that survives into the next kit today.
    ("emerge_mapper::build::TileHistory", Ownership::Door),
    ("emerge_mapper::build::Build", Ownership::Door),
    ("emerge_mapper::editor::EditorState", Ownership::Door),
    ("emerge_mapper::editor::CloneDrag", Ownership::Door),
    ("emerge_mapper::editor::MoveDrag", Ownership::Door),
    ("emerge_mapper::editor::PlaceDrag", Ownership::Door),
    ("emerge_mapper::editor::RemovalDrag", Ownership::Door),
    ("emerge_mapper::editor::FineAnchor", Ownership::Door),
    ("emerge_mapper::editor::Proposal", Ownership::Door),
    ("emerge_mapper::editor::Rung", Ownership::Door),
    ("emerge_mapper::editor::SizeEdit", Ownership::Door),
    ("emerge_mapper::editor::StampPicture", Ownership::Door),
    ("emerge_mapper::editor::TargetLock", Ownership::Door),
    ("emerge_mapper::editor::UnderCursor", Ownership::Door),
    ("emerge_mapper::editor::EdgeFaults", Ownership::Door),
    ("emerge_mapper::tiles::ImportState", Ownership::Door),
    ("emerge_mapper::tiles::CellEdit", Ownership::Door),
    ("emerge_mapper::tiles::DemoteArm", Ownership::Door),
    ("emerge_mapper::tiles::DerivedEdges", Ownership::Door),
    ("emerge_mapper::tiles::HeightEdit", Ownership::Door),
    ("emerge_mapper::tiles::LatticePick", Ownership::Door),
    ("emerge_mapper::tiles::MapView", Ownership::Door),
    ("emerge_mapper::tiles::NoteEdit", Ownership::Door),
    ("emerge_mapper::tiles::ScaleEdit", Ownership::Door),
    ("emerge_mapper::tiles::StagedLift", Ownership::Door),
    ("emerge_mapper::compose::ComposeState", Ownership::Door),
    ("emerge_mapper::compose::Budget", Ownership::Door),
    ("emerge_mapper::compose::StagedCarousel", Ownership::Door),
    ("emerge_mapper::anim_tab::BenchState", Ownership::Door),
    ("emerge_mapper::anim_tab::AdoptExclude", Ownership::Door),
    ("emerge_mapper::anim_stage::BenchAb", Ownership::Door),
    ("emerge_mapper::anim_stage::BenchCamera", Ownership::Door),
    ("emerge_mapper::anim_stage::BenchScrub", Ownership::Door),
    ("emerge_mapper::anim_watch::MeasureQueue", Ownership::Door),
    ("emerge_mapper::labels::LabelQueue", Ownership::Door),
    ("emerge_mapper::filter::Filters", Ownership::Door),
    ("emerge_mapper::notice::Showing", Ownership::Door),
    // Holds entity ids from the frame this screen spawned, so it is meaningless the moment those
    // are despawned — and `chrome::spawn_frame` replaces it on every entry.
    ("emerge_mapper::chrome::Frame", Ownership::Door),
    ("emerge_mapper::chrome::ShowingFor", Ownership::Door),
    ("emerge_mapper::view::Rig", Ownership::Door),

    // ── True for as long as the application runs ────────────────────────────────────────────────
    // The surface is how this application DRAWS — see `crate::surface`, and `scene_roots`, which
    // excludes its entities for the same reason.
    ("emerge_mapper::surface::Surface", Ownership::Session),
    ("emerge_mapper::keys::Live", Ownership::Session),
    ("emerge_mapper::keys::Repeat", Ownership::Session),
    ("emerge_mapper::view::Pointer", Ownership::Session),
    // Persisted to disk under `target/`, so that STALE is truthful at startup rather than after the
    // bench's first audit.
    ("emerge_mapper::anim_cache::BenchCache", Ownership::Session),
    ("emerge_mapper::anim_watch::BenchGeneration", Ownership::Session),
    ("emerge_mapper::anim_plots::BenchPlots", Ownership::Session),
    ("emerge_mapper::anim_stage::GhostMaterial", Ownership::Session),
    ("emerge_mapper::label_booth::ShotRig", Ownership::Session),
    ("emerge_mapper::labels::LabelGeneration", Ownership::Session),
    // In-flight vision-model work. Dropping it mid-request would strand the task, not cancel it.
    ("emerge_mapper::labels::LabelTasks", Ownership::Session),
];

/// What a door change does to this resource, or `None` if nobody has said.
pub fn ownership(type_path: &str) -> Option<Ownership> {
    OWNERSHIP
        .iter()
        .find(|(name, _)| *name == type_path)
        .map(|(_, class)| *class)
}
