//! **Archetype 2 — area-denial quarantine** (FVS-B-6) and **archetype 3 — source elimination**
//! (FVS-B-7).
//!
//! These are genuinely different verbs from the thrown device, and the backlog is explicit that they
//! must not be pitched as "one thrown sphere":
//!
//! | Archetype | What it acts on | Yields |
//! |---|---|---|
//! | [`super::device`] | one **body** | a specimen |
//! | [`Quarantine`] | a bounded **region** | a specimen |
//! | [`Capped`] | a **structure** | a *secured* flag, and **no specimen** |
//!
//! # Quarantine: the region is the container
//!
//! For SCP-610 the canon procedure is isolating an *area*, not point-capturing a body — there is no
//! single body to capture. So a [`Quarantine`] bounds a region, and any [`Quarantinable`] anomaly inside
//! it is under containment for as long as it stays inside. A breach — the anomaly crossing back out — is
//! a **cancel**, not a mid-hold lapse: the containment attempt itself has ended, so the hold is
//! discarded regardless of the rule's [`super::rule::OnBreak`] policy.
//!
//! # Capping: an honest name for kill-with-no-reward
//!
//! Sealing a crab nest halts breeding and secures the site, and it deliberately grants **no specimen**.
//! It reuses none of the capture machinery: no `Containment`, no `Contained`, no hook. That is the point
//! — it is the archetype that is honestly "kill the source for no specimen", and giving it a specimen
//! would quietly undo the pivot the whole backlog is built on.
//!
//! # Determinism
//!
//! Both systems are per-entity and order-independent: an anomaly tests itself against *all* quarantines
//! (an `any()` — commutative), and a capped nest only reads its own marker. No pick, no shared counter,
//! no budget, so no canonical sort is required.

use bevy::prelude::*;

use super::state::{Containment, Contained, Phase};

/// A bounded containment region. Anything [`Quarantinable`] inside it is under containment.
#[derive(Component, Debug, Clone, Copy)]
pub struct Quarantine {
    /// Radius from this entity's own `Transform`, in metres.
    pub radius: f32,
}

/// Marks an anomaly that a [`Quarantine`] can hold — the area-denial roster (SCP-610).
///
/// A marker inserted **at spawn and never toggled**, so it does not churn the hashed archetype (the rule
/// `scp1048` states and `containment::state` follows). It is a species property, not a state.
#[derive(Component)]
pub struct Quarantinable;

/// Begin containment for quarantined anomalies, and break it on a breach.
///
/// Deliberately does *not* evaluate the rule — [`super::state::tick_containment`] still owns that, so a
/// quarantined anomaly is held by the same one path as a device-captured one. This system only opens and
/// closes the attempt based on containment geometry.
pub fn tick_quarantine(
    quarantines: Query<(&Quarantine, &Transform)>,
    mut anomalies: Query<
        (&mut Containment, &Transform),
        (With<Quarantinable>, Without<Contained>, Without<Quarantine>),
    >,
) {
    for (mut containment, anomaly_tf) in &mut anomalies {
        // Order-independent: `any` over the quarantines is commutative, and each anomaly writes only
        // its own component.
        let inside = quarantines.iter().any(|(q, q_tf)| {
            q_tf.translation.distance(anomaly_tf.translation) <= q.radius
        });
        match (inside, containment.phase()) {
            // Entering a live quarantine opens the attempt.
            (true, Phase::Uncontained) => containment.begin(),
            // A breach ends the attempt outright — the region stopped containing it, which is not the
            // same as a condition lapsing while it is still inside (that is `OnBreak`'s job).
            (false, Phase::BeingContained) => containment.cancel(),
            _ => {}
        }
    }
}

/// A sealed structure. Inserted once when the squad caps it; never removed.
///
/// Terminal and one-way, so a marker is the right shape here (same rule as [`Contained`]) — but note it
/// carries **no hook**: capping grants nothing. See the module docs.
#[derive(Component)]
pub struct Capped;

/// Set when at least one structure has been capped this run — the "site secured" flag B-7 asks for.
///
/// A count rather than a bool so the HUD can say *how much* of the infestation is sealed; run-scoped
/// state, reset by `session::reset_run` semantics (it lives on the resource, and a new run re-derives it
/// from the surviving nests).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SiteSecured {
    /// How many structures have been capped.
    pub capped: usize,
    /// How many structures exist in total (capped or not).
    pub total: usize,
}

impl SiteSecured {
    /// Whether every structure in the level is sealed.
    pub fn fully_secured(&self) -> bool {
        self.total > 0 && self.capped == self.total
    }
}

