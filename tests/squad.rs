//! Squad composition (feature `test-harness`): the squad↔member relationship and its despawn hygiene.
//!
//! The point of modelling the roster as a Bevy **relationship** rather than a hand-kept `Vec<Entity>` is
//! that the removal side is not ours to forget: the relationship's own hooks drop a despawned member
//! from `SquadRoster`, so there is never a stale `Entity` to dereference after a death. This test pins
//! both directions — the roster is populated at spawn, and it empties itself on a wipe with no
//! bookkeeping system in between.
#![cfg(feature = "test-harness")]

use bevy::prelude::{Entity, With};
use foundation_vs_slop::sim_harness::{build_headless_app, kill_squad, serial_guard, step, SimConfig};
use foundation_vs_slop::squad::{MemberOf, Squad, SquadRoster, Unit};

#[test]
fn the_roster_enumerates_its_members_and_empties_itself_on_a_wipe() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 5);

    // Exactly one roster node, and it knows all five operatives.
    let squad: Entity = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<Entity, With<Squad>>();
        let found: Vec<Entity> = q.iter(world).collect();
        assert_eq!(found.len(), 1, "there must be exactly one Squad roster node");
        found[0]
    };
    {
        let roster = app.world().get::<SquadRoster>(squad).expect("the squad owns a roster");
        assert_eq!(roster.len(), 5, "the shipped squad is five operatives");
    }

    // ...and every operative names it back. Both halves matter: the relationship is only useful if the
    // source side is on *every* unit (a unit without `MemberOf` would also sit in a different archetype,
    // which is what the module docs forbid).
    {
        let world = app.world_mut();
        let mut q = world.query_filtered::<&MemberOf, With<Unit>>();
        let parents: Vec<Entity> = q.iter(world).map(|m| m.0).collect();
        assert_eq!(parents.len(), 5, "every unit carries MemberOf");
        assert!(parents.iter().all(|&e| e == squad), "every unit points at the one squad");
    }

    // Wipe. Two ticks: one for `despawn_dead_units` to see zero health, one for its commands to flush.
    kill_squad(&mut app);
    step(&mut app, &cfg, 2);

    // The roster NODE survives — only its members are gone.
    {
        let world = app.world_mut();
        let mut q = world.query_filtered::<Entity, With<Squad>>();
        assert_eq!(q.iter(world).count(), 1, "the squad entity itself outlives its members");
    }

    // ...and Bevy expresses "empty" by **removing** the `RelationshipTarget` component, not by leaving
    // an empty one behind. Pinned deliberately: it is the gotcha every future consumer of this roster
    // will hit (P2's role differentiation reads it), so anything that queries `SquadRoster` must tolerate
    // its absence — `Option<&SquadRoster>`, never a bare `Query<&SquadRoster>` that silently matches
    // nothing on a wiped squad. If a Bevy upgrade changes this to "present but empty", this test says so.
    assert!(
        app.world().get::<SquadRoster>(squad).is_none(),
        "a despawned member must leave the roster on its own, and an emptied roster component is removed"
    );
}
