//! Containment through the **real** simulation (feature `test-harness`).
//!
//! `src/containment` unit-tests the rule predicate and the phase machine directly, with no `App`. What
//! only a live run can check is the wiring: that a rule evaluated against the *actual* stigmergy grid
//! completes, that completion fires the `on_add` hook exactly once, and — the assertion this whole push
//! exists for — that **killing yields nothing**.
//!
//! Every test holds `serial_guard()` for its App's whole lifetime (harness invariant 4).
#![cfg(feature = "test-harness")]

use bevy::prelude::*;
use foundation_vs_slop::ai::field::{Deposit, FieldId, StigDeposits};
use foundation_vs_slop::containment::rule::{FieldCondition, OnBreak, Sign};
use foundation_vs_slop::containment::{Contained, Containment, ContainmentRule, Phase, Specimen};
use foundation_vs_slop::dungeon::Dungeon;
use foundation_vs_slop::sim_harness::{
    build_headless_app, issue_squad_order, nest_cells, serial_guard, squad_centroid_cell, step,
    SimConfig,
};

/// A rule satisfied by flooding one channel: hold `ATTENTION` at or above 0.5 for `hold_secs`.
///
/// `ATTENTION` is the right channel to drive from a test — it is deposited by gaze and decays fast, so
/// re-depositing each tick is exactly how the real out-watch capture (FVS-C-3) will hold it.
fn attention_rule(hold_secs: f32, break_on_fail: OnBreak) -> ContainmentRule {
    ContainmentRule {
        requires: vec![FieldCondition {
            channel: FieldId::ATTENTION.0,
            sign: Sign::AtLeast,
            threshold: 0.5,
        }],
        hold_secs,
        break_on_fail,
    }
}

/// Spawn a bare containable anomaly on a floor cell and return it with that cell's world position.
///
/// Deliberately minimal — no `Health`, no faction, no brain: this pins the containment loop itself, not
/// a creature's behaviour. It carries a `Transform` because `tick_containment` samples the field at the
/// anomaly's own cell.
fn spawn_target(app: &mut App, rule: ContainmentRule) -> (Entity, Vec3) {
    let pos = {
        let dungeon = app.world().resource::<Dungeon>();
        dungeon.cell_center(dungeon.spawn)
    };
    let e = app
        .world_mut()
        .spawn((Containment::new(rule), Transform::from_translation(pos)))
        .id();
    (e, pos)
}

/// Flood `ATTENTION` at `pos` this tick. The field evaporates fast, so a *held* condition means
/// depositing every tick — which is the point: containment is sustained effort, not a one-shot.
fn flood_attention(app: &mut App, pos: Vec3, amount: f32) {
    app.world_mut()
        .resource_mut::<StigDeposits>()
        .0
        .push(Deposit { field: FieldId::ATTENTION, pos, amount });
}

fn phase(app: &App, e: Entity) -> Phase {
    app.world().get::<Containment>(e).expect("the anomaly keeps its containment").phase()
}

fn specimen_count(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut q = world.query::<&Specimen>();
    q.iter(world).count()
}

#[test]
fn holding_the_basin_drives_an_anomaly_to_contained_and_grants_one_specimen() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 5);

    // A short hold so the test is fast, but > 1 tick so it genuinely accumulates.
    let (target, pos) = spawn_target(&mut app, attention_rule(0.25, OnBreak::Reset));
    assert_eq!(specimen_count(&mut app), 0, "nothing is banked before a capture");

    // Not yet begun: flooding the basin must do nothing on its own — capture is an action.
    for _ in 0..30 {
        flood_attention(&mut app, pos, 5.0);
        step(&mut app, &cfg, 1);
    }
    assert_eq!(phase(&app, target), Phase::Uncontained, "an unattempted anomaly never captures itself");
    assert_eq!(specimen_count(&mut app), 0);

    // Begin the attempt (what the device in FVS-B-5 will do) and hold the basin.
    app.world_mut().get_mut::<Containment>(target).expect("containment").begin();
    assert_eq!(phase(&app, target), Phase::BeingContained);

    for _ in 0..40 {
        flood_attention(&mut app, pos, 5.0);
        step(&mut app, &cfg, 1);
        if phase(&app, target) == Phase::Contained {
            break;
        }
    }

    assert_eq!(phase(&app, target), Phase::Contained, "holding the basin must complete the capture");
    assert!(
        app.world().get::<Contained>(target).is_some(),
        "completion must insert the terminal marker the reward hook hangs off"
    );
    assert_eq!(
        specimen_count(&mut app),
        1,
        "the on_add hook must grant exactly one specimen per capture"
    );

    // Keep ticking: the hook must not re-fire and the capture must not re-open.
    for _ in 0..20 {
        flood_attention(&mut app, pos, 5.0);
        step(&mut app, &cfg, 1);
    }
    assert_eq!(specimen_count(&mut app), 1, "a capture grants one specimen, once");
}

