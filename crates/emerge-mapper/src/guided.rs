//! **The editor's own words for the guide channel** — the conditions it is willing to have watched.
//!
//! `bevy_debugger_bevy::guide` posts a script and renders one step; it cannot know what a tile is and
//! must not learn, because it is an engine-level plugin with five dependencies and no reach into any
//! particular application. So the vocabulary lives here: one-shot systems that answer `bool`,
//! registered under names an author would recognise, and a script names one of them.
//!
//! # Why this is in the shared plugin list and the transport is not
//!
//! [`crate::harness::add_debugger_plugins`] is deliberately outside [`crate::harness::add_editor_plugins`]
//! because it **binds a port**, and a test process builds several `App`s. Nothing here binds
//! anything. The harness is in fact exactly where these want to be: `every_checkpoint_a_shipped_guide_names_is_registered`
//! boots the editor headless and asserts that every condition named by a file under `guides/` exists,
//! which is the one failure a running editor cannot warn about in time — a script naming a checkpoint
//! nobody registered strands the author mid-exercise, and by then they are at the keyboard.
//!
//! # What is a checkpoint and what is not
//!
//! A checkpoint answers *"has the state the step asked for arrived?"* — never *"did the author press
//! the right key?"*. The distinction is the whole value: a script that watched keystrokes would be
//! testing whether the author can follow instructions, and the exercise exists to test the **editor**.
//! Khanum & Trivedi's finding on think-aloud sessions is the same worry from the other side: their
//! participants felt *"it was they who were tested"*.
//!
//! So a step whose result only a person can judge — *does this wall sit flush?* — has **no
//! checkpoint**, and that is a supported state rather than a gap. It answers
//! `waiting_on_a_person: true` on the watch stream and waits for an explicit `skip`.

use bevy::prelude::*;
use bevy_debugger_bevy::Checkpoints;

use crate::build::Build;
use crate::keys::{self, Live};
use crate::project::Project;

/// Registers the editor's checkpoint vocabulary.
///
/// Adding a name here is cheap and deleting one is not: a shipped guide may name it, which is what
/// `every_checkpoint_a_shipped_guide_names_is_registered_and_runs` is for. Registering **more** names
/// than any shipped script uses is fine and expected — `edges are staged` and `a proposal is on the
/// map` are conditions the Meshes and Map tabs offer, waiting for the scripts that will want them.
///
/// **The overlay is not here**, and must not be: `DebuggerPlugin` adds `GuideOverlayPlugin`, and Bevy
/// rejects a duplicate plugin by name, so adding it here as well would panic the editor's binary the
/// moment both were present. The consequence is that `build_headless` has the vocabulary but no card
/// on screen — which is right, since the harness has no window either. That the overlay builds its
/// text tree is pinned where it belongs, in `bevy_debugger_bevy`'s own `tests/guide_steps.rs`.
pub struct GuidePlugin;

impl Plugin for GuidePlugin {
    fn build(&self, app: &mut App) {
        // `DebuggerPlugin` inits both of these, and it is **not** in `build_headless` — it binds a
        // port. Same argument as `DebugCursor` in `harness.rs`: in Bevy 0.19 a missing `Res<T>`
        // panics its system rather than skipping it, so a resource the editor's own code reads has to
        // exist whether or not the transport does.
        app.init_resource::<Checkpoints>()
            .init_resource::<bevy_debugger_bevy::Guide>()
            // **Below the tab bar, because a capture showed it sitting on top of one.** The plugin's
            // default is 12 px, which here is exactly where MAP / MESHES / TILES live: the first
            // devshot came back with ANIM reading through the card. `insert_resource` rather than
            // `init_resource`, so this wins whichever order the two plugins are added in --
            // `DebuggerPlugin` only inits it, and init is insert-if-absent.
            .insert_resource(bevy_debugger_bevy::GuidePlacement {
                top: 58.0,
                ..default()
            });

        let tiles_tab = app.register_system(|live: Res<Live>| live.0 == keys::Context::Tiles);
        let map_tab = app.register_system(|live: Res<Live>| live.0 == keys::Context::Map);
        let tile_open = app.register_system(|build: Res<Build>| build.open.is_some());
        let has_a_piece = app.register_system(|build: Res<Build>| members(&build) >= 1);
        let has_two_pieces = app.register_system(|build: Res<Build>| members(&build) >= 2);
        // Not `Build::placing` alone and not `focused` alone: each was tried and each broke the
        // opposite end of the Tiles tab. `docs/tiles_tab_contract.md` records both failures.
        let placing = app.register_system(|build: Res<Build>| {
            build.placing && crate::build::focused(&build)
        });
        let one_cell = app.register_system(|build: Res<Build>| {
            build.open.as_ref().is_some_and(|c| match c.envelope {
                emerge_core::composition::Envelope::Bounded { size } => {
                    crate::build::is_one_cell(size)
                }
                // An `Anchored` composition claims no tile, so "is it one cell" has no answer for it.
                // Answering false is right: the step that asks this wants a solver prototype.
                emerge_core::composition::Envelope::Anchored => false,
            })
        });
        // **On disk, not in hand.** `build.open` is the tile being assembled and survives a failed
        // save untouched, so a checkpoint reading it would pass while nothing had been written.
        let saved = app.register_system(|build: Res<Build>, project: Res<Project>| {
            build.open.as_ref().is_some_and(|c| {
                project.compositions.compositions.iter().any(|s| s.id == c.id && s.members == c.members)
            })
        });
        let edges_staged =
            app.register_system(|derived: Res<crate::tiles::DerivedEdges>| derived.0.is_some());
        let proposal_on_the_map =
            app.register_system(|proposal: Res<crate::editor::Proposal>| proposal.0.is_some());

        let mut checkpoints = app.world_mut().resource_mut::<Checkpoints>();
        checkpoints.register("the Tiles tab is open", tiles_tab);
        checkpoints.register("the Map tab is open", map_tab);
        checkpoints.register("a tile is open", tile_open);
        checkpoints.register("the tile has a piece in it", has_a_piece);
        checkpoints.register("the tile has two pieces", has_two_pieces);
        checkpoints.register("a piece is in hand", placing);
        checkpoints.register("the tile is one cell", one_cell);
        checkpoints.register("the tile is saved", saved);
        checkpoints.register("edges are staged", edges_staged);
        checkpoints.register("a proposal is on the map", proposal_on_the_map);
    }
}

fn members(build: &Build) -> usize {
    build.open.as_ref().map_or(0, |c| c.members.len())
}

/// Where a shipped script lives, relative to this crate's root.
///
/// **The script is a file, not a constant.** An agent posts it over BRP, which is the one path — an
/// editor that also shipped a "start the tour" key would be a second way to load a script, and the
/// two would drift the first time one of them was edited. Keeping it on disk means the test below can
/// read exactly what an agent would send.
pub const GUIDES_DIR: &str = "guides";
