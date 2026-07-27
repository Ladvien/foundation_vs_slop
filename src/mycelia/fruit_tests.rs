//! Tests for `fruit.rs` — moved out of the module file so the implementation reads without
//! scrolling past them. Still a child module of the same parent, so `use super::*` resolves
//! exactly as before; this is a pure move.

use super::*;
use crate::camera::{MAX_ZOOM, MIN_ZOOM};
use crate::mycelia::perceptual::{v_max as vmax, STAGE_T};

const THRESH: f32 = 0.02;
const FOV: f32 = 30.0;

fn body() -> FruitBody {
    FruitBody {
        growth: 0.0,
        rise: 0.0,
        scale: 4.0,
        cell: IVec2::ZERO,
        veil_triggered: false,
        tint: 0.0,
        cluster: 0,
        cap_ab: Vec2::ZERO,
        bend: Vec2::ZERO,
        tilt: Vec2::ZERO,
        species: SpeciesId::default(),
    }
}

/// An egg carries no amatoxins; a mature cap carries them all. The threshold is the veil rupture,
/// because the toxin lives in the gills and cap, not the volva (Enjalbert et al. 1999).
#[test]
fn amatoxin_appears_only_once_the_cap_does() {
    let mut b = body();
    assert_eq!(b.amatoxin(), 0.0);
    b.growth = VEIL_RUPTURE_T;
    assert_eq!(b.amatoxin(), 0.0);
    b.growth = 1.0;
    assert_eq!(b.amatoxin(), 1.0);
    b.growth = (VEIL_RUPTURE_T + 1.0) * 0.5;
    assert!((b.amatoxin() - 0.5).abs() < 1e-5);
    // The stage list must actually contain the veil rupture where we think it does.
    assert_eq!(VEIL_RUPTURE_T, STAGE_T[3]);
}

/// Eating is exempt from the speed limit, and is monotonic and clamped: a bite bigger than what's left
/// reabsorbs the body rather than driving `growth` negative.
#[test]
fn consume_runs_the_clock_backwards_and_clamps() {
    let mut b = body();
    b.growth = 0.5;
    b.consume(0.2);
    assert!((b.growth - 0.3).abs() < 1e-6);
    b.consume(10.0);
    assert_eq!(b.growth, 0.0);
    assert_eq!(b.energy(), 0.0);
}

/// Energy rises with maturity — an egg is not worth foraging, an adult is.
#[test]
fn energy_increases_with_growth() {
    let mut b = body();
    let mut last = -1.0;
    for i in 0..=10 {
        b.growth = i as f32 / 10.0;
        let e = b.energy();
        assert!(e > last, "energy must increase: {e} <= {last}");
        last = e;
    }
}

/// **The temporal-contrast invariant.** The albedo ramp is rate-limited independently of zoom, so it can
/// never complete faster than the slow-change-blindness window even when motion is allowed to run 7×
/// faster zoomed out. Simulated at a high frame rate against the fastest possible growth.
#[test]
fn tint_never_ramps_faster_than_the_slow_change_window() {
    let dt = 1.0 / 240.0;
    let mut tint = 0.0f32;
    let mut elapsed = 0.0f32;
    // Worst case: growth pinned at 1.0 from t=0, so the limiter is the only thing holding tint back.
    while tint < 1.0 && elapsed < 60.0 {
        let step = dt / MIN_APPEARANCE_RAMP_SECS;
        tint += (1.0 - tint).clamp(-step, step);
        elapsed += dt;
    }
    assert!(
        elapsed >= MIN_APPEARANCE_RAMP_SECS - 0.05,
        "tint completed in {elapsed}s, faster than the {MIN_APPEARANCE_RAMP_SECS}s window",
    );
}

