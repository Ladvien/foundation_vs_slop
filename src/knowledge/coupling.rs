//! **What knowledge DOES** (FVS-O-2) — the cost half, wired into FEAR.
//!
//! `HANDOFF.md` claimed this shipped "wired inert at gain zero". It had not shipped at all:
//! [`super::Knowledge::fear_scale`] and [`super::Knowledge::can_read_rule`] existed as pure functions
//! with **no callers anywhere in the repo**, and there was no gain constant and no config knob — the
//! "gain zero" staging existed only as a literal `0.0` passed by one unit test. This is the wiring.
//!
//! # The asymmetry is the thesis
//!
//! Understanding a thing is what makes it frightening, **and** the only way to contain it (design doc
//! §3.4). A confident `Lethal` belief raises that operative's FEAR when the subject is in front of
//! them — they flee sooner, aim worse, break a containment hold — while a `Containable` belief is what
//! lets them read the anomaly's rule clauses at all. Same trade the research economy encodes, pushed
//! down onto the individual.
//!
//! # It ships at gain ZERO
//!
//! ⚠️ The paragraph below is kept because it records the original design, but its factual claim is
//! **stale**: `SimTuning::default().belief_fear_gain` has been `0.4` since the coupling was turned on,
//! and `a_known_lethal_subject_is_more_frightening_than_an_unknown_one` asserts it is `> 0.0`. The
//! bit-exact-no-op argument now applies to `strain` instead, which really does ship at zero in every
//! headless rollout.
//!
//! `sim.belief_fear_gain` defaults to `0.0`, which makes [`apply_belief_fear`] a **bit-exact no-op**:
//! `fear_scale` returns exactly 1.0 at gain 0 for every belief, and multiplying an `f32` by 1.0 is the
//! identity. So the goldens do not move, and turning it on is a deliberate act that gets its own
//! measured re-pin. Same two-step staging FVS-B-8 used, and the same one `ai::drives`'
//! `a_zero_gain_din_is_a_bit_exact_no_op` established for the acoustic din.
//!
//! # Determinism
//!
//! This runs on `FixedUpdate` in the pinned core, so it is held to the full standard:
//!
//! * The reduction over nearby anomalies is a **`max`**, which is order-independent in `f32` — the
//!   same argument `drives::track_max_target` makes and pins. No canonical sort is needed because
//!   there is no pick, no shared counter and no RNG draw.
//! * Each operative writes **only its own** `Drives`, from its own `Knowledge` plus shared read-only
//!   state, so ECS iteration order cannot change the outcome.
//! * Ordered `.after(AiSet::Drives)` so it scales the fear the drive rules already settled this tick,
//!   rather than a half-updated value whose contents depend on schedule accident — the same
//!   "read settled state" edge `containment::tick_containment` uses.

use bevy::prelude::*;

use super::{Claim, Knowledge, Provenance, Subject};
use crate::ai::drives::{DriveId, Drives};
use crate::squad::Unit;

/// How far an operative must be from an anomaly for their beliefs about it to bite.
///
/// A belief is about a *kind of thing* and only acts when that kind is **present** — that is the whole
/// distinction from a level, which would apply everywhere forever. The radius is generous because the
/// point is "it is in the room with me", not a precise cone.
pub const PRESENCE_RADIUS: f32 = 12.0;

/// Everything on the field an operative can hold beliefs *about*, with its position.
///
/// Assembled from two queries rather than one marker component, deliberately: `Containment` already
/// carries a `subject` for 999 / 1048 / an extracted parasite, and `Scp1048` already distinguishes the
/// benign original from its hostile copies. Adding a third `AnomalyKind(Subject)` component to say what
/// those two already know would be a second source of truth **and** another component on hashed
/// archetypes.
fn present_subjects(
    contained: &Query<(&Transform, &crate::containment::Containment)>,
    bears: &Query<(&Transform, &crate::scp1048::Scp1048)>,
) -> Vec<(Vec3, Subject)> {
    let mut out: Vec<(Vec3, Subject)> = Vec::new();
    for (tf, c) in contained.iter() {
        out.push((tf.translation, c.subject));
    }
    for (tf, bear) in bears.iter() {
        out.push((tf.translation, subject_of_bear(bear)));
    }
    out
}

