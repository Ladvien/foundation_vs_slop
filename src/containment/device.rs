//! **Archetype 1 — the single-target capture device** (FVS-B-5), and the device↔anomaly link (FVS-D-3).
//!
//! The Portable Spatial Containment Device (equipment doc §10 Armaments) is thrown at *one body*. It is
//! the first of three genuinely distinct archetypes and must not be conflated with them: single-target
//! **captures a body**, area-denial **bounds a region** (B-6), source-elimination **caps a structure**
//! (B-7, which yields no specimen at all).
//!
//! # The device names its target; it does not search for one
//!
//! [`ContainmentDevice`] carries the `Entity` it was thrown at, chosen when the player threw it. The
//! alternative — landing the device and having it grab the nearest eligible anomaly — would be a *pick
//! from a query*, which in this codebase means a mandatory total sort and a stable per-anomaly key that
//! does not exist yet (`tests/determinism_lint.rs`, and the `util::nearest_planar_keyed` docs on why a
//! raw `Entity` id is not a valid key). Naming the target removes the pick entirely: there is nothing to
//! order, so nothing to get wrong. It is also the better mechanic — the player chooses *which* anomaly
//! to spend a device on, rather than the device choosing for them.
//!
//! A throw can still **miss**: the target may have died, wandered outside [`ContainmentDevice::reach`],
//! or already be captured. All three spend the device and do nothing, which is one path — there is no
//! "fall back to another target" branch.

use bevy::prelude::*;

use super::state::{Containment, Phase};

/// A deployed capture device, mid-flight or landed.
///
/// Spawned by the throw (equipment/`placement` will own that; for now anything may spawn one), consumed
/// by [`deploy_devices`] on the tick it resolves.
#[derive(Component, Debug, Clone, Copy)]
pub struct ContainmentDevice {
    /// The anomaly this device was thrown at.
    pub target: Entity,
    /// How close the device must land to its target for the throw to connect, in metres.
    pub reach: f32,
}

/// **The device→anomaly half of the link** (FVS-D-3): this device is holding that anomaly.
///
/// A Bevy relationship rather than a bare `Entity` field, for the reason relationships exist: when the
/// anomaly despawns, Bevy's own hooks clear the link, so a device can never point at a dead target. That
/// is the "breaking containment clears the link" half of D-3's acceptance, with no bookkeeping system.
#[derive(Component)]
#[relationship(relationship_target = HeldBy)]
pub struct Holding(pub Entity);

/// **The anomaly→device half of the link**: the devices currently holding this anomaly.
///
/// Like every `RelationshipTarget`, this component is **removed** when the last device releases — read
/// it as `Option<&HeldBy>`, never a bare `Query<&HeldBy>` (the same gotcha `squad::SquadRoster`
/// documents and `tests/squad.rs` pins).
#[derive(Component)]
#[relationship_target(relationship = Holding)]
pub struct HeldBy(Vec<Entity>);

impl HeldBy {
    /// How many devices are holding this anomaly.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no device is holding it (only observable transiently — the component is removed when it
    /// would become empty).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The holding devices, in attach order.
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }
}

/// Resolve every deployed device against its named target.
///
/// A connecting throw begins containment and links the two; a miss spends the device silently. Either
/// way the [`ContainmentDevice`] is consumed this tick, so a device resolves exactly once.
///
/// **Determinism:** each device reads and writes only its own target, and there is no pick, no shared
/// counter and no budget — so iteration order cannot change the outcome and no canonical sort is needed.
/// Two devices thrown at the *same* anomaly are also safe: `Containment::begin` is idempotent, and the
/// second simply joins [`HeldBy`].
pub fn deploy_devices(
    mut commands: Commands,
    devices: Query<(Entity, &ContainmentDevice, &Transform)>,
    mut targets: Query<(&mut Containment, &Transform)>,
) {
    for (device_entity, device, device_tf) in &devices {
        // The device is spent whether or not it connects — one path, no retry.
        commands.entity(device_entity).remove::<ContainmentDevice>();

        let Ok((mut containment, target_tf)) = targets.get_mut(device.target) else {
            continue; // target died or was never containable
        };
        if containment.phase() != Phase::Uncontained {
            continue; // already being contained, or already captured
        }
        if device_tf.translation.distance(target_tf.translation) > device.reach {
            continue; // missed
        }

        containment.begin();
        commands.entity(device_entity).insert(Holding(device.target));
    }
}

