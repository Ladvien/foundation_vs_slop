//! Tests for `habitat.rs` — moved out of the module file so the implementation reads without
//! scrolling past them. Still a child module of the same parent, so `use super::*` resolves
//! exactly as before; this is a pure move.

use super::*;
use crate::placement::ir::{PropertyBag, Rect2, Region};

/// Room footprint used by [`fixture`], in cells.
const ROOM: i32 = 10;
/// Cells of corridor in the fixture's single run (edge 0).
const RUN: std::ops::Range<usize> = 15..75;

/// A 192² dungeon: twelve 10×10 rooms on a 4×3 lattice, plus one corridor run (edge 0) clear of them all.
///
/// Twelve rooms rather than two, deliberately: the greedy's granularity is one whole room, so a fixture
/// with a couple of huge rooms cannot land anywhere near a 25% target and would tell us nothing about the
/// selection. The shipped dungeon has ~24 rooms.
fn fixture() -> Dungeon {
    let n = CONTROL_SIZE as usize;
    let mut walkable = vec![false; n * n];
    let mut corridor_of = vec![u32::MAX; n * n];

    // Alternating damp/dry so the susceptibility ordering has something to bite on.
    const TAGS: [&str; 4] = ["bathroom", "office", "kitchen", "bedroom"];
    let mut regions = Vec::new();
    for (i, (gx, gy)) in (0..3).flat_map(|y| (0..4).map(move |x| (x, y))).enumerate() {
        let (x0, y0) = (10 + gx * 20, 10 + gy * 20);
        for y in y0..y0 + ROOM {
            for x in x0..x0 + ROOM {
                walkable[y as usize * n + x as usize] = true;
            }
        }
        regions.push(Region {
            id: i as u32,
            rect: Rect2 { min: [x0, y0], max: [x0 + ROOM, y0 + ROOM] },
            openings: Vec::new(),
            adjacency: Vec::new(),
            props: PropertyBag { tags: vec!["room".into(), TAGS[i % TAGS.len()].into()] },
        });
    }

    // One corridor run along y = 75, below every room (rooms end at y = 60).
    for x in RUN {
        walkable[75 * n + x] = true;
        corridor_of[75 * n + x] = 0;
    }
    Dungeon::from_parts(n, n, walkable, regions, corridor_of)
}

/// How many of the fixture's rooms ended up with any habitat at all?
fn infested_rooms(d: &Dungeon, bytes: &[u8], field: usize) -> usize {
    d.regions
        .iter()
        .filter(|r| {
            (r.rect.min[1]..r.rect.max[1]).any(|y| {
                (r.rect.min[0]..r.rect.max[0]).any(|x| at_cell(bytes, field, x, y) > 0)
            })
        })
        .count()
}

/// A config with a small field so the tests stay fast, and no corridor infestation unless a test wants it.
fn cfg(field_size: u32, coverage: f32, corridor_chance: f32) -> MyceliaConfig {
    let mut c = crate::mycelia::tests::valid();
    c.field_size = field_size;
    c.habitat_coverage = coverage;
    c.corridor_infest_chance = corridor_chance;
    c
}

/// Read the mask byte at the field texel containing cell `(x, y)`'s centre.
fn at_cell(bytes: &[u8], field: usize, x: i32, y: i32) -> u8 {
    let cells_per_texel = CONTROL_SIZE as f32 / field as f32;
    let tx = ((x as f32 + 0.5) / cells_per_texel) as usize;
    let ty = ((y as f32 + 0.5) / cells_per_texel) as usize;
    bytes[ty * field + tx]
}

fn covered(bytes: &[u8], field: usize, x: i32, y: i32) -> bool {
    at_cell(bytes, field, x, y) >= (COVERED * 255.0).round() as u8
}

/// Measured coverage of a built mask, as a fraction of walkable floor.
fn coverage(d: &Dungeon, bytes: &[u8], field: usize) -> f32 {
    let floor = d.floor_cells().count();
    let cov = d.floor_cells().filter(|c| covered(bytes, field, c.x, c.y)).count();
    cov as f32 / floor as f32
}

