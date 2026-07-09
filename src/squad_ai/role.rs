//! Squad **roles** — the SCP Mobile-Task-Force archetypes (Gunman, Researcher, Psionic, Medic,
//! Engineer). A role is nothing but a **repertoire of dual-utility behaviours** (Dill, "Dual-Utility
//! Reasoning", Game AI Pro 2 Ch.3) over the shared [`crate::ai::utility`] engine — exactly the crab/boss
//! pattern, but for units. The repertoire is a data literal here (compile-safe defaults) AND
//! deserialisable from `assets/config/roles.ron`, so a designer retunes or adds a role without touching
//! code (Jacopin, "Optimizing Practical Planning for Game AI", Game AI Pro 2 Ch.13 — actions as text
//! files). The stereotype comes from the ranked behaviour set; the customisation is the RON override.
//!
//! Rank convention (absolute-utility buckets, highest wins outright): survival `Flee` (5) > role duty
//! (4) > secondary duty / support (2–3) > cohesion `Regroup` (3) > `FollowAnchor` (1) > `Wander` (0).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::Deserialize;

use crate::ai::brain::Brain;
use crate::ai::drives::{DriveId, DRIVE_COUNT};
use crate::ai::utility::{Behavior, Consideration, Curve, Fact, Input, Mode, TargetKind};

/// Which SCP task-force role a unit plays. `Deserialize`/`Hash`/`Eq` so it keys the `roles.ron` map and
/// the [`RoleBrains`] registry.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug, Deserialize)]
pub enum RoleId {
    Gunman,
    Researcher,
    Psionic,
    Medic,
    Engineer,
}

impl RoleId {
    /// Every role, in spawn order (index-matched to the five squad members).
    pub const ALL: [RoleId; 5] = [
        RoleId::Gunman,
        RoleId::Researcher,
        RoleId::Psionic,
        RoleId::Medic,
        RoleId::Engineer,
    ];
}

/// A role's authored repertoire, as it appears in `roles.ron`. One list of behaviours — the same
/// [`Behavior`] literal the engine already scores, now `Deserialize`.
#[derive(Deserialize)]
pub struct RoleDef {
    pub behaviors: Vec<Behavior>,
}

/// The active brain per role, built at startup (the behaviour extension point — mirrors
/// [`crate::ai::brain::AiBrains`] for creatures). Populated from the code-literal defaults, then
/// overlaid by any roles present in `roles.ron`.
#[derive(Resource)]
pub struct RoleBrains {
    brains: HashMap<RoleId, Brain>,
}

impl RoleBrains {
    /// The compile-safe defaults for all five roles (no config needed — the game runs without a
    /// `roles.ron`, and RON only *overrides*).
    pub fn defaults() -> Self {
        let mut brains = HashMap::new();
        for role in RoleId::ALL {
            brains.insert(role, Brain { behaviors: default_behaviors(role) });
        }
        RoleBrains { brains }
    }

    /// Overlay parsed RON role definitions onto the defaults (a role absent from the file keeps its
    /// default; a present role is fully replaced — one path, no per-behaviour merge).
    pub fn overlay(&mut self, defs: HashMap<RoleId, RoleDef>) {
        for (role, def) in defs {
            self.brains.insert(role, Brain { behaviors: def.behaviors });
        }
    }

    pub fn get(&self, role: RoleId) -> &Brain {
        // `defaults()` inserts every `RoleId::ALL` role, and `overlay` only REPLACES existing entries,
        // so a lookup for any role in ALL is always present. A miss means ALL is out of sync with the
        // `RoleId` enum (a variant was added without extending ALL): fail loud with a message naming
        // the cause — never silently substitute another role's brain (a wrong-behaviour bug the
        // one-path/no-fallback rule forbids) or index-panic on a bare `[]`. This can't fire in the
        // shipped game (all five roles are in ALL); it is a developer invariant guard.
        self.brains.get(&role).unwrap_or_else(|| {
            panic!("no brain registered for {role:?}; RoleId::ALL must list every RoleId variant")
        })
    }
}

/// Parse a `roles.ron` document: `{ Gunman: (behaviors: [ ... ]), ... }`.
pub fn parse_roles_ron(src: &str) -> Result<HashMap<RoleId, RoleDef>, ron::error::SpannedError> {
    ron::from_str(src)
}

