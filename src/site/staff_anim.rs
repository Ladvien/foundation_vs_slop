//! **Animating Site-67's staff** — N rigs, one clip vocabulary, one table, N graphs.
//!
//! The shape is `src/scp1048/anim.rs`, not `src/squad.rs`, and there is one simplification the bears
//! do not get: measured 2026-08-02, **all eight staff rigs ship the same 20 clips at the same
//! indices**, so where that module needs a `TABLES` array with one `ClipSpec` list per variant, this
//! needs a single [`STAFF_CLIPS`]. `tests/staff_asset.rs` is what keeps that true.
//!
//! # Why every slot is `Free`, and no slot is `Gait`
//!
//! A [`Gait`](crate::anim::Playback::Gait) slot needs `(duration, phase_offset, cycle_distance)`
//! **measured off the GLB by hand** — `docs/animation.md` says plainly there is no tool in this repo
//! for that, and it is the largest hidden cost in animating a new character. It also sanctions the
//! alternative: the crab, the manca and the SCP-1048 family use `Free` throughout, because their clips
//! share no stride.
//!
//! Staff qualify for the same exemption for a different reason. The squad co-weights walk against run
//! against two strafes every frame, and feet that are not on a shared phase visibly skate. Staff move
//! at one speed under a discrete state machine, so **no two locomotion clips are ever co-weighted at
//! once**, and there is nothing for a shared phase to synchronise. That is a decision, not an
//! omission — if staff ever blend two gaits, the measurement becomes mandatory and this note is where
//! to start.
//!
//! # The bug this file's split exists to prevent
//!
//! `site::visuals::drive_avatar_animation` drove **every** `AvatarModel` through
//! `squad::valkyrie_weights`, which returns `[f32; 10]` in the Valkyrie's slot order. Feed that to a
//! staff blender and `PoseBlender::set_targets` returns `Err` and **writes nothing at all** — no
//! padding, no truncation (`src/anim/mod.rs`). The visible result is not a wrong pose but a body frozen
//! in bind pose forever, plus one `error!` per staff member per frame. So the driver is split by rig,
//! and the two drivers never see each other's models.
//!
//! Cosmetic → `Update` only, never `FixedUpdate` (`docs/animation.md`). Nothing here is pinned state.

use std::sync::Arc;

use bevy::prelude::*;

use super::people::StaffRig;
use crate::anim;

// Slot → glTF clip index now lives in `assets/emerge/rigs.ron`, one entry per body, and
// `crates/emerge-core/tests/rigs_match_assets.rs` checks each against the GLB it names. It was a
// `STAFF_CLIPS` array here; `tests/staff_asset.rs` still holds the full 20-clip vocabulary and fails
// if any rig's order drifts.
//
// Only the clips a driver actually weights are wired — the other sixteen exist in the asset and are
// loaded by nothing, which is deliberate: a slot nobody drives is a weight that is always zero, and
// the repo's named process risk is shipping exactly that kind of unreachable correctness.
//
// ⚠️ **Stage C adds `sit` (17), `wave` (13) and `point` (15)** when the routine layer has a reason to
// play them. `sit` is the whole reason the staff rigs were chosen — the Valkyrie has no such clip, and
// the design leans on that (*operatives stand and lean; staff are the ones who sit*). Adding them is
// now an edit to each body's slot list in the manifest, not to a table here.

pub const SLOT_IDLE: usize = 0;
pub const SLOT_IDLE_LOOK: usize = 1;
pub const SLOT_WALK: usize = 2;
pub const SLOT_JOG: usize = 3;
/// How many slots a staff blender carries. **Not** `anim::blend::LOCO_SLOTS` (8): that partition is
/// built for a unit that strafes and walks backwards under fire, and a cook does neither. A narrower
/// table is a narrower `set_targets` contract and one fewer clip resident per body.
pub const SLOTS: usize = 4;