/// Which *kind* a bear is. The benign original and its hostile copies are different subjects, and that
/// distinction is the entire reason `Subject` separates them: an operative can rationally believe one
/// is harmless and the other lethal, and confusing the two is the FVS-O-5 misinformation case.
pub fn subject_of_bear(bear: &crate::scp1048::Scp1048) -> Subject {
    match bear.variant {
        crate::scp1048::Scp1048Variant::Original => Subject::BuilderBear,
        _ => Subject::BearCopies,
    }
}

/// Scale each operative's FEAR by what they believe about what is in front of them.
pub fn apply_belief_fear(
    sim: Res<crate::sim::SimTuning>,
    contained: Query<(&Transform, &crate::containment::Containment)>,
    bears: Query<(&Transform, &crate::scp1048::Scp1048)>,
    mut units: Query<(&Transform, &Knowledge, &mut Drives), With<Unit>>,
) {
    // ── STRAIN: the floor a worn-out veteran cannot get below ────────────────────────────────────
    //
    // First, and deliberately **outside** both early returns below: strain is not a reaction to
    // anything being present. It is what is left over from the last expedition, so it applies in an
    // empty corridor exactly as much as beside a contained anomaly. That is what makes it a
    // counter-pressure to veteran lock-in rather than a second kind of belief.
    //
    // Written as a raised floor rather than a multiplier so it can never *lower* fear, and so it
    // compounds correctly with the belief scaling that follows: knowledge amplifies the strained
    // baseline instead of replacing it.
    //
    // ⚠️ **Bit-exact while nobody is strained**, which is every headless rollout: strain accrues only
    // on a completed expedition, and `AppState::Debrief` does not exist in the harness. The inner
    // guard means an unstrained operative's `Drives` is never even dereferenced mutably, so no
    // `Changed<Drives>` is raised either.
    let floor_at_full = sim.strain.fear_floor;
    if floor_at_full > 0.0 {
        for (_, knowledge, mut drives) in &mut units {
            if knowledge.strain <= 0.0 {
                continue;
            }
            let floor = (floor_at_full * knowledge.strain).clamp(0.0, 1.0);
            if drives.v[DriveId::FEAR.0] < floor {
                drives.v[DriveId::FEAR.0] = floor;
            }
        }
    }

    let gain = sim.belief_fear_gain;
    if gain == 0.0 {
        // Bailing rather than multiplying by an identity keeps it free as well as bit-exact, and
        // makes the "this is off" state obvious at the top of the function.
        return;
    }
    let present = present_subjects(&contained, &bears);
    if present.is_empty() {
        return;
    }
    let radius_sq = PRESENCE_RADIUS * PRESENCE_RADIUS;
    for (tf, knowledge, mut drives) in &mut units {
        // `max`, not a sum or a product: an operative beside two frightening things is as afraid as the
        // more frightening one. A running product would also make the result order-dependent, which is
        // exactly what `drives::track_max_is_independent_of_source_order` exists to prevent.
        let mut scale = 1.0f32;
        for (pos, subject) in &present {
            if (pos.xz() - tf.translation.xz()).length_squared() > radius_sq {
                continue;
            }
            scale = scale.max(knowledge.fear_scale(*subject, gain));
        }
        drives.v[DriveId::FEAR.0] = (drives.v[DriveId::FEAR.0] * scale).clamp(0.0, 1.0);
    }
}

