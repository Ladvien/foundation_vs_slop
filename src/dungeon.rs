//! Builds the playable dungeon: a coarse WFC room graph, expanded into rooms + corridors
//! on a fine tile grid, rendered as textured primitives (Backrooms wallpaper walls + carpet
//! floor). The [`Dungeon`] resource is the single source of truth for walkability (used by
//! player collision and fog).

use avian3d::prelude::*;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use serde::Deserialize;

use crate::placement::ir::{Opening, PropertyBag, Rect2, Region};
use crate::rng::{seeded, DetRng};
use crate::wfc::{self, CellKind, E, N, S, W};

/// World size of one fine grid cell.
pub const TILE_SIZE: f32 = 1.0;
/// Half the wall cuboid's thickness — used to inset walls flush with the tile edge. Thin (0.14 total)
/// so a 1-tile doorway has `TILE - 2·WALL_THICKNESS = 0.72` of clear width: enough for a 0.44-wide
/// unit to pass without wedging (the earlier 0.2 walls left only 0.6 and units caught in doorways).
const WALL_HALF_THICKNESS: f32 = 0.07;
/// Full wall thickness. Walls sit flush inside the tile edge, so a walled cell's
/// walkable area is inset by this much — the collision uses it as the barrier plane.
pub const WALL_THICKNESS: f32 = WALL_HALF_THICKNESS * 2.0;
/// Max distance the player box may move per collision sub-step. Kept below
/// [`WALL_THICKNESS`] so a fast (large-dt) step can't overshoot a wall and tunnel through.
const MAX_STEP: f32 = WALL_THICKNESS * 0.5;
/// Wall height (full, for the enclosed Backrooms look). Public so the crab surface-nav graph
/// (`surface_nav`) knows the vertical extent of each climbable wall face (Y 0→`WALL_HEIGHT`).
/// ~8 ft (2.4 m) at 1 unit = 1 m, so a ~6 ft squad member and real-scale furniture read correctly.
pub const WALL_HEIGHT: f32 = 2.4;
/// Clear height of a doorway opening (top of the door / bottom of the lintel). Below the ~2.05 m door
/// so the door tucks under the header with no gap; the wall runs continuous above it (`WALL_HEIGHT`).
const DOORWAY_HEIGHT: f32 = 2.0;

/// Camera-facing (SE/SW — i.e. the E and S edge) walls render at this fraction of `WALL_HEIGHT`: a low
/// knee wall you always see over into every room, regardless of where the squad is. Their doors and
/// headers are dropped too (nothing to frame on a knee wall). This knee-wall cutaway is the *single*
/// camera-occlusion path — keep it below `1.0` (there is no full-wall fallback mode).
pub const CAMERA_WALL_FRACTION: f32 = 0.25;

/// True when the camera-facing-wall knee-wall mode is active (any fraction below full height).
pub const SHORT_CAMERA_WALLS: bool = CAMERA_WALL_FRACTION < 1.0;

/// A wall counts as camera-facing when its position sits at least this far toward +X or +Z of its cell
/// centre (its E or S edge). Straight edges sit at ≈0.4 and corner arms similarly, so 0.1 cleanly
/// separates the near (E/S) faces from the far (N/W) ones.
pub const CAMERA_FACING_EPS: f32 = 0.1;

/// In the fixed 45° iso view the camera looks from (+X,+Z), so the walls that occlude a room's interior
/// are its E/S faces, whose inner faces point toward the camera with normal `-X` / `-Z`. This is the
/// single source of truth for "camera-facing": knee-wall squashing, furniture wall-face selection,
/// blood-splatter placement, and crab-nest seating all classify walls through this rule (by normal here,
/// or via the positional [`is_camera_facing_pos`] twin), so they can never disagree about which walls
/// face the camera.
pub fn is_camera_facing(inner_face_normal: Vec3) -> bool {
    inner_face_normal == Vec3::NEG_X || inner_face_normal == Vec3::NEG_Z
}

/// [`is_camera_facing`] for callers holding a spawned wall's world position rather than its face
/// normal: a wall on its cell's E/S edge sits `> CAMERA_FACING_EPS` toward +X/+Z of the cell centre.
pub fn is_camera_facing_pos(wall_pos: Vec3, cell_center: Vec3) -> bool {
    wall_pos.x - cell_center.x > CAMERA_FACING_EPS || wall_pos.z - cell_center.z > CAMERA_FACING_EPS
}