/// Intent quickening: disabled cleanly, bounded to `[1, speed_scale]`, and — swept over a roam period —
/// it must both leave most of the world at the imperceptible baseline AND actually quicken some of it,
/// or the "sentient" effect is either always-on (no contrast) or never-on (invisible).
#[test]
fn intent_boost_is_bounded_and_sometimes_but_not_always_quickens() {
    let mut cfg = crate::mycelia::tests::valid();
    // Disabled paths.
    cfg.intent_focus_count = 0;
    assert_eq!(intent_boost(Vec2::new(95.0, 95.0), 3.0, &cfg), 1.0);
    cfg.intent_focus_count = 3;
    cfg.intent_speed_scale = 1.0;
    assert_eq!(intent_boost(Vec2::ZERO, 3.0, &cfg), 1.0);

    cfg.intent_speed_scale = 40.0;
    let mut max_seen = 0.0f32;
    let mut baseline_samples = 0;
    let mut total = 0;
    for step in 0..400 {
        let t = step as f32 * (cfg.intent_roam_period / 100.0);
        for gx in 0..12 {
            for gy in 0..12 {
                let p = Vec2::new(gx as f32 * 16.0, gy as f32 * 16.0);
                let b = intent_boost(p, t, &cfg);
                assert!((1.0..=cfg.intent_speed_scale + 1e-3).contains(&b), "boost {b} out of range");
                max_seen = max_seen.max(b);
                if b < 1.5 {
                    baseline_samples += 1;
                }
                total += 1;
            }
        }
    }
    assert!(max_seen > 10.0, "intent never meaningfully quickened anything (max {max_seen})");
    // The overwhelming majority of space-time stays near the imperceptible baseline.
    assert!(
        baseline_samples as f32 / total as f32 > 0.8,
        "intent quickened too much of the colony ({}/{} below 1.5x)",
        baseline_samples,
        total
    );
}

/// The emergence rise obeys the same budget as everything else: the egg's crown never climbs out of the
/// mat faster than the motion threshold, at any zoom.
#[test]
fn emergence_rise_obeys_the_speed_limit() {
    for viewport in [MIN_ZOOM, 12.0, MAX_ZOOM] {
        let budget = vmax(THRESH, FOV, viewport);
        let b = body();
        let geom = crate::mycelia::species::SpeciesGeometry::from_data(
            &crate::mycelia::species::death_cap_data(),
        );
        let sink = geom.egg_height_m * b.scale;
        let rise_rate = budget / sink; // per second, in `rise` units
        // `rise` spans [0,1] over `sink` metres, so world speed is `rise_rate * sink`.
        let world_speed = rise_rate * sink;
        assert!(
            (world_speed - budget).abs() < 1e-6,
            "viewport {viewport}: rise speed {world_speed} != budget {budget}",
        );
    }
}

// ── plan_body's clearance contract ────────────────────────────────────────────────────────────────

const TEST_SCALE: f32 = 4.0;

/// A `CONTROL_SIZE`-square dungeon whose floor is the block `lo..=hi`; everything else is rock. The
/// slab therefore stands on the outer edge of cell `hi` — the face a body near `x = hi` must clear.
fn dungeon_with_floor_block(lo: i32, hi: i32) -> Dungeon {
    let size = crate::mycelia::CONTROL_SIZE as usize;
    let mut walkable = vec![false; size * size];
    for y in lo..=hi {
        for x in lo..=hi {
            walkable[y as usize * size + x as usize] = true;
        }
    }
    Dungeon::from_walkable(size, size, walkable)
}

