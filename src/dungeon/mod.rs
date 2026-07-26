//! Builds the playable dungeon: a coarse WFC room graph, expanded into rooms + corridors
//! on a fine tile grid, rendered as textured primitives (Backrooms wallpaper walls + carpet
//! floor). The [`Dungeon`] resource is the single source of truth for walkability (used by
//! player collision and fog).

use std::collections::HashMap;

use avian3d::prelude::*;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::geom::{self, Point};
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
///
/// `pub` because it is already part of the **published authoring contract** — `docs/artist_guide.md` §1
/// lists it beside `TILE_SIZE` and `WALL_HEIGHT` as a number art must build to — and `site::pieces` now
/// scales the greybox doorway kit to it. A private constant with a public doc entry is one source of
/// truth pretending to be two.
pub const DOORWAY_HEIGHT: f32 = 2.0;

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

mod cutaway;
mod config;
mod layout;
mod render;
mod rooms;
#[cfg(test)]
mod tests;

// Glob re-exports so the split is invisible to the rest of the crate: every path that worked against
// the old single-file `dungeon` module still resolves. `pub` for the two surfaces other modules use
// directly (config types, the cutaway components the camera drives); `pub(crate)` for the internals the
// submodules share with each other.
pub use config::*;
pub use cutaway::*;
pub(crate) use layout::*;
pub(crate) use rooms::*;

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

/// Marks a tile entity as a wall (not a floor). Both carry [`Tile`], so the camera-side knee-wall
/// squash (see `CAMERA_WALL_FRACTION`) needs this to target walls only.
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

/// Sentinel in [`Dungeon::corridor_of`] for "this cell is not corridor floor".
const NO_CORRIDOR: u32 = u32::MAX;

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
    /// Which adjacency edge carved each cell, or [`NO_CORRIDOR`]. Rooms are `Region`s; corridors were only
    /// ever strokes in the walkability mask, so a system that wants to reason about *a passage* — mold that
    /// infests whole runs, say — had no handle on one. This restores it: a corridor run is an edge index.
    /// Private, like `walkable`, and read through [`Dungeon::corridor_id`] / [`Dungeon::is_corridor`].
    corridor_of: Vec<u32>,
}

pub struct DungeonPlugin;

impl Plugin for DungeonPlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback. The `dungeon:` slice comes from the unified
        // `assets/config/config.ron`, loaded and validated once by `ConfigPlugin` (registered first);
        // a missing or malformed file is already a loud startup failure there, not a silent default world.
        let config = app
            .world()
            .resource::<crate::config::GameConfig>()
            .dungeon
            .clone();
        // The `dungeon:` slice is stashed as a resource rather than consumed here, because generation is
        // now **per run**: `generate_dungeon` runs on `OnEnter(RunState::Active)` so a second expedition
        // gets a genuinely different world (FVS-A-5). It used to be generated at plugin-build time, which
        // made the dungeon a process-lifetime fact — `QUIT TO TITLE` → `NEW RUN` then resumed the same
        // used world, and "NEW RUN" was a lie.
        app.insert_resource(DungeonConfigRes(config))
            .add_systems(
                OnEnter(crate::session::RunState::Active),
                generate_dungeon.in_set(crate::session::RunBuild::World),
            )
            // Plugins add plugins. This one owns the **generated grid** — the `Dungeon` resource that
            // nav, fog, placement and containment all read — and delegates the two *presentations* of
            // it. Each is independently replaceable, which is the point of the split: a different art
            // treatment or a different see-into-rooms trick swaps one plugin and leaves the simulation
            // untouched.
            .add_plugins((render::DungeonRenderPlugin, cutaway::CutawayPlugin));
    }
}

/// The authored `dungeon:` config slice, held so each run can generate from it.
#[derive(Resource)]
pub struct DungeonConfigRes(pub DungeonConfig);

/// Generate this run's `Dungeon` from [`crate::session::RunSeed`].
///
/// The seed comes from the run, not the config: `RunSeed` starts *at* `config.seed` (so the first
/// expedition — and every golden — is unchanged) and advances on each run's exit. A zero-room generation
/// stays a loud, one-path failure.
fn generate_dungeon(
    mut commands: Commands,
    config: Res<DungeonConfigRes>,
    seed: Res<crate::session::RunSeed>,
) {
    let mut config = config.0.clone();
    config.seed = seed.0;
    commands.insert_resource(Dungeon::generate(&config).unwrap_or_else(|e| panic!("dungeon: {e}")));
}