/// Validate parsed `roles.ron` role definitions before they overlay the defaults — the "reject bad
/// input at the door" gate (per `~/.claude/CLAUDE.md`: fail loudly, one path, no silent defaults).
/// Two invariants make a data-authored brain safe for the engine, which the code-literal defaults hold
/// by construction but a hand-edited RON file can break (returns a human-readable error naming the
/// offending role so the author sees exactly what to fix):
///
/// 1. **Non-empty behaviours.** `decide` returns index 0 when nothing scores and `squad_think` indexes
///    `behaviors[idx]`, so an empty list index-panics on the unit's first think.
/// 2. **In-range drive indices.** `Perception::read` evaluates `self.drives[id.0]` unchecked; a
///    `Drive((n))` with `n >= DRIVE_COUNT` (now data-authorable via `Deserialize`) index-panics at the
///    first decision that scores it.
pub fn validate_role_defs(defs: &HashMap<RoleId, RoleDef>) -> Result<(), String> {
    for (role, def) in defs {
        if def.behaviors.is_empty() {
            return Err(format!(
                "role {role:?} has an empty `behaviors` list; a role must define at least one \
                 behaviour (e.g. a Wander safety default)"
            ));
        }
        for (i, behavior) in def.behaviors.iter().enumerate() {
            for consideration in &behavior.considerations {
                if let Input::Drive(id) = consideration.input
                    && id.0 >= DRIVE_COUNT
                {
                    return Err(format!(
                        "role {role:?} behaviour #{i} references Drive(({})) but only \
                         {DRIVE_COUNT} drives exist (valid indices 0..{DRIVE_COUNT})",
                        id.0
                    ));
                }
            }
        }
    }
    Ok(())
}

// --- Shared behaviour fragments (every role carries these tails) ---

/// Distance (world units) past which a strayed unit is strongly pulled back to the anchor — the
/// "leash" of the wander-but-cohere group model (Game AI Pro 2 Ch.20, "Hierarchical Architecture for
/// Group Navigation Behaviors"; Moussaïd et al. 2010 field-of-view cohesion).
pub const LEASH: f32 = 6.0;

/// Survival: retreat when fear spikes — the top bucket for every role.
fn flee() -> Behavior {
    Behavior {
        mode: Mode::Flee,
        rank: 5,
        target: TargetKind::None,
        considerations: vec![Consideration {
            input: Input::Drive(DriveId::FEAR),
            curve: Curve::Logistic { k: 10.0, x0: 0.5 },
        }],
    }
}

/// Cohesion pull: return toward the squad anchor once past the leash. A soft threshold on anchor
/// distance, so a unit doing role work nearby is un-pulled but a strayed one snaps back.
fn regroup() -> Behavior {
    Behavior {
        mode: Mode::Regroup,
        rank: 3,
        target: TargetKind::SquadAnchor,
        considerations: vec![Consideration {
            input: Input::Perc(Fact::AnchorDist),
            curve: Curve::Logistic { k: 1.0, x0: LEASH },
        }],
    }
}

/// Default drift: loosely follow the moving anchor (constant low weight, always available).
fn follow_anchor() -> Behavior {
    Behavior {
        mode: Mode::FollowAnchor,
        rank: 1,
        target: TargetKind::SquadAnchor,
        considerations: vec![Consideration {
            input: Input::Perc(Fact::SelfHealthFrac),
            curve: Curve::Linear { m: 0.0, b: 0.3 },
        }],
    }
}

/// Safety default so a choice always exists (Dill: include an unconditional low-rank option).
fn wander() -> Behavior {
    Behavior {
        mode: Mode::Wander,
        rank: 0,
        target: TargetKind::None,
        considerations: vec![Consideration {
            input: Input::Perc(Fact::SelfHealthFrac),
            curve: Curve::Linear { m: 0.0, b: 0.12 },
        }],
    }
}

/// The tail every role shares: flee, regroup, follow, wander.
fn tail() -> Vec<Behavior> {
    vec![flee(), regroup(), follow_anchor(), wander()]
}

/// A `Step` gate that turns a boolean-ish fact into a hard on/off consideration.
fn gate(fact: Fact) -> Consideration {
    Consideration {
        input: Input::Perc(fact),
        curve: Curve::Step { threshold: 0.5, below: 0.0, above: 1.0 },
    }
}