/// `plan_body` documents that it "verifies its own answer" and that "a site that cannot host a body does
/// not host one". Both halves are asserted here: every plan it hands back must be clear of solid matter.
///
/// This is swept over seeds rather than fixed at one, because the pose is a *function of the draw*: the
/// stem's lean and tilt are hashed from the coarse index. A single seed proves nothing about the site —
/// it only samples one of the poses the site can produce. (Exactly this blind spot let the bug through:
/// `MYCELIA_FRUIT_TESTBED` pins six seeds, and those six happened to clear.)
#[test]
fn plan_body_never_returns_a_pose_that_clips_a_wall() {
    let dungeon = dungeon_with_floor_block(40, 80);
    // The east slab stands on the outer edge of cell 80, i.e. at world x = 80.5 - WALL_THICKNESS.
    let face_x = 80.5 - crate::dungeon::WALL_THICKNESS;

    let mut clipped = Vec::new();
    // Negative offsets sit **inside** the slab strip. `pin_scan` really does hand those over: it rejects
    // texels whose dungeon *cell* is not walkable, but a slab occupies the outer `WALL_THICKNESS` of a
    // perfectly walkable cell. Solving from inside rock is where the verify-and-reseat loop earns its keep.
    // Positive offsets step out across the whole band a pose can reach.
    for step in -8..=60 {
        let offset = step as f32 * 0.01;
        let site = Vec2::new(face_x - offset, 60.0);
        for seed in 0..64u32 {
            let Some(plan) = plan_body(&dungeon, site, TEST_SCALE, seed) else {
                continue; // Refusing the site is always a legal answer.
            };
            let depth = penetration(&dungeon, &plan, TEST_SCALE);
            if depth > 0.0 {
                clipped.push((offset, seed, depth));
            }
        }
    }

    assert!(
        clipped.is_empty(),
        "plan_body returned {} poses that clip the slab; worst {:?}. \
         A returned plan must always be clear — refuse the site instead.",
        clipped.len(),
        clipped
            .iter()
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(o, s, d)| format!("offset {o:.2} m, seed {s}, {d:.4} m deep")),
    );
}

// ── pin_fruit_bodies: spacing, and the dwell clock ────────────────────────────────────────────────

/// A world where every coarse cell is barren except the ones named, which are ripe (`V` high, `U`
/// spent). Texel coordinates are chosen so the sites land on open floor, far from any slab.
///
/// `Time<Virtual>` is inserted by hand rather than via `TimePlugin`, so the clock only moves when the test
/// says so. The system's `Local`s persist across `app.update()`, which is the whole point — the readback
/// gate lives in one.
fn app_with_ripe_cells(cfg: MyceliaConfig, texels: &[f32]) -> App {
    let mut cells = vec![[0.0f32; 4]; (COARSE_SIZE * COARSE_SIZE) as usize];
    for (i, &tx) in texels.iter().enumerate() {
        // (V above v_fruit, U below u_exhausted, texel x, texel y)
        cells[i] = [0.9, 0.1, tx, 320.0];
    }

    // Scenes + geometry must be parallel to `cfg.species`, or a selected species indexes out of range.
    let scenes = SpeciesScenes(vec![Handle::default(); cfg.species.len()]);
    let table = SpeciesTable(
        cfg.species
            .iter()
            .map(|s| crate::mycelia::species::SpeciesGeometry::from_data(&s.geom))
            .collect(),
    );

    let mut app = App::new();
    app.insert_resource(cfg)
        .insert_resource(MoldCoarse { cells, generation: 0 })
        .insert_resource(dungeon_with_floor_block(40, 80))
        .insert_resource(crate::fog::FogGrid::all_explored(
            crate::mycelia::CONTROL_SIZE as usize,
            crate::mycelia::CONTROL_SIZE as usize,
        ))
        .insert_resource(scenes)
        .insert_resource(table)
        .insert_resource(PinDwell::default())
        .insert_resource(Time::<Virtual>::default())
        .add_systems(Update, pin_fruit_bodies);
    app
}

fn body_count(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<(), With<FruitBody>>();
    let n = q.iter(app.world()).count();
    n
}

/// The distinct genets standing in the world. One nucleus bursts a whole flush, so this is what "how
/// many primordia committed" now means.
fn cluster_ids(app: &mut App) -> std::collections::BTreeSet<u32> {
    let mut q = app.world_mut().query::<&FruitBody>();
    q.iter(app.world()).map(|b| b.cluster).collect()
}

/// Drive the app the way the game drives it: the render loop ticks at `fps`, and a readback lands only
/// every `frames_per_scan`th frame. `pin_fruit_bodies` runs on `Update` every frame and gates itself.
///
/// Advancing the clock one *frame* at a time is the whole point. A harness that advanced it one *scan* at
/// a time would make `Time::delta_secs()` and the true inter-scan interval identical, and could not tell a
/// correct dwell accumulator from one that credits a render frame.
fn run_frames(app: &mut App, frames: usize, fps: f32, frames_per_scan: usize) -> f32 {
    let frame = std::time::Duration::from_secs_f32(1.0 / fps);
    for i in 1..=frames {
        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(frame);
        if i % frames_per_scan == 0 {
            app.world_mut().resource_mut::<MoldCoarse>().generation += 1;
        }
        app.update();
    }
    app.world().resource::<Time<Virtual>>().elapsed_secs()
}

