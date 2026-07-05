//! Wall-capable surface navigation for the dimensional-crab swarm.
//!
//! The dungeon is a flat floor (Y=0) walled by thin, axis-aligned vertical slabs (Y 0→`WALL_HEIGHT`).
//! Units and the smiley boss navigate the 2D floor grid (`flowfield`), but crabs must also crawl *on*
//! the walls. This module lifts the floor-only flow field of `crate::flowfield` onto a 2.5D surface
//! manifold: a graph whose nodes are surface **patches** — one per floor cell and one per walled edge —
//! joined by edges wherever a crab can physically walk from one patch to an adjacent one (mount a wall
//! from the floor, crawl along a wall run, round a convex/concave corner, and drop back down).
//!
//! A single multi-source Dijkstra over that graph, seeded from the squad's cells, gives every patch a
//! cost-to-nearest-unit and a down-gradient "flow" neighbor — the discrete potential field of
//! **Continuum Crowds** (Treuille, Cooper & Popović, SIGGRAPH 2006, DOI 10.1145/1141911.1142008),
//! realized as integration-field/flow tiles (Emerson, "Crowd Pathfinding and Steering Using Flow Field
//! Tiles", Game AI Pro) — the same references `flowfield` uses, extended from floor to surfaces. Because
//! every wall patch links to its own floor cell and the floor is a connected component reaching the
//! squad, **every** patch (floor or wall) has a strictly-descending route home, so a crab anywhere on
//! any surface always flows toward the nearest unit with no local minima. One `Arc`-shared field serves
//! the whole swarm, so global navigation is O(patches) once per squad move, independent of crab count.
//!
//! Note on wall tops: the dungeon's walls are single-sided perimeter slabs (a wall exists only on a
//! floor↔non-floor edge, and opposing rooms are always separated by a full rock cell), so there is no
//! back-to-back thin wall a crab could crawl *over the top* to a far room. Wall patches therefore model
//! the full climbable face (Y 0→`WALL_HEIGHT`) but emit no top-crossing edges — the geometry admits none.

use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::prelude::*;

use crate::dungeon::{Dungeon, TILE_SIZE, WALL_HEIGHT, WALL_THICKNESS};
use crate::wfc::{E, N, S, W};

/// Slot 0 in the patch index is a cell's floor patch; slots 1..=4 are its N/E/S/W wall faces.
fn wall_slot(dir: usize) -> u8 {
    (dir + 1) as u8
}
const FLOOR_SLOT: u8 = 0;

/// Grid offset of a cardinal direction (matches `Dungeon::neighbor`, which is private).
fn dir_offset(dir: usize) -> IVec2 {
    match dir {
        N => IVec2::new(0, -1),
        E => IVec2::new(1, 0),
        S => IVec2::new(0, 1),
        W => IVec2::new(-1, 0),
        _ => IVec2::ZERO,
    }
}

/// The cardinal direction whose [`dir_offset`] equals `off`, if any (inverse of `dir_offset`).
fn offset_to_dir(off: IVec2) -> Option<usize> {
    match (off.x, off.y) {
        (0, -1) => Some(N),
        (1, 0) => Some(E),
        (0, 1) => Some(S),
        (-1, 0) => Some(W),
        _ => None,
    }
}

/// The two grid offsets a wall face runs along (perpendicular to its normal). E/W faces run along Z
/// (grid-y); N/S faces run along X (grid-x).
fn run_offsets(dir: usize) -> [IVec2; 2] {
    match dir {
        E | W => [IVec2::new(0, 1), IVec2::new(0, -1)],
        _ => [IVec2::new(1, 0), IVec2::new(-1, 0)],
    }
}

/// Which surface a patch is: a floor cell, or one walled edge of a cell.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PatchKind {
    Floor(IVec2),
    Wall(IVec2, usize),
}

/// One axis-aligned rectangular surface a crab can stand on. `center`/`normal`/`tan_u`/`tan_v` define
/// its plane and in-plane axes; `half` is the (u,v) half-extent. Floor: normal +Y, tan_u +X, tan_v +Z.
/// Wall: normal is the inward face normal (±X/±Z), tan_u the horizontal run, tan_v +Y (up the face).
pub struct Patch {
    pub kind: PatchKind,
    pub center: Vec3,
    pub normal: Vec3,
    pub tan_u: Vec3,
    pub tan_v: Vec3,
    pub half: Vec2,
}

