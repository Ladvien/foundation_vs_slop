//! The generation↔runtime boundary: the one Bevy system that consumes solver `Outcome`s and spawns
//! entities. Everything upstream (IR, solvers, orchestrator) is engine-free; this is where placement
//! becomes GLBs in the world, spawned via `WorldAssetRoot` exactly like `crate::crab`/`crate::nest`.
//!
//! Per region, three passes:
//!   1. **Anchors** — deterministic host attachment (ceiling light at room centre, a door in every
//!      opening). An anchor's pose is a function of its host, so this is a direct pass, no search.
//!   2. **Tiled** — small floor props scattered by routing a `PlacementProblem` through the
//!      orchestrator → `WfcSolver` (Hard+Local).
//!   3. **Freestanding** — a room-profile set of furniture arranged by the orchestrator →
//!      `MetropolisSolver` (Soft+Relational): backs to walls, non-overlapping, sofa facing TV.
//!
//! The solve step is pure and per-region-independent, so it runs in parallel across regions with
//! `rayon`; only the ECS spawn afterwards is serial (Commands is not `Sync`). Determinism holds:
//! each region seeds its own `ChaCha8Rng` sub-stream, independent of thread order.

use std::collections::HashSet;
use std::f32::consts::PI;
use std::sync::Arc;

use bevy::prelude::*;
use rayon::prelude::*;

use crate::dungeon::{Dungeon, WALL_HEIGHT};
use crate::rng::{seeded, DetRng};

use super::ir::{
    Candidate, Constraint, Dof, Host, Modality, Outcome, Placement, PlacementProblem, Predicate,
    Region, RegionId, Role, Scope,
};
use super::manifest::{FurnitureManifest, ManifestItem};
use super::scatter;
use super::{splitmix64, PlacedIn, PlacementSolvers, PLACEMENT_SEED};

/// Global furniture scale. The kit is authored in real-world metres and the dungeon now has ~8 ft
/// (`WALL_HEIGHT` = 2.4 m) ceilings, so furniture renders at native 1:1 — a 2.05 m door and 2.15 m
/// shelf fit under the ceiling, sized to the ~6 ft squad. Applied to BOTH the solver footprints (so
/// layout reasons at the rendered size) and the spawn transform (so the GLB renders at it) — they
/// must agree.
const FURNITURE_SCALE: f32 = 1.0;

/// The per-room counts and spacing distances — how much furniture a room gets. Promoted out of module
/// constants into config (`crate::config::PlacementDensity`) so the offline level search can evolve
/// furniture amount and a chosen elite is a readable RON diff. See that struct for each field.
pub(crate) use crate::config::PlacementDensity;

/// Inset (metres) from a support surface's footprint edge to the usable top, so a rested prop doesn't
/// perch flush with the edge and read as about to fall off.
const SURFACE_INSET: f32 = 0.08;

/// Height up the wall to seat a wall light's origin (a sconce sits above head height).
const WALL_LIGHT_HEIGHT: f32 = 1.8;

/// Preferred centre-to-centre spacing (metres) between sconces in a wall row — about one every 2 m
/// reads as a lit corridor. The row still forces a minimum of [`SCONCE_ROW_MIN`] where the wall can
/// hold them, and grows up to the wall's capacity (the player's "3-to-X" rule; X = space to the
/// corner). Merrell et al. 2011 align decor to a wall — a "prominent feature" — rather than scattering.
const SCONCE_SPACING: f32 = 2.0;

/// Hard minimum centre-to-centre spacing (metres): the sconce mesh is ~0.38 m wide, so 0.6 m keeps a
/// forced-minimum row from overlapping on a short wall.
const SCONCE_MIN_SEP: f32 = 0.6;

/// Gap (metres) between the end sconce and the corner, so a row never crowds into a corner.
const SCONCE_CORNER_GAP: f32 = 0.9;

/// Minimum sconces in a row when the wall is long enough to seat them without overlap (the "3" in
/// "3-to-X"). A wall too short for three falls to whatever fits — never crammed (one path, no fudge).
const SCONCE_ROW_MIN: usize = 3;

/// Minimum centre-to-centre distance (metres, = tiles) between two tiled floor props (bins). A room's
/// bins are dispersed to at least this far apart so a couple of trashcans never cluster in one corner
/// (player request). A hard placement rule, not an "amount" dial — the *number* of bins is the evolvable
/// knob (`tiled_per_room`); this is a spacing contract like [`SCONCE_SPACING`]. ~11 ft.
const TILED_MIN_GAP: f32 = 3.5;

/// How far (metres, = tiles) into a room a doorway's keep-clear band reaches from the wall it pierces.
/// No furniture footprint may overlap this band, so a piece never lands in a corridor mouth and blocks
/// the only way in/out of a room (player request 2026-07-19). A hard placement rule, not an "amount"
/// dial — a spacing contract like [`TILED_MIN_GAP`]. ~1.1 tiles ≈ a body-width of approach room.
const DOORWAY_KEEP_CLEAR: f32 = 1.1;

/// The parsed furniture catalogue, held in the ECS world for the furnish pass.
#[derive(Resource)]
pub struct Manifest(pub FurnitureManifest);

/// The furniture density knobs, held in the ECS world for the furnish pass (see [`PlacementDensity`]).
#[derive(Resource)]
pub struct Density(pub PlacementDensity);

/// A resolved thing to spawn — computed in the parallel solve phase, consumed by the serial spawn.
struct SpawnReq {
    region: RegionId,
    glb: String,
    pos: Vec3,
    rot: Quat,
    /// Set for a wall-mounted prop (a sconce): its host wall's outward normal. The serial spawn tags it
    /// `CutawayMounted` so it hides when Q/E rotation makes that wall a near knee wall — otherwise the
    /// sconce would float in the cutaway gap. `None` for floor/ceiling props (unaffected by the cutaway).
    cutaway_outward: Option<Vec3>,
    /// True when the piece `affords("emit")` — a light source (ceiling tube, sconce, lamp, screen). The
    /// serial spawn tags it `light::LightEmitter` so the lighting system lights it and the `LightField`
    /// bake reads its position. Kit-agnostic (affordance, not asset key) — an asset kit lights its rooms
    /// with zero code change.
    emits: bool,
    /// True when the piece `affords("screen")` — a TV/monitor. The serial spawn tags it
    /// `light::ScreenEmitter` so the windowed lighting gives it an eery flickering screen glow (a cool-cyan
    /// LOS spotlight) instead of the generic fixture light. Kit-agnostic (affordance, not asset key).
    /// Purely cosmetic: the gameplay `LightField` still reads it via `emits`, so it is determinism-neutral.
    screen: bool,
    /// The item's mesh-centre pivot (world XZ, scaled) — see [`ManifestItem::pivot`]. The serial spawn
    /// shifts the model by `−(rot · pivot)` so its bounding-box centre lands on `pos`, so the symmetric
    /// `footprint` the placement solver reserved is what actually occupies the floor and an off-centre
    /// mesh never pokes through the wall its footprint clears.
    pivot: Vec2,
}

/// True when a manifest item offers affordance `aff` (e.g. "sit", "emit") — the portable, kit-agnostic
/// way to reason about what a piece is *for* (Fisher 2012; Qi 2018), rather than its kit-specific key.
fn affords(item: &ManifestItem, aff: &str) -> bool {
    item.affordances.iter().any(|a| a == aff)
}

// Support-surface classes — the bitmask vocabulary that pairs a scatter prop with the *kind* of top it
// may rest on. A support piece `provides` the OR of the class bits for every surface token among its
// affordances; a scatter prop `requires` the bit for its `Role::Scatter { surface }` token, and rests
// only where `provides & requires != 0`. This makes the `surface` token — previously dead config — the
// one lever that keeps a desk lamp on a desk/table and off a bed or dresser. A typed support is a
// surface *feature*, not a generic shelf (Tutenel et al. 2010, "A Semantic Scene Description Language
// for Procedural Layout Solving", AIIDE; props attach to a specific support class in Infinigen Indoors,
// Raistrick et al. 2024, arXiv 2406.11824).
const SURFACE_SUPPORT: u32 = 1 << 0; // any support top (bed/drawer/table/desk)
const SURFACE_WORKTOP: u32 = 1 << 1; // a desk/table worktop only

