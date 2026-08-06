//! **One rig's slot table, as a graph the blender can drive** — the builder, beside the blender.
//!
//! Moved here from the game's `src/rigs.rs` (which now delegates) so the editor's bench can stage a
//! rig through the *real* machinery rather than a copy — the same "a map cannot look one way here
//! and another in the game" argument this crate's manifest makes for the blender itself. The game
//! and the bench call the one builder; neither carries a second.

use std::sync::Arc;

use bevy::prelude::*;

use emerge_core::rigs::{Playback, Rig};

use crate::Slot;

/// Build a rig's `AnimationGraph` and slot table from its manifest entry.
///
/// Flat by necessity, and the reason is worth keeping: a blend node contributes its own *static*
/// weight, and per-instance control exists only on leaf clips (`weight = active_animation.weight *
/// graph_node.weight`), so an intermediate "action layer" node could not be faded per unit. Masking
/// the action clips individually gets the same layering with none of that problem.
///
/// Slot order is the manifest's order, and that order is the contract — the index of a slot is the
/// handle `blend`'s `SLOT_*` constants name.
pub fn build(
    rig: &Rig,
    assets: &AssetServer,
    graphs: &mut Assets<AnimationGraph>,
) -> (Handle<AnimationGraph>, Arc<[Slot]>) {
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
            Playback::Free { speed } => Slot::free(node, speed),
            Playback::Gait {
                duration,
                phase_offset,
                cycle_distance,
            } => Slot::gait(node, duration, phase_offset, cycle_distance),
            Playback::OneShot { speed } => Slot::one_shot(node, speed),
        });
    }
    (graphs.add(graph), Arc::from(slots))
}
