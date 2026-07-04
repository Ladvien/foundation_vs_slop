//! Builds the playable dungeon: a coarse WFC room graph, expanded into rooms + corridors
//! on a fine tile grid, rendered as textured primitives (Backrooms wallpaper walls + carpet
//! floor). The [`Dungeon`] resource is the single source of truth for walkability (used by
//! player collision and fog).

use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;

use crate::occlusion::WallMaterials;
use crate::wfc::{self, CellKind, Rng, E, N, S, W};

/// World size of one fine grid cell.
pub const TILE_SIZE: f32 = 1.0;
/// Half the wall cuboid's thickness (walls are 0.2 thick) — used to inset walls flush
/// with the tile edge.
const WALL_HALF_THICKNESS: f32 = 0.1;
/// Full wall thickness. Walls sit flush inside the tile edge, so a walled cell's
/// walkable area is inset by this much — the collision uses it as the barrier plane.
const WALL_THICKNESS: f32 = WALL_HALF_THICKNESS * 2.0;
/// Max distance the player box may move per collision sub-step. Kept below
/// [`WALL_THICKNESS`] so a fast (large-dt) step can't overshoot a wall and tunnel through.
const MAX_STEP: f32 = WALL_THICKNESS * 0.5;
/// Wall height (full, for the enclosed Backrooms look).
const WALL_HEIGHT: f32 = 1.0;

// Coarse WFC operates on room slots; each expands to a BLOCK×BLOCK patch of fine tiles.
// 21×21 slots → a dungeon 3× larger per side than the original 7×7 (≈9× the floor area).
const COARSE_W: usize = 21;
const COARSE_H: usize = 21;
const BLOCK: usize = 7;
/// Room side length range (kept ≤ BLOCK-2 so every room has a ≥1-tile rock margin).
const ROOM_MIN: usize = 4;
const ROOM_MAX: usize = 5;

const DUNGEON_SEED: u64 = 0x5C0_9191; // nods to SCP-9191, the slop generator
const MAX_ATTEMPTS: u32 = 20;

// CC0 Backrooms textures (see assets/textures/CREDITS.md). Seamless 1024² diffuse maps;
// mapped onto textured primitives because the Kenney GLB UVs are palette-atlas points.
const WALL_TEXTURE: &str = "textures/backrooms-wall-diffuse.png";
const FLOOR_TEXTURE: &str = "textures/backrooms-carpet-diffuse.png";

/// Tags every spawned tile entity (floor or wall) with the fine grid cell it belongs to,
/// so fog of war can reveal/hide a cell's geometry as a unit.
#[derive(Component)]
pub struct Tile {
    pub cell: IVec2,
}

/// Marks a tile entity as a wall (not a floor). Both carry [`Tile`], so the camera-side
/// cutaway in `occlusion` needs this to target walls only.
#[derive(Component)]
pub struct Wall;

/// The two shared floor materials the line-of-sight fog swaps between per cell: `bright` when a
/// unit currently sees the cell, `dim` when it has only been explored before (see `fog`). Only
/// two handles exist, so fog swaps a handle rather than cloning a material per tile.
#[derive(Resource)]
pub struct FloorMaterials {
    pub bright: Handle<StandardMaterial>,
    pub dim: Handle<StandardMaterial>,
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

