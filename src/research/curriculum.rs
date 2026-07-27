//! **The Thaumiel curriculum** — the per-subject experiment battery (FVS-E-5), the unlock payout
//! (FVS-F-3), and the prerequisite graph that was FVS-F-1's un-landed half.
//!
//! # What this slice is for
//!
//! [`crate::research`] shipped as an excellent pure library with nothing authored for it to read: no
//! experiment battery existed, and no captured specimen carried a payout, so `capture → research →
//! unlock` was reachable only from test setup. This is the authored content that closes it, and the
//! graph that turns four independent unlocks into a **curriculum**.
//!
//! # Why the graph is authored backwards, and why the order is derived
//!
//! Grounded in Wang, Cohen, Yi, Park, Teo & Andersen, *Goal-based Progression Synthesis in a Korean
//! Learning Game* (FDG '19, DOI 10.1145/3337722.3337745). Two of their findings decide the shape here:
//!
//! * **A good progression is goal-driven, not merely gradual.** Their headline result is that
//!   engagement "often comes from a sense of accomplishment after completing hard tasks" — which is why
//!   games have boss levels — so a progression should build toward a hard task *as fast as the
//!   prerequisites allow* rather than ramping smoothly with nothing to aim at. So the curriculum is
//!   authored from the hard capture backwards, and [`Curriculum::goals`] names what it is building to.
//! * **Post-order DFS over a "harder than" graph gives the ordering for free.** Their traversal is
//!   topologically sorted, so a prerequisite is never introduced after the thing that depends on it,
//!   *and* it front-loads the small achievable sub-goals along the way. [`Curriculum::progression`] is
//!   that traversal. The guarantee therefore comes from the data structure rather than from a check
//!   somebody has to remember to write.
//!
//! Their other characteristic — **pacing**, difficulty rising with the player's skill — is deliberately
//! *not* authored here. That is FVS-H-3's runtime director, and encoding it statically as well would put
//! two systems in charge of one thing.
//!
//! # The gate is on offering research, not on the payout
//!
//! FVS-F-1 asks that "a node unlocks only when prerequisites are met". Enforcing that by *refusing the
//! payout* would dead-end a player who researched a gated specimen: the work is spent and the capability
//! is gone. So the gate sits one step earlier — a specimen whose prerequisites are unmet is not offered
//! experiments at all, and the readout says which prior research it is waiting on. That is the same
//! topological guarantee applied where the player still has a choice, and it cannot soft-lock, because
//! validation proves every prerequisite is itself granted by something capturable.
//!
//! # Determinism
//!
//! Pure data and pure functions over an **authored `Vec`**, never an ECS query — index order is a total
//! order by construction, so the traversal needs no canonical sort and cannot introduce an ordering
//! hazard. No RNG, no `App`.

use serde::{Deserialize, Serialize};

use super::unlock::Capability;
use super::{Experiment, HiddenParam};
use crate::knowledge::Subject;

/// **What is actually true** about one kind of anomaly — the answers research converges on.
///
/// The player never sees this; it is the ground truth an experiment is a noisy reading of. Authored
/// rather than derived from the creature's code, because "is it lethal" is a claim about how the thing
/// behaves in play, not a field on it — and because authoring it is what lets a *false* belief
/// (FVS-O-5) be identifiable as false.
///
/// Four bools rather than something richer for the reason [`super::posterior`] gives: every parameter
/// here is a question the player asks in words, and a Bernoulli is a sentence. A parameter needing more
/// than two answers should be **split into more parameters**, not widened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HiddenTruth {
    /// Will it kill an operative who engages it?
    pub lethality: bool,
    /// Does it spread — to operatives, or by making more of itself?
    pub contagion: bool,
    /// Is it contained by **maintaining** a stigmergy channel, rather than by starving one?
    pub capture_basin: bool,
    /// Does it get stronger, or produce more of itself, if left alone?
    pub proliferation: bool,
}

