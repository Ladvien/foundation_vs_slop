//! **The player's containment verbs** — the input half of Push 2.
//!
//! Push 2 shipped all three containment archetypes and every one of them was reachable only from a
//! test: nothing in `src/` ever spawned a [`crate::containment::ContainmentDevice`], spawned a
//! [`crate::containment::Quarantine`], or inserted [`crate::containment::Capped`]. This module is the
//! data those verbs need; `crate::selection` reads the mouse and drives them.
//!
//! **Design: a verb is armed, then aimed.** Press a key to arm ([`ArmedTool`]), then left-click the
//! target. An unarmed left-click is still a move order, so nothing that worked before changes.
//!
//! The grounding is [SDT-13] (Vansteenkiste & Ryan 2013, *On psychological growth and vulnerability*,
//! DOI 10.1037/a0032359): *"need supportive environments, like those that provide **meaningful choice**
//! or deliver effectance-relevant feedback facilitate intrinsically motivated behavior through the
//! satisfaction of the needs for autonomy and competence. Conversely, **controlling reward
//! contingencies** … "* — which is the same argument FVS-F-2 makes for unlocks granting *verbs* rather
//! than numbers, pushed down onto the moment-to-moment input. A distinct verb the player elects to
//! spend is a choice; a passive damage multiplier is a reward contingency.

use bevy::prelude::*;

/// Which verb the next left-click will spend, or [`ArmedTool::None`] for a plain move order.
///
/// A resource rather than per-unit state: the whole squad is permanently selected (see
/// `crate::selection`), so arming is a property of the *player's* intent, not of any operative.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ArmedTool {
    /// Left-click issues a move order — the shipped behaviour, unchanged.
    #[default]
    None,
    /// Left-click throws a capture device at the anomaly under the cursor.
    Device,
    /// Left-click places a quarantine region on the floor under the cursor.
    Quarantine,
    /// Left-click orders the squad to cap the nest under the cursor.
    Cap,
    /// Left-click throws a noisemaker onto the floor under the cursor (`crate::lure`).
    Lure,
}

/// Capture devices left this expedition. Zeroed from tuning on `OnEnter(RunState::Active)`.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceSupply(pub u32);

/// Quarantine regions left this expedition.
///
/// A **separate pool** from [`DeviceSupply`], deliberately: `area`'s module docs insist the three
/// archetypes are genuinely distinct verbs (one captures a body, one bounds a region, one caps a
/// structure). One fungible charge would let the player collapse them back into a single resource and
/// the distinction would stop being a decision.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QuarantineSupply(pub u32);

/// A **stable, unique identity** for anything the player can aim a verb at.
///
/// Aiming resolves an entity from a cursor position, which is a *pick from a query* — and
/// `util::nearest_planar_keyed`'s contract is explicit that breaking the tie needs a key that is
/// (a) reproducible across same-seed `App`s, (b) unique per candidate, and (c) **never derived from the
/// tied quantity** (so position is disqualified — that was the original `GibKey` mistake).
///
/// No such key existed across the targetable set: SCP-999 carries `Scp999Seed`, SCP-1048-A/B carry
/// `CyanideSmell::id` via `Biological`, and nests carry nothing at all. This is the one uniform key, and
/// it is deliberately a component rather than a field on `Containment` because **nests are targetable
/// but carry no `Containment`**.
///
/// Inserted at spawn, never toggled, so it cannot split a hashed archetype.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TargetId(pub u32);

/// Mints [`TargetId`]s in spawn order.
///
/// **Contract for anyone adding a targetable spawner:** advance this only from a spawner whose own
/// order is already deterministic — a seeded loop or a grid raster, as the two current callers are.
/// Advancing it while walking an *ECS query* would launder query order (which is not stable across
/// `App` instances) into the key, and the key would then be exactly as unstable as the thing it exists
/// to stabilise. Sort the query by a stable key first, or mint from the spawner instead.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct TargetSeq(pub u32);

impl TargetSeq {
    /// Take the next id.
    pub fn next(&mut self) -> TargetId {
        let id = TargetId(self.0);
        self.0 = self.0.wrapping_add(1);
        id
    }
}