/// Release the link when containment is no longer in progress.
///
/// A device holds an anomaly only while the capture is live. Once the anomaly is `Contained` (the
/// capture succeeded) or back to `Uncontained` (the attempt was cancelled), the device has nothing left
/// to hold, so the relationship is dropped — the other half of D-3's "breaking containment clears the
/// link". The anomaly *despawning* is already handled for free by the relationship's own hooks.
pub fn release_finished_devices(
    mut commands: Commands,
    devices: Query<(Entity, &Holding)>,
    targets: Query<&Containment>,
) {
    for (device_entity, holding) in &devices {
        let still_holding = targets
            .get(holding.0)
            .is_ok_and(|c| c.phase() == Phase::BeingContained);
        if !still_holding {
            commands.entity(device_entity).remove::<Holding>();
        }
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
            break_on_fail: OnBreak::Reset,
        }
    }

    /// A bare `App` with just the two device systems — no sim, no assets. The systems are pure ECS, so
    /// the whole archetype is testable without the harness.
    fn app() -> App {
        let mut app = App::new();
        app.add_systems(Update, (deploy_devices, release_finished_devices).chain());
        app
    }

    fn spawn_target(app: &mut App, at: Vec3) -> Entity {
        app.world_mut()
            .spawn((Containment::new(rule()), Transform::from_translation(at)))
            .id()
    }

    fn throw(app: &mut App, target: Entity, from: Vec3, reach: f32) -> Entity {
        app.world_mut()
            .spawn((
                ContainmentDevice { target, reach },
                Transform::from_translation(from),
            ))
            .id()
    }

    #[test]
    fn a_connecting_throw_begins_containment_and_links_both_ways() {
        let mut app = app();
        let target = spawn_target(&mut app, Vec3::ZERO);
        let device = throw(&mut app, target, Vec3::new(0.5, 0.0, 0.0), 2.0);
        app.update();

        assert_eq!(
            app.world().get::<Containment>(target).expect("containment").phase(),
            Phase::BeingContained,
            "a connecting throw must begin the capture"
        );
        // D-3: the device knows its target...
        assert_eq!(app.world().get::<Holding>(device).expect("holding").0, target);
        // ...and the anomaly knows its device.
        let held = app.world().get::<HeldBy>(target).expect("the anomaly is held");
        assert_eq!(held.len(), 1);
        assert_eq!(held.iter().next(), Some(device));
        // The device is spent — it cannot resolve twice.
        assert!(app.world().get::<ContainmentDevice>(device).is_none());
    }

    #[test]
    fn a_throw_that_lands_short_does_nothing_and_is_still_spent() {
        let mut app = app();
        let target = spawn_target(&mut app, Vec3::ZERO);
        let device = throw(&mut app, target, Vec3::new(10.0, 0.0, 0.0), 2.0);
        app.update();

        assert_eq!(
            app.world().get::<Containment>(target).expect("containment").phase(),
            Phase::Uncontained,
            "a miss must not begin containment"
        );
        assert!(app.world().get::<Holding>(device).is_none(), "a miss links nothing");
        assert!(
            app.world().get::<ContainmentDevice>(device).is_none(),
            "a spent device must not retry next tick"
        );
    }

    #[test]
    fn a_throw_at_a_dead_or_already_captured_target_does_nothing() {
        let mut app = app();

        // Dead target.
        let ghost = spawn_target(&mut app, Vec3::ZERO);
        app.world_mut().entity_mut(ghost).despawn();
        let d1 = throw(&mut app, ghost, Vec3::ZERO, 2.0);
        app.update();
        assert!(app.world().get::<Holding>(d1).is_none(), "a throw at a dead target links nothing");

        // Already captured target: `begin` must not re-open it, and the device must not attach.
        let done = spawn_target(&mut app, Vec3::ZERO);
        {
            let mut c = app.world_mut().get_mut::<Containment>(done).expect("containment");
            c.begin();
            // Drive it to completion through the public API.
            while c.phase() == Phase::BeingContained {
                c.advance_for_test(1.0, true);
            }
        }
        let d2 = throw(&mut app, done, Vec3::ZERO, 2.0);
        app.update();
        assert!(app.world().get::<Holding>(d2).is_none(), "a captured anomaly takes no new device");
    }

    #[test]
    fn two_devices_can_hold_one_anomaly_and_the_capture_is_not_restarted() {
        let mut app = app();
        let target = spawn_target(&mut app, Vec3::ZERO);
        let a = throw(&mut app, target, Vec3::ZERO, 2.0);
        app.update();
        let b = throw(&mut app, target, Vec3::ZERO, 2.0);
        app.update();

        // The second throw finds the target already `BeingContained`, so it does not attach — the
        // capture in progress is not disturbed and no second link is created.
        assert!(app.world().get::<Holding>(a).is_some());
        assert!(app.world().get::<Holding>(b).is_none());
        assert_eq!(app.world().get::<HeldBy>(target).expect("held").len(), 1);
    }

    #[test]
    fn the_link_clears_when_the_capture_finishes_or_the_target_dies() {
        // Finished capture → released.
        let mut app = app();
        let target = spawn_target(&mut app, Vec3::ZERO);
        let device = throw(&mut app, target, Vec3::ZERO, 2.0);
        app.update();
        assert!(app.world().get::<Holding>(device).is_some());

        {
            let mut c = app.world_mut().get_mut::<Containment>(target).expect("containment");
            while c.phase() == Phase::BeingContained {
                c.advance_for_test(1.0, true);
            }
        }
        app.update();
        assert!(
            app.world().get::<Holding>(device).is_none(),
            "a completed capture releases its device"
        );

        // Dead target → released by Bevy's own relationship hooks, with no system of ours involved.
        let target2 = spawn_target(&mut app, Vec3::ZERO);
        let device2 = throw(&mut app, target2, Vec3::ZERO, 2.0);
        app.update();
        assert!(app.world().get::<Holding>(device2).is_some());
        app.world_mut().entity_mut(target2).despawn();
        assert!(
            app.world().get::<Holding>(device2).is_none(),
            "the relationship's own hooks must clear a link to a despawned anomaly"
        );
    }
}
