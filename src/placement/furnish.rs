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
use crate::fog::FogGrid;
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

/// Cap on tiled decor props per room, so floors don't fill with scatter.
const TILED_PER_ROOM: usize = 2;

/// Cap on freestanding furniture pieces per room (modest so a 4–5 tile room reads furnished without
/// crowding).
const FREESTANDING_PER_ROOM: usize = 2;

/// Minimum centre-to-centre spacing (metres) requested between freestanding pieces, so a room reads as
/// sparse scatter (backrooms) rather than a clump. Emitted as a Soft `MinDistance` the Metropolis
/// solver honors via `w_min_distance` — sparseness, independent of the `coherence` arrangement knob.
const FREESTANDING_MIN_GAP: f32 = 1.5;

/// Maximum centre-to-centre distance (metres) a `Near` grouping band pulls same-`group` pieces to, so
/// a bathroom's toilet + sink cluster on one wall instead of scattering. Larger than the pieces so they
/// sit adjacent (overlap is prevented separately by the solver's `w_overlap`), smaller than
/// `FREESTANDING_MIN_GAP` so grouping wins over the default spread.
const GROUP_NEAR_MAX: f32 = 1.2;

/// Cap on scatter props (lamp/plant/TV) rested on support surfaces per room, so a desk isn't buried.
const SCATTER_PER_ROOM: usize = 3;

/// Inset (metres) from a support surface's footprint edge to the usable top, so a rested prop doesn't
/// perch flush with the edge and read as about to fall off.
const SURFACE_INSET: f32 = 0.08;

/// Wall lights placed per room — a sparse accent on a full-height wall (see the wall-anchor pass).
const WALL_LIGHTS_PER_ROOM: usize = 1;

/// Height up the wall to seat a wall light's origin (a sconce sits above head height).
const WALL_LIGHT_HEIGHT: f32 = 1.8;

/// The parsed furniture catalogue, held in the ECS world for the furnish pass.
#[derive(Resource)]
pub struct Manifest(pub FurnitureManifest);

/// A resolved thing to spawn — computed in the parallel solve phase, consumed by the serial spawn.
struct SpawnReq {
    region: RegionId,
    glb: String,
    pos: Vec3,
    rot: Quat,
}

/// True when a manifest item offers affordance `aff` (e.g. "sit", "emit") — the portable, kit-agnostic
/// way to reason about what a piece is *for* (Fisher 2012; Qi 2018), rather than its kit-specific key.
fn affords(item: &ManifestItem, aff: &str) -> bool {
    item.affordances.iter().any(|a| a == aff)
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
    chosen
}

