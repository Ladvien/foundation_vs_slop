//! **The rig manifest, read once, built the same way for every creature.**
//!
//! Six creatures each hand-wrote their clip table in Rust — `CLIP_*` indices and `GAIT_*` triples in
//! `squad.rs`, `from_clips([7, 6, 10, 3, 1, 2])` in `parasite.rs`, a `ClipSpec` table per SCP-1048
//! variant. The numbers in them were measured off the GLB by hand, and `docs/animation.md` records
//! that measuring as *"a manual offline step, not a repo tool"*: when an artist re-exported a rig,
//! nothing re-checked them. A shifted clip index or a stale cycle length has no error path — it is a
//! creature that skates or drifts out of phase, which reads as "the animation feels bad".
//!
//! `assets/emerge/rigs.ron` is now that data, and `emerge_core::clips` can re-measure it from the
//! asset — `crates/emerge-core/tests/rigs_match_assets.rs` fails the build when the two disagree.
//! This module is the bridge: read the file, and turn one rig's slot table into the
//! `AnimationGraph` + [`anim::Slot`] pair the blender wants.
//!
//! **One builder, not six.** Every creature's graph is the same shape — a flat set of clips under the
//! root, some masked — and the differences that mattered were all data. `build` is that shape; the
//! per-creature systems now only say which rig they want.

use std::sync::Arc;

use bevy::prelude::*;
use emerge_core::rigs::{Playback, Rig, Rigs};

use crate::anim;

/// Read at runtime like every other file under `assets/` — see `TESTING.md`, "assets/ is read at
/// RUNTIME".
pub const RIGS_PATH: &str = "assets/emerge/rigs.ron";

/// The parsed manifest.
#[derive(Resource)]
pub struct RigManifest(pub Rigs);

impl RigManifest {
    /// The named rig, or a message naming the file and what was asked for.
    ///
    /// A miss is fatal to whatever asked: a creature with no slot table cannot animate, and returning
    /// an empty one would be a creature that stands still for a reason nobody can find.
    pub fn rig(&self, name: &str) -> Result<&Rig, String> {
        self.0.get(name).ok_or_else(|| {
            format!(
                "{RIGS_PATH} has no rig named `{name}` — it lists {:?}",
                self.0.rigs.keys().collect::<Vec<_>>()
            )
        })
    }
}

/// Read and validate the manifest.
pub fn load() -> Result<Rigs, String> {
    let text = std::fs::read_to_string(RIGS_PATH)
        .map_err(|e| format!("cannot read {RIGS_PATH}: {e}"))?;
    Rigs::parse(&text).map_err(|e| format!("{RIGS_PATH}: {e}"))
}

/// **One rig's slot table, as a graph the blender can drive.**
///
/// Flat by necessity, and the reason is worth keeping: a blend node contributes its own *static*
/// weight, and per-instance control exists only on leaf clips (`weight = active_animation.weight *
/// graph_node.weight`), so an intermediate "action layer" node could not be faded per unit. Masking
/// the action clips individually gets the same layering with none of that problem.
///
/// Slot order is the manifest's order, and that order is the contract — the index of a slot is the
/// handle `anim::blend`'s `SLOT_*` constants name.
pub fn build(
    rig: &Rig,
    assets: &AssetServer,
    graphs: &mut Assets<AnimationGraph>,
) -> (Handle<AnimationGraph>, Arc<[anim::Slot]>) {
    let mut graph = AnimationGraph::new();
    let root = graph.root;
    let mut slots = Vec::with_capacity(rig.slots.len());
    for s in &rig.slots {
        let clip: Handle<AnimationClip> =
            assets.load(GltfAssetLabel::Animation(s.clip).from_asset(rig.mesh.clone()));
        let node = match s.mask {
            // The manifest stores the mask GROUP; the graph wants the bit.
            Some(group) => graph.add_clip_with_mask(clip, 1 << group, 1.0, root),
            None => graph.add_clip(clip, 1.0, root),
        };
        slots.push(match s.playback {
            Playback::Free { speed } => anim::Slot::free(node, speed),
            Playback::Gait {
                duration,
                phase_offset,
                cycle_distance,
            } => anim::Slot::gait(node, duration, phase_offset, cycle_distance),
            Playback::OneShot { speed } => anim::Slot::one_shot(node, speed),
        });
    }
    (graphs.add(graph), Arc::from(slots))
}

/// Puts [`RigManifest`] in the world.
///
/// **Fatal on a bad manifest, at build time.** The alternative is every creature falling back to no
/// animation, which is the shape of failure this project's one-path rule exists to prevent: a rig
/// that silently does not move looks like an asset problem, and the real cause is a file that did not
/// parse. `src/emerge_map.rs` treats `vocab.ron` the same way.
pub struct RigsPlugin;

impl Plugin for RigsPlugin {
    fn build(&self, app: &mut App) {
        match load() {
            Ok(rigs) => {
                app.insert_resource(RigManifest(rigs));
            }
            Err(e) => panic!("{e}"),
        }
    }
}