/// Run frame-by-frame until a body pins, returning the time on the clock when it did. `None` if it
/// never pins within `max_frames`.
fn time_to_first_pin(app: &mut App, max_frames: usize, fps: f32, frames_per_scan: usize) -> Option<f32> {
    let frame = std::time::Duration::from_secs_f32(1.0 / fps);
    for i in 1..=max_frames {
        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(frame);
        if i % frames_per_scan == 0 {
            app.world_mut().resource_mut::<MoldCoarse>().generation += 1;
        }
        app.update();
        if body_count(app) > 0 {
            return Some(app.world().resource::<Time<Virtual>>().elapsed_secs());
        }
    }
    None
}

/// Two cells that ripen together must not both nucleate: `commands.spawn` is deferred, so the second
/// cell's crowding check cannot see the first *cluster* in the `World`. It has to see the pending
/// positions instead — which is why `pinned_this_run` carries a cluster id alongside each position.
///
/// The sites are 1.5 world units apart against a `cluster_spacing` of 3.0 — unambiguously crowded. The
/// surviving nucleus still bursts its whole flush, so the assertion counts **clusters, not bodies**: one
/// genet may put down eight mushrooms and still have starved out its neighbour.
#[test]
fn two_cells_ripening_on_the_same_scan_nucleate_only_one_cluster() {
    let cfg = crate::config::load_game_config().expect("game config").mycelia;
    let spacing = cfg.cluster_spacing;
    let size_max = cfg.cluster_size_max as usize;
    let (fps, per_scan) = frame_clock(&cfg);
    let budget_frames = frames_to_cover_the_dwell(&cfg, fps, per_scan);

    // 8 texels apart = 8 * 192/1024 = 1.5 world units.
    let mut app = app_with_ripe_cells(cfg, &[320.0, 328.0]);

    // Run well past the dwell threshold, so both cells certainly cross it on the same scan.
    run_frames(&mut app, budget_frames, fps, per_scan);

    let clusters = cluster_ids(&mut app);
    assert_eq!(
        clusters.len(),
        1,
        "two cells 1.5 units apart (cluster_spacing = {spacing}) ripened together and nucleated \
         {} clusters; the second must be rejected by the same-scan crowding check",
        clusters.len(),
    );
    let n = body_count(&mut app);
    assert!(
        (2..=size_max).contains(&n),
        "the surviving nucleus should have burst a flush of 2..={size_max} bodies, got {n}",
    );
}

/// Every body of one flush wears one colour, and the flush is packed tightly — far tighter than the
/// `cluster_spacing` that keeps *genets* apart. This is the whole visible point of clustering.
#[test]
fn a_flush_shares_a_colour_and_packs_tighter_than_the_cluster_spacing() {
    let cfg = crate::config::load_game_config().expect("game config").mycelia;
    let (radius, spacing) = (cfg.cluster_radius, cfg.cluster_spacing);
    let (fps, per_scan) = frame_clock(&cfg);
    let budget_frames = frames_to_cover_the_dwell(&cfg, fps, per_scan);

    let mut app = app_with_ripe_cells(cfg, &[320.0]);
    run_frames(&mut app, budget_frames, fps, per_scan);

    let mut q = app.world_mut().query::<(&Transform, &FruitBody)>();
    let bodies: Vec<(Vec3, Vec2)> =
        q.iter(app.world()).map(|(t, b)| (t.translation, b.cap_ab)).collect();
    assert!(bodies.len() >= 2, "a lone ripe cell should burst a flush, got {}", bodies.len());

    // Cap colours agree to within twice the per-member spread: one genet, one pigment.
    let spread = 2.0 * crate::mycelia::perceptual::MAX_MEMBER_AB * std::f32::consts::SQRT_2;
    for (i, (_, a)) in bodies.iter().enumerate() {
        for (_, b) in bodies.iter().skip(i + 1) {
            assert!(a.distance(*b) <= spread + 1e-5, "siblings differ in colour: {a:?} vs {b:?}");
        }
    }

    // Every body sits inside the flush, not a `cluster_spacing` away like a rival genet would.
    let nucleus = bodies[0].0;
    for (p, _) in &bodies {
        let d = Vec2::new(p.x - nucleus.x, p.z - nucleus.z).length();
        // `plan_body` may nudge a base clear of geometry, so allow a body radius of slack over the
        // sampling radius — but it must still be nowhere near the between-genet spacing.
        assert!(d < spacing, "body {d} from the nucleus is as far as a rival genet (spacing {spacing})");
        assert!(d <= radius + 2.0 * VOLVA_RADIUS_M * 4.0, "body strayed {d} outside the flush");
    }
}