/// Recompute [`SiteSecured`] from the live nests.
///
/// Derived every tick rather than incremented on capping: a nest can also be destroyed outright, and a
/// derived count cannot drift out of step with the world the way an incremented one can. Counting is
/// order-independent.
pub fn track_secured_sites(
    mut secured: ResMut<SiteSecured>,
    nests: Query<Option<&Capped>, With<crate::nest::Nest>>,
) {
    let mut total = 0usize;
    let mut capped = 0usize;
    for cap in &nests {
        total += 1;
        if cap.is_some() {
            capped += 1;
        }
    }
    let next = SiteSecured { capped, total };
    if *secured != next {
        *secured = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containment::rule::{ContainmentRule, FieldCondition, OnBreak, Sign};

    fn rule() -> ContainmentRule {
        ContainmentRule {
            requires: vec![FieldCondition { channel: 0, sign: Sign::AtLeast, threshold: 0.5 }],
            hold_secs: 1.0,
            break_on_fail: OnBreak::Keep,
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_systems(Update, tick_quarantine);
        app
    }

    #[test]
    fn an_anomaly_inside_the_region_comes_under_containment() {
        let mut app = app();
        app.world_mut().spawn((Quarantine { radius: 3.0 }, Transform::default()));
        let a = app
            .world_mut()
            .spawn((
                Containment::new(rule(), crate::knowledge::Subject::ComfortBlob),
                Quarantinable,
                Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Containment>(a).expect("containment").phase(),
            Phase::BeingContained,
            "the region itself opens the attempt — no device is thrown"
        );
    }

    #[test]
    fn an_anomaly_outside_every_region_is_untouched() {
        let mut app = app();
        app.world_mut().spawn((Quarantine { radius: 3.0 }, Transform::default()));
        let a = app
            .world_mut()
            .spawn((
                Containment::new(rule(), crate::knowledge::Subject::ComfortBlob),
                Quarantinable,
                Transform::from_translation(Vec3::new(50.0, 0.0, 0.0)),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Containment>(a).expect("containment").phase(),
            Phase::Uncontained
        );
    }

    #[test]
    fn a_breach_ends_the_attempt_and_discards_the_hold_even_under_keep() {
        // The distinction this test exists for: `OnBreak::Keep` banks progress across a *condition*
        // lapsing while the anomaly is still inside. Leaving the region is a different event — the
        // containment attempt is over — so the hold goes regardless of the policy.
        let mut app = app();
        app.world_mut().spawn((Quarantine { radius: 3.0 }, Transform::default()));
        let a = app
            .world_mut()
            .spawn((Containment::new(rule(), crate::knowledge::Subject::ComfortBlob), Quarantinable, Transform::default()))
            .id();
        app.update();

        {
            let mut c = app.world_mut().get_mut::<Containment>(a).expect("containment");
            assert_eq!(c.phase(), Phase::BeingContained);
            c.advance_for_test(0.5, true);
            assert!(c.held_secs() > 0.0);
        }

        // Walk it out of the region.
        app.world_mut().get_mut::<Transform>(a).expect("transform").translation =
            Vec3::new(50.0, 0.0, 0.0);
        app.update();

        let c = app.world().get::<Containment>(a).expect("containment");
        assert_eq!(c.phase(), Phase::Uncontained, "a breach ends the attempt");
        assert_eq!(c.held_secs(), 0.0, "and discards the hold even under OnBreak::Keep");
    }

    #[test]
    fn overlapping_regions_hold_an_anomaly_in_either_of_them() {
        // `any()` over the regions: two quarantines are not two attempts, and stepping from one into
        // the other is not a breach.
        let mut app = app();
        app.world_mut().spawn((Quarantine { radius: 2.0 }, Transform::default()));
        app.world_mut().spawn((
            Quarantine { radius: 2.0 },
            Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)),
        ));
        let a = app
            .world_mut()
            .spawn((Containment::new(rule(), crate::knowledge::Subject::ComfortBlob), Quarantinable, Transform::default()))
            .id();
        app.update();
        {
            let mut c = app.world_mut().get_mut::<Containment>(a).expect("containment");
            c.advance_for_test(0.5, true);
        }
        // Move into the SECOND region — still contained, hold preserved.
        app.world_mut().get_mut::<Transform>(a).expect("transform").translation =
            Vec3::new(3.0, 0.0, 0.0);
        app.update();
        let c = app.world().get::<Containment>(a).expect("containment");
        assert_eq!(c.phase(), Phase::BeingContained, "moving between regions is not a breach");
        assert!(c.held_secs() > 0.0, "and does not discard the hold");
    }

    #[test]
    fn secured_tracks_capped_versus_total_and_only_reads_full_when_all_are_sealed() {
        let a = SiteSecured { capped: 0, total: 4 };
        assert!(!a.fully_secured());
        let b = SiteSecured { capped: 4, total: 4 };
        assert!(b.fully_secured());
        // A level with no structures is not "secured" — that would report success for a level the
        // archetype does not apply to.
        let none = SiteSecured { capped: 0, total: 0 };
        assert!(!none.fully_secured());
    }
}