/// Firsthand acquisition (FVS-O-1b): being struck by a hostile copy teaches you it is lethal.
///
/// A **request-free** direct write, unlike `parasite::CureRequest`, because there is exactly one writer
/// already — the strike system that knows a blow connected. Routing it through a message would add a
/// second hop without adding a second author.
pub fn learn_from_a_blow(knowledge: &mut Knowledge, subject: Subject, tick: u64) {
    knowledge.learn(subject, Claim::Lethal, Provenance::Firsthand, tick);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_gain_belief_leaves_fear_bit_identical() {
        // The shipped default must be a *bit-exact* no-op, not merely a small one — otherwise turning
        // the feature on is indistinguishable from a regression, and the goldens move for a mechanic
        // nobody enabled.
        let mut k = Knowledge::default();
        k.learn(Subject::BearCopies, Claim::Lethal, Provenance::Firsthand, 10);
        assert_eq!(k.fear_scale(Subject::BearCopies, 0.0).to_bits(), 1.0f32.to_bits());
        for f in [0.0f32, 0.25, 0.5, 0.9, 1.0] {
            assert_eq!(
                (f * k.fear_scale(Subject::BearCopies, 0.0)).to_bits(),
                f.to_bits(),
                "gain 0 must not perturb FEAR at all"
            );
        }
    }

    #[test]
    fn knowing_a_thing_is_lethal_makes_it_more_frightening_than_not_knowing() {
        // The cost half of the trade, and the whole reason knowledge is not a level: it makes the
        // operative *worse* in the presence of the thing they understand.
        let mut knows = Knowledge::default();
        knows.learn(Subject::BearCopies, Claim::Lethal, Provenance::Firsthand, 1);
        let ignorant = Knowledge::default();
        assert!(
            knows.fear_scale(Subject::BearCopies, 0.5) > ignorant.fear_scale(Subject::BearCopies, 0.5)
        );
        assert_eq!(
            ignorant.fear_scale(Subject::BearCopies, 0.5),
            1.0,
            "never having met it must leave fear UNCHANGED — not raise it cautiously"
        );
    }

    /// The **live** path, driven through a real `App` — because M-1's warning applies here verbatim:
    /// "the goldens did NOT move, and that is a warning, not a reassurance". The 1800-tick golden run
    /// has no synthetic player, so no operative ever acquires a belief and this system's contended path
    /// is never exercised by it. This test is what actually covers the mechanic.
    #[test]
    fn a_knowing_operative_beside_the_subject_ends_up_more_afraid_than_an_ignorant_one() {
        use crate::ai::drives::{DriveId, Drives, DRIVE_COUNT};
        use crate::containment::Containment;
        use crate::containment::rule::{ContainmentRule, FieldCondition, OnBreak, Sign};

        fn run(gain: f32, knows: bool) -> f32 {
            let mut app = App::new();
            let mut sim = crate::sim::SimTuning::default();
            sim.belief_fear_gain = gain;
            app.insert_resource(sim).add_systems(Update, apply_belief_fear);
            // The subject, standing right there. `Containment` is what carries a subject on the field.
            app.world_mut().spawn((
                Transform::from_translation(Vec3::ZERO),
                Containment::new(
                    ContainmentRule {
                        requires: vec![FieldCondition { channel: 0, sign: Sign::AtLeast, threshold: 0.5 }],
                        hold_secs: 1.0,
                        break_on_fail: OnBreak::Reset,
                    },
                    Subject::ComfortBlob,
                ),
            ));
            let mut k = Knowledge::default();
            if knows {
                k.learn(Subject::ComfortBlob, Claim::Lethal, Provenance::Firsthand, 1);
            }
            let mut drives = Drives { v: [0.0; DRIVE_COUNT] };
            drives.v[DriveId::FEAR.0] = 0.5;
            let e = app.world_mut().spawn((Unit, Transform::from_translation(Vec3::ZERO), k, drives)).id();
            app.update();
            app.world().get::<Drives>(e).expect("drives").v[DriveId::FEAR.0]
        }

        let ignorant = run(0.4, false);
        let knowing = run(0.4, true);
        assert!(
            knowing > ignorant,
            "an operative who knows the thing is lethal must fear it MORE ({knowing} vs {ignorant}) — \
             that cost is what stops knowledge being a strict upgrade"
        );
        assert_eq!(ignorant.to_bits(), 0.5f32.to_bits(), "knowing nothing must leave FEAR untouched");
        // ...and the shipped gain is no longer zero, so the mechanic is actually live.
        assert!(
            crate::sim::SimTuning::default().belief_fear_gain > 0.0,
            "FVS-O-2 was turned on; a default back at 0.0 would silently disable it again"
        );
    }

    #[test]
    fn a_belief_about_one_bear_does_not_transfer_to_the_other() {
        // The benign original and the hostile copies are different subjects on purpose. If a belief
        // leaked between them, FVS-O-5's whole misinformation case would be unmodellable.
        let mut k = Knowledge::default();
        learn_from_a_blow(&mut k, Subject::BearCopies, 5);
        assert!(k.knows(Subject::BearCopies));
        assert!(!k.knows(Subject::BuilderBear), "beliefs must not bleed across subjects");
    }

    #[test]
    fn being_struck_twice_reinforces_without_reaching_certainty() {
        let mut k = Knowledge::default();
        learn_from_a_blow(&mut k, Subject::BearCopies, 1);
        let once = k.of(Subject::BearCopies, Claim::Lethal).expect("learned").confidence;
        learn_from_a_blow(&mut k, Subject::BearCopies, 2);
        let twice = k.of(Subject::BearCopies, Claim::Lethal).expect("learned").confidence;
        assert!(twice > once, "a second blow should be more convincing than the first");
        assert!(twice < 1.0, "an operative is evidence, never proof");
    }
}
