//! **Containment state, the tick that runs it, and the outcome hook** (FVS-B-2 / B-3 / B-4).
//!
//! # A value field for the state machine, a marker only for the terminal event
//!
//! The backlog's engine baseline asks for four *marker components*
//! (`Uncontained`/`BeingContained`/`Contained`/`Killed`) with exactly one present at a time. Shipped as
//! written, that would toggle markers on hashed entities every time containment started or broke — and
//! this codebase has a standing rule against exactly that, stated in `crate::scp1048`: *"Every component
//! is inserted at spawn and never toggled — a flipped marker would split the hashed archetype and make
//! ECS iteration order run-dependent."* `crate::parasite`'s `Infestation` and `MancaMood` are the
//! established idiom: **state that changes is a value field; only a one-way terminal transition earns a
//! marker.**
//!
//! So:
//! * [`Containment`] — a value component present on every containable anomaly **from spawn**, holding
//!   the [`Phase`], the accumulated hold, and the anomaly's rule. This is what [`tick_containment`]
//!   mutates, and the archetype never moves.
//! * [`Contained`] — a marker inserted **once**, at completion, never removed. It exists solely to carry
//!   the reward hook, and a one-way terminal insert is the case markers are for.
//!
//! # "Killing yields nothing" is enforced by absence, not by a branch
//!
//! There is deliberately **no `Killed` component**. The reward lives in an `on_add` hook on
//! [`Contained`]; death is already `Health <= 0` → despawn, and there is no hook, no marker, and no
//! branch anywhere on that path. A `Killed` marker with no reward hook would still be a *place* someone
//! could later attach one; having no component at all means the only way to produce a specimen is to
//! insert `Contained`, which only [`tick_containment`] does, only on completion. That is the type-system
//! enforcement the design asks for, taken one step further.
//!
//! # Determinism
//!
//! [`tick_containment`] runs on `FixedUpdate` and is in the pinned core. Every anomaly's update is a
//! **pure function of its own transform, its own rule, and the shared field** — no shared counter, no
//! budget, no pick, no RNG draw — so iteration order cannot change the outcome and no canonical sort is
//! required (contrast every site that *picks* from a query; see `tests/determinism_lint.rs`).

use bevy::prelude::*;

use super::rule::{ContainmentRule, OnBreak};

/// Where an anomaly is in the capture process.
///
/// A value field, never a set of markers — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// No containment attempt is in progress. The resting state.
    #[default]
    Uncontained,
    /// A device or quarantine is active and the hold timer is live.
    BeingContained,
    /// Captured. Terminal — [`Contained`] has been inserted and the specimen granted.
    Contained,
}

/// The containment state of one anomaly. Present from spawn on anything capturable.
#[derive(Component, Debug, Clone)]
pub struct Containment {
    /// The current phase.
    phase: Phase,
    /// Seconds of satisfied hold accumulated so far.
    held_secs: f32,
    /// What it takes to capture this anomaly.
    pub rule: ContainmentRule,
}

impl Containment {
    /// A fresh, uncontained anomaly carrying `rule`.
    pub fn new(rule: ContainmentRule) -> Self {
        Self { phase: Phase::Uncontained, held_secs: 0.0, rule }
    }

    /// The current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Seconds of satisfied hold accumulated so far.
    pub fn held_secs(&self) -> f32 {
        self.held_secs
    }

