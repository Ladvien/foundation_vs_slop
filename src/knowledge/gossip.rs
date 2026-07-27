//! **Belief propagation by conversation** (FVS-O-3) — the `Told` channel.
//!
//! One operative tells another what they know, in the field, and the retelling is **weaker than the
//! telling**. That is what turns a set of private beliefs into a squad-wide one without making them
//! all identical, and it is what gives `crate::dialogue` a mechanical job: the module had exactly one
//! authored conversation on a dev hotkey and no reason to exist.
//!
//! # Grounding
//!
//! [MISPERCEPT] — *Secrets and Misperceptions: The Creation of Self-Fulfilling Illusions*
//! (Sociological Science, 2014, DOI 10.15195/v1.a26) — supplies two things this design leans on:
//!
//! * **Propagation requires contact.** *"Network studies and public opinion research find that we
//!   influence each other. But to be influenced by another's characteristics and behaviors, one must
//!   know of them. Without that knowledge social influence is stifled."* So a belief spreads between
//!   operatives who are **near each other**, never as a squad-wide broadcast. [`EARSHOT`] is that
//!   constraint, and it is why a lone scout comes home still holding what nobody else learned.
//! * **Intentionality is what separates a lie from a mistake.** *"Some are the result of secrets. Some
//!   are the result of mere error. Intentionality differentiates the gap that results from a secret
//!   from the gap that results from an error."* This module produces the **error** kind — honest
//!   retelling that degrades. FVS-O-5's SCP-9191 seeding will produce the **secret** kind, and the
//!   distinction matters because the counter-play differs: you *verify* against a degraded rumour and
//!   you *curate* against a planted one.
//!
//! # Why retelling degrades rather than copying
//!
//! `Provenance::Told` carries a flat `base_confidence` of 0.45, which is right for "someone told me"
//! but cannot express a *chain*. The item asks for confidence that "decays with each retelling", so
//! [`Knowledge::hear`] scales the **teller's own** confidence by [`RETELL_DECAY`] instead of stamping a
//! constant. A belief therefore fades along its path — firsthand 0.85 → 0.60 → 0.42 → … — and dies out
//! on its own rather than saturating the squad with certainty nobody earned.
//!
//! That also makes the *shape* of a rumour legible on FVS-L-5's roster screen: an operative holding a
//! 0.42 `Told` belief is visibly three people from whoever actually saw the thing.
//!
//! # Determinism
//!
//! This is `FixedUpdate`, in the pinned core, and "A tells B" is a **pick over a query** — precisely the
//! shape `tests/determinism_lint.rs` exists to catch. Two properties make it safe:
//!
//! * The roster is sorted by [`SquadMember`], which is unique per operative and is the key every other
//!   site in this repo picks on. `sort_total!` panics on a tie rather than trusting the comment.
//! * Transfers are computed from a **snapshot** taken before any write, so no belief can cross two
//!   operatives in one tick and the result cannot depend on the order writes are applied. A rumour
//!   takes time to cross the squad, which is both the deterministic choice and the better fiction.

use bevy::prelude::*;

use super::{Claim, Knowledge, Provenance, Subject};
use crate::squad::{SquadMember, Unit};

/// How far apart two operatives can be and still talk (world units).
///
/// Contact is the whole constraint ([MISPERCEPT]): without it this would be a squad-wide broadcast,
/// and a scout who saw something alone would arrive home with everyone already knowing it.
pub const EARSHOT: f32 = 6.0;

/// What one retelling costs. The teller's confidence times this becomes the listener's.
///
/// Below 1.0 by definition — a rumour that lost nothing in transit would make hearsay as good as
/// experience and collapse the whole provenance model. 0.7 gives a chain of roughly four useful hops
/// from a firsthand 0.85 before it falls under [`WORTH_SAYING`], which is about the width of a squad.
pub const RETELL_DECAY: f32 = 0.7;

/// Confidence below which a belief is not worth passing on.
///
/// Without a floor, beliefs would propagate forever at vanishing strength — a permanent trickle of
/// writes to pinned state for no behavioural effect, and a roster screen full of 0.01% rumours. This is
/// where a rumour dies of its own accord.
pub const WORTH_SAYING: f32 = 0.2;

