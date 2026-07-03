//! Builds the playable dungeon: a coarse WFC room graph, expanded into rooms + corridors
//! on a fine tile grid, instantiated with Kenney floor/wall/corner models. The [`Dungeon`]
//! resource is the single source of truth for walkability (used by player collision and fog).

use bevy::prelude::*;

use crate::wfc::{self, CellKind, Rng, E, N, S, W};

/// World size of one fine grid cell (Kenney `floor-square` is 1×1 in X/Z).
pub const TILE_SIZE: f32 = 1.0;

// Coarse WFC operates on room slots; each expands to a BLOCK×BLOCK patch of fine tiles.
const COARSE_W: usize = 7;
const COARSE_H: usize = 7;
const BLOCK: usize = 7;
/// Room side length range (kept ≤ BLOCK-2 so every room has a ≥1-tile rock margin).
const ROOM_MIN: usize = 4;
const ROOM_MAX: usize = 5;

const DUNGEON_SEED: u64 = 0x5C0_9191; // nods to SCP-9191, the slop generator
const MAX_ATTEMPTS: u32 = 20;

const FLOOR_GLB: &str = "kenney_prototype-kit/Models/GLB format/floor-square.glb";
const WALL_GLB: &str = "kenney_prototype-kit/Models/GLB format/wall.glb";
const CORNER_GLB: &str = "kenney_prototype-kit/Models/GLB format/wall-corner.glb";

/// Tags every spawned tile entity (floor or wall) with the fine grid cell it belongs to,
/// so fog of war can reveal/hide a cell's geometry as a unit.
#[derive(Component)]
pub struct Tile {
    pub cell: IVec2,
}

/// The realized dungeon on the fine grid: a walkability mask plus the player spawn.
#[derive(Resource)]
pub struct Dungeon {
    pub width: usize,
    pub height: usize,
    walkable: Vec<bool>,
    pub spawn: IVec2,
}

pub struct DungeonPlugin;

impl Plugin for DungeonPlugin {
    fn build(&self, app: &mut App) {
        // Generation is pure CPU work with no asset dependency, so build the grid and
        // insert the resource now — it is then available to every Startup system
        // (player spawn, fog init) without cross-plugin ordering games.
        app.insert_resource(Dungeon::generate(DUNGEON_SEED));
        app.add_systems(Startup, spawn_tiles);
    }
}

impl Dungeon {
    /// Collapse a coarse room graph, keep the largest connected component, and expand
    /// each surviving slot into a room + corridors on the fine grid.
    fn generate(seed: u64) -> Self {
        let coarse = wfc::generate(COARSE_W, COARSE_H, seed, MAX_ATTEMPTS);
        let kept = largest_room_component(&coarse);

        let width = COARSE_W * BLOCK;
        let height = COARSE_H * BLOCK;
        let mut walkable = vec![false; width * height];
        let mut rng = Rng::new(seed ^ 0xC0FFEE);

        let block_center = |cx: usize, cy: usize| (cx * BLOCK + BLOCK / 2, cy * BLOCK + BLOCK / 2);
        let coarse_open = |cx: usize, cy: usize, dir: usize| coarse.cells[cy * COARSE_W + cx].open[dir];

        // Carve a centred room in every kept slot's block.
        for cy in 0..COARSE_H {
            for cx in 0..COARSE_W {
                if !kept[cy * COARSE_W + cx] {
                    continue;
                }
                let rw = rng.range_usize(ROOM_MIN, ROOM_MAX);
                let rh = rng.range_usize(ROOM_MIN, ROOM_MAX);
                let ox = cx * BLOCK + (BLOCK - rw) / 2;
                let oy = cy * BLOCK + (BLOCK - rh) / 2;
                for y in oy..oy + rh {
                    for x in ox..ox + rw {
                        walkable[y * width + x] = true;
                    }
                }
            }
        }

        // Carve a 1-wide corridor along each Link edge, between the two block centres.
        for cy in 0..COARSE_H {
            for cx in 0..COARSE_W {
                if !kept[cy * COARSE_W + cx] {
                    continue;
                }
                let (bx, by) = block_center(cx, cy);
                if cx + 1 < COARSE_W && kept[cy * COARSE_W + cx + 1] && coarse_open(cx, cy, E) {
                    let (nx, _) = block_center(cx + 1, cy);
                    for x in bx..=nx {
                        walkable[by * width + x] = true;
                    }
                }
                if cy + 1 < COARSE_H && kept[(cy + 1) * COARSE_W + cx] && coarse_open(cx, cy, S) {
                    let (_, ny) = block_center(cx, cy + 1);
                    for y in by..=ny {
                        walkable[y * width + bx] = true;
                    }
                }
            }
        }

        // Spawn at the block centre of the kept room nearest the coarse centre.
        let center = Vec2::new(COARSE_W as f32 / 2.0, COARSE_H as f32 / 2.0);
        let spawn_slot = (0..COARSE_W * COARSE_H)
            .filter(|&i| kept[i])
            .min_by(|&a, &b| {
                let pa = Vec2::new((a % COARSE_W) as f32, (a / COARSE_W) as f32);
                let pb = Vec2::new((b % COARSE_W) as f32, (b / COARSE_W) as f32);
                (pa - center)
                    .length_squared()
                    .partial_cmp(&(pb - center).length_squared())
                    .unwrap()
            })
            .expect("dungeon must contain at least one room");
        let (sx, sy) = block_center(spawn_slot % COARSE_W, spawn_slot / COARSE_W);

        Dungeon {
            width,
            height,
            walkable,
            spawn: IVec2::new(sx as i32, sy as i32),
        }
    }