impl HiddenTruth {
    /// The authored answer to one question.
    pub fn get(&self, param: HiddenParam) -> bool {
        match param {
            HiddenParam::Lethality => self.lethality,
            HiddenParam::Contagion => self.contagion,
            HiddenParam::CaptureBasin => self.capture_basin,
            HiddenParam::Proliferation => self.proliferation,
        }
    }
}

/// Everything authored about researching one kind of anomaly.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectResearch {
    /// Which anomaly this describes. Matches `containment::Specimen::subject`.
    pub subject: Subject,
    /// What is actually true about it — what the experiments are noisy readings of.
    pub truth: HiddenTruth,
    /// The tests the researchers can run on it, ranked at runtime by expected information gain.
    pub experiments: Vec<Experiment>,
    /// The capabilities completing this specimen's research grants.
    ///
    /// Non-empty, and validated as such: a capturable anomaly that teaches nothing is a content gap,
    /// and [`ResearchConfig::validate`] is where that gets caught rather than surfacing as `unlock.rs`'s
    /// "no payout authored" warning after the player has already done the work.
    ///
    /// **A list rather than exactly one, decided 2026-07-27.** The roster has three researchable
    /// anomalies and four capabilities, because `RemoteCapping` has no natural parent — FVS-B-7 makes
    /// capping a nest yield **no specimen** on purpose, so there is no crab specimen to derive it from.
    /// The alternatives were to leave a capability permanently unearnable (which reads as broken in the
    /// tech-tree HUD), to invent a crab capture (real gameplay, and it would change every offline
    /// rollout), or to let one anomaly teach two things. The last is both the smallest change and the
    /// best fiction: studying how SCP-1048 sources and assembles material is how you learn to *seal* a
    /// structure, so the bear yields both the remote observer and the sealing charge.
    pub unlocks: Vec<Capability>,
    /// Capabilities that must already be unlocked before this research can begin — the graph edges.
    ///
    /// Empty for the entry points. Authored per *subject* rather than per *capability* because that is
    /// the sentence a designer writes: "you cannot make sense of the bear until you have the morale
    /// field", not "MoraleField precedes RemoteObserver".
    #[serde(default)]
    pub requires: Vec<Capability>,
}

/// The `research:` config slice.
///
/// **A top-level slice, deliberately outside [`crate::config::WorldConfig`]** — the offline search does
/// not evolve it, for exactly the reason `containment::ContainmentConfig` and `session::SessionConfig`
/// are excluded: this defines what research *means* and what it is worth, so a search free to retune it
/// would be moving the objective rather than solving it, and archive fitness would stop being
/// comparable between bakes.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchConfig {
    /// One entry per researchable anomaly. Order is the authored order and is load-bearing: it is the
    /// tiebreak for the derived progression, so reordering the file reorders the curriculum.
    pub subjects: Vec<SubjectResearch>,
}

