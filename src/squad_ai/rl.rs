//! **RL readiness** — the learned-controller side of the policy seam, plus the data plumbing to train
//! and evaluate one. [`RemotePolicy`] is a [`SquadPolicy`] driven by an external trainer over the same
//! `(Observation, Action)` space as the hand-authored brain (Bergdahl et al. 2021). [`TrajectoryLog`]
//! records `(obs, action, reward)` from deterministic-core rollouts (repeatable, so a trainer can
//! replay them), and [`novelty_reward`] is the curiosity signal that rewards visiting fresh game state
//! — the driver of exploratory, "interesting" behaviour. It is a count-based state-visitation bonus
//! (Bellemare et al., "Unifying Count-Based Exploration and Intrinsic Motivation", 2016; Gordillo et al.,
//! "Improving Playtesting Coverage via Curiosity Driven RL", 2021), NOT the forward-model prediction
//! error of Pathak et al. 2017 — that paper's curiosity is the contrasting approach, and it explicitly
//! classifies tabular visitation counts as a different method it argues against.

use std::collections::HashMap;
use std::sync::Mutex;

use bevy::prelude::{IVec2, Resource};

use crate::ai::utility::{Behavior, Mode, Perception};

use super::policy::SquadPolicy;
use super::role::RoleId;

/// A policy driven by an external reinforcement-learning process. The trainer pushes one action index
/// per unit per step; between steps (or before the trainer has stepped) the unit **holds** — it
/// selects the [`Mode::Wander`] safety default, the one behaviour that is a true no-op (`resolve_goal`
/// maps it to no movement and `unit_actions` fires no effect for it). NOT index 0: in a role brain
/// index 0 is the rank-4 role DUTY (Examine / TendWounded / SecureDoor …), several of which move and
/// mutate world state, so "hold = index 0" would make an un-stepped controller silently perform each
/// unit's primary duty. Holding is a no-op action on the RL path, not a fallback to a different policy
/// — the one-path rule holds.
#[derive(Default)]
pub struct RemotePolicy {
    queue: Mutex<std::collections::VecDeque<usize>>,
}

/// The behaviour index a held unit selects: the unconditional [`Mode::Wander`] safety default (a true
/// stationary no-op). Falls back to the last behaviour — the tail's conventional safety-default slot —
/// only if a brain authors no Wander at all, keeping the return in range without another policy path.
fn hold_index(behaviors: &[Behavior]) -> usize {
    behaviors
        .iter()
        .position(|b| b.mode == Mode::Wander)
        .unwrap_or_else(|| behaviors.len().saturating_sub(1))
}

impl RemotePolicy {
    /// The trainer enqueues the next action index (clamped to range at read time).
    pub fn push_action(&self, index: usize) {
        if let Ok(mut q) = self.queue.lock() {
            q.push_back(index);
        }
    }
}

impl SquadPolicy for RemotePolicy {
    fn choose(&self, _perc: &Perception, behaviors: &[Behavior], _role: RoleId, _rng: &mut u32) -> usize {
        match self.queue.lock().ok().and_then(|mut q| q.pop_front()) {
            // A queued action index, clamped into range.
            Some(idx) => idx.min(behaviors.len().saturating_sub(1)),
            // Nothing queued → hold (the Wander no-op), NOT index 0 (a role duty).
            None => hold_index(behaviors),
        }
    }
}

/// Novelty (curiosity) reward for visiting a cell that has been seen `visits` times before: high for
/// the first visit, decaying as the cell becomes familiar — a count-based state-visitation bonus
/// (Bellemare et al. 2016; Gordillo et al. 2021). `visits = 0` → reward 1.0.
pub fn novelty_reward(visits: u32) -> f32 {
    1.0 / ((visits as f32) + 1.0).sqrt()
}

/// Count of how often each dungeon cell has been occupied by the squad — the state-visitation table
/// behind [`novelty_reward`]. A shared coverage map for RL reward + playtesting analysis.
#[derive(Resource, Default)]
pub struct Visitation {
    counts: HashMap<IVec2, u32>,
}

impl Visitation {
    /// Record a visit and return the novelty reward *before* this visit (so the first visit pays 1.0).
    pub fn visit(&mut self, cell: IVec2) -> f32 {
        let c = self.counts.entry(cell).or_insert(0);
        let reward = novelty_reward(*c);
        *c += 1;
        reward
    }