/// Fixed ticks between conversations. Operatives are not narrating continuously.
///
/// A modulo on the run clock rather than a per-unit timer: it is deterministic by construction, and it
/// keeps the cadence a property of the *world* rather than state that could drift per entity.
pub const TELL_INTERVAL: u64 = 120;

impl Knowledge {
    /// Hear a belief from someone else. Returns whether it actually took.
    ///
    /// **A weaker provenance never displaces a stronger one** — that rule already lives in
    /// [`Knowledge::learn`], and it is what stops hearsay overwriting something an operative saw
    /// themselves. This adds the missing piece: a `Told` belief also never displaces a *more confident*
    /// `Told` belief, so a rumour reaching someone by a shorter path wins over the same rumour arriving
    /// the long way round.
    pub fn hear(
        &mut self,
        subject: Subject,
        claim: Claim,
        from_confidence: f32,
        tick: u64,
    ) -> bool {
        let confidence = (from_confidence * RETELL_DECAY).clamp(0.0, 1.0);
        if confidence < WORTH_SAYING {
            return false;
        }
        // Anything held on better evidence — or the same evidence, more strongly — stands.
        if let Some(held) = self.of(subject, claim) {
            let stronger_kind =
                held.provenance.base_confidence() > Provenance::Told.base_confidence();
            if stronger_kind || held.confidence >= confidence {
                return false;
            }
        }
        // A contradicting claim on firsthand/witnessed evidence also stands: being told the opposite of
        // what you saw does not change your mind, and that is the asymmetry FVS-O-5 will attack.
        if let Some(opposite) = claim.contradicts() {
            if let Some(held) = self.of(subject, opposite) {
                if held.provenance.base_confidence() > Provenance::Told.base_confidence() {
                    return false;
                }
            }
        }
        self.learn(subject, claim, Provenance::Told, tick);
        self.set_confidence(subject, claim, confidence);
        true
    }
}

/// One thing that was said, for the windowed dialogue layer to voice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Telling {
    pub teller: usize,
    pub listener: usize,
    pub subject: Subject,
    pub claim: Claim,
}

/// What was said this tick.
///
/// **Cleared and refilled every tick**, so it cannot grow without bound, and read only by the windowed
/// dialogue adapter — nothing pinned consumes it, so voicing a rumour cannot feed back into the sim.
#[derive(Resource, Debug, Default)]
pub struct RecentTellings(pub Vec<Telling>);