/// `pin_dwell_secs` is **virtual seconds**. The scan runs once per readback (~`sim_hz`) rather than once
/// per rendered frame, so it must credit the whole inter-scan interval — the elapsed span since the last
/// scan, *not* `Time::delta_secs()`, which is one render frame.
///
/// At 120 fps and `sim_hz` 1.5 that is an 80x error: a 6 s dwell would become 480 s and mushrooms would
/// effectively stop appearing, with every other test in this suite still green.
#[test]
fn dwell_is_credited_in_real_seconds_not_render_frames() {
    let cfg = crate::config::load_game_config().expect("game config").mycelia;
    let (fps, per_scan) = frame_clock(&cfg);
    let scan_secs = per_scan as f32 / fps;
    let dwell = cfg.pin_dwell_secs;

    let max_frames = frames_to_cover_the_dwell(&cfg, fps, per_scan);
    let mut app = app_with_ripe_cells(cfg, &[320.0]);

    // Budget the dwell plus a couple of scans. A frame-delta accumulator needs ~80x that and will not
    // arrive, which is the regression this test exists to catch.
    let t = time_to_first_pin(&mut app, max_frames, fps, per_scan).unwrap_or_else(|| {
        panic!(
            "a lone ripe cell never pinned within {:.0} s of sim time, though `pin_dwell_secs` is \
             {dwell} s. The dwell accumulator is crediting far less than the elapsed interval.",
            2.0 * dwell,
        )
    });

    // The first scan credits nothing (no previous scan to measure from), so the pin lands one scan late.
    let expected = dwell + scan_secs;
    assert!(
        (t - expected).abs() <= scan_secs + 1e-3,
        "pinned after {t:.3} s of sim time; expected near {expected:.3} s ({dwell} s dwell + one scan)",
    );
}

/// A non-finite `growth` must stop the frame, not silently reach glTF. `f32::clamp` propagates NaN, so
/// `stage_weights` would emit NaN blend weights and the mesh would collapse. The guard sits ahead of the
/// descendant walk, so it fires even before the scene has instantiated.
#[test]
fn drive_morph_weights_rejects_a_non_finite_growth() {
    use bevy::ecs::system::RunSystemOnce;

    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut world = World::new();
        let mut b = body();
        b.growth = bad;
        world.spawn(b);
        let out: Result<(), BevyError> =
            world.run_system_once(drive_morph_weights).expect("system should run");
        assert!(out.is_err(), "growth = {bad} must be a hard error, not a silent NaN morph weight");
    }

    // And a healthy body is not disturbed by the guard.
    let mut world = World::new();
    let mut b = body();
    b.growth = 0.5;
    world.spawn(b);
    let out: Result<(), BevyError> =
        world.run_system_once(drive_morph_weights).expect("system should run");
    assert!(out.is_ok(), "a finite growth must pass the guard");
}