impl ResearchConfig {
    /// Reject malformed authored content at load — one path, no fallback.
    ///
    /// Every check here exists because the failure it prevents is silent rather than loud: a duplicate
    /// subject would shadow a battery, a missing prerequisite would soft-lock a campaign, and a cycle
    /// would make the progression undefined. All three would present as "research just does nothing".
    pub fn validate(&self) -> Result<(), String> {
        if self.subjects.is_empty() {
            return Err("research.subjects is empty — nothing is researchable".into());
        }
        for s in &self.subjects {
            if s.experiments.is_empty() {
                return Err(format!(
                    "research.{:?}: no experiments authored — the specimen would be permanently \
                     unresearchable and its unlock unreachable",
                    s.subject
                ));
            }
            for e in &s.experiments {
                e.validate().map_err(|err| format!("research.{:?}: {err}", s.subject))?;
            }
            if s.unlocks.is_empty() {
                return Err(format!(
                    "research.{:?}: grants no capability — research the player can complete for \
                     nothing reads as a bug, not as a design",
                    s.subject
                ));
            }
            if s.unlocks.iter().any(|c| s.requires.contains(c)) {
                return Err(format!(
                    "research.{:?} requires a capability it grants — an unsatisfiable node",
                    s.subject
                ));
            }
        }
        // A duplicated subject would mean two batteries for one specimen and a silent winner; a
        // duplicated capability would mean research the player completes for nothing.
        for (i, a) in self.subjects.iter().enumerate() {
            for b in &self.subjects[i + 1..] {
                if a.subject == b.subject {
                    return Err(format!("research: {:?} is authored twice", a.subject));
                }
                if let Some(dup) = a.unlocks.iter().find(|c| b.unlocks.contains(c)) {
                    return Err(format!(
                        "research: {:?} and {:?} both grant {dup:?} — one of them can never be the \
                         reason the player has it",
                        a.subject, b.subject
                    ));
                }
            }
        }
        // Every prerequisite must be *obtainable*, or the campaign soft-locks: the player would hold a
        // specimen they can never be allowed to study.
        for s in &self.subjects {
            for req in &s.requires {
                if !self.subjects.iter().any(|o| o.unlocks.contains(req)) {
                    return Err(format!(
                        "research.{:?} requires {:?}, which nothing in the table grants — that \
                         specimen could never be researched",
                        s.subject, req
                    ));
                }
            }
        }
        self.check_acyclic()?;
        self.check_resolvable(super::pacing::ExperimentFatigue::default())
    }

    /// Reject a battery that **cannot finish**, under the shipped fatigue.
    ///
    /// # Why this check exists, and what it caught
    ///
    /// Found 2026-07-27, the first time anything actually ran an experiment. Three numbers that were
    /// each independently reasonable are jointly unsatisfiable:
    ///
    /// * `ExperimentFatigue::decay = 0.8` and `USELESS_BELOW = 0.5` together allow only **three** tests
    ///   on a parameter before it stops being offered (`0.8 → 0.64 → 0.512 → 0.41`, which would lie).
    /// * Three concordant readings at those reliabilities multiply to a likelihood ratio of ~7.5, i.e.
    ///   a belief of **0.882**.
    /// * [`super::posterior::REVEAL_AT`] is **0.9**.
    ///
    /// So a 0.8-reliability battery could never resolve a single parameter, no specimen could ever
    /// complete, and no capability could ever be unlocked — while every unit test stayed green, because
    /// each piece is correct in isolation and nothing composed them. That is the same "pure library with
    /// no caller" failure this whole item exists to close, one layer down.
    ///
    /// The check simulates the **best case**: every reading concordant with the truth, which is the
    /// most a player could ever get. It runs the real [`ResearchPosterior::observe`] rather than
    /// re-deriving the arithmetic, so it cannot drift from the update it is checking.
    fn check_resolvable(&self, fatigue: super::pacing::ExperimentFatigue) -> Result<(), String> {
        for s in &self.subjects {
            for param in HiddenParam::ALL {
                let Some(base) =
                    s.experiments.iter().filter(|e| e.param == param).map(|e| e.reliability).fold(
                        None::<f32>,
                        |acc, r| Some(acc.map_or(r, |a: f32| a.max(r))),
                    )
                else {
                    return Err(format!(
                        "research.{:?}: no experiment bears on {param:?}, so that parameter can never \
                         resolve and the specimen can never complete",
                        s.subject
                    ));
                };
                let mut p = super::ResearchPosterior::unknown();
                let truth = s.truth.get(param);
                let mut runs = 0u32;
                while let Some(r) = fatigue.effective(base, runs) {
                    p.observe(param, truth, r);
                    runs += 1;
                    if p.is_revealed(param) {
                        break;
                    }
                }
                if !p.is_revealed(param) {
                    let reached = p.belief(param).max(1.0 - p.belief(param));
                    let threshold = super::posterior::REVEAL_AT;
                    return Err(format!(
                        "research.{:?}: {param:?} can never be resolved — {runs} tests at base \
                         reliability {base} reach only {reached:.3} against REVEAL_AT {threshold}. \
                         Raise the reliability, add a second experiment on this parameter, or \
                         retune ExperimentFatigue.",
                        s.subject
                    ));
                }
            }
        }
        Ok(())
    }