/// Map a support-surface token to its class bit. `support` = any support top; `worktop` = a desk/table.
/// An unrecognised token is `0` (matches nothing) — a scatter prop targeting it is dropped, never placed
/// on a wrong surface. Used both for a support's provided classes and a scatter prop's required class.
fn surface_bits(token: &str) -> u32 {
    match token {
        "support" => SURFACE_SUPPORT,
        "worktop" => SURFACE_WORKTOP,
        _ => 0,
    }
}

/// The surface classes a support piece provides — the OR of [`surface_bits`] over its affordances (a
/// desk affording `support` + `worktop` provides both; a bed affording only `support` provides only it).
fn provided_surfaces(item: &ManifestItem) -> u32 {
    item.affordances.iter().map(|a| surface_bits(a)).fold(0, |acc, b| acc | b)
}

/// The surface class a scatter prop requires, from its `Role::Scatter { surface }` token. A non-Scatter
/// role (never reached in Pass 4) requires nothing.
fn required_surface(item: &ManifestItem) -> u32 {
    match &item.role {
        Role::Scatter { surface } => surface_bits(surface),
        _ => 0,
    }
}

/// Pick a freestanding furniture set for a region from whatever the manifest offers, keyed by the
/// region's own room-type tags (chosen at generation in `dungeon.rs`, stored on `Region.props.tags`).
/// Selection is by semantic TAGS and AFFORDANCES, never by hardcoded asset keys, so any asset kit
/// furnishes rooms with zero code changes — the Stage-5 asset-swap contract (Tutenel et al. semantic
/// room classes `[home-still: cgf.12276]`; Merrell et al. 2011). The scan is rotated by `region_id` so
/// two rooms of the same type don't get an identical set, and a living room that picks a seat is also
/// given a screen so the seat→screen relation can fire. Returns up to `count` distinct items; a room
/// whose type-tags aren't in the kit still gets furniture via the top-up pass, so it's never left empty.
fn room_profile<'a>(
    region_id: RegionId,
    type_tags: &[String],
    freestanding: &[&'a ManifestItem],
    count: usize,
) -> Vec<&'a ManifestItem> {
    if freestanding.is_empty() || count == 0 {
        return Vec::new();
    }
    // The region's own type tags ARE the preferred room class. A kit that tags its items to match
    // reproduces themed rooms; a kit that tags differently (or a room whose type has no kit match) still
    // furnishes via the top-up pass below. (The base "room" tag matches nothing in the kit — harmless.)
    let preferred = type_tags;
    let n = freestanding.len();
    // Region-rotated scan offset so two rooms of the same type don't select an identical set (the old
    // fixed manifest-order scan made every same-type room identical, and never reached later items).
    let start = region_id as usize % n;
    let mut chosen: Vec<&ManifestItem> = Vec::new();
    // Preferred (room-type-tagged) items first, then top up from the rest — both scanned from the
    // rotated offset, so the room fills to `min(count, n)`, varies by region, and is never empty.
    for want_preferred in [true, false] {
        for k in 0..n {
            if chosen.len() >= count {
                break;
            }
            let it = freestanding[(start + k) % n];
            let is_preferred = it.tags.iter().any(|t| preferred.contains(t));
            if is_preferred == want_preferred && !chosen.iter().any(|c| c.key == it.key) {
                chosen.push(it);
            }
        }
    }
    // One coherent pairing, kit-agnostic via affordances: a living room that picked a seat ("sit") but
    // no screen ("emit") swaps a non-seat pick for a screen, so the seat→screen `Facing` relation can
    // fire (the showcase sofa-faces-TV rule). A swap, not a growth, so the room stays sparse.
    if preferred.iter().any(|t| t == "living")
        && chosen.iter().any(|it| affords(it, "sit"))
        && !chosen.iter().any(|it| affords(it, "emit"))
    {
        if let Some(screen) = freestanding
            .iter()
            .copied()
            .find(|it| affords(it, "emit") && !chosen.iter().any(|c| c.key == it.key))
        {
            if let Some(slot) = chosen.iter().position(|it| !affords(it, "sit")) {
                chosen[slot] = screen;
            } else if chosen.len() < count {
                chosen.push(screen);
            }
        }
    }
    // Same-group co-selection, kit-agnostic: pieces sharing a `group` (a bathroom's toilet + sink) only
    // cluster if BOTH are picked — the `Near` band in `freestanding_constraints` needs the pair present.
    // If a chosen item has a group whose partner was left out, swap a non-grouped pick for the partner (a
    // swap, not a growth, so the room stays sparse at `count`). This is what makes toilet+sink read as one
    // plumbed wall instead of being dropped to a lone fixture by the 2-of-3 selection above.
    for idx in 0..chosen.len() {
        let group = match &chosen[idx].group {
            Some(g) => g.clone(),
            None => continue,
        };
        let partner_present = chosen
            .iter()
            .enumerate()
            .any(|(k, it)| k != idx && it.group.as_deref() == Some(group.as_str()));
        if partner_present {
            continue;
        }
        // A same-group partner not already chosen...
        if let Some(partner) = freestanding
            .iter()
            .copied()
            .find(|it| it.group.as_deref() == Some(group.as_str()) && !chosen.iter().any(|c| c.key == it.key))
        {
            // ...swaps into a slot holding a non-grouped item (never evict another group's member).
            if let Some(slot) = chosen.iter().position(|it| it.group.is_none()) {
                chosen[slot] = partner;
            }
        }
    }
    chosen
}

/// A maximal run of contiguous full-height wall faces along one side of a region: the ordered face
/// points (world, y=0, on the wall's inner plane) and the shared inward normal. Grouping wall faces
/// into runs is what lets a sconce *row* align to a whole wall (from one corner to the next) instead
/// of the old flat-list "one light somewhere in the room" pick. The camera-facing E/S knee walls are
/// excluded (see [`wall_runs`]) so a wall-mounted light never floats in the cutaway gap above a short
/// wall — the same rule the nest placement uses (`crab.rs`).
struct WallRun {
    normal: Vec3,
    faces: Vec<Vec3>,
}

/// Group the region's full-height wall faces into contiguous runs (one per straight wall segment). Only
/// the +X (west) and +Z (north) inward normals survive the camera-facing filter, so faces split into
/// two families: north walls run along X at a fixed Z, west walls run along Z at a fixed X. Within a
/// family a run is a set of collinear, cell-adjacent faces. Deterministic: the sort key `(line, moving)`
/// is a cell coordinate pair, unique per face, so it is a total order (no RNG, no query input).
fn wall_runs(dungeon: &Dungeon, region: &Region) -> Vec<WallRun> {
    let (mn, mx) = (region.rect.min, region.rect.max);
    // (line, moving, face): north keyed by (cz line, cx moving); west by (cx line, cz moving).
    let mut north: Vec<(i32, i32, Vec3)> = Vec::new();
    let mut west: Vec<(i32, i32, Vec3)> = Vec::new();
    for cx in mn[0]..mx[0] {
        for cz in mn[1]..mx[1] {
            if cx != mn[0] && cx != mx[0] - 1 && cz != mn[1] && cz != mx[1] - 1 {
                continue; // interior cell — no bounding wall
            }
            let cell = IVec2::new(cx, cz);
            if !dungeon.is_floor(cell) {
                continue; // notched-out corner — no real wall
            }
            for (face, normal) in dungeon.wall_faces_near(dungeon.cell_center(cell)) {
                if crate::dungeon::SHORT_CAMERA_WALLS && crate::dungeon::is_camera_facing(normal) {
                    continue; // camera-facing knee wall — a sconce there floats in the cutaway gap
                }
                if normal == Vec3::Z {
                    north.push((cz, cx, face));
                } else if normal == Vec3::X {
                    west.push((cx, cz, face));
                }
            }
        }
    }
    let mut runs = Vec::new();
    split_into_runs(&mut north, Vec3::Z, &mut runs);
    split_into_runs(&mut west, Vec3::X, &mut runs);
    runs
}