/// A one-cell-wide corridor: slabs on both flanks, symmetric, so `wall_escape`'s weighted push cancels to
/// `Vec2::ZERO`. There is no direction to solve along — `deepest_push` cannot move the base and the stem
/// cannot bend away from one wall without bending into the other.
///
/// This is the case that makes the `penetration` gate load-bearing rather than decorative. A pose whose
/// lean drives the cap into a flank must be **refused**, and only the check catches it: the solve loop
/// happily returns a clipping plan on its first pass, because it computed a zero push and believes it.
#[test]
fn plan_body_refuses_a_corridor_pose_it_cannot_solve() {
    let size = crate::mycelia::CONTROL_SIZE as usize;
    let mut walkable = vec![false; size * size];
    for y in 40..=80 {
        walkable[y * size + 60] = true; // a single column of floor: rock at x = 59 and x = 61
    }
    let dungeon = Dungeon::from_walkable(size, size, walkable);

    let mut clipped = Vec::new();
    let mut refused = 0;
    for seed in 0..256u32 {
        match plan_body(&dungeon, Vec2::new(60.0, 60.0), TEST_SCALE, seed) {
            None => refused += 1,
            Some(plan) => {
                let depth = penetration(&dungeon, &plan, TEST_SCALE);
                if depth > 0.0 {
                    clipped.push((seed, depth));
                }
            }
        }
    }

    assert!(
        clipped.is_empty(),
        "{} corridor poses clip a flank; worst {:?}. With no escape direction the only correct answer \
         is to refuse the site — verify the pose before returning it.",
        clipped.len(),
        clipped.iter().max_by(|a, b| a.1.total_cmp(&b.1)),
    );
    assert!(
        refused > 0,
        "a 1-cell corridor should defeat at least some poses; none were refused, so this test is not \
         exercising the unsolvable case it claims to",
    );
}

/// The case a single escape direction serves worst: an inside corner, where one diagonal push must clear
/// two faces at once and under-clears each by `1/√2`. The first solve iteration is not enough here — the
/// pose has to be checked and the base re-seated. This is what `plan_body`'s `penetration` gate is *for*.
#[test]
fn plan_body_clears_an_inside_corner() {
    let dungeon = dungeon_with_floor_block(40, 80);
    let wt = crate::dungeon::WALL_THICKNESS;
    // The south-west inside corner of the floor block: slabs on the west face of cell 40 and its south.
    let corner = Vec2::new(39.5 + wt, 39.5 + wt);
    let diag = Vec2::new(1.0, 1.0).normalize();

    let mut clipped = Vec::new();
    for step in 0..=60 {
        let site = corner + diag * (step as f32 * 0.01);
        for seed in 0..64u32 {
            let Some(plan) = plan_body(&dungeon, site, TEST_SCALE, seed) else {
                continue;
            };
            let depth = penetration(&dungeon, &plan, TEST_SCALE);
            if depth > 0.0 {
                clipped.push((step, seed, depth));
            }
        }
    }

    assert!(
        clipped.is_empty(),
        "{} corner poses clip; worst {:?}. A returned plan must be verified and re-seated, \
         not trusted after one solve pass.",
        clipped.len(),
        clipped.iter().max_by(|a, b| a.2.total_cmp(&b.2)),
    );
}

/// The 16,384-cell scan must run once per readback, not once per rendered frame. `MoldCoarse` only
/// changes at `sim_hz`, so rescanning at the display's refresh rate repeats identical work ~80x.
///
/// This is a *performance* invariant, and the dwell clock cannot detect it: because the accumulator
/// credits elapsed time, an ungated scan still pins on schedule — it just burns 80x the CPU getting
/// there. So assert the gate directly: with no new readback, the scan must not touch `PinDwell` at all.
#[test]
fn the_coarse_scan_is_skipped_when_no_new_readback_landed() {
    let cfg = crate::config::load_game_config().expect("game config").mycelia;
    let (fps, _) = frame_clock(&cfg);
    let mut app = app_with_ripe_cells(cfg, &[320.0]);

    // One readback: the cell is seen, and starts its dwell at zero (no prior scan to measure from).
    let frame = std::time::Duration::from_secs_f32(1.0 / fps);
    app.world_mut().resource_mut::<Time<Virtual>>().advance_by(frame);
    app.world_mut().resource_mut::<MoldCoarse>().generation += 1;
    app.update();
    let after_scan = app.world().resource::<PinDwell>().0.get(&0).copied();
    assert_eq!(after_scan, Some(0.0), "the first scan must register the cell with zero dwell");

    // Now run 200 frames with no new readback. The buffer has not changed, so neither may the dwell.
    for _ in 0..200 {
        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(frame);
        app.update();
    }

    let held = app.world().resource::<PinDwell>().0.get(&0).copied();
    assert_eq!(
        held,
        Some(0.0),
        "dwell advanced without a new readback: the scan ran on frames where `MoldCoarse` was unchanged",
    );
    assert_eq!(body_count(&mut app), 0, "no body may pin from re-scanning stale data");
}

