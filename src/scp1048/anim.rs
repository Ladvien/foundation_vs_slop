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

/// This variant's name in `assets/emerge/rigs.ron`, where its clip table lives.
///
/// Four entries rather than one, because the four ship **different clip sets** — only the original can
/// draw, SCP-1048-A cannot dance — and the manifest has to be able to say so per body.
pub(crate) fn rig_name(variant: Scp1048Variant) -> &'static str {
    match variant {
        Scp1048Variant::Original => "scp1048_original",
        Scp1048Variant::EarCopy => "scp1048_ear",
        Scp1048Variant::InfantArm => "scp1048_infant",
        Scp1048Variant::Scrap => "scp1048_scrap",
    }
}

/// The manifest's `state` string → this module's enum.
///
/// **Refused loudly on an unknown name.** This is the one place a slot label is a lookup key rather
/// than a note (see `emerge_core::rigs::SlotDef::state`), and the cost of that is exactly this
/// function: a typo in the manifest has to fail at startup naming the bad string, not resolve to some
/// other state and animate the wrong thing.
fn state_from(s: &str) -> Result<AnimState, String> {
    Ok(match s {
        "rest_idle" => AnimState::RestIdle,
        "dance" => AnimState::Dance,
        "jump" => AnimState::Jump,
        "sit_down" => AnimState::SitDown,
        "draw" => AnimState::Draw,
        "rage" => AnimState::Rage,
        "attack" => AnimState::Attack,
        "aim" => AnimState::Aim,
        "fire" => AnimState::Fire,
        "whip" => AnimState::Whip,
        other => return Err(format!("`{other}` is not an SCP-1048 animation state")),
    })
}


/// Which slot plays `state` on `variant`, and whether that slot is a one-shot.
///
/// `None` means this variant does not ship a clip for that state — SCP-1048-A cannot dance, and only
/// the original can draw. The executor never asks for a state its variant lacks (the brains keep the
/// benign and hostile mode sets disjoint), and `every_state_the_executor_can_request_has_a_clip`
/// below is what holds that line; the driver simply leaves the pose alone if it ever happens, rather
/// than substituting some other animation.
pub(crate) fn slot_for(
    manifest: &crate::rigs::RigManifest,
    variant: Scp1048Variant,
    state: AnimState,
) -> Option<(usize, bool)> {
    let rig = manifest.rig(rig_name(variant)).ok()?;
    rig.slots.iter().position(|s| {
        s.state
            .as_deref()
            .and_then(|n| state_from(n).ok())
            .is_some_and(|st| st == state)
    })
    .map(|i| {
        let one_shot = matches!(
            rig.slots[i].playback,
            emerge_core::rigs::Playback::OneShot { .. }
        );
        (i, one_shot)
    })
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
    manifest: &crate::rigs::RigManifest,
    assets: &AssetServer,
    graphs: &mut Assets<AnimationGraph>,
) -> Option<BearAnim> {
    // Playback speed is 1.0 throughout: unlike the crab's sped-up scuttle, these clips are authored at
    // gameplay tempo (24 fps, in-place) and the hand-off doc's timings assume they play at 1x.
    let rig = match manifest.rig(rig_name(variant)) {
        Ok(r) => r,
        Err(e) => {
            error!("{e}");
            return None;
        }
    };
    // Every state the manifest names must resolve, or the bear would silently lack a pose.
    for slot in &rig.slots {
        if let Some(name) = slot.state.as_deref() {
            if let Err(e) = state_from(name) {
                error!("{}: {e}", rig_name(variant));
                return None;
            }
        }
    }
    let (graph, slots) = crate::rigs::build(rig, assets, graphs);
    Some(BearAnim { graph, slots })
}

