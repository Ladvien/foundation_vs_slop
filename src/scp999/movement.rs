//! SCP-999 seek + tickle-contact calm — the whole gameplay behaviour, on `FixedUpdate` `.after(Think)`.
//!
//! Each blob picks the **most-anxious** squad member (highest FEAR), oozes toward it around walls, and —
//! once it *touches* them — tickles: draining that member's FEAR and lifting MORALE. Contact-only delivery
//! (there is no passive aura): the comfort is the touch, mirroring that petting a *live* animal calms more
//! than mere presence (Shiloh 2003, in Beetz et al. 2012). Seek is Reynolds steering toward a target
//! (Reynolds, GDC 1999); the wall-slide reuses `Dungeon::resolve_move`, the same solver the squad uses.

use bevy::prelude::*;

use super::{Scp999, Scp999Motion, MAX_FRAME_DT, SCP999_HALF, UNIT_BODY_RADIUS};
use crate::ai::drives::{DriveId, Drives};
use crate::dungeon::Dungeon;
use crate::sim::SimTuning;
use crate::squad::{SquadMember, Unit};

/// Distance (world units) within which the wall-standoff push acts. Sized to the (shrunk) gel dome's
/// ~0.55 m visible half-width (+ margin) so the centre is held far enough off a wall that the body clears.
const WALL_STANDOFF: f32 = 0.6;
/// Gain on the wall-standoff steering push (mirrors `enemy::WALL_PUSH_GAIN`).
const WALL_PUSH_GAIN: f32 = 2.0;

/// One blob → most-anxious member → ooze → tickle-calm on contact.
///
/// ## Determinism
/// - **Target pick** is a deterministic arg-max over a *total* key `(FEAR desc, planar-dist asc,
///   SquadMember asc)`. FEAR ties break to the nearest member; a dist tie (bit-identical positions) breaks
///   to the lowest `SquadMember` (unique, never `Entity`) — so the choice never depends on ECS query order
///   (`util::nearest_planar`'s rule).
/// - **The calm write** subtracts from FEAR toward a 0 floor and adds to MORALE toward a 1 ceiling, both
///   via `Drives::set` (which clamps). Clamp-bounded subtraction/addition is *order-independent*: for any
///   `f, a, b ≥ 0`, `max(0, max(0, f−a)−b) == max(0, max(0, f−b)−a)`, and calm is applied **once per unit**
///   (if touched by *any* blob), not once per blob — so neither unit-iteration order nor multiple
///   overlapping blobs can perturb the result. No `sort_total!` needed here (contrast `crab_alarm_on_damage`,
///   whose additive-into-a-shared-field deposits DO need sorting).
pub(crate) fn scp999_seek_and_tickle(
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    mut blobs: Query<(&mut Transform, &mut Scp999Motion), With<Scp999>>,
    // `Without<Scp999>` makes this provably disjoint from `blobs` (which mutates `Transform`), so Bevy's
    // query-conflict checker is satisfied — the same convention `enemy::enemy_seek` uses for its unit query.
    mut units: Query<(Entity, &Transform, &SquadMember, &mut Drives), (With<Unit>, Without<Scp999>)>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    if dt <= 0.0 {
        return;
    }
    let t = &sim.scp999;
    let reach = UNIT_BODY_RADIUS + t.contact_radius;

    // Snapshot member (entity, pos, id, fear) once for targeting — a read-only pass; the mutable calm pass
    // is below (`.iter()` on a `&mut Drives` query still yields read-only `&Drives`).
    let members: Vec<(Entity, Vec3, usize, f32)> = units
        .iter()
        .map(|(e, tf, m, d)| (e, tf.translation, m.0, d.get(DriveId::FEAR)))
        .collect();

    // Move each blob toward its most-anxious member (or hold + tickle if already in contact).
    for (mut tf, mut motion) in &mut blobs {
        let pos = tf.translation;
        // Build the total-order key per member and pick deterministically (see `pick_most_anxious`).
        let keyed: Vec<(f32, f32, usize)> = members
            .iter()
            .map(|&(_, mpos, member, fear)| (fear, (mpos.xz() - pos.xz()).length(), member))
            .collect();
        let Some(idx) = pick_most_anxious(&keyed) else {
            // No squad members to comfort — hold still (a lonely blob).
            motion.target = None;
            motion.moving = false;
            motion.tickling = false;
            continue;
        };
        let (target_entity, target, _member, _fear) = members[idx];
        let dist = keyed[idx].1;
        motion.target = Some(target_entity);
        motion.gaze = target;

        let seeking = dist > reach;
        motion.tickling = !seeking;
        motion.moving = seeking;

        // Steer = pursuit (toward the member, only while seeking) + a wall-standoff push. SCP-999's
        // COLLISION box (`SCP999_HALF`) is far narrower than its visible gel dome (~0.8 m half-width), so
        // `resolve_move` alone keeps only the *centre* out of walls and the wide body still pokes through
        // near a wall (the reported "cutting through the wall"). The standoff push steers the centre off
        // nearby wall faces so the dome's edge clears — the exact wide-body/narrow-collider fix
        // `enemy::enemy_seek` uses for the boss's billboard. It runs whether seeking OR tickling, so a blob
        // pressed against a wall while comforting a wall-hugging member still drifts its body clear (the
        // generous `contact_radius` keeps it in tickle range as it does). Opposite walls cancel, so it
        // still funnels straight through a corridor rather than stalling.
        let base = if seeking { (target.xz() - pos.xz()).normalize_or_zero() } else { Vec2::ZERO };
        let push = wall_standoff_push(&dungeon, pos, WALL_STANDOFF, WALL_PUSH_GAIN);
        let dir = (base + push).normalize_or_zero();
        if dir != Vec2::ZERO {
            let dir3 = Vec3::new(dir.x, 0.0, dir.y);
            let resolved = dungeon.resolve_move(pos, dir3 * (t.move_speed * dt), SCP999_HALF);
            tf.translation.x = resolved.x;
            tf.translation.z = resolved.z;
        }
    }

    // Tickle-calm: every member within `reach` of ANY blob gets calmed this tick (order-independent — see
    // the determinism note above). Snapshot blob positions after the move so contact uses this tick's pose.
    let reach_sq = reach * reach;
    let blob_positions: Vec<Vec3> = blobs.iter().map(|(tf, _)| tf.translation).collect();
    for (_entity, utf, _member, mut drives) in &mut units {
        let touched = blob_positions
            .iter()
            .any(|bp| (bp.xz() - utf.translation.xz()).length_squared() <= reach_sq);
        if touched {
            let fear = drives.get(DriveId::FEAR);
            drives.set(DriveId::FEAR, fear - t.calm_rate * dt); // `set` clamps to [0,1] (no underflow)
            let morale = drives.get(DriveId::MORALE);
            drives.set(DriveId::MORALE, morale + t.morale_rate * dt);
        }
    }
}