    /// Reject a prerequisite cycle. Without this the progression is undefined and the player would hold
    /// a set of specimens that each wait on another.
    fn check_acyclic(&self) -> Result<(), String> {
        // Iterative Kahn-style peel: repeatedly drop every subject whose prerequisites are all already
        // granted. Whatever cannot be dropped is in (or behind) a cycle.
        let mut granted: Vec<Capability> = Vec::new();
        let mut open: Vec<&SubjectResearch> = self.subjects.iter().collect();
        while !open.is_empty() {
            let before = open.len();
            open.retain(|s| {
                let ready = s.requires.iter().all(|r| granted.contains(r));
                if ready {
                    granted.extend(s.unlocks.iter().copied());
                }
                !ready
            });
            if open.len() == before {
                let stuck: Vec<Subject> = open.iter().map(|s| s.subject).collect();
                return Err(format!(
                    "research: prerequisite cycle — {stuck:?} can never become available, because \
                     each waits on a capability only another of them grants"
                ));
            }
        }
        Ok(())
    }
}

/// The validated curriculum, as a resource.
///
/// Registered from the config slice; the graph queries below are what the research verb and the
/// FVS-L-3 HUD read.
#[derive(bevy::prelude::Resource, Debug, Clone)]
pub struct Curriculum(pub ResearchConfig);

impl Curriculum {
    /// The authored entry for `subject`, if it is researchable at all.
    pub fn entry(&self, subject: Subject) -> Option<&SubjectResearch> {
        self.0.subjects.iter().find(|s| s.subject == subject)
    }

    /// The experiment battery for `subject`. Empty for an anomaly with no authored research.
    pub fn experiments(&self, subject: Subject) -> &[Experiment] {
        self.entry(subject).map(|s| s.experiments.as_slice()).unwrap_or(&[])
    }

    /// What completing `subject`'s research grants. Empty for an anomaly with no authored research.
    pub fn payouts(&self, subject: Subject) -> &[Capability] {
        self.entry(subject).map(|s| s.unlocks.as_slice()).unwrap_or(&[])
    }

    /// The ground truth an experiment on `subject` is a noisy reading of.
    pub fn truth(&self, subject: Subject) -> Option<HiddenTruth> {
        self.entry(subject).map(|s| s.truth)
    }

    /// Which prerequisites of `subject` are **not yet** unlocked.
    ///
    /// Returned rather than a bare bool for the same reason `ContainmentRule::unmet` exists: the HUD's
    /// job is to say *why* something is unavailable, and "AWAITING PRIOR RESEARCH" with no name is a
    /// dead end for the player.
    pub fn unmet_prerequisites(
        &self,
        subject: Subject,
        tree: &super::unlock::TechTree,
    ) -> Vec<Capability> {
        self.entry(subject)
            .map(|s| s.requires.iter().copied().filter(|c| !tree.has(*c)).collect())
            .unwrap_or_default()
    }

    /// Whether `subject` may be studied right now.
    pub fn is_available(&self, subject: Subject, tree: &super::unlock::TechTree) -> bool {
        self.entry(subject).is_some() && self.unmet_prerequisites(subject, tree).is_empty()
    }

    /// The **goals** — subjects whose capability nothing else requires.
    ///
    /// These are [PROG]'s boss levels: the things the curriculum is building toward. A designer adding
    /// a harder anomaly gets a new goal for free by pointing its `requires` at existing capabilities.
    pub fn goals(&self) -> Vec<Subject> {
        self.0
            .subjects
            .iter()
            .filter(|s| {
                // A goal is a subject NONE of whose capabilities anything else waits on. If even one
                // is a prerequisite, the curriculum still builds past it.
                !s.unlocks
                    .iter()
                    .any(|c| self.0.subjects.iter().any(|o| o.requires.contains(c)))
            })
            .map(|s| s.subject)
            .collect()
    }