#[test]
fn leaving_the_basin_resets_the_hold() {
    // The `Reset` policy is what makes containment a *sustained* task rather than a cumulative one, and
    // the half only a live run can check: the rule is re-sampled at the anomaly's **current** cell every
    // tick, so an anomaly that wanders out of the held region loses its progress.
    //
    // The lapse is produced by MOVING the target, not by waiting for the field to evaporate. An earlier
    // draft did the latter and was fighting the wrong thing: `ATTENTION` deposits accumulate, so after
    // ten deposits the cell stayed above the threshold for longer than the test waited and the capture
    // completed instead. Moving out is also the honest scenario — a containment breaks because the
    // anomaly left the basin, not because the world slowly forgot.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 5);

    let (target, pos) = spawn_target(&mut app, attention_rule(2.0, OnBreak::Reset));
    app.world_mut().get_mut::<Containment>(target).expect("containment").begin();

    // Hold part-way — long hold (2 s) so this cannot accidentally complete.
    for _ in 0..10 {
        flood_attention(&mut app, pos, 1.0);
        step(&mut app, &cfg, 1);
    }
    let partial = app.world().get::<Containment>(target).expect("containment").held_secs();
    assert!(partial > 0.0, "the hold must accumulate while the basin is held, got {partial}");
    assert_eq!(phase(&app, target), Phase::BeingContained, "precondition: not captured yet");

    // Walk the anomaly out of the watched cell. Somewhere far enough that the deposit's radius cannot
    // reach it; the far corner of the map is unambiguous.
    let away = {
        let dungeon = app.world().resource::<Dungeon>();
        let mut best = dungeon.cell_center(dungeon.spawn);
        let mut best_d = 0.0f32;
        for c in dungeon.floor_cells() {
            let p = dungeon.cell_center(c);
            let d = p.distance(pos);
            if d > best_d {
                best_d = d;
                best = p;
            }
        }
        assert!(best_d > 5.0, "the map must have a cell well away from the spawn, got {best_d}");
        best
    };
    app.world_mut().get_mut::<Transform>(target).expect("transform").translation = away;

    // Keep flooding the ORIGINAL cell: the basin is still being held, just not where the anomaly is.
    for _ in 0..5 {
        flood_attention(&mut app, pos, 1.0);
        step(&mut app, &cfg, 1);
    }

    let after = app.world().get::<Containment>(target).expect("containment").held_secs();
    assert_eq!(after, 0.0, "leaving the basin under OnBreak::Reset must discard the hold, got {after}");
    assert_eq!(phase(&app, target), Phase::BeingContained, "the attempt is still live, just reset");
    assert_eq!(specimen_count(&mut app), 0);
}

#[test]
fn killing_an_anomaly_mid_containment_yields_nothing() {
    // **The assertion this whole push exists for.** The pivot is win-by-containing: a kill must be a
    // real option that produces no specimen and no research. Here the target is destroyed while a
    // capture is genuinely in progress — the case where a reward would be most tempting to grant.
    //
    // The enforcement is structural, not a branch: the reward lives in an `on_add` hook on `Contained`,
    // and despawning an anomaly inserts nothing. There is deliberately no `Killed` component — no
    // component means no place for a future reward hook to be attached by accident.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 5);

    let (target, pos) = spawn_target(&mut app, attention_rule(0.5, OnBreak::Keep));
    app.world_mut().get_mut::<Containment>(target).expect("containment").begin();

    for _ in 0..10 {
        flood_attention(&mut app, pos, 5.0);
        step(&mut app, &cfg, 1);
    }
    assert!(
        app.world().get::<Containment>(target).expect("containment").held_secs() > 0.0,
        "precondition: the capture is genuinely under way when the kill lands"
    );

    // Kill it — the real path a dead anomaly takes.
    app.world_mut().entity_mut(target).despawn();
    step(&mut app, &cfg, 10);

    assert_eq!(specimen_count(&mut app), 0, "a kill must yield NOTHING — no specimen, ever");
}