/// Test/harness accessor for a role's default repertoire (the private builder is the production path).
pub fn default_behaviors_for_test(role: RoleId) -> Vec<Behavior> {
    default_behaviors(role)
}

/// The code-literal default repertoire for a role (role-specific behaviours, then the shared tail).
fn default_behaviors(role: RoleId) -> Vec<Behavior> {
    let mut b: Vec<Behavior> = match role {
        // Gunman: hold and priority-fire when a threat bearing is known (Overwatch), else advance to
        // contact (Engage, a rank below so it holds by default). Combat sits just under survival.
        RoleId::Gunman => vec![
            Behavior {
                mode: Mode::Overwatch,
                rank: 4,
                target: TargetKind::TrackedThreat,
                considerations: vec![gate(Fact::ThreatBearingKnown)],
            },
            Behavior {
                mode: Mode::Engage,
                rank: 2,
                target: TargetKind::TrackedThreat,
                considerations: vec![gate(Fact::ThreatBearingKnown)],
            },
        ],
        // Researcher: study the nearest unexamined subject when one is in range and curiosity is up.
        RoleId::Researcher => vec![Behavior {
            mode: Mode::Examine,
            rank: 4,
            target: TargetKind::NearestExaminable,
            considerations: vec![
                gate(Fact::HasUnexaminedNearby),
                Consideration {
                    input: Input::Drive(DriveId::CURIOSITY),
                    curve: Curve::Linear { m: 1.0, b: 0.2 },
                },
            ],
        }],
        // Psionic detective: scan anomalies (top duty), ward the squad when an ally is down, commune
        // with the watcher when a threat is known.
        RoleId::Psionic => vec![
            Behavior {
                mode: Mode::PsiScan,
                rank: 4,
                target: TargetKind::None,
                considerations: vec![gate(Fact::AnomalyResidueNearby)],
            },
            Behavior {
                mode: Mode::Ward,
                rank: 3,
                target: TargetKind::None,
                considerations: vec![gate(Fact::AllyDownNearby)],
            },
            Behavior {
                mode: Mode::Commune,
                rank: 2,
                target: TargetKind::TrackedThreat,
                considerations: vec![gate(Fact::ThreatBearingKnown)],
            },
        ],
        // Medic: move to and heal the nearest critically wounded ally.
        RoleId::Medic => vec![Behavior {
            mode: Mode::TendWounded,
            rank: 4,
            target: TargetKind::NearestWoundedAlly,
            considerations: vec![gate(Fact::AllyDownNearby)],
        }],
        // Engineer: inspect/secure unexamined machinery & doors. `DeploySensor` (extend the squad's
        // senses) is deliberately NOT an unconditional idle behaviour — deploying on loop is spam; it
        // lands in the actions layer behind a real "unexplored area" gate. The `Mode` still exists for
        // the RL action space.
        RoleId::Engineer => vec![Behavior {
            mode: Mode::SecureDoor,
            rank: 4,
            target: TargetKind::NearestExaminable,
            considerations: vec![gate(Fact::HasUnexaminedNearby)],
        }],
    };
    b.extend(tail());
    b
}

#[cfg(test)]
mod tests {
    // Per-role decision locks — a fixed perception must select the stereotyped action. Mirrors the
    // seed-in/assert-out convention of `ai::utility::tests`. Uses the same `decide` engine.
    use super::*;
    use crate::ai::utility::{decide, Perception, SquadFields};
    use crate::ai::drives::DRIVE_COUNT;