/// Metres per second past which a walking body reads as jogging. Staff have one travel speed today
/// (`visuals::AVATAR_SPEED`), so this exists to make the jog slot reachable at all rather than to
/// model a second gear.
const JOG_ABOVE: f32 = 2.6;
/// Below this a body is standing still.
const MOVING_ABOVE: f32 = 0.05;

/// One rig's graph and slot table, cloned by refcount onto every body wearing it.
#[derive(Debug, Clone)]
pub struct RigAnim {
    pub graph: Handle<AnimationGraph>,
    pub slots: Arc<[anim::Slot]>,
    /// The manifest's render scale for this body's model child (1.13; see rigs.ron).
    pub scale: f32,
}

/// Every staff rig's animation, built once at `Startup`.
///
/// Indexed by [`StaffRig::index`], exactly the way `Scp1048Anim` is indexed by
/// `Scp1048Variant::index()`. Eight graphs is eight, not one per body: nine staff wearing eight rigs
/// share by `Arc` refcount, and adding a tenth costs two refcount bumps.
#[derive(Resource, Debug)]
pub struct StaffAnim {
    per_rig: [RigAnim; StaffRig::ALL.len()],
}

impl StaffAnim {
    pub fn get(&self, rig: StaffRig) -> &RigAnim {
        &self.per_rig[rig.index()]
    }
}

fn build_one(
    rig: StaffRig,
    manifest: &crate::rigs::RigManifest,
    assets: &AssetServer,
    graphs: &mut Assets<AnimationGraph>,
) -> Option<RigAnim> {
    // Every clip loops and is never rewound; only its weight moves - the manifest says `Free` for all
    // four. See the header for why none of these is a `Gait`.
    let spec = match manifest.rig(rig.rig_name()) {
        Ok(r) => r,
        Err(e) => {
            error!("{e}");
            return None;
        }
    };
    let (graph, slots) = crate::rigs::build(spec, assets, graphs);
    debug_assert_eq!(slots.len(), SLOTS);
    Some(RigAnim { graph, slots, scale: spec.scale })
}

/// `Startup`. Must run before any staff body spawns, since the spawn clones the graph handle onto the
/// body as an `anim::BlendSource` — the same ordering constraint `build_valkyrie_anim` states.
pub fn build_staff_anim(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    manifest: Res<crate::rigs::RigManifest>,
) {
    // **All eight or none.** The eight share one table, so a manifest missing one of them is a
    // manifest edited wrongly rather than a body that happens to be absent - and eight bodies where
    // one silently never animates is much the harder failure to find.
    let mut built = Vec::with_capacity(StaffRig::ALL.len());
    for r in StaffRig::ALL {
        match build_one(r, &manifest, &assets, &mut graphs) {
            Some(a) => built.push(a),
            None => return,
        }
    }
    let Ok(per_rig) = <[RigAnim; 8]>::try_from(built) else {
        error!("staff: expected {} rigs", StaffRig::ALL.len());
        return;
    };
    commands.insert_resource(StaffAnim { per_rig });
}