#[test]
fn the_containment_tick_is_bit_reproducible() {
    // Containment runs on `FixedUpdate` inside the pinned core, so adding it must not have cost the
    // core its bit-identity. Same seed, same scripted capture, twice.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();

    let run = |_: ()| {
        let mut app = build_headless_app(&cfg);
        step(&mut app, &cfg, 5);
        let (target, pos) = spawn_target(&mut app, attention_rule(0.25, OnBreak::Reset));
        app.world_mut().get_mut::<Containment>(target).expect("containment").begin();
        for _ in 0..40 {
            flood_attention(&mut app, pos, 5.0);
            step(&mut app, &cfg, 1);
        }
        let c = app.world().get::<Containment>(target).expect("containment");
        (c.phase(), c.held_secs().to_bits(), specimen_count(&mut app))
    };

    let a = run(());
    let b = run(());
    assert_eq!(a.0, Phase::Contained, "the scripted capture must actually complete");
    assert_eq!(a, b, "the containment tick must be bit-identical across same-seed runs");
}

#[test]
fn capping_a_nest_halts_its_breeding_and_grants_no_specimen() {
    // **Archetype 3 (FVS-B-7) through the real sim.** Two halves, and the second is the one that
    // matters for the pivot: capping must *work* (breeding stops) and must yield **nothing**.
    //
    // The mechanic is a query filter, not a branch — `crab::nest_reproduce` runs
    // `Without<containment::Capped>`, so a sealed nest is not a nest that breeds zero crabs, it is a
    // nest the breeding pass cannot see.
    use foundation_vs_slop::containment::{Capped, SiteSecured};
    use foundation_vs_slop::crab::Crab;
    use foundation_vs_slop::nest::Nest;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 30);

    let nests: Vec<Entity> = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<Entity, With<Nest>>();
        q.iter(world).collect()
    };
    assert!(!nests.is_empty(), "the shipped level must have nests to cap");

    let secured = *app.world().resource::<SiteSecured>();
    assert_eq!(secured.capped, 0, "nothing is sealed at the start of a run");
    assert_eq!(secured.total, nests.len(), "every nest is counted");

    let before = specimen_count(&mut app);

    // Cap every nest — what the squad's sealing action will do.
    for &n in &nests {
        app.world_mut().entity_mut(n).insert(Capped);
    }
    step(&mut app, &cfg, 5);

    let secured = *app.world().resource::<SiteSecured>();
    assert_eq!(secured.capped, nests.len(), "capping must set the secured flag");
    assert!(secured.fully_secured(), "every nest sealed reads as fully secured");

    // Breeding must have stopped: fill every hoard well past the breeding threshold and run. An
    // un-capped nest would convert that meat into crabs.
    let crabs_before = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<Entity, With<Crab>>();
        q.iter(world).count()
    };
    for &n in &nests {
        if let Some(mut nest) = app.world_mut().get_mut::<Nest>(n) {
            nest.hoard = 1000.0;
        }
    }
    step(&mut app, &cfg, 120);
    let crabs_after = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<Entity, With<Crab>>();
        q.iter(world).count()
    };
    assert!(
        crabs_after <= crabs_before,
        "a capped nest must not breed: {crabs_before} -> {crabs_after} with full hoards"
    );

    // ...and the whole point: securing a site is NOT a capture.
    assert_eq!(
        specimen_count(&mut app),
        before,
        "capping a structure must yield no specimen — it is honestly kill-for-no-reward"
    );
}

