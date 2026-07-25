//! SCP-1048 "Builder Bear" behavioural tests (feature `test-harness`). Boots the real game headless
//! and proves the three mechanics the module exists for, end to end:
//!
//! 1. the bear seeds **into the deterministic core**, out in the level rather than at the squad's feet;
//! 2. the original **builds hostile copies**, and stops exactly at the population cap;
//! 3. SCP-1048-A's shriek **grows ear tissue** on nearby units, which then suffocates them.
//!
//! Determinism of all of it is covered by the replay suite (`deterministic_core_is_bit_identical*`,
//! `search_rollouts_are_reproducible_under_load`), which includes the bear in the harness; the pure
//! variant draw, clip tables and strike dispatch are unit-tested inside `src/scp1048/`.
//!
//! Each test holds `serial_guard()` for its whole lifetime — two headless Apps must not run at once.
#![cfg(feature = "test-harness")]

use bevy::prelude::*;
use foundation_vs_slop::dungeon::Dungeon;
use foundation_vs_slop::enemy::Hostile;
use foundation_vs_slop::health::Health;
use foundation_vs_slop::laser::LaserTarget;
use foundation_vs_slop::scp1048::{
    EarGrowth, Scp1048, Scp1048Build, Scp1048Variant,
};
use foundation_vs_slop::sim::SimTuning;
use foundation_vs_slop::sim_harness::{build_headless_app, serial_guard, step, SimConfig};
use foundation_vs_slop::squad::SquadMember;

/// Every bear alive, as `(entity, variant, position)`.
fn bears(app: &mut App) -> Vec<(Entity, Scp1048Variant, Vec3)> {
    app.world_mut()
        .query::<(Entity, &Scp1048, &Transform)>()
        .iter(app.world())
        .map(|(e, b, t)| (e, b.variant, t.translation))
        .collect()
}

/// Member 0 by its **stable** `SquadMember` id. Query order is not stable across `App` instances, so
/// `.next()` could pick a different unit in each arm of an A/B and silently compare two people.
fn member0(app: &mut App) -> (Entity, Vec3) {
    let mut q = app.world_mut().query::<(Entity, &SquadMember, &Transform)>();
    q.iter(app.world())
        .filter(|(_, m, _)| m.0 == 0)
        .map(|(e, _, t)| (e, t.translation))
        .next()
        .expect("the squad must have SquadMember(0)")
}

#[test]
fn scp1048_is_present_in_the_deterministic_core() {
    // Integration proof: the shipped `scp1048.count` seeds the bear INTO the pinned sim, so the
    // determinism gate covers it — not just the windowed game.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 2);
    let found = bears(&mut app);
    assert!(!found.is_empty(), "the shipped config must seed >= 1 Builder Bear into the core");
    assert!(
        found.iter().all(|(_, v, _)| *v == Scp1048Variant::Original),
        "only the BENIGN original may be seeded — the copies must be built during play, or the \
         replication mechanic is given away at t=0"
    );
}

#[test]
fn the_builder_bear_starts_out_in_the_level_not_beside_the_squad() {
    // Same rule every creature seeds by: at least `spawn_min_dist` tiles from the squad spawn, so the
    // bear has to be found. Measured on the first frames, before it has shuffled anywhere.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 2);

    let min_dist = app.world().resource::<SimTuning>().scp1048.spawn_min_dist;
    let dungeon = app.world().resource::<Dungeon>();
    let spawn = dungeon.cell_center(dungeon.spawn);

    for (_, _, pos) in bears(&mut app) {
        let d = pos.distance(spawn);
        assert!(d >= min_dist, "a bear started {d:.1} units from spawn, under the {min_dist} minimum");
    }
}

/// Bank enough material on every original and clear its cooldown, so the next `scp1048_replicate` pass
/// builds. A **durable** forced state (not a one-tick nudge), so emergent think timing cannot flake the
/// assertion — the same discipline `parasite::rouse_all_mancae` documents.
fn force_ready_to_build(app: &mut App) {
    let cost = app.world().resource::<SimTuning>().scp1048.build_cost;
    let originals: Vec<Entity> = app
        .world_mut()
        .query::<(Entity, &Scp1048)>()
        .iter(app.world())
        .filter(|(_, b)| b.variant == Scp1048Variant::Original)
        .map(|(e, _)| e)
        .collect();
    for e in originals {
        if let Some(mut b) = app.world_mut().get_mut::<Scp1048Build>(e) {
            b.materials = cost;
            b.cooldown = 0.0;
        }
    }
}

#[test]
fn the_original_builds_a_copy_and_stops_at_the_cap() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 2);

    // A tight cap and no cooldown, so the loop runs to its ceiling inside the test window.
    {
        let mut sim = app.world_mut().resource_mut::<SimTuning>();
        sim.scp1048.max_bears = 3;
        sim.scp1048.build_cooldown = 0.0;
    }
    let before = bears(&mut app).len();
    assert!(before < 3, "the seeded population must leave room to build");

    for _ in 0..12 {
        force_ready_to_build(&mut app);
        step(&mut app, &cfg, 5);
    }

    let after = bears(&mut app);
    assert!(after.len() > before, "the original must actually build a copy ({before} -> {})", after.len());
    assert_eq!(
        after.len(),
        3,
        "the population must stop EXACTLY at max_bears, not overshoot it — the cap is the only \
         thing standing between this mechanic and an exponential bear population"
    );
    assert!(
        after.iter().filter(|(_, v, _)| *v != Scp1048Variant::Original).count() > 0,
        "every bear built must be one of the hostile copies"
    );
}