/// The weight vector for a staff body moving at `speed`, with `look` selecting the idle variant.
///
/// A partition of unity, like `anim::blend::locomotion_weights` — the weights sum to exactly 1 so the
/// blend never darkens or doubles a pose. Pure, so it is unit-testable with no `App`.
///
/// `look` is per-person and constant: nine bodies all playing clip 0 breathe in lockstep, which reads
/// as a rendering artefact rather than as people. Splitting them across two idles on a stable hash of
/// `CastId` costs nothing and breaks the pattern.
pub fn staff_weights(speed: f32, look: bool) -> [f32; SLOTS] {
    let mut w = [0.0; SLOTS];
    if speed <= MOVING_ABOVE {
        // Standing. One idle or the other, never a blend of both — they are different poses, not two
        // points on one axis, and crossfading them would read as a body that cannot decide.
        w[if look { SLOT_IDLE_LOOK } else { SLOT_IDLE }] = 1.0;
        return w;
    }
    if speed >= JOG_ABOVE {
        w[SLOT_JOG] = 1.0;
        return w;
    }
    // Between the two, cross-fade so a body accelerating out of a doorway does not pop.
    let t = ((speed - MOVING_ABOVE) / (JOG_ABOVE - MOVING_ABOVE)).clamp(0.0, 1.0);
    w[SLOT_WALK] = 1.0 - t;
    w[SLOT_JOG] = t;
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants are how a driver addresses a slot; the manifest is what each slot loads. They
    /// are still two lists of the same thing — that is exactly how the Valkyrie's clip indices
    /// drifted once already — so this checks them against each other, now reading the manifest rather
    /// than a second array in this file.
    ///
    /// **Every body, not just one.** The eight share a clip vocabulary, and a manifest edit that
    /// touched seven of them is the failure this catches.
    #[test]
    fn the_manifest_and_the_slot_constants_agree() {
        let rigs = crate::rigs::load().unwrap_or_else(|e| panic!("{e}"));
        for body in StaffRig::ALL {
            let rig = rigs
                .get(body.rig_name())
                .unwrap_or_else(|| panic!("rigs.ron has no `{}`", body.rig_name()));
            assert_eq!(rig.slots.len(), SLOTS, "{}", body.rig_name());
            for (slot, want, what) in [
                (SLOT_IDLE, 0, "<rig>_idle"),
                (SLOT_IDLE_LOOK, 2, "<rig>_idle_look"),
                (SLOT_WALK, 4, "<rig>_walk"),
                (SLOT_JOG, 6, "<rig>_jog"),
            ] {
                assert_eq!(
                    rig.slots[slot].clip,
                    want,
                    "{} slot {slot} must load {what}",
                    body.rig_name()
                );
            }
            // Every wired index must be inside the 20-clip vocabulary `tests/staff_asset.rs` pins.
            for s in &rig.slots {
                assert!(
                    s.clip < 20,
                    "{}: clip index {} is past the end of the shared 20-clip vocabulary",
                    body.rig_name(),
                    s.clip
                );
            }
        }
    }

    #[test]
    fn the_weights_always_partition_unity() {
        // A blend that does not sum to 1 either darkens the pose or doubles it. Swept rather than
        // spot-checked because the interesting failures are at the seams between the branches.
        for step in 0..=60 {
            let speed = step as f32 * 0.1;
            for look in [false, true] {
                let w = staff_weights(speed, look);
                let sum: f32 = w.iter().sum();
                assert!(
                    (sum - 1.0).abs() < 1.0e-5,
                    "speed {speed} look {look} sums to {sum}, not 1"
                );
                assert!(w.iter().all(|x| *x >= 0.0), "negative weight at speed {speed}");
            }
        }
    }

    #[test]
    fn a_standing_body_is_in_exactly_one_idle_and_never_a_blend_of_two() {
        // The two idles are different poses. Crossfading them reads as a body that cannot decide which
        // way to look, which is worse than either.
        let still = staff_weights(0.0, false);
        assert_eq!(still[SLOT_IDLE], 1.0);
        assert_eq!(still[SLOT_IDLE_LOOK], 0.0);

        let looking = staff_weights(0.0, true);
        assert_eq!(looking[SLOT_IDLE_LOOK], 1.0);
        assert_eq!(looking[SLOT_IDLE], 0.0);
    }

    #[test]
    fn a_walking_body_leaves_both_idles_behind() {
        // The regression: an idle weight bleeding into a walk is a body that moves while standing, and
        // it is the specific thing a partition-of-unity check alone would not catch.
        let w = staff_weights(1.5, false);
        assert_eq!(w[SLOT_IDLE], 0.0);
        assert_eq!(w[SLOT_IDLE_LOOK], 0.0);
        assert!(w[SLOT_WALK] > 0.0);
    }
}