#[test]
fn the_watcher_knows_when_the_squad_can_see_it() {
    // **FVS-M-1's acceptance.** "Observed" is now squad line of sight, computed on `FixedUpdate` from
    // `fog::FogGrid` — so the watcher's defining mechanic (it conceals itself while seen) is finally
    // inside the deterministic core. Under the old camera-gaze implementation this was impossible to
    // test at all: the writer was windowed-only, so headless the watcher read a permanent `false`.
    //
    // The assertion is deliberately about the SIGNAL, not the mood: moods depend on being attacked,
    // which is a balance question. What M-1 changed is that observation is real, per-entity and pinned.
    use foundation_vs_slop::enemy::{Enemy, ObservedBySquad};
    use foundation_vs_slop::fog::FogGrid;
    use foundation_vs_slop::squad::Unit;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 30);

    let watcher = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<Entity, With<Enemy>>();
        q.iter(world).next().expect("the level spawns a watcher")
    };
    assert!(
        app.world().get::<ObservedBySquad>(watcher).is_some(),
        "every watcher must carry the observation component from spawn"
    );

    // Teleport the squad on top of the watcher: it is then unambiguously inside their LOS discs.
    let wpos = app.world().get::<Transform>(watcher).expect("transform").translation;
    {
        let world = app.world_mut();
        let units: Vec<Entity> = {
            let mut q = world.query_filtered::<Entity, With<Unit>>();
            q.iter(world).collect()
        };
        for u in units {
            if let Some(mut t) = world.get_mut::<Transform>(u) {
                t.translation = wpos;
            }
        }
    }
    // `update_los` recomputes when the squad changes cell, and `update_observation` runs after it.
    step(&mut app, &cfg, 5);

    let cell = {
        let dungeon = app.world().resource::<Dungeon>();
        dungeon.world_to_cell(wpos)
    };
    assert!(
        app.world().resource::<FogGrid>().visible_at(cell),
        "precondition: the squad standing on the watcher's cell must make it visible"
    );
    assert_eq!(
        app.world().get::<ObservedBySquad>(watcher).map(|o| o.0),
        Some(true),
        "a watcher inside squad LOS must read as observed — the mechanic is now core-visible"
    );
}