/// Resolve the cursor to a target, or `None` if nothing eligible is close enough.
///
/// Pure, so the tie-breaking is unit-testable with no `App` — which matters, because a tie here is not
/// theoretical: two anomalies at bit-identical positions would otherwise hand the pick to ECS iteration
/// order and two same-seed runs would spend the device on different creatures.
///
/// `max_dist` keeps a click on empty floor from grabbing something across the room; the caller reports
/// the miss rather than silently retargeting.
pub fn pick_target<T>(
    cursor: Vec3,
    max_dist: f32,
    candidates: impl IntoIterator<Item = (TargetId, T, Vec3)>,
) -> Option<T> {
    let keyed = candidates.into_iter().map(|(id, payload, pos)| (u64::from(id.0), payload, pos));
    crate::util::nearest_planar_keyed(cursor, keyed)
        .filter(|(_, _, dist)| *dist <= max_dist)
        .map(|(payload, _, _)| payload)
}

/// Reset the per-run verb state as an expedition begins.
///
/// Registered on `OnEnter(RunState::Active)` before the world is built, alongside `session::reset_run`.
pub fn reset_verbs(
    mut armed: ResMut<ArmedTool>,
    mut devices: ResMut<DeviceSupply>,
    mut zones: ResMut<QuarantineSupply>,
    mut seq: ResMut<TargetSeq>,
    mut tight: ResMut<crate::laser::WeaponsTight>,
    tuning: Res<crate::sim::SimTuning>,
) {
    *armed = ArmedTool::None;
    *devices = DeviceSupply(tuning.containment.device_supply);
    *zones = QuarantineSupply(tuning.containment.quarantine_supply);
    *seq = TargetSeq::default();
    *tight = crate::laser::WeaponsTight(false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tied_pick_is_broken_by_target_id_not_input_order() {
        // Two candidates at BIT-IDENTICAL positions. Without a stable key this resolves by whichever
        // order the query happened to yield, which is not stable across `App` instances — so the same
        // seed would spend the device on a different creature on a different run.
        let p = Vec3::new(4.0, 0.0, 4.0);
        let forward = [(TargetId(7), 'a', p), (TargetId(3), 'b', p)];
        let backward = [(TargetId(3), 'b', p), (TargetId(7), 'a', p)];
        assert_eq!(pick_target(p, 10.0, forward), pick_target(p, 10.0, backward));
        // And it is the LOWER id that wins, not "whichever came first".
        assert_eq!(pick_target(p, 10.0, forward), Some('b'));
    }

    #[test]
    fn distance_beats_id_so_the_key_only_breaks_true_ties() {
        // The key is a tiebreak, not the ranking. A nearer candidate with a higher id still wins.
        let cursor = Vec3::ZERO;
        let cands = [
            (TargetId(9), "near", Vec3::new(1.0, 0.0, 0.0)),
            (TargetId(0), "far", Vec3::new(5.0, 0.0, 0.0)),
        ];
        assert_eq!(pick_target(cursor, 10.0, cands), Some("near"));
    }

    #[test]
    fn a_click_on_empty_floor_picks_nothing_rather_than_grabbing_across_the_room() {
        let cands = [(TargetId(1), 'x', Vec3::new(20.0, 0.0, 0.0))];
        assert_eq!(pick_target(Vec3::ZERO, 2.5, cands), None);
    }

    #[test]
    fn the_pick_is_planar_so_a_flying_target_is_still_reachable() {
        // Every other reach check in this module tree ignores Y for the same reason.
        let cands = [(TargetId(1), 'x', Vec3::new(1.0, 30.0, 0.0))];
        assert_eq!(pick_target(Vec3::ZERO, 2.5, cands), Some('x'));
    }

    #[test]
    fn the_sequence_is_unique_and_monotonic() {
        let mut seq = TargetSeq::default();
        let ids: Vec<_> = (0..4).map(|_| seq.next()).collect();
        assert_eq!(ids, vec![TargetId(0), TargetId(1), TargetId(2), TargetId(3)]);
    }
}