/// The whole point: a quarter of the floor, not all of it.
#[test]
fn coverage_lands_near_the_target() {
    let d = fixture();
    let c = cfg(768, 0.25, 0.0);
    let bytes = build(&d, &c).expect("fixture builds");
    let achieved = coverage(&d, &bytes, 768);
    // The greedy's granularity is one room, so it cannot do better than half a room's worth of error.
    assert!((achieved - 0.25).abs() <= 0.06, "coverage {achieved} should be near 0.25");
    assert!(achieved > 0.0, "some floor must be infested");
}

/// The aesthetic requirement, made testable: most rooms are untouched, so walking into a moldy one is an
/// event. If a future falloff change thins the patches, the greedy will start infesting every room to
/// spend its budget — and this fails, rather than the mold quietly going back to coating everything.
#[test]
fn most_rooms_are_left_completely_clean() {
    let d = fixture();
    let bytes = build(&d, &cfg(768, 0.25, 0.0)).expect("builds");
    let dirty = infested_rooms(&d, &bytes, 768);
    assert!(dirty >= 1, "at least one room must rot");
    assert!(
        dirty * 2 < d.regions.len(),
        "{dirty} of {} rooms infested — a subset must stay clean",
        d.regions.len()
    );
}

/// An infested room is *heavily* patched, not lightly speckled — the other half of the contrast.
#[test]
fn an_infested_room_is_heavily_patched() {
    let d = fixture();
    let bytes = build(&d, &cfg(768, 0.25, 0.0)).expect("builds");
    let worst = d
        .regions
        .iter()
        .map(|r| {
            (r.rect.min[1]..r.rect.max[1])
                .flat_map(|y| (r.rect.min[0]..r.rect.max[0]).map(move |x| (x, y)))
                .filter(|&(x, y)| covered(&bytes, 768, x, y))
                .count()
        })
        .max()
        .unwrap_or(0);
    let area = (ROOM * ROOM) as usize;
    assert!(
        worst * 100 / area >= 40,
        "the most-infested room covers only {worst}/{area} of its floor; patches are too thin"
    );
}

/// Rock is never habitat — the rasterizer clips to owned floor, and nothing bleeds outside a room.
#[test]
fn rock_is_never_habitat() {
    let d = fixture();
    let c = cfg(768, 0.60, 0.0);
    let bytes = build(&d, &c).expect("builds");
    let n = CONTROL_SIZE as usize;
    for i in 0..n * n {
        let (x, y) = ((i % n) as i32, (i / n) as i32);
        if !d.is_floor(IVec2::new(x, y)) && at_cell(&bytes, 768, x, y) != 0 {
            panic!("rock cell ({x},{y}) has habitat {}", at_cell(&bytes, 768, x, y));
        }
    }
}

/// A corridor run is infested end to end, or not at all. Never half.
#[test]
fn corridor_runs_are_all_or_nothing() {
    let d = fixture();
    // Chance 1.0 → the single run must be solid across its whole length.
    let bytes = build(&d, &cfg(768, 0.9, 1.0)).expect("builds");
    for x in RUN {
        assert!(covered(&bytes, 768, x as i32, 75), "corridor cell ({x},75) must be infested");
    }
    // Chance 0.0 → not one cell of it.
    let bytes = build(&d, &cfg(768, 0.9, 0.0)).expect("builds");
    for x in RUN {
        assert_eq!(at_cell(&bytes, 768, x as i32, 75), 0, "corridor cell ({x},75) must be bare");
    }
}

/// Same seed, same mask — the colony layout is a pure function of the dungeon.
#[test]
fn build_is_deterministic() {
    let d = fixture();
    let c = cfg(384, 0.25, 0.5);
    assert_eq!(build(&d, &c).expect("a"), build(&d, &c).expect("b"));
}