#[test]
fn the_bear_breeds_unattended_with_nothing_forced() {
    // **The end-to-end proof, and the one that matters most.** Every other replication test hands the
    // bear its materials; this one hands it nothing and just lets the game run. It is what catches the
    // class of bug where the economy is unreachable from a cold start — the first draft gated
    // `Mode::Build` on `BuildReady`, but material only accrues *while in* `Mode::Build`, so the bear
    // could never take the first step and would have shipped sterile with every forced test green.
    //
    // The shipped economy is 12 material at 1.0/s, so ~12 s of unobserved building; 1800 ticks is 30 s
    // at the fixed rate, leaving room for think throttling and the odd interruption.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 2);
    let before = bears(&mut app).len();

    step(&mut app, &cfg, 1800);

    let after = bears(&mut app);
    assert!(
        after.len() > before,
        "left alone and unobserved, the Builder Bear must actually build something ({before} -> {}) \
         — if this fails, check that Mode::Build is reachable with no material banked",
        after.len()
    );
    assert!(
        after.iter().any(|(_, v, _)| *v != Scp1048Variant::Original),
        "what it builds must be a hostile copy"
    );
}

#[test]
fn every_built_copy_is_hostile_and_shootable_and_the_original_is_neither() {
    // The archetype contract the module docs rest on. If the original ever gained `Hostile`,
    // `fire_laser` would delete it on sight and replication would never run once.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 2);
    {
        let mut sim = app.world_mut().resource_mut::<SimTuning>();
        sim.scp1048.build_cooldown = 0.0;
    }
    for _ in 0..8 {
        force_ready_to_build(&mut app);
        step(&mut app, &cfg, 5);
    }

    for (e, variant, _) in bears(&mut app) {
        let hostile = app.world().get::<Hostile>(e).is_some();
        let targetable = app.world().get::<LaserTarget>(e).is_some();
        if variant == Scp1048Variant::Original {
            assert!(!hostile, "the benign original must not be Hostile");
            assert!(!targetable, "the benign original must not be shootable");
        } else {
            assert!(hostile, "{variant:?} must be Hostile");
            assert!(targetable, "{variant:?} must carry a LaserTarget");
        }
    }
}

/// Place a screaming SCP-1048-A either **on** member 0 or far away, run a short window, and return
/// member 0's ear-growth severity. Both arms share a seed and are identical up to the bear's position,
/// so differencing them isolates the shriek from everything else in the sim.
fn severity_after(bear_on_member0: bool) -> f32 {
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 2);

    // Force the family to build ear-bears specifically, then build one.
    {
        let mut sim = app.world_mut().resource_mut::<SimTuning>();
        sim.scp1048.copy_w_a = 1.0;
        sim.scp1048.copy_w_b = 0.0;
        sim.scp1048.build_cooldown = 0.0;
    }
    force_ready_to_build(&mut app);
    step(&mut app, &cfg, 5);

    let (unit, upos) = member0(&mut app);
    let ear = bears(&mut app)
        .into_iter()
        .find(|(_, v, _)| *v == Scp1048Variant::EarCopy)
        .map(|(e, _, _)| e)
        .expect("forcing copy_w_a = 1 must build an ear bear");
    {
        let mut t = app.world_mut().get_mut::<Transform>(ear).expect("the copy has a Transform");
        // On the member ⇒ inside the growth band and inside strike range, so it screams at them.
        // Otherwise 60 m away ⇒ it cannot shuffle into range within the window.
        t.translation = if bear_on_member0 { upos } else { upos + Vec3::new(60.0, 0.0, 60.0) };
    }

    step(&mut app, &cfg, 120); // ~2 s: long enough to decide, close, and shriek

    app.world().get::<EarGrowth>(unit).expect("every unit carries EarGrowth").severity
}

#[test]
fn screaming_grows_ear_tissue_on_a_nearby_unit() {
    let _serial = serial_guard();
    let near = severity_after(true);
    let far = severity_after(false);
    assert!(
        near > far,
        "a unit standing under SCP-1048-A's shriek must grow ear tissue (near {near} vs far {far})"
    );
    assert_eq!(far, 0.0, "a unit 60 m away must be untouched");
}

#[test]
fn a_fully_grown_unit_asphyxiates_and_an_unafflicted_one_does_not() {
    // The DoT's terminal half, isolated from the shriek that causes it: pin severity at the threshold
    // and check `Health` actually falls — and that a clean unit in the same run does not.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 2);

    let (unit, _) = member0(&mut app);
    // Fully covered — the state a victim is left in when the shriek stops, NOT the threshold itself.
    // Pinning it exactly at the threshold would be a knife-edge: the repair term drops severity under
    // the bar on the very first tick and nothing would ever fire. That the *shipped* threshold leaves
    // real headroom below 1.0 is what makes this scenario reachable at all, and
    // `effects::the_lethal_band_is_wide_enough_to_outlive_the_scream` is what pins it.
    app.world_mut().get_mut::<EarGrowth>(unit).expect("unit has EarGrowth").severity = 1.0;

    // A control: some other member, left unafflicted.
    let control = {
        let mut q = app.world_mut().query::<(Entity, &SquadMember)>();
        q.iter(app.world())
            .filter(|(_, m)| m.0 != 0)
            .map(|(e, _)| e)
            .next()
            .expect("the squad must have more than one member")
    };
    let hp_before = app.world().get::<Health>(unit).expect("unit has Health").current;
    let control_before = app.world().get::<Health>(control).expect("control has Health").current;

    step(&mut app, &cfg, 120); // ~2 s

    let hp_after = app.world().get::<Health>(unit).expect("unit has Health").current;
    let control_after = app.world().get::<Health>(control).expect("control has Health").current;
    assert!(
        hp_after < hp_before,
        "a fully-grown unit must suffocate ({hp_before} -> {hp_after})"
    );
    assert_eq!(
        control_after, control_before,
        "an unafflicted member must take no asphyxiation damage in the same run"
    );
}