// Coarse WFC operates on room slots; each expands to a `block`×`block` patch of fine tiles. At 1 tile =
// 1 m the sizes read at real, Backrooms-like human scale under 2.4 m (8 ft) ceilings. When `block` is
// vastly larger than the rooms each floats in deep negative space — the liminal look — so the
// generation shape is data-driven via `assets/dungeon.ron` (see `DungeonConfig`), not hardcoded here.

/// Generation parameters loaded from `assets/dungeon.ron` — the single source of truth for the coarse
/// WFC, room sizing, and (Phase 2) the liminality dial. 1 tile = 1 m. This is the *dungeon shape* knob;
/// physical wall/tile dimensions stay compile-time `const`s above, since they are consumed by `const`
/// initializers in other modules (`squad`, `metropolis`, `nest`) and are a world-physics contract, not
/// a per-seed generation knob.
#[derive(Debug, Clone, Deserialize)]
pub struct DungeonConfig {
    /// Coarse WFC grid, in room slots. Each slot expands to a `block`×`block` fine-tile patch.
    pub coarse_w: usize,
    pub coarse_h: usize,
    /// Fine tiles (= metres) per coarse slot side. Rooms float inside their block (the Backrooms void).
    pub block: usize,
    /// Corridor width in tiles (2 = today's carve: the block-centre lane plus `corridor_width - 1` more).
    pub corridor_width: usize,
    pub seed: u64,
    /// WFC restart budget before a convergence failure panics (loud, one-path).
    pub max_attempts: u32,
    /// Liminality dial in [0,1] (consumed in Phase 2). 1.0 = sparse Backrooms boxes adrift in the void;
    /// 0.0 = realistic contiguous rooms sharing walls. Present now so Phase 1 and 2 share one schema.
    pub liminality: f32,
    /// The six coarse WFC prototype weights (the dungeon's shape distribution).
    pub wfc_weights: WfcWeights,
    /// Weighted room classes with realistic metric footprints (Merrell 2011: per-room area + aspect).
    pub room_types: Vec<RoomType>,
}

/// The six coarse WFC base-prototype weights, in `wfc::build_prototypes` order.
#[derive(Debug, Clone, Deserialize)]
pub struct WfcWeights {
    pub rock: f64,
    pub dead_end: f64,
    pub corridor: f64,
    pub corner: f64,
    pub tee: f64,
    pub cross: f64,
}

/// One weighted room class. `area` in m² (= tiles², since 1 tile = 1 m); `aspect` = long/short (≥ 1).
/// Realistic residential ranges: Merrell, Schkufza & Koltun, "Computer-Generated Residential Building
/// Layouts"; Smelik et al., "A Survey on Procedural Modelling for Virtual Worlds" (cgf.12276).
#[derive(Debug, Clone, Deserialize)]
pub struct RoomType {
    pub tag: String,
    pub area_min: f32,
    pub area_max: f32,
    pub aspect_min: f32,
    pub aspect_max: f32,
    pub weight: f64,
}

/// Minimum room side in tiles, so a tiny-area type (a ~3 m² bathroom) never rounds to a degenerate
/// 0/1-tile room. A structural floor of the carve, not a per-seed knob.
const ROOM_FLOOR: usize = 2;

/// Path to the required dungeon generation config (mirrors the placement RON load contract).
const DUNGEON_CONFIG_PATH: &str = "assets/dungeon.ron";

/// Parse + validate a [`DungeonConfig`] from RON text. Returns a descriptive error rather than
/// panicking — the caller decides how loudly (mirrors `placement::manifest::parse_manifest`). Validates
/// every invariant generation relies on, so a bad config fails at the door, not mid-carve.
pub fn parse_config(text: &str) -> Result<DungeonConfig, String> {
    let cfg: DungeonConfig =
        ron::from_str(text).map_err(|e| format!("{DUNGEON_CONFIG_PATH} parse error: {e}"))?;
    if cfg.coarse_w == 0 || cfg.coarse_h == 0 {
        return Err("coarse_w and coarse_h must be > 0".into());
    }
    if cfg.block < 4 {
        return Err(format!("block must be >= 4 (got {})", cfg.block));
    }
    if cfg.corridor_width == 0 || cfg.corridor_width > cfg.block {
        return Err(format!(
            "corridor_width must be in 1..=block (got {})",
            cfg.corridor_width
        ));
    }
    if !(0.0..=1.0).contains(&cfg.liminality) {
        return Err(format!("liminality must be in [0,1] (got {})", cfg.liminality));
    }
    if cfg.room_types.is_empty() {
        return Err("room_types must be non-empty".into());
    }
    for t in &cfg.room_types {
        if t.weight < 0.0 {
            return Err(format!("room type '{}' weight must be >= 0", t.tag));
        }
        if t.area_min <= 0.0 || t.area_max < t.area_min {
            return Err(format!("room type '{}' area range invalid", t.tag));
        }
        if t.aspect_min < 1.0 || t.aspect_max < t.aspect_min {
            return Err(format!(
                "room type '{}' aspect range invalid (aspect must be >= 1)",
                t.tag
            ));
        }
    }
    if cfg.room_types.iter().map(|t| t.weight).sum::<f64>() <= 0.0 {
        return Err("room_types weights must sum to > 0".into());
    }
    Ok(cfg)
}

