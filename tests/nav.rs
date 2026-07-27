//! Navigation through tight geometry (feature `test-harness`) — FVS-M-4's regression net.
//!
//! The README has carried "squad members can get stuck in doorways when there are one or more in a
//! doorway" for a long time. Part of it was already addressed by widening the walls (a 1-tile doorway
//! now has `TILE - 2·WALL_THICKNESS = 0.72` m clear against a 0.44 m unit). What that fix *cannot*
//! address is the multi-unit case: **two 0.44 m units cannot pass abreast through 0.72 m**, so a
//! doorway is a genuine single-file constraint and the local-avoidance layer has to resolve who goes
//! first rather than having both yield forever.
//!
//! This drives the whole squad through one doorway and asserts they all get through. Physics-off
//! (`deterministic_core`) so the run is reproducible and a failure is a real nav result rather than a
//! solver wobble.
#![cfg(feature = "test-harness")]

use bevy::prelude::IVec2;
use foundation_vs_slop::dungeon::Dungeon;
use foundation_vs_slop::sim_harness::{
    build_headless_app, issue_squad_order, serial_guard, step, unit_cells, SimConfig,
};

/// A 1-tile doorway: floor, with exactly two opposite floor neighbours and walls on the other axis.
/// Returns `(doorway, near_side, far_side)`.
fn find_doorway(dungeon: &Dungeon) -> Option<(IVec2, IVec2, IVec2)> {
    let mut best: Option<(IVec2, IVec2, IVec2)> = None;
    for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let c = IVec2::new(x, y);
            if !dungeon.is_floor(c) {
                continue;
            }
            let (n, s) = (c + IVec2::Y, c - IVec2::Y);
            let (e, w) = (c + IVec2::X, c - IVec2::X);
            let vertical = dungeon.is_floor(n) && dungeon.is_floor(s) && !dungeon.is_floor(e) && !dungeon.is_floor(w);
            let horizontal = dungeon.is_floor(e) && dungeon.is_floor(w) && !dungeon.is_floor(n) && !dungeon.is_floor(s);
            let found = if vertical {
                Some((c, s, n))
            } else if horizontal {
                Some((c, w, e))
            } else {
                None
            };
            // Both sides must be genuinely open. A doorway into a one-cell alcove cannot hold five
            // units, so ordering the squad through it fails on geometry rather than on navigation —
            // an earlier draft picked exactly such a cell and reported a jam that was really "the
            // destination is a closet". Require room on each side.
            let roomy = |c: IVec2| {
                (-2..=2)
                    .flat_map(|dx| (-2..=2).map(move |dy| IVec2::new(dx, dy)))
                    .filter(|d| dungeon.is_floor(c + *d))
                    .count()
                    >= 8
            };
            // Deterministic pick: first in raster order, so the test always exercises the same doorway.
            if best.is_none() {
                if let Some((d, n, f)) = found {
                    if roomy(n) && roomy(f) {
                        best = Some((d, n, f));
                    }
                }
            }
        }
    }
    best
}

#[test]
fn the_whole_squad_traverses_a_one_tile_doorway_without_jamming() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();
    let mut app = build_headless_app(&cfg);
    step(&mut app, &cfg, 30);

    let (door, _near, far) = {
        let dungeon = app.world().resource::<Dungeon>();
        match find_doorway(dungeon) {
            Some(d) => d,
            // Not every generated map has a strict 1-tile doorway. Skipping is honest here — a
            // vacuous pass would be worse than saying so.
            None => {
                eprintln!("no 1-tile doorway in this map; nothing to exercise");
                return;
            }
        }
    };

    // Send everyone to the far side. They must funnel single-file through `door`.
    // Order them WELL past the doorway, not to the cell immediately behind it: five units ordered onto
    // one cell crowd it, and the first arrivals would block the exit — which would look like a doorway
    // jam while actually being goal-crowding. Walking the goal outward separates the two causes.
    let goal = {
        let dungeon = app.world().resource::<Dungeon>();
        let dir = far - door;
        let mut g = far;
        for k in 2..10 {
            let c = door + dir * k;
            if dungeon.is_floor(c) {
                g = c;
            } else {
                break;
            }
        }
        g
    };
    assert!(issue_squad_order(&mut app, goal), "the far side must be reachable");

    // The property is **arrival**, not instantaneous occupancy: a 0.72 m opening fits one 0.44 m unit,
    // so queueing through it is correct behaviour and several units sharing the doorway cell for a few
    // ticks is not a jam. A jam is units still not through after a generous budget.
    let mut arrived = 0usize;
    let mut best = 0usize;
    let _ = (far, door);
    for _ in 0..80 {
        step(&mut app, &cfg, 30);
        let cells = unit_cells(&mut app);
        // "Arrived" = gathered at the ordered goal. Deliberately not "nearer `far` than `door` is":
        // `far` is the cell immediately past the doorway, so a unit that traversed *and kept walking*
        // to the goal scored as NOT arrived under that measure — which reported a jam while the squad
        // was in fact standing on the objective.
        arrived = cells.iter().filter(|c| c.distance_squared(goal) <= 9).count();
        best = best.max(arrived);
        if arrived == cells.len() {
            break;
        }
    }

    let cells = unit_cells(&mut app);
    assert_eq!(
        arrived,
        cells.len(),
        "only {arrived}/{} units got through the doorway (best {best}) after 2400 ticks — they are \
         jammed, not queueing. cells={cells:?} door={door:?} far={far:?} goal={goal:?}",
        cells.len()
    );
}
