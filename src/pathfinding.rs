//! Grid pathfinding for squad movement: 8-connected A\* with a no-corner-cutting rule, plus a
//! line-of-sight string-pull to straighten the staircase into clean waypoints.
//!
//! A\* is the correct, sufficient baseline here — the map is ≤147² and paths are computed only on
//! a move order, not per frame. The SOTA upgrade for larger maps / heavier query loads is **Jump
//! Point Search** (Harabor & Grastien, "Online Graph Pruning for Pathfinding on Grid Maps", AAAI
//! 2011, DOI 10.1609/aaai.v25i1.7994) or **Subgoal Graphs** (Uras, Koenig & Hernandez, ICAPS 2013,
//! DOI 10.1609/icaps.v23i1.13568), both drop-in over this same `is_floor` grid. The diagonal
//! no-corner-cutting rule below is exactly the one Uras et al. state ("move diagonally only if both
//! associated cardinal directions are unblocked") and matches the collision resolver's void-corner
//! rejection in `dungeon`.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use bevy::prelude::*;

use crate::dungeon::Dungeon;

/// Integer step costs (×10) so the open set orders on `i32` — no float `Ord` headaches.
const CARDINAL: i32 = 10;
const DIAGONAL: i32 = 14; // ≈ √2 · 10

const NEIGHBORS: [(i32, i32, i32); 8] = [
    (1, 0, CARDINAL),
    (-1, 0, CARDINAL),
    (0, 1, CARDINAL),
    (0, -1, CARDINAL),
    (1, 1, DIAGONAL),
    (1, -1, DIAGONAL),
    (-1, 1, DIAGONAL),
    (-1, -1, DIAGONAL),
];

/// Open-set entry, ordered as a *min-heap* on `f` (so `BinaryHeap` pops the lowest estimate).
struct Node {
    f: i32,
    g: i32,
    cell: IVec2,
}
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.f == other.f
    }
}
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.cmp(&self.f) // reversed → min-heap
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Octile heuristic in the same ×10 integer units — admissible for 8-connected grids.
fn heuristic(a: IVec2, b: IVec2) -> i32 {
    let dx = (a.x - b.x).abs();
    let dy = (a.y - b.y).abs();
    DIAGONAL * dx.min(dy) + CARDINAL * (dx.max(dy) - dx.min(dy))
}

/// Shortest floor path from `start` to `goal` (inclusive of both), or `None` if unreachable. If
/// `goal` isn't a floor cell (e.g. the player clicked a wall/void), it snaps to the nearest floor.
pub fn find_path(dungeon: &Dungeon, start: IVec2, goal: IVec2) -> Option<Vec<IVec2>> {
    if !dungeon.is_floor(start) {
        return None;
    }
    let goal = if dungeon.is_floor(goal) {
        goal
    } else {
        nearest_floor(dungeon, goal)?
    };
    if start == goal {
        return Some(vec![start]);
    }

    let mut open = BinaryHeap::new();
    let mut g_score: HashMap<IVec2, i32> = HashMap::new();
    let mut came_from: HashMap<IVec2, IVec2> = HashMap::new();
    g_score.insert(start, 0);
    open.push(Node {
        f: heuristic(start, goal),
        g: 0,
        cell: start,
    });

    while let Some(node) = open.pop() {
        if node.cell == goal {
            return Some(reconstruct(&came_from, goal));
        }
        // Lazy-deletion: skip a stale entry we've since beaten.
        if node.g > *g_score.get(&node.cell).unwrap_or(&i32::MAX) {
            continue;
        }
        for (dx, dy, cost) in NEIGHBORS {
            let n = IVec2::new(node.cell.x + dx, node.cell.y + dy);
            if !dungeon.is_floor(n) {
                continue;
            }
            // No corner cutting: a diagonal is legal only if both shared orthogonal cells are floor.
            if dx != 0
                && dy != 0
                && (!dungeon.is_floor(IVec2::new(node.cell.x + dx, node.cell.y))
                    || !dungeon.is_floor(IVec2::new(node.cell.x, node.cell.y + dy)))
            {
                continue;
            }
            let tentative = node.g + cost;
            if tentative < *g_score.get(&n).unwrap_or(&i32::MAX) {
                g_score.insert(n, tentative);
                came_from.insert(n, node.cell);
                open.push(Node {
                    f: tentative + heuristic(n, goal),
                    g: tentative,
                    cell: n,
                });
            }
        }
    }
    None
}

/// Collapse a cell-by-cell path into waypoints with clear line-of-sight between consecutive ones,
/// so units walk straight diagonals instead of a jagged staircase (a simple string-pull).
pub fn smooth_path(dungeon: &Dungeon, path: &[IVec2]) -> Vec<IVec2> {
    if path.len() <= 2 {
        return path.to_vec();
    }
    let mut out = vec![path[0]];
    let mut i = 0;
    while i < path.len() - 1 {
        // Farthest node still in line-of-sight from `path[i]`.
        let mut j = path.len() - 1;
        while j > i + 1 && !dungeon.line_of_sight(path[i], path[j]) {
            j -= 1;
        }
        out.push(path[j]);
        i = j;
    }
    out
}

fn reconstruct(came_from: &HashMap<IVec2, IVec2>, goal: IVec2) -> Vec<IVec2> {
    let mut path = vec![goal];
    let mut cur = goal;
    while let Some(&prev) = came_from.get(&cur) {
        path.push(prev);
        cur = prev;
    }
    path.reverse();
    path
}

/// Nearest floor cell to `c` by an outward ring search (used when a move is ordered onto a
/// non-floor cell). Bounded so a click deep in the void fails gracefully rather than scanning forever.
fn nearest_floor(dungeon: &Dungeon, c: IVec2) -> Option<IVec2> {
    const MAX_RING: i32 = 8;
    for r in 1..=MAX_RING {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue; // ring perimeter only
                }
                let cell = IVec2::new(c.x + dx, c.y + dy);
                if dungeon.is_floor(cell) {
                    return Some(cell);
                }
            }
        }
    }
    None
}
