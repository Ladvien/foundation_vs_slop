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
    for e in scene_roots(world) {
        if let Ok(entity) = world.get_entity_mut(e) {
            entity.despawn();
        }
    }
    crate::args::Opened::remove_from(world);
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
        )>()
        .iter(world)
        .collect()
}
