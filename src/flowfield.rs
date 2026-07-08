//! Flow-field global navigation for large groups: a single **integration field** (Dijkstra
//! cost-to-goal over the floor grid) plus a per-cell **flow vector** pointing down-gradient toward
//! the goal. One field is built per move command and shared (via `Arc`) by every unit in the
//! selection, so the global-navigation cost is O(cells) *once per command*, independent of how many
//! units follow it — this is what lets hundreds of units share a destination cheaply.
//!
//! This is the discrete realization of the potential-field idea from **Continuum Crowds** (Treuille,
//! Cooper & Popović, SIGGRAPH 2006, DOI 10.1145/1141911.1142008): a field that unifies global
//! navigation so agents "never get stuck in local minima" — on a connected floor component every
//! reachable cell has a strictly-descending path to the goal, so there are no dead ends to trap a
//! follower. The grid integration-field / flow-tile formulation follows Emerson, "Crowd Pathfinding
//! and Steering Using Flow Field Tiles" (Game AI Pro). Local collision avoidance between units is a
//! separate layer (see `orca`); this module only decides *where each cell wants to go*.

use bevy::prelude::*;

use crate::dungeon::Dungeon;
use crate::pathfind::{dijkstra_multi_source, UNREACHABLE};

/// Integer step costs (×10) so the integration field orders on `u32` — matches the octile costs the
/// old A\* used (`CARDINAL`/`DIAGONAL`) and the no-corner-cutting rule in `dungeon::line_of_sight`.
const CARDINAL: u32 = 10;
const DIAGONAL: u32 = 14; // ≈ √2 · 10

/// 8-connected neighbor offsets with their step cost. Diagonals are gated by the no-corner-cutting
/// rule at relaxation time (both shared orthogonal cells must be floor).
const NEIGHBORS: [(i32, i32, u32); 8] = [
    (1, 0, CARDINAL),
    (-1, 0, CARDINAL),
    (0, 1, CARDINAL),
    (0, -1, CARDINAL),
    (1, 1, DIAGONAL),
    (1, -1, DIAGONAL),
    (-1, 1, DIAGONAL),
    (-1, -1, DIAGONAL),
];

/// A goal-directed navigation field over the whole fine grid. Shared read-only by all units that
/// were given the same move command.
pub struct FlowField {
    width: usize,
    height: usize,
    goal: IVec2,
    /// Integration field: Dijkstra cost from each cell to `goal`; [`UNREACHABLE`] off-floor / unreachable.
    cost: Vec<u32>,
    /// Per-cell unit direction toward the goal (down the cost gradient); `Vec2::ZERO` at the goal
    /// and on unreachable cells.
    flow: Vec<Vec2>,
}

impl FlowField {
    #[inline]
    fn index(&self, c: IVec2) -> usize {
        crate::util::row_major(c, self.width)
    }

    /// Build the integration + flow field for a move toward `goal`. Returns `None` only if `goal`
    /// is not a floor cell — callers snap the raw click to floor first, so a `None` here is a genuine
    /// "there is nowhere to go" and should be surfaced, never silently swallowed.
    pub fn build(dungeon: &Dungeon, goal: IVec2) -> Option<FlowField> {
        Self::build_from(dungeon, &[goal])
    }

