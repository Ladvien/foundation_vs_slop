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

/// The regression net for "the squad flees from its own gunfire".
///
/// Unit tests already assert that no faction fears a channel it emits. This drives the whole real
/// pipeline instead — deposit → drain → evaporate → `update_drives` → FEAR — and checks the outcome on
/// live `Unit` entities, so a future rewiring of `laser.rs`, the deposit sets, or the drive registry
/// cannot quietly restore the coupling.
#[test]
fn a_units_fear_ignores_gunfire_and_answers_to_creatures() {
    use bevy::prelude::{App, Transform, With};
    use foundation_vs_slop::ai::drives::{DriveId, Drives};
    use foundation_vs_slop::ai::field::{Deposit, FieldId, StigDeposits};
    use foundation_vs_slop::squad::Unit;

    let _serial = serial_guard();
    // Physics-off core: we want the drive pipeline, not the Avian solver.
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1);

    /// Flood one channel at every unit's own position for a while, then report the max FEAR reached.
    fn flood(app: &mut App, cfg: &SimConfig, field: FieldId, amount: f32) -> f32 {
        for _ in 0..90 {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&Transform, With<Unit>>();
            let spots: Vec<_> = q.iter(world).map(|t| t.translation).collect();
            let mut deposits = world.resource_mut::<StigDeposits>();
            for pos in spots {
                deposits.0.push(Deposit { pos, field, amount });
            }
            step(app, cfg, 1);
        }
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Drives, With<Unit>>();
        q.iter(world).map(|d| d.get(DriveId::FEAR)).fold(0.0f32, f32::max)
    }

    // A unit standing in its own muzzle flash. `fire_laser` deposits THREAT_GUN at the SHOOTER's own
    // position ~6.7x/second, so this is a faithful — in fact gentler — model of sustained fire. Before the
    // channels were split by emitter, FEAR here saturated to ~1.0 within a second and `Flee` (the top rank
    // for every role) preempted Overwatch, Ward, TendWounded and the rest, forever.
    //
    // A tolerance rather than an exact zero: the swarm exists in this world too, so a crab that wanders
    // into range during the 1.5 s flood is a *legitimate* fear source. What must not happen is the squad's
    // own muzzle driving FEAR anywhere near `Flee`'s ~0.28 onset.
    let fear_from_own_gunfire = flood(&mut app, &cfg, FieldId::THREAT_GUN, 0.6);
    assert!(
        fear_from_own_gunfire < 0.05,
        "units are afraid of their own gunfire again (FEAR {fear_from_own_gunfire})",
    );

    // The same flood on a channel a *crab* emits must frighten them past the Flee onset, or the fix has
    // simply deafened the squad to danger rather than pointing it at the right source.
    let fear_from_crabs = flood(&mut app, &cfg, FieldId::THREAT_CRAB, 0.6);
    assert!(
        fear_from_crabs > 0.28,
        "units no longer fear crabs enough to break (FEAR {fear_from_crabs}) — the squad is deaf to \
         danger, not merely brave",
    );
}

#[test]
fn almond_water_seeps_and_pools_on_the_floor() {
    // End-to-end proof that the Almond Water field actually accumulates in the real harness: bake the seep
    // sources (Startup), then run the accumulate/evaporate/diffuse tick for ~10 s and assert the field has
    // pooled somewhere (`peak > 0`). Covers the bake + tick path the GPU-free unit tests can only exercise
    // in isolation. Deterministic core (physics off); the field is CPU state, harness-visible.
    use foundation_vs_slop::almond_water::AlmondWater;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 600);
    let peak = app.world().resource::<AlmondWater>().peak();
    assert!(peak > 0.0, "Almond Water never pooled — the seeps aren't accumulating (peak={peak})");
}