    fn perc() -> Perception {
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
            // Near the anchor by default so cohesion is quiet unless a test strays it.
            squad: SquadFields { anchor_dist: 0.0, ..SquadFields::neutral() },
        }
    }

    fn chosen_mode(role: RoleId, p: &Perception) -> Mode {
        let brain = Brain { behaviors: default_behaviors(role) };
        let mut rng = 42u32;
        brain.behaviors[decide(&brain.behaviors, p, &mut rng)].mode
    }

    #[test]
    fn gunman_overwatches_a_known_threat() {
        let mut p = perc();
        p.squad.threat_bearing_known = 1.0;
        assert_eq!(chosen_mode(RoleId::Gunman, &p), Mode::Overwatch);
    }

    #[test]
    fn researcher_examines_unexamined_nearby() {
        let mut p = perc();
        p.squad.has_unexamined = 1.0;
        p.drives[DriveId::CURIOSITY.0] = 0.8;
        assert_eq!(chosen_mode(RoleId::Researcher, &p), Mode::Examine);
    }

    #[test]
    fn psionic_scans_anomaly_residue() {
        let mut p = perc();
        p.squad.anomaly_residue = 1.0;
        assert_eq!(chosen_mode(RoleId::Psionic, &p), Mode::PsiScan);
    }

    #[test]
    fn medic_tends_a_downed_ally() {
        let mut p = perc();
        p.squad.ally_down = 1.0;
        p.squad.wounded_ally_dist = 3.0;
        assert_eq!(chosen_mode(RoleId::Medic, &p), Mode::TendWounded);
    }

    #[test]
    fn any_role_flees_when_afraid() {
        // Fear (rank 5) trumps every role's duty. Check all five.
        for role in RoleId::ALL {
            let mut p = perc();
            p.drives[DriveId::FEAR.0] = 0.95;
            // Even with a live duty cue, survival wins.
            p.squad.has_unexamined = 1.0;
            p.squad.threat_bearing_known = 1.0;
            p.squad.anomaly_residue = 1.0;
            p.squad.ally_down = 1.0;
            assert_eq!(chosen_mode(role, &p), Mode::Flee, "role {role:?} should flee");
        }
    }

    #[test]
    fn strayed_unit_regroups() {
        // Far from the anchor with no duty cue → the cohesion pull (rank 3) beats FollowAnchor (1).
        for role in RoleId::ALL {
            let mut p = perc();
            p.squad.anchor_dist = 30.0;
            assert_eq!(chosen_mode(role, &p), Mode::Regroup, "role {role:?} should regroup");
        }
    }

    #[test]
    fn idle_unit_follows_the_anchor() {
        // No fear, no stray, no duty → default drift with the squad, never bare Wander.
        for role in RoleId::ALL {
            let p = perc();
            assert_eq!(chosen_mode(role, &p), Mode::FollowAnchor, "role {role:?} should follow");
        }
    }

    #[test]
    fn validate_rejects_empty_behaviors() {
        // A well-formed RON file can still author an unsafe brain: an empty behaviour list would
        // index-panic on the unit's first think. Validation must reject it at the door (fail loud).
        let src = r#"{ Gunman: (behaviors: []) }"#;
        let defs = parse_roles_ron(src).expect("parses (empty list is valid RON)");
        let err = validate_role_defs(&defs).expect_err("empty behaviors must be rejected");
        assert!(err.contains("Gunman") && err.contains("empty"), "unhelpful error: {err}");
    }

    #[test]
    fn validate_rejects_out_of_range_drive_index() {
        // `Drive((9))` deserializes fine (any usize) but would index-panic `self.drives[9]` on a
        // 5-slot array. Validation must catch the out-of-range index before it reaches the engine.
        let src = r#"{
            Medic: (behaviors: [
                (mode: Wander, rank: 0, target: None, considerations: [
                    (input: Drive((9)), curve: Linear(m: 1.0, b: 0.0)),
                ]),
            ]),
        }"#;
        let defs = parse_roles_ron(src).expect("parses");
        let err = validate_role_defs(&defs).expect_err("out-of-range drive must be rejected");
        assert!(err.contains("Drive((9))"), "unhelpful error: {err}");
    }

    #[test]
    fn validate_accepts_a_well_formed_override() {
        let src = r#"{
            Gunman: (behaviors: [
                (mode: Wander, rank: 0, target: None, considerations: [
                    (input: Drive((1)), curve: Linear(m: 1.0, b: 0.0)),
                ]),
            ]),
        }"#;
        let defs = parse_roles_ron(src).expect("parses");
        assert!(validate_role_defs(&defs).is_ok(), "a valid override must pass validation");
    }

    #[test]
    fn roles_ron_overrides_a_default() {
        // The RON authoring path: a one-behaviour Gunman override replaces the default repertoire.
        let src = r#"{
            Gunman: (behaviors: [
                (mode: Wander, rank: 0, target: None, considerations: []),
            ]),
        }"#;
        let defs = parse_roles_ron(src).expect("valid roles.ron");
        let mut brains = RoleBrains::defaults();
        brains.overlay(defs);
        assert_eq!(brains.get(RoleId::Gunman).behaviors.len(), 1);
        // A role not in the override keeps its full default repertoire.
        assert!(brains.get(RoleId::Medic).behaviors.len() > 1);
    }
}
