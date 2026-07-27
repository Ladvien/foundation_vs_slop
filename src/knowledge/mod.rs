//! **Operative knowledge** (Push 10) — the progression system that is not levelling.
//!
//! Design: `docs/2026-07-26-site-hub-and-operative-knowledge.md` §3.
//!
//! A level is a scalar that makes an operative better at everything, everywhere, forever. A **belief**
//! is a proposition about a *kind of thing* that only acts when that kind is present. It is contextual,
//! legible ("Okafor knows 1048-A is lethal"), **transmissible**, and it can be **wrong**. None of that
//! is true of a number going up, which is why this replaces squad levelling rather than supplementing
//! it — levelling is the archetypal "+X%" that FVS-F-2 forbids.
//!
//! ## Absence of a belief is NOT a low-confidence belief
//!
//! The one modelling point not to compromise on, and the corpus has the argument. Fisher, quoted in the
//! uncertainty-representation paper in the local corpus (OpenAlex W3014596384): *"not knowing the chance
//! of mutually exclusive events and knowing the chance to be equal are two quite different states of
//! knowledge."*
//!
//! So an operative who has never met SCP-1048 holds **no** `Belief` for it — not `confidence: 0.5`.
//! [`Knowledge::of`] returns `Option`, and "unknown" is a distinct behavioural state from "unsure".
//! This is precisely where it differs from `research::ResearchPosterior`, which *does* start at 0.5:
//! a specimen on the slab always has a posterior, because the Foundation is actively studying it.
//!
//! ## Component discipline
//!
//! The belief set is a **value field** on a component present from spawn, never a marker toggled on
//! acquisition — `scp1048`'s standing rule: a flipped marker splits the hashed archetype and makes ECS
//! iteration order run-dependent. `containment::Containment` is the pattern copied here.
//!
//! ## Determinism
//!
//! Beliefs modulate FEAR, which feeds Think → movement → hashed `Transform`, so this is **pinned
//! simulation state**, not cosmetic. The behavioural coupling ships at **gain zero** so it is a
//! bit-exact no-op until deliberately enabled — the same staging `ai::drives`'
//! `a_zero_gain_din_is_a_bit_exact_no_op` established.

use serde::{Deserialize, Serialize};

/// What a belief is *about*.
///
/// **Append-only.** Beliefs are saved (FVS-G-2) and keyed by index, so reordering these silently
/// reinterprets a stored campaign — the same discipline as `research::HiddenParam` and
/// `squad_ai::surprise::ActorKind`.
///
/// A *kind*, never an individual: "1048-A copies are lethal" is knowledge that transfers to the next
/// one you meet, which is what makes it worth carrying between expeditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Subject {
    /// The dimensional crabs.
    Crabs,
    /// SCP-150, the tongue-eating parasite.
    Parasite,
    /// SCP-999, the comfort blob.
    ComfortBlob,
    /// SCP-1048, the benign original.
    BuilderBear,
    /// SCP-1048-A and its siblings — the hostile copies.
    BearCopies,
    /// The smiley watcher.
    Watcher,
}

impl Subject {
    pub const ALL: [Subject; 6] = [
        Subject::Crabs,
        Subject::Parasite,
        Subject::ComfortBlob,
        Subject::BuilderBear,
        Subject::BearCopies,
        Subject::Watcher,
    ];

    fn index(self) -> usize {
        match self {
            Subject::Crabs => 0,
            Subject::Parasite => 1,
            Subject::ComfortBlob => 2,
            Subject::BuilderBear => 3,
            Subject::BearCopies => 4,
            Subject::Watcher => 5,
        }
    }
}

/// What is believed about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Claim {
    /// It will kill you. Raises FEAR when the subject is present.
    Lethal,
    /// It will not. Lowers FEAR — and is the belief most worth being *wrong* about.
    Harmless,
    /// There is a way to contain it. Makes its rule legible in the containment HUD.
    Containable,
}

impl Claim {
    pub const ALL: [Claim; 3] = [Claim::Lethal, Claim::Harmless, Claim::Containable];

    fn index(self) -> usize {
        match self {
            Claim::Lethal => 0,
            Claim::Harmless => 1,
            Claim::Containable => 2,
        }
    }

    /// The claim this one directly contradicts, if any.
    ///
    /// **Only `Lethal` and `Harmless` compete.** `Containable` is orthogonal to both — knowing how to
    /// hold something says nothing about whether it will kill you, and an operative can perfectly well
    /// know both. A first draft stored one belief per *subject*, which forced these into competition and
    /// made "it is lethal" silently erase "and here is how to contain it". The test
    /// `only_a_containable_belief_makes_a_rule_legible` caught it.
    fn contradicts(self) -> Option<Claim> {
        match self {
            Claim::Lethal => Some(Claim::Harmless),
            Claim::Harmless => Some(Claim::Lethal),
            Claim::Containable => None,
        }
    }
}