impl Dungeon {
    /// Collapse a coarse room graph, keep the largest connected component, and expand
    /// each surviving slot into a room + corridors on the fine grid. Fails loud (one path) if the
    /// collapse yields zero rooms, rather than returning a degenerate empty dungeon.
    ///
    /// `pub(crate)` so `mycelia::habitat` can assert, in a GPU-free test, that the *shipped* seed and the
    /// *shipped* config together produce the level the design intends.
    pub(crate) fn generate(config: &DungeonConfig) -> Result<Self, String> {
        // Build the coarse layout for the selected topology — both fail loud (one path) if they can't
        // yield a usable dungeon — then carve the fine grid through the single shared `expand_to_fine`.
        let layout = match &config.topology {
            Topology::Grid => Self::grid_coarse_layout(config)?,
            Topology::Graph {
                site_spacing,
                link_weights,
            } => graph_layout(config, *site_spacing, link_weights)?,
        };

        // The carve RNG is seeded here (separately from the coarse seed) and drawn only inside
        // `expand_to_fine`, in site order — so the Grid path stays byte-identical to the pre-refactor carve.
        let mut rng = seeded(config.seed ^ 0xC0FFEE);
        let (walkable, regions, spawn, corridor_of) = expand_to_fine(&layout, config, &mut rng);

        Ok(Dungeon {
            width: layout.width,
            height: layout.height,
            walkable,
            spawn,
            regions,
            corridor_of,
        })
    }

    /// The Grid topology's coarse layout: collapse the coarse WFC room graph (re-rolling with offset
    /// seeds until it yields ≥1 room, else fail loud), keep the largest connected component, and hand it
    /// to `grid_layout`. Attempt 0 uses the config seed unchanged, so this is byte-identical to a single
    /// collapse when the config already produces rooms.
    fn grid_coarse_layout(config: &DungeonConfig) -> Result<CoarseLayout, String> {
        let (cw, ch) = (config.coarse_w, config.coarse_h);
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
        // the whole coarse collapse until it yields at least one room, then fail loud rather than carve
        // a degenerate empty dungeon.
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
        grid_layout(&coarse, &kept, config)
    }

    #[inline]
    fn index(&self, c: IVec2) -> usize {
        crate::util::row_major(c, self.width)
    }

    #[inline]
    pub fn in_bounds(&self, c: IVec2) -> bool {
        crate::util::in_grid(c, self.width, self.height)
    }

    pub fn is_floor(&self, c: IVec2) -> bool {
        self.in_bounds(c) && self.walkable[self.index(c)]
    }