/// Read + parse the dungeon config file. One path: a missing or malformed config is a hard error the
/// caller surfaces loudly (there is no default generation to fall back to).
fn load_config(path: &str) -> Result<DungeonConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    parse_config(&text)
}

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
    /// One bounded region per kept room slot — the addressable containers the placement grammar
    /// furnishes (see `crate::placement`). Carries each room's rect, boundary openings, and the
    /// corridor-adjacency graph, so cross-room rules are first-class.
    pub regions: Vec<Region>,
}

pub struct DungeonPlugin;

impl Plugin for DungeonPlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback (mirrors `PlacementPlugin`). A missing or malformed
        // `assets/dungeon.ron` is a loud startup failure, not a silent default world.
        let config = load_config(DUNGEON_CONFIG_PATH).unwrap_or_else(|e| panic!("dungeon: {e}"));
        // Generation is pure CPU work with no asset dependency, so build the grid and insert the
        // resource now — it is then available to every Startup system (player spawn, fog init) without
        // cross-plugin ordering games. A zero-room generation is a loud, one-path failure.
        app.insert_resource(Dungeon::generate(&config).unwrap_or_else(|e| panic!("dungeon: {e}")));
        app.add_systems(Startup, spawn_tiles);
    }
}

impl Dungeon {
    /// Collapse a coarse room graph, keep the largest connected component, and expand
    /// each surviving slot into a room + corridors on the fine grid. Fails loud (one path) if the
    /// collapse yields zero rooms, rather than returning a degenerate empty dungeon.
    fn generate(config: &DungeonConfig) -> Result<Self, String> {
        let (cw, ch, block) = (config.coarse_w, config.coarse_h, config.block);
        let weights = [
            config.wfc_weights.rock,
            config.wfc_weights.dead_end,
            config.wfc_weights.corridor,
            config.wfc_weights.corner,
            config.wfc_weights.tee,
            config.wfc_weights.cross,
        ];
        // An all-Solid collapse is a *valid* (non-contradiction) WFC result, so `wfc::generate` won't
        // re-roll it on its own — a tiny grid or a heavily rock-weighted config can land there. Re-roll
        // the whole coarse collapse with offset seeds until it yields at least one room, then fail loud
        // (one path) rather than carve a degenerate empty dungeon. Attempt 0 uses the config seed
        // unchanged, so a config that already produces rooms is byte-identical to a single collapse.
        let (coarse, kept) = (0..config.max_attempts.max(1))
            .map(|attempt| {
                let c = wfc::generate(
                    cw,
                    ch,
                    config.seed.wrapping_add(attempt as u64),
                    config.max_attempts,
                    &weights,
                );
                let kept = largest_room_component(&c);
                (c, kept)
            })
            .find(|(_, kept)| kept.iter().any(|&b| b))
            .ok_or_else(|| {
                format!(
                    "dungeon generation produced zero rooms after {} attempts (coarse {cw}x{ch}); \
                     the room/weight config cannot fill this grid",
                    config.max_attempts.max(1)
                )
            })?;

        let width = cw * block;
        let height = ch * block;
        let mut walkable = vec![false; width * height];
        let mut rng = seeded(config.seed ^ 0xC0FFEE);

        let block_center = |cx: usize, cy: usize| (cx * block + block / 2, cy * block + block / 2);
        let coarse_open = |cx: usize, cy: usize, dir: usize| coarse.cells[cy * cw + cx].open[dir];

        // One placement Region per kept slot. `slot_region[slot]` maps a coarse slot to its region id
        // so the adjacency/opening pass below can link neighbours. Rects are captured here (in the same
        // RNG-consuming loop) so region geometry stays in lockstep with the carved rooms.
        let mut regions: Vec<Region> = Vec::new();
        let mut slot_region: Vec<Option<u32>> = vec![None; cw * ch];

        // Carve a centred room in every kept slot's block. Size + type are drawn per room from the
        // config's weighted room-type table (Merrell 2011: per-room area + aspect ratio), so rooms read
        // at realistic metric scale and carry a type tag the furniture pass couples to.
        let max_side = block.saturating_sub(2); // keep a >=1-tile rock margin inside the block
        for cy in 0..ch {
            for cx in 0..cw {
                if !kept[cy * cw + cx] {
                    continue;
                }
                let (rw, rh, tag) = pick_room(config, max_side, &mut rng);
                let ox = cx * block + (block - rw) / 2;
                let oy = cy * block + (block - rh) / 2;
                for y in oy..oy + rh {
                    for x in ox..ox + rw {
                        walkable[y * width + x] = true;
                    }
                }

                let id = regions.len() as u32;
                slot_region[cy * cw + cx] = Some(id);
                regions.push(Region {
                    id,
                    rect: Rect2 {
                        min: [ox as i32, oy as i32],
                        max: [(ox + rw) as i32, (oy + rh) as i32],
                    },
                    openings: Vec::new(),
                    adjacency: Vec::new(),
                    props: PropertyBag {
                        tags: vec!["room".to_string(), tag],
                    },
                });
            }
        }

        // Carve a `corridor_width`-wide corridor along each Link edge, between the two block centres,
        // so the (0.7-wide) hero fits with room to spare. Lanes stack off the block-centre lane: an E
        // corridor stacks in +y, an S corridor in +x. At `corridor_width = 2` this is the classic carve.
        let lanes = config.corridor_width;
        for cy in 0..ch {
            for cx in 0..cw {
                if !kept[cy * cw + cx] {
                    continue;
                }
                let (bx, by) = block_center(cx, cy);
                if cx + 1 < cw && kept[cy * cw + cx + 1] && coarse_open(cx, cy, E) {
                    let (nx, _) = block_center(cx + 1, cy);
                    for x in bx..=nx {
                        for lane in 0..lanes {
                            walkable[(by + lane) * width + x] = true;
                        }
                    }
                }
                if cy + 1 < ch && kept[(cy + 1) * cw + cx] && coarse_open(cx, cy, S) {
                    let (_, ny) = block_center(cx, cy + 1);
                    for y in by..=ny {
                        for lane in 0..lanes {
                            walkable[y * width + bx + lane] = true;
                        }
                    }
                }
            }
        }

        // Link regions: for every corridor leaving a kept slot, record an adjacency edge and the
        // interior floor cell where the corridor meets the room wall (the region's opening). Edge
        // links are symmetric under the WFC socket rule (a slot's `open[dir]` agrees with its
        // neighbour's `open[opposite]`), so scanning all four directions finds every opening once.
        for cy in 0..ch {
            for cx in 0..cw {
                let Some(id) = slot_region[cy * cw + cx] else {
                    continue;
                };
                let (bx, by) = block_center(cx, cy);
                let rect = regions[id as usize].rect;
                for dir in [N, E, S, W] {
                    let (nx, ny) = match dir {
                        N => (cx as i32, cy as i32 - 1),
                        E => (cx as i32 + 1, cy as i32),
                        S => (cx as i32, cy as i32 + 1),
                        W => (cx as i32 - 1, cy as i32),
                        _ => unreachable!(),
                    };
                    if nx < 0 || ny < 0 || nx >= cw as i32 || ny >= ch as i32 {
                        continue;
                    }
                    if !coarse_open(cx, cy, dir) {
                        continue;
                    }
                    let Some(nid) = slot_region[ny as usize * cw + nx as usize] else {
                        continue;
                    };
                    let cell = match dir {
                        N => [bx as i32, rect.min[1]],
                        S => [bx as i32, rect.max[1] - 1],
                        E => [rect.max[0] - 1, by as i32],
                        W => [rect.min[0], by as i32],
                        _ => unreachable!(),
                    };
                    let region = &mut regions[id as usize];
                    region.adjacency.push(nid);
                    region.openings.push(Opening { dir, cell });

                    // Neck the corridor down to a single-tile doorway at the room wall, so one door leaf
                    // fills the gap (no open strip beside it). The carve stacks `corridor_width` lanes off
                    // the block-centre lane; the doorway keeps only the block-centre lane, so step one
                    // cell out of the room into the corridor (per `dir`) and rock out every *extra* lane
                    // there. Rocking (not just drawing a wall) keeps walkability, collision, and flow
                    // fields consistent — the block-centre tile stays open, so the room stays connected.
                    for lane in 1..config.corridor_width as i32 {
                        let neck = match dir {
                            E => IVec2::new(cell[0] + 1, cell[1] + lane),
                            W => IVec2::new(cell[0] - 1, cell[1] + lane),
                            N => IVec2::new(cell[0] + lane, cell[1] - 1),
                            S => IVec2::new(cell[0] + lane, cell[1] + 1),
                            _ => unreachable!(),
                        };
                        if neck.x >= 0
                            && neck.y >= 0
                            && (neck.x as usize) < width
                            && (neck.y as usize) < height
                        {
                            walkable[neck.y as usize * width + neck.x as usize] = false;
                        }
                    }
                }
            }
        }

        // Spawn at the block centre of the kept room nearest the coarse centre. A dungeon with zero
        // rooms is a generation failure surfaced loudly (one path), not a degenerate empty world.
        let center = Vec2::new(cw as f32 / 2.0, ch as f32 / 2.0);
        let spawn_slot = (0..cw * ch)
            .filter(|&i| kept[i])
            .min_by(|&a, &b| {
                let pa = Vec2::new((a % cw) as f32, (a / cw) as f32);
                let pb = Vec2::new((b % cw) as f32, (b / cw) as f32);
                // total_cmp is a total order over f32 (finite coords never NaN), so no unwrap/panic.
                (pa - center)
                    .length_squared()
                    .total_cmp(&(pb - center).length_squared())
            })
            .ok_or_else(|| {
                "dungeon generation produced zero rooms (largest component empty)".to_string()
            })?;
        let (sx, sy) = block_center(spawn_slot % cw, spawn_slot / cw);

        Ok(Dungeon {
            width,
            height,
            walkable,
            spawn: IVec2::new(sx as i32, sy as i32),
            regions,
        })
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
    /// or off-grid — the room perimeter. Public so `surface_nav` can enumerate the four
    /// climbable wall faces of every floor cell when building the crab navigation graph.
    pub fn walled(&self, c: IVec2, dir: usize) -> bool {
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

    /// Build a `Dungeon` directly from a row-major `walkable` mask, for tests that need a
    /// deterministic hand-crafted layout without running WFC generation.
    #[cfg(test)]
    pub(crate) fn from_walkable(width: usize, height: usize, walkable: Vec<bool>) -> Self {
        assert_eq!(walkable.len(), width * height, "walkable mask size mismatch");
        Dungeon {
            width,
            height,
            walkable,
            spawn: IVec2::ZERO,
            regions: Vec::new(),
        }
    }

    /// Test-only accessor for the private [`Self::is_solid`] ground-truth wall test.
    #[cfg(test)]
    pub(crate) fn is_solid_test(&self, x: f32, z: f32) -> bool {
        self.is_solid(x, z)
    }

    /// The inner faces of any walls bounding the cell that contains `pos`, as
    /// `(face_point, inward_normal)` pairs. `face_point` lies on the wall's inner plane at `pos`'s
    /// lateral projection (clamped within the cell) with `y = 0`; `inward_normal` points into the
    /// room. Used to splatter blood on nearby walls at a death (see `gore`). Same inset/`walled`
    /// math as [`Self::is_solid`].
    pub fn wall_faces_near(&self, pos: Vec3) -> Vec<(Vec3, Vec3)> {
        let cell = self.world_to_cell(pos);
        let cx = cell.x as f32 * TILE_SIZE;
        let cz = cell.y as f32 * TILE_SIZE;
        let inner = 0.5 * TILE_SIZE - WALL_THICKNESS;
        // Lateral position within the cell, so the splat lands next to where the death happened.
        let lx = (pos.x - cx).clamp(-inner, inner);
        let lz = (pos.z - cz).clamp(-inner, inner);
        let mut faces = Vec::new();
        if self.walled(cell, E) {
            faces.push((Vec3::new(cx + inner, 0.0, cz + lz), Vec3::NEG_X));
        }
        if self.walled(cell, W) {
            faces.push((Vec3::new(cx - inner, 0.0, cz + lz), Vec3::X));
        }
        if self.walled(cell, N) {
            faces.push((Vec3::new(cx + lx, 0.0, cz - inner), Vec3::Z));
        }
        if self.walled(cell, S) {
            faces.push((Vec3::new(cx + lx, 0.0, cz + inner), Vec3::NEG_Z));
        }
        faces
    }

    /// Clear (non-solid) distance from `pos` in eight directions, up to `max`. Returns
    /// `(axis, diag)` where `axis = (+X, -X, +Z, -Z)` and `diag = (+X+Z, -X+Z, +X-Z, -X-Z)` (each a
    /// unit-length diagonal). Marches out until it hits a wall slab / void (see [`Self::is_solid`]);
    /// used to clip a floor blood pool to an 8-sided region so it stops at the walls around it
    /// instead of seeping through (see `gore`).
    pub fn open_extents(&self, pos: Vec3, max: f32) -> (Vec4, Vec4) {
        let step = 0.04;
        let cast = |dx: f32, dz: f32| -> f32 {
            let mut d = step;
            while d <= max {
                if self.is_solid(pos.x + dx * d, pos.z + dz * d) {
                    return d;
                }
                d += step;
            }
            max
        };
        let axis = Vec4::new(cast(1.0, 0.0), cast(-1.0, 0.0), cast(0.0, 1.0), cast(0.0, -1.0));
        let r = std::f32::consts::FRAC_1_SQRT_2;
        let diag = Vec4::new(
            cast(r, r),
            cast(-r, r),
            cast(r, -r),
            cast(-r, -r),
        );
        (axis, diag)
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
            let start = p;

            // Preferred: slide both axes. If that keeps the box on floor, take it (the common case,
            // including flush wall-sliding since walls sit inside floor cells).
            let mut both = start;
            self.slide_axis(&mut both, d.x, half.x, half.y, true);
            self.slide_axis(&mut both, d.z, half.y, half.x, false);
            if !self.box_over_void(both, half) {
                p = both;
                continue;
            }

            // The combined slide cut a corner into the void (the thin diagonal slit between inset
            // walls). Slide along whichever single axis stays on floor instead of stalling dead —
            // this is what keeps a unit moving along a wall at an inside corner rather than freezing.
            let mut only_x = start;
            self.slide_axis(&mut only_x, d.x, half.x, half.y, true);
            let x_ok = !self.box_over_void(only_x, half);

            let mut only_z = start;
            self.slide_axis(&mut only_z, d.z, half.y, half.x, false);
            let z_ok = !self.box_over_void(only_z, half);

            p = match (x_ok, z_ok) {
                // Both valid alone but not together → keep the axis that advances further (the one
                // parallel to the wall), never squeezing diagonally through the slit into void.
                (true, true) => {
                    if (only_x - start).length_squared() >= (only_z - start).length_squared() {
                        only_x
                    } else {
                        only_z
                    }
                }
                (true, false) => only_x,
                (false, true) => only_z,
                (false, false) => start, // genuinely boxed in for this sub-step
            };
        }

        p
    }
}

/// Pick a weighted room type and draw its footprint in tiles (Merrell 2011: per-room area + aspect).
/// Deterministic — draws type, area, aspect, then orientation from the carve RNG in a fixed order.
/// Dimensions round to whole tiles and clamp to `[ROOM_FLOOR, max_side]` so the room fits its block with
/// a rock margin. Returns `(width, depth, type_tag)`.
fn pick_room(config: &DungeonConfig, max_side: usize, rng: &mut impl DetRng) -> (usize, usize, String) {
    let ty = weighted_room_type(&config.room_types, rng);
    let area = rand_range_f32(rng, ty.area_min, ty.area_max);
    let aspect = rand_range_f32(rng, ty.aspect_min, ty.aspect_max);
    let long = (area * aspect).sqrt();
    let short = (area / aspect).sqrt();
    // Randomly orient the long axis to x or y so rooms aren't all landscape.
    let (w_f, h_f) = if rng.unit() < 0.5 { (long, short) } else { (short, long) };
    let cap = max_side.max(ROOM_FLOOR); // guard clamp's min <= max even for a tiny block
    let rw = (w_f.round() as usize).clamp(ROOM_FLOOR, cap);
    let rh = (h_f.round() as usize).clamp(ROOM_FLOOR, cap);
    (rw, rh, ty.tag.clone())
}

/// Weighted choice of a room type (same idiom as `wfc::collapse_grid`'s prototype pick). `types` is
/// validated non-empty with positive total weight at config load, so the fall-through is unreachable.
fn weighted_room_type<'a>(types: &'a [RoomType], rng: &mut impl DetRng) -> &'a RoomType {
    let total: f64 = types.iter().map(|t| t.weight).sum();
    let mut r = rng.unit() * total;
    for t in types {
        r -= t.weight;
        if r <= 0.0 {
            return t;
        }
    }
    &types[types.len() - 1]
}

