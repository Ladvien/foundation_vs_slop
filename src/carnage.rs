//! # Carnage — the game's half of `bevy_carnage`
//!
//! The fracture itself — the triangle soup, the recursive plane cuts, the watertight caps, the
//! bake-once-per-source cache — lives in `crates/bevy_carnage`, which knows nothing about this game.
//! What stays here is the part that is *this game's content*: VALKYRIE carries her rifle inside the
//! body scene, and the bake only runs while a run is active.
//!
//! **The crate's determinism record moved with the code.** `seed_from_path`'s writeup (why a fracture
//! seed may never come from an `AssetId`), the canonical vertex-soup sort, and the streaming gate that
//! treats an empty detached part as "not yet" rather than "absent" are all in
//! `crates/bevy_carnage/src/bake.rs`, with `crates/bevy_carnage/CLAUDE.md` carrying the summary. That
//! is FVS-N-8 and G0d; read them there before touching the bake.
//!
//! Spawning is `gore::spawn_fragments` — avian bodies, box colliders, the `GibRing` slot, the
//! `Carryable` weight the crabs forage on. The crate never learns any of it.

use bevy::prelude::*;

use crate::squad::{FigurineModel, GunModel};

/// The baked fragment set, under the name every call site in this game already uses.
///
/// `CarnageCache` is `bevy_carnage::FractureCache`, and `GunChunk` is its `DetachedChunk` — the crate
/// must not say "gun", because a fracture library is useless to a project that has none.
pub use bevy_carnage::{DetachedChunk as GunChunk, Fragment, FractureCache as CarnageCache};

/// Registers the fracture cache and its one-shot bake, and gives them this game's schedule.
///
/// **Not a re-export of the crate's plugin**, because two things have to be added on top: the
/// authored [`FractureSettings`](bevy_carnage::FractureSettings) from `config.ron`, and the run gate.
/// `lib.rs` and `sim_harness.rs` both add `carnage::CarnagePlugin` and neither needed to change.
pub struct CarnagePlugin;

impl Plugin for CarnagePlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback. The `fracture:` slice comes from the unified
        // `assets/config/config.ron`, loaded + validated once by `ConfigPlugin` (registered first),
        // exactly as `GorePlugin` reads its own slice.
        //
        // Inserted BEFORE the crate's plugin on purpose: `CarnagePlugin` there `init_resource`s
        // `FractureSettings`, which does nothing when the resource already exists. So the authored
        // values win and the crate's `Default` only ever covers a standalone user. One owner, one
        // value — never a merge.
        let settings = app.world().resource::<crate::config::GameConfig>().fracture.clone();
        app.insert_resource(settings)
            .add_plugins(bevy_carnage::CarnagePlugin)
            // The crate deliberately configures no run condition — the caller owns the schedule.
            .configure_sets(
                Update,
                bevy_carnage::CarnageSystems.run_if(in_state(crate::session::RunState::Active)),
            )
            // Tag the in-scene rifle before the bake reads the scene, so the gun chunk is pruned out of
            // the body soup and the bake gate sees a non-empty detached part (see `tag_valkyrie_rifle`).
            // This was a `.chain()` before the extraction; the crate now exposes a set, so the edge can
            // be stated directly instead of relying on tuple order.
            .add_systems(
                Update,
                tag_valkyrie_rifle
                    .before(bevy_carnage::CarnageSystems)
                    .run_if(in_state(crate::session::RunState::Active)),
            );
    }
}

/// Marks a `FigurineModel` child whose in-scene rifle has already been tagged `GunModel`, so
/// [`tag_valkyrie_rifle`] runs once per unit. Lives on the cosmetic figurine child (never the `Unit`), so
/// it can't split the hashed squad archetype — same discipline as `squad::Recolored`.
#[derive(Component)]
struct RifleTagged;

/// VALKYRIE carries her rifle *inside* the body scene (a rigid mesh on the `spine_03` bone), not as the
/// separate held-item child the old greybox used. Once the scene streams in, find that `rifle` sub-mesh
/// and tag it `GunModel` so the bake prunes it into the intact, self-materialed gun chunk exactly as it
/// did the old blaster — the rifle still flies off on death, and the crate's "empty detached part means
/// still streaming" gate stays satisfied. Runs `.before(CarnageSystems)` so the tag is in place the same
/// frame the scene's meshes finish loading.
///
/// **The `"rifle"` node name is a content contract, which is exactly why this system did not move into
/// the crate.** It is authored in `characters/valkyrie.glb` and documented in `docs/artist_guide.md`;
/// a fracture library has no business knowing it.
fn tag_valkyrie_rifle(
    mut commands: Commands,
    figurines: Query<Entity, (With<FigurineModel>, Without<RifleTagged>)>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    for figurine in &figurines {
        let mut stack: Vec<Entity> = match children.get(figurine) {
            Ok(c) => c.iter().collect(),
            Err(_) => continue, // scene not instantiated yet — retry next frame
        };
        let mut tagged = false;
        while let Some(e) = stack.pop() {
            if names.get(e).map(|n| n.as_str().contains("rifle")).unwrap_or(false) {
                // Tag the whole rifle node subtree as the gun chunk; don't descend past it.
                commands.entity(e).insert(GunModel);
                tagged = true;
                continue;
            }
            if let Ok(ch) = children.get(e) {
                stack.extend(ch.iter());
            }
        }
        if tagged {
            commands.entity(figurine).insert(RifleTagged);
        }
    }
}