    /// Number of distinct cells the squad has ever occupied — a coverage metric for QD descriptors.
    pub fn coverage(&self) -> usize {
        self.counts.len()
    }
}

/// One logged step for offline RL / analysis.
#[derive(Clone)]
pub struct Sample {
    pub tick: u64,
    pub unit: u32,
    pub obs: Vec<f32>,
    pub action: Mode,
    pub reward: f32,
}

/// Trajectory buffer, filled from deterministic-core rollouts when `enabled`. Off by default so the
/// live game pays nothing; a headless training/eval harness flips it on.
#[derive(Resource, Default)]
pub struct TrajectoryLog {
    pub enabled: bool,
    pub samples: Vec<Sample>,
}

impl TrajectoryLog {
    pub fn record(&mut self, sample: Sample) {
        if self.enabled {
            self.samples.push(sample);
        }
    }

    /// Serialise to JSON Lines for an external trainer (one object per step). Hand-formatted to avoid a
    /// serde_json dependency; the obs vector is compact.
    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        for s in &self.samples {
            let obs: Vec<String> = s.obs.iter().map(|x| format!("{x:.4}")).collect();
            out.push_str(&format!(
                "{{\"tick\":{},\"unit\":{},\"action\":\"{:?}\",\"reward\":{:.4},\"obs\":[{}]}}\n",
                s.tick,
                s.unit,
                s.action,
                s.reward,
                obs.join(",")
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::utility::SquadFields;
    use crate::squad_ai::role::default_behaviors_for_test;
    use crate::squad_ai::role::RoleId;
    use bevy::math::Vec3;

    fn perc() -> Perception {
        Perception {
            pos: Vec3::ZERO,
            nearest_unit: None,
            nearest_dist: 999.0,
            health_frac: 1.0,
            drives: [0.0; crate::ai::drives::DRIVE_COUNT],
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
            squad: SquadFields::neutral(),
            water: crate::ai::utility::WaterObs::default(),
        }
    }

    #[test]
    fn remote_policy_replays_queued_actions_then_holds() {
        let behaviors = default_behaviors_for_test(RoleId::Medic);
        let p = RemotePolicy::default();
        p.push_action(2);
        p.push_action(999); // clamped
        let mut rng = 1u32;
        assert_eq!(p.choose(&perc(), &behaviors, RoleId::Medic, &mut rng), 2);
        assert_eq!(p.choose(&perc(), &behaviors, RoleId::Medic, &mut rng), behaviors.len() - 1);
        // Empty → HOLD, which must be the Wander no-op, NOT index 0 (the Medic's index 0 is
        // TendWounded — walking to and healing an ally — so a held controller must not select it).
        let held = p.choose(&perc(), &behaviors, RoleId::Medic, &mut rng);
        assert_eq!(behaviors[held].mode, Mode::Wander, "hold must be the Wander no-op, not a duty");
        assert_ne!(held, 0, "the Medic's index 0 (TendWounded) is not a hold");
    }

    #[test]
    fn novelty_decays_with_familiarity() {
        assert!((novelty_reward(0) - 1.0).abs() < 1e-6);
        assert!(novelty_reward(0) > novelty_reward(3));
        assert!(novelty_reward(3) > novelty_reward(99));
    }

    #[test]
    fn visitation_pays_first_visit_most() {
        let mut v = Visitation::default();
        let first = v.visit(IVec2::new(1, 1));
        let second = v.visit(IVec2::new(1, 1));
        assert!(first > second);
        assert_eq!(v.coverage(), 1);
        v.visit(IVec2::new(2, 2));
        assert_eq!(v.coverage(), 2);
    }

    #[test]
    fn trajectory_log_respects_enabled_and_serialises() {
        let mut log = TrajectoryLog::default();
        log.record(Sample { tick: 0, unit: 0, obs: vec![0.5, 1.0], action: Mode::Examine, reward: 1.0 });
        assert!(log.samples.is_empty()); // disabled → dropped
        log.enabled = true;
        log.record(Sample { tick: 1, unit: 2, obs: vec![0.25], action: Mode::Flee, reward: 0.3 });
        let jsonl = log.to_jsonl();
        assert!(jsonl.contains("\"action\":\"Flee\""));
        assert!(jsonl.contains("\"unit\":2"));
    }
}
