//! Builds the playable dungeon: a coarse WFC room graph, expanded into rooms + corridors
//! on a fine tile grid, rendered as textured primitives (Backrooms wallpaper walls + carpet
//! floor). The [`Dungeon`] resource is the single source of truth for walkability (used by
//! player collision and fog).

use std::collections::HashMap;

use avian3d::prelude::*;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use serde::Deserialize;

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

// ── View-relative knee-wall cutaway ──────────────────────────────────────────────────────────────
// The knee-wall cutaway used to be baked once at spawn for the fixed (+X,+Z) iso view. With Q/E map
// rotation the camera can look from any of four corners, so *which* walls occlude a room changes with
// the view. These components + `update_cutaway` make the squash follow the camera: it is a purely
// *visual* effect — collision, navigation (`surface_nav`), and prop/nest/splat placement stay baked to
// the canonical orientation, so the camera never changes gameplay (it stays deterministic at every
// angle; only the rendered geometry re-poses).

/// Ease rate for the cutaway height/scale lerp — matched to the camera's own rotation smoothing so a
/// wall grows or shrinks over the same turn. Frame-rate independent via `1 − exp(−k·dt)`.
const CUTAWAY_SMOOTHING: f32 = 9.0;

/// How a spawned tile participates in the cutaway: floors don't; walls squash to knee height on the
/// near edge; wall-mounted lintels hide on the near edge. Passed to the tile spawner so the tag and
/// the initial (yaw=0) pose are set in one place.
#[derive(Clone, Copy)]
enum Cutaway {
    None,
    Wall,
    Mounted,
}

/// A wall that participates in the view-relative cutaway. `outward` is its outward-facing horizontal
/// normal (±X/±Z). The wall is squashed to `CAMERA_WALL_FRACTION` whenever that normal faces the
/// camera (its inner face then occludes the room). Full walls and corner arms both stand 0→`WALL_HEIGHT`.
#[derive(Component)]
pub struct CutawayWall {
    pub outward: Vec3,
}

/// A decoration mounted on a wall face (doorway lintel; wall-hung prop). `outward` is the host wall's
/// outward normal; the item is scaled to zero — hidden — while that wall is a near knee wall, so it
/// never floats in the cutaway gap above the squashed wall. `base_scale` is its shown scale. Hiding
/// rides `scale`, not `Visibility`, so it composes with the fog reveal (which owns `Visibility`).
#[derive(Component)]
pub struct CutawayMounted {
    pub outward: Vec3,
    pub base_scale: Vec3,
}

/// A wall's outward horizontal normal (±X/±Z), derived from its offset off the cell centre. Straight
/// walls sit ~0.4 along one axis; corner arms likewise, each dominant on a single axis — so the larger
/// component names the edge. The single classifier for both [`CutawayWall`] tagging and initial squash.
pub fn wall_outward(wall_pos: Vec3, cell_center: Vec3) -> Vec3 {
    let dx = wall_pos.x - cell_center.x;
    let dz = wall_pos.z - cell_center.z;
    if dx.abs() >= dz.abs() {
        Vec3::new(dx.signum(), 0.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, dz.signum())
    }
}

/// True when an outward-facing wall normal points toward the camera — its inner face occludes the room,
/// so the wall should be a knee wall. At the four 90° detents exactly the two adjacent edges qualify.
fn faces_camera(outward: Vec3, to_camera: Vec3) -> bool {
    outward.dot(to_camera) > 0.0
}

/// `(scale.y, translation.y)` for a wall standing 0→`WALL_HEIGHT`: knee-high and reseated on the floor
/// when near the camera, full height and centred otherwise.
fn wall_pose(near: bool) -> (f32, f32) {
    if near {
        (CAMERA_WALL_FRACTION, WALL_HEIGHT * CAMERA_WALL_FRACTION * 0.5)
    } else {
        (1.0, WALL_HEIGHT * 0.5)
    }
}

/// Ease every cutaway wall's height and every wall-mounted decoration's scale toward the pose implied
/// by the current camera direction, so the knee-wall cutaway rotates with the Q/E view. Visual only —
/// see the module comment above; nothing here touches nav, placement, or the fog's `Visibility`.
fn update_cutaway(
    time: Res<Time<bevy::time::Real>>,
    view: Res<crate::camera::CameraView>,
    mut walls: Query<(&CutawayWall, &mut Transform), Without<CutawayMounted>>,
    mut mounted: Query<(&CutawayMounted, &mut Transform), Without<CutawayWall>>,
) {
    if view.to_camera == Vec3::ZERO {
        return; // not yet seeded by the camera (first frame ordering) — leave the baked pose.
    }
    let ease = 1.0 - (-CUTAWAY_SMOOTHING * time.delta_secs()).exp();
    for (wall, mut tf) in &mut walls {
        let (scale_y, y) = wall_pose(faces_camera(wall.outward, view.to_camera));
        let (cur_scale, cur_y) = (tf.scale.y, tf.translation.y);
        tf.scale.y = cur_scale + (scale_y - cur_scale) * ease;
        tf.translation.y = cur_y + (y - cur_y) * ease;
    }
    for (deco, mut tf) in &mut mounted {
        let target = if faces_camera(deco.outward, view.to_camera) {
            Vec3::ZERO
        } else {
            deco.base_scale
        };
        let cur = tf.scale;
        tf.scale = cur + (target - cur) * ease;
    }
}

// Coarse WFC operates on room slots; each expands to a `block`×`block` patch of fine tiles. At 1 tile =
// 1 m the sizes read at real, Backrooms-like human scale under 2.4 m (8 ft) ceilings. When `block` is
// vastly larger than the rooms each floats in deep negative space — the liminal look — so the
// generation shape is data-driven via the `dungeon:` slice of `assets/config/config.ron` (see
// `DungeonConfig`), not hardcoded here.

