//! Acceptance tests for the shared anomaly placement pass.
//!
//! The `_tests.rs` NAME is load-bearing (the `mycelia/fruit_tests.rs` idiom): `tests/panic_budget.rs`
//! walks files independently, so the parent module's `#[cfg(test)]` gate is invisible to it — the
//! filename is what tells the scanner the panics in here are test expectations, not shipped crashes.

use super::*;
use crate::rng::seeded;

/// A plain open grid of `side²` floor cells — the shape that made the old raster scans fail, since
/// "first cell past a radius from the centre" is always a corner.
fn open_floor(side: i32) -> Vec<IVec2> {
    (0..side).flat_map(|y| (0..side).map(move |x| IVec2::new(x, y))).collect()
}

/// Spawn at the grid's centre, matching `Dungeon::spawn` (the kept site nearest the level centre) —
/// which is precisely what made "far from spawn, in raster order" mean "in the corner".
fn centre(side: i32) -> IVec2 {
    IVec2::new(side / 2, side / 2)
}

fn rule(key: &'static str, count: usize, min_from_spawn: f32) -> AnomalyRule {
    AnomalyRule { key, count, min_from_spawn }
}

/// The headline regression: **no two anomalies, of any species, land within `separation`**.
///
/// This is the property the five independent raster scans could not have: each tracked spacing only
/// against its own kind, so a bear and a bloom and the boss could occupy the same corner without any
/// of them noticing. Asserted across every cross-species pair, not just within a species.
#[test]
fn no_two_anomalies_of_any_species_are_closer_than_the_separation() {
    let side = 96;
    let floor = open_floor(side);
    let spawn = centre(side);
    let rules = [
        rule("a", 3, 24.0),
        rule("b", 1, 16.0),
        rule("c", 1, 18.0),
        rule("d", 1, 4.0),
        rule("e", 4, 8.0),
    ];
    let sep = 12.0;
    let mut rng = seeded(7);
    let (sites, short) = solve_anomaly_sites(&floor, spawn, &rules, sep, &mut rng);
    assert!(short.is_empty(), "every species should place fully on an open 96² map, got {short:?}");

    let all = sites.all_sites();
    assert_eq!(all.len(), 10, "3+1+1+1+4 sites");
    for (i, a) in all.iter().enumerate() {
        for b in all.iter().skip(i + 1) {
            let dist = (*a - *b).as_vec2().length();
            assert!(
                dist >= sep,
                "two anomalies at {a} and {b} are {dist:.2} apart, under the {sep} separation"
            );
        }
    }
}

/// Every site honours its own species' minimum distance from the squad's entry.
#[test]
fn every_site_clears_its_species_minimum_from_spawn() {
    let side = 96;
    let floor = open_floor(side);
    let spawn = centre(side);
    let rules = [rule("far", 2, 30.0), rule("near", 2, 5.0)];
    let mut rng = seeded(11);
    let (sites, _) = solve_anomaly_sites(&floor, spawn, &rules, 10.0, &mut rng);
    for r in &rules {
        for c in sites.get(r.key) {
            let dist = (*c - spawn).as_vec2().length();
            assert!(
                dist >= r.min_from_spawn,
                "{} site at {c} is {dist:.2} from spawn, under its {} minimum",
                r.key,
                r.min_from_spawn
            );
        }
    }
}

/// **The corner bug, pinned.** The old scans put every species in the low-x/low-y corner; the whole
/// point of best-candidate selection is that the population spreads over the map instead.
///
/// Measured as: the sites' bounding box must span a real fraction of the reachable area, and their
/// centroid must not sit in one quadrant. A raster-order placement fails both — its bounding box is a
/// small corner patch and its centroid is jammed against the origin.
#[test]
fn sites_spread_across_the_map_rather_than_clustering_in_a_corner() {
    let side = 96;
    let floor = open_floor(side);
    let spawn = centre(side);
    let rules = [rule("a", 3, 20.0), rule("b", 3, 20.0), rule("c", 3, 20.0)];
    let mut rng = seeded(3);
    let (sites, _) = solve_anomaly_sites(&floor, spawn, &rules, 12.0, &mut rng);
    let all = sites.all_sites();
    assert!(all.len() >= 6, "need several sites to talk about spread, got {}", all.len());

    let (minx, maxx) = (
        all.iter().map(|c| c.x).min().unwrap_or(0),
        all.iter().map(|c| c.x).max().unwrap_or(0),
    );
    let (miny, maxy) = (
        all.iter().map(|c| c.y).min().unwrap_or(0),
        all.iter().map(|c| c.y).max().unwrap_or(0),
    );
    let span_x = (maxx - minx) as f32 / side as f32;
    let span_y = (maxy - miny) as f32 / side as f32;
    assert!(
        span_x > 0.4 && span_y > 0.4,
        "sites span only {span_x:.2}x{span_y:.2} of the map — that is clustering, the bug this replaced"
    );

    // Centroid near the middle-ish, not pinned to a corner. Raster order lands this at ~(0.15, 0.15).
    let cx = all.iter().map(|c| c.x as f32).sum::<f32>() / all.len() as f32 / side as f32;
    let cy = all.iter().map(|c| c.y as f32).sum::<f32>() / all.len() as f32 / side as f32;
    assert!(
        (0.25..=0.75).contains(&cx) && (0.25..=0.75).contains(&cy),
        "site centroid at ({cx:.2}, {cy:.2}) of the map is corner-biased"
    );
}

/// Same seed, same sites — the pass is reproducible, and it does not read anything ECS-ordered.
#[test]
fn placement_is_deterministic_for_a_seed() {
    let side = 64;
    let floor = open_floor(side);
    let spawn = centre(side);
    let rules = [rule("a", 2, 12.0), rule("b", 2, 12.0)];
    let run = |seed: u64| {
        let mut rng = seeded(seed);
        solve_anomaly_sites(&floor, spawn, &rules, 10.0, &mut rng).0.all_sites()
    };
    assert_eq!(run(99), run(99), "same seed must give the same sites");
    assert_ne!(run(99), run(100), "different seeds must give different populations");
}

/// A separation the level cannot satisfy is reported, not silently relaxed.
///
/// The one-path rule: a species that quietly placed fewer (or ignored its spacing to fit) is how the
/// original bug hid for so long. The shortfall comes back to the caller, which warns with specifics.
#[test]
fn an_unsatisfiable_separation_is_reported_rather_than_relaxed() {
    let side = 32;
    let floor = open_floor(side);
    let spawn = centre(side);
    // Ten sites at 40 tiles apart cannot fit on a 32-tile map.
    let rules = [rule("greedy", 10, 0.0)];
    let mut rng = seeded(5);
    let (sites, short) = solve_anomaly_sites(&floor, spawn, &rules, 40.0, &mut rng);
    assert_eq!(short.len(), 1, "the shortfall must be reported");
    let (key, placed, wanted) = short[0];
    assert_eq!((key, wanted), ("greedy", 10));
    assert!(placed < 10, "it cannot have placed all ten");
    // Whatever it did place still honours the rule — it degrades in COUNT, never in spacing.
    let all = sites.all_sites();
    for (i, a) in all.iter().enumerate() {
        for b in all.iter().skip(i + 1) {
            assert!(
                (*a - *b).as_vec2().length() >= 40.0,
                "spacing was relaxed to fit the count — that is the fallback this forbids"
            );
        }
    }
}
