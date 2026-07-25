//! Per-variant animation graphs and the cosmetic clip driver for the SCP-1048 family.
//!
//! **Four bears, four clip orders.** They share a rig and a clip vocabulary but not an index layout:
//! only the original ships `draw_picture`, each copy adds its own hostile set, and SCP-1048-C carries
//! a `dance` it must never play. So there is no shared `SLOT_*` const block — instead each variant
//! owns a [`ClipSpec`] table, in slot order, which is the single source of truth for three things at
//! once: which glTF clip index a slot loads, whether that slot is a one-shot, and which [`AnimState`]
//! selects it. `tests/creature_clip_contract.rs` pins those glTF indices against the asset bytes.
//!
//! Wiring follows `docs/animation.md`'s "wire a new animated creature" checklist. Everything here is
//! **cosmetic** and runs on `Update`, never `FixedUpdate` — it only ever *reads* [`Scp1048State`],
//! which the gameplay executor owns. Never attach `AnimationTransitions`: the shared `PoseBlender`
//! eases weights itself and a transition pass would stomp them.
//!
//! No `Gait` slots. The bears have no locomotion cycle to phase-lock — a shuffle, a hop and a shriek
//! share no stride — so every slot is `Free` or `OneShot`, as with the crab and the manca.

use std::sync::Arc;

use bevy::prelude::*;

use super::{AnimState, Scp1048, Scp1048State, Scp1048Variant};

/// One animation slot: which clip it loads, and how it plays.
struct ClipSpec {
    /// The [`AnimState`] that selects this slot.
    state: AnimState,
    /// Index into the variant's glb animation list (pinned by `tests/creature_clip_contract.rs`).
    gltf: usize,
    /// `true` ⇒ a `OneShot` slot: triggered on the edge and allowed to run to its end, holding its
    /// final frame. `false` ⇒ a `Free` slot that loops and is never rewound.
    one_shot: bool,
}

const fn clip(state: AnimState, gltf: usize, one_shot: bool) -> ClipSpec {
    ClipSpec { state, gltf, one_shot }
}

/// SCP-1048, the benign original — all five clips wired.
const ORIGINAL_CLIPS: &[ClipSpec] = &[
    clip(AnimState::RestIdle, 0, false),
    clip(AnimState::Dance, 1, false),
    clip(AnimState::Jump, 2, true),
    clip(AnimState::Draw, 3, false),
    clip(AnimState::SitDown, 4, true),
];

/// SCP-1048-A, the ear bear — all five wired. `scream` is a one-shot attack that returns to neutral.
const EAR_CLIPS: &[ClipSpec] = &[
    clip(AnimState::RestIdle, 0, false),
    clip(AnimState::Jump, 1, true),
    clip(AnimState::SitDown, 2, true),
    clip(AnimState::Attack, 3, true), // scream
    clip(AnimState::Rage, 4, false),
];

/// SCP-1048-B, the infant-arm bear — all six wired.
///
/// Note `Attack` (`tantrum`) is **`Free`, not `OneShot`**: it is authored as a looping fit with two
/// flail cycles per pass, so it is driven as a *state* held for as long as the bear is attacking,
/// never triggered as an event. This is the one attack in the codebase that loops.
const INFANT_CLIPS: &[ClipSpec] = &[
    clip(AnimState::RestIdle, 0, false),
    clip(AnimState::Dance, 1, false),
    clip(AnimState::Jump, 2, true),
    clip(AnimState::SitDown, 3, true),
    clip(AnimState::Attack, 4, false), // tantrum — LOOPS
    clip(AnimState::Rage, 5, false),
];

/// SCP-1048-C, the rusted scrap bear — seven of its eight clips.
///
/// **`scp1048c_dance` (glTF index 1) is deliberately not wired.** It ships as legacy motion inherited
/// from the benign original and reads wrong on a violent copy; the asset's own hand-off note says to
/// leave it alone. The contract test still pins its index, so a re-export that drops it — which would
/// shift every hostile clip below it — fails loudly instead of silently playing the wrong animation.
const SCRAP_CLIPS: &[ClipSpec] = &[
    clip(AnimState::RestIdle, 0, false),
    clip(AnimState::Jump, 2, true),
    clip(AnimState::SitDown, 3, true),
    clip(AnimState::Aim, 4, true),
    clip(AnimState::Fire, 5, true),
    clip(AnimState::Whip, 6, true),
    clip(AnimState::Rage, 7, false),
];

/// The clip table for a variant, indexed by [`Scp1048Variant::index`].
const TABLES: [&[ClipSpec]; 4] = [ORIGINAL_CLIPS, EAR_CLIPS, INFANT_CLIPS, SCRAP_CLIPS];