/// Operatives within earshot swap what they know.
pub fn spread_beliefs_by_conversation(
    clock: Res<crate::session::RunClock>,
    mut said: ResMut<RecentTellings>,
    mut operatives: Query<(&SquadMember, &Transform, &mut Knowledge), With<Unit>>,
) {
    said.0.clear();
    if clock.ticks % TELL_INTERVAL != 0 {
        return;
    }
    // Snapshot BEFORE any write. Everything below reads this, so a belief cannot hop twice in one tick
    // and the outcome cannot depend on the order the writes land.
    let mut roster: Vec<(usize, Vec3, Knowledge)> =
        operatives.iter().map(|(m, t, k)| (m.0, t.translation, *k)).collect();
    if roster.len() < 2 {
        return;
    }
    crate::sort_total!(&mut roster, |r: &(usize, Vec3, Knowledge)| r.0);

    let reach_sq = EARSHOT * EARSHOT;
    // Every transfer is decided from the snapshot before any of them is applied.
    let mut transfers: Vec<(usize, Subject, Claim, f32, usize)> = Vec::new();
    for i in 0..roster.len() {
        for j in (i + 1)..roster.len() {
            let (a_member, a_pos, a_knows) = roster[i];
            let (b_member, b_pos, b_knows) = roster[j];
            if (a_pos.xz() - b_pos.xz()).length_squared() > reach_sq {
                continue;
            }
            // Both directions: a conversation is not one-way, and making it so would let roster order
            // decide who learns from whom — the order-dependence the sort exists to remove.
            for (from_member, from_knows, to_member) in
                [(a_member, a_knows, b_member), (b_member, b_knows, a_member)]
            {
                for subject in Subject::ALL {
                    for claim in Claim::ALL {
                        let Some(belief) = from_knows.of(subject, claim) else { continue };
                        // Skip what would arrive already dead, so the transfer list stays the set of
                        // things actually worth saying.
                        if belief.confidence * RETELL_DECAY < WORTH_SAYING {
                            continue;
                        }
                        transfers.push((to_member, subject, claim, belief.confidence, from_member));
                    }
                }
            }
        }
    }

    for (listener, subject, claim, confidence, teller) in transfers {
        let Some((_, _, mut knowledge)) =
            operatives.iter_mut().find(|(m, _, _)| m.0 == listener)
        else {
            continue;
        };
        if knowledge.hear(subject, claim, confidence, clock.ticks) {
            said.0.push(Telling { teller, listener, subject, claim });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn veteran() -> Knowledge {
        let mut k = Knowledge::default();
        k.learn(Subject::BearCopies, Claim::Lethal, Provenance::Firsthand, 1);
        k
    }

    #[test]
    fn a_retelling_is_weaker_than_the_telling() {
        // The whole point of the channel: hearsay must not be as good as experience, or provenance
        // stops meaning anything.
        let src = veteran();
        let firsthand = src.of(Subject::BearCopies, Claim::Lethal).expect("held").confidence;
        let mut heard = Knowledge::default();
        assert!(heard.hear(Subject::BearCopies, Claim::Lethal, firsthand, 5));
        let told = heard.of(Subject::BearCopies, Claim::Lethal).expect("heard").confidence;
        assert!(told < firsthand, "{told} should be below {firsthand}");
        assert_eq!(heard.of(Subject::BearCopies, Claim::Lethal).expect("heard").provenance, Provenance::Told);
    }

    #[test]
    fn a_rumour_dies_out_along_a_chain_instead_of_saturating_the_squad() {
        // Without decay a belief would reach everyone at full strength and the roster screen would show
        // five identical certainties nobody earned.
        let mut c = veteran().of(Subject::BearCopies, Claim::Lethal).expect("held").confidence;
        let mut hops = 0;
        loop {
            let mut next = Knowledge::default();
            if !next.hear(Subject::BearCopies, Claim::Lethal, c, 1) {
                break;
            }
            c = next.of(Subject::BearCopies, Claim::Lethal).expect("heard").confidence;
            hops += 1;
            assert!(hops < 50, "the chain must terminate");
        }
        assert!(hops >= 2, "a rumour should survive at least a couple of hops, got {hops}");
        assert!(hops <= 8, "a rumour should not cross an arbitrarily long chain, got {hops}");
    }

    #[test]
    fn hearsay_never_overwrites_what_an_operative_saw_themselves() {
        // The asymmetry FVS-O-5 will attack: you cannot talk someone out of their own experience.
        let mut k = veteran();
        let before = k.of(Subject::BearCopies, Claim::Lethal).expect("held");
        assert!(!k.hear(Subject::BearCopies, Claim::Lethal, 0.99, 9), "a retelling must not take");
        assert_eq!(k.of(Subject::BearCopies, Claim::Lethal), Some(before));

        // ...including a contradicting one.
        assert!(!k.hear(Subject::BearCopies, Claim::Harmless, 0.99, 9));
        assert!(k.of(Subject::BearCopies, Claim::Harmless).is_none());
    }

    #[test]
    fn a_shorter_path_beats_the_same_rumour_arriving_the_long_way() {
        let mut k = Knowledge::default();
        assert!(k.hear(Subject::Parasite, Claim::Lethal, 0.30, 1), "the weak path arrives first");
        let weak = k.of(Subject::Parasite, Claim::Lethal).expect("heard").confidence;
        assert!(k.hear(Subject::Parasite, Claim::Lethal, 0.85, 2), "a stronger retelling replaces it");
        assert!(k.of(Subject::Parasite, Claim::Lethal).expect("heard").confidence > weak);
        // ...but not the reverse.
        assert!(!k.hear(Subject::Parasite, Claim::Lethal, 0.30, 3));
    }

    #[test]
    fn every_belief_an_operative_can_hold_has_something_to_say() {
        // The guard `containment_hud::channel_name` and `research_hud::subject_name` both use: a new
        // Subject or Claim must not reach a speech balloon as a debug string.
        for subject in Subject::ALL {
            for claim in Claim::ALL {
                let line = line_for(subject, claim);
                assert!(!line.is_empty(), "{subject:?}/{claim:?} has no line");
                assert!(
                    line.ends_with('.') || line.ends_with('!'),
                    "{subject:?}/{claim:?} should read as spoken dialogue: {line}"
                );
            }
        }
    }

    #[test]
    fn nothing_below_the_floor_is_worth_saying() {
        let mut k = Knowledge::default();
        assert!(!k.hear(Subject::Crabs, Claim::Lethal, WORTH_SAYING * 0.5, 1));
        assert!(k.of(Subject::Crabs, Claim::Lethal).is_none(), "a dead rumour leaves no trace");
    }
}

/// What an operative actually says when they pass this on.
///
/// Lives here rather than in `crate::dialogue` because it is a pure function of the belief, so it is
/// testable without a `App` and a new [`Subject`] or [`Claim`] fails to compile until someone decides
/// what the squad says about it — the same `match`-not-fallback discipline `containment_hud`'s channel
/// names use. The dialogue layer only decides *when* to say it.
pub fn line_for(subject: Subject, claim: Claim) -> &'static str {
    match (subject, claim) {
        (Subject::BearCopies, Claim::Lethal) => "The copies aren't like the bear. They'll kill you.",
        (Subject::BearCopies, Claim::Harmless) => "The copies never touched me.",
        (Subject::BearCopies, Claim::Containable) => "You out-stare the copies. They stop building.",
        (Subject::BuilderBear, Claim::Lethal) => "Don't turn your back on the bear.",
        (Subject::BuilderBear, Claim::Harmless) => "The bear itself won't hurt you. It just builds.",
        (Subject::BuilderBear, Claim::Containable) => "Keep eyes on the bear and it can't scavenge.",
        (Subject::ComfortBlob, Claim::Lethal) => "That blob is not as friendly as it looks.",
        (Subject::ComfortBlob, Claim::Harmless) => "The blob's harmless. It just wants you calm.",
        (Subject::ComfortBlob, Claim::Containable) => "Holster up near the blob. It comes to you.",
        (Subject::Parasite, Claim::Lethal) => "If it gets in you and nobody cuts it out, you're gone.",
        (Subject::Parasite, Claim::Harmless) => "The parasite won't finish the job. I've seen it stall.",
        (Subject::Parasite, Claim::Containable) => "You can cut it out clean if you're fast.",
        (Subject::Crabs, Claim::Lethal) => "One crab's nothing. Ten will take you apart.",
        (Subject::Crabs, Claim::Harmless) => "Crabs scatter if you hold the line.",
        (Subject::Crabs, Claim::Containable) => "Seal the nests and they stop coming back.",
        (Subject::Watcher, Claim::Lethal) => "It moves when you're not looking. Don't stop looking.",
        (Subject::Watcher, Claim::Harmless) => "The watcher just watches. It's never touched anyone.",
        (Subject::Watcher, Claim::Containable) => "Keep it in someone's line of sight. Always.",
    }
}

/// Set the decayed confidence after [`Knowledge::learn`] has stamped the provenance.
///
/// Private to this module on purpose: everywhere else confidence is a *consequence* of provenance, and
/// the `Told` chain is the one place that has to override it. A child module can reach the parent's
/// private field, which is exactly the visibility this wants — no public setter for anyone else to use.
impl Knowledge {
    fn set_confidence(&mut self, subject: Subject, claim: Claim, confidence: f32) {
        if let Some(b) = self.beliefs[subject.index()][claim.index()].as_mut() {
            b.confidence = confidence;
        }
    }
}