/// Split `(line, moving, face)` entries into maximal runs: same `line` (collinear) and consecutive
/// `moving` (cell-adjacent). Appends each run to `out`.
fn split_into_runs(items: &mut Vec<(i32, i32, Vec3)>, normal: Vec3, out: &mut Vec<WallRun>) {
    // SORT-OK: input is region geometry (cell coordinates), never an ECS query; the key is unique.
    crate::sort_total!(items, |&(line, moving, _)| (line, moving));
    let mut cur: Vec<Vec3> = Vec::new();
    let mut prev: Option<(i32, i32)> = None;
    for &(line, moving, face) in items.iter() {
        let contiguous = matches!(prev, Some((pl, pm)) if pl == line && moving == pm + 1);
        if !contiguous && !cur.is_empty() {
            out.push(WallRun {
                normal,
                faces: std::mem::take(&mut cur),
            });
        }
        cur.push(face);
        prev = Some((line, moving));
    }
    if !cur.is_empty() {
        out.push(WallRun { normal, faces: cur });
    }
}

/// The sconce positions for one wall run: a row filling the wall with a minimum of [`SCONCE_ROW_MIN`]
/// lights (where it fits) up to the wall's capacity, spaced ~[`SCONCE_SPACING`] apart and inset
/// [`SCONCE_CORNER_GAP`] from each corner (the player's "3-to-X in a row, gap before the corner").
/// Positions are on the wall's inner plane at y=0; the caller lifts them to [`WALL_LIGHT_HEIGHT`].
fn sconce_row(run: &WallRun) -> Vec<Vec3> {
    let n = run.faces.len();
    if n == 0 {
        return Vec::new();
    }
    let f0 = run.faces[0];
    let fl = run.faces[n - 1];
    let tile = crate::dungeon::TILE_SIZE;
    // Direction along the wall. A single-cell run's endpoints coincide, so take the perpendicular of
    // the inward normal in the ground plane instead of normalising a zero vector.
    let dir = if n == 1 {
        Vec3::new(run.normal.z, 0.0, -run.normal.x)
    } else {
        (fl - f0).normalize_or_zero()
    };
    // The wall's true corners sit half a tile past the end cell centres; its length is n tiles.
    let wall_len = n as f32 * tile;
    let usable = wall_len - 2.0 * SCONCE_CORNER_GAP;
    if usable <= 0.0 {
        // Too short to inset a gap at both ends: one centred sconce.
        return vec![(f0 + fl) * 0.5];
    }
    let a = f0 - dir * (0.5 * tile) + dir * SCONCE_CORNER_GAP; // first slot (gap in from one corner)
    let b = fl + dir * (0.5 * tile) - dir * SCONCE_CORNER_GAP; // last slot (gap in from the other)
    let hard_cap = (usable / SCONCE_MIN_SEP).floor() as usize + 1; // most that fit without overlap
    let nominal = (usable / SCONCE_SPACING).floor() as usize + 1; // count at the preferred spacing
    let count = nominal.max(SCONCE_ROW_MIN).min(hard_cap).max(1);
    if count == 1 {
        return vec![(a + b) * 0.5];
    }
    (0..count)
        .map(|i| a.lerp(b, i as f32 / (count - 1) as f32))
        .collect()
}