/// Uniform f32 in `[lo, hi)` from the deterministic RNG; returns `lo` for a degenerate/inverted range.
fn rand_range_f32(rng: &mut impl DetRng, lo: f32, hi: f32) -> f32 {
    if hi <= lo {
        return lo;
    }
    lo + (rng.unit() as f32) * (hi - lo)
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

/// Transform for a doorway lintel: a straight wall on edge `dir` of cell `c`, but raised so its
/// bottom sits at [`DOORWAY_HEIGHT`] and it fills up to the ceiling — the header above a door.
fn header_wall(c: IVec2, dir: usize) -> Transform {
    let base = straight_wall(c, dir);
    let y = DOORWAY_HEIGHT + (WALL_HEIGHT - DOORWAY_HEIGHT) * 0.5;
    base.with_translation(base.translation.with_y(y))
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
    let full_size = Vec3::new(WALL_THICKNESS, WALL_HEIGHT, TILE_SIZE);
    let short_size = Vec3::new(TILE_SIZE - WALL_THICKNESS, WALL_HEIGHT, WALL_THICKNESS);
    let wall_full = meshes.add(wall_mesh(full_size));
    let wall_short = meshes.add(wall_mesh(short_size));

    // One static half-space at y=0 catches every gib chunk (the whole floor is at y=0), so we don't
    // need a physics collider per floor tile — only the gib-chunk physics world uses these (see `gore`).
    commands.spawn((
        RigidBody::Static,
        Collider::half_space(Vec3::Y),
        Transform::default(),
    ));

    // Walls get a static cuboid collider matching their mesh box, so gib chunks bounce off them and
    // stay in the room. `wall_size` is the box for walls, `None` for floor tiles (which need none).
    let mut spawn_tile = |cell: IVec2,
                          mesh: Handle<Mesh>,
                          material: Handle<StandardMaterial>,
                          mut transform: Transform,
                          wall_size: Option<Vec3>| {
        // Knee-wall mode: any wall on the camera-facing (E/S) edge is squashed vertically to
        // `CAMERA_WALL_FRACTION` and reseated on the floor, so the camera sees over it. Classified by
        // the wall's offset from its cell centre (+X ⇒ E edge, +Z ⇒ S edge); floors sit at the centre
        // and are never squashed. Scaling the transform scales its collider to match.
        if SHORT_CAMERA_WALLS && wall_size.is_some() {
            let cx = cell.x as f32 * TILE_SIZE;
            let cz = cell.y as f32 * TILE_SIZE;
            if is_camera_facing_pos(transform.translation, Vec3::new(cx, 0.0, cz)) {
                transform.scale.y = CAMERA_WALL_FRACTION;
                transform.translation.y = WALL_HEIGHT * CAMERA_WALL_FRACTION * 0.5;
            }
        }
        let mut entity = commands.spawn((
            Tile { cell },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            Visibility::Hidden,
        ));
        if let Some(size) = wall_size {
            // avian `Collider::cuboid` takes FULL side lengths; the wall mesh is an origin-centred
            // `Cuboid` of the same size, so the collider lines up exactly under the transform.
            entity.insert((Wall, RigidBody::Static, Collider::cuboid(size.x, size.y, size.z)));
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
                None,
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
                    spawn_tile(cell, wall_full.clone(), wall_mat.clone(), full, Some(full_size));
                    spawn_tile(cell, wall_short.clone(), wall_mat.clone(), short, Some(short_size));
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
                        Some(full_size),
                    );
                }
            }
        }
    }

    // Doorway lintels: a short wall header above each 1-tile doorway (region opening), so the wall
    // reads as one continuous run from above — the door tucks under it. Placed on the doorway cell's
    // open wall edge, raised to `DOORWAY_HEIGHT`. Tagged like a wall (fog reveal + occlusion cutaway).
    let header_size = Vec3::new(WALL_THICKNESS, WALL_HEIGHT - DOORWAY_HEIGHT, TILE_SIZE);
    let header_mesh = meshes.add(wall_mesh(header_size));
    for region in &dungeon.regions {
        for op in &region.openings {
            // No lintel over an opening in a knee-high camera-facing (E/S) wall — it would float.
            if SHORT_CAMERA_WALLS && (op.dir == E || op.dir == S) {
                continue;
            }
            let cell = IVec2::new(op.cell[0], op.cell[1]);
            spawn_tile(
                cell,
                header_mesh.clone(),
                wall_mat.clone(),
                header_wall(cell, op.dir),
                Some(header_size),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small, valid config for generation tests (avoids depending on the shipped RON's exact values).
    fn test_config() -> DungeonConfig {
        DungeonConfig {
            coarse_w: 4,
            coarse_h: 4,
            block: 16,
            corridor_width: 2,
            seed: 0x5C0_9191,
            max_attempts: 20,
            liminality: 1.0,
            wfc_weights: WfcWeights {
                rock: 6.0,
                dead_end: 1.2,
                corridor: 2.5,
                corner: 2.5,
                tee: 1.2,
                cross: 0.6,
            },
            room_types: vec![
                RoomType { tag: "bathroom".into(), area_min: 3.0, area_max: 6.0, aspect_min: 1.0, aspect_max: 1.6, weight: 0.8 },
                RoomType { tag: "bedroom".into(), area_min: 9.0, area_max: 20.0, aspect_min: 1.0, aspect_max: 1.5, weight: 1.5 },
                RoomType { tag: "living".into(), area_min: 16.0, area_max: 40.0, aspect_min: 1.0, aspect_max: 1.7, weight: 1.6 },
            ],
        }
    }

    #[test]
    fn shipped_config_parses_and_generates() {
        // The real assets/dungeon.ron must parse, validate, and generate a non-empty dungeon.
        let text = std::fs::read_to_string(DUNGEON_CONFIG_PATH).expect("read dungeon.ron");
        let config = parse_config(&text).expect("dungeon.ron must be valid");
        let d = Dungeon::generate(&config).expect("must generate at least one room");
        assert!(!d.regions.is_empty(), "shipped config must produce rooms");
        assert!(d.walkable.iter().any(|&w| w), "dungeon must have floor");
    }

    #[test]
    fn generate_is_deterministic_for_a_config() {
        let config = test_config();
        let a = Dungeon::generate(&config).expect("gen a");
        let b = Dungeon::generate(&config).expect("gen b");
        assert_eq!(a.walkable, b.walkable, "same (config, seed) → same walkable mask");
        assert_eq!(a.spawn, b.spawn);
        assert_eq!(a.regions.len(), b.regions.len());
        for (ra, rb) in a.regions.iter().zip(&b.regions) {
            assert_eq!(ra.rect, rb.rect);
            assert_eq!(ra.props.tags, rb.props.tags);
        }
    }

    #[test]
    fn every_region_carries_a_type_tag() {
        let config = test_config();
        let type_tags: Vec<&str> = config.room_types.iter().map(|t| t.tag.as_str()).collect();
        let d = Dungeon::generate(&config).expect("gen");
        for r in &d.regions {
            assert!(r.props.has("room"), "region {} missing base 'room' tag", r.id);
            assert!(
                r.props.tags.iter().any(|t| type_tags.contains(&t.as_str())),
                "region {} has no room-type tag: {:?}",
                r.id,
                r.props.tags
            );
        }
    }

    #[test]
    fn room_dims_fit_block_with_margin() {
        let config = test_config();
        let max_side = (config.block - 2) as i32;
        let d = Dungeon::generate(&config).expect("gen");
        for r in &d.regions {
            let (w, h) = (r.rect.width(), r.rect.height());
            assert!(w >= ROOM_FLOOR as i32 && w <= max_side, "room width {w} out of range");
            assert!(h >= ROOM_FLOOR as i32 && h <= max_side, "room height {h} out of range");
        }
    }

    #[test]
    fn zero_room_config_returns_err_not_panic() {
        // A 1×1 coarse grid: the sole cell borders the void on all four edges, so the boundary rule
        // (`wfc::boundary_initial`) forbids every Link and it collapses to rock → zero rooms → a loud
        // Err, never a panic. Exercises the `Result` path that replaced the old `.expect(...)`.
        let mut config = test_config();
        config.coarse_w = 1;
        config.coarse_h = 1;
        assert!(Dungeon::generate(&config).is_err());
    }

    #[test]
    fn config_validation_rejects_bad_values() {
        // parse_config fails loud at the door on invalid input (one path, no silent default).
        assert!(parse_config("not ron").is_err());
        let bad_liminality = r#"(coarse_w:6,coarse_h:6,block:32,corridor_width:2,seed:1,max_attempts:20,
            liminality:2.0,wfc_weights:(rock:6.0,dead_end:1.2,corridor:2.5,corner:2.5,tee:1.2,cross:0.6),
            room_types:[(tag:"a",area_min:3.0,area_max:6.0,aspect_min:1.0,aspect_max:1.6,weight:1.0)])"#;
        assert!(parse_config(bad_liminality).is_err(), "liminality > 1 must be rejected");
        let empty_types = r#"(coarse_w:6,coarse_h:6,block:32,corridor_width:2,seed:1,max_attempts:20,
            liminality:1.0,wfc_weights:(rock:6.0,dead_end:1.2,corridor:2.5,corner:2.5,tee:1.2,cross:0.6),
            room_types:[])"#;
        assert!(parse_config(empty_types).is_err(), "empty room_types must be rejected");
    }
}
