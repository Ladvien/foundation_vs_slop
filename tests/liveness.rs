//! Stage 4 — liveness / soft-lock net (feature `test-harness`). A scripted agent drives the squad across
//! the generated dungeon and asserts the run stays healthy and actually covers ground. Physics-inclusive
//! (the real sim), so the oracles are LIVENESS + COVERAGE, never an exact hash (Lu et al. 2022 Go-Explore
//! reachability; the "unstable oracle" caveat). This is the crash/soft-lock net: if pathing dead-locks,
//! an actor NaNs, or the squad can't move, it trips here.
#![cfg(feature = "test-harness")]

use bevy::math::IVec2;
use foundation_vs_slop::sim_harness::{
    build_headless_app, floor_cells, issue_squad_order, liveness_violations, serial_guard,
    squad_centroid_cell, step, step_until_squad_blenders_ready, unit_cells, SimConfig,
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

/// The animation blender's net, driven by the real game rather than a synthetic `App`.
///
/// `src/anim` unit-tests the ease and the phase directly; what only a live run can check is that the
/// wiring actually happens — that figurines stream in, get their blend sets, and stay well-formed while
/// units accelerate, strafe, shoot and stop. The oracle is deliberately structural (finite, summing to
/// one, phase in range) rather than pixel-exact: this is the physics-inclusive sim, so exact values are
/// not reproducible (see the module header).
#[test]
fn every_wired_figurine_keeps_a_well_formed_pose_blend_through_a_live_run() {
    use bevy::prelude::IVec2;
    use foundation_vs_slop::anim::{blend, PoseBlender};

    let _serial = serial_guard();
    let cfg = SimConfig::default();
    let mut app = build_headless_app(&cfg);

    // Warm up, then haul the squad across the map so it accelerates, turns and stops — and let the
    // crab swarm reach it, so units aim and fire and the masked upper-body layer takes weight.
    step(&mut app, &cfg, 1);
    let floors = floor_cells(&mut app);
    let stride = (floors.len() / 4).max(1);
    let goals: Vec<IVec2> = floors.iter().step_by(stride).copied().collect();

    let mut blenders_seen = 0usize;
    let mut moving_seen = 0usize;
    for goal in goals {
        issue_squad_order(&mut app, goal);
        for _ in 0..6 {
            step(&mut app, &cfg, 10);
            let world = app.world_mut();
            let mut q = world.query::<&PoseBlender>();
            let mut here = 0usize;
            for b in q.iter(world) {
                here += 1;
                let phase = b.phase();
                assert!(
                    (0.0..1.0).contains(&phase),
                    "gait phase escaped [0,1): {phase} — a non-finite ground speed would do this"
                );
                let mut sum = 0.0f32;
                for slot in 0..b.len() {
                    let w = b.live_weight(slot);
                    assert!(w.is_finite() && (0.0..=1.0).contains(&w), "slot {slot} weight is {w}");
                    sum += w;
                }
                // Every driver hands over a partition of unity, and the ease preserves it, so a total
                // that drifts means a slot table and a driver have fallen out of step.
                assert!(
                    (sum - 1.0).abs() < 0.01,
                    "blend weights sum to {sum}, not 1 — driver and slot table disagree"
                );
                // A figurine that is actually locomoting must have weight on a gait clip, not be
                // standing in idle while its transform slides across the floor.
                if b.live_weight(blend::SLOT_IDLE) + b.live_weight(blend::SLOT_IDLE_ALERT) < 0.5 {
                    moving_seen += 1;
                }
            }
            blenders_seen = blenders_seen.max(here);
        }
    }

    // Guard against the assertions above passing vacuously: the five figurines and the crab swarm all
    // carry blenders, and at least some of them were seen mid-stride rather than parked in idle.
    assert!(
        blenders_seen >= 5,
        "expected at least the five squad figurines to be wired, saw {blenders_seen}"
    );
    assert!(moving_seen > 0, "no figurine ever left idle — the blend space is never being exercised");
}

/// The headline behaviour of the animation rework: **shooting no longer stops the legs.**
///
/// The state machine this replaced played the fire clip as a full-body override and skipped locomotion
/// entirely for its 1.167 s, so a unit shot mid-stride froze and then snapped. Now `aim`/`fire` are
/// masked out of the lower body and layered over the locomotion mixture, and because the action clips
/// are never pushed for lower-body bones the legs are driven by the locomotion mixture alone — at
/// whatever common factor it carries.
///
/// `src/squad.rs` unit-tests the weight construction; this drives the real engine. The unattended
/// scenario never brings the squad into contact (measured: no unit acquires an `AimTarget` in 3000
/// ticks), so a bare hostile is planted in front of each unit — `laser::fire_laser` needs only
/// `(Hostile, Transform)`, fog visibility and the front arc — and the squad engages for real.
/// **Firing layers OVER locomotion — it does not replace it** (FVS-N-9).
///
/// The property: a unit shooting while it moves must carry gait weight *and* upper-body action weight
/// at the same time. If the action layer ever replaced locomotion, the squad would slide into contact
/// with dead legs.
///
/// # The scenario used to be the unreliable part, and this is what fixed it
///
/// It was `#[ignore]`d for a day because it *marched the squad the length of the level and hoped* it ran
/// into something. Measured while diagnosing: 43 hostiles alive, 91 visible cells, and `aimed = 0/5`
/// over 1200 ticks — the squad simply never closed with anything on that seed. Three decoy placements
/// were tried and all read zero, so the decoys were never the variable either.
///
/// The fix is the one FVS-N-9 prescribed: **drive the squad THROUGH a known hostile cluster** rather
/// than across the map hoping. The target is the densest hostile neighbourhood, and the goal is the
/// farthest reachable floor cell *within* that neighbourhood — so the path routes through the crabs and
/// the squad is still locomoting when it is shot at. Ordering them *to* the cluster is not enough: they
/// arrive, stop, and fire from a standstill, which can never satisfy a layering assertion.
///
/// # Still `#[ignore]`d, and the reason is now much narrower
///
/// That scenario work moved `max_action` from **0.000 over 1200 ticks** (the squad never fired at all)
/// to **0.169**, reproducibly. So contact happens now. What does not happen is the assertion's `> 0.5`.
///
/// `squad.rs` sets `alpha = ACTION_ALPHA (0.9)` whenever a unit is `firing || aiming`, and weights ease
/// to target in ~3·`FADE_TAU` ≈ 0.24 s — so a unit that *held* an aim would sit at 0.9, comfortably over
/// the bar. Reaching only ~19% of `ACTION_ALPHA` therefore says the **aim state is flickering** rather
/// than the blend being wrong: units acquire a target, lose it, and the ease never converges.
///
/// That is a target-acquisition question (`fire_laser`'s front-arc gate and `AimTarget`'s lifetime),
/// not an animation one, and it is a different investigation from the one this test is for. Left
/// `#[ignore]`d rather than relaxed to a threshold the design happens to produce — a test tuned until it
/// passes stops being evidence.
///
/// Two other things kept from the diagnosis, because each was its own false failure:
/// * **Wait for the figurines** before measuring, or the loop finds no `PoseBlender`, `max_action`
///   stays 0.0, and the test fails claiming "the squad did not engage" — a statement about GLB load
///   timing dressed as one about gameplay (TESTING.md invariant 9).
/// * **Wait for the engagement, then measure**, rather than measuring for a fixed window. Conflating
///   "did a firefight start in N ticks" with "does the blend layer" is what made a combat-pacing
///   accident report itself as an animation regression.
#[test]
#[ignore = "scenario now engages, but the aim state flickers — see FVS-N-9 for the narrowed diagnosis"]
fn a_unit_shooting_on_the_move_keeps_its_legs_running() {
    use bevy::prelude::{Transform, Vec3, With};
    use foundation_vs_slop::anim::{blend, PoseBlender};
    use foundation_vs_slop::crab::Crab;
    use foundation_vs_slop::dungeon::Dungeon;

    let _serial = serial_guard();
    let cfg = SimConfig::default();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 1);
    assert!(
        step_until_squad_blenders_ready(&mut app, &cfg, 600).is_some(),
        "the squad figurines never streamed in and wired to pose blenders"
    );

    // The densest live-hostile NEIGHBOURHOOD, and it must be a real cluster: a lone wanderer can die or
    // drift away before the squad arrives, which is how "drive at the enemy" quietly degenerates back
    // into "march and hope".
    //
    // A neighbourhood rather than a single cell, because measured at spawn the crabs sit roughly one per
    // cell — the first version of this asked for 3 in one cell and correctly refused to run. `CLUSTER_R`
    // is a couple of tiles, which is "the squad will be shot at on arrival" rather than a precise claim.
    const CLUSTER_R: i32 = 3;
    const MIN_CLUSTER: usize = 3;
    let target: IVec2 = {
        let world = app.world_mut();
        let positions: Vec<Vec3> = {
            let mut q = world.query_filtered::<&Transform, With<Crab>>();
            q.iter(world).map(|t| t.translation).collect()
        };
        let dungeon = world.resource::<Dungeon>();
        // A `BTreeMap` so the walk order is CELL order rather than hash order — the same discipline
        // every canonical pick in this repo uses, and it matters here because the chosen goal steers
        // the whole rest of the test.
        let mut counts: std::collections::BTreeMap<(i32, i32), usize> = Default::default();
        for p in &positions {
            let c = dungeon.world_to_cell(*p);
            if dungeon.is_floor(c) {
                *counts.entry((c.x, c.y)).or_default() += 1;
            }
        }
        let best = counts
            .keys()
            .map(|&(x, y)| {
                let near: usize = counts
                    .iter()
                    .filter(|((ox, oy), _)| (ox - x).abs() <= CLUSTER_R && (oy - y).abs() <= CLUSTER_R)
                    .map(|(_, n)| *n)
                    .sum();
                (near, -x, -y)
            })
            .max();
        let Some((n, nx, ny)) = best else {
            panic!("no live hostile stands on a floor cell — the scenario cannot produce an engagement");
        };
        assert!(
            n >= MIN_CLUSTER,
            "densest hostile neighbourhood holds only {n} (< {MIN_CLUSTER}) within {CLUSTER_R} tiles — \
             too thin to rely on for contact"
        );
        IVec2::new(-nx, -ny)
    };

    // **Order the squad THROUGH the cluster, not to it** — and this is the whole trick.
    //
    // Ordering them *at* the cluster makes them arrive and stop, so they fire from a standstill: gait
    // weight goes to zero and `best_together` (the min of gait and action) can never rise, no matter how
    // hard they are shooting. Measured on the first attempt at this: `max_action` climbed 0.000 -> 0.169,
    // i.e. contact was finally happening, and the layering assertion still could not be satisfied. The
    // test needs them *locomoting through* contact, which is also the situation the property is about.
    //
    // So: continue past the cluster along the squad -> cluster direction, and take the farthest
    // reachable floor cell. Candidates are tried nearest-last in a fixed order, so the choice is
    // reproducible.
    let goal = {
        const NEAR_CLUSTER: i32 = 8;
        let centre = squad_centroid_cell(&mut app);
        let floors = floor_cells(&mut app);
        // Every floor cell in the cluster's neighbourhood, farthest-from-the-squad first. Walking to one
        // of those routes the squad THROUGH the crabs and keeps it moving until it is past them.
        //
        // A straight-line extrapolation past the cluster was tried first and is wrong: it walks into
        // walls (measured — it found nothing reachable beyond `IVec2(45, 16)`). Reachability has to come
        // from the actual floor set, not from arithmetic on coordinates.
        let mut candidates: Vec<(i32, IVec2)> = floors
            .iter()
            .filter(|c| {
                (c.x - target.x).abs() <= NEAR_CLUSTER && (c.y - target.y).abs() <= NEAR_CLUSTER
            })
            .map(|&c| ((c - centre).length_squared(), c))
            .collect();
        // SORT-OK: `(−distance², cell)` is total — the cell breaks any distance tie, and a floor set is
        // an authored/derived list rather than an ECS query, so this cannot launder archetype order.
        candidates.sort_unstable_by_key(|(d, c)| (-*d, c.x, c.y));
        let chosen = candidates.iter().find(|(_, c)| issue_squad_order(&mut app, *c)).map(|(_, c)| *c);
        // Falling back to the cluster cell itself would silently reinstate the stand-and-shoot scenario
        // this whole block exists to avoid, so it is a loud failure instead.
        chosen.unwrap_or_else(|| {
            panic!(
                "no reachable floor cell within {NEAR_CLUSTER} tiles of the hostile cluster at \
                 {target:?} — the squad would have to stop ON the enemy, which cannot exercise firing \
                 WHILE moving"
            )
        })
    };
    let _ = goal;

    const GAIT: [usize; 6] = [
        blend::SLOT_WALK,
        blend::SLOT_RUN,
        blend::SLOT_WALK_BACK,
        blend::SLOT_RUN_BACK,
        blend::SLOT_STRAFE_L,
        blend::SLOT_STRAFE_R,
    ];
    // Bounded wait for the engagement, measuring as we go, and stopping the moment the property is
    // observed — so a healthy run is fast and a timeout is an honest, separate failure.
    let mut best_together = 0.0f32;
    let mut max_action = 0.0f32;
    for _ in 0..1200 {
        step(&mut app, &cfg, 1);
        let world = app.world_mut();
        let mut q = world.query::<&PoseBlender>();
        for b in q.iter(world).filter(|b| b.len() == blend::LOCO_SLOTS + 2) {
            let gait: f32 = GAIT.iter().map(|&i| b.live_weight(i)).sum();
            let action = b.live_weight(blend::LOCO_SLOTS) + b.live_weight(blend::LOCO_SLOTS + 1);
            max_action = max_action.max(action);
            best_together = best_together.max(gait.min(action));
        }
        if best_together > 0.05 {
            break;
        }
    }

    assert!(
        max_action > 0.5,
        "the upper-body layer never armed (max {max_action:.3}) — the squad never engaged even when \
         driven straight at a {MIN_CLUSTER}+ hostile cluster, so this proved nothing about layering. \
         That is a SCENARIO failure, not an animation one: check `fire_laser`'s target acquisition \
         before suspecting the blend."
    );
    assert!(
        best_together > 0.05,
        "the action layer and the gait never carried weight at the same time (best {best_together:.3}) \
         — firing is overriding locomotion again instead of layering over it"
    );
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