/// Generation parameters — the `dungeon:` slice of `assets/config/config.ron`, the single source of
/// truth for the coarse
/// WFC, room sizing, and (Phase 2) the liminality dial. 1 tile = 1 m. This is the *dungeon shape* knob;
/// physical wall/tile dimensions stay compile-time `const`s above, since they are consumed by `const`
/// initializers in other modules (`squad`, `metropolis`, `nest`) and are a world-physics contract, not
/// a per-seed generation knob.
/// Which coarse layout the dungeon is built on. `Grid` is the fixed `coarse_w × coarse_h` lattice (the
/// default, unchanged). `Graph` places rooms irregularly (Poisson-disk sites connected by a Delaunay
/// graph collapsed with `wfc::collapse_graph`) for an organic, non-lattice look. This is config-selected
/// routing, not a fallback — each topology fails loud if it can't yield a usable dungeon (one path).
#[derive(Debug, Clone, Deserialize, Default)]
pub enum Topology {
    #[default]
    Grid,
    /// `site_spacing`: minimum tile distance between room sites (Poisson radius; must fit the level).
    /// `link_weights[k]`: relative weight of a site having `k` corridors (0-link rare, 1–2 dominant).
    Graph {
        site_spacing: f32,
        link_weights: [f64; 6],
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct DungeonConfig {
    /// Coarse WFC grid, in room slots. Each slot expands to a `block`×`block` fine-tile patch. For
    /// `Topology::Graph` this defines the level extent (`coarse_w*block × coarse_h*block`) that the
    /// Poisson sites are scattered across.
    pub coarse_w: usize,
    pub coarse_h: usize,
    /// Fine tiles (= metres) per coarse slot side. Rooms float inside their block (the Backrooms void).
    pub block: usize,
    /// Corridor width in tiles — the *minimum* of the per-corridor width range. Each corridor draws a
    /// width uniformly in `[corridor_width, corridor_width_max]`, so passages vary from tight to broad
    /// instead of every corridor being identical.
    pub corridor_width: usize,
    /// Upper bound of the per-corridor width range (tiles). `#[serde(default)]` → `None` means "no
    /// spread": every corridor is exactly `corridor_width`. When set it must be `>= corridor_width` and
    /// `<= block` (validated at load). This is the single width-variation knob (one path — the draw always
    /// runs; an unset/collapsed range just yields a constant width).
    #[serde(default)]
    pub corridor_width_max: Option<usize>,
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
    /// Corner-notching (room-shape complexity). `#[serde(default)]` → `None` means every room is a plain
    /// rectangle (4 corners). When set, eligible rooms are cut into L/T/plus shapes with up to 12 corners.
    #[serde(default)]
    pub notch: Option<NotchConfig>,
    /// Coarse layout selector. `#[serde(default)]` → the shipped RON (no `topology` field) stays `Grid`,
    /// so there is no behaviour change until a config opts in.
    #[serde(default)]
    pub topology: Topology,
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
    /// Spacious types (halls, large living rooms) set this: below liminality 1.0 they grow toward *all
    /// four* block edges to dominate their slot, so they read as large anchor spaces. Compact types leave
    /// it `false` and keep their drawn footprint (position still jitters), preserving the size hierarchy —
    /// a tiny bathroom stays tiny next to a sprawling hall. `#[serde(default)]` so only large types opt in.
    #[serde(default)]
    pub expands: bool,
}

/// Corner-notching: turns rectangular rooms into rectilinear polygons (L / T / Z / U / plus shapes) by
/// biting rectangular chunks out of a room's corners. Each notched corner adds two vertices, so a room
/// goes 4 → 6 → 8 → 10 → 12 corners as 0–4 corners are cut. Every notch stays strictly inside its own
/// corner quadrant — it never touches the room's centre row or column — so the central "cross" of floor
/// is always intact: the room stays connected, the block-centre corridor still lands on floor, and the
/// doorway derivation is unchanged. Purely a shape knob; walls/fog/collision/nav follow the per-cell
/// walkable mask and need no changes.
#[derive(Debug, Clone, Deserialize)]
pub struct NotchConfig {
    /// Probability an *eligible* room (min side ≥ `min_side`) gets any notches at all.
    pub chance: f64,
    /// Upper bound on how many distinct corners to cut (1..=4). A room draws `1..=max_corners` corners;
    /// 4 yields a 12-corner plus/cross. Values above 4 are rejected at load (a rect has four corners).
    pub max_corners: usize,
    /// Notch extent as a fraction of the corner's available quadrant (the space between the room edge and
    /// its centre row/column). Drawn per notch in `[depth_min, depth_max]`; larger = deeper bites.
    pub depth_min: f32,
    pub depth_max: f32,
    /// Only rooms whose shorter side is at least this many tiles are notched, so tiny rooms (bathrooms)
    /// stay clean rectangles and notches only shape the larger, more legible rooms.
    pub min_side: usize,
}

/// Minimum room side in tiles, so a tiny-area type (a ~3 m² bathroom) never rounds to a degenerate
/// 0/1-tile room. A structural floor of the carve, not a per-seed knob.
const ROOM_FLOOR: usize = 2;

/// Path to the required dungeon generation config (mirrors the placement RON load contract).
/// Parse + validate a [`DungeonConfig`] from standalone RON text (used by tests that build a config
/// inline). The shipped game reads its `dungeon:` slice from the unified `assets/config/config.ron`
/// via [`crate::config::load_game_config`]; both paths funnel through [`validate_config`]. Returns a
/// descriptive error rather than panicking — the caller decides how loudly.
pub fn parse_config(text: &str) -> Result<DungeonConfig, String> {
    let cfg: DungeonConfig =
        ron::from_str(text).map_err(|e| format!("dungeon config parse error: {e}"))?;
    validate_config(&cfg)?;
    Ok(cfg)
}

/// Validate every invariant generation relies on, on an already-deserialized [`DungeonConfig`]. Split
/// from [`parse_config`] so the unified config loader (`crate::config::load_game_config`) can validate
/// the `dungeon:` slice it deserializes as part of the master `GameConfig` — one path, no fallback.
pub fn validate_config(cfg: &DungeonConfig) -> Result<(), String> {
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
    if let Some(max) = cfg.corridor_width_max {
        if max < cfg.corridor_width || max > cfg.block {
            return Err(format!(
                "corridor_width_max must be in corridor_width..=block ({}..={}), got {}",
                cfg.corridor_width, cfg.block, max
            ));
        }
    }
    if !(0.0..=1.0).contains(&cfg.liminality) {
        return Err(format!(
            "liminality must be in [0,1] (got {})",
            cfg.liminality
        ));
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
    if let Some(n) = &cfg.notch {
        if !(0.0..=1.0).contains(&n.chance) {
            return Err(format!("notch.chance must be in [0,1] (got {})", n.chance));
        }
        if n.max_corners == 0 || n.max_corners > 4 {
            return Err(format!(
                "notch.max_corners must be in 1..=4 (got {})",
                n.max_corners
            ));
        }
        if !(0.0..=1.0).contains(&n.depth_min)
            || !(0.0..=1.0).contains(&n.depth_max)
            || n.depth_max < n.depth_min
        {
            return Err(format!(
                "notch depth must satisfy 0 <= depth_min <= depth_max <= 1 (got {}..={})",
                n.depth_min, n.depth_max
            ));
        }
        if n.min_side < ROOM_FLOOR {
            return Err(format!(
                "notch.min_side must be >= {ROOM_FLOOR} (got {})",
                n.min_side
            ));
        }
    }
    // The six coarse WFC prototype weights feed the Grid collapse (`wfc::collapse_one`). A NaN makes the
    // `r <= 0.0` pick never match, silently collapsing every cell to the highest-index prototype; a
    // non-positive sum forces the lowest-index (all-rock) prototype. Reject both at the door (mirrors the
    // `link_weights` check below), so a bad shape distribution never degenerates or fails to converge.
    let wfc_w = [
        cfg.wfc_weights.rock,
        cfg.wfc_weights.dead_end,
        cfg.wfc_weights.corridor,
        cfg.wfc_weights.corner,
        cfg.wfc_weights.tee,
        cfg.wfc_weights.cross,
    ];
    if wfc_w.iter().any(|w| !w.is_finite() || *w < 0.0) {
        return Err("wfc_weights must all be finite and >= 0".into());
    }
    if wfc_w.iter().sum::<f64>() <= 0.0 {
        return Err("wfc_weights must sum to > 0".into());
    }
    // `rock` is negative space: a config with only `rock` non-zero collapses to an all-solid, floorless
    // (unplayable) dungeon. Require some weight on a non-rock prototype so a playable floor set exists.
    if wfc_w[1..].iter().sum::<f64>() <= 0.0 {
        return Err(
            "wfc_weights must give weight to a non-rock prototype (else the dungeon has no floor)"
                .into(),
        );
    }
    if let Topology::Graph {
        site_spacing,
        link_weights,
    } = &cfg.topology
    {
        // Lower bound keeps per-site bounds large enough that rooms provably never overlap (the sizing
        // needs the nearest-neighbour Chebyshev distance ≥ ~4 tiles, i.e. Poisson radius ≥ ~5.66).
        let min_spacing = ROOM_FLOOR as f32 + 4.0;
        let level = (cfg.coarse_w.min(cfg.coarse_h) * cfg.block) as f32;
        if !site_spacing.is_finite() || *site_spacing < min_spacing {
            return Err(format!(
                "topology Graph: site_spacing must be >= {min_spacing} (got {site_spacing})"
            ));
        }
        if *site_spacing >= level {
            return Err(format!(
                "topology Graph: site_spacing {site_spacing} does not fit the {level}-tile level"
            ));
        }
        if link_weights.iter().any(|w| !w.is_finite() || *w < 0.0) {
            return Err("topology Graph: link_weights must be finite and >= 0".into());
        }
        if link_weights.iter().sum::<f64>() <= 0.0 {
            return Err("topology Graph: link_weights must sum to > 0".into());
        }
    }
    Ok(())
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
        // Generation is pure CPU work with no asset dependency, so build the grid and insert the
        // resource now — it is then available to every Startup system (player spawn, fog init) without
        // cross-plugin ordering games. A zero-room generation is a loud, one-path failure.
        app.insert_resource(Dungeon::generate(&config).unwrap_or_else(|e| panic!("dungeon: {e}")));
        app.add_systems(Startup, spawn_tiles);
        // The knee-wall cutaway follows the Q/E camera rotation (visual only). Runs on `Update` — it
        // re-poses render geometry from the camera direction and touches no pinned sim state.
        app.add_systems(Update, update_cutaway);
    }
}

// ── Topology-agnostic coarse layer ───────────────────────────────────────────────────────────────
// Both dungeon topologies — the fixed grid lattice and (Phase 3) the Poisson/Delaunay graph — produce
// a `CoarseLayout` and hand it to the single fine carver `expand_to_fine`, so furnish/nav/fog
// never depend on which topology ran. The grid front-end (`grid_layout`) is a byte-identical restatement
// of the old carve's coarse phase (Step-0 golden gate); the graph front-end is added later.

/// One room slot: its fine-grid centre and the block-like extent that bounds its room sizing, jitter,
/// and expansion-to-touch. For the grid, `bounds` is the exact block rect and `center` its centre.
struct Site {
    center: IVec2,
    bounds: Rect2,
}

/// The coarse room graph handed to `expand_to_fine`, independent of how it was produced. `sites` are in
/// carve order (the grid emits them row-major over kept slots, so RNG draws and region ids stay in
/// lockstep with the pre-refactor loop). `adjacency` is the undirected set of corridor links between kept
/// sites (already trimmed to one connected component). `spawn_site` is chosen by the front-end (each
/// topology owns its spawn rule) so `expand_to_fine` never re-derives it.
struct CoarseLayout {
    width: usize,
    height: usize,
    sites: Vec<Site>,
    adjacency: Vec<(usize, usize)>,
    spawn_site: usize,
}

/// The wall each endpoint of a corridor actually pierces, matching `carve_corridor`'s L-route: the
/// horizontal leg runs from `a`, the vertical leg into `b`. So `a` exits horizontally (E/W) unless the
/// edge is purely vertical, and `b` enters vertically (N/S) unless the edge is purely horizontal.
/// Returns `(a_dir, b_dir)`. For axis-aligned edges — always, for grid block centres offset by exactly
/// `(±block, 0)` or `(0, ±block)` — both reduce to the straight-line cardinal, so the Grid path stays
/// byte-identical; for a diagonal graph edge each end gets the wall its leg crosses, not a dominant-axis
/// guess (which would land one endpoint's door on a wall the corridor never touches).
fn corridor_exit_dirs(a: IVec2, b: IVec2) -> (usize, usize) {
    let a_dir = if a.x == b.x {
        if b.y > a.y {
            S
        } else {
            N
        }
    } else if b.x > a.x {
        E
    } else {
        W
    };
    let b_dir = if a.y == b.y {
        if a.x > b.x {
            E
        } else {
            W
        }
    } else if a.y > b.y {
        S
    } else {
        N
    };
    (a_dir, b_dir)
}

/// Where a corridor pierces one endpoint's room wall, from the carved L geometry (see `carve_corridor`).
/// `sc` is this room's site centre, `nc` the neighbour's, `rect` this room's rect, `is_first` whether this
/// site is the edge's FIRST endpoint (horizontal leg leaves it) or SECOND (vertical leg enters it).
/// Returns `(dir, cell)`. The first endpoint always exits through its own centre row/col. The second is
/// entered vertically (N/S) unless the L-corner `(sc.x, nc.y)` lands inside this room's y-range — then the
/// corridor actually enters via the horizontal leg through the E/W wall at row `nc.y`. This handles the
/// L-corner-inside-room case a dominant-axis guess gets wrong, and is byte-identical to the old
/// block-centre formula for axis-aligned (grid) edges.
fn derive_opening(sc: IVec2, nc: IVec2, rect: Rect2, is_first: bool) -> (usize, [i32; 2]) {
    if is_first {
        if sc.x == nc.x {
            if nc.y > sc.y {
                (S, [sc.x, rect.max[1] - 1])
            } else {
                (N, [sc.x, rect.min[1]])
            }
        } else if nc.x > sc.x {
            (E, [rect.max[0] - 1, sc.y])
        } else {
            (W, [rect.min[0], sc.y])
        }
    } else if nc.y == sc.y {
        // Pure-horizontal edge: the second endpoint is entered horizontally at its own centre row.
        if nc.x < sc.x {
            (W, [rect.min[0], sc.y])
        } else {
            (E, [rect.max[0] - 1, sc.y])
        }
    } else if nc.y < rect.min[1] {
        (N, [sc.x, rect.min[1]])
    } else if nc.y >= rect.max[1] {
        (S, [sc.x, rect.max[1] - 1])
    } else if nc.x < sc.x {
        // L-corner inside this room's y-range → entered via the horizontal leg through the W/E wall.
        (W, [rect.min[0], nc.y])
    } else {
        (E, [rect.max[0] - 1, nc.y])
    }
}

/// Build the Grid topology's `CoarseLayout` from a collapsed coarse WFC grid + its kept-slot mask. Sites
/// are the kept block centres (row-major, preserving carve order); each site's `bounds` is its exact
/// block rect; adjacency is every kept∧kept∧`open` edge (E and S per slot, each counted once, oriented
/// so the neighbour's W/N view is reconstructed from the socket-rule symmetry in `expand_to_fine`);
/// `spawn_site` is the kept slot nearest the coarse centre — the exact pre-refactor spawn rule.
fn grid_layout(
    coarse: &wfc::WfcResult,
    kept: &[bool],
    config: &DungeonConfig,
) -> Result<CoarseLayout, String> {
    let (cw, ch, block) = (config.coarse_w, config.coarse_h, config.block);
    let mut slot_site: Vec<Option<usize>> = vec![None; cw * ch];
    let mut sites: Vec<Site> = Vec::new();
    for cy in 0..ch {
        for cx in 0..cw {
            if !kept[cy * cw + cx] {
                continue;
            }
            slot_site[cy * cw + cx] = Some(sites.len());
            sites.push(Site {
                center: IVec2::new(
                    (cx * block + block / 2) as i32,
                    (cy * block + block / 2) as i32,
                ),
                bounds: Rect2 {
                    min: [(cx * block) as i32, (cy * block) as i32],
                    max: [((cx + 1) * block) as i32, ((cy + 1) * block) as i32],
                },
            });
        }
    }

    // E and S edges only (each undirected link counted once); the neighbour's W/N view is reconstructed
    // in `expand_to_fine`. `if let Some(b)` subsumes the old `kept[neighbour]` guard (a slot has a site
    // iff it is kept).
    let mut adjacency: Vec<(usize, usize)> = Vec::new();
    let coarse_open = |cx: usize, cy: usize, dir: usize| coarse.cells[cy * cw + cx].open[dir];
    for cy in 0..ch {
        for cx in 0..cw {
            let Some(a) = slot_site[cy * cw + cx] else {
                continue;
            };
            if cx + 1 < cw && coarse_open(cx, cy, E) {
                if let Some(b) = slot_site[cy * cw + cx + 1] {
                    adjacency.push((a, b));
                }
            }
            if cy + 1 < ch && coarse_open(cx, cy, S) {
                if let Some(b) = slot_site[(cy + 1) * cw + cx] {
                    adjacency.push((a, b));
                }
            }
        }
    }

    // Spawn at the kept slot nearest the coarse centre (total_cmp → no NaN/unwrap).
    let center = Vec2::new(cw as f32 / 2.0, ch as f32 / 2.0);
    let spawn_slot = (0..cw * ch)
        .filter(|&i| kept[i])
        .min_by(|&a, &b| {
            let pa = Vec2::new((a % cw) as f32, (a / cw) as f32);
            let pb = Vec2::new((b % cw) as f32, (b / cw) as f32);
            (pa - center)
                .length_squared()
                .total_cmp(&(pb - center).length_squared())
        })
        .ok_or_else(|| {
            "dungeon generation produced zero rooms (largest component empty)".to_string()
        })?;
    let spawn_site = slot_site[spawn_slot]
        .ok_or_else(|| "spawn slot was not a kept site (unreachable)".to_string())?;

    Ok(CoarseLayout {
        width: cw * block,
        height: ch * block,
        sites,
        adjacency,
        spawn_site,
    })
}

/// Carve the fine grid from a topology-agnostic `CoarseLayout`: one room per site (type-driven size,
/// liminality jitter + expansion-to-touch), one corridor per adjacency edge, then doorway openings +
/// necking. Shared by both topologies. Byte-identical to the pre-refactor grid carve when handed a
/// `grid_layout` (the Step-0 golden gate). The carve RNG (`config.seed ^ 0xC0FFEE`, created by the
/// caller) is drawn only in the room pass, in site order — so the Grid path's draw sequence is unchanged.
fn expand_to_fine(
    layout: &CoarseLayout,
    config: &DungeonConfig,
    rng: &mut impl DetRng,
) -> (Vec<bool>, Vec<Region>, IVec2, Vec<u32>) {
    let (width, height) = (layout.width, layout.height);
    let mut walkable = vec![false; width * height];
    let t = 1.0 - config.liminality; // 0 at Backrooms (liminality 1), 1 at realistic (liminality 0)

    // Per-site incidence, derived from adjacency once (no RNG). `incident[si]` = (sort_dir, neighbour,
    // is_first), sorted by (dir, neighbour) to drive the opening/necking pass in the same N,E,S,W order the
    // pre-refactor four-direction scan used. `is_first` marks the FIRST endpoint of the edge (carve_corridor
    // runs the horizontal leg from the first endpoint, the vertical leg into the second), which the opening
    // pass needs to derive each doorway from the real L geometry. Each undirected edge contributes its
    // cardinal to one endpoint and the opposite to the other (socket-rule symmetry).
    let mut incident: Vec<Vec<(usize, usize, bool)>> = vec![Vec::new(); layout.sites.len()];
    for &(a, b) in &layout.adjacency {
        let (da, db) = corridor_exit_dirs(layout.sites[a].center, layout.sites[b].center);
        incident[a].push((da, b, true));
        incident[b].push((db, a, false));
    }
    for inc in &mut incident {
        inc.sort_by_key(|&(dir, nb, _)| (dir, nb));
    }

    // Room pass — one room per site (the only RNG-consuming pass; draws stay in site order). Size + type
    // are drawn from the config's weighted room-type table (Merrell 2011); the room is block-centred at
    // liminality 1.0 and slides off-centre + grows toward its linked edges as liminality drops.
    let mut regions: Vec<Region> = Vec::new();
    for (si, site) in layout.sites.iter().enumerate() {
        let (bmin, bmax) = (site.bounds.min, site.bounds.max);
        let (bw, bh) = ((bmax[0] - bmin[0]) as usize, (bmax[1] - bmin[1]) as usize);
        let max_side = bw.min(bh).saturating_sub(2); // keep a >=1-tile rock margin inside the block
        let (rw, rh, tag, expands) = pick_room(config, max_side, rng);
        let (bx, by) = (site.center.x, site.center.y);
        let cox = bmin[0] as usize + (bw - rw) / 2;
        let coy = bmin[1] as usize + (bh - rh) / 2;
        let ox = jitter_origin(cox, rw, bmin[0] as usize, bw, bx as usize, t, rng);
        let oy = jitter_origin(coy, rh, bmin[1] as usize, bh, by as usize, t, rng);

        // Expansion-to-touch (spacious types only, per `RoomType::expands`): a hall/large room grows toward
        // *all four* block edges — by fraction `t` of the gap — so it fills its slot and dominates as an
        // anchor space. Compact types don't grow, keeping their realistic drawn footprint so the size
        // hierarchy survives (tiny bathroom beside a sprawling hall). Growth is capped one cell short of the
        // block wall (a >=2-cell doorway gap always remains) and draws no RNG — liminality 1.0 (t=0) is a
        // no-op, and the block centre stays interior so every corridor still connects.
        let toward = |near: i32, cap: i32| near + ((cap - near) as f32 * t).round() as i32;
        let mut left = ox as i32;
        let mut right = (ox + rw) as i32;
        let mut top = oy as i32;
        let mut bot = (oy + rh) as i32;
        if expands {
            left = toward(left, bmin[0] + 1);
            right = toward(right, bmax[0] - 1);
            top = toward(top, bmin[1] + 1);
            bot = toward(bot, bmax[1] - 1);
        }
        let (ox, rw) = (left as usize, (right - left) as usize);
        let (oy, rh) = (top as usize, (bot - top) as usize);
        for y in oy..oy + rh {
            for x in ox..ox + rw {
                walkable[y * width + x] = true;
            }
        }

        // Corner-notching (shape complexity): bite chunks out of the room's corners so it reads as an
        // L / T / plus (6–12 corners) instead of a plain box. Draws RNG in site order like the rest of the
        // room pass, so replays stay deterministic; `None` (no `notch` in the config) leaves rooms rectangular.
        if let Some(nc) = &config.notch {
            notch_room(
                &mut walkable,
                width,
                ox,
                oy,
                rw,
                rh,
                bx as usize,
                by as usize,
                nc,
                rng,
            );
        }

        regions.push(Region {
            id: si as u32,
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

    // The room pass is complete, so this snapshot is exactly "floor that belongs to a room". Corridors are
    // carved site-centre to site-centre, which means their paths run straight *through* room interiors — so
    // "corridor cell" cannot be "floor outside a room rect". It is "floor the corridor pass added that the
    // room pass had not already set", and this is the only moment that distinction is observable.
    let room_floor = walkable.clone();
    let mut corridor_of = vec![NO_CORRIDOR; width * height];

    // Corridor pass — each adjacency edge draws its own width in `[corridor_width, corridor_width_max]`
    // (uniform, from the carve RNG in adjacency order) so passages vary from tight to broad instead of
    // being identical. The drawn width is stashed per unordered edge so the necking pass below can reuse
    // the exact same value. The draw always runs (one path); a collapsed range just yields a constant.
    let (cw_min, cw_max) = (
        config.corridor_width,
        config.corridor_width_max.unwrap_or(config.corridor_width),
    );
    let mut edge_width: HashMap<(usize, usize), usize> = HashMap::new();
    for (edge_idx, &(a, b)) in layout.adjacency.iter().enumerate() {
        let w = cw_min + rng.below(cw_max - cw_min + 1);
        edge_width.insert((a.min(b), a.max(b)), w);
        carve_corridor(
            &mut walkable,
            width,
            height,
            layout.sites[a].center,
            layout.sites[b].center,
            w,
        );
        // Claim every cell this edge just opened. The `NO_CORRIDOR` guard makes the FIRST edge to open a
        // cell its owner, so an overlap at a junction resolves deterministically in adjacency order rather
        // than by whichever edge happened to be carved last. `carve_corridor` is left untouched — it stays
        // a pure walkability carve, and the identity is recovered here by diffing against `room_floor`.
        for i in 0..walkable.len() {
            if walkable[i] && !room_floor[i] && corridor_of[i] == NO_CORRIDOR {
                corridor_of[i] = edge_idx as u32;
            }
        }
    }

    // Opening pass — record each region's adjacency + the interior wall cell where its corridor actually
    // meets the room (derived from the carved L geometry, not a dominant-axis guess), and neck the doorway
    // down to the centre lane. Iterated in sort order per site so openings/adjacency match the grid's
    // N,E,S,W scan. `derive_opening` is byte-identical to the old cell formula for axis-aligned edges.
    for (si, inc) in incident.iter().enumerate() {
        let sc = layout.sites[si].center;
        let rect = regions[si].rect;
        for &(_, nb, is_first) in inc {
            let (dir, cell) = derive_opening(sc, layout.sites[nb].center, rect, is_first);
            regions[si].adjacency.push(nb as u32);
            regions[si].openings.push(Opening { dir, cell });
            // Same width the corridor was carved at (looked up per unordered edge), so the doorway necks
            // down from this corridor's real width — not a global constant — to the single centre lane.
            let cw = edge_width
                .get(&(si.min(nb), si.max(nb)))
                .copied()
                .unwrap_or(cw_min);
            for lane in 1..cw as i32 {
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

    let spawn = layout.sites[layout.spawn_site].center;
    // The necking pass above may have un-set cells that the corridor pass opened. Their `corridor_of` entry
    // survives, which is harmless: every read goes through `is_corridor`/`corridor_id`, and both gate on
    // `is_floor` first. A necked-out doorway cell is simply not floor, so it is not a corridor cell either.
    (walkable, regions, spawn, corridor_of)
}

/// Carve a `lanes`-wide corridor between two site centres. Axis-aligned edges (always, for the grid) are
/// a single straight run — lanes stack +y for a horizontal corridor, +x for a vertical one, byte-
/// identical to the pre-refactor E/S carve. Diagonal graph routes carve an L (both legs), keeping each
/// room-mouth segment axis-aligned so necking/openings still apply. Writes are bounds-checked (a no-op
/// for in-bounds grid corridors) so a wide/edge diagonal can never index out of the mask.
fn carve_corridor(
    walkable: &mut [bool],
    width: usize,
    height: usize,
    a: IVec2,
    b: IVec2,
    lanes: usize,
) {
    let carve_h = |walkable: &mut [bool], x0: i32, x1: i32, y: i32| {
        for x in x0.min(x1)..=x0.max(x1) {
            for lane in 0..lanes as i32 {
                let (px, py) = (x, y + lane);
                if px >= 0 && py >= 0 && (px as usize) < width && (py as usize) < height {
                    walkable[py as usize * width + px as usize] = true;
                }
            }
        }
    };
    let carve_v = |walkable: &mut [bool], y0: i32, y1: i32, x: i32| {
        for y in y0.min(y1)..=y0.max(y1) {
            for lane in 0..lanes as i32 {
                let (px, py) = (x + lane, y);
                if px >= 0 && py >= 0 && (px as usize) < width && (py as usize) < height {
                    walkable[py as usize * width + px as usize] = true;
                }
            }
        }
    };
    if a.y == b.y {
        carve_h(walkable, a.x, b.x, a.y);
    } else if a.x == b.x {
        carve_v(walkable, a.y, b.y, a.x);
    } else {
        // Diagonal (graph only): an L via the corner (b.x, a.y) — the horizontal leg leaves `a`'s wall,
        // the vertical leg enters `b`'s wall, both axis-aligned. The grid never reaches this branch.
        carve_h(walkable, a.x, b.x, a.y);
        carve_v(walkable, a.y, b.y, b.x);
    }
}

// ── Graph topology front-end ─────────────────────────────────────────────────────────────────────
// Poisson-disk sites → Bowyer–Watson Delaunay → degree-≤5 prune (`geom`) → `wfc::collapse_graph` decides
// which edges are corridors → keep the largest linked component → a `CoarseLayout` for `expand_to_fine`.

/// Build the Graph topology's `CoarseLayout`. Fails loud (one path) if the sites are too sparse to
/// sample or no collapse connects at least half of them (a rock-heavy roll can strand rooms).
fn graph_layout(
    config: &DungeonConfig,
    site_spacing: f32,
    link_weights: &[f64],
) -> Result<CoarseLayout, String> {
    let (width, height) = (
        config.coarse_w * config.block,
        config.coarse_h * config.block,
    );

    // Poisson sites, from their own RNG sub-stream (independent of the carve RNG). The rect is inset by
    // a small margin so every site sits at least `ROOM_FLOOR + 1` tiles from the level edge — that lets
    // `build_graph_layout` give each site a *symmetric*, in-bounds bounds box, which keeps `site.center`
    // interior to its room (the invariant the corridor/opening pass depends on).
    let margin = (ROOM_FLOOR + 1) as f64;
    let (inset_w, inset_h) = (
        (width as f64 - 2.0 * margin).max(1.0),
        (height as f64 - 2.0 * margin).max(1.0),
    );
    let mut site_rng = seeded(config.seed ^ 0x517E_5EED);
    let mut points = geom::poisson_disk(inset_w, inset_h, site_spacing as f64, 30, &mut site_rng);
    for p in &mut points {
        p[0] += margin;
        p[1] += margin;
    }
    let n = points.len();
    if n < 2 {
        return Err(format!(
            "graph topology: Poisson sampling produced {n} site(s) — site_spacing {site_spacing} is \
             too large for the {width}x{height} level"
        ));
    }

    // Delaunay graph, pruned to the collapse's degree cap, as port-indexed adjacency.
    let edges = geom::prune_to_max_degree(&points, &geom::delaunay_edges(&points), wfc::MAX_DEGREE);
    let neighbors = port_neighbors(n, &edges);

    // Collapse which edges are corridors, retrying with offset seeds until the largest linked component
    // covers at least half the sites — else fail loud rather than ship a mostly-isolated dungeon.
    let need = n.div_ceil(2).max(1);
    for attempt in 0..config.max_attempts.max(1) {
        let seed = (config.seed ^ 0xC011_AB5E).wrapping_add(attempt as u64);
        let Some(pattern) = wfc::collapse_graph(&neighbors, link_weights, seed) else {
            continue; // only a malformed table returns None; the prune guarantees it won't
        };
        let links = corridor_edges(&neighbors, &pattern);
        let kept = largest_graph_component(n, &links);
        if kept.len() >= need {
            return Ok(build_graph_layout(&points, &links, &kept, width, height));
        }
    }
    Err(format!(
        "graph topology: no collapse connected at least {need} of {n} sites after {} attempts; \
         raise link_weights or lower site_spacing",
        config.max_attempts.max(1)
    ))
}

/// Turn an undirected edge set into port-indexed adjacency for `wfc::collapse_graph`: each node's
/// neighbours sorted (a deterministic port order), every edge present at both endpoints with swapped
/// ports so the socket rule can pair them.
fn port_neighbors(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<(usize, usize)>> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        adj[a].push(b);
        adj[b].push(a);
    }
    for nb in &mut adj {
        nb.sort_unstable();
        nb.dedup();
    }
    let mut neighbors: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for a in 0..n {
        for &b in &adj[a] {
            // `a`'s port to `b` is `b`'s slot in `adj[a]` (push order); `b`'s back-port is `a`'s slot in
            // `adj[b]`. `position` is always `Some` since `adj` is symmetric.
            if let Some(b_port) = adj[b].iter().position(|&x| x == a) {
                neighbors[a].push((b, b_port));
            }
        }
    }
    neighbors
}

/// The undirected corridor edges implied by a collapse result: `a`'s port `p → b` is a corridor iff bit
/// `p` of `pattern[a]` is set (the socket rule guarantees `b` agrees). Counted once (`a < b`).
fn corridor_edges(neighbors: &[Vec<(usize, usize)>], pattern: &[usize]) -> Vec<(usize, usize)> {
    let mut edges = Vec::new();
    for (a, ports) in neighbors.iter().enumerate() {
        for (p, &(b, _)) in ports.iter().enumerate() {
            if (pattern[a] >> p) & 1 == 1 && a < b {
                edges.push((a, b));
            }
        }
    }
    edges
}

/// The largest connected component of a site graph (given its corridor edges), as a sorted node list.
/// Isolated sites are size-1 components, so the largest linked cluster wins — the graph analogue of
/// `largest_room_component`.
fn largest_graph_component(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut visited = vec![false; n];
    let mut best: Vec<usize> = Vec::new();
    for start in 0..n {
        if visited[start] {
            continue;
        }
        let mut comp = Vec::new();
        let mut stack = vec![start];
        visited[start] = true;
        while let Some(u) = stack.pop() {
            comp.push(u);
            for &v in &adj[u] {
                if !visited[v] {
                    visited[v] = true;
                    stack.push(v);
                }
            }
        }
        if comp.len() > best.len() {
            best = comp;
        }
    }
    best.sort_unstable();
    best
}

/// Assemble a `CoarseLayout` from the kept sites. Each site's centre rounds to the fine grid; its bounds
/// are a square, symmetric around the centre, sized to the smaller of half the nearest-neighbour
/// Chebyshev distance (so rooms provably never overlap — `h_i + h_j ≤ Cheb(i,j)`) and the distance to the
/// nearest level edge (so it stays in-bounds without breaking symmetry — the centre stays interior).
/// Adjacency is remapped to kept indices; spawn is the kept site nearest the level centre.
fn build_graph_layout(
    points: &[Point],
    links: &[(usize, usize)],
    kept: &[usize],
    width: usize,
    height: usize,
) -> CoarseLayout {
    let mut old_to_new = vec![None; points.len()];
    for (new_i, &old) in kept.iter().enumerate() {
        old_to_new[old] = Some(new_i);
    }

    let sites: Vec<Site> = kept
        .iter()
        .map(|&old| {
            let c = points[old];
            let mut min_cheb = f64::MAX;
            for &other in kept {
                if other != old {
                    let o = points[other];
                    min_cheb = min_cheb.min((o[0] - c[0]).abs().max((o[1] - c[1]).abs()));
                }
            }
            let (cx, cy) = (c[0].round() as i32, c[1].round() as i32);
            // Symmetric half-side: the smaller of half the nearest-neighbour Chebyshev distance (so no two
            // rooms overlap — `h_i + h_j ≤ Cheb(i,j)`) and the distance to the nearest level edge (so the
            // box stays symmetric around the centre AND in-bounds — keeping `site.center` interior to any
            // room centred in it, which the corridor pass relies on). A lone kept site has
            // `min_cheb == f64::MAX`; the edge terms bound it to a finite box, so there is no `i32`
            // overflow. The Poisson inset guarantees `edge ≥ ROOM_FLOOR`, so `.max(ROOM_FLOOR)` never
            // pushes the box out of bounds.
            let edge = cx.min(width as i32 - cx).min(cy).min(height as i32 - cy);
            let h = ((0.5 * min_cheb).min(edge as f64) as i32).max(ROOM_FLOOR as i32);
            Site {
                center: IVec2::new(cx, cy),
                bounds: Rect2 {
                    min: [cx - h, cy - h],
                    max: [cx + h, cy + h],
                },
            }
        })
        .collect();

    let mut adjacency: Vec<(usize, usize)> = Vec::new();
    for &(a, b) in links {
        if let (Some(na), Some(nb)) = (old_to_new[a], old_to_new[b]) {
            adjacency.push((na, nb));
        }
    }

    let center = Vec2::new(width as f32 / 2.0, height as f32 / 2.0);
    let spawn_site = (0..sites.len())
        .min_by(|&a, &b| {
            let pa = Vec2::new(sites[a].center.x as f32, sites[a].center.y as f32);
            let pb = Vec2::new(sites[b].center.x as f32, sites[b].center.y as f32);
            (pa - center)
                .length_squared()
                .total_cmp(&(pb - center).length_squared())
        })
        .unwrap_or(0); // kept is non-empty (need >= 1), so always Some

    CoarseLayout {
        width,
        height,
        sites,
        adjacency,
        spawn_site,
    }
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
/// a rock margin. Returns `(width, depth, type_tag, expands)` — `expands` is the type's spacious flag,
/// which drives the all-edge expansion in `expand_to_fine`.
fn pick_room(
    config: &DungeonConfig,
    max_side: usize,
    rng: &mut impl DetRng,
) -> (usize, usize, String, bool) {
    let ty = weighted_room_type(&config.room_types, rng);
    let area = rand_range_f32(rng, ty.area_min, ty.area_max);
    let aspect = rand_range_f32(rng, ty.aspect_min, ty.aspect_max);
    let long = (area * aspect).sqrt();
    let short = (area / aspect).sqrt();
    // Randomly orient the long axis to x or y so rooms aren't all landscape.
    let (w_f, h_f) = if rng.unit() < 0.5 {
        (long, short)
    } else {
        (short, long)
    };
    let cap = max_side.max(ROOM_FLOOR); // guard clamp's min <= max even for a tiny block
    let rw = (w_f.round() as usize).clamp(ROOM_FLOOR, cap);
    let rh = (h_f.round() as usize).clamp(ROOM_FLOOR, cap);
    (rw, rh, ty.tag.clone(), ty.expands)
}

/// Cut rectangular bites from a filled room's corners, turning it into a rectilinear polygon (L / T / Z /
/// U / plus — 6 to 12 corners) by clearing cells in `walkable`. `(bx, by)` is the block-centre cell, which
/// is interior to the room; every notch stays strictly inside its own corner quadrant relative to
/// `(bx, by)`, so the centre cross (row `by`, column `bx`) is never cut — the room stays connected, the
/// block-centre corridor still meets floor, and `derive_opening` is unaffected. Deterministic: it draws
/// the chance roll, the corner count, a Fisher–Yates corner order (three swaps, always), then one depth per
/// cut — a fixed sequence for a given room size, so replays stay byte-stable.
#[allow(clippy::too_many_arguments)]
fn notch_room(
    walkable: &mut [bool],
    width: usize,
    ox: usize,
    oy: usize,
    rw: usize,
    rh: usize,
    bx: usize,
    by: usize,
    cfg: &NotchConfig,
    rng: &mut impl DetRng,
) {
    if rw.min(rh) < cfg.min_side || rng.unit() >= cfg.chance {
        return; // too small to shape, or the chance roll declined — stays a clean rectangle
    }
    let count = 1 + rng.below(cfg.max_corners); // 1..=max_corners distinct corners
                                                // Fisher–Yates over the four corners (0=NW,1=NE,2=SW,3=SE); always three swaps so the draw count is
                                                // independent of `count`, then take the first `count`.
    let mut order = [0usize, 1, 2, 3];
    for i in (1..4).rev() {
        order.swap(i, rng.below(i + 1));
    }
    for &corner in order.iter().take(count) {
        let depth = cfg.depth_min + rng.unit() as f32 * (cfg.depth_max - cfg.depth_min);
        let (is_left, is_top) = (corner == 0 || corner == 2, corner == 0 || corner == 1);
        // Available quadrant between the room edge and the centre cross (exclusive of the centre row/col).
        let avail_w = if is_left { bx - ox } else { (ox + rw - 1) - bx };
        let avail_h = if is_top { by - oy } else { (oy + rh - 1) - by };
        if avail_w == 0 || avail_h == 0 {
            continue; // room hugs the centre on this side — no room for a corner bite here
        }
        let nw = ((depth * avail_w as f32).round() as usize).clamp(1, avail_w);
        let nh = ((depth * avail_h as f32).round() as usize).clamp(1, avail_h);
        let x0 = if is_left { ox } else { ox + rw - nw };
        let y0 = if is_top { oy } else { oy + rh - nh };
        for y in y0..y0 + nh {
            for x in x0..x0 + nw {
                walkable[y * width + x] = false;
            }
        }
    }
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

/// Slide a room's centred origin off-centre within its block for the liminality dial (Phase 2). `t` is
/// `1 - liminality`: at `t = 0` (liminality 1.0) the room stays centred — the shipped Backrooms grid,
/// and *no RNG is drawn*, so that layout is byte-identical to the pre-dial carve. As `t` grows the room
/// slides by up to `t` of its available slack, chosen from the carve RNG. The slack is bounded by two
/// rules: keep a >=1-tile rock margin inside the block, and keep the block centre (`bcenter`) at least
/// one cell inside the room walls — so the block-centre corridor lane is always interior floor and every
/// corridor still connects, with no change to the corridor carve. Rooms stay axis-aligned rectangles.
fn jitter_origin(
    centered: usize,
    room: usize,
    block_start: usize,
    block: usize,
    bcenter: usize,
    t: f32,
    rng: &mut impl DetRng,
) -> usize {
    if t <= 0.0 {
        return centered;
    }
    let (c, r, bs, blk, bc) = (
        centered as i64,
        room as i64,
        block_start as i64,
        block as i64,
        bcenter as i64,
    );
    let lo = (bs + 1).max(bc - r + 2);
    let hi = (bs + blk - r - 1).min(bc - 1);
    if hi <= lo {
        return centered; // no slack (a small room hugging the block centre) → stay centred
    }
    // Window of half-widths `t·(c-lo)` left and `t·(hi-c)` right, both within [lo, hi] since t <= 1.
    let left = (((c - lo) as f32) * t).round() as i64;
    let right = (((hi - c) as f32) * t).round() as i64;
    ((c - left) + rng.below((left + right + 1) as usize) as i64) as usize
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
    )
    else {
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
                          wall_size: Option<Vec3>,
                          cutaway: Cutaway| {
        // The knee-wall cutaway is view-relative (see `update_cutaway`). We only *seed* the pose here for
        // the opening yaw=0 view (camera from +X,+Z ⇒ E/S near); the per-frame system re-poses it from
        // the live camera direction. A `Wall` (squashed) reseats to knee height on its near edge; a
        // `Mounted` decoration (doorway lintel) hides — scale 0 — on its near edge so it never floats.
        let outward = match cutaway {
            Cutaway::None => Vec3::ZERO,
            Cutaway::Wall | Cutaway::Mounted => {
                let center = Vec3::new(cell.x as f32 * TILE_SIZE, 0.0, cell.y as f32 * TILE_SIZE);
                wall_outward(transform.translation, center)
            }
        };
        if SHORT_CAMERA_WALLS && faces_camera(outward, Vec3::new(1.0, 0.0, 1.0)) {
            match cutaway {
                Cutaway::Wall => {
                    let (scale_y, y) = wall_pose(true);
                    transform.scale.y = scale_y;
                    transform.translation.y = y;
                }
                Cutaway::Mounted => transform.scale = Vec3::ZERO,
                Cutaway::None => {}
            }
        }
        let mut entity = commands.spawn((
            Tile { cell },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            Visibility::Hidden,
        ));
        // Both walls and lintels carry `Wall` so the fog reveal treats them as walls (not floors); only
        // solid walls get a physics collider (lintels are cosmetic — gibs pass under the ceiling beam).
        if matches!(cutaway, Cutaway::Wall | Cutaway::Mounted) {
            entity.insert(Wall);
        }
        if let Some(size) = wall_size {
            // avian `Collider::cuboid` takes FULL side lengths; the wall mesh is an origin-centred
            // `Cuboid` of the same size, so the collider lines up exactly under the transform.
            entity.insert((RigidBody::Static, Collider::cuboid(size.x, size.y, size.z)));
        }
        match cutaway {
            Cutaway::Wall => {
                entity.insert(CutawayWall { outward });
            }
            Cutaway::Mounted => {
                entity.insert(CutawayMounted {
                    outward,
                    base_scale: Vec3::ONE,
                });
            }
            Cutaway::None => {}
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
                Cutaway::None,
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
                    spawn_tile(
                        cell,
                        wall_full.clone(),
                        wall_mat.clone(),
                        full,
                        Some(full_size),
                        Cutaway::Wall,
                    );
                    spawn_tile(
                        cell,
                        wall_short.clone(),
                        wall_mat.clone(),
                        short,
                        Some(short_size),
                        Cutaway::Wall,
                    );
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
                        Cutaway::Wall,
                    );
                }
            }
        }
    }

    // Doorway lintels: a short wall header above each 1-tile doorway (region opening), so the wall
    // reads as one continuous run from above — the door tucks under it. Placed on the doorway cell's
    // open wall edge, raised to `DOORWAY_HEIGHT`. A lintel is a `Cutaway::Mounted` decoration: it shows
    // only while its wall is far/full and hides (scale 0) when that wall becomes a near knee wall, so it
    // never floats in the cutaway gap. All four edges are spawned (the pose seeds E/S hidden at yaw=0);
    // rotation reveals the pair on whichever wall is currently full-height.
    let header_size = Vec3::new(WALL_THICKNESS, WALL_HEIGHT - DOORWAY_HEIGHT, TILE_SIZE);
    let header_mesh = meshes.add(wall_mesh(header_size));
    for region in &dungeon.regions {
        for op in &region.openings {
            let cell = IVec2::new(op.cell[0], op.cell[1]);
            spawn_tile(
                cell,
                header_mesh.clone(),
                wall_mat.clone(),
                header_wall(cell, op.dir),
                None,
                Cutaway::Mounted,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Corridor identity is a partition of floor: every cell is room floor XOR corridor floor, never both,
    /// and rock is neither. This is the invariant `mycelia::habitat` leans on to keep patches out of halls.
    #[test]
    fn corridor_cells_are_floor_and_never_room_floor() {
        let d = Dungeon::generate(&test_config()).expect("test config generates");
        let mut corridor_cells = 0usize;
        for y in 0..d.height as i32 {
            for x in 0..d.width as i32 {
                let c = IVec2::new(x, y);
                match d.corridor_id(c) {
                    Some(_) => {
                        assert!(d.is_floor(c), "corridor cell {c:?} must be floor");
                        corridor_cells += 1;
                    }
                    None => {}
                }
                // Rock is never a corridor, whatever the carve left painted underneath it.
                if !d.is_floor(c) {
                    assert!(!d.is_corridor(c), "rock {c:?} reported as corridor");
                }
            }
        }
        assert!(corridor_cells > 0, "a connected dungeon must have corridor floor");
    }

    /// The corridor painting must be a pure function of the seed, like every other carve decision.
    #[test]
    fn corridor_identity_is_deterministic() {
        let a = Dungeon::generate(&test_config()).expect("generates");
        let b = Dungeon::generate(&test_config()).expect("generates");
        assert_eq!(a.corridor_of, b.corridor_of, "same seed must paint the same runs");
    }

    /// A small, valid config for generation tests (avoids depending on the shipped RON's exact values).
    fn test_config() -> DungeonConfig {
        DungeonConfig {
            coarse_w: 4,
            coarse_h: 4,
            block: 16,
            corridor_width: 2,
            corridor_width_max: Some(4),
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
                RoomType {
                    tag: "bathroom".into(),
                    area_min: 3.0,
                    area_max: 6.0,
                    aspect_min: 1.0,
                    aspect_max: 1.6,
                    weight: 0.8,
                    expands: false,
                },
                RoomType {
                    tag: "bedroom".into(),
                    area_min: 9.0,
                    area_max: 20.0,
                    aspect_min: 1.0,
                    aspect_max: 1.5,
                    weight: 1.5,
                    expands: false,
                },
                RoomType {
                    tag: "living".into(),
                    area_min: 16.0,
                    area_max: 40.0,
                    aspect_min: 1.0,
                    aspect_max: 1.7,
                    weight: 1.6,
                    expands: true,
                },
            ],
            notch: None,
            topology: Topology::Grid,
        }
    }

    #[test]
    fn shipped_config_parses_and_generates() {
        // The shipped assets/config/config.ron `dungeon:` slice must parse, validate, and generate a
        // non-empty dungeon (loaded + validated through the unified loader, one path).
        let config = crate::config::load_game_config()
            .expect("shipped config.ron must be valid")
            .dungeon;
        let d = Dungeon::generate(&config).expect("must generate at least one room");
        assert!(!d.regions.is_empty(), "shipped config must produce rooms");
        assert!(d.walkable.iter().any(|&w| w), "dungeon must have floor");
    }

    #[test]
    fn notching_carves_deterministic_non_rectangular_rooms() {
        // Rooms big enough to notch: one spacious type that fills its block at liminality 0, with notching
        // forced on for all four corners. Every room should become a rectilinear polygon (a plus/cross).
        let mut cfg = test_config();
        cfg.liminality = 0.0;
        cfg.room_types = vec![RoomType {
            tag: "hall".into(),
            area_min: 60.0,
            area_max: 120.0,
            aspect_min: 1.0,
            aspect_max: 1.4,
            weight: 1.0,
            expands: true,
        }];
        cfg.notch = Some(NotchConfig {
            chance: 1.0,
            max_corners: 4,
            depth_min: 0.4,
            depth_max: 0.5,
            min_side: 4,
        });

        let a = Dungeon::generate(&cfg).expect("gen a");
        let b = Dungeon::generate(&cfg).expect("gen b");
        assert_eq!(
            a.walkable, b.walkable,
            "notching must be deterministic for a (config, seed)"
        );

        // A plain rectangle has every bounding-box cell as floor; a notched room has non-floor bites in
        // its bbox. At least one room must be non-rectangular.
        let non_rect = a.regions.iter().filter(|r| {
            let (mn, mx) = (r.rect.min, r.rect.max);
            (mn[1]..mx[1]).any(|y| (mn[0]..mx[0]).any(|x| !a.is_floor(IVec2::new(x, y))))
        });
        assert!(
            non_rect.count() > 0,
            "expected at least one notched (non-rectangular) room"
        );
    }

    #[test]
    fn notching_never_severs_a_room_from_its_corridors() {
        // The notch invariant: the block-centre cross is never cut, so every walkable cell stays reachable.
        // A full flood-fill from the spawn must still cover every floor cell (no notch orphans a region).
        let mut cfg = test_config();
        cfg.notch = Some(NotchConfig {
            chance: 1.0,
            max_corners: 4,
            depth_min: 0.4,
            depth_max: 0.6,
            min_side: 4,
        });
        let d = Dungeon::generate(&cfg).expect("gen");
        let idx = |c: IVec2| (c.y as usize) * d.width + c.x as usize;
        let mut seen = vec![false; d.width * d.height];
        let mut stack = vec![d.spawn];
        seen[idx(d.spawn)] = true;
        while let Some(c) = stack.pop() {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let n = IVec2::new(c.x + dx, c.y + dy);
                if n.x >= 0
                    && n.y >= 0
                    && (n.x as usize) < d.width
                    && (n.y as usize) < d.height
                    && d.is_floor(n)
                    && !seen[idx(n)]
                {
                    seen[idx(n)] = true;
                    stack.push(n);
                }
            }
        }
        let unreached = (0..d.width * d.height)
            .filter(|&i| d.walkable[i] && !seen[i])
            .count();
        assert_eq!(
            unreached, 0,
            "notching orphaned {unreached} floor cells from the spawn"
        );
    }

    #[test]
    fn generate_is_deterministic_for_a_config() {
        let config = test_config();
        let a = Dungeon::generate(&config).expect("gen a");
        let b = Dungeon::generate(&config).expect("gen b");
        assert_eq!(
            a.walkable, b.walkable,
            "same (config, seed) → same walkable mask"
        );
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
            assert!(
                r.props.has("room"),
                "region {} missing base 'room' tag",
                r.id
            );
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
            assert!(
                w >= ROOM_FLOOR as i32 && w <= max_side,
                "room width {w} out of range"
            );
            assert!(
                h >= ROOM_FLOOR as i32 && h <= max_side,
                "room height {h} out of range"
            );
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
        assert!(
            parse_config(bad_liminality).is_err(),
            "liminality > 1 must be rejected"
        );
        let empty_types = r#"(coarse_w:6,coarse_h:6,block:32,corridor_width:2,seed:1,max_attempts:20,
            liminality:1.0,wfc_weights:(rock:6.0,dead_end:1.2,corridor:2.5,corner:2.5,tee:1.2,cross:0.6),
            room_types:[])"#;
        assert!(
            parse_config(empty_types).is_err(),
            "empty room_types must be rejected"
        );
    }

    #[test]
    fn config_validation_rejects_bad_wfc_weights() {
        // The grid WFC weights feed `collapse_one`; a NaN, a negative, a zero sum, or an all-rock
        // (floorless) distribution must fail at the door rather than silently degenerate the dungeon.
        let with_wfc = |wfc: &str| {
            format!(
                r#"(coarse_w:6,coarse_h:6,block:32,corridor_width:2,seed:1,max_attempts:20,
                liminality:1.0,wfc_weights:{wfc},
                room_types:[(tag:"a",area_min:3.0,area_max:6.0,aspect_min:1.0,aspect_max:1.6,weight:1.0)])"#
            )
        };
        assert!(
            parse_config(&with_wfc(
                "(rock:6.0,dead_end:1.2,corridor:NaN,corner:2.5,tee:1.2,cross:0.6)"
            ))
            .is_err(),
            "a NaN wfc_weight must be rejected"
        );
        assert!(
            parse_config(&with_wfc(
                "(rock:6.0,dead_end:1.2,corridor:-2.5,corner:2.5,tee:1.2,cross:0.6)"
            ))
            .is_err(),
            "a negative wfc_weight must be rejected"
        );
        assert!(
            parse_config(&with_wfc(
                "(rock:0.0,dead_end:0.0,corridor:0.0,corner:0.0,tee:0.0,cross:0.0)"
            ))
            .is_err(),
            "a zero-sum wfc_weights must be rejected"
        );
        assert!(
            parse_config(&with_wfc(
                "(rock:6.0,dead_end:0.0,corridor:0.0,corner:0.0,tee:0.0,cross:0.0)"
            ))
            .is_err(),
            "an all-rock (floorless) wfc_weights must be rejected"
        );
        assert!(
            parse_config(&with_wfc(
                "(rock:6.0,dead_end:1.2,corridor:2.5,corner:2.5,tee:1.2,cross:0.6)"
            ))
            .is_ok(),
            "a valid wfc_weights distribution must parse"
        );
    }

    #[test]
    fn liminality_1_centers_rooms() {
        // At liminality 1.0 (t=0) jitter_origin is a no-op: every room stays block-centred (the shipped
        // grid). ox = cx*block + (block-rw)/2, so ox % block == (block-rw)/2.
        let mut config = test_config();
        config.liminality = 1.0;
        let block = config.block;
        let d = Dungeon::generate(&config).expect("gen");
        for r in &d.regions {
            let w = r.rect.width() as usize;
            let h = r.rect.height() as usize;
            assert_eq!(
                r.rect.min[0] as usize % block,
                (block - w) / 2,
                "room not x-centred"
            );
            assert_eq!(
                r.rect.min[1] as usize % block,
                (block - h) / 2,
                "room not y-centred"
            );
        }
    }

    #[test]
    fn liminality_0_still_generates_connected_rooms() {
        // At liminality 0.0 (max jitter) generation still succeeds with rooms + floor — the jitter is
        // bounded to keep each block centre interior, so corridors still connect. And at least one room
        // slides off its centred position, so the dial demonstrably did something.
        let mut config = test_config();
        config.liminality = 0.0;
        let block = config.block;
        let d = Dungeon::generate(&config).expect("gen at liminality 0");
        assert!(!d.regions.is_empty());
        assert!(d.walkable.iter().any(|&w| w), "must have floor");
        let any_offset = d.regions.iter().any(|r| {
            let w = r.rect.width() as usize;
            let h = r.rect.height() as usize;
            r.rect.min[0] as usize % block != (block - w) / 2
                || r.rect.min[1] as usize % block != (block - h) / 2
        });
        assert!(
            any_offset,
            "liminality 0 should slide at least one room off-centre"
        );
    }

    #[test]
    fn liminality_0_rooms_never_overlap() {
        // Expansion-to-touch grows rooms toward their links, but each stays within its own block, so no
        // two rooms ever overlap — a safety net on the extension math at maximum growth.
        let mut config = test_config();
        config.liminality = 0.0;
        let d = Dungeon::generate(&config).expect("gen");
        let overlaps = |a: &Rect2, b: &Rect2| {
            a.min[0] < b.max[0] && b.min[0] < a.max[0] && a.min[1] < b.max[1] && b.min[1] < a.max[1]
        };
        for (i, a) in d.regions.iter().enumerate() {
            for b in &d.regions[i + 1..] {
                assert!(
                    !overlaps(&a.rect, &b.rect),
                    "regions {} and {} overlap",
                    a.id,
                    b.id
                );
            }
        }
    }

    // ---- Phase 3 Step 5: Graph topology (Poisson + Delaunay + collapse_graph) integration ----------

    /// A `Topology::Graph` config over a 96×96 level (~40 Poisson sites at spacing 14).
    fn graph_test_config() -> DungeonConfig {
        let mut c = test_config();
        c.coarse_w = 6;
        c.coarse_h = 6;
        c.block = 16;
        c.topology = Topology::Graph {
            site_spacing: 14.0,
            link_weights: [0.05, 1.2, 2.5, 1.2, 0.6, 0.6],
        };
        c
    }

    /// Flood-fill `d.walkable` (4-connected) from `d.spawn`, returning the reached-cell mask.
    fn reachable_from_spawn(d: &Dungeon) -> Vec<bool> {
        let (w, h) = (d.width, d.height);
        let mut seen = vec![false; w * h];
        let start = d.spawn.y as usize * w + d.spawn.x as usize;
        assert!(d.walkable[start], "spawn must be on a walkable cell");
        seen[start] = true;
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                    let ni = ny as usize * w + nx as usize;
                    if d.walkable[ni] && !seen[ni] {
                        seen[ni] = true;
                        stack.push(ni);
                    }
                }
            }
        }
        seen
    }

    /// Assert every region has at least one interior floor cell reachable from spawn — true *geometric*
    /// connectivity over the carved `walkable` mask, not just the logical `region.adjacency` graph (which
    /// is connected by construction and so cannot catch a room whose corridor misses its rect).
    fn assert_all_regions_reachable(d: &Dungeon) {
        let seen = reachable_from_spawn(d);
        let w = d.width;
        for r in &d.regions {
            let reached = (r.rect.min[1]..r.rect.max[1])
                .any(|y| (r.rect.min[0]..r.rect.max[0]).any(|x| seen[y as usize * w + x as usize]));
            assert!(reached, "region {} has no floor reachable from spawn", r.id);
        }
    }

    fn assert_no_overlap(d: &Dungeon) {
        let overlaps = |a: &Rect2, b: &Rect2| {
            a.min[0] < b.max[0] && b.min[0] < a.max[0] && a.min[1] < b.max[1] && b.min[1] < a.max[1]
        };
        for (i, a) in d.regions.iter().enumerate() {
            for b in &d.regions[i + 1..] {
                assert!(
                    !overlaps(&a.rect, &b.rect),
                    "graph regions {} and {} overlap",
                    a.id,
                    b.id
                );
            }
        }
    }

    #[test]
    fn graph_topology_generates_connected_non_overlapping_rooms() {
        let config = graph_test_config();
        let d = Dungeon::generate(&config).expect("graph topology must generate");
        assert!(!d.regions.is_empty(), "graph must produce rooms");
        assert!(d.walkable.iter().any(|&w| w), "graph must have floor");
        assert_no_overlap(&d);
        assert_all_regions_reachable(&d);
    }

    #[test]
    fn graph_topology_generates_at_liminality_0() {
        // Exercise the graph carve's RNG-drawing geometry (room jitter + expansion-to-touch, t = 1) —
        // the grid golden covers liminality 0.0, but the other graph tests run only at t = 0.
        let mut config = graph_test_config();
        config.liminality = 0.0;
        let d = Dungeon::generate(&config).expect("graph must generate at liminality 0");
        assert!(!d.regions.is_empty() && d.walkable.iter().any(|&x| x));
        assert_no_overlap(&d);
        assert_all_regions_reachable(&d);
    }

    // #4 regression + a target invariant for the deferred #5 work. Openings now land on the correct wall
    // for the L-route (the #4 `derive_opening` fix, incl. the L-corner-inside-room case) and every room is
    // reachable, but this stricter check — every doorway faces a real corridor mouth — still trips on the
    // known Graph limitation #5: a Delaunay node can have >4 neighbours (forced at degree 5), so two
    // corridors can share a wall and one's necking rocks the other's lane-0 mouth. That is doorway
    // cosmetics, not connectivity (see graph_topology_generates_connected...). Un-ignore once corridors
    // fan out along the wall / necking is coordinated across same-wall openings.
    #[test]
    #[ignore = "known Graph limitation #5: multiple corridors per wall can rock a lane-0 doorway mouth"]
    fn graph_openings_sit_on_real_corridor_mouths() {
        // Every recorded Opening must lie on its region's perimeter per its `dir`, AND the cell one step
        // OUT of the room must be walkable — i.e. the door faces the corridor the L-route actually carved.
        let config = graph_test_config();
        let d = Dungeon::generate(&config).expect("gen");
        let (w, h) = (d.width as i32, d.height as i32);
        for r in &d.regions {
            for o in &r.openings {
                let [cx, cy] = o.cell;
                match o.dir {
                    N => assert_eq!(
                        cy, r.rect.min[1],
                        "N opening off the N wall of region {}",
                        r.id
                    ),
                    S => assert_eq!(
                        cy,
                        r.rect.max[1] - 1,
                        "S opening off the S wall of region {}",
                        r.id
                    ),
                    E => assert_eq!(
                        cx,
                        r.rect.max[0] - 1,
                        "E opening off the E wall of region {}",
                        r.id
                    ),
                    W => assert_eq!(
                        cx, r.rect.min[0],
                        "W opening off the W wall of region {}",
                        r.id
                    ),
                    _ => unreachable!(),
                }
                let (ox, oy) = match o.dir {
                    N => (cx, cy - 1),
                    S => (cx, cy + 1),
                    E => (cx + 1, cy),
                    W => (cx - 1, cy),
                    _ => unreachable!(),
                };
                assert!(
                    ox >= 0 && oy >= 0 && ox < w && oy < h,
                    "opening mouth off-grid on region {}",
                    r.id
                );
                assert!(
                    d.walkable[oy as usize * d.width + ox as usize],
                    "region {} opening (dir {}) faces a wall, not a corridor",
                    r.id,
                    o.dir
                );
            }
        }
    }

    #[test]
    fn graph_topology_is_deterministic() {
        let config = graph_test_config();
        let a = Dungeon::generate(&config).expect("gen a");
        let b = Dungeon::generate(&config).expect("gen b");
        assert_eq!(
            a.walkable, b.walkable,
            "same graph config + seed → same walkable mask"
        );
        assert_eq!(a.spawn, b.spawn);
        assert_eq!(a.regions.len(), b.regions.len());
    }

    #[test]
    fn topology_defaults_to_grid_when_absent() {
        // The shipped config.ron `dungeon:` slice has no `topology` field → serde default → Grid.
        let config = crate::config::load_game_config().expect("valid").dungeon;
        assert!(matches!(config.topology, Topology::Grid), "absent topology must default to Grid");
    }

    #[test]
    fn graph_config_validation() {
        let base = r#"(coarse_w:6,coarse_h:6,block:16,corridor_width:2,seed:1,max_attempts:20,
            liminality:1.0,wfc_weights:(rock:6.0,dead_end:1.2,corridor:2.5,corner:2.5,tee:1.2,cross:0.6),
            room_types:[(tag:"a",area_min:3.0,area_max:6.0,aspect_min:1.0,aspect_max:1.6,weight:1.0)],"#;
        // NB: serde encodes `[f64; 6]` as a *tuple*, so RON writes `link_weights` with `(...)`, not `[...]`.
        let small = format!(
            "{base}topology:Graph(site_spacing:3.0,link_weights:(0.05,1.2,2.5,1.2,0.6,0.6)))"
        );
        assert!(
            parse_config(&small).is_err(),
            "site_spacing below the floor must be rejected"
        );
        let zero = format!(
            "{base}topology:Graph(site_spacing:14.0,link_weights:(0.0,0.0,0.0,0.0,0.0,0.0)))"
        );
        assert!(
            parse_config(&zero).is_err(),
            "zero-sum link_weights must be rejected"
        );
        let ok = format!(
            "{base}topology:Graph(site_spacing:14.0,link_weights:(0.05,1.2,2.5,1.2,0.6,0.6)))"
        );
        let cfg = parse_config(&ok).expect("valid graph config must parse");
        assert!(
            Dungeon::generate(&cfg).is_ok(),
            "valid graph config must generate"
        );
    }

    // ---- Dungeon carve golden: locks the full Grid carve output so unintended drift in geometry, RNG
    // draw order, or region-link order flips the hash. FNV-1a over the FULL Dungeon output (dims, walkable
    // mask, spawn, and every region's rect/tags/adjacency/openings). Uses `test_config()` (self-contained)
    // rather than the shipped RON so the gate is stable even while `assets/config/config.ron` is edited.
    // Order: liminality 1.0 for seeds [1,2,3], then liminality 0.0 for seeds [1,2,3] (1.0 draws zero
    // jitter RNG, so 0.0 must be covered too to exercise the `jitter_origin` draw path).
    // Re-pinned for the layout-diversity work: per-corridor width variation (`corridor_width_max`) and
    // type-aware expansion (`RoomType::expands`) deliberately change the carve — a legitimate worldgen
    // change with sign-off, not accidental drift.
    const GOLDEN_DUNGEON: [u64; 6] = [
        2568236482067835968,
        5241347363305519598,
        7950630862814937742,
        2581036281007484390,
        9682684589496540033,
        18361432492331364935,
    ];

    /// FNV-1a accumulator — deterministic across runs (unlike `DefaultHasher`'s per-process seed).
    struct Fnv(u64);
    impl Fnv {
        fn new() -> Self {
            Fnv(0xcbf2_9ce4_8422_2325)
        }
        fn push(&mut self, v: u64) {
            self.0 ^= v;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        fn push_str(&mut self, s: &str) {
            for b in s.bytes() {
                self.push(b as u64);
            }
            self.push(0xFF); // field separator
        }
    }

    fn fingerprint(d: &Dungeon) -> u64 {
        let mut f = Fnv::new();
        f.push(d.width as u64);
        f.push(d.height as u64);
        for &w in &d.walkable {
            f.push(w as u64);
        }
        f.push(d.spawn.x as u64);
        f.push(d.spawn.y as u64);
        f.push(d.regions.len() as u64);
        for r in &d.regions {
            f.push(r.id as u64);
            f.push(r.rect.min[0] as u64);
            f.push(r.rect.min[1] as u64);
            f.push(r.rect.max[0] as u64);
            f.push(r.rect.max[1] as u64);
            for t in &r.props.tags {
                f.push_str(t);
            }
            f.push(0xF0F0);
            for &a in &r.adjacency {
                f.push(a as u64);
            }
            f.push(0x0F0F);
            for o in &r.openings {
                f.push(o.dir as u64);
                f.push(o.cell[0] as u64);
                f.push(o.cell[1] as u64);
            }
        }
        f.0
    }

    fn golden_fingerprints() -> Vec<u64> {
        let base = test_config();
        let mut fps = Vec::new();
        for lim in [1.0f32, 0.0] {
            for seed in [1u64, 2, 3] {
                let mut cfg = base.clone();
                cfg.seed = seed;
                cfg.liminality = lim;
                let d = Dungeon::generate(&cfg).expect("golden config must generate");
                fps.push(fingerprint(&d));
            }
        }
        fps
    }

    #[test]
    fn golden_dungeon_snapshot_is_stable() {
        // Byte-identical gate for the Grid carve. If this fails after a change meant to be behaviour-
        // neutral, the carve drifted; if the change was intentional (a worldgen tweak), re-pin from the
        // printed value with sign-off.
        let fps = golden_fingerprints();
        println!("GOLDEN_DUNGEON = {fps:?}");
        assert_eq!(
            fps.as_slice(),
            &GOLDEN_DUNGEON,
            "dungeon carve output changed"
        );
    }

    /// Footprint-aware containment (README ISSUES 1 & 2): a piece is legal only when its whole body
    /// lies on floor, never when it overhangs a wall or a notched-out corner — the discrete
    /// `C_free` non-penetration test (Merrell et al. 2011).
    #[test]
    fn footprint_on_floor_rejects_wall_overhang() {
        // A 3×3 floor room (cells (1,1)..=(3,3)) walled in by rock in a 5×5 grid.
        let mut mask = vec![false; 5 * 5];
        for y in 1..4 {
            for x in 1..4 {
                mask[y * 5 + x] = true;
            }
        }
        let d = Dungeon::from_walkable(5, 5, mask);

        // A small piece dead-centre in the interior cell (2,2) is fully clear.
        assert!(d.footprint_on_floor(Vec3::new(2.0, 0.0, 2.0), Vec2::new(0.1, 0.1), 0.0));

        // A large piece at a corner cell (1,1) overhangs the N and W walls → rejected. Its old
        // center-only `is_floor` check would have wrongly accepted it.
        assert!(d.is_floor(IVec2::new(1, 1)));
        assert!(!d.footprint_on_floor(Vec3::new(1.0, 0.0, 1.0), Vec2::new(0.4, 0.4), 0.0));

        // A piece centred on a rock cell (outside the room) is rejected outright.
        assert!(!d.footprint_on_floor(Vec3::new(0.0, 0.0, 0.0), Vec2::new(0.1, 0.1), 0.0));

        // Quarter-turn: a long-thin piece at edge cell (2,1) (walled on N) clears at yaw 0 (long axis
        // runs along X, away from the wall) but overhangs once rotated 90° (long axis into the N wall).
        let half = Vec2::new(0.4, 0.1); // 0.8 (w) × 0.2 (d)
        assert!(d.footprint_on_floor(Vec3::new(2.0, 0.0, 1.0), half, 0.0));
        assert!(!d.footprint_on_floor(Vec3::new(2.0, 0.0, 1.0), half, std::f32::consts::FRAC_PI_2));
    }
}