    /// Progress toward capture in `[0, 1]` — what the containment HUD (FVS-L-1) draws.
    pub fn progress(&self) -> f32 {
        if self.rule.hold_secs > 0.0 {
            (self.held_secs / self.rule.hold_secs).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Begin a containment attempt. Idempotent, and **never re-opens a completed capture** — a device
    /// thrown at an already-contained anomaly does nothing rather than restarting its timer.
    pub fn begin(&mut self) {
        if self.phase == Phase::Uncontained {
            self.phase = Phase::BeingContained;
        }
    }

    /// Abandon an in-progress attempt (the device is destroyed, the quarantine drops). Returns to
    /// `Uncontained` and discards accumulated hold regardless of [`OnBreak`] — this is the attempt
    /// ending, not a condition lapsing mid-hold.
    pub fn cancel(&mut self) {
        if self.phase == Phase::BeingContained {
            self.phase = Phase::Uncontained;
            self.held_secs = 0.0;
        }
    }

    /// Test-only driver for [`Self::advance`], so sibling modules can script a capture without a field
    /// grid. `pub(crate)` + `cfg(test)`: it cannot exist in a shipped binary, so it is not a second
    /// production path into the state machine.
    #[cfg(test)]
    pub(crate) fn advance_for_test(&mut self, dt: f32, satisfied: bool) -> bool {
        self.advance(dt, satisfied)
    }

    /// Advance one tick. Pure: returns whether the capture **completed on this tick**, so the caller
    /// owns the one side effect (inserting [`Contained`]). Split out so the whole rule — accumulate,
    /// break policy, completion — is unit-testable without an `App`.
    fn advance(&mut self, dt: f32, satisfied: bool) -> bool {
        if self.phase != Phase::BeingContained {
            return false;
        }
        if satisfied {
            self.held_secs += dt;
            if self.held_secs >= self.rule.hold_secs {
                self.phase = Phase::Contained;
                return true;
            }
        } else if self.rule.break_on_fail == OnBreak::Reset {
            self.held_secs = 0.0;
        }
        false
    }
}

/// Terminal marker: this anomaly was **captured**. Inserted once by [`tick_containment`], never
/// removed. Its `on_add` hook is the single place a specimen is ever granted (see the module docs).
#[derive(Component)]
#[component(on_add = grant_specimen)]
pub struct Contained;

/// A captured anomaly, banked. Deliberately **not** `session::run_scoped()`: a specimen is the whole
/// point of an expedition and must outlive it — this is the roguelite meta-progress boundary (FVS-G-3).
///
/// Carries no `Transform` and no `Health`, so it is invisible to `sim_harness::snapshot_hash` and to the
/// liveness actor count. FVS-D-4 will link it to the persistent Site with a relationship; until the Site
/// exists (FVS-G-1) it simply accumulates.
#[derive(Component, Debug, Clone, Copy)]
pub struct Specimen {
    /// The entity that was captured, as it was at capture time. Recorded for the research posterior
    /// (FVS-E-1) to key on; the anomaly itself may be despawned once extraction lands.
    pub captured: Entity,
    /// The run tick the capture completed on.
    ///
    /// **A stable ordering key, and that is its job.** `SiteSpecimens` (FVS-D-4) is a Bevy relationship
    /// target whose order is *attach* order, not a total order, so anything that assigns specimens to
    /// containment cells is a pick and needs a key that does not come from ECS iteration. This is that
    /// key: it is a pure function of when the capture happened, and `(captured_tick, captured)` breaks
    /// even a same-tick double capture.
    ///
    /// It is also the timestamp the records office (FVS-O-4) will want, so it earns its place twice.
    pub captured_tick: u64,
}

/// The reward. **The only path from containment to a specimen**, and a component hook rather than a
/// system so it cannot be forgotten, reordered, or run-condition'd away: it fires at command-apply time,
/// exactly once, for every `Contained` that is ever inserted.
///
/// Bevy 0.19 spelling note: hooks take `HookContext` (`entity`) plus a `DeferredWorld`; observers are
/// the `On<Add, C>` form. The hook is used here — not an observer — because this is an *invariant*, and
/// the backlog's engine baseline is explicit that hooks are the stable path for invariants while
/// observers are for softer fan-out (FX, telemetry).
/// Attaching the specimen to the Site happens **here, in the hook**, rather than in a system that sweeps
/// up unparented specimens afterwards. Two reasons, both load-bearing:
/// * A system would be a new `FixedUpdate` node, which permutes the schedule's linearisation and moves
///   the goldens for nothing. A hook adds no node at all.
/// * It preserves the property the module docs argue for — the hook is the *only* path from containment
///   to a specimen, so there is no second place where a specimen can come into existence unlinked.
///
/// The Site may legitimately not exist: bare-`App` unit tests never build one. That is not a fallback
/// path (the specimen is granted identically either way) — it is one optional *link*, and the
/// relationship's own hooks handle removal, so nothing has to clean up after a despawned Site.
fn grant_specimen(mut world: bevy::ecs::world::DeferredWorld, ctx: bevy::ecs::lifecycle::HookContext) {
    let tick = world.resource::<crate::session::RunClock>().ticks;
    let site = world.get_resource::<crate::site::SiteRoot>().map(|s| s.0);
    let specimen = Specimen { captured: ctx.entity, captured_tick: tick };
    // FVS-E-1: the posterior is created **with** the specimen, at maximum entropy. Attaching it here
    // rather than in a later "initialise research" pass means there is no window in which a banked
    // specimen exists without a record — and no second place that could create one differently.
    let posterior = crate::research::ResearchPosterior::unknown();
    match site {
        Some(site) => {
            world.commands().spawn((specimen, posterior, crate::site::HeldAt(site)));
        }
        None => {
            world.commands().spawn((specimen, posterior));
        }
    }
}

/// Run every in-progress containment against the live stigmergy field.
///
/// Basin-holding, not HP: each tick the anomaly's rule is evaluated at its own cell, satisfied ticks
/// accumulate, and a lapse either resets or banks per [`OnBreak`]. Completion inserts [`Contained`],
/// whose hook grants the specimen.
pub fn tick_containment(
    time: Res<Time>,
    stig: Res<crate::ai::field::Stig>,
    dungeon: Res<crate::dungeon::Dungeon>,
    mut commands: Commands,
    mut anomalies: Query<(Entity, &Transform, &mut Containment), Without<Contained>>,
) {
    let dt = time.delta_secs();
    // Order-independent by construction: each anomaly reads the shared field and writes only its own
    // component, so no canonical sort is needed (see the module docs).
    for (entity, transform, mut containment) in &mut anomalies {
        let pos = transform.translation;
        let satisfied = containment
            .rule
            .is_satisfied(|field| stig.sample(field, &dungeon, pos));
        if containment.advance(dt, satisfied) {
            commands.entity(entity).insert(Contained);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containment::rule::{FieldCondition, Sign};

    fn rule(hold: f32, on_break: OnBreak) -> ContainmentRule {
        ContainmentRule {
            requires: vec![FieldCondition { channel: 0, sign: Sign::AtLeast, threshold: 0.5 }],
            hold_secs: hold,
            break_on_fail: on_break,
        }
    }

    #[test]
    fn an_uncontained_anomaly_never_accumulates() {
        // No device applied ⇒ standing in a satisfying field does nothing. Capture is an *action*, not
        // an ambient consequence of the world being in a certain state.
        let mut c = Containment::new(rule(1.0, OnBreak::Reset));
        assert!(!c.advance(0.5, true));
        assert_eq!(c.held_secs(), 0.0);
        assert_eq!(c.phase(), Phase::Uncontained);
    }

    #[test]
    fn holding_the_basin_for_the_full_duration_captures() {
        let mut c = Containment::new(rule(1.0, OnBreak::Reset));
        c.begin();
        assert!(!c.advance(0.6, true), "not yet");
        assert_eq!(c.phase(), Phase::BeingContained);
        assert!(c.advance(0.6, true), "crossing hold_secs completes on that tick");
        assert_eq!(c.phase(), Phase::Contained);
    }

    #[test]
    fn completion_is_reported_exactly_once() {
        // The hook must fire once, so `advance` may only return true on the crossing tick — a second
        // true would insert `Contained` twice and grant two specimens for one capture.
        let mut c = Containment::new(rule(1.0, OnBreak::Reset));
        c.begin();
        assert!(c.advance(1.0, true));
        assert!(!c.advance(1.0, true), "a completed capture must not re-complete");
        assert!(!c.advance(1.0, false));
    }

    #[test]
    fn a_lapse_resets_or_banks_according_to_the_policy() {
        let mut reset = Containment::new(rule(1.0, OnBreak::Reset));
        reset.begin();
        reset.advance(0.6, true);
        reset.advance(0.1, false);
        assert_eq!(reset.held_secs(), 0.0, "Reset makes containment a SUSTAINED task");

        let mut keep = Containment::new(rule(1.0, OnBreak::Keep));
        keep.begin();
        keep.advance(0.6, true);
        keep.advance(0.1, false);
        assert!((keep.held_secs() - 0.6).abs() < 1e-6, "Keep makes it CUMULATIVE");
        assert!(keep.advance(0.4, true), "banked progress resumes where it left off");
    }

    #[test]
    fn a_capture_cannot_be_re_opened_and_progress_is_readable() {
        let mut c = Containment::new(rule(2.0, OnBreak::Reset));
        c.begin();
        c.advance(1.0, true);
        assert!((c.progress() - 0.5).abs() < 1e-6, "half-held reads as half progress");

        c.advance(1.0, true);
        assert_eq!(c.phase(), Phase::Contained);
        c.begin(); // a second device thrown at a captured anomaly
        assert_eq!(c.phase(), Phase::Contained, "a completed capture must not restart");
        assert!((c.progress() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cancelling_an_attempt_discards_progress_but_a_capture_survives() {
        let mut c = Containment::new(rule(2.0, OnBreak::Keep));
        c.begin();
        c.advance(1.0, true);
        c.cancel();
        assert_eq!(c.phase(), Phase::Uncontained);
        assert_eq!(c.held_secs(), 0.0, "cancelling is the attempt ending, not a mid-hold lapse");

        let mut done = Containment::new(rule(1.0, OnBreak::Reset));
        done.begin();
        done.advance(1.0, true);
        done.cancel();
        assert_eq!(done.phase(), Phase::Contained, "cancel must not un-capture");
    }
}