/// A directed step in the surface graph: to a neighbor patch, its integer cost (≈10·world distance,
/// octile-scaled like `flowfield`), and the world-space `gate` — the shared-boundary midpoint a crab
/// steers toward to make the crossing.
struct Adj {
    to: u32,
    cost: u32,
    gate: Vec3,
}

/// The static surface-navigation graph, built once from the (never-regenerated) dungeon.
#[derive(Resource)]
pub struct SurfaceGraph {
    patches: Vec<Patch>,
    index: HashMap<(IVec2, u8), u32>,
    adj: Vec<Vec<Adj>>,
}

impl SurfaceGraph {
    pub fn patch(&self, id: u32) -> &Patch {
        &self.patches[id as usize]
    }

    pub fn len(&self) -> usize {
        self.patches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    /// (floor patches, wall patches) — the split of climbable surfaces, for the startup sanity log.
    pub fn patch_stats(&self) -> (usize, usize) {
        let walls = self
            .patches
            .iter()
            .filter(|p| matches!(p.kind, PatchKind::Wall(..)))
            .count();
        (self.patches.len() - walls, walls)
    }

    /// The floor patch of `cell`, if it is a floor cell (used to seed the field from unit cells).
    pub fn floor_patch_cell(&self, cell: IVec2) -> Option<u32> {
        self.index.get(&(cell, FLOOR_SLOT)).copied()
    }

    /// The first walled-edge patch of the cell containing `pos` (for seeding crabs onto a wall).
    pub fn wall_patch_at(&self, dungeon: &Dungeon, pos: Vec3) -> Option<u32> {
        let cell = dungeon.world_to_cell(pos);
        [N, E, S, W]
            .into_iter()
            .find_map(|dir| self.index.get(&(cell, wall_slot(dir))).copied())
    }

    /// Build the full surface graph from the dungeon geometry.
    pub fn build(dungeon: &Dungeon) -> SurfaceGraph {
        let inner = 0.5 * TILE_SIZE - WALL_THICKNESS; // inner-face inset from cell centre
        let hy = WALL_HEIGHT * 0.5;

        let mut patches: Vec<Patch> = Vec::new();
        let mut index: HashMap<(IVec2, u8), u32> = HashMap::new();

        // --- Nodes: a floor patch per floor cell, a wall patch per walled edge. ---
        for y in 0..dungeon.height as i32 {
            for x in 0..dungeon.width as i32 {
                let cell = IVec2::new(x, y);
                if !dungeon.is_floor(cell) {
                    continue;
                }
                let c = dungeon.cell_center(cell); // y = 0
                index.insert((cell, FLOOR_SLOT), patches.len() as u32);
                patches.push(Patch {
                    kind: PatchKind::Floor(cell),
                    center: c,
                    normal: Vec3::Y,
                    tan_u: Vec3::X,
                    tan_v: Vec3::Z,
                    half: Vec2::splat(0.5 * TILE_SIZE),
                });

                for dir in [N, E, S, W] {
                    if !dungeon.walled(cell, dir) {
                        continue;
                    }
                    let (base, normal, tan_u) = wall_frame(c, dir, inner);
                    index.insert((cell, wall_slot(dir)), patches.len() as u32);
                    patches.push(Patch {
                        kind: PatchKind::Wall(cell, dir),
                        center: base + Vec3::Y * hy,
                        normal,
                        tan_u,
                        tan_v: Vec3::Y,
                        half: Vec2::new(0.5 * TILE_SIZE, hy),
                    });
                }
            }
        }

        // --- Edges: collect undirected, dedup by (min,max) so each pair is added once. ---
        let mut seen: HashSet<(u32, u32)> = HashSet::new();
        let mut edges: Vec<(u32, u32, Vec3)> = Vec::new();
        let mut add = |a: u32, b: u32, gate: Vec3, edges: &mut Vec<(u32, u32, Vec3)>| {
            let key = (a.min(b), a.max(b));
            if seen.insert(key) {
                edges.push((a, b, gate));
            }
        };

        for y in 0..dungeon.height as i32 {
            for x in 0..dungeon.width as i32 {
                let cell = IVec2::new(x, y);
                if !dungeon.is_floor(cell) {
                    continue;
                }
                let c = dungeon.cell_center(cell);
                let floor_id = index[&(cell, FLOOR_SLOT)];

                // (1) Floor↔floor, 8-connected with the no-corner-cutting rule (both shared
                //     orthogonals must be floor), matching `flowfield` and the collision resolver.
                const NB: [(i32, i32); 8] = [
                    (1, 0),
                    (-1, 0),
                    (0, 1),
                    (0, -1),
                    (1, 1),
                    (1, -1),
                    (-1, 1),
                    (-1, -1),
                ];
                for (dx, dy) in NB {
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
                    let nid = index[&(n, FLOOR_SLOT)];
                    let nc = dungeon.cell_center(n);
                    add(floor_id, nid, (c + nc) * 0.5, &mut edges);
                }

                // Per walled edge of this cell: mount, along-run, corner adjacencies.
                for dir in [N, E, S, W] {
                    if !dungeon.walled(cell, dir) {
                        continue;
                    }
                    let wall_id = index[&(cell, wall_slot(dir))];
                    let (base, _n, _u) = wall_frame(c, dir, inner);

                    // (2) Floor↔wall base — mount/dismount. Gate at the face base.
                    add(floor_id, wall_id, base, &mut edges);

                    // (3) Along-run — same-dir wall on a run-neighbour cell. Shared vertical edge.
                    for off in run_offsets(dir) {
                        let rn = cell + off;
                        if dungeon.is_floor(rn) && dungeon.walled(rn, dir) {
                            if let Some(&rid) = index.get(&(rn, wall_slot(dir))) {
                                let gate = base
                                    + Vec3::Y * hy
                                    + Vec3::new(off.x as f32, 0.0, off.y as f32) * (0.5 * TILE_SIZE);
                                add(wall_id, rid, gate, &mut edges);
                            }
                        } else if dungeon.is_floor(rn) {
                            // (4b) Concave corner — the run ends at a floor cell; a perpendicular wall
                            //      may wrap around the rock corner on the diagonal cell.
                            let partner_cell = rn + dir_offset(dir);
                            if let Some(pdir) = offset_to_dir(-off) {
                                if dungeon.is_floor(partner_cell)
                                    && dungeon.walled(partner_cell, pdir)
                                {
                                    if let Some(&pid) =
                                        index.get(&(partner_cell, wall_slot(pdir)))
                                    {
                                        let gate =
                                            (patches[wall_id as usize].center
                                                + patches[pid as usize].center)
                                                * 0.5;
                                        add(wall_id, pid, gate, &mut edges);
                                    }
                                }
                            }
                        }
                    }
                }

                // (4a) Convex corner — two perpendicular walls on THIS cell meet at a vertical edge.
                for (a, b, sx, sz) in [
                    (N, E, 1.0f32, -1.0f32),
                    (E, S, 1.0, 1.0),
                    (S, W, -1.0, 1.0),
                    (W, N, -1.0, -1.0),
                ] {
                    if dungeon.walled(cell, a) && dungeon.walled(cell, b) {
                        let aid = index[&(cell, wall_slot(a))];
                        let bid = index[&(cell, wall_slot(b))];
                        let gate = Vec3::new(c.x + sx * inner, hy, c.z + sz * inner);
                        add(aid, bid, gate, &mut edges);
                    }
                }
            }
        }

        // Materialize the adjacency lists (both directions) with integer costs.
        let mut adj: Vec<Vec<Adj>> = (0..patches.len()).map(|_| Vec::new()).collect();
        for (a, b, gate) in edges {
            let cost = edge_cost(&patches[a as usize], &patches[b as usize]);
            adj[a as usize].push(Adj { to: b, cost, gate });
            adj[b as usize].push(Adj { to: a, cost, gate });
        }

        SurfaceGraph {
            patches,
            index,
            adj,
        }
    }
}

/// Inner-face base centre (y=0), inward normal, and horizontal run axis for a walled edge.
fn wall_frame(cell_center: Vec3, dir: usize, inner: f32) -> (Vec3, Vec3, Vec3) {
    let c = cell_center;
    match dir {
        E => (Vec3::new(c.x + inner, 0.0, c.z), Vec3::NEG_X, Vec3::Z),
        W => (Vec3::new(c.x - inner, 0.0, c.z), Vec3::X, Vec3::Z),
        N => (Vec3::new(c.x, 0.0, c.z - inner), Vec3::Z, Vec3::X),
        _ /* S */ => (Vec3::new(c.x, 0.0, c.z + inner), Vec3::NEG_Z, Vec3::X),
    }
}

/// Integer edge cost ≈ 10·(world distance between patch centres), floored at 1, matching the octile
/// scale (`CARDINAL=10`) `flowfield` uses so surface and floor costs are comparable.
fn edge_cost(a: &Patch, b: &Patch) -> u32 {
    (((a.center - b.center).length() * 10.0).round() as u32).max(1)
}

/// Min-heap entry for the Dijkstra expansion, ordered so `BinaryHeap` pops the lowest cost (mirrors
/// `flowfield::Node`).
struct Node {
    cost: u32,
    patch: u32,
}
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.cmp(&self.cost) // reversed → min-heap
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

const UNREACHABLE: u32 = u32::MAX;

/// A goal-directed field over the surface graph: per-patch cost to the nearest source and the
/// down-gradient neighbour a crab should head to (with the gate to steer at). Shared read-only via
/// `Arc` by the whole swarm.
pub struct SurfaceField {
    cost: Vec<u32>,
    flow_to: Vec<u32>,   // steepest-descent neighbour patch; self ⇒ source/arrived, sentinel via cost
    flow_gate: Vec<Vec3>,
}

impl SurfaceField {
    /// Multi-source Dijkstra from every patch in `sources` (each seeded at cost 0), so every reachable
    /// patch flows toward its nearest source. `None` if `sources` is empty (no squad to pursue).
    pub fn build(graph: &SurfaceGraph, sources: &[u32]) -> Option<SurfaceField> {
        if sources.is_empty() || graph.is_empty() {
            return None;
        }
        let n = graph.patches.len();
        let mut cost = vec![UNREACHABLE; n];
        let mut heap = BinaryHeap::new();
        for &s in sources {
            let si = s as usize;
            if si < n && cost[si] != 0 {
                cost[si] = 0;
                heap.push(Node { cost: 0, patch: s });
            }
        }

        while let Some(Node { cost: c, patch }) = heap.pop() {
            let pi = patch as usize;
            if c > cost[pi] {
                continue; // stale
            }
            for a in &graph.adj[pi] {
                let ni = a.to as usize;
                let nc = c + a.cost;
                if nc < cost[ni] {
                    cost[ni] = nc;
                    heap.push(Node {
                        cost: nc,
                        patch: a.to,
                    });
                }
            }
        }

        // Steepest descent: each patch points at the neighbour of strictly-lower cost, aiming at that
        // edge's gate. Sources and unreachable patches point at themselves (no flow).
        let mut flow_to: Vec<u32> = (0..n as u32).collect();
        let mut flow_gate = vec![Vec3::ZERO; n];
        for p in 0..n {
            if cost[p] == UNREACHABLE || cost[p] == 0 {
                continue;
            }
            let mut best = cost[p];
            for a in &graph.adj[p] {
                if cost[a.to as usize] < best {
                    best = cost[a.to as usize];
                    flow_to[p] = a.to;
                    flow_gate[p] = a.gate;
                }
            }
        }

        Some(SurfaceField {
            cost,
            flow_to,
            flow_gate,
        })
    }

    /// The next patch to head to and the gate to steer toward, or `None` if `patch` is a source,
    /// unreachable, or out of range (crab holds position / attacks).
    pub fn flow(&self, patch: u32) -> Option<(u32, Vec3)> {
        let p = patch as usize;
        if p >= self.flow_to.len() || self.cost[p] == UNREACHABLE || self.flow_to[p] == patch {
            return None;
        }
        Some((self.flow_to[p], self.flow_gate[p]))
    }
}