    /// Build a **multi-source** field: Dijkstra outward from *every* cell in `sources` at once (each
    /// seeded at cost 0), so every reachable cell flows toward its NEAREST source. Used for enemy
    /// pursuit — one field seeded from all unit cells lets every enemy path to the closest unit from
    /// a single O(cells) build. Non-floor sources are skipped; returns `None` if none is floor (so a
    /// single non-floor `goal` still yields `None`, preserving `build`'s contract).
    pub fn build_from(dungeon: &Dungeon, sources: &[IVec2]) -> Option<FlowField> {
        let width = dungeon.width;
        let height = dungeon.height;

        // Seed indices for every floor source; the first floor source also becomes `goal`. `goal` is only
        // meaningful for a single-source field; for multi-source it records the first source (enemies
        // steer by flow and never read `goal()`). `None` ⇒ no floor source at all.
        let mut goal: Option<IVec2> = None;
        let mut source_idx: Vec<usize> = Vec::new();
        for &s in sources {
            if !dungeon.is_floor(s) {
                continue;
            }
            source_idx.push(crate::util::row_major(s, width));
            goal.get_or_insert(s);
        }
        let goal = goal?;

        // Uniform-cost multi-source Dijkstra: exact cost from each cell to the nearest source. The 8-way
        // grid successor (with the no-corner-cutting diagonal rule, identical to the A\* rule and the
        // collision resolver's void-corner rule) is the only field-specific part.
        let cost = dijkstra_multi_source(width * height, source_idx, |node, relax| {
            let cell = IVec2::new((node % width) as i32, (node / width) as i32);
            for (dx, dy, step) in NEIGHBORS {
                let n = IVec2::new(cell.x + dx, cell.y + dy);
                if !dungeon.is_floor(n) {
                    continue;
                }
                if dx != 0
                    && dy != 0
                    && (!dungeon.is_floor(IVec2::new(cell.x + dx, cell.y))
                        || !dungeon.is_floor(IVec2::new(cell.x, cell.y + dy)))
                {
                    continue;
                }
                relax(crate::util::row_major(n, width), step);
            }
        });

        // Flow vectors: each cell points toward its lowest-cost admissible neighbor (steepest
        // descent of the integration field). The goal and unreachable cells stay `ZERO`.
        let mut flow = vec![Vec2::ZERO; width * height];
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let ci = crate::util::row_major(IVec2::new(x, y), width);
                if cost[ci] == UNREACHABLE || cost[ci] == 0 {
                    continue;
                }
                let mut best = cost[ci];
                let mut dir = Vec2::ZERO;
                for (dx, dy, _) in NEIGHBORS {
                    let n = IVec2::new(x + dx, y + dy);
                    if !dungeon.is_floor(n) {
                        continue;
                    }
                    if dx != 0
                        && dy != 0
                        && (!dungeon.is_floor(IVec2::new(x + dx, y))
                            || !dungeon.is_floor(IVec2::new(x, y + dy)))
                    {
                        continue;
                    }
                    let ni = crate::util::row_major(n, width);
                    if cost[ni] < best {
                        best = cost[ni];
                        dir = Vec2::new(dx as f32, dy as f32);
                    }
                }
                flow[ci] = dir.normalize_or_zero();
            }
        }

        Some(FlowField {
            width,
            height,
            goal,
            cost,
            flow,
        })
    }

    /// The goal cell this field points toward.
    #[inline]
    pub fn goal(&self) -> IVec2 {
        self.goal
    }

    /// Is `cell` a floor cell from which the goal is reachable through this field?
    pub fn reachable(&self, cell: IVec2) -> bool {
        if cell.x < 0
            || cell.y < 0
            || cell.x as usize >= self.width
            || cell.y as usize >= self.height
        {
            return false;
        }
        self.cost[self.index(cell)] != UNREACHABLE
    }

    /// Steering direction toward a look-ahead point on the current cell's *centerline*
    /// (`cell_center + flow · LOOKAHEAD`), not the bare gradient. The perpendicular component pulls a
    /// unit back onto the corridor centerline so its body threads narrow gaps instead of wedging its
    /// AABB into a wall corner; the along-flow component carries it forward. `Vec2::ZERO` at the goal.
    pub fn steer(&self, dungeon: &Dungeon, world_pos: Vec3) -> Vec2 {
        let cell = dungeon.world_to_cell(world_pos);
        if cell.x < 0
            || cell.y < 0
            || cell.x as usize >= self.width
            || cell.y as usize >= self.height
        {
            return Vec2::ZERO;
        }
        let flow = self.flow[self.index(cell)];
        if flow == Vec2::ZERO {
            return Vec2::ZERO; // goal / unreachable cell
        }
        const LOOKAHEAD: f32 = 1.0;
        let center = dungeon.cell_center(cell).xz();
        let target = center + flow * LOOKAHEAD;
        (target - world_pos.xz()).normalize_or_zero()
    }
}