/// Which slot plays `state` on `variant`, and whether that slot is a one-shot.
///
/// `None` means this variant does not ship a clip for that state — SCP-1048-A cannot dance, and only
/// the original can draw. The executor never asks for a state its variant lacks (the brains keep the
/// benign and hostile mode sets disjoint), and `every_state_the_executor_can_request_has_a_clip`
/// below is what holds that line; the driver simply leaves the pose alone if it ever happens, rather
/// than substituting some other animation.
pub(crate) fn slot_for(variant: Scp1048Variant, state: AnimState) -> Option<(usize, bool)> {
    TABLES[variant.index()]
        .iter()
        .position(|c| c.state == state)
        .map(|slot| (slot, TABLES[variant.index()][slot].one_shot))
}

/// One variant's built graph plus its slot table.
pub struct BearAnim {
    pub(crate) graph: Handle<AnimationGraph>,
    pub(crate) slots: Arc<[crate::anim::Slot]>,
}

/// The four graphs, built once at `Startup`. Spawning clones by refcount — never a table copy.
///
/// Public because [`super::spawn_scp1048_at`] is: the Research Room dev tool drops bears through the
/// same one builder the seeder and the replicator use, so an F6-spawned bear is byte-identical.
#[derive(Resource)]
pub struct Scp1048Anim {
    per_variant: [BearAnim; 4],
}

impl Scp1048Anim {
    pub fn get(&self, variant: Scp1048Variant) -> &BearAnim {
        &self.per_variant[variant.index()]
    }
}

/// Build one variant's `AnimationGraph` + slot table from its [`ClipSpec`] table.
fn build_one(
    variant: Scp1048Variant,
    assets: &AssetServer,
    graphs: &mut Assets<AnimationGraph>,
) -> BearAnim {
    let specs = TABLES[variant.index()];
    let (graph, nodes) = AnimationGraph::from_clips(
        specs
            .iter()
            .map(|c| assets.load(GltfAssetLabel::Animation(c.gltf).from_asset(variant.glb()))),
    );
    // Playback speed is 1.0 throughout: unlike the crab's sped-up scuttle, these clips are authored at
    // gameplay tempo (24 fps, in-place) and the hand-off doc's timings assume they play at 1×.
    let slots: Arc<[crate::anim::Slot]> = specs
        .iter()
        .zip(nodes.iter())
        .map(|(c, &node)| {
            if c.one_shot {
                crate::anim::Slot::one_shot(node, 1.0)
            } else {
                crate::anim::Slot::free(node, 1.0)
            }
        })
        .collect();
    BearAnim { graph: graphs.add(graph), slots }
}

/// `Startup`: build all four graphs. Every bear that spawns later clones handles out of this.
pub(crate) fn build_scp1048_anim(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let per_variant = Scp1048Variant::ALL.map(|v| build_one(v, &assets, &mut graphs));
    commands.insert_resource(Scp1048Anim { per_variant });
}

