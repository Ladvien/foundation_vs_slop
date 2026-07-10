//! Stage 4 — liveness / soft-lock net (feature `test-harness`). A scripted agent drives the squad across
//! the generated dungeon and asserts the run stays healthy and actually covers ground. Physics-inclusive
//! (the real sim), so the oracles are LIVENESS + COVERAGE, never an exact hash (Lu et al. 2022 Go-Explore
//! reachability; the "unstable oracle" caveat). This is the crash/soft-lock net: if pathing dead-locks,
//! an actor NaNs, or the squad can't move, it trips here.
#![cfg(feature = "test-harness")]

use bevy::math::IVec2;
use foundation_vs_slop::sim_harness::{
    build_headless_app, floor_cells, issue_squad_order, liveness_violations, serial_guard, step, unit_cells,
    SimConfig,
};
use std::collections::HashSet;

#[test]
fn scripted_squad_stays_live_and_covers_ground() {
    let _serial = serial_guard();
    let cfg = SimConfig::default(); // full physics sim
    let mut app = build_headless_app(&cfg);

    // Warm up one tick so the dungeon + squad exist, then gather the reachable floor and pick a spread of
    // goals across it (deterministic: every Nth floor cell — no RNG).
    step(&mut app, &cfg, 1);
    let floors = floor_cells(&mut app);
    assert!(floors.len() > 50, "dungeon should have plenty of floor, got {}", floors.len());
    let stride = (floors.len() / 8).max(1);
    let goals: Vec<IVec2> = floors.iter().step_by(stride).copied().collect();

    // Drive the squad from goal to goal, accumulating the set of cells any unit occupied, and assert
    // liveness at every checkpoint. Coverage of distinct visited cells proves the squad actually moves
    // (no soft-lock) and the flow-field nav reaches spread-out targets.
    let mut visited: HashSet<(i32, i32)> = HashSet::new();
    let mut any_order_taken = false;
    for goal in goals {
        any_order_taken |= issue_squad_order(&mut app, goal);
        for _ in 0..4 {
            step(&mut app, &cfg, 20); // ~1/3 s per sub-step, 4 sub-steps per goal
            for c in unit_cells(&mut app) {
                visited.insert((c.x, c.y));
            }
            let v = liveness_violations(&mut app);
            assert!(v.is_empty(), "liveness violated en route to {goal:?}: {v:?}");
        }
    }

    assert!(any_order_taken, "at least one goal must have been reachable / ordered");
    // The squad (5 units) hauled across ~8 spread goals should touch a healthy number of distinct cells.
    assert!(
        visited.len() >= 15,
        "squad barely moved — only {} distinct cells visited (soft-lock?)",
        visited.len()
    );
}

#[test]
fn squad_survives_a_long_unattended_run() {
    // No orders at all: the swarm hunts the idle squad for ~10 s. The net catches a crash / NaN / runaway
    // reproduction / total wipe over a long horizon.
    let _serial = serial_guard();
    let cfg = SimConfig::default();
    let mut app = build_headless_app(&cfg);
    for checkpoint in 1..=20 {
        step(&mut app, &cfg, 30);
        let v = liveness_violations(&mut app);
        assert!(v.is_empty(), "liveness violated at tick {}: {v:?}", checkpoint * 30);
    }
}

#[test]
fn every_drives_carrier_has_a_faction_throughout_a_live_run() {
    // `update_drives` picks an agent's fear sources by `Faction`. `ai::faction::validate_factions` covers
    // the Startup population, but crabs are also bred at runtime (`nest::nest_reproduce`) — an untagged
    // agent there would simply never feel fear, an invisible-in-play bug rather than a crash. Both crab
    // paths funnel through `crab::spawn_crab_on_patch`, so the tag is structural; this asserts it stays so
    // over a long unattended run, while the swarm hunts and breeds.
    use bevy::prelude::{Entity, With, Without};
    use foundation_vs_slop::ai::drives::Drives;
    use foundation_vs_slop::ai::faction::Faction;

    let _serial = serial_guard();
    let cfg = SimConfig::default();
    let mut app = build_headless_app(&cfg);

    let mut agents_seen = 0usize;
    for checkpoint in 1..=20 {
        step(&mut app, &cfg, 30);
        let world = app.world_mut();
        let mut untagged = world.query_filtered::<Entity, (With<Drives>, Without<Faction>)>();
        let missing: Vec<Entity> = untagged.iter(world).collect();
        assert!(
            missing.is_empty(),
            "at tick {}: {} agent(s) carry Drives without a Faction (first {:?}) — they would never feel fear",
            checkpoint * 30,
            missing.len(),
            missing.first(),
        );
        let mut tagged = world.query_filtered::<Entity, (With<Drives>, With<Faction>)>();
        agents_seen = agents_seen.max(tagged.iter(world).count());
    }
    // Guard against the assertion above passing vacuously on an empty world.
    assert!(agents_seen > 5, "expected the squad plus a swarm to exist, saw {agents_seen} agents");
}