    #[inline]
    fn index(&self, c: IVec2) -> usize {
        c.y as usize * self.width + c.x as usize
    }

    #[inline]
    pub fn in_bounds(&self, c: IVec2) -> bool {
        c.x >= 0 && c.y >= 0 && (c.x as usize) < self.width && (c.y as usize) < self.height
    }

    pub fn is_floor(&self, c: IVec2) -> bool {
        self.in_bounds(c) && self.walkable[self.index(c)]
    }

    #[inline]
    fn neighbor(c: IVec2, dir: usize) -> IVec2 {
        match dir {
            N => IVec2::new(c.x, c.y - 1),
            E => IVec2::new(c.x + 1, c.y),
            S => IVec2::new(c.x, c.y + 1),
            W => IVec2::new(c.x - 1, c.y),
            _ => unreachable!(),
        }
    }

    /// Can the player move from `c` into its `dir` neighbour? Walls sit only on
    /// floor↔rock boundaries, so this is simply "the neighbour is floor".
    fn can_cross(&self, c: IVec2, dir: usize) -> bool {
        self.is_floor(c) && self.is_floor(Self::neighbor(c, dir))
    }

    /// Does floor cell `c` need a wall on edge `dir`? True when the neighbour is rock
    /// or off-grid — the room perimeter.
    fn walled(&self, c: IVec2, dir: usize) -> bool {
        self.is_floor(c) && !self.is_floor(Self::neighbor(c, dir))
    }

    pub fn cell_center(&self, c: IVec2) -> Vec3 {
        Vec3::new(c.x as f32 * TILE_SIZE, 0.0, c.y as f32 * TILE_SIZE)
    }

    pub fn world_to_cell(&self, pos: Vec3) -> IVec2 {
        IVec2::new(
            (pos.x / TILE_SIZE).round() as i32,
            (pos.z / TILE_SIZE).round() as i32,
        )
    }

    pub fn spawn_world(&self) -> Vec3 {
        self.cell_center(self.spawn)
    }

    /// Resolve continuous movement against walls, one axis at a time so the player
    /// slides along walls instead of stopping dead. Walls sit on the grid lines
    /// between a floor cell and rock, so each axis clamps to the wall it would cross.
    pub fn resolve_move(&self, pos: Vec3, delta: Vec3, radius: f32) -> Vec3 {
        let mut p = pos;

        let cell = self.world_to_cell(p);
        if delta.x > 0.0 && !self.can_cross(cell, E) {
            p.x = (p.x + delta.x).min((cell.x as f32 + 0.5) * TILE_SIZE - radius);
        } else if delta.x < 0.0 && !self.can_cross(cell, W) {
            p.x = (p.x + delta.x).max((cell.x as f32 - 0.5) * TILE_SIZE + radius);
        } else {
            p.x += delta.x;
        }

        let cell = self.world_to_cell(p);
        if delta.z > 0.0 && !self.can_cross(cell, S) {
            p.z = (p.z + delta.z).min((cell.y as f32 + 0.5) * TILE_SIZE - radius);
        } else if delta.z < 0.0 && !self.can_cross(cell, N) {
            p.z = (p.z + delta.z).max((cell.y as f32 - 0.5) * TILE_SIZE + radius);
        } else {
            p.z += delta.z;
        }

        p
    }
}