/// `Update`: point each bear's blender at the clip its gameplay state calls for.
///
/// One-shots are triggered on the **edge** — detected from the blender's own weights
/// (`target_weight(slot) <= 0.0`), the driver's one-frame memory, exactly as `docs/animation.md`
/// step 4 prescribes and as `parasite::drive_manca_animation` does. Do not reach for `active_shot()`:
/// how long a pose is *held* is the state machine's business, not the animation layer's.
///
/// Re-triggering a running one-shot restarts it, which is what SCP-1048-C's sustained fire wants —
/// `fire_gun` begins and ends in the same aim pose (a measured 0.000 mm seam), so shots replay
/// cleanly, and staying on that slot afterwards leaves the bear holding its aim with no fade back.
pub(crate) fn drive_scp1048_animation(
    mut bears: Query<(&Scp1048, &Scp1048State, &mut crate::anim::PoseBlender)>,
) {
    for (bear, state, mut blender) in &mut bears {
        // A variant with no clip for this state keeps whatever it is already showing. This is
        // unreachable for the shipped brains (see the test below) and is deliberately NOT a
        // substitute-another-clip path.
        let Some((slot, one_shot)) = slot_for(bear.variant, state.anim) else {
            continue;
        };
        let entering = one_shot && blender.target_weight(slot) <= 0.0;
        blender.set_only(slot);
        if entering {
            blender.trigger(slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The states each variant's executor can actually ask for. Kept beside the assertion it feeds so
    /// a new behaviour that forgets its clip is a loud failure rather than a bear frozen mid-pose.
    fn reachable_states(variant: Scp1048Variant) -> Vec<AnimState> {
        match variant {
            // The benign original: idle, the emote repertoire, and the seated drawing chain.
            Scp1048Variant::Original => vec![
                AnimState::RestIdle,
                AnimState::Dance,
                AnimState::Jump,
                AnimState::SitDown,
                AnimState::Draw,
            ],
            // A and B: idle, the threat display, and their own attack.
            Scp1048Variant::EarCopy | Scp1048Variant::InfantArm => {
                vec![AnimState::RestIdle, AnimState::Rage, AnimState::Attack]
            }
            // C attacks through the gun chain instead of a single Attack clip.
            Scp1048Variant::Scrap => vec![
                AnimState::RestIdle,
                AnimState::Rage,
                AnimState::Aim,
                AnimState::Fire,
                AnimState::Whip,
            ],
        }
    }

    #[test]
    fn every_state_the_executor_can_request_has_a_clip() {
        for variant in Scp1048Variant::ALL {
            for state in reachable_states(variant) {
                assert!(
                    slot_for(variant, state).is_some(),
                    "{variant:?} has no clip for {state:?} — the driver would silently hold its pose"
                );
            }
        }
    }

    #[test]
    fn slots_are_dense_unique_and_in_range_per_variant() {
        for variant in Scp1048Variant::ALL {
            let specs = TABLES[variant.index()];
            let mut seen: Vec<AnimState> = Vec::new();
            for (i, spec) in specs.iter().enumerate() {
                assert!(!seen.contains(&spec.state), "{variant:?} maps {:?} twice", spec.state);
                seen.push(spec.state);
                let (slot, one_shot) =
                    slot_for(variant, spec.state).expect("a tabled state must resolve");
                assert_eq!(slot, i, "{variant:?} {:?} resolved to the wrong slot", spec.state);
                assert_eq!(one_shot, spec.one_shot);
                assert!(slot < specs.len());
            }
        }
    }

    #[test]
    fn clip_indices_are_unique_within_a_variant() {
        // Two slots loading the same glTF clip would mean one of them is a copy-paste slip: the
        // blender would cross-fade a clip with itself and the second state would look like a no-op.
        for variant in Scp1048Variant::ALL {
            let mut idx: Vec<usize> = TABLES[variant.index()].iter().map(|c| c.gltf).collect();
            let before = idx.len();
            idx.sort_unstable();
            idx.dedup();
            assert_eq!(idx.len(), before, "{variant:?} wires one glTF clip into two slots");
        }
    }

    #[test]
    fn the_infant_arm_tantrum_loops_but_every_other_attack_is_a_one_shot() {
        // The asset contract that most easily gets "tidied" into uniformity. B's tantrum is authored
        // as a looping fit and must be driven as a state; A's scream is a discrete event.
        let (_, b_one_shot) = slot_for(Scp1048Variant::InfantArm, AnimState::Attack).expect("B");
        assert!(!b_one_shot, "SCP-1048-B's tantrum must loop, not fire once");
        let (_, a_one_shot) = slot_for(Scp1048Variant::EarCopy, AnimState::Attack).expect("A");
        assert!(a_one_shot, "SCP-1048-A's scream must be a one-shot");
    }

    #[test]
    fn the_scrap_bear_never_wires_its_inherited_dance() {
        // Tonal contract from the asset hand-off: C ships `dance` (glTF 1) as legacy motion and must
        // never play it. Its index stays pinned in the clip-contract test, but no slot may load it.
        assert!(slot_for(Scp1048Variant::Scrap, AnimState::Dance).is_none());
        assert!(
            !SCRAP_CLIPS.iter().any(|c| c.gltf == 1),
            "SCP-1048-C must not wire glTF clip 1 (its inherited dance)"
        );
    }

    #[test]
    fn only_the_original_can_draw_and_only_c_can_use_the_gun() {
        assert!(slot_for(Scp1048Variant::Original, AnimState::Draw).is_some());
        for v in [Scp1048Variant::EarCopy, Scp1048Variant::InfantArm, Scp1048Variant::Scrap] {
            assert!(slot_for(v, AnimState::Draw).is_none(), "{v:?} must not draw pictures");
        }
        for state in [AnimState::Aim, AnimState::Fire, AnimState::Whip] {
            assert!(slot_for(Scp1048Variant::Scrap, state).is_some());
            for v in
                [Scp1048Variant::Original, Scp1048Variant::EarCopy, Scp1048Variant::InfantArm]
            {
                assert!(slot_for(v, state).is_none(), "{v:?} has no arm gun");
            }
        }
    }
}