    /// The player-facing curriculum order: **post-order DFS from each goal**.
    ///
    /// Per Wang et al., this is topologically sorted — a prerequisite always appears before the subject
    /// that needs it — *and* it reaches each goal as early as its prerequisites allow, which is the
    /// goal-drivenness their evaluation found mattered. Deterministic without a sort: the walk visits
    /// goals and prerequisites in authored index order, which is unique by construction.
    pub fn progression(&self) -> Vec<Subject> {
        let mut out: Vec<Subject> = Vec::new();
        for goal in self.goals() {
            self.visit(goal, &mut out);
        }
        // Anything unreachable from a goal would be a subject nothing builds toward. `validate` makes
        // that impossible (a sink is always its own goal), but appending rather than dropping means a
        // future authoring mistake shows up as a badly-placed entry instead of a missing one.
        for s in &self.0.subjects {
            if !out.contains(&s.subject) {
                out.push(s.subject);
            }
        }
        out
    }

    /// Post-order visit: prerequisites first, then the subject itself.
    fn visit(&self, subject: Subject, out: &mut Vec<Subject>) {
        if out.contains(&subject) {
            return;
        }
        if let Some(entry) = self.entry(subject) {
            for req in &entry.requires {
                // Walk to whoever grants it. `validate` proved somebody does.
                if let Some(prev) = self.0.subjects.iter().find(|o| o.unlocks.contains(req)) {
                    self.visit(prev.subject, out);
                }
            }
        }
        // Re-check: a prerequisite walk can have inserted us via a diamond.
        if !out.contains(&subject) {
            out.push(subject);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::unlock::TechTree;
    use crate::research::HiddenParam;

    fn exp(name: &str, param: HiddenParam) -> Experiment {
        Experiment { name: name.into(), param, reliability: 0.85 }
    }

    fn entry(
        subject: Subject,
        unlocks: Vec<Capability>,
        requires: Vec<Capability>,
    ) -> SubjectResearch {
        SubjectResearch {
            subject,
            truth: HiddenTruth {
                lethality: true,
                contagion: false,
                capture_basin: true,
                proliferation: false,
            },
            // A FULL battery — one experiment per hidden parameter. `check_resolvable` rejects a
            // partial one, correctly: a parameter no test bears on can never resolve, so the specimen
            // never completes and its unlock is unreachable.
            experiments: HiddenParam::ALL.iter().map(|p| exp(&format!("{p:?} ASSAY"), *p)).collect(),
            unlocks,
            requires,
        }
    }

    /// The shape the shipped curriculum has: one entry point, a middle, and a goal behind two unlocks.
    fn chain() -> Curriculum {
        Curriculum(ResearchConfig {
            subjects: vec![
                entry(Subject::ComfortBlob, vec![Capability::MoraleField], vec![]),
                entry(Subject::Crabs, vec![Capability::RemoteCapping], vec![]),
                entry(Subject::Parasite, vec![Capability::FieldCure], vec![Capability::MoraleField]),
                entry(
                    Subject::BuilderBear,
                    vec![Capability::RemoteObserver],
                    vec![Capability::FieldCure, Capability::RemoteCapping],
                ),
            ],
        })
    }

    #[test]
    fn the_shipped_shape_validates() {
        chain().0.validate().expect("the test fixture must be a legal curriculum");
    }

    #[test]
    fn the_progression_never_introduces_a_subject_before_its_prerequisite() {
        // The topological guarantee, which is the whole reason the order is derived rather than
        // hand-sequenced (Wang et al. 2019).
        let c = chain();
        let order = c.progression();
        let pos = |s: Subject| order.iter().position(|&o| o == s).expect("every subject appears");
        for s in &c.0.subjects {
            for req in &s.requires {
                let granter = c
                    .0
                    .subjects
                    .iter()
                    .find(|o| o.unlocks.contains(req))
                    .expect("validate proves this exists");
                assert!(
                    pos(granter.subject) < pos(s.subject),
                    "{:?} is offered before {:?}, which grants its prerequisite {:?}",
                    s.subject,
                    granter.subject,
                    req
                );
            }
        }
        assert_eq!(order.len(), c.0.subjects.len(), "every subject appears exactly once");
    }

    #[test]
    fn the_goal_is_the_thing_nothing_else_needs() {
        // [PROG]'s boss level: the curriculum builds toward it, so it must be the sink, and it must
        // come last in the progression.
        let c = chain();
        assert_eq!(c.goals(), vec![Subject::BuilderBear]);
        assert_eq!(*c.progression().last().expect("non-empty"), Subject::BuilderBear);
    }

    #[test]
    fn a_gated_subject_reports_what_it_is_waiting_on() {
        // The HUD's requirement is to say WHY, exactly like `ContainmentRule::unmet` — a bare
        // "unavailable" is a dead end for the player.
        let c = chain();
        let mut tree = TechTree::default();
        assert!(!c.is_available(Subject::Parasite, &tree));
        assert_eq!(c.unmet_prerequisites(Subject::Parasite, &tree), vec![Capability::MoraleField]);

        tree.grant(Capability::MoraleField);
        assert!(c.is_available(Subject::Parasite, &tree));
        assert!(c.unmet_prerequisites(Subject::Parasite, &tree).is_empty());
    }

    #[test]
    fn an_entry_point_is_available_from_a_cold_start() {
        // If nothing were researchable at zero unlocks the campaign could never begin — the soft-lock
        // this whole validation pass exists to prevent, in its most basic form.
        let c = chain();
        let tree = TechTree::default();
        assert!(
            c.0.subjects.iter().any(|s| c.is_available(s.subject, &tree)),
            "no anomaly is researchable with an empty tech tree"
        );
    }

    #[test]
    fn a_prerequisite_nothing_grants_is_rejected() {
        let cfg = ResearchConfig {
            subjects: vec![entry(
                Subject::ComfortBlob,
                vec![Capability::MoraleField],
                vec![Capability::FieldCure],
            )],
        };
        let err = cfg.validate().expect_err("an ungrantable prerequisite must be refused");
        assert!(err.contains("FieldCure"), "the error must name the unreachable capability: {err}");
    }

    #[test]
    fn a_prerequisite_cycle_is_rejected() {
        let cfg = ResearchConfig {
            subjects: vec![
                entry(Subject::ComfortBlob, vec![Capability::MoraleField], vec![Capability::FieldCure]),
                entry(Subject::Parasite, vec![Capability::FieldCure], vec![Capability::MoraleField]),
            ],
        };
        let err = cfg.validate().expect_err("a cycle must be refused");
        assert!(err.contains("cycle"), "{err}");
    }

    #[test]
    fn two_subjects_granting_the_same_capability_are_rejected() {
        // Not pedantry: the second one would be research the player completes for nothing, which reads
        // as a bug rather than as a design.
        let cfg = ResearchConfig {
            subjects: vec![
                entry(Subject::ComfortBlob, vec![Capability::MoraleField], vec![]),
                entry(Subject::Crabs, vec![Capability::MoraleField], vec![]),
            ],
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn a_subject_with_no_experiments_is_rejected() {
        let cfg = ResearchConfig {
            subjects: vec![SubjectResearch {
                subject: Subject::ComfortBlob,
                truth: HiddenTruth {
                    lethality: false,
                    contagion: false,
                    capture_basin: true,
                    proliferation: false,
                },
                experiments: vec![],
                unlocks: vec![Capability::MoraleField],
                requires: vec![],
            }],
        };
        assert!(cfg.validate().is_err(), "an unresearchable specimen makes its unlock unreachable");
    }

    #[test]
    fn an_unresearchable_subject_offers_nothing_rather_than_panicking() {
        // The crab nests are capturable-ish but grant no specimen, and a future anomaly may ship
        // before its battery is written. That is a content gap, not a crash.
        let c = chain();
        assert!(c.experiments(Subject::Watcher).is_empty());
        assert!(c.payouts(Subject::Watcher).is_empty());
        assert!(!c.is_available(Subject::Watcher, &TechTree::default()));
    }
}