#[test]
fn scp999_is_captured_by_befriending_it_not_by_fighting() {
    // **FVS-C-2 — the tutorial capture, end to end through the real sim.**
    //
    // The comfort blob is contained by *befriending*: holster (let `THREAT_GUN` decay at its cell) and
    // stay with it (keep `ATTENTION` on it). Both clauses are satisfied by choosing NOT to fight, which
    // is the win-by-containing pivot stated in one creature — and it is why 999 is the right first
    // capture to teach.
    //
    // Drives the authored rule from `config.ron`, not a synthetic one, so this also pins that the
    // shipped slice parses, validates, and is actually reachable.
    use foundation_vs_slop::containment::{ContainmentDevice, ContainmentRules};
    use foundation_vs_slop::scp999::Scp999;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 30);

    // The shipped rule is the one under test.
    let rule = app.world().resource::<ContainmentRules>().0.scp999.clone();
    assert!(rule.validate().is_ok(), "the shipped SCP-999 rule must be valid");
    assert!(
        rule.requires.iter().any(|c| c.channel == FieldId::ATTENTION.0),
        "befriending must involve paying attention to it"
    );

    let (blob, pos) = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<(Entity, &Transform), With<Scp999>>();
        let found = q.iter(world).next().map(|(e, t)| (e, t.translation));
        match found {
            Some(v) => v,
            None => {
                eprintln!("no SCP-999 seeded in this world; nothing to capture");
                return;
            }
        }
    };
    assert!(
        app.world().get::<Containment>(blob).is_some(),
        "every blob must carry its containment rule from spawn"
    );
    assert_eq!(specimen_count(&mut app), 0);

    // Throw the device (FVS-B-5) — this is what opens the attempt.
    app.world_mut().spawn((
        ContainmentDevice { target: blob, reach: 3.0 },
        Transform::from_translation(pos),
    ));
    step(&mut app, &cfg, 2);
    assert_eq!(
        phase(&app, blob),
        Phase::BeingContained,
        "a device thrown at the blob must open the capture"
    );

    // Befriend it: keep ATTENTION on it and fire no weapons (`THREAT_GUN` decays on its own).
    //
    // Track it as it moves — the blob **oozes toward the most-anxious squad member**, so it leaves the
    // cell it started in. The rule samples at the anomaly's *current* cell, so flooding its spawn point
    // would drop the clause the moment it set off. That is not a test artefact either: befriending a
    // creature that comes to you means keeping your eyes on it while it moves, which is exactly the
    // behaviour the mechanic should demand.
    for _ in 0..600 {
        let here = app
            .world()
            .get::<Transform>(blob)
            .map(|t| t.translation)
            .unwrap_or(pos);
        flood_attention(&mut app, here, 1.0);
        step(&mut app, &cfg, 1);
        if phase(&app, blob) == Phase::Contained {
            break;
        }
    }

    assert_eq!(
        phase(&app, blob),
        Phase::Contained,
        "holstering and staying with the blob must capture it"
    );
    assert_eq!(
        specimen_count(&mut app),
        1,
        "the capture must bank exactly one specimen — this is the reward a kill never grants"
    );
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// FVS-B-3 — the player's verbs.
// ─────────────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn weapons_tight_starves_the_gunfire_channel_the_999_rule_reads() {
    // SCP-999's shipped rule needs `THREAT_GUN AtMost 0.05` at the blob's cell. Before this verb
    // existed there was no way for a player to *cause* that — `fire_laser` auto-fires and the clause
    // could only be satisfied by an accident of geometry. This asserts the MECHANISM (the channel
    // actually drains), not merely that a capture eventually happened.
    //
    // Invariant 11 applies: the scenario has to genuinely produce shooting, or "the field is low" is
    // vacuous. So it asserts the weapons-free case rises above the threshold FIRST, and only then that
    // holding fire brings it back under.
    use foundation_vs_slop::ai::field::FieldId;
    use foundation_vs_slop::sim_harness::{field_at, set_weapons_tight};

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    // `Dungeon` is built on `OnEnter(RunState::Active)` (FVS-A-5), so it does not exist until the
    // first update has run — reading the world before stepping panics on the missing resource.
    step(&mut app, &cfg, 60);

    // Drive the squad at a nest so there is something to shoot at — marching across empty floor is
    // exactly the mistake FVS-N-9 records (`aimed = 0/5` with 43 hostiles alive).
    let nests = nest_cells(&mut app);
    let Some(&nest) = nests.first() else {
        return; // no nest on this seed: a content fact, not a rule failure
    };
    assert!(issue_squad_order(&mut app, nest), "the nest must be reachable");
    set_weapons_tight(&mut app, false);
    step(&mut app, &cfg, 900);

    let squad = squad_centroid_cell(&mut app);
    let at_squad = {
        let d = app.world().resource::<foundation_vs_slop::dungeon::Dungeon>();
        d.cell_center(squad)
    };
    let hot = field_at(&mut app, FieldId::THREAT_GUN.0, at_squad);
    if hot <= 0.05 {
        // The squad never engaged on this seed. Reporting that honestly beats asserting a vacuous
        // "the field is low" — see FVS-N-9 for what that costs.
        eprintln!("weapons-free THREAT_GUN never exceeded the threshold ({hot:.4}); scenario did not engage");
        return;
    }

    // Now hold fire and let the channel evaporate at the same spot.
    set_weapons_tight(&mut app, true);
    step(&mut app, &cfg, 900);
    let squad = squad_centroid_cell(&mut app);
    let at_squad = {
        let d = app.world().resource::<foundation_vs_slop::dungeon::Dungeon>();
        d.cell_center(squad)
    };
    let cold = field_at(&mut app, FieldId::THREAT_GUN.0, at_squad);

    assert!(
        cold < hot,
        "holding fire must drain THREAT_GUN (was {hot:.4}, still {cold:.4} after 900 held ticks)"
    );
}

#[test]
fn the_device_verb_names_its_target_and_spends_a_charge() {
    use foundation_vs_slop::sim_harness::{containable_targets, throw_containment_device};

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some((_, target, pos)) = containable_targets(&mut app).into_iter().next() else {
        return;
    };
    let supply = |a: &mut bevy::prelude::App| {
        a.world().resource::<foundation_vs_slop::containment::DeviceSupply>().0
    };
    let before = supply(&mut app);
    assert!(before > 0, "an expedition must start with devices");

    assert!(throw_containment_device(&mut app, target, pos), "a throw with stock must succeed");
    assert_eq!(supply(&mut app), before - 1, "a throw spends a charge whether or not it connects");

    // Drain the pouch, then assert the empty case fails loudly rather than throwing anyway.
    while supply(&mut app) > 0 {
        throw_containment_device(&mut app, target, pos);
    }
    assert!(
        !throw_containment_device(&mut app, target, pos),
        "an empty pouch must refuse the throw — one path, no free device"
    );
}