/// Where a belief came from, in descending reliability.
///
/// Provenance is not decoration: it is what makes a false belief *traceable* to its source, which is
/// the counter-play FVS-O-5 depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Provenance {
    /// It happened to me.
    Firsthand,
    /// I saw it happen to someone else.
    Witnessed,
    /// Another operative told me. Confidence decays with each retelling.
    Told,
    /// I read it in a report at the Site. Lowest, and the only one that crosses runs.
    Read,
}

impl Provenance {
    /// The confidence a freshly-acquired belief carries.
    ///
    /// Firsthand is deliberately **not** 1.0: an operative who was struck once by one copy has strong
    /// evidence, not proof, and leaving headroom is what lets a belief be *strengthened* by a second
    /// encounter rather than being saturated on the first.
    pub fn base_confidence(self) -> f32 {
        match self {
            Provenance::Firsthand => 0.85,
            Provenance::Witnessed => 0.65,
            Provenance::Told => 0.45,
            Provenance::Read => 0.35,
        }
    }
}

/// One proposition an operative holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Belief {
    pub claim: Claim,
    pub confidence: f32,
    pub provenance: Provenance,
    /// Run tick it was acquired — for recency, and for the records office's timestamps.
    pub acquired: u64,
}

/// What one operative knows. A value field present from spawn; **never** a toggled marker.
#[derive(bevy::prelude::Component, Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Knowledge {
    /// `None` means **never encountered**, which is a different state from an uncertain belief.
    ///
    /// Indexed `[subject][claim]`, because an operative can hold several *non-contradicting* claims
    /// about one kind of thing at once — "1048-A copies are lethal" and "they can be contained by
    /// out-watching them" are both true, and both worth knowing separately.
    beliefs: [[Option<Belief>; Claim::ALL.len()]; Subject::ALL.len()],
}

impl Knowledge {
    /// What this operative believes about `subject` on a specific `claim`, if anything.
    pub fn of(&self, subject: Subject, claim: Claim) -> Option<Belief> {
        self.beliefs[subject.index()][claim.index()]
    }

    /// Does this operative know *anything* about it?
    pub fn knows(&self, subject: Subject) -> bool {
        self.beliefs[subject.index()].iter().any(|b| b.is_some())
    }

    /// Acquire or update a belief.
    ///
    /// **A stronger provenance always wins; an equal one reinforces; a weaker one is ignored.** That
    /// ordering is the whole reliability model in one rule, and the last clause is what stops hearsay
    /// overwriting something you saw yourself — which is exactly the attack FVS-O-5 will mount.
    pub fn learn(&mut self, subject: Subject, claim: Claim, provenance: Provenance, tick: u64) {
        let incoming = Belief {
            claim,
            confidence: provenance.base_confidence(),
            provenance,
            acquired: tick,
        };
        // A contradicting claim held on *weaker* evidence is displaced; on stronger evidence it wins and
        // the incoming claim is dropped. This is where hearsay fails to overwrite experience.
        if let Some(opposite) = claim.contradicts() {
            let held = self.beliefs[subject.index()][opposite.index()];
            if let Some(h) = held {
                if h.provenance.base_confidence() >= provenance.base_confidence() {
                    return; // the stronger existing belief stands
                }
                self.beliefs[subject.index()][opposite.index()] = None;
            }
        }
        let slot = &mut self.beliefs[subject.index()][claim.index()];
        match slot {
            None => *slot = Some(incoming),
            Some(existing) => {
                if provenance.base_confidence() > existing.provenance.base_confidence() {
                    *slot = Some(incoming);
                } else if provenance == existing.provenance {
                    // Reinforcement: seeing it twice yourself is more convincing than once, but it
                    // asymptotes rather than reaching certainty — an operative is never *proof*.
                    existing.confidence = (existing.confidence + (1.0 - existing.confidence) * 0.4)
                        .clamp(0.0, 0.99);
                    existing.acquired = tick;
                }
            }
        }
    }

    /// How much this operative's fear of `subject` should be scaled, given what they believe.
    ///
    /// **The asymmetry is the thesis** (design doc §3.4): understanding a thing is what makes it
    /// frightening, *and* the only way to contain it. A confident `Lethal` belief raises fear; a
    /// confident `Harmless` one lowers it; **knowing nothing leaves it unchanged** — which is why this
    /// returns exactly 1.0 for an unknown subject rather than some "cautious default".
    pub fn fear_scale(&self, subject: Subject, gain: f32) -> f32 {
        let mut scale = 1.0;
        if let Some(b) = self.of(subject, Claim::Lethal) {
            scale += gain * b.confidence;
        }
        if let Some(b) = self.of(subject, Claim::Harmless) {
            scale -= gain * b.confidence * 0.5;
        }
        // `Containable` deliberately does not touch fear: knowing HOW to hold something is not the same
        // as being less afraid of it, and conflating them would collapse the asymmetry this system is
        // built on.
        scale.max(0.0)
    }

