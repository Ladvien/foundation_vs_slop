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
use serde_json::Value;

use crate::build::Build;
use crate::keys::{self, Live};
use crate::project::{OpenMap, Project};

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

        // **Every checkpoint takes `In<Value>`, whether or not it reads it.** One shape, so a step
        // can always state what it means -- see the note on `Step::with` for what vague conditions
        // cost. A condition that ignores its arguments is a condition with nothing to vary, not a
        // second kind of checkpoint.
        let arg_u64 = |args: &Value, key: &str, default: u64| -> u64 {
            args.get(key).and_then(Value::as_u64).unwrap_or(default)
        };

        let tiles_tab = app.register_system(|_: In<Value>, live: Res<Live>| {
            live.0 == keys::Context::Tiles
        });
        let map_tab = app.register_system(|_: In<Value>, live: Res<Live>| {
            live.0 == keys::Context::Map
        });
        let tile_open = app.register_system(|_: In<Value>, build: Res<Build>| build.open.is_some());
        // Not `Build::placing` alone and not `focused` alone: each was tried and each broke the
        // opposite end of the Tiles tab. `docs/tiles_tab_contract.md` records both failures.
        let placing = app.register_system(|_: In<Value>, build: Res<Build>| {
            build.placing && crate::build::focused(&build)
        });
        let one_cell = app.register_system(|_: In<Value>, build: Res<Build>| {
            build.open.as_ref().is_some_and(|c| match c.envelope {
                emerge_core::composition::Envelope::Bounded { size } => {
                    crate::build::is_one_cell(size)
                }
                // An `Anchored` composition claims no tile, so "is it one cell" has no answer for
                // it. Answering false is right: the step that asks this wants a solver prototype.
                emerge_core::composition::Envelope::Anchored => false,
            })
        });

        // **The kit's tile count, and it is MONOTONIC -- which is the whole point.**
        //
        // `the tile is saved` was true whenever any already-saved tile happened to be open, so a
        // step passed for a tile that was never made and the transcript recorded 1/1 for work that
        // did not happen. Counting what is committed cannot be re-satisfied by revisiting old work:
        // a step that wants the third tile asks for `{"n": 3}` and only a third tile answers it.
        //
        // Prefer this over `the open tile is saved` in any script that authors more than one thing.
        let kit_tiles = app.register_system(
            move |args: In<Value>, project: Option<Res<Project>>| {
                project.is_some_and(|p| {
                    p.compositions.compositions.len() as u64 >= arg_u64(&args.0, "n", 1)
                })
            },
        );

        // **On disk, not in hand.** `build.open` is the tile being assembled and survives a failed
        // save untouched, so a checkpoint reading it would pass while nothing had been written.
        // Weak on its own -- see `kit_tiles` -- and kept for single-tile scripts, where there is no
        // older tile to be confused with.
        let saved = app.register_system(|_: In<Value>, build: Res<Build>, project: Option<Res<Project>>| {
            let Some(project) = project else { return false };
            build.open.as_ref().is_some_and(|c| {
                project.compositions.compositions.iter().any(|s| s.id == c.id && s.members == c.members)
            })
        });
        let unsaved = app.register_system(|_: In<Value>, build: Res<Build>, project: Option<Res<Project>>| {
            let Some(project) = project else { return false };
            build.open.as_ref().is_some_and(|c| {
                !project
                    .compositions
                    .compositions
                    .iter()
                    .any(|s| s.id == c.id && s.members == c.members)
            })
        });

        // **What is actually in the tile**, so a step claiming "floor plus two walls" can say so.
        // `{"ids": ["site/wall"], "n": 2}` means at least two members whose descriptor is that id;
        // omit `ids` to count members of any kind.
        let tile_contains = app.register_system(
            move |args: In<Value>, build: Res<Build>| {
                let want = arg_u64(&args.0, "n", 1) as usize;
                let ids: Vec<&str> = args
                    .0
                    .get("ids")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                build.open.as_ref().is_some_and(|c| {
                    c.members
                        .iter()
                        .filter(|m| match &m.body {
                            emerge_core::composition::Body::Descriptor { id, .. } => {
                                ids.is_empty() || ids.iter().any(|want| want == id)
                            }
                            _ => ids.is_empty(),
                        })
                        .count()
                        >= want
                })
            },
        );

        // **How many distinct quarter-turns the members sit at.** A corner is two walls that are NOT
        // parallel, and nothing else in this vocabulary can tell that from two walls side by side.
        let tile_turns = app.register_system(
            move |args: In<Value>, build: Res<Build>| {
                let want = arg_u64(&args.0, "n", 2) as usize;
                build.open.as_ref().is_some_and(|c| {
                    // **`Member::yaw` is DEGREES.** `build::turn` writes
                    // `(m.yaw + 90.0).rem_euclid(360.0)`, and dividing that by `FRAC_PI_2` as though
                    // it were radians gives 57 for a quarter turn -- which happens to be non-zero,
                    // so a two-wall corner would still have "passed", and 270 degrees would have
                    // collided with 0. A condition that is right by accident on the case you tried
                    // is the same defect as one that is simply wrong.
                    let mut quarters: Vec<i32> = c
                        .members
                        .iter()
                        .map(|m| ((m.yaw / 90.0).round() as i32).rem_euclid(4))
                        .collect();
                    quarters.sort_unstable();
                    quarters.dedup();
                    quarters.len() >= want
                })
            },
        );

        let edges_staged = app.register_system(|_: In<Value>, derived: Res<crate::tiles::DerivedEdges>| {
            derived.0.is_some()
        });
        let proposal_on_the_map = app.register_system(|_: In<Value>, proposal: Res<crate::editor::Proposal>| {
            proposal.0.is_some()
        });

        // The two fixed-count conditions the first script named, kept as their own systems so that
        // script still reads the way it was written. `the tile contains` supersedes both for
        // anything new: it says the number rather than spelling it into a name.
        let has_a_piece = app.register_system(|_: In<Value>, build: Res<Build>| members(&build) >= 1);
        let has_two_pieces = app.register_system(|_: In<Value>, build: Res<Build>| members(&build) >= 2);

        let meshes_tab = app.register_system(|_: In<Value>, live: Res<Live>| {
            live.0 == keys::Context::Meshes
        });
        let compose_tab = app.register_system(|_: In<Value>, live: Res<Live>| {
            live.0 == keys::Context::Compose
        });
        // **By id, so a script can send the author to a named mesh** and know they arrived — the
        // selection is what `B` derives for and what `Enter` imports.
        // **Does the focused piece hold this token, on this axis?**
        //
        // The one objective question in a walk of the tag block, and the reason it is worth a
        // checkpoint rather than a judgement call: the block's whole point is that a keystroke can
        // now do what only a click could, so *"did the token actually land"* has a true answer and
        // an author should not have to be the one who checks it. Takes `{"axis": "look", "token":
        // "wood"}`; an unknown axis answers `false` rather than panicking, because a script is a
        // file somebody typed.
        let carries_token = app.register_system(
            |args: In<Value>,
             state: Res<crate::tiles::ImportState>,
             project: Option<Res<Project>>| {
                let (Some(project), Some(target)) = (project, state.target()) else {
                    return false;
                };
                let Some(d) = state.placed_at_target(&target, &project) else {
                    return false;
                };
                let Some(token) = args.0.get("token").and_then(Value::as_str) else {
                    return false;
                };
                let held: &[String] = match args.0.get("axis").and_then(Value::as_str) {
                    Some("kind") => &d.kind,
                    Some("effects") => &d.effects,
                    Some("look") => &d.look,
                    Some("surfaces") => &d.offers.surfaces,
                    _ => return false,
                };
                held.iter().any(|t| t == token)
            },
        );
        let selected_mesh = app.register_system(
            |args: In<Value>, state: Res<crate::tiles::ImportState>| {
                args.0
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|want| state.selected_library_id.as_deref() == Some(want))
            },
        );
        // The Map's count, on `kit_tiles`' argument: monotonic, so revisiting old work cannot
        // re-satisfy a step that asks for new rows.
        //
        // **`Option<Res<OpenMap>>` on every map checkpoint, because only one door has a map.**
        // `args::Opened::insert_into` removes `OpenMap` for the Kit and Rigs doors, and in Bevy 0.19
        // a missing `Res<T>` fails the system rather than skipping it — so watching a script that
        // crosses from the Tiles tab into the map (`build_a_room`, `room_from_nothing`,
        // `branch_verbs` all do) killed the exercise at that step instead of simply not passing it
        // yet. No map is "not there yet", which is exactly what a checkpoint answers `false` to.
        let map_placements = app.register_system(move |args: In<Value>, open: Option<Res<OpenMap>>| {
            open.is_some_and(|o| o.map.placements.len() as u64 >= arg_u64(&args.0, "n", 1))
        });
        // **Stamps, which `map_placements` deliberately cannot see.** A stamped tile is a reference
        // in `Map::stamps`, never rows in `Map::placements` — that is the whole of "editing a
        // composition changes every map that stamped it" — so a script that walks an author through
        // building a room *out of tiles* has no observable state in the placement count, and every
        // step of it would have to be a judgement call. Counted separately rather than folded into
        // `map_placements`, because "I placed five pieces" and "I placed five tiles" are different
        // claims and a script that meant one should not pass on the other.
        let map_tiles = app.register_system(move |args: In<Value>, open: Option<Res<OpenMap>>| {
            open.is_some_and(|o| o.map.stamps.len() as u64 >= arg_u64(&args.0, "n", 1))
        });
        // **Kept, not merely gone.** A discarded proposal also answers `is_none`, so the keep half
        // of the door is the pair: no proposal waiting AND the map carrying at least `n` solver
        // rows — stamps for a composed grammar, placements for a learned one, so both are counted.
        let proposal_kept = app.register_system(
            move |args: In<Value>,
                  proposal: Res<crate::editor::Proposal>,
                  open: Option<Res<OpenMap>>| {
                proposal.0.is_none()
                    && open.is_some_and(|o| {
                        (o.map.stamps.len() + o.map.placements.len()) as u64
                            >= arg_u64(&args.0, "n", 1)
                    })
            },
        );
        let map_saved =
            app.register_system(|_: In<Value>, open: Option<Res<OpenMap>>| {
                open.is_some_and(|o| !o.dirty)
            });
        // **On the measured lattice, which is what accepting a derivation writes** — so this is
        // "the tokens landed", never "a proposal exists"; `edges are staged` is that one.
        let mesh_declares_edge = app.register_system(
            |args: In<Value>,
             state: Res<crate::tiles::ImportState>,
             project: Option<Res<Project>>| {
                let Some(project) = project else { return false };
                let id = args
                    .0
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| state.selected_library_id.clone());
                id.is_some_and(|id| {
                    project
                        .measured
                        .descriptors
                        .iter()
                        .find(|d| d.id == id)
                        .and_then(|d| d.subgrid.as_ref())
                        .is_some_and(|g| g.cells.iter().any(|c| c.edge.is_some()))
                })
            },
        );

        let mut checkpoints = app.world_mut().resource_mut::<Checkpoints>();
        checkpoints.register("the Tiles tab is open", tiles_tab);
        checkpoints.register("the Map tab is open", map_tab);
        checkpoints.register("a tile is open", tile_open);
        checkpoints.register("the tile has a piece in it", has_a_piece);
        checkpoints.register("the tile has two pieces", has_two_pieces);
        checkpoints.register("a piece is in hand", placing);
        checkpoints.register("the tile is one cell", one_cell);
        checkpoints.register("the tile is saved", saved);
        checkpoints.register("the kit has tiles", kit_tiles);
        checkpoints.register("the tile contains", tile_contains);
        checkpoints.register("the tile has turns", tile_turns);
        checkpoints.register("the open tile is unsaved", unsaved);
        checkpoints.register("edges are staged", edges_staged);
        checkpoints.register("a proposal is on the map", proposal_on_the_map);
        checkpoints.register("the Meshes tab is open", meshes_tab);
        checkpoints.register("the selected mesh is", selected_mesh);
        checkpoints.register("the map has placements", map_placements);
        checkpoints.register("the map has tiles on it", map_tiles);
        checkpoints.register("the Compose tab is open", compose_tab);
        checkpoints.register("the proposal was kept", proposal_kept);
        checkpoints.register("the map is saved", map_saved);
        checkpoints.register("the mesh declares an edge", mesh_declares_edge);
        checkpoints.register("the piece carries", carries_token);
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

/// **Exercises waiting on a kit that is not bound yet** — `guides/pending/`.
///
/// A card names the pieces it sends an author to select, so a guide is only walkable while its kit
/// ships. Six of them name `site/*`, and `assets/emerge/kits.ron` says in its own note that that kit
/// *"was cleared … and is being re-authored"* — so they strand at the step that names a piece.
///
/// Parking is a **move, not a branch**: nothing here reads this directory, and the two tests that
/// scan [`GUIDES_DIR`] take only files whose extension is `json`, which a directory has none of. So
/// a parked guide simply stops being a shipped exercise. The drive tests still walk these by name
/// against fixtures, which is what stops them rotting while they wait — and moving one back is a
/// `git mv` once `site` is bound again.
pub const PENDING_GUIDES_DIR: &str = "guides/pending";