/// Nearest floor cell to `start` within a region's bounding rect (Chebyshev distance). Non-rectangular
/// rooms can have a non-floor bounding-box centre (a notched corner or a plus-shape's arm gap), so anchors
/// that key off `rect.center_cell()` resolve through this to a real floor cell. `None` only if the rect
/// holds no floor at all (never for a real room).
fn nearest_floor_cell(
    dungeon: &Dungeon,
    rect: &crate::placement::ir::Rect2,
    start: IVec2,
) -> Option<IVec2> {
    let mut best: Option<(i32, IVec2)> = None;
    for cz in rect.min[1]..rect.max[1] {
        for cx in rect.min[0]..rect.max[0] {
            let c = IVec2::new(cx, cz);
            if !dungeon.is_floor(c) {
                continue;
            }
            let d = (cx - start.x).abs().max((cz - start.y).abs());
            if best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// The catalogue partitioned by placement `Role` — computed once, shared read-only across every
/// region's parallel solve. Holds borrows into the `FurnitureManifest`, so it lives only as long as
/// the manifest resource does.
struct RolePartitions<'a> {
    ceiling: Vec<&'a ManifestItem>,
    wall_lights: Vec<&'a ManifestItem>,
    tiled: Vec<&'a ManifestItem>,
    tiled_candidates: Arc<[Candidate]>,
    freestanding: Vec<&'a ManifestItem>,
    scatter: Vec<&'a ManifestItem>,
}

impl<'a> RolePartitions<'a> {
    fn from_catalogue(catalogue: &'a FurnitureManifest) -> Self {
        let tiled = catalogue.by_role(|r| matches!(r, Role::Tiled));
        let tiled_candidates: Arc<[Candidate]> = tiled
            .iter()
            .map(|it| to_candidate(it))
            .collect::<Vec<_>>()
            .into();
        RolePartitions {
            ceiling: catalogue.by_role(|r| {
                matches!(
                    r,
                    Role::Anchor {
                        host: Host::Ceiling
                    }
                )
            }),
            wall_lights: catalogue.by_role(|r| matches!(r, Role::Anchor { host: Host::Wall })),
            tiled,
            tiled_candidates,
            freestanding: catalogue.by_role(|r| matches!(r, Role::Freestanding)),
            scatter: catalogue.by_role(|r| matches!(r, Role::Scatter { .. })),
        }
    }
}

/// Furnish every region. Parallel solve → serial spawn.
pub fn furnish_regions(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    solvers: Res<PlacementSolvers>,
    manifest: Res<Manifest>,
    density: Res<Density>,
    assets: Res<AssetServer>,
) {
    let parts = RolePartitions::from_catalogue(&manifest.0);

    // ---- Parallel solve: each region is independent, so fan out over rayon. ----
    let orchestrator = &solvers.0;
    let density = &density.0;
    let requests: Vec<SpawnReq> = dungeon
        .regions
        .par_iter()
        .flat_map_iter(|region| furnish_region(&dungeon, orchestrator, region, &parts, density))
        .collect();

    // ---- Serial spawn. ----
    for req in requests {
        let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(req.glb));
        // Recentre the model on its placement point: shift the glTF origin by −(yaw · pivot) so the
        // mesh's bounding-box centre — not its (often off-centre) authored origin — lands on `req.pos`.
        // This is what makes the symmetric footprint an accurate reservation, so an against-wall piece
        // sits flush instead of poking its far side through the wall (README: furniture through a wall).
        let origin = req.pos - req.rot * Vec3::new(req.pivot.x, 0.0, req.pivot.y);
        let mut entity = commands.spawn((
            PlacedIn(req.region),
            WorldAssetRoot(scene),
            Transform::from_translation(origin)
                .with_rotation(req.rot)
                .with_scale(Vec3::splat(FURNITURE_SCALE)),
            // Starts hidden; `furniture_room_visibility` shows it once the squad has entered its room.
            Visibility::Hidden,
        ));
        // A wall-mounted prop rides the view-relative cutaway: it hides (scale → 0) whenever Q/E
        // rotation makes its host wall a near knee wall, so it never floats above the squash. Cutaway
        // hiding uses `scale`; the room-entry reveal owns `Visibility`; the two compose (see `dungeon`).
        if let Some(outward) = req.cutaway_outward {
            entity.insert(crate::dungeon::CutawayMounted {
                outward,
                base_scale: Vec3::splat(FURNITURE_SCALE),
            });
        }
        // A light-emitting piece: tag it so `light::LightingPlugin` gives it a real light and the
        // `LightField` bake reads its position. Added at spawn (archetype fixed here), so it never
        // churns iteration order later — furniture is not a sim actor, so this is harness-safe.
        if req.emits {
            entity.insert(crate::light::LightEmitter);
        }
        // A screen (TV) additionally gets the eery-glow marker; the windowed lighting swaps its generic
        // fixture light for a cool-cyan flickering LOS spotlight. Cosmetic — never read by the harness.
        if req.screen {
            entity.insert(crate::light::ScreenEmitter);
        }
    }
}

/// Compute the furniture to spawn in one region (all four passes) as a list of [`SpawnReq`]s. Pure and
/// engine-free apart from the `Dungeon` geometry it reads — no `Commands`, no `AssetServer` — so it is
/// deterministic under the region's seeded sub-stream and unit-testable without a GPU. `furnish_regions`
/// fans this out over `rayon`; the serial ECS spawn happens in the caller.
fn furnish_region(
    dungeon: &Dungeon,
    orchestrator: &super::solver::Orchestrator,
    region: &Region,
    parts: &RolePartitions,
    density: &PlacementDensity,
) -> Vec<SpawnReq> {
    let RolePartitions {
        ceiling,
        wall_lights,
        tiled,
        tiled_candidates,
        freestanding,
        scatter,
    } = parts;
    let mut rng = seeded(PLACEMENT_SEED ^ splitmix64(region.id as u64));
    let mut out: Vec<SpawnReq> = Vec::new();

    // Pass 1 — anchors.
    if let Some(item) = ceiling.first() {
        let c = region.rect.center_cell();
        // Resolve to a real floor cell — a notched room's bounding-box centre can be non-floor.
        if let Some(cell) = nearest_floor_cell(&dungeon, &region.rect, IVec2::new(c[0], c[1])) {
            let pos = dungeon.cell_center(cell).with_y(WALL_HEIGHT);
            out.push(SpawnReq {
                region: region.id,
                glb: item.glb.clone(),
                pos,
                rot: Quat::from_rotation_x(PI),
                cutaway_outward: None,
                emits: affords(item, "emit"),
                screen: affords(item, "screen"),
                pivot: Vec2::ZERO, // ceiling light hangs at the cell centre; no recentre
            });
        }
    }
    // No doors — the Backrooms look leaves every opening as a bare doorway (the dungeon still
    // frames each with a header lintel, so it reads as a doorway, just without a door).

    // Pass 1b — wall lights: sconce rows on the room's full-height walls. Kit-agnostic — any
    // manifest item with role Anchor{Wall} is placed here, so an asset kit lights its rooms
    // with zero code changes (the Stage-5 asset-swap contract). Camera-facing knee walls are
    // skipped (see `wall_runs`) so a light never floats in the cutaway gap.
    if let Some(light) = wall_lights.first() {
        // Group the room's full-height walls into runs and lay a 3-to-X sconce row along each (gap in
        // from each corner). `wall_lights_per_room` is the room's total budget, an evolvable brightness
        // knob (0 = unlit): fill the wall rows, then keep up to the budget, taken round-robin across
        // runs so a cap doesn't dump every light on one wall and leave the rest dark.
        let runs = wall_runs(&dungeon, region);
        let rows: Vec<Vec<Vec3>> = runs.iter().map(sconce_row).collect();
        let total: usize = rows.iter().map(Vec::len).sum();
        let budget = density.wall_lights_per_room.min(total);
        let mut kept = 0usize;
        let mut col = 0usize;
        'fill: while kept < budget {
            let before = kept;
            for (ri, row) in rows.iter().enumerate() {
                if col >= row.len() {
                    continue;
                }
                let normal = runs[ri].normal;
                out.push(SpawnReq {
                    region: region.id,
                    glb: light.glb.clone(),
                    // Sit the sconce on the wall's inner plane at head height, a touch proud of it.
                    pos: row[col].with_y(WALL_LIGHT_HEIGHT) + normal * 0.02,
                    // Front points into the room along the inward normal.
                    rot: Quat::from_rotation_y(normal.x.atan2(normal.z)),
                    // Tag with the host wall's outward normal so the sconce hides when Q/E rotation
                    // squashes that wall to knee height — otherwise it floats in the cutaway gap.
                    cutaway_outward: Some(-normal),
                    emits: affords(light, "emit"),
                    screen: false, // a wall sconce is never a screen
                    pivot: Vec2::ZERO, // mounted on the wall plane deliberately; no recentre
                });
                kept += 1;
                if kept >= budget {
                    break 'fill;
                }
            }
            if kept == before {
                break; // every row exhausted
            }
            col += 1;
        }
    }

    // Footprints of the freestanding pieces placed below, so no other mesh spawns inside them (player
    // report: a bin fully inside a couch). Furniture has priority: tiled clutter is collected here and
    // pushed only AFTER the freestanding layout is known, dropping any bin that would sit in a piece.
    let mut placed_fp: Vec<(Vec3, Vec2, f32)> = Vec::new();
    let mut tiled_kept: Vec<(SpawnReq, Vec2, f32)> = Vec::new();

    // Pass 2 — tiled scatter (→ WfcSolver). WFC returns a sparse fill in row-major order, so
    // taking the first N would bunch the props in the room's min (upper-left) corner; shuffle
    // first, then take, to spread the kept props across the whole floor.
    if !tiled_candidates.is_empty() {
        let problem = PlacementProblem {
            region,
            candidates: tiled_candidates.clone(),
            constraints: Vec::new(),
        };
        let mut placed = solve_placements(orchestrator, &problem, &mut rng, region.id, "tiled");
        shuffle_placements(&mut placed, &mut rng);
        // Perimeter bias: small floor props (a bin) read as clutter marooned in the middle of a
        // room — the amateur "push it to the wall" instinct that Merrell et al. 2011 formalize as
        // aligning with the room's edges. Stably partition wall-adjacent cells to the front so the
        // `take(N)` below keeps those first; the shuffle already fixed a deterministic order, and a
        // stable sort preserves it within each group (no RNG, no query input).
        // SORT-OK: input is the deterministic WFC solve + seeded shuffle, never an ECS query; stable
        // sort keeps the seeded order within each partition.
        placed.sort_by_key(|p| {
            let cell = IVec2::new(p.pos[0] as i32, p.pos[2] as i32);
            !(0..4).any(|d| dungeon.walled(cell, d)) // false (wall-adjacent) sorts before true
        });
        // Greedy min-distance dispersion: walk the perimeter-biased order and keep a prop only if it is
        // at least `TILED_MIN_GAP` from every prop already kept, up to `tiled_per_room`. So two bins
        // never cluster in one corner — a candidate too close to a kept one is skipped and the next
        // far-enough one taken (player request: "a trashcan can't spawn within X feet of another"). A
        // room too small to disperse simply keeps fewer — one path, never crammed.
        let mut kept_pos: Vec<Vec3> = Vec::new();
        for p in placed.into_iter() {
            if kept_pos.len() >= density.tiled_per_room {
                break;
            }
            let Some(item) = tiled.get(p.candidate) else {
                continue;
            };
            let cell = IVec2::new(p.pos[0] as i32, p.pos[2] as i32);
            let pos = dungeon.cell_center(cell);
            // Footprint-aware containment: the WFC solver scatters over the bounding rect, so reject any
            // slot whose *body* would cross a wall or fall in a notched-out corner of a non-rectangular
            // room — not just a center-cell test. Merrell et al. 2011 free-configuration-space.
            let half = footprint_half(item);
            if !dungeon.footprint_on_floor(pos, half, p.yaw) {
                continue;
            }
            // Keep the doorway approach clear: a prop dropped in a corridor mouth blocks the only way
            // in/out (player request 2026-07-19). `region.openings` carries each doorway's interior cell
            // + pierced wall, so reject any footprint overlapping a doorway keep-clear band.
            if !dungeon.footprint_clears_openings(pos, half, p.yaw, &region.openings, DOORWAY_KEEP_CLEAR) {
                continue;
            }
            if kept_pos.iter().any(|q| q.distance(pos) < TILED_MIN_GAP) {
                continue; // too close to an already-placed bin — skip, take a farther candidate
            }
            kept_pos.push(pos);
            // Defer: pushed after the freestanding pass so a bin overlapping furniture can be dropped.
            tiled_kept.push((
                SpawnReq {
                    region: region.id,
                    glb: item.glb.clone(),
                    pos,
                    rot: Quat::from_rotation_y(p.yaw),
                    cutaway_outward: None,
                    emits: affords(item, "emit"),
                    screen: affords(item, "screen"),
                    pivot: footprint_pivot(item),
                },
                half,
                p.yaw,
            ));
        }
    }

    // Pass 3 — freestanding furniture (→ MetropolisSolver). Kit-agnostic: the set is drawn
    // from the manifest's Freestanding items by semantic room-type tags, never hardcoded asset
    // keys, so any asset kit furnishes rooms with zero code changes (Tutenel et al. semantic
    // room classes; Merrell et al. 2011 — the Stage-5 asset-swap contract).
    // Placed support pieces (desk/table/drawer) with their world pose — the surfaces Pass 4
    // rests scatter props on. Collected here so scatter runs after the freestanding layout.
    let mut placed_supports: Vec<(&ManifestItem, Vec3, f32)> = Vec::new();
    let profile = room_profile(
        region.id,
        &region.props.tags,
        &freestanding,
        density.freestanding_per_room,
    );
    if !profile.is_empty() {
        let candidates: Arc<[Candidate]> = profile
            .iter()
            .map(|it| to_candidate(it))
            .collect::<Vec<_>>()
            .into();
        let constraints = freestanding_constraints(&profile, density);
        let problem = PlacementProblem {
            region,
            candidates,
            constraints,
        };
        for p in solve_placements(orchestrator, &problem, &mut rng, region.id, "freestanding") {
            if let Some(item) = profile.get(p.candidate) {
                // Freestanding solver works in world/tile coords already.
                let pos = Vec3::new(p.pos[0], 0.0, p.pos[2]);
                // Footprint-aware containment: Metropolis optimizes within the bounding rect,
                // so on a notched (L/T/plus) room a piece can drift into a carved-out area.
                // Reject any whose *body* crosses a wall so freestanding furniture never lands
                // inside or half-through a wall (README ISSUES 1 & 2). Merrell et al. 2011.
                let half = footprint_half(item);
                if !dungeon.footprint_on_floor(pos, half, p.yaw) {
                    continue;
                }
                // Keep the doorway approach clear (player request 2026-07-19): a freestanding piece in a
                // corridor mouth blocks the room's entrance. Same opening-band reject as the tiled pass.
                if !dungeon.footprint_clears_openings(pos, half, p.yaw, &region.openings, DOORWAY_KEEP_CLEAR) {
                    continue;
                }
                // No mesh-in-mesh: the Metropolis overlap term is soft, so hard-reject a piece that would
                // still intersect one already accepted this pass (player report 2026-07-20).
                if footprint_overlaps(pos, half, p.yaw, &placed_fp) {
                    continue;
                }
                out.push(SpawnReq {
                    region: region.id,
                    glb: item.glb.clone(),
                    pos,
                    rot: Quat::from_rotation_y(p.yaw),
                    cutaway_outward: None,
                    emits: affords(item, "emit"),
                    screen: affords(item, "screen"),
                    pivot: footprint_pivot(item),
                });
                placed_fp.push((pos, half, p.yaw));
                if affords(item, "support") {
                    placed_supports.push((*item, pos, p.yaw));
                }
            }
        }
    }

    // Push the deferred tiled clutter now that the freestanding layout is known, dropping any bin that
    // would sit inside a piece of furniture (player report 2026-07-20: a bin inside a couch). Furniture
    // wins; bin-vs-bin spacing was already enforced by `TILED_MIN_GAP` above.
    for (req, half, yaw) in tiled_kept {
        if !footprint_overlaps(req.pos, half, yaw, &placed_fp) {
            out.push(req);
        }
    }

    // Pass 4 — scatter props (lamp/plant/TV) on support surfaces (README ISSUE 4). The fine
    // level of the two-level grid: each support's top is subdivided into an inner 9×9 lattice
    // and props are dropped into free sub-cells (see `scatter::scatter_on_surfaces`). Runs last
    // because it reads the just-placed supports (Tutenel et al. 2010: support is a surface
    // feature, so a prop's height falls out of the surface it rests on).
    if !scatter.is_empty() {
        let surfaces: Vec<scatter::SupportSurface> = placed_supports
            .iter()
            .filter_map(|(item, pos, yaw)| support_surface(item, *pos, *yaw))
            .collect();
        if !surfaces.is_empty() {
            // A small, region-rotated set of props so same-type rooms don't get identical tops.
            let start = region.id as usize % scatter.len();
            let chosen: Vec<&ManifestItem> = (0..scatter.len().min(density.scatter_per_room))
                .map(|k| scatter[(start + k) % scatter.len()])
                .collect();
            let props: Vec<scatter::ScatterProp> = chosen
                .iter()
                .enumerate()
                .map(|(i, it)| scatter::ScatterProp {
                    candidate: i,
                    half_x: it.footprint.0 * 0.5 * FURNITURE_SCALE,
                    half_z: it.footprint.1 * 0.5 * FURNITURE_SCALE,
                    // The support class this prop demands (desk lamp → worktop); it rests only on a
                    // support that provides it, else it is dropped (no fallback onto a bed/dresser).
                    requires: required_surface(it),
                })
                .collect();
            for pl in scatter::scatter_on_surfaces(&surfaces, &props, &mut rng) {
                if let Some(item) = chosen.get(pl.candidate) {
                    let pos = Vec3::new(pl.pos[0], pl.pos[1], pl.pos[2]);
                    out.push(SpawnReq {
                        region: region.id,
                        glb: item.glb.clone(),
                        pos,
                        rot: Quat::from_rotation_y(pl.yaw),
                        cutaway_outward: None,
                        emits: affords(item, "emit"),
                        screen: affords(item, "screen"),
                        pivot: footprint_pivot(item),
                    });
                }
            }
        }
    }
    out
}

