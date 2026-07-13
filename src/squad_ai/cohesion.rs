//! Squad **cohesion** — the "wander off but stay together" group model. A virtual [`SquadAnchor`] (the
//! moving centroid of the living squad) is the group's shared reference point; each unit's role brain
//! pulls it back toward the anchor once it strays past the leash (Behaviour `Regroup`), and otherwise
//! drifts loosely with it (`FollowAnchor`). This is the group-entity pattern of Game AI Pro 2 Ch.20
//! ("Hierarchical Architecture for Group Navigation Behaviors"): members read the group's position
//! rather than obey a physical leader, so individuals can peel off for role work and re-converge.
//! Reynolds separation is already provided by the ORCA reciprocal-avoidance layer, so cohesion only
//! needs to supply a goal point; ORCA keeps units from overlapping (Reynolds, "Flocks, Herds and
//! Schools", SIGGRAPH 1987, DOI 10.1145/37402.37406).

use bevy::prelude::*;

use crate::squad::Unit;

/// How the squad is driven. Defaults to fully autonomous (the AI runs everything); a one-line config
/// change flips to `BetweenOrders` (RTS: the AI only fills idle time between player move orders) or
/// `ControlOne` (the player drives the leader, the rest are AI teammates). The gate is applied in the
/// locomotion planner, so switching mode never touches the decision or perception layers.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SquadControlMode {
    /// AI plans a goal for every order-less unit (the default).
    Autonomous,
    /// Same mechanics, but framed as filler: the AI acts only where the player has issued no order
    /// (identical to `Autonomous` for an order-less unit; distinct once player orders are frequent).
    BetweenOrders,
    /// The leader (squad member 0) is player-driven; the other four are AI teammates.
    ControlOne,
}

impl Default for SquadControlMode {
    fn default() -> Self {
        SquadControlMode::Autonomous
    }
}

/// The squad's virtual group reference point (Game AI Pro 2 Ch.20). Position eases toward the living
/// squad's centroid so it lags slightly and reads as a smooth "where the group is" rather than a jumpy
/// mean; velocity is the smoothed drift used for alignment.
#[derive(Resource, Default)]
pub struct SquadAnchor {
    pub pos: Vec3,
    pub vel: Vec3,
    /// False until at least one unit exists (so consumers can skip cohesion on the first frame).
    pub valid: bool,
}

/// A unit's autonomous movement goal, written by the squad locomotion planner from its chosen
/// behaviour and consumed by `squad::unit_movement` when the unit has no player `MoveOrder`. `None`
/// means "hold position" (idle). Kept separate from `MoveOrder` so the authoritative player-order path
/// in `unit_movement` is untouched.
#[derive(Component, Default)]
pub struct DesiredMove {
    pub goal: Option<Vec3>,
}

/// How fast the anchor eases toward the squad centroid (per second). A gentle lag (not a hard snap) so
/// the group reference is smooth.
// ANCHOR_EASE (cohesion anchor-tracking ease rate) now lives in the `behavior:` config slice
// (`BehaviorTuning::squad_move::anchor_ease`), read as `Res<BehaviorTuning>`. See src/behavior_tuning.rs.

/// The mean of `positions`, summed in a CANONICAL (value-sorted) order rather than raw iteration order
/// — `None` if empty. Float addition is non-associative, so summing in entity-iteration order would
/// make the centroid's low bits depend on ECS iteration order (which can shift between two same-seed
/// runs as units change archetype). Sorting the addends by bit pattern makes the sum a pure function of
/// the position SET, so the anchor — which feeds movement goals → hashed Transforms — is reproducible.
/// Same value-sort determinism idiom as `sim_harness::snapshot_hash`. Pure + unit-tested for exactly
/// this order-independence.
fn deterministic_centroid(positions: Vec<Vec3>) -> Option<Vec3> {
    if positions.is_empty() {
        return None;
    }
    let n = positions.len();
    let mut keyed: Vec<[u32; 3]> =
        positions.into_iter().map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).collect();
    keyed.sort_unstable();
    let mut sum = Vec3::ZERO;
    for k in &keyed {
        sum += Vec3::new(f32::from_bits(k[0]), f32::from_bits(k[1]), f32::from_bits(k[2]));
    }
    Some(sum / n as f32)
}

/// Recompute the squad anchor from the living units' centroid, eased for smoothness. Runs on
/// `FixedUpdate` before the squad decision (`squad_think`), which reads `anchor_dist`.
pub fn update_anchor(
    time: Res<Time>,
    mut anchor: ResMut<SquadAnchor>,
    units: Query<&Transform, With<Unit>>,
    beh: Res<crate::behavior_tuning::BehaviorTuning>,
) {
    let positions: Vec<Vec3> = units.iter().map(|t| t.translation).collect();
    let Some(centroid) = deterministic_centroid(positions) else {
        anchor.valid = false;
        return;
    };
    if !anchor.valid {
        // First observation: place the anchor on the centroid so it doesn't ease in from the origin.
        anchor.pos = centroid;
        anchor.vel = Vec3::ZERO;
        anchor.valid = true;
        return;
    }
    let dt = time.delta_secs();
    let prev = anchor.pos;
    let k = (beh.squad_move.anchor_ease * dt).min(1.0);
    anchor.pos = prev + (centroid - prev) * k;
    // Smoothed drift (per-second) for alignment consumers.
    if dt > 0.0 {
        anchor.vel = (anchor.pos - prev) / dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centroid_is_none_when_empty() {
        assert_eq!(deterministic_centroid(Vec::new()), None);
    }

    #[test]
    fn centroid_is_bit_identical_regardless_of_input_order() {
        // Values chosen so the naive left-to-right float sum is order-sensitive in its low bits (a big
        // magnitude next to small ones loses precision differently depending on order). The canonical
        // value-sort must make every permutation produce the EXACT same bits — the property the
        // determinism gate relies on for the squad anchor.
        let base = vec![
            Vec3::new(1_000_000.0, 0.0, -3.3),
            Vec3::new(0.1, 2.0, 7.7),
            Vec3::new(0.2, -5.0, 0.0),
            Vec3::new(-9.9, 1.0, 1_000_000.0),
            Vec3::new(0.3, 0.25, -0.75),
        ];
        let reference = deterministic_centroid(base.clone()).expect("non-empty");
        // Every rotation + a reversal must hash-identically to the reference.
        for shift in 0..base.len() {
            let mut permuted = base.clone();
            permuted.rotate_left(shift);
            let c = deterministic_centroid(permuted).expect("non-empty");
            assert_eq!(c.to_array().map(f32::to_bits), reference.to_array().map(f32::to_bits),
                "rotation by {shift} changed the centroid bits");
        }
        let mut reversed = base.clone();
        reversed.reverse();
        let c = deterministic_centroid(reversed).expect("non-empty");
        assert_eq!(c.to_array().map(f32::to_bits), reference.to_array().map(f32::to_bits),
            "reversal changed the centroid bits");
    }
}
