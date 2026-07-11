//! The **policy seam** — the single interface through which a unit turns perception into a chosen
//! behaviour. The hand-authored dual-utility role brain ([`UtilityPolicy`]) is *one* implementation;
//! a learned controller ([`crate::squad_ai::rl::RemotePolicy`]) is another, over the **same**
//! `(Observation, Action)` space. This is what makes the squad RL-ready: swapping the decision layer
//! is a config choice, not a rewrite (Bergdahl et al., "Augmenting Automated Game Testing with Deep
//! Reinforcement Learning", 2021 — scripted and RL agents behind one interface; Wu et al., HMS MOBA
//! 2019 — the discrete high-level action is the RL "option").
//!
//! One path per configuration (per the project's no-fallback rule): a build selects a policy; there
//! is no runtime "try RL, fall back to utility".

use bevy::prelude::Resource;

use crate::ai::drives::DRIVE_COUNT;
use crate::ai::utility::{decide, Behavior, Mode, Perception, TargetKind};

use super::role::RoleId;

/// The selected decision policy for the whole squad, inserted at startup by config (one path — no
/// runtime fallback). `squad_think` routes every unit's decision through it, so a build swaps the
/// hand-authored [`UtilityPolicy`] for a learned controller without touching perception or execution.
#[derive(Resource)]
pub struct ActivePolicy(pub Box<dyn SquadPolicy>);

impl Default for ActivePolicy {
    fn default() -> Self {
        ActivePolicy(Box::new(UtilityPolicy))
    }
}

/// Distance normaliser for the observation vector (world units → ~[0,1] over a room-ish span).
const DIST_SCALE: f32 = 32.0;

/// The fixed-layout feature view an RL policy consumes. Built from a [`Perception`] + the unit's role.
/// `to_vec` is the observation tensor; the struct fields keep it self-documenting.
#[derive(Clone, Debug)]
pub struct Observation {
    pub role: RoleId,
    pub health_frac: f32,
    pub drives: [f32; DRIVE_COUNT],
    pub nearest_threat_dist: f32,
    pub anchor_dist: f32,
    pub examinable_dist: f32,
    pub has_unexamined: f32,
    pub wounded_ally_dist: f32,
    pub ally_down: f32,
    pub threat_bearing_known: f32,
    pub anomaly_residue: f32,
    pub seen_by_squad: f32,
}

impl Observation {
    pub fn from_perception(perc: &Perception, role: RoleId) -> Self {
        Observation {
            role,
            health_frac: perc.health_frac,
            drives: perc.drives,
            nearest_threat_dist: perc.nearest_dist,
            anchor_dist: perc.squad.anchor_dist,
            examinable_dist: perc.squad.examinable_dist,
            has_unexamined: perc.squad.has_unexamined,
            wounded_ally_dist: perc.squad.wounded_ally_dist,
            ally_down: perc.squad.ally_down,
            threat_bearing_known: perc.squad.threat_bearing_known,
            anomaly_residue: perc.squad.anomaly_residue,
            seen_by_squad: perc.seen_by_squad,
        }
    }

    /// The observation tensor: role one-hot ++ drives ++ scalar features, distances squashed to ~[0,1].
    pub fn to_vec(&self) -> Vec<f32> {
        let mut v = Vec::with_capacity(RoleId::ALL.len() + DRIVE_COUNT + 8);
        for r in RoleId::ALL {
            v.push(if r == self.role { 1.0 } else { 0.0 });
        }
        v.extend_from_slice(&self.drives);
        let squash = |d: f32| (d / DIST_SCALE).clamp(0.0, 1.0);
        v.push(self.health_frac);
        v.push(squash(self.nearest_threat_dist));
        v.push(squash(self.anchor_dist));
        v.push(squash(self.examinable_dist));
        v.push(self.has_unexamined);
        v.push(squash(self.wounded_ally_dist));
        v.push(self.ally_down);
        v.push(self.threat_bearing_known);
        v.push(self.anomaly_residue);
        v.push(self.seen_by_squad);
        v
    }

    /// The length of [`Observation::to_vec`] output — the RL observation dimensionality.
    pub const fn dim() -> usize {
        RoleId::ALL.len() + DRIVE_COUNT + 10
    }
}

/// The chosen high-level action: a movement/effect mode plus where it aims. This is the RL action
/// space (a discrete `Mode` + a symbolic `TargetKind`).
#[derive(Clone, Copy, Debug)]
pub struct Action {
    pub mode: Mode,
    pub target: TargetKind,
}

/// The decision interface. Returns the index of the chosen behaviour within `behaviors` (so the caller
/// resolves the concrete aim point the same way for every policy), keeping perception, target
/// resolution, and execution shared across hand-authored and learned controllers.
pub trait SquadPolicy: Send + Sync {
    fn choose(&self, perc: &Perception, behaviors: &[Behavior], rng: &mut u32) -> usize;
}

/// The default policy: the dual-utility role brain (Dill). Deterministic given the per-unit RNG.
pub struct UtilityPolicy;

impl SquadPolicy for UtilityPolicy {
    fn choose(&self, perc: &Perception, behaviors: &[Behavior], rng: &mut u32) -> usize {
        decide(behaviors, perc, rng)
    }
}

/// A fixed-choice policy for tests and scripted scenarios — always selects a given behaviour index
/// (clamped into range), bypassing utility scoring.
pub struct ScriptedPolicy {
    pub index: usize,
}

impl SquadPolicy for ScriptedPolicy {
    fn choose(&self, _perc: &Perception, behaviors: &[Behavior], _rng: &mut u32) -> usize {
        self.index.min(behaviors.len().saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::utility::SquadFields;
    use crate::squad_ai::role::default_behaviors_for_test;
    use bevy::math::Vec3;

    fn zeroed() -> Perception {
        Perception {
            pos: Vec3::ZERO,
            nearest_unit: None,
            nearest_dist: 999.0,
            health_frac: 1.0,
            drives: [0.0; DRIVE_COUNT],
            scent_hotspot: Vec3::ZERO,
            scent_val: 0.0,
            meat_hotspot: Vec3::ZERO,
            meat_val: 0.0,
            carrying: 0.0,
            prey_spotted: 0.0,
            rally_val: 0.0,
            alarm_val: 0.0,
            seen_by_squad: 0.0,
            noise_draw: 0.0,
            squad: SquadFields { anchor_dist: 0.0, ..SquadFields::neutral() },
        }
    }

    #[test]
    fn observation_vector_has_stable_dim() {
        let obs = Observation::from_perception(&zeroed(), RoleId::Gunman);
        assert_eq!(obs.to_vec().len(), Observation::dim());
    }

    #[test]
    fn utility_policy_matches_engine_decide() {
        let behaviors = default_behaviors_for_test(RoleId::Researcher);
        let mut p = zeroed();
        p.squad.has_unexamined = 1.0;
        p.drives[crate::ai::drives::DriveId::CURIOSITY.0] = 0.8;
        let policy = UtilityPolicy;
        let mut r1 = 7u32;
        let mut r2 = 7u32;
        let a = policy.choose(&p, &behaviors, &mut r1);
        let b = decide(&behaviors, &p, &mut r2);
        assert_eq!(a, b);
        assert_eq!(behaviors[a].mode, Mode::Examine);
    }

    #[test]
    fn scripted_policy_is_clamped() {
        let behaviors = default_behaviors_for_test(RoleId::Medic);
        let policy = ScriptedPolicy { index: 999 };
        let p = zeroed();
        let mut rng = 1u32;
        let idx = policy.choose(&p, &behaviors, &mut rng);
        assert_eq!(idx, behaviors.len() - 1);
    }
}