    /// Can this operative read the subject's containment rule in the HUD?
    ///
    /// The **benefit** half of the trade: you cannot drive an anomaly into a basin whose shape you do
    /// not know.
    pub fn can_read_rule(&self, subject: Subject) -> bool {
        self.of(subject, Claim::Containable).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_having_met_something_is_not_the_same_as_being_unsure_about_it() {
        // THE modelling point, and the one the corpus argues for directly (Fisher, via W3014596384):
        // "not knowing the chance ... and knowing the chance to be equal are two quite different states
        // of knowledge." An implementation that seeded 0.5 would make ignorance behaviourally identical
        // to a balanced belief, and the whole system would collapse into a stat.
        let k = Knowledge::default();
        assert!(!k.knows(Subject::BearCopies));
        assert_eq!(k.of(Subject::BearCopies, Claim::Lethal), None, "absence must be None, never a 0.5 belief");
        assert_eq!(k.fear_scale(Subject::BearCopies, 1.0), 1.0, "ignorance changes nothing");
    }

    #[test]
    fn knowing_a_thing_is_lethal_makes_an_operative_more_afraid_of_it() {
        let mut k = Knowledge::default();
        k.learn(Subject::BearCopies, Claim::Lethal, Provenance::Firsthand, 10);
        assert!(
            k.fear_scale(Subject::BearCopies, 1.0) > 1.0,
            "the COST half: understanding a thing is what makes it frightening"
        );
        // …and only of that subject. A belief is about a KIND, not a mood.
        assert_eq!(k.fear_scale(Subject::Crabs, 1.0), 1.0);
    }

    #[test]
    fn hearsay_cannot_overwrite_something_you_saw_yourself() {
        // The reliability ordering, and the property FVS-O-5's attack surface depends on: if `Told`
        // could overwrite `Firsthand`, seeding misinformation would be trivial and experience worthless.
        let mut k = Knowledge::default();
        k.learn(Subject::BearCopies, Claim::Lethal, Provenance::Firsthand, 10);
        k.learn(Subject::BearCopies, Claim::Harmless, Provenance::Told, 20);
        let b = k.of(Subject::BearCopies, Claim::Lethal).expect("still known");
        assert_eq!(b.claim, Claim::Lethal, "rumour must not overwrite experience");
        assert_eq!(b.provenance, Provenance::Firsthand);
    }

    #[test]
    fn a_stronger_source_does_replace_a_weaker_one() {
        // The other direction: being told something, then seeing it yourself, must update you.
        let mut k = Knowledge::default();
        k.learn(Subject::Parasite, Claim::Harmless, Provenance::Told, 5);
        k.learn(Subject::Parasite, Claim::Lethal, Provenance::Firsthand, 30);
        let b = k.of(Subject::Parasite, Claim::Lethal).expect("known");
        assert_eq!(b.claim, Claim::Lethal);
        assert_eq!(b.provenance, Provenance::Firsthand, "firsthand verification is the counter-play");
    }

    #[test]
    fn seeing_it_twice_reinforces_but_never_reaches_certainty() {
        let mut k = Knowledge::default();
        k.learn(Subject::Watcher, Claim::Lethal, Provenance::Firsthand, 1);
        let first = k.of(Subject::Watcher, Claim::Lethal).expect("known").confidence;
        for t in 2..12 {
            k.learn(Subject::Watcher, Claim::Lethal, Provenance::Firsthand, t);
        }
        let after = k.of(Subject::Watcher, Claim::Lethal).expect("known").confidence;
        assert!(after > first, "repeated firsthand experience must strengthen a belief");
        assert!(after < 1.0, "but an operative is evidence, never proof");
    }

    #[test]
    fn only_a_containable_belief_makes_a_rule_legible() {
        // The BENEFIT half of the trade. You cannot drive an anomaly into a basin whose shape you do
        // not know — and knowing it is dangerous does not tell you how to hold it.
        let mut k = Knowledge::default();
        assert!(!k.can_read_rule(Subject::ComfortBlob));
        k.learn(Subject::ComfortBlob, Claim::Lethal, Provenance::Firsthand, 1);
        assert!(!k.can_read_rule(Subject::ComfortBlob), "fear is not understanding");
        k.learn(Subject::ComfortBlob, Claim::Containable, Provenance::Firsthand, 2);
        assert!(k.can_read_rule(Subject::ComfortBlob));
    }

    #[test]
    fn a_zero_gain_belief_is_a_bit_exact_no_op() {
        // The staging that keeps this out of the goldens until it is deliberately switched on — the
        // same discipline `ai::drives::a_zero_gain_din_is_a_bit_exact_no_op` established.
        let mut k = Knowledge::default();
        k.learn(Subject::Crabs, Claim::Lethal, Provenance::Firsthand, 1);
        assert_eq!(k.fear_scale(Subject::Crabs, 0.0), 1.0, "gain 0 must change nothing at all");
    }

    #[test]
    fn subjects_have_distinct_slots() {
        let mut k = Knowledge::default();
        for (i, s) in Subject::ALL.iter().enumerate() {
            k.learn(*s, Claim::Lethal, Provenance::Firsthand, i as u64);
        }
        for s in Subject::ALL {
            assert!(k.knows(s), "{s:?} shares a slot with another subject");
        }
    }
}