/// The damp table must actually order rooms: a bathroom outscores an office at equal geometry.
#[test]
fn damp_rooms_outscore_dry_ones() {
    let c = cfg(384, 0.25, 0.0);
    let bath = hash01(HABITAT_SEED ^ splitmix64(0) ^ SCORE_SALT)
        * c.damp_weight(&["room".into(), "bathroom".into()]).expect("listed");
    let office = hash01(HABITAT_SEED ^ splitmix64(0) ^ SCORE_SALT)
        * c.damp_weight(&["room".into(), "office".into()]).expect("listed");
    assert!(bath > office, "bathroom ({bath}) must outrank office ({office}) at equal hash");
}

/// An unlisted room type is a loud error, never a silent middling weight.
#[test]
fn unlisted_room_type_fails_loudly() {
    let c = cfg(384, 0.25, 0.0);
    let err = c.damp_weight(&["room".into(), "dungeon_of_doom".into()]).expect_err("unlisted");
    assert!(err.contains("damp_weights"), "error should name the table: {err}");
}

/// Overshooting the target is a usable colony, so it returns `Ok` and reports the truth.
#[test]
fn corridor_overshoot_is_ok_not_err() {
    let d = fixture();
    // Tiny target, every run infested: the corridors alone blow the budget.
    let bytes = build(&d, &cfg(384, 0.01, 1.0)).expect("overshoot is still a usable mask");
    for x in RUN {
        assert!(covered(&bytes, 384, x as i32, 75), "the run is still fully infested");
    }
    assert!(coverage(&d, &bytes, 384) > 0.01, "the report must admit the overshoot");
}

/// A dungeon with no floor is a generation bug, not something to paper over.
#[test]
fn no_floor_fails_loudly() {
    let n = CONTROL_SIZE as usize;
    let d = Dungeon::from_parts(n, n, vec![false; n * n], Vec::new(), vec![u32::MAX; n * n]);
    let err = build(&d, &cfg(384, 0.25, 0.0)).expect_err("no floor must error");
    assert!(err.contains("no walkable floor"), "got: {err}");
}

/// A patch must be SOLID across its core, not a cone that fades from the centre.
///
/// The regression this pins: with a linear `1 - d/r` falloff the value crosses 0.5 at `d = r/2`, so every
/// patch covered a quarter of its nominal area, every room needed infesting to meet the budget, and no
/// room was ever clean. Here the value must still be ~1 at 60% of the radius and dead by the radius.
#[test]
fn a_patch_has_a_solid_core_and_a_soft_rim() {
    let mut c = crate::mycelia::tests::valid();
    c.edge_noise_amp = 0.0; // isolate the radial profile from the border noise
    let n = Nucleus { x: 0.0, y: 0.0, radius: 10.0 };

    assert!(nucleus_value(&n, 0.0, 0.0, &c, 1) >= 0.999, "the centre must be solid");
    assert!(nucleus_value(&n, 6.0, 0.0, &c, 1) >= 0.999, "60% of the radius must still be solid");
    let rim = nucleus_value(&n, 8.5, 0.0, &c, 1);
    assert!((0.05..0.95).contains(&rim), "the rim must be a ramp, got {rim}");
    assert_eq!(nucleus_value(&n, 10.0, 0.0, &c, 1), 0.0, "nothing at the nominal radius");

    // ...and the covered radius (value >= COVERED) must be most of `r`, not half of it.
    assert!(nucleus_value(&n, 8.0, 0.0, &c, 1) >= COVERED, "80% of the radius must count as covered");
}

/// The rasterizer walks `nucleus_reach`; if that bound understates the contour, patches get square edges.
#[test]
fn nucleus_reach_bounds_the_noisy_contour() {
    let c = crate::mycelia::tests::valid();
    let n = Nucleus { x: 0.0, y: 0.0, radius: 6.0 };
    let reach = nucleus_reach(&n, &c);
    // Sample a ring just outside the claimed reach; nothing may be alive there, whatever the noise does.
    for i in 0..720 {
        let a = i as f32 * std::f32::consts::TAU / 720.0;
        let (x, y) = ((reach + 0.01) * a.cos(), (reach + 0.01) * a.sin());
        assert_eq!(nucleus_value(&n, x, y, &c, 7), 0.0, "value beyond reach at angle {a}");
    }
}