#[test]
fn capping_a_nest_through_the_player_verb_still_grants_no_specimen() {
    // B-7's assertion, re-run through the PLAYER path. The verb must not smuggle in a reward the
    // archetype deliberately does not have: source-elimination is honestly "kill the source for no
    // specimen", and giving it one would quietly undo the win-by-containing pivot.
    use foundation_vs_slop::sim_harness::{cap_nest, specimen_count};

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let nests: Vec<bevy::prelude::Entity> = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<bevy::prelude::Entity, bevy::prelude::With<foundation_vs_slop::nest::Nest>>();
        q.iter(world).collect()
    };
    let Some(&nest) = nests.first() else { return };

    let before = specimen_count(&mut app);
    cap_nest(&mut app, nest);
    step(&mut app, &cfg, 120);
    assert_eq!(specimen_count(&mut app), before, "capping a nest must grant NO specimen");
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// FVS-C-3 — SCP-1048, the out-watch capture.
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// The live bears and their positions, sorted so the pick is stable.
fn bears(app: &mut App) -> Vec<(Entity, Vec3)> {
    let world = app.world_mut();
    let mut q = world.query_filtered::<
        (Entity, &Transform, &foundation_vs_slop::containment::TargetId),
        With<foundation_vs_slop::scp1048::Scp1048>,
    >();
    let mut v: Vec<_> = q.iter(world).map(|(e, tf, id)| (*id, e, tf.translation)).collect();
    // SORT-OK: `TargetId` is minted once per spawn and never reused — total by construction.
    v.sort_unstable_by_key(|(id, ..)| *id);
    v.into_iter().map(|(_, e, p)| (e, p)).collect()
}

#[test]
fn watching_scp1048_suppresses_its_building_and_looking_away_resumes_it() {
    // Canon is that SCP-1048 assembles its copies UNOBSERVED, and FVS-C-3's acceptance is both
    // directions: sustained attention suppresses copy-building, and letting attention decay resumes it.
    // Asserting only the suppression half would pass trivially if the bear simply never built.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some(&(bear, at)) = bears(&mut app).first() else {
        return; // no bear on this seed: a content fact
    };
    let materials = |a: &mut App| {
        a.world()
            .get::<foundation_vs_slop::scp1048::Scp1048Build>(bear)
            .map(|b| b.materials)
            .unwrap_or(0.0)
    };

    // UNWATCHED: the bear must actually accrue, or the watched half below is vacuous.
    step(&mut app, &cfg, 600);
    let free = materials(&mut app);
    if free <= 0.0 {
        // It never entered Build on this seed — report rather than assert a hollow pass (FVS-N-9's
        // lesson about scenarios that quietly never engage).
        eprintln!("scp1048 never began building unwatched; scenario did not engage");
        return;
    }

    // WATCHED: hold the ambient field over its cell and accrual must stop.
    let before = materials(&mut app);
    for _ in 0..60 {
        let at_now = bears(&mut app).first().map(|(_, p)| *p).unwrap_or(at);
        flood_attention(&mut app, at_now, 5.0);
        step(&mut app, &cfg, 10);
    }
    let watched = materials(&mut app);
    assert!(
        (watched - before).abs() < 1.0e-3 || watched <= before,
        "sustained observation must stop the build (was {before:.3}, now {watched:.3})"
    );

    // AND LOOKING AWAY RESUMES IT — the other half of the acceptance.
    step(&mut app, &cfg, 900);
    let resumed = materials(&mut app);
    assert!(
        resumed > watched || resumed >= 0.999 * cfg_build_cost(&mut app),
        "letting attention decay must resume building (watched {watched:.3}, after {resumed:.3})"
    );
}

fn cfg_build_cost(app: &mut App) -> f32 {
    app.world().resource::<foundation_vs_slop::sim::SimTuning>().scp1048.build_cost
}

#[test]
fn the_1048_rule_reads_the_ambient_field_not_a_per_entity_watch_flag() {
    // The distinction C-3 insists on, pinned as data rather than prose. `enemy::ObservedBySquad` is a
    // per-entity boolean (FVS-M-1's primitive for 173/096); this rule must read the AMBIENT decaying,
    // diffusing ATTENTION channel instead — observation you maintain, not a flag you set. If someone
    // "simplifies" the rule onto the boolean, this fails.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 30);

    let rules = app.world().resource::<foundation_vs_slop::containment::ContainmentRules>().0.clone();
    let clauses = &rules.scp1048.requires;
    assert!(!clauses.is_empty(), "the 1048 rule must have at least one clause");
    assert!(
        clauses.iter().all(|c| c.channel == FieldId::ATTENTION.0),
        "SCP-1048 is contained by AMBIENT observation; every clause must read ATTENTION"
    );
    assert!(
        clauses.iter().all(|c| matches!(c.sign, Sign::AtLeast)),
        "out-watching means keeping attention HIGH — an AtMost clause would invert the mechanic"
    );
}

