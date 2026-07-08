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
const ANCHOR_EASE: f32 = 4.0;

/// Recompute the squad anchor from the living units' centroid, eased for smoothness. Runs on
/// `FixedUpdate` before the squad decision (`squad_think`), which reads `anchor_dist`.
pub fn update_anchor(
    time: Res<Time>,
    mut anchor: ResMut<SquadAnchor>,
    units: Query<&Transform, With<Unit>>,
) {
    let mut sum = Vec3::ZERO;
    let mut n = 0u32;
    for t in &units {
        sum += t.translation;
        n += 1;
    }
    if n == 0 {
        anchor.valid = false;
        return;
    }
    let centroid = sum / n as f32;
    if !anchor.valid {
        // First observation: place the anchor on the centroid so it doesn't ease in from the origin.
        anchor.pos = centroid;
        anchor.vel = Vec3::ZERO;
        anchor.valid = true;
        return;
    }
    let dt = time.delta_secs();
    let prev = anchor.pos;
    let k = (ANCHOR_EASE * dt).min(1.0);
    anchor.pos = prev + (centroid - prev) * k;
    // Smoothed drift (per-second) for alignment consumers.
    if dt > 0.0 {
        anchor.vel = (anchor.pos - prev) / dt;
    }
}