/// The blob steers off walls by exactly the boss's rule (a wide gel body around a narrow collider — the
/// identical wide-visible-body problem), so it calls the one implementation rather than keeping a second
/// copy: [`crate::enemy::wall_standoff_push`], which carries the shared unit test.
use crate::enemy::wall_standoff_push;

/// Index of the most-anxious member under the **total order** `(FEAR desc, planar-dist asc, SquadMember
/// asc)`, given `(fear, dist, member)` per candidate. Pure so the determinism-critical pick is unit-tested
/// without an ECS: FEAR ties break to the nearest, a dist tie breaks to the lowest (unique) `SquadMember`,
/// so the winner never depends on slice/query order — the rule of `util::nearest_planar`. `None` if empty.
fn pick_most_anxious(candidates: &[(f32, f32, usize)]) -> Option<usize> {
    let mut best: Option<(usize, f32, f32, usize)> = None; // (idx, fear, dist, member)
    for (i, &(fear, dist, member)) in candidates.iter().enumerate() {
        let better = match best {
            None => true,
            Some((_, bf, bd, bm)) => {
                fear > bf || (fear == bf && dist < bd) || (fear == bf && dist == bd && member < bm)
            }
        };
        if better {
            best = Some((i, fear, dist, member));
        }
    }
    best.map(|(i, ..)| i)
}

#[cfg(test)]
mod tests {
    // Pure target-pick math — no App, no ECS (the seed-in/assert-out convention of `wfc.rs`/`drives.rs`).
    use super::pick_most_anxious;

    /// The winning member id (3rd tuple field), for order-independence assertions.
    fn winner_member(c: &[(f32, f32, usize)]) -> Option<usize> {
        pick_most_anxious(c).map(|i| c[i].2)
    }

    #[test]
    fn highest_fear_wins() {
        // (fear, dist, member). The blob triages the *most* frightened member, not the nearest.
        let c = [(0.2, 5.0, 0), (0.9, 3.0, 1), (0.5, 1.0, 2)];
        assert_eq!(winner_member(&c), Some(1));
    }

    #[test]
    fn fear_tie_breaks_to_the_nearest() {
        let c = [(0.5, 3.0, 0), (0.5, 1.0, 1)];
        assert_eq!(winner_member(&c), Some(1));
    }

    #[test]
    fn fear_and_dist_tie_breaks_to_the_lowest_member() {
        // Bit-identical fear AND distance (the crab-clamp trap: real actors do land on identical floats),
        // so the unique SquadMember is the total-order tiebreak — never slice/query order.
        let c = [(0.5, 2.0, 7), (0.5, 2.0, 3)];
        assert_eq!(winner_member(&c), Some(3));
    }

    #[test]
    fn pick_is_independent_of_slice_order() {
        // The pick feeds the blob's target → its movement (hashed via the tickled unit's motion), so a
        // reordered candidate slice must never change WHO is chosen.
        let a = [(0.5, 2.0, 7), (0.5, 2.0, 3), (0.9, 4.0, 5)];
        let b = [(0.9, 4.0, 5), (0.5, 2.0, 3), (0.5, 2.0, 7)];
        assert_eq!(winner_member(&a), winner_member(&b));
        assert_eq!(winner_member(&a), Some(5)); // highest fear regardless of order
    }

    #[test]
    fn no_members_no_target() {
        assert_eq!(pick_most_anxious(&[]), None);
    }
}