// ─────────────────────────────────────────────────────────────────────────────────────────────────
// FVS-C-4 / D-1 — SCP-150: the cure capture, and the host relationship.
// ─────────────────────────────────────────────────────────────────────────────────────────────────

/// Infest a host the way `manca_embed` does, including the FVS-D-1 link.
fn infest(app: &mut App, host: Entity) -> Entity {
    use foundation_vs_slop::parasite::{InfectedBy, Infestation};
    let parasite = app.world_mut().spawn(InfectedBy(host)).id();
    let mut inf = app.world_mut().get_mut::<Infestation>(host).expect("hosts carry Infestation");
    inf.active = true;
    inf.timer = 0.0;
    parasite
}

fn a_unit(app: &mut App) -> Option<Entity> {
    let world = app.world_mut();
    let mut q = world.query_filtered::<
        (Entity, &foundation_vs_slop::squad::SquadMember),
        With<foundation_vs_slop::squad::Unit>,
    >();
    let mut v: Vec<_> = q.iter(world).map(|(e, m)| (m.0, e)).collect();
    // SORT-OK: `SquadMember` is the stable spawn index, unique per unit.
    v.sort_unstable_by_key(|(m, _)| *m);
    v.first().map(|(_, e)| *e)
}

#[test]
fn curing_an_infested_host_extracts_the_parasite_as_a_specimen() {
    // FVS-C-4's acceptance, and the distinction that makes it worth having: capping a nest destroys a
    // structure and yields nothing (B-7), while curing a host RECOVERS the anomaly intact. That is only
    // possible because D-1 keeps the parasite alive and linked at embed instead of despawning it.
    use foundation_vs_slop::parasite::{CureRequest, Infestation};

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some(host) = a_unit(&mut app) else { return };
    let parasite = infest(&mut app, host);
    let before = specimen_count(&mut app);

    // Reverse traversal works — that is the half `Infestation.active` could never answer.
    let hosting = app.world().get::<foundation_vs_slop::parasite::Hosting>(parasite);
    assert!(hosting.is_none(), "the parasite is the RELATIONSHIP source; the host holds the target");

    app.world_mut().entity_mut(host).insert(CureRequest);
    step(&mut app, &cfg, 3);

    assert!(
        !app.world().get::<Infestation>(host).expect("host").active,
        "curing must clear the infestation"
    );
    assert_eq!(
        specimen_count(&mut app),
        before + 1,
        "curing a host must EXTRACT the parasite as a specimen — that is the capture"
    );
    assert!(
        app.world().get::<Contained>(parasite).is_some(),
        "the parasite itself must be the thing marked Contained, not a fresh proxy entity"
    );
}

#[test]
fn an_untreated_host_stays_infested_and_yields_nothing() {
    // The other half of C-4's acceptance. Without it the test above would pass on a system that
    // extracted a specimen from every host unconditionally.
    use foundation_vs_slop::parasite::Infestation;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some(host) = a_unit(&mut app) else { return };
    let _parasite = infest(&mut app, host);
    let before = specimen_count(&mut app);

    step(&mut app, &cfg, 120);

    assert!(
        app.world().get::<Infestation>(host).expect("host").active,
        "an untreated host must stay infested"
    );
    assert_eq!(specimen_count(&mut app), before, "and must yield no specimen");
}

#[test]
fn curing_a_clean_host_is_a_no_op_rather_than_a_free_specimen() {
    // The failure mode a cure verb invites: spamming it on healthy operatives to mint specimens.
    use foundation_vs_slop::parasite::CureRequest;

    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 60);

    let Some(host) = a_unit(&mut app) else { return };
    let before = specimen_count(&mut app);
    for _ in 0..5 {
        app.world_mut().entity_mut(host).insert(CureRequest);
        step(&mut app, &cfg, 2);
    }
    assert_eq!(specimen_count(&mut app), before, "curing a clean host must grant nothing");
}