/// Furnish every region and return each placed piece as `(region, world position)` — the GPU-free,
/// engine-free entry point the offline level search (`crate::squad_ai::level_eval`) uses to score
/// furniture amount without spawning ECS entities. Runs the same per-region pipeline as
/// [`furnish_regions`] minus the serial spawn; deterministic under the per-region seeded sub-streams.
pub(crate) fn furnish_all(
    dungeon: &Dungeon,
    manifest: &FurnitureManifest,
    weights: crate::placement::solvers::metropolis::MetropolisWeights,
    density: &PlacementDensity,
) -> Vec<(RegionId, Vec3)> {
    let parts = RolePartitions::from_catalogue(manifest);
    let orchestrator = super::build_solvers(weights);
    dungeon
        .regions
        .iter()
        .flat_map(|region| furnish_region(dungeon, &orchestrator, region, &parts, density))
        .map(|req| (req.region, req.pos))
        .collect()
}

/// Deterministic Fisher–Yates shuffle of solver placements via the region's seeded RNG. Used to spread
/// tiled scatter across the room: WFC returns filled cells in row-major order, so taking the first N
/// without shuffling biases them into the min (upper-left) corner. `below` is unbiased (see `rng`).
fn shuffle_placements(placements: &mut [Placement], rng: &mut rand_chacha::ChaCha8Rng) {
    for i in (1..placements.len()).rev() {
        placements.swap(i, rng.below(i + 1));
    }
}

/// Route a problem through the orchestrator and flatten its outcome to a placement list.
fn solve_placements(
    orchestrator: &super::solver::Orchestrator,
    problem: &PlacementProblem,
    rng: &mut rand_chacha::ChaCha8Rng,
    region: RegionId,
    label: &str,
) -> Vec<Placement> {
    match orchestrator.solve_group(problem, rng) {
        Ok(Outcome::Assignment(p)) => p,
        Ok(Outcome::Partial { placed, .. }) => placed,
        Ok(Outcome::Ranked(ranked)) => ranked
            .into_iter()
            .next()
            .map(|(_, p)| p)
            .unwrap_or_default(),
        Err(e) => {
            warn!("placement: region {region} {label} pass unsolved: {e}");
            Vec::new()
        }
    }
}

