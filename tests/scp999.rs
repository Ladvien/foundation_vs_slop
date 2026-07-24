//! SCP-999 comfort-blob behavioural tests (feature `test-harness`, needs a GPU). Boots the real game
//! headless and proves the tickle-calm mechanic works end-to-end: being tickled *lowers a member's FEAR
//! and lifts their MORALE*. The determinism of the blob's seek/calm is covered by the replay suite
//! (`deterministic_core_is_bit_identical*`, `search_rollouts_are_reproducible_under_load`), which now
//! includes SCP-999 in the harness; the pure target-pick is unit-tested in `scp999::movement`.
//!
//! Each test holds `serial_guard()` for its whole lifetime — two headless Apps must not run concurrently.
#![cfg(feature = "test-harness")]

use bevy::prelude::*;
use foundation_vs_slop::ai::drives::{DriveId, Drives};
use foundation_vs_slop::scp999::Scp999;
use foundation_vs_slop::sim_harness::{build_headless_app, serial_guard, step, SimConfig};
use foundation_vs_slop::dungeon::Dungeon;
use foundation_vs_slop::sim::SimTuning;
use foundation_vs_slop::squad::SquadMember;

#[test]
fn scp999_is_present_in_the_deterministic_core() {
    // Integration proof: the shipped config spawns the comfort blob INTO the pinned sim (so the
    // determinism gate covers it), not just the windowed game.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 2);
    let n = app
        .world_mut()
        .query_filtered::<(), With<Scp999>>()
        .iter(app.world())
        .count();
    assert!(n >= 1, "the shipped `scp999.count` must spawn >= 1 comfort blob into the deterministic core");
}

#[test]
fn the_comfort_blob_starts_out_in_the_level_not_beside_the_squad() {
    // The blob used to fan out one tile behind the squad, so relief was handed over at t=0 and the squad
    // never carried its fear anywhere. It now seeds like the crabs and the Smiley — at least
    // `sim.scp999.spawn_min_dist` tiles from the squad spawn (player debug capture 2026-07-24).
    //
    // Measured on the *first* frames, before it has oozed anywhere: `step(.., 2)` is dungeon gen + all
    // spawns, matching the other tests here.
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 2);

    let min_dist = app.world().resource::<SimTuning>().scp999.spawn_min_dist;
    let dungeon = app.world().resource::<Dungeon>();
    let spawn = dungeon.cell_center(dungeon.spawn);

    let blobs: Vec<Vec3> = app
        .world_mut()
        .query_filtered::<&Transform, With<Scp999>>()
        .iter(app.world())
        .map(|t| t.translation)
        .collect();
    assert!(!blobs.is_empty(), "the shipped config must place a comfort blob");
    for pos in blobs {
        let d = pos.distance(spawn);
        assert!(
            d >= min_dist,
            "a comfort blob started {d:.1} units from the squad spawn, under the {min_dist} minimum"
        );
    }
}

/// Run the core for a short window with member-0 pinned frightened + demoralised, and the single comfort
/// blob placed either ON member 0 (in tickle contact) or FAR away (out of reach). Returns member 0's final
/// `(FEAR, MORALE)`. Both arms are the same seed and identical state up to the placement — the ONLY
/// difference is blob proximity — so differencing them isolates the tickle effect from natural FEAR decay
/// and any self-Ward, exactly like `replay::a_mutated_audio_config_changes_the_sim`'s A/B.
fn fear_morale_after(blob_on_member0: bool) -> (f32, f32) {
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 2); // dungeon gen + all spawns

    // Select member 0 by its STABLE `SquadMember` id (query order is not stable across App instances, so
    // `.next()` could pick a different unit in each arm — the id keeps both arms on the same member).
    let member0 = {
        let mut q = app.world_mut().query::<(Entity, &SquadMember, &Transform)>();
        q.iter(app.world())
            .filter(|(_, m, _)| m.0 == 0)
            .map(|(e, _, t)| (e, t.translation))
            .next()
            .expect("the squad must have SquadMember(0)")
    };
    let (unit, upos) = member0;

    // Pin member 0 maximally frightened + demoralised.
    {
        let mut d = app.world_mut().get_mut::<Drives>(unit).expect("member 0 has Drives");
        d.set(DriveId::FEAR, 1.0);
        d.set(DriveId::MORALE, 0.0);
    }
    // Place the blob (member 0 is now the most-anxious, so every blob targets it either way).
    let blob = app
        .world_mut()
        .query_filtered::<Entity, With<Scp999>>()
        .iter(app.world())
        .next()
        .expect("a comfort blob must exist in the core");
    {
        let mut t = app.world_mut().get_mut::<Transform>(blob).expect("blob has Transform");
        // ON member 0 → immediate contact; else 60 m away → cannot ooze into reach within the window.
        t.translation = if blob_on_member0 { upos } else { upos + Vec3::new(60.0, 0.0, 60.0) };
    }

    step(&mut app, &cfg, 30); // ~0.5 s

    let d = app.world().get::<Drives>(unit).expect("member 0 still has Drives");
    (d.get(DriveId::FEAR), d.get(DriveId::MORALE))
}

#[test]
fn tickling_lowers_fear_and_lifts_morale() {
    let _serial = serial_guard();
    let (fear_tickled, morale_tickled) = fear_morale_after(true);
    let (fear_far, morale_far) = fear_morale_after(false);

    // The tickle drains EXTRA fear beyond the natural decay both arms share...
    assert!(
        fear_tickled < fear_far,
        "tickling must lower FEAR faster than proximity-free decay (tickled {fear_tickled} vs far {fear_far})"
    );
    // ...and lifts morale, which nothing else raises for a non-warding member (so far-arm morale is ~0).
    assert!(
        morale_tickled > morale_far,
        "tickling must lift MORALE (tickled {morale_tickled} vs far {morale_far})"
    );
}