        // Carve a 2-wide corridor along each Link edge, between the two block centres,
        // so the (0.7-wide) hero fits with room to spare.
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
                        walkable[(by + 1) * width + x] = true;
                    }
                }
                if cy + 1 < COARSE_H && kept[(cy + 1) * COARSE_W + cx] && coarse_open(cx, cy, S) {
                    let (_, ny) = block_center(cx, cy + 1);
                    for y in by..=ny {
                        walkable[y * width + bx] = true;
                        walkable[y * width + bx + 1] = true;
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

    /// Does floor cell `c` need a wall on edge `dir`? True when the neighbour is rock
    /// or off-grid — the room perimeter.
    fn walled(&self, c: IVec2, dir: usize) -> bool {
        self.is_floor(c) && !self.is_floor(Self::neighbor(c, dir))
    }

    /// Is the world point `(x, z)` inside solid geometry — rock, off-grid, or a wall
    /// slab? Walls sit flush inside the tile edge, so a walled cell's walkable area is
    /// inset by [`WALL_THICKNESS`]. This is the ground truth the collision samples.
    fn is_solid(&self, x: f32, z: f32) -> bool {
        let cell = self.world_to_cell(Vec3::new(x, 0.0, z));
        if !self.is_floor(cell) {
            return true;
        }
        let lx = x - cell.x as f32 * TILE_SIZE; // offset within the tile, [-0.5, 0.5]·T
        let lz = z - cell.y as f32 * TILE_SIZE;
        let inner = 0.5 * TILE_SIZE - WALL_THICKNESS;
        (self.walled(cell, E) && lx > inner)
            || (self.walled(cell, W) && lx < -inner)
            || (self.walled(cell, N) && lz < -inner)
            || (self.walled(cell, S) && lz > inner)
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

    /// Grid line-of-sight from cell `a` to cell `b`: true iff every cell the straight segment
    /// crosses is floor. Walls only ever sit on floor↔non-floor edges (see [`Self::walled`]), so
    /// a sightline is blocked exactly when it enters a non-floor cell. Uses an integer supercover
    /// (Bresenham-family) walk so the returned visibility is symmetric. Reused by path smoothing
    /// (`pathfinding`) and the fog-of-war LOS (`fog`).
    pub fn line_of_sight(&self, a: IVec2, b: IVec2) -> bool {
        let (mut x, mut y) = (a.x, a.y);
        let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
        let (sx, sy) = ((b.x - a.x).signum(), (b.y - a.y).signum());
        // Endpoints must themselves be floor to be visible at all.
        if !self.is_floor(a) || !self.is_floor(b) {
            return false;
        }
        // Step cell-by-cell; when the line passes exactly through a corner, require *both*
        // diagonally-shared cells to be floor (no peeking through a diagonal wall slit).
        let mut err = dx - dy;
        while x != b.x || y != b.y {
            let e2 = 2 * err;
            let (mut step_x, mut step_y) = (false, false);
            if e2 > -dy {
                err -= dy;
                step_x = true;
            }
            if e2 < dx {
                err += dx;
                step_y = true;
            }
            if step_x && step_y {
                // Diagonal step: both orthogonal neighbours must be floor, else sight is blocked.
                if !self.is_floor(IVec2::new(x + sx, y)) || !self.is_floor(IVec2::new(x, y + sy)) {
                    return false;
                }
                x += sx;
                y += sy;
            } else if step_x {
                x += sx;
            } else {
                y += sy;
            }
            if !self.is_floor(IVec2::new(x, y)) {
                return false;
            }
        }
        true
    }

    /// Slide the box one axis by `step` (X if `axis_x`, else Z), snapping the leading edge
    /// to a wall's inner face if it would enter solid. Sub-stepping in [`Self::resolve_move`]
    /// keeps `|step|` below a wall's thickness so the edge can't skip past a wall. The edge
    /// is sampled at three points across the box's perpendicular span (low / mid / high).
    fn slide_axis(&self, p: &mut Vec3, step: f32, half_along: f32, half_perp: f32, axis_x: bool) {
        if step == 0.0 {
            return;
        }
        let dir = step.signum();
        let moved = (if axis_x { p.x } else { p.z }) + step;
        let perp = if axis_x { p.z } else { p.x };
        let edge = moved + dir * half_along;
        let e = 0.001;
        let solid = |q: f32| {
            if axis_x {
                self.is_solid(edge, q)
            } else {
                self.is_solid(q, edge)
            }
        };
        let resolved = if solid(perp - half_perp + e) || solid(perp) || solid(perp + half_perp - e)
        {
            let c = (edge / TILE_SIZE).round();
            (c + 0.5 * dir) * TILE_SIZE - dir * WALL_THICKNESS - dir * half_along
        } else {
            moved
        };
        if axis_x {
            p.x = resolved;
        } else {
            p.z = resolved;
        }
    }

    /// True if the axis-aligned box centered at `p` (half-extents `half`) overlaps any
    /// non-floor cell — i.e. it has cut a corner into the void. Walls *within* floor cells are
    /// handled by the inset snap in [`Self::slide_axis`]; this guards only against entering
    /// void/rock cells, which per-axis edge sampling can leak through at a diagonal notch
    /// corner (the inset walls leave a thin diagonal slit the box can squeeze through).
    fn box_over_void(&self, p: Vec3, half: Vec2) -> bool {
        let min = self.world_to_cell(Vec3::new(p.x - half.x, 0.0, p.z - half.y));
        let max = self.world_to_cell(Vec3::new(p.x + half.x, 0.0, p.z + half.y));
        for cy in min.y..=max.y {
            for cx in min.x..=max.x {
                if !self.is_floor(IVec2::new(cx, cy)) {
                    return true;
                }
            }
        }
        false
    }

    /// Resolve continuous movement against walls, one axis at a time so the player slides
    /// along walls instead of stopping dead. `half` is the player's box half-extents (X, Z).
    /// The move is sub-stepped so no single step exceeds a wall's thickness — a large-dt
    /// step would otherwise overshoot the thin wall slab and snap the player through it.
    pub fn resolve_move(&self, pos: Vec3, delta: Vec3, half: Vec2) -> Vec3 {
        let mut p = pos;
        let steps = (delta.length() / MAX_STEP).ceil().max(1.0) as u32;
        let d = delta / steps as f32;
        for _ in 0..steps {
            // Slide each axis, then reject that axis if it cut a corner into the void. Reverting
            // per-axis still lets the box slide flush along walls (which sit inside floor cells).
            let before_x = p.x;
            self.slide_axis(&mut p, d.x, half.x, half.y, true);
            if self.box_over_void(p, half) {
                p.x = before_x;
            }
            let before_z = p.z;
            self.slide_axis(&mut p, d.z, half.y, half.x, false);
            if self.box_over_void(p, half) {
                p.z = before_z;
            }
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

/// Transform for a full-length straight wall cuboid on edge `dir` of cell `c`. The cuboid
/// is inset by its half-thickness to sit *flush* with the tile edge (outer face on ±0.5),
/// and lifted by half its height (Bevy cuboids are origin-centred) so it rests on the floor.
fn straight_wall(c: IVec2, dir: usize) -> Transform {
    let (i, j) = (c.x as f32, c.y as f32);
    let quarter = std::f32::consts::FRAC_PI_2;
    let h = WALL_HALF_THICKNESS;
    let y = WALL_HEIGHT * 0.5;
    match dir {
        // Vertical wall lines run along Z (the cuboid's long axis).
        E => Transform::from_xyz((i + 0.5) * TILE_SIZE - h, y, j * TILE_SIZE),
        W => Transform::from_xyz((i - 0.5) * TILE_SIZE + h, y, j * TILE_SIZE),
        // Horizontal wall lines run along X → rotate the cuboid 90°.
        S => Transform::from_xyz(i * TILE_SIZE, y, (j + 0.5) * TILE_SIZE - h)
            .with_rotation(Quat::from_rotation_y(quarter)),
        N => Transform::from_xyz(i * TILE_SIZE, y, (j - 0.5) * TILE_SIZE + h)
            .with_rotation(Quat::from_rotation_y(quarter)),
        _ => unreachable!(),
    }
}

/// Transforms for the two arms of a convex corner: a full-length arm plus a shortened arm
/// that stops at the full arm's inner face, so the two cuboids meet without overlapping
/// (no coincident tops → no z-fighting on textured corners). Built as an NE template, then
/// rotated by `quarter_turns` about the cell centre (NE→NW→SW→SE) — the `(full, short)` pair.
fn corner_arms(c: IVec2, quarter_turns: u32) -> (Transform, Transform) {
    let center = Vec3::new(c.x as f32 * TILE_SIZE, 0.0, c.y as f32 * TILE_SIZE);
    let rot = Quat::from_rotation_y(quarter_turns as f32 * std::f32::consts::FRAC_PI_2);
    let flush = 0.5 * TILE_SIZE - WALL_HALF_THICKNESS; // arm inset so the outer face is on ±0.5
    let y = WALL_HEIGHT * 0.5;
    // Full arm on the east edge; short arm on the north edge (spans x −0.5 → +0.3).
    let full_local = Vec3::new(flush, y, 0.0);
    let short_local = Vec3::new(-WALL_HALF_THICKNESS, y, -flush);
    let full = Transform {
        translation: center + rot * full_local,
        rotation: rot,
        ..default()
    };
    let short = Transform {
        translation: center + rot * short_local,
        rotation: rot,
        ..default()
    };
    (full, short)
}

/// The four convex corners as `(edge_a, edge_b, quarter_turns)`.
const CORNERS: [(usize, usize, u32); 4] = [(N, E, 0), (N, W, 1), (S, W, 2), (S, E, 3)];

/// Build a wall cuboid whose wallpaper stands upright on every side face. Bevy's default
/// `Cuboid` UVs lay the texture on its side on the ±X faces, so straight walls and full corner
/// arms (which show their ±X faces) render the Backrooms stripes/chevrons running horizontally.
/// Here every side face maps the texture's V axis to world +Y, so the pattern is vertical
/// regardless of which way the wall faces (Y-axis rotations keep "up" as up).
fn wall_mesh(size: Vec3) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::new(size.x, size.y, size.z));
    let (
        Some(VertexAttributeValues::Float32x3(positions)),
        Some(VertexAttributeValues::Float32x3(normals)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION).cloned(),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL).cloned(),
    ) else {
        return mesh;
    };
    let half = size * 0.5;
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .zip(normals.iter())
        .map(|(p, n)| {
            let p = Vec3::from_array(*p);
            let n = Vec3::from_array(*n);
            if n.y.abs() > 0.5 {
                // Top / bottom faces: floor-plane mapping (their orientation is barely seen).
                [(p.x + half.x) / size.x, (p.z + half.z) / size.z]
            } else {
                // Side face: V climbs with world height so the wallpaper is upright; U runs
                // along the face's horizontal edge (Z for the ±X faces, X for the ±Z faces).
                let u = if n.x.abs() > 0.5 {
                    (p.z + half.z) / size.z
                } else {
                    (p.x + half.x) / size.x
                };
                let v = (half.y - p.y) / size.y; // V=0 at the top → texture right-way-up
                [u, v]
            }
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh
}

/// Instantiate the dungeon as textured primitives: a Backrooms-carpet floor quad per floor
/// cell, wallpaper cuboid walls on perimeter edges (corner pairs as clean two-cuboid Ls,
/// remaining single edges as straight walls). Tiles start hidden so fog reveals them.
fn spawn_tiles(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Shared materials + meshes (built once, reused for every tile). Both textures are
    // seamless, so the default sampler + [0,1] cuboid/plane UVs tile cleanly across cells.
    let wall_mat = materials.add(StandardMaterial {
        base_color_texture: Some(assets.load(WALL_TEXTURE)),
        perceptual_roughness: 0.95,
        metallic: 0.0,
        ..default()
    });
    // Ghosted twin of `wall_mat`, used by the cutaway for walls between the camera and the hero
    // (see `occlusion`). Only two wall material handles ever exist, so the cutaway swaps between
    // them per wall without cloning a material per entity. Alpha-to-coverage gives a *dithered*
    // (screen-door) transparency via MSAA sample coverage — no depth-sort artifacts, and the
    // hero reads through the stipple holes.
    let wall_mat_faded = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.5),
        base_color_texture: Some(assets.load(WALL_TEXTURE)),
        perceptual_roughness: 0.95,
        metallic: 0.0,
        alpha_mode: AlphaMode::AlphaToCoverage,
        ..default()
    });
    commands.insert_resource(WallMaterials {
        opaque: wall_mat.clone(),
        faded: wall_mat_faded,
    });
    let floor_mat = materials.add(StandardMaterial {
        base_color_texture: Some(assets.load(FLOOR_TEXTURE)),
        perceptual_roughness: 0.95,
        metallic: 0.0,
        ..default()
    });
    // Dimmed twin of `floor_mat` — the "explored but not currently in a unit's line of sight"
    // fog state. `base_color` tints the texture, so a dark cool grey remembers the terrain
    // without lighting it up. The fog swaps floor tiles between these two (see `fog`).
    let floor_mat_dim = materials.add(StandardMaterial {
        base_color: Color::srgb(0.28, 0.28, 0.36),
        base_color_texture: Some(assets.load(FLOOR_TEXTURE)),
        perceptual_roughness: 0.95,
        metallic: 0.0,
        ..default()
    });
    commands.insert_resource(FloorMaterials {
        bright: floor_mat.clone(),
        dim: floor_mat_dim,
    });

    let floor_mesh = meshes.add(Plane3d::default().mesh().size(TILE_SIZE, TILE_SIZE));
    let wall_full = meshes.add(wall_mesh(Vec3::new(WALL_THICKNESS, WALL_HEIGHT, TILE_SIZE)));
    let wall_short = meshes.add(wall_mesh(Vec3::new(
        TILE_SIZE - WALL_THICKNESS,
        WALL_HEIGHT,
        WALL_THICKNESS,
    )));

    let mut spawn_tile = |cell: IVec2,
                          mesh: Handle<Mesh>,
                          material: Handle<StandardMaterial>,
                          transform: Transform,
                          is_wall: bool| {
        let mut entity = commands.spawn((
            Tile { cell },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            Visibility::Hidden,
        ));
        if is_wall {
            entity.insert(Wall);
        }
    };

    for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }

            spawn_tile(
                cell,
                floor_mesh.clone(),
                floor_mat.clone(),
                Transform::from_translation(dungeon.cell_center(cell)),
                false,
            );

            // Which of this cell's edges border rock / off-grid → need a wall.
            let mut walled = [false; 4];
            for dir in [N, E, S, W] {
                walled[dir] = dungeon.walled(cell, dir);
            }

            // Greedily consume adjacent walled pairs as clean L-corners (full + short arm)...
            for (a, b, turns) in CORNERS {
                if walled[a] && walled[b] {
                    let (full, short) = corner_arms(cell, turns);
                    spawn_tile(cell, wall_full.clone(), wall_mat.clone(), full, true);
                    spawn_tile(cell, wall_short.clone(), wall_mat.clone(), short, true);
                    walled[a] = false;
                    walled[b] = false;
                }
            }
            // ...then straight walls for any remaining single edges.
            for dir in [N, E, S, W] {
                if walled[dir] {
                    spawn_tile(
                        cell,
                        wall_full.clone(),
                        wall_mat.clone(),
                        straight_wall(cell, dir),
                        true,
                    );
                }
            }
        }
    }
}