/// Full-height wall faces `(face_point, inward_normal)` bounding a region — the W/N faces (inward
/// normals +X/+Z). The camera-facing E/S knee walls (normals -X/-Z, squashed to `CAMERA_WALL_FRACTION`)
/// are excluded so a wall-mounted prop never floats in the cutaway gap above a short wall — the same
/// rule the nest placement uses (`crab.rs`). Only border cells can carry a bounding wall.
fn full_height_wall_faces(dungeon: &Dungeon, region: &Region) -> Vec<(Vec3, Vec3)> {
    let (mn, mx) = (region.rect.min, region.rect.max);
    let mut faces = Vec::new();
    for cx in mn[0]..mx[0] {
        for cz in mn[1]..mx[1] {
            if cx != mn[0] && cx != mx[0] - 1 && cz != mn[1] && cz != mx[1] - 1 {
                continue; // interior cell — no bounding wall
            }
            if !dungeon.is_floor(IVec2::new(cx, cz)) {
                continue; // a notched-out corner of a non-rectangular room — no real wall here
            }
            let center = dungeon.cell_center(IVec2::new(cx, cz));
            for (face, normal) in dungeon.wall_faces_near(center) {
                if !crate::dungeon::SHORT_CAMERA_WALLS || !crate::dungeon::is_camera_facing(normal)
                {
                    faces.push((face, normal));
                }
            }
        }
    }
    faces
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
    assets: Res<AssetServer>,
) {
    let parts = RolePartitions::from_catalogue(&manifest.0);

    // ---- Parallel solve: each region is independent, so fan out over rayon. ----
    let orchestrator = &solvers.0;
    let requests: Vec<SpawnReq> = dungeon
        .regions
        .par_iter()
        .flat_map_iter(|region| furnish_region(&dungeon, orchestrator, region, &parts))
        .collect();

    // ---- Serial spawn. ----
    for req in requests {
        let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(req.glb));
        commands.spawn((
            PlacedIn(req.region),
            WorldAssetRoot(scene),
            Transform::from_translation(req.pos)
                .with_rotation(req.rot)
                .with_scale(Vec3::splat(FURNITURE_SCALE)),
            // Starts hidden; `furniture_room_visibility` shows it only while the squad is in its room.
            Visibility::Hidden,
        ));
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
            });
        }
    }
    // No doors — the Backrooms look leaves every opening as a bare doorway (the dungeon still
    // frames each with a header lintel, so it reads as a doorway, just without a door).

    // Pass 1b — wall lights: sconces on the room's full-height walls. Kit-agnostic — any
    // manifest item with role Anchor{Wall} is placed here, so an asset kit lights its rooms
    // with zero code changes (the Stage-5 asset-swap contract). Camera-facing knee walls are
    // skipped (see `full_height_wall_faces`) so a light never floats in the cutaway gap.
    if let Some(light) = wall_lights.first() {
        let faces = full_height_wall_faces(&dungeon, region);
        let n = faces.len();
        for i in 0..WALL_LIGHTS_PER_ROOM.min(n) {
            // Space the lights evenly along the collected faces (mid-wall for a single light).
            let (face, normal) = faces[(i * n + n / 2) / WALL_LIGHTS_PER_ROOM.max(1)];
            let pos = face.with_y(WALL_LIGHT_HEIGHT) + normal * 0.02;
            // Yaw the sconce so its front points into the room along the inward normal.
            let yaw = normal.x.atan2(normal.z);
            out.push(SpawnReq {
                region: region.id,
                glb: light.glb.clone(),
                pos,
                rot: Quat::from_rotation_y(yaw),
            });
        }
    }

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
        for p in placed.into_iter().take(TILED_PER_ROOM) {
            if let Some(item) = tiled.get(p.candidate) {
                let cell = IVec2::new(p.pos[0] as i32, p.pos[2] as i32);
                let pos = dungeon.cell_center(cell);
                // Footprint-aware containment: the WFC solver scatters over the bounding rect,
                // so reject any slot whose *body* would cross a wall or fall in a notched-out
                // corner of a non-rectangular room — not just a center-cell test (README
                // ISSUES 1 & 2). Merrell et al. 2011 free-configuration-space non-penetration.
                let half = footprint_half(item);
                if !dungeon.footprint_on_floor(pos, half, p.yaw) {
                    continue;
                }
                out.push(SpawnReq {
                    region: region.id,
                    glb: item.glb.clone(),
                    pos,
                    rot: Quat::from_rotation_y(p.yaw),
                });
            }
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
        FREESTANDING_PER_ROOM,
    );
    if !profile.is_empty() {
        let candidates: Arc<[Candidate]> = profile
            .iter()
            .map(|it| to_candidate(it))
            .collect::<Vec<_>>()
            .into();
        let constraints = freestanding_constraints(&profile);
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
                out.push(SpawnReq {
                    region: region.id,
                    glb: item.glb.clone(),
                    pos,
                    rot: Quat::from_rotation_y(p.yaw),
                });
                if affords(item, "support") {
                    placed_supports.push((*item, pos, p.yaw));
                }
            }
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
            let chosen: Vec<&ManifestItem> = (0..scatter.len().min(SCATTER_PER_ROOM))
                .map(|k| scatter[(start + k) % scatter.len()])
                .collect();
            let props: Vec<scatter::ScatterProp> = chosen
                .iter()
                .enumerate()
                .map(|(i, it)| scatter::ScatterProp {
                    candidate: i,
                    half_x: it.footprint.0 * 0.5 * FURNITURE_SCALE,
                    half_z: it.footprint.1 * 0.5 * FURNITURE_SCALE,
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
                    });
                }
            }
        }
    }
    out
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
fn freestanding_constraints(profile: &[&ManifestItem]) -> Vec<Constraint> {
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
                Predicate::Near(GROUP_NEAR_MAX)
            } else {
                Predicate::MinDistance(FREESTANDING_MIN_GAP)
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

/// Rooms that have entered the squad's line of sight at least once. Furniture reveal is one-way and
/// per-room: a region is inserted the first frame any of its cells is `seen`, and its furniture then
/// stays visible for the rest of the run — the same permanent reveal the floor/wall tiles use.
#[derive(Resource, Default)]
pub struct RevealedRooms(pub HashSet<RegionId>);

/// Reveal a room's furniture the first time the squad gains line of sight into it, and keep it visible
/// thereafter (remembered, per-room). The knee-wall layout lets the camera see into every room, so
/// gating on live *occupancy* left explored rooms reading empty until re-entry; instead we key off the
/// fog's permanent `seen` memory ([`FogGrid::seen_at`]). Owns furniture `Visibility` exclusively (fog
/// never touches furniture), so nothing else fights it. One-way — only ever flips Hidden→Visible.
pub fn furniture_room_visibility(
    fog: Res<FogGrid>,
    dungeon: Res<Dungeon>,
    mut revealed: ResMut<RevealedRooms>,
    mut furniture: Query<(&PlacedIn, &mut Visibility)>,
) {
    // Grow the revealed set: a region is revealed once any of its interior cells has been seen.
    // Already-revealed regions are skipped (one-way), so this settles to a cheap membership check.
    for region in &dungeon.regions {
        if revealed.0.contains(&region.id) {
            continue;
        }
        let (mn, mx) = (region.rect.min, region.rect.max);
        'scan: for cx in mn[0]..mx[0] {
            for cz in mn[1]..mx[1] {
                if fog.seen_at(IVec2::new(cx, cz)) {
                    revealed.0.insert(region.id);
                    break 'scan;
                }
            }
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
            let profile = room_profile(region_id, &living, &refs, FREESTANDING_PER_ROOM);
            let has_seat = profile.iter().any(|it| affords(it, "sit"));
            let has_screen = profile.iter().any(|it| affords(it, "emit"));
            assert!(
                !has_seat || has_screen,
                "living room {region_id} picked a seat but no screen: {:?}",
                profile.iter().map(|it| it.key.as_str()).collect::<Vec<_>>()
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
            let mut k: Vec<&str> = room_profile(rid, &living, &refs, FREESTANDING_PER_ROOM)
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
        let cs = freestanding_constraints(&refs);

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
    fn freestanding_constraints_are_kit_agnostic_and_spread() {
        // A seat + screen chosen by AFFORDANCE (not by the keys "sofa"/"tv") must still emit the Facing
        // relation, and every pair must get a spreading MinDistance.
        let items = vec![
            item("couch", &["lounge"], &["sit"]),
            item("monitor", &["lounge"], &["emit"]),
        ];
        let refs: Vec<&ManifestItem> = items.iter().collect();
        let cs = freestanding_constraints(&refs);
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
        use crate::placement::manifest::load_manifest;
        use crate::placement::solver::Orchestrator;
        use crate::placement::solvers::constraint::ConstraintSolver;
        use crate::placement::solvers::metropolis::{MetropolisSolver, MetropolisWeights};
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

        let catalogue = load_manifest("assets/placement/furniture.ron").expect("shipped manifest");
        let parts = RolePartitions::from_catalogue(&catalogue);
        let weights: MetropolisWeights =
            ron::from_str(&std::fs::read_to_string("assets/placement/metropolis.ron").expect("weights"))
                .expect("parse weights");
        let mut orch = Orchestrator::new();
        orch.register(Box::new(WfcSolver));
        orch.register(Box::new(MetropolisSolver::new(weights)));
        orch.register(Box::new(ConstraintSolver));

        let reqs = furnish_region(&dungeon, &orch, &dungeon.regions[0], &parts);
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