    /// Every floor cell, in row-major order (y outer, x inner). The single source of truth for "which
    /// cells carry field value" — the field passes, the harness coverage denominator, and the habitat
    /// mask all draw the floor set from here so they cannot drift apart. Ascending order is load-bearing:
    /// callers such as `Stig::hotspot` rely on a stable first-max-wins scan.
    pub fn floor_cells(&self) -> impl Iterator<Item = IVec2> + '_ {
        let (w, h) = (self.width as i32, self.height as i32);
        (0..h)
            .flat_map(move |y| (0..w).map(move |x| IVec2::new(x, y)))
            .filter(move |&c| self.is_floor(c))
    }

    /// Which corridor run (adjacency-edge index) owns this cell, or `None` for room floor and rock.
    ///
    /// Gated on `is_floor`, so a doorway cell the necking pass closed reports `None` even though the
    /// corridor pass had opened it. Corridors cross room interiors on their way between site centres; those
    /// crossing cells are room floor and report `None` too.
    pub fn corridor_id(&self, c: IVec2) -> Option<u32> {
        if !self.is_floor(c) {
            return None;
        }
        match self.corridor_of[self.index(c)] {
            NO_CORRIDOR => None,
            id => Some(id),
        }
    }

    /// Is this cell corridor floor rather than room floor? See [`Dungeon::corridor_id`].
    pub fn is_corridor(&self, c: IVec2) -> bool {
        self.corridor_id(c).is_some()
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

    /// Does a yaw-snapped furniture footprint centred at `center` with pre-rotation half-extents
    /// `half` (½ width, ½ depth) rest **entirely on open floor** — no part outside the room and no
    /// part inside a wall slab? This is the footprint-aware containment gate the placement pass uses
    /// so a piece is rejected when its *body* — not merely its centre — crosses a wall or a
    /// notched-out corner of a non-rectangular room. It is the discrete analogue of the
    /// free-configuration-space non-penetration test of Merrell, Schkufza, Li, Agrawala & Koltun,
    /// "Interactive Furniture Layout Using Interior Design Guidelines" (SIGGRAPH 2011): a placement is
    /// legal iff its footprint lies in `C_free`. [`Self::is_solid`] is the ground truth (true outside
    /// the room or within a wall band), so a single solid sample means the piece would clip geometry.
    ///
    /// Quarter-turn furniture swaps width/depth at 90°/270°. The footprint is sampled on a lattice
    /// fine enough (≤ ½ [`WALL_THICKNESS`]) that a wall band can never slip between samples.
    pub fn footprint_on_floor(&self, center: Vec3, half: Vec2, yaw: f32) -> bool {
        // Quarter-turn yaw: at 90°/270° the footprint's width and depth swap.
        let quarter = (yaw / std::f32::consts::FRAC_PI_2).round() as i32 & 3;
        let (hx, hz) = if quarter % 2 == 1 {
            (half.y, half.x)
        } else {
            (half.x, half.y)
        };
        // Sample step finer than the wall band so a thin wall slab can't hide between samples.
        let step = (WALL_THICKNESS * 0.5).max(0.05);
        let nx = (hx / step).ceil().max(1.0) as i32;
        let nz = (hz / step).ceil().max(1.0) as i32;
        for ix in -nx..=nx {
            let x = center.x + (ix as f32 / nx as f32) * hx;
            for iz in -nz..=nz {
                let z = center.z + (iz as f32 / nz as f32) * hz;
                if self.is_solid(x, z) {
                    return false;
                }
            }
        }
        true
    }

    /// Does a yaw-snapped furniture footprint centred at `center` keep clear of every doorway's
    /// approach band? Furniture parked in a corridor mouth blocks the room's only entrance (player
    /// request 2026-07-19), so this rejects any footprint AABB overlapping the keep-clear band a
    /// doorway projects `keep_clear` metres into the room. Each [`Opening`] carries its interior floor
    /// cell (`cell`, lane 0) and the wall it pierces (`dir`); the doorway spans `width` lanes stacked
    /// perpendicular to `dir` (E/W stack along +Z, N/S along +X — matching the necking pass in
    /// `carve`), and the room interior lies opposite `dir`. Complements [`Self::footprint_on_floor`]:
    /// that keeps a piece out of the *walls*, this keeps it out of the *doorways*.
    pub fn footprint_clears_openings(
        &self,
        center: Vec3,
        half: Vec2,
        yaw: f32,
        openings: &[Opening],
        keep_clear: f32,
    ) -> bool {
        // Yaw-snapped footprint half-extents (quarter turns swap width/depth), as in `footprint_on_floor`.
        let quarter = (yaw / std::f32::consts::FRAC_PI_2).round() as i32 & 3;
        let (hx, hz) = if quarter % 2 == 1 { (half.y, half.x) } else { (half.x, half.y) };
        let (fx0, fx1) = (center.x - hx, center.x + hx);
        let (fz0, fz1) = (center.z - hz, center.z + hz);
        let h = 0.5 * TILE_SIZE;
        for op in openings {
            let cx = op.cell[0] as f32 * TILE_SIZE;
            let cz = op.cell[1] as f32 * TILE_SIZE;
            let span = (op.width.max(1) as f32 - 0.5) * TILE_SIZE; // lanes 0..width from `cell`
            // Keep-clear band: from the pierced wall plane, `keep_clear` deep into the room, across the
            // doorway's open lanes.
            let (bx0, bx1, bz0, bz1) = match op.dir {
                E => (cx + h - keep_clear, cx + h, cz - h, cz + span),
                W => (cx - h, cx - h + keep_clear, cz - h, cz + span),
                N => (cx - h, cx + span, cz - h, cz - h + keep_clear),
                S => (cx - h, cx + span, cz + h - keep_clear, cz + h),
                _ => continue,
            };
            // AABB overlap (strict, so a piece flush at the band edge is allowed).
            if fx0 < bx1 && fx1 > bx0 && fz0 < bz1 && fz1 > bz0 {
                return false;
            }
        }
        true
    }

    /// Build a `Dungeon` directly from a row-major `walkable` mask, for tests that need a
    /// deterministic hand-crafted layout without running WFC generation.
    #[cfg(test)]
    pub(crate) fn from_walkable(width: usize, height: usize, walkable: Vec<bool>) -> Self {
        assert_eq!(
            walkable.len(),
            width * height,
            "walkable mask size mismatch"
        );
        Dungeon {
            width,
            height,
            corridor_of: vec![NO_CORRIDOR; walkable.len()],
            walkable,
            spawn: IVec2::ZERO,
            regions: Vec::new(),
        }
    }

    /// Test-only constructor for a dungeon with rooms *and* corridor identity — the shape `mycelia::habitat`
    /// actually reasons about. `corridor_of` uses [`NO_CORRIDOR`] for room floor and rock.
    #[cfg(test)]
    pub(crate) fn from_parts(
        width: usize,
        height: usize,
        walkable: Vec<bool>,
        regions: Vec<Region>,
        corridor_of: Vec<u32>,
    ) -> Self {
        assert_eq!(walkable.len(), width * height, "walkable mask size mismatch");
        assert_eq!(corridor_of.len(), width * height, "corridor mask size mismatch");
        Dungeon { width, height, walkable, spawn: IVec2::ZERO, regions, corridor_of }
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
        let axis = Vec4::new(
            cast(1.0, 0.0),
            cast(-1.0, 0.0),
            cast(0.0, 1.0),
            cast(0.0, -1.0),
        );
        let r = std::f32::consts::FRAC_1_SQRT_2;
        let diag = Vec4::new(cast(r, r), cast(-r, r), cast(r, -r), cast(-r, -r));
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
    /// a sightline is blocked exactly when it enters a non-floor cell. Uses an integer
    /// (Bresenham-family) walk. Fog reveal wants the lenient variant,
    /// [`Self::line_of_sight_reveal`], instead.
    ///
    /// **Symmetric**, and enforced rather than assumed: the walk canonicalises its endpoint pair
    /// (see `line_of_sight_impl`) because a raw Bresenham is directional, and
    /// `line_of_sight_is_symmetric_over_random_maps` sweeps random maps to prove it.
    ///
    /// Callers of the strict rule: path smoothing (`pathfinding`) and the laser's target gate
    /// (`laser::fire_laser`, which tests this explicitly — it must not inherit the lenient rule the
    /// fog grid is built with, since a diagonal corner-peek is a real aiming exploit).
    pub fn line_of_sight(&self, a: IVec2, b: IVec2) -> bool {
        self.line_of_sight_impl(a, b, true)
    }

    /// Fog-reveal-only relaxation of [`Self::line_of_sight`]. A 1-wide corridor bordering a room
    /// makes the strict rule fail constantly: from almost any off-row viewpoint, a Bresenham
    /// sightline down the corridor takes diagonal steps whose "far" orthogonal neighbour is, by
    /// construction, the corridor's own bounding wall — so the strict rule reads that as peeking
    /// through a diagonal slit and blocks it, even though the corridor cell in question is plainly
    /// visible down the straight run. That produced the "picket fence" bug: every other wall
    /// segment along a corridor stuck `Unseen` forever (`debug_screenshots/
    /// region_2026-07-25_12-27-00-608.png`). Here a diagonal step is blocked only when *neither*
    /// orthogonal neighbour is floor — a true closed diagonal pinch (two walls meeting corner to
    /// corner) — not merely when one of them happens to be the wall you're walking alongside.
    pub(crate) fn line_of_sight_reveal(&self, a: IVec2, b: IVec2) -> bool {
        self.line_of_sight_impl(a, b, false)
    }

    fn line_of_sight_impl(&self, a: IVec2, b: IVec2, strict_corners: bool) -> bool {
        // Canonicalise the endpoint pair before walking, so visibility is SYMMETRIC by construction:
        // `line_of_sight(p, q) == line_of_sight(q, p)` for every p and q, under either corner rule.
        //
        // Bresenham is directional. `err` is seeded `dx - dy` and stepped from the start toward the
        // end, so on an exact diagonal tie the two directions visit different cells: (0,0)→(2,1)
        // crosses (1,0) while (2,1)→(0,0) crosses (1,1). If one of those is rock the two directions
        // disagree, and a wall segment then reveals when the squad walks past on one side but stays
        // `Unseen` on the mirrored approach — a second, independent source of the "picket fence"
        // residue, which relaxing the corner rule alone could not remove.
        //
        // Ordering the pair rather than OR-ing the two walks is deliberate. `f(a,b) || f(b,a)` is
        // also symmetric, but strictly more permissive — it would weaken the strict corner rule that
        // pathfinding and the laser gate depend on. Ordering changes only *which* of two already-
        // possible answers is returned, never how permissive the relation is. The key is a total
        // order (two `IVec2`s compare equal only when identical, and `a == b` is symmetric anyway),
        // so there is no tie for input order to decide.
        let (a, b) = if (a.x, a.y) <= (b.x, b.y) { (a, b) } else { (b, a) };
        let (mut x, mut y) = (a.x, a.y);
        let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
        let (sx, sy) = ((b.x - a.x).signum(), (b.y - a.y).signum());
        // Endpoints must themselves be floor to be visible at all.
        if !self.is_floor(a) || !self.is_floor(b) {
            return false;
        }
        // Step cell-by-cell; when the line passes exactly through a corner, test the two
        // diagonally-shared cells (no peeking through a diagonal wall slit).
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
                let n1 = self.is_floor(IVec2::new(x + sx, y));
                let n2 = self.is_floor(IVec2::new(x, y + sy));
                let blocked = if strict_corners { !n1 || !n2 } else { !n1 && !n2 };
                if blocked {
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