/// Flood-fill the coarse room slots across Link edges, returning a per-slot mask of the
/// single largest connected component (the playable dungeon; the rest becomes rock).
fn largest_room_component(coarse: &wfc::WfcResult) -> Vec<bool> {
    let (w, h) = (coarse.width, coarse.height);
    let is_room = |i: usize| coarse.cells[i].kind == CellKind::Floor;
    let mut visited = vec![false; w * h];
    let mut best: Vec<usize> = Vec::new();

    for start in 0..w * h {
        if visited[start] || !is_room(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        visited[start] = true;
        while let Some(i) = stack.pop() {
            component.push(i);
            let (cx, cy) = (i % w, i / w);
            // A Link edge on this slot connects to a room neighbour (socket rule).
            let links = [
                (cy > 0, N, i.wrapping_sub(w)),
                (cx + 1 < w, E, i + 1),
                (cy + 1 < h, S, i + w),
                (cx > 0, W, i.wrapping_sub(1)),
            ];
            for (in_bounds, dir, ni) in links {
                if in_bounds && coarse.cells[i].open[dir] && is_room(ni) && !visited[ni] {
                    visited[ni] = true;
                    stack.push(ni);
                }
            }
        }
        if component.len() > best.len() {
            best = component;
        }
    }

    let mut kept = vec![false; w * h];
    for i in best {
        kept[i] = true;
    }
    kept
}

/// Transform for a straight wall panel on edge `dir` of cell `c`.
fn straight_wall(c: IVec2, dir: usize) -> Transform {
    let (i, j) = (c.x as f32, c.y as f32);
    let quarter = std::f32::consts::FRAC_PI_2;
    match dir {
        // Vertical wall lines run along Z (the wall's default orientation).
        E => Transform::from_xyz((i + 0.5) * TILE_SIZE, 0.0, j * TILE_SIZE),
        W => Transform::from_xyz((i - 0.5) * TILE_SIZE, 0.0, j * TILE_SIZE),
        // Horizontal wall lines run along X → rotate the panel 90°.
        S => Transform::from_xyz(i * TILE_SIZE, 0.0, (j + 0.5) * TILE_SIZE)
            .with_rotation(Quat::from_rotation_y(quarter)),
        N => Transform::from_xyz(i * TILE_SIZE, 0.0, (j - 0.5) * TILE_SIZE)
            .with_rotation(Quat::from_rotation_y(quarter)),
        _ => unreachable!(),
    }
}

/// Transform for an L-corner piece at cell `c`. The model covers the N+E edges; each
/// quarter-turn about Y rotates it to the next corner (NE→NW→SW→SE).
fn corner_piece(c: IVec2, quarter_turns: u32) -> Transform {
    let center = Vec3::new(c.x as f32 * TILE_SIZE, 0.0, c.y as f32 * TILE_SIZE);
    Transform::from_translation(center)
        .with_rotation(Quat::from_rotation_y(quarter_turns as f32 * std::f32::consts::FRAC_PI_2))
}

/// The four convex corners as `(edge_a, edge_b, quarter_turns)` for the corner model.
const CORNERS: [(usize, usize, u32); 4] = [(N, E, 0), (N, W, 1), (S, W, 2), (S, E, 3)];

/// Instantiate one floor scene per floor cell, and perimeter walls using corner pieces
/// for convex corners plus straight panels for the rest. Tiles start hidden so fog of
/// war reveals them as the player explores.
fn spawn_tiles(mut commands: Commands, dungeon: Res<Dungeon>, assets: Res<AssetServer>) {
    let floor_scene = assets.load(GltfAssetLabel::Scene(0).from_asset(FLOOR_GLB));
    let wall_scene = assets.load(GltfAssetLabel::Scene(0).from_asset(WALL_GLB));
    let corner_scene = assets.load(GltfAssetLabel::Scene(0).from_asset(CORNER_GLB));

    let spawn_piece = |commands: &mut Commands, cell: IVec2, scene: Handle<_>, transform: Transform| {
        commands.spawn((
            Tile { cell },
            WorldAssetRoot(scene),
            transform,
            Visibility::Hidden,
        ));
    };

    for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }

            spawn_piece(
                &mut commands,
                cell,
                floor_scene.clone(),
                Transform::from_translation(dungeon.cell_center(cell)),
            );

            // Which of this cell's edges border rock / off-grid → need a wall.
            let mut walled = [false; 4];
            for dir in [N, E, S, W] {
                walled[dir] = dungeon.walled(cell, dir);
            }

            // Greedily consume adjacent walled pairs as clean L-corner pieces...
            for (a, b, turns) in CORNERS {
                if walled[a] && walled[b] {
                    spawn_piece(&mut commands, cell, corner_scene.clone(), corner_piece(cell, turns));
                    walled[a] = false;
                    walled[b] = false;
                }
            }
            // ...then straight panels for any remaining single edges.
            for dir in [N, E, S, W] {
                if walled[dir] {
                    spawn_piece(&mut commands, cell, wall_scene.clone(), straight_wall(cell, dir));
                }
            }
        }
    }
}
