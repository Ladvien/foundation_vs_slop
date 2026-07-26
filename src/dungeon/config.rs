//! The `dungeon:` config slice and its validation: room types, WFC weights, notches, topology.
//! Pure data + a loud validator — a malformed slice fails at load, never silently defaults.
//! Split out of the former single-file `dungeon.rs` (3,447 lines) — a **pure move**, no logic
//! changed, so the replay goldens are untouched (FVS-N-1). `use super::*` at the top of each submodule
//! inherits the parent's imports, which is what keeps the move mechanical and reviewable: the diff is
//! whole items relocated, not hundreds of rewritten `use` lines.

use super::*;

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
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    /// Doorway width as a fraction of each corridor's carved width, in `(0, 1]`. A corridor carved `cw`
    /// tiles wide opens a doorway of `round(cw * doorway_ratio)` lanes, clamped to `1..=cw` (see
    /// [`doorway_width`]). At `1.0` the mouth is as wide as the corridor; small values pinch it toward a
    /// single lane. This is the evolvable knob behind the "every doorway is one body wide" fix — surfaced
    /// to the QD level search (`squad_ai::level_genome`). `#[serde(default)]` keeps older configs valid.
    #[serde(default = "default_doorway_ratio")]
    pub doorway_ratio: f32,
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
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
pub(crate) const ROOM_FLOOR: usize = 2;

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
    if !(cfg.doorway_ratio.is_finite() && cfg.doorway_ratio > 0.0 && cfg.doorway_ratio <= 1.0) {
        return Err(format!(
            "doorway_ratio must be in (0,1] (got {})",
            cfg.doorway_ratio
        ));
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