/// Soft constraints for a freestanding set. Every item prefers its back to a wall (which keeps the
/// group Soft so the orchestrator routes it to Metropolis, not WFC); every pair is pushed apart by a
/// `MinDistance` so the room reads as sparse scatter (backrooms) rather than a clump; and a seat is
/// asked to face a screen so a sofa faces its TV. The relation is selected by AFFORDANCE ("sit" faces
/// "emit"), not by hardcoded asset keys, so it survives an asset-kit swap; its arrangement strength is
/// scaled by `coherence` in the solver's cost.
fn freestanding_constraints(profile: &[&ManifestItem], density: &PlacementDensity) -> Vec<Constraint> {
    let mut constraints = Vec::new();
    let mut id = 0u32;
    // Back-to-wall: HARD for pieces that must sit flush to a wall (plumbing fixtures, a fridge —
    // tagged `back_to_wall`), so the solver seats them against the perimeter with the correct facing;
    // SOFT (a mild perimeter preference) for everything else. One clear intent per item, keyed by
    // affordance so it survives an asset-kit swap (README ISSUE 3).
    for (i, it) in profile.iter().enumerate() {
        let modality = if affords(it, "back_to_wall") {
            Modality::Hard
        } else {
            Modality::Soft(1.0)
        };
        constraints.push(Constraint {
            id,
            scope: Scope::Object(i),
            predicate: Predicate::AgainstWall,
            modality,
            guard: None,
        });
        id += 1;
    }
    // Pairwise spacing: pieces sharing a `group` (e.g. a bathroom's toilet + sink) are drawn TOGETHER
    // by a `Near` band so they read as one plumbed wall; every other pair is pushed APART by
    // `MinDistance` so a room reads as sparse scatter rather than a clump. Both Soft.
    for i in 0..profile.len() {
        for j in (i + 1)..profile.len() {
            let same_group = matches!(
                (&profile[i].group, &profile[j].group),
                (Some(a), Some(b)) if a == b
            );
            let predicate = if same_group {
                Predicate::Near(density.group_near_max)
            } else {
                Predicate::MinDistance(density.freestanding_min_gap)
            };
            constraints.push(Constraint {
                id,
                scope: Scope::Pair(i, j),
                predicate,
                modality: Modality::Soft(1.0),
                guard: None,
            });
            id += 1;
        }
    }
    // The one relational rule, kit-agnostic: a seat faces a screen (sofa → TV). Its strength is scaled
    // by `coherence` in the solver, so it ranges from ignored (backrooms) to firmly arranged.
    let seat = profile.iter().position(|it| affords(it, "sit"));
    let screen = profile.iter().position(|it| affords(it, "emit"));
    if let (Some(s), Some(t)) = (seat, screen) {
        if s != t {
            constraints.push(Constraint {
                id,
                scope: Scope::Object(s),
                predicate: Predicate::Facing(t),
                modality: Modality::Soft(1.0),
                guard: None,
            });
        }
    }
    constraints
}

/// Rooms a squad unit has physically stood in at least once. Furniture reveal is one-way and per-room: a
/// region is inserted the first frame any unit occupies one of its cells, and its furniture then stays
/// visible for the rest of the run — the same permanent reveal the floor/wall tiles use.
#[derive(Resource, Default)]
pub struct RevealedRooms(pub HashSet<RegionId>);

/// Reveal a room's furniture the first time a squad unit **enters** it, and keep it visible thereafter
/// (remembered, per-room). Owns furniture `Visibility` exclusively (fog never touches furniture), so
/// nothing else fights it. One-way — only ever flips Hidden→Visible.
///
/// Entry, not line of sight. The knee-wall layout lets the camera see into a room long before the squad
/// walks in, so a `FogGrid::seen_at` gate furnished rooms the squad had merely glimpsed down a corridor.
/// An earlier revision keyed off *live* occupancy and rooms visibly emptied as the squad left; the one-way
/// [`RevealedRooms`] guard is what makes entry-gating stable, and it is why this reads occupancy rather
/// than the fog at all. Matches [`super::PlacedIn`]'s original intent.
pub fn furniture_room_visibility(
    dungeon: Res<Dungeon>,
    units: Query<&Transform, With<crate::squad::Unit>>,
    mut revealed: ResMut<RevealedRooms>,
    mut furniture: Query<(&PlacedIn, &mut Visibility)>,
) {
    // Once per call, not once per region: a squad is a handful of units, a dungeon is many rooms.
    let occupied: Vec<IVec2> = units.iter().map(|t| dungeon.world_to_cell(t.translation)).collect();

    // Grow the revealed set: a region is revealed once a unit stands inside it. Already-revealed regions
    // are skipped (one-way), so this settles to a cheap membership check.
    for region in &dungeon.regions {
        if revealed.0.contains(&region.id) {
            continue;
        }
        if occupied.iter().any(|c| region.rect.contains([c.x, c.y])) {
            revealed.0.insert(region.id);
        }
    }
    // Reveal furniture in revealed rooms. One-way, so we only ever write the Hidden→Visible transition.
    for (placed, mut vis) in &mut furniture {
        if revealed.0.contains(&placed.0) && *vis != Visibility::Visible {
            *vis = Visibility::Visible;
        }
    }
}

/// A manifest item's footprint half-extents (½ width, ½ depth) in rendered (scaled) metres — the
/// form [`Dungeon::footprint_on_floor`] and the solvers reason about. Kept in one place so the
/// footprint↔scale agreement (see `FURNITURE_SCALE`) holds for every containment test.
fn footprint_half(item: &ManifestItem) -> Vec2 {
    Vec2::new(
        item.footprint.0 * 0.5 * FURNITURE_SCALE,
        item.footprint.1 * 0.5 * FURNITURE_SCALE,
    )
}

/// A manifest item's mesh-centre pivot in rendered (scaled) metres — the local XZ offset the spawn
/// shifts the model by (see [`ManifestItem::pivot`]) so an off-centre mesh recentres on its placement
/// point. `(0,0)` for a centred piece.
fn footprint_pivot(item: &ManifestItem) -> Vec2 {
    Vec2::new(item.pivot.0 * FURNITURE_SCALE, item.pivot.1 * FURNITURE_SCALE)
}

/// Does a yaw-snapped footprint centred at `center` overlap any already-placed one? Each `placed` entry
/// is `(centre, pre-rotation half-extents, yaw)`. An AABB test on the quarter-turn-swapped half-extents
/// (matching [`Dungeon::footprint_on_floor`]), used to stop furniture meshes spawning inside one another
/// across placement passes (player report: a bin fully inside a couch). Edge-touching (zero overlap
/// area) is allowed, so pieces may still sit flush side by side.
fn footprint_overlaps(center: Vec3, half: Vec2, yaw: f32, placed: &[(Vec3, Vec2, f32)]) -> bool {
    let swap = |h: Vec2, y: f32| -> Vec2 {
        let quarter = (y / std::f32::consts::FRAC_PI_2).round() as i32 & 3;
        if quarter % 2 == 1 { Vec2::new(h.y, h.x) } else { h }
    };
    let a = swap(half, yaw);
    placed.iter().any(|(c, h, y)| {
        let b = swap(*h, *y);
        (a.x + b.x) - (center.x - c.x).abs() > 0.0 && (a.y + b.y) - (center.z - c.z).abs() > 0.0
    })
}

/// The usable top of a placed support piece as a [`scatter::SupportSurface`], or `None` if the piece
/// has no authored height (nothing to rest a prop on). The usable area is the footprint (yaw-aware,
/// so a rotated table swaps width/depth) inset by [`SURFACE_INSET`] so props don't perch on the edge;
/// `top_y` is the piece's height, the plane props rest on (Tutenel et al. 2010 surface feature).
fn support_surface(item: &ManifestItem, pos: Vec3, yaw: f32) -> Option<scatter::SupportSurface> {
    if item.height <= 0.0 {
        return None;
    }
    let hw = item.footprint.0 * 0.5 * FURNITURE_SCALE;
    let hd = item.footprint.1 * 0.5 * FURNITURE_SCALE;
    let quarter = (yaw / std::f32::consts::FRAC_PI_2).round() as i32 & 3;
    let (hx, hz) = if quarter % 2 == 1 { (hd, hw) } else { (hw, hd) };
    Some(scatter::SupportSurface {
        cx: pos.x,
        cz: pos.z,
        half_x: (hx - SURFACE_INSET).max(0.02),
        half_z: (hz - SURFACE_INSET).max(0.02),
        top_y: item.height * FURNITURE_SCALE,
        // Props rested here inherit the host's facing (TV on a drawer faces the way the drawers open).
        yaw,
        // The classes this top offers, so a typed scatter prop (desk lamp → worktop) only rests here
        // when this support actually provides that class.
        provides: provided_surfaces(item),
    })
}