/// `Startup`: build all four graphs. Every bear that spawns later clones handles out of this.
pub(crate) fn build_scp1048_anim(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    manifest: Res<crate::rigs::RigManifest>,
) {
    // **All four or none**, on the same argument the staff bodies make: a manifest missing one
    // variant is a manifest edited wrongly, and three bears that animate beside one that does not is
    // the harder failure to notice.
    let mut built = Vec::with_capacity(Scp1048Variant::ALL.len());
    for v in Scp1048Variant::ALL {
        match build_one(v, &manifest, &assets, &mut graphs) {
            Some(a) => built.push(a),
            None => return,
        }
    }
    let Ok(per_variant) = <[BearAnim; 4]>::try_from(built) else {
        error!("scp1048: expected {} variants", Scp1048Variant::ALL.len());
        return;
    };
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
    manifest: Res<crate::rigs::RigManifest>,
    mut bears: Query<(&Scp1048, &Scp1048State, &mut crate::anim::PoseBlender)>,
) {
    for (bear, state, mut blender) in &mut bears {
        // A variant with no clip for this state keeps whatever it is already showing. This is
        // unreachable for the shipped brains (see the test below) and is deliberately NOT a
        // substitute-another-clip path.
        let Some((slot, one_shot)) = slot_for(&manifest, bear.variant, state.anim) else {
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

    /// The shipped manifest, so these contracts are asserted against the file the game reads.
    fn manifest() -> crate::rigs::RigManifest {
        crate::rigs::RigManifest(crate::rigs::load().unwrap_or_else(|e| panic!("{e}")))
    }

    /// One variant's slots, in manifest order.
    fn slots(m: &crate::rigs::RigManifest, v: Scp1048Variant) -> &[emerge_core::rigs::SlotDef] {
        &m.rig(rig_name(v)).unwrap_or_else(|e| panic!("{e}")).slots
    }

    #[test]
    fn every_state_the_executor_can_request_has_a_clip() {
        let m = manifest();
        for variant in Scp1048Variant::ALL {
            for state in reachable_states(variant) {
                assert!(
                    slot_for(&m, variant, state).is_some(),
                    "{variant:?} has no clip for {state:?} — the driver would silently hold its pose"
                );
            }
        }
    }

    /// Every state a variant declares resolves to its own position, exactly once.
    ///
    /// This is what makes `state` safe to use as a lookup key: a manifest that named a state twice
    /// would silently resolve to the first, and the second slot would be unreachable.
    #[test]
    fn slots_are_dense_unique_and_in_range_per_variant() {
        let m = manifest();
        for variant in Scp1048Variant::ALL {
            let specs = slots(&m, variant);
            let mut seen: Vec<AnimState> = Vec::new();
            for (i, spec) in specs.iter().enumerate() {
                let name = spec
                    .state
                    .as_deref()
                    .unwrap_or_else(|| panic!("{variant:?} slot {i} declares no state"));
                let st = state_from(name).unwrap_or_else(|e| panic!("{variant:?} slot {i}: {e}"));
                assert!(!seen.contains(&st), "{variant:?} maps {st:?} twice");
                seen.push(st);
                let (slot, one_shot) =
                    slot_for(&m, variant, st).expect("a declared state must resolve");
                assert_eq!(slot, i, "{variant:?} {st:?} resolved to the wrong slot");
                assert_eq!(
                    one_shot,
                    matches!(spec.playback, emerge_core::rigs::Playback::OneShot { .. })
                );
                assert!(slot < specs.len());
            }
        }
    }

    #[test]
    fn clip_indices_are_unique_within_a_variant() {
        // Two slots loading the same glTF clip would mean one of them is a copy-paste slip: the
        // blender would cross-fade a clip with itself and the second state would look like a no-op.
        for variant in Scp1048Variant::ALL {
            let m = manifest();
            let mut idx: Vec<usize> = slots(&m, variant).iter().map(|c| c.clip).collect();
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
        let m = manifest();
        let (_, b_one_shot) =
            slot_for(&m, Scp1048Variant::InfantArm, AnimState::Attack).expect("B");
        assert!(!b_one_shot, "SCP-1048-B's tantrum must loop, not fire once");
        let (_, a_one_shot) = slot_for(&m, Scp1048Variant::EarCopy, AnimState::Attack).expect("A");
        assert!(a_one_shot, "SCP-1048-A's scream must be a one-shot");
    }

    #[test]
    fn the_scrap_bear_never_wires_its_inherited_dance() {
        // Tonal contract from the asset hand-off: C ships `dance` (glTF 1) as legacy motion and must
        // never play it. Its index stays pinned in the clip-contract test, but no slot may load it.
        let m = manifest();
        assert!(slot_for(&m, Scp1048Variant::Scrap, AnimState::Dance).is_none());
        assert!(
            !slots(&m, Scp1048Variant::Scrap).iter().any(|c| c.clip == 1),
            "SCP-1048-C must not wire glTF clip 1 (its inherited dance)"
        );
    }

    #[test]
    fn only_the_original_can_draw_and_only_c_can_use_the_gun() {
        let m = manifest();
        assert!(slot_for(&m, Scp1048Variant::Original, AnimState::Draw).is_some());
        for v in [Scp1048Variant::EarCopy, Scp1048Variant::InfantArm, Scp1048Variant::Scrap] {
            assert!(slot_for(&m, v, AnimState::Draw).is_none(), "{v:?} must not draw pictures");
        }
        for state in [AnimState::Aim, AnimState::Fire, AnimState::Whip] {
            assert!(slot_for(&m, Scp1048Variant::Scrap, state).is_some());
            for v in
                [Scp1048Variant::Original, Scp1048Variant::EarCopy, Scp1048Variant::InfantArm]
            {
                assert!(slot_for(&m, v, state).is_none(), "{v:?} has no arm gun");
            }
        }
    }
}