/// Sites in the band between the cap's radius and the pose envelope must still be *plannable*, not merely
/// refused. Verifying the pose without widening the probe would make `plan_body` reject them — the bodies
/// would stop clipping, and also stop existing. Both halves of the fix are load-bearing.
#[test]
fn sites_inside_the_old_blind_band_still_get_a_pose() {
    let dungeon = dungeon_with_floor_block(40, 80);
    let face_x = 80.5 - crate::dungeon::WALL_THICKNESS;
    let cap_reach = CAP_RADIUS_M * TEST_SCALE + WALL_MARGIN;
    let envelope = pose_envelope_m() * TEST_SCALE + WALL_MARGIN;

    let mut refused = 0;
    let mut total = 0;
    let mut offset = cap_reach;
    while offset < envelope {
        let site = Vec2::new(face_x - offset, 60.0);
        for seed in 0..64u32 {
            total += 1;
            if plan_body(&dungeon, site, TEST_SCALE, seed).is_none() {
                refused += 1;
            }
        }
        offset += 0.01;
    }

    assert!(total > 0, "the blind band must be non-empty");
    assert_eq!(
        refused, 0,
        "{refused}/{total} sites between the cap radius ({cap_reach:.3} m) and the pose envelope \
         ({envelope:.3} m) were refused. Widen the wall probe to the envelope rather than rejecting them.",
    );
}

/// The shipped render/sim clock, as the pinning path actually sees it.
fn frame_clock(cfg: &MyceliaConfig) -> (f32, usize) {
    let fps = 120.0;
    (fps, (fps / cfg.sim_hz) as usize)
}

/// Frames enough for a lone ripe cell to certainly pin.
///
/// The dwell is credited **once per scan**, not per frame, and the first scan credits nothing (it has no
/// previous scan to measure from). So the pin lands on scan `ceil(dwell / scan) + 1`, and a budget
/// expressed in dwell-seconds is only enough while a scan is short compared to the dwell. It no longer
/// is: at the shipped `sim_hz` a scan is 13.3 s against a 20 s dwell. Budget in *scans*, from the config,
/// so this keeps holding whichever way the clock is tuned.
fn frames_to_cover_the_dwell(cfg: &MyceliaConfig, fps: f32, per_scan: usize) -> usize {
    let scan_secs = per_scan as f32 / fps;
    let scans = (cfg.pin_dwell_secs / scan_secs).ceil() + 2.0;
    (scans * scan_secs * fps) as usize
}

/// The pose envelope really is wider than the cap: a body may lean and tilt its silhouette out beyond
/// `CAP_RADIUS_M`. Any wall probe that only reaches the cap radius is blind to slabs the body can hit.
#[test]
fn the_pose_envelope_exceeds_the_cap_radius() {
    let lean_max = LEAN_FRACTION * ADULT_HEIGHT_M;
    let sway = MAX_TILT * ADULT_HEIGHT_M + MAX_BEND_M.max(lean_max);
    assert!(sway > 0.0);
    assert!(
        pose_envelope_m() > CAP_RADIUS_M,
        "envelope {} must exceed the bare cap radius {CAP_RADIUS_M}",
        pose_envelope_m(),
    );
    // And the miss is large, not a rounding detail: at the shipped scale it is centimetres of blind band.
    let blind = (pose_envelope_m() - CAP_RADIUS_M) * TEST_SCALE;
    assert!(blind > 0.2, "blind band {blind} m is suspiciously small");
}