/// Map a manifest entry to an IR candidate (asset key + role + footprint + affordances).
fn to_candidate(item: &ManifestItem) -> Candidate {
    Candidate {
        asset: item.key.clone(),
        role: item.role.clone(),
        // Footprints in rendered (scaled) units so the layout solver reasons at the size we draw.
        footprint: [
            item.footprint.0 * FURNITURE_SCALE,
            item.footprint.1 * FURNITURE_SCALE,
        ],
        dof: Dof {
            translate: true,
            rotate_quarter: true,
            rotate_free: false,
        },
        affordances: item.affordances.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::ir::{Predicate, Role};

    /// `footprint_overlaps` flags a mesh spawned inside another (the bin-in-couch report), allows a piece
    /// well clear, and allows two pieces flush edge-to-edge (zero overlap area).
    #[test]
    fn footprint_overlaps_detects_mesh_intersection() {
        // A couch at the origin (half 1.04 × 0.44).
        let couch = [(Vec3::ZERO, Vec2::new(1.04, 0.44), 0.0)];
        // A bin sitting inside it (the captured case: ~0.68/0.29 offset, half 0.20) → overlaps.
        assert!(
            footprint_overlaps(Vec3::new(0.68, 0.0, 0.29), Vec2::splat(0.20), 0.0, &couch),
            "a bin inside the couch footprint must be flagged"
        );
        // A bin well clear of the couch → no overlap.
        assert!(
            !footprint_overlaps(Vec3::new(3.0, 0.0, 0.0), Vec2::splat(0.20), 0.0, &couch),
            "a bin far from the couch must not be flagged"
        );
        // Flush side by side (edges exactly touching: 1.04 + 0.20 = 1.24 apart) → allowed.
        assert!(
            !footprint_overlaps(Vec3::new(1.24, 0.0, 0.0), Vec2::splat(0.20), 0.0, &couch),
            "edge-touching pieces must be allowed"
        );
    }

    /// The shipped density knobs, inlined so the unit tests don't need the config file.
    const TEST_DENSITY: PlacementDensity = PlacementDensity {
        tiled_per_room: 2,
        freestanding_per_room: 2,
        scatter_per_room: 3,
        wall_lights_per_room: 1,
        freestanding_min_gap: 1.5,
        group_near_max: 1.2,
    };

    fn item(key: &str, tags: &[&str], affs: &[&str]) -> ManifestItem {
        ManifestItem {
            key: key.into(),
            glb: format!("{key}.glb"),
            category: "furniture".into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            role: Role::Freestanding,
            footprint: (1.0, 1.0),
            affordances: affs.iter().map(|s| s.to_string()).collect(),
            group: None,
            height: 0.0,
            pivot: (0.0, 0.0),
        }
    }

    /// The shipped kit's 8 freestanding items in manifest order: the seat (sofa) and screen (tv) are
    /// both "living"-tagged but the screen is last, which the old cap+order could never co-select.
    fn kit() -> Vec<ManifestItem> {
        vec![
            item("bed", &["bedroom"], &["sleep", "support"]),
            item("sofa", &["living"], &["sit"]),
            item("table", &["living", "dining"], &["support"]),
            item("chair", &["dining"], &["sit"]),
            item("drawer", &["bedroom"], &["store", "support"]),
            item("shelf", &["living"], &["store"]),
            item("fridge", &["kitchen"], &["store"]),
            item("tv", &["living"], &["emit"]),
        ]
    }

    #[test]
    fn living_room_that_picks_a_seat_also_gets_a_screen() {
        let items = kit();
        let refs: Vec<&ManifestItem> = items.iter().collect();
        let living = vec!["room".to_string(), "living".to_string()];
        for region_id in [0u32, 3, 4, 7, 8, 11] {
            let profile = room_profile(region_id, &living, &refs, TEST_DENSITY.freestanding_per_room);
            let has_seat = profile.iter().any(|it| affords(it, "sit"));
            let has_screen = profile.iter().any(|it| affords(it, "emit"));
            assert!(
                !has_seat || has_screen,
                "living room {region_id} picked a seat but no screen: {:?}",
                profile.iter().map(|it| it.key.as_str()).collect::<Vec<_>>()
            );
        }
    }

    /// Build a north-wall run (inward normal +Z) of `n` cells along +X at cell centres, z fixed.
    fn north_run(n: usize) -> WallRun {
        let faces = (0..n)
            .map(|i| Vec3::new(i as f32 + 0.5, 0.0, 0.0))
            .collect();
        WallRun {
            normal: Vec3::Z,
            faces,
        }
    }

    #[test]
    fn a_long_wall_gets_a_row_of_at_least_three_with_corner_gaps() {
        // A 6-tile wall: corners at x=0 and x=6. Expect ≥3 sconces, each inset ≥ the corner gap.
        let run = north_run(6);
        let row = sconce_row(&run);
        assert!(
            row.len() >= SCONCE_ROW_MIN,
            "a 6 m wall should seat at least {SCONCE_ROW_MIN} sconces, got {}",
            row.len()
        );
        let (lo, hi) = (0.0f32, 6.0f32);
        for p in &row {
            assert!(
                p.x >= lo + SCONCE_CORNER_GAP - 1e-3 && p.x <= hi - SCONCE_CORNER_GAP + 1e-3,
                "sconce at x={} crowds a corner (gap {SCONCE_CORNER_GAP})",
                p.x
            );
            assert!(p.z.abs() < 1e-6, "sconce stays on the wall line (z=0)");
        }
        // Rows fill more of a longer wall (X grows with available space).
        assert!(
            sconce_row(&north_run(12)).len() > row.len(),
            "a longer wall should seat more sconces (the 'X' in 3-to-X)"
        );
    }

    #[test]
    fn a_short_wall_gets_one_centred_sconce_never_crammed() {
        // A 2-tile wall cannot hold 3 with corner gaps — one path is a single centred light, not a
        // forced-and-overlapping trio.
        let row = sconce_row(&north_run(2));
        assert_eq!(row.len(), 1, "a 2 m wall seats a single sconce");
        assert!((row[0].x - 1.0).abs() < 1e-3, "centred on the 2 m wall");
    }

    #[test]
    fn sconce_row_is_ordered_and_evenly_spaced() {
        let row = sconce_row(&north_run(8));
        assert!(row.len() >= 3);
        // Strictly increasing along the wall, with equal gaps (an even row, no bunching).
        let gaps: Vec<f32> = row.windows(2).map(|w| w[1].x - w[0].x).collect();
        for g in &gaps {
            assert!(*g > 0.0, "row is ordered along the wall");
            assert!(
                (g - gaps[0]).abs() < 1e-3,
                "sconces are evenly spaced (gap {g} vs {})",
                gaps[0]
            );
        }
    }

    #[test]
    fn same_type_living_rooms_can_differ() {
        // The region-rotated scan differentiates two same-type rooms that the old fixed manifest-order
        // scan would have furnished identically.
        let items = kit();
        let refs: Vec<&ManifestItem> = items.iter().collect();
        let living = vec!["room".to_string(), "living".to_string()];
        let keys = |rid| {
            let mut k: Vec<&str> = room_profile(rid, &living, &refs, TEST_DENSITY.freestanding_per_room)
                .iter()
                .map(|it| it.key.as_str())
                .collect();
            k.sort_unstable();
            k
        };
        assert_ne!(
            keys(0),
            keys(4),
            "two living rooms should not get an identical set"
        );
    }

    #[test]
    fn room_type_tag_selects_matching_furniture() {
        // A room's generation-time type tag drives selection: an "office" room prefers the office desk
        // over the bed/sofa, even though all three are eligible freestanding items.
        let items = vec![
            item("desk", &["office"], &["support"]),
            item("bed", &["bedroom"], &["sleep"]),
            item("sofa", &["living"], &["sit"]),
        ];
        let refs: Vec<&ManifestItem> = items.iter().collect();
        let office = vec!["room".to_string(), "office".to_string()];
        let profile = room_profile(7, &office, &refs, 1);
        assert_eq!(profile.len(), 1);
        assert_eq!(
            profile[0].key, "desk",
            "office room must prefer the office-tagged desk"
        );
    }

    #[test]
    fn untyped_room_still_furnishes_via_topup() {
        // A room whose type has no kit match (e.g. "hall") is never left empty — the universal top-up
        // pass fills it from the rest of the catalogue. This is the single furnishing path (no branch).
        let items = vec![
            item("bed", &["bedroom"], &["sleep"]),
            item("sofa", &["living"], &["sit"]),
        ];
        let refs: Vec<&ManifestItem> = items.iter().collect();
        let hall = vec!["room".to_string(), "hall".to_string()];
        let profile = room_profile(2, &hall, &refs, 2);
        assert!(
            !profile.is_empty(),
            "a room with no type match must still get furniture via top-up"
        );
    }

    #[test]
    fn fixtures_get_hard_against_wall_and_group_pull() {
        // README ISSUE 3: plumbing fixtures back onto a wall (HARD), and a shared `group` draws the
        // toilet + sink together (Near) while unrelated pieces stay spread (MinDistance).
        use crate::placement::ir::{Modality, Scope};
        let mut toilet = item("toilet", &["bathroom"], &["hygiene", "back_to_wall"]);
        toilet.group = Some("bath".into());
        let mut sink = item("sink", &["bathroom"], &["hygiene", "back_to_wall"]);
        sink.group = Some("bath".into());
        let sofa = item("sofa", &["living"], &["sit"]); // not back_to_wall, no group
        let items = vec![toilet, sink, sofa];
        let refs: Vec<&ManifestItem> = items.iter().collect();
        let cs = freestanding_constraints(&refs, &TEST_DENSITY);

        let against = |i: usize| {
            cs.iter()
                .find(|c| {
                    matches!(c.scope, Scope::Object(x) if x == i)
                        && matches!(c.predicate, Predicate::AgainstWall)
                })
                .unwrap_or_else(|| panic!("no AgainstWall for object {i}"))
        };
        assert!(
            matches!(against(0).modality, Modality::Hard),
            "toilet must be HARD against-wall"
        );
        assert!(
            matches!(against(1).modality, Modality::Hard),
            "sink must be HARD against-wall"
        );
        assert!(
            matches!(against(2).modality, Modality::Soft(_)),
            "sofa stays a soft preference"
        );

        let pair = |i: usize, j: usize| {
            cs.iter()
                .find(|c| matches!(c.scope, Scope::Pair(a, b) if a == i && b == j))
                .unwrap_or_else(|| panic!("no pair constraint for ({i},{j})"))
        };
        assert!(
            matches!(pair(0, 1).predicate, Predicate::Near(_)),
            "toilet+sink grouped by Near"
        );
        assert!(
            matches!(pair(0, 2).predicate, Predicate::MinDistance(_)),
            "toilet↔sofa spread apart"
        );
        assert!(
            matches!(pair(1, 2).predicate, Predicate::MinDistance(_)),
            "sink↔sofa spread apart"
        );
    }

    #[test]
    fn grouped_fixtures_are_co_selected() {
        // README ISSUE 1: the toilet+sink `Near` band only fires when BOTH are chosen. A bathroom has
        // three eligible fixtures (toilet, sink, bath) but `freestanding_per_room` picks two, and the
        // rotated scan can pick {toilet, bath} or {sink, bath} — dropping half the pair. `room_profile`
        // must co-select the group partner so the pair always co-occurs, at every region rotation.
        let mut toilet = item("toilet", &["bathroom"], &["hygiene", "back_to_wall"]);
        toilet.group = Some("bath".into());
        let mut sink = item("sink", &["bathroom"], &["hygiene", "back_to_wall"]);
        sink.group = Some("bath".into());
        let bath = item("bath", &["bathroom"], &["hygiene", "back_to_wall"]); // no group
        let items = vec![toilet, sink, bath];
        let refs: Vec<&ManifestItem> = items.iter().collect();
        let bathroom = vec!["room".to_string(), "bathroom".to_string()];
        // Every region rotation must yield the plumbed pair, never a lone fixture beside the bath.
        for region_id in 0..6 {
            let profile = room_profile(region_id, &bathroom, &refs, 2);
            let keys: Vec<&str> = profile.iter().map(|it| it.key.as_str()).collect();
            assert!(
                keys.contains(&"toilet") && keys.contains(&"sink"),
                "region {region_id}: bathroom must co-select toilet+sink, got {keys:?}"
            );
        }
    }

    #[test]
    fn freestanding_constraints_are_kit_agnostic_and_spread() {
        // A seat + screen chosen by AFFORDANCE (not by the keys "sofa"/"tv") must still emit the Facing
        // relation, and every pair must get a spreading MinDistance.
        let items = vec![
            item("couch", &["lounge"], &["sit"]),
            item("monitor", &["lounge"], &["emit"]),
        ];
        let refs: Vec<&ManifestItem> = items.iter().collect();
        let cs = freestanding_constraints(&refs, &TEST_DENSITY);
        assert!(
            cs.iter().any(|c| matches!(c.predicate, Predicate::Facing(_))),
            "a seat + screen (by affordance) should emit a Facing relation regardless of asset keys"
        );
        assert!(
            cs.iter()
                .any(|c| matches!(c.predicate, Predicate::MinDistance(_))),
            "freestanding pairs should be spread apart"
        );
    }

    /// End-to-end: run the whole per-region pipeline (all four passes) on a hand-built room with the
    /// SHIPPED manifest + real solvers, GPU-free. Proves the Phase 1–3 wiring composes: nothing floor
    /// furniture escapes the room (containment), and a scatter prop rests on the desk (surface stacking).
    #[test]
    fn furnish_region_end_to_end_stacks_props_and_contains_furniture() {
        use crate::dungeon::Dungeon;
        use crate::placement::ir::{PropertyBag, Rect2, Region};
        use crate::placement::solver::Orchestrator;
        use crate::placement::solvers::constraint::ConstraintSolver;
        use crate::placement::solvers::metropolis::MetropolisSolver;
        use crate::placement::solvers::wfc::WfcSolver;

        // 8×8 grid with a 6×6 floor room (cells (1,1)..=(6,6)) walled in by rock.
        let (w, h) = (8usize, 8usize);
        let mut mask = vec![false; w * h];
        for y in 1..7 {
            for x in 1..7 {
                mask[y * w + x] = true;
            }
        }
        let mut dungeon = Dungeon::from_walkable(w, h, mask);
        // An "office" room → room_profile prefers the desk, which affords "support" (a surface).
        dungeon.regions.push(Region {
            id: 0,
            rect: Rect2 { min: [1, 1], max: [7, 7] },
            openings: Vec::new(),
            adjacency: Vec::new(),
            props: PropertyBag { tags: vec!["room".into(), "office".into()] },
        });

        // Load the shipped placement slice from the unified game config (manifest + solver weights).
        let cfg = crate::config::load_game_config().expect("shipped game config");
        let catalogue = cfg.placement.furniture.clone();
        let parts = RolePartitions::from_catalogue(&catalogue);
        let weights = cfg.placement.metropolis.clone();
        let mut orch = Orchestrator::new();
        orch.register(Box::new(WfcSolver));
        orch.register(Box::new(MetropolisSolver::new(weights)));
        orch.register(Box::new(ConstraintSolver));

        let reqs = furnish_region(&dungeon, &orch, &dungeon.regions[0], &parts, &cfg.placement.density);
        assert!(!reqs.is_empty(), "the office room should be furnished");

        // Phase 3: at least one scatter prop rests on the desk surface. Floor furniture sits at y≈0, the
        // ceiling light at WALL_HEIGHT (2.4), a wall sconce at 1.8 — a rested prop is the only 0<y<1 case.
        let on_surface = reqs.iter().filter(|r| r.pos.y > 0.05 && r.pos.y < 1.0).count();
        assert!(on_surface >= 1, "a scatter prop should rest on the desk (found {on_surface})");

        // Phase 1: every floor-level piece (y≈0) sits on real floor — nothing escaped into a wall/void.
        for r in &reqs {
            if r.pos.y.abs() < 1e-3 {
                assert!(
                    dungeon.is_floor(dungeon.world_to_cell(r.pos)),
                    "floor furniture at {:?} escaped the room floor",
                    r.pos
                );
            }
        }
    }
}