/// The three design rules, asserted against the **shipped** config and the **shipped** dungeon seed.
///
/// The other tests here prove the algorithm behaves on a synthetic fixture. This one proves the numbers
/// actually in `assets/config/config.ron` produce the level the design asked for — a different claim, and
/// the one that broke first.
///
/// The three rules pull against each other, because this dungeon is only about a THIRD room floor by area.
/// Fund the coverage quota out of rooms alone and nearly all of them must rot (measured: 17 of 24 at
/// `coverage 0.25 / corridor 0.12`). Fund it out of corridors instead and the mold stops being a room
/// phenomenon (measured: 57% of it in halls at `0.25 / 0.30`). Only a lower `habitat_coverage` satisfies
/// all three at once. So assert all three together — tuning one dial otherwise breaks another in silence.
#[test]
fn the_shipped_config_delivers_the_intended_level() {
    let game = crate::config::load_game_config().expect("the shipped config must load and validate");
    let d = Dungeon::generate(&game.dungeon).expect("the shipped seed must generate");
    let bytes = build(&d, &game.mycelia).expect("the shipped dungeon must be habitable");
    let field = game.mycelia.field_size as usize;

    // 1. It covers about what it was asked to cover.
    let achieved = coverage(&d, &bytes, field);
    assert!(
        (achieved - game.mycelia.habitat_coverage).abs() <= 0.03,
        "shipped coverage {achieved} misses the configured {}",
        game.mycelia.habitat_coverage
    );

    // 2. Most rooms stay clean, so walking into a moldy one is an event.
    let dirty = infested_rooms(&d, &bytes, field);
    let total = d.regions.len();
    assert!(
        dirty * 2 <= total,
        "{dirty} of {total} rooms infested — most rooms must stay clean. Lower \
         mycelia.habitat_coverage, or raise corridor_infest_chance and accept mold in the halls."
    );
    assert!(dirty >= 2, "only {dirty} rooms rot; the mold should still be a presence");

    // 3. The mold is a ROOM phenomenon. This is the rule the corridor dial quietly destroys: raise
    //    `corridor_infest_chance` to fund a high coverage target and the colony moves into the halls,
    //    while rules 1 and 2 both still pass.
    let n = CONTROL_SIZE as usize;
    let cell = |i: usize| IVec2::new((i % n) as i32, (i / n) as i32);
    let (mut in_rooms, mut in_halls) = (0usize, 0usize);
    for i in 0..n * n {
        let c = cell(i);
        if d.is_floor(c) && covered(&bytes, field, c.x, c.y) {
            if d.is_corridor(c) {
                in_halls += 1;
            } else {
                in_rooms += 1;
            }
        }
    }
    let mold = in_rooms + in_halls;
    assert!(
        in_rooms * 100 / mold.max(1) >= 60,
        "only {in_rooms} of {mold} mold cells are in rooms; the mold must live in rooms, not halls"
    );
}

/// The noise is deterministic and stays in range — everything downstream assumes both.
#[test]
fn value_noise_is_deterministic_and_bounded() {
    for i in 0..500 {
        let (x, y) = (i as f32 * 0.37, i as f32 * -0.11);
        let a = fbm(x, y, 0xABCD);
        let b = fbm(x, y, 0xABCD);
        assert_eq!(a, b, "fbm must be a pure function");
        assert!((0.0..1.0).contains(&a), "fbm out of range at ({x},{y}): {a}");
    }
    // Two different seeds must not agree everywhere (a constant would silently disable the raggedness).
    let differs = (0..64).any(|i| {
        let (x, y) = (i as f32 * 0.7, 3.0);
        (fbm(x, y, 1) - fbm(x, y, 2)).abs() > 1e-6
    });
    assert!(differs, "fbm must depend on its seed");
}