#[test]
fn almond_pools_stay_small_and_isolated() {
    // The sparse-spring seep model must produce discrete puddles, not one continuous sheet: warm the field
    // to near steady state, then assert every connected pool (cells above `min_visible_level`) is at most
    // `POOL_TILE_CAP` tiles. Guards `bake_almond_sources`'s spring spacing + the diffuse/evaporate balance
    // against a regression back to the whole-floor blanket that defeated fog of war. Deterministic core
    // (physics off); the field is CPU state, harness-visible.
    use foundation_vs_slop::almond_water::AlmondWater;
    use foundation_vs_slop::config::GameConfig;

    const POOL_TILE_CAP: usize = 10;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 3000); // ~50 s: several evaporation time-constants, so pools are near steady state

    let threshold = app.world().resource::<GameConfig>().almond_water.min_visible_level;
    let peak = app.world().resource::<AlmondWater>().peak();
    let sizes = app.world().resource::<AlmondWater>().pool_sizes(threshold);
    let largest = sizes.first().copied().unwrap_or(0);
    println!(
        "almond pools: {} pools, {} wet tiles, peak {:.1}, thresh {:.1}, largest {:?}",
        sizes.len(),
        sizes.iter().sum::<usize>(),
        peak,
        threshold,
        &sizes[..sizes.len().min(12)]
    );
    assert!(!sizes.is_empty(), "sparse springs must still pool somewhere");
    assert!(
        largest <= POOL_TILE_CAP,
        "an almond pool grew to {largest} tiles (> {POOL_TILE_CAP}) — pools are merging into a sheet"
    );
}

#[test]
fn almond_water_heals_a_wounded_biological() {
    // The heal direction, isolated from combat noise: flood the field, wound every biological to half
    // health, run ONE tick, and assert at least one recovered. Only a handful of biologicals are in melee on
    // any given tick, so with the whole floor flooded the vast majority heal — `>= 1` is bulletproof against
    // the few that take contact damage this tick. Together with the `drink` unit test (exact drain) this
    // pins the write→heal coupling end-to-end.
    use bevy::prelude::With;
    use foundation_vs_slop::almond_water::AlmondWater;
    use foundation_vs_slop::config::GameConfig;
    use foundation_vs_slop::health::{Biological, Health};

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 120); // let spawns settle

    // Wound every biological to exactly half its max.
    let mut wounded = 0usize;
    {
        let world = app.world_mut();
        let mut q = world.query_filtered::<&mut Health, With<Biological>>();
        for mut h in q.iter_mut(world) {
            h.current = h.max * 0.5;
            wounded += 1;
        }
    }
    assert!(wounded > 0, "the sim must have biologicals (units + crabs) to heal");

    // Put water under all of them, then run a single heal tick.
    let capacity = app.world().resource::<GameConfig>().almond_water.capacity;
    app.world_mut().resource_mut::<AlmondWater>().test_flood(capacity);
    step(&mut app, &cfg, 1);

    // At least one biological standing in water recovered above the half-health mark.
    let healed = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Health, With<Biological>>();
        q.iter(world).filter(|h| h.current > h.max * 0.5 + 1.0e-4).count()
    };
    assert!(healed > 0, "no wounded biological healed while flooded with Almond Water");
}

#[test]
fn almond_water_poisons_when_the_pool_reads_as_cyanide() {
    // The inversion: a pool the population reads as CYANIDE (belief 0) damages a biological standing in it,
    // even at full health. Flood the field, force every cell's belief to 0 (poison), set every biological to
    // full HP, run ONE tick, and assert at least one lost HP. The signed twin of the heal test.
    use bevy::prelude::With;
    use foundation_vs_slop::almond_water::AlmondWater;
    use foundation_vs_slop::config::GameConfig;
    use foundation_vs_slop::health::{Biological, Health};

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 120); // let spawns settle

    // Top every biological to full so any drop is unambiguously the poison, not prior combat.
    let mut count = 0usize;
    {
        let world = app.world_mut();
        let mut q = world.query_filtered::<&mut Health, With<Biological>>();
        for mut h in q.iter_mut(world) {
            h.current = h.max;
            count += 1;
        }
    }
    assert!(count > 0, "the sim must have biologicals to poison");

    // Flood the floor and make every pool read as cyanide, then run a single effect tick.
    let capacity = app.world().resource::<GameConfig>().almond_water.capacity;
    {
        let mut field = app.world_mut().resource_mut::<AlmondWater>();
        field.test_flood(capacity);
        field.test_set_belief(0.0); // pure cyanide reading everywhere
    }
    step(&mut app, &cfg, 1);

    let poisoned = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Health, With<Biological>>();
        q.iter(world).filter(|h| h.current < h.max - 1.0e-4).count()
    };
    assert!(poisoned > 0, "no biological was poisoned while standing in a cyanide-belief pool");
}
