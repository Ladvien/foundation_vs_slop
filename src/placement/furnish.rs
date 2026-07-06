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

use bevy::prelude::*;
use rayon::prelude::*;

use crate::dungeon::{Dungeon, WALL_HEIGHT};
use crate::rng::seeded;
use crate::squad::Unit;

use super::ir::{
    Candidate, Constraint, Dof, Host, Modality, Outcome, Placement, PlacementProblem,
    Predicate, Region, RegionId, Role, Scope,
};
use super::manifest::{FurnitureManifest, ManifestItem};
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

/// Pick a freestanding furniture set for a region from whatever the manifest offers, keyed by a
/// stand-in room "type" (`region_id % 4`, a Stage-4+ region-typing placeholder). Selection is by
/// semantic TAGS, never by hardcoded asset keys, so any asset kit furnishes rooms with zero code
/// changes — the Stage-5 asset-swap contract (Tutenel et al. semantic room classes `[home-still:
/// cgf.12276]`; Merrell et al. 2011). Returns up to `count` distinct items; a room whose type-tags
/// aren't in the kit still gets furniture via the top-up cycle, so it's never left empty.
fn room_profile<'a>(
    region_id: RegionId,
    freestanding: &[&'a ManifestItem],
    count: usize,
) -> Vec<&'a ManifestItem> {
    if freestanding.is_empty() || count == 0 {
        return Vec::new();
    }
    // Room "type" → preferred semantic tags (the room class). A kit that tags its items reproduces
    // themed rooms; a kit that tags differently (or not at all) still furnishes via the cycle below.
    let preferred: &[&str] = match region_id % 4 {
        0 => &["living"],
        1 => &["bedroom"],
        2 => &["kitchen", "dining"],
        _ => &["study", "living"],
    };
    let mut chosen: Vec<&ManifestItem> = Vec::new();
    // First, items whose tags match this room's type.
    for it in freestanding {
        if chosen.len() >= count {
            break;
        }
        if it.tags.iter().any(|t| preferred.contains(&t.as_str())) {
            chosen.push(it);
        }
    }
    // Then top up from the whole freestanding set, offset by region so adjacent rooms differ, so a
    // room is never empty just because the kit lacks the preferred tags. Scanning `n` consecutive
    // offsets visits every item exactly once, so this fills to `min(count, n)` and always terminates.
    let n = freestanding.len();
    for k in 0..n {
        if chosen.len() >= count {
            break;
        }
        let cand = freestanding[(region_id as usize + k) % n];
        if !chosen.iter().any(|c| c.key == cand.key) {
            chosen.push(cand);
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
            let center = dungeon.cell_center(IVec2::new(cx, cz));
            for (face, normal) in dungeon.wall_faces_near(center) {
                if !crate::dungeon::SHORT_CAMERA_WALLS
                    || (normal != Vec3::NEG_X && normal != Vec3::NEG_Z)
                {
                    faces.push((face, normal));
                }
            }
        }
    }
    faces
}

/// Furnish every region. Parallel solve → serial spawn.
pub fn furnish_regions(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    solvers: Res<PlacementSolvers>,
    manifest: Res<Manifest>,
    assets: Res<AssetServer>,
) {
    let catalogue = &manifest.0;
    let ceiling = catalogue.by_role(|r| matches!(r, Role::Anchor { host: Host::Ceiling }));
    let wall_lights = catalogue.by_role(|r| matches!(r, Role::Anchor { host: Host::Wall }));
    let tiled = catalogue.by_role(|r| matches!(r, Role::Tiled));
    let tiled_candidates: Vec<Candidate> = tiled.iter().map(|it| to_candidate(it)).collect();
    let freestanding = catalogue.by_role(|r| matches!(r, Role::Freestanding));

    // ---- Parallel solve: each region is independent, so fan out over rayon. ----
    let orchestrator = &solvers.0;
    let requests: Vec<SpawnReq> = dungeon
        .regions
        .par_iter()
        .flat_map_iter(|region| {
            let mut rng = seeded(PLACEMENT_SEED ^ splitmix64(region.id as u64));
            let mut out: Vec<SpawnReq> = Vec::new();

            // Pass 1 — anchors.
            if let Some(item) = ceiling.first() {
                let c = region.rect.center_cell();
                let pos = dungeon.cell_center(IVec2::new(c[0], c[1])).with_y(WALL_HEIGHT);
                out.push(SpawnReq { region: region.id, glb: item.glb.clone(), pos, rot: Quat::from_rotation_x(PI) });
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
                    out.push(SpawnReq { region: region.id, glb: light.glb.clone(), pos, rot: Quat::from_rotation_y(yaw) });
                }
            }

            // Pass 2 — tiled scatter (→ WfcSolver).
            if !tiled_candidates.is_empty() {
                let problem = PlacementProblem { region, candidates: tiled_candidates.clone(), constraints: Vec::new() };
                for p in solve_placements(orchestrator, &problem, &mut rng, region.id, "tiled")
                    .into_iter()
                    .take(TILED_PER_ROOM)
                {
                    if let Some(item) = tiled.get(p.candidate) {
                        let pos = dungeon.cell_center(IVec2::new(p.pos[0] as i32, p.pos[2] as i32));
                        out.push(SpawnReq { region: region.id, glb: item.glb.clone(), pos, rot: Quat::from_rotation_y(p.yaw) });
                    }
                }
            }

            // Pass 3 — freestanding furniture (→ MetropolisSolver). Kit-agnostic: the set is drawn
            // from the manifest's Freestanding items by semantic room-type tags, never hardcoded asset
            // keys, so any asset kit furnishes rooms with zero code changes (Tutenel et al. semantic
            // room classes; Merrell et al. 2011 — the Stage-5 asset-swap contract).
            let profile = room_profile(region.id, &freestanding, FREESTANDING_PER_ROOM);
            if !profile.is_empty() {
                let candidates: Vec<Candidate> = profile.iter().map(|it| to_candidate(it)).collect();
                let constraints = freestanding_constraints(&profile);
                let problem = PlacementProblem { region, candidates, constraints };
                for p in solve_placements(orchestrator, &problem, &mut rng, region.id, "freestanding") {
                    if let Some(item) = profile.get(p.candidate) {
                        // Freestanding solver works in world/tile coords already.
                        let pos = Vec3::new(p.pos[0], 0.0, p.pos[2]);
                        out.push(SpawnReq { region: region.id, glb: item.glb.clone(), pos, rot: Quat::from_rotation_y(p.yaw) });
                    }
                }
            }
            out
        })
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
        Ok(Outcome::Ranked(ranked)) => ranked.into_iter().next().map(|(_, p)| p).unwrap_or_default(),
        Err(e) => {
            warn!("placement: region {region} {label} pass unsolved: {e}");
            Vec::new()
        }
    }
}

/// Soft constraints for a freestanding set: every item prefers its back to a wall (which also makes
/// the group Soft+Relational so the orchestrator routes it to Metropolis, not WFC), and a sofa is
/// asked to face a TV if both are present.
fn freestanding_constraints(profile: &[&ManifestItem]) -> Vec<Constraint> {
    let mut constraints = Vec::new();
    let mut id = 0u32;
    for (i, _) in profile.iter().enumerate() {
        constraints.push(Constraint {
            id,
            scope: Scope::Object(i),
            predicate: Predicate::AgainstWall,
            modality: Modality::Soft(1.0),
            guard: None,
        });
        id += 1;
    }
    let sofa = profile.iter().position(|it| it.key == "sofa");
    let tv = profile.iter().position(|it| it.key == "tv");
    if let (Some(s), Some(t)) = (sofa, tv) {
        constraints.push(Constraint {
            id,
            scope: Scope::Object(s),
            predicate: Predicate::Facing(t),
            modality: Modality::Soft(1.0),
            guard: None,
        });
    }
    constraints
}

/// Show furniture only in rooms a squad member currently occupies; hide everything else. Owns
/// furniture `Visibility` exclusively (furniture is not fog-managed), so nothing else fights it.
pub fn furniture_room_visibility(
    units: Query<&Transform, With<Unit>>,
    dungeon: Res<Dungeon>,
    mut furniture: Query<(&PlacedIn, &mut Visibility)>,
) {
    let occupied: HashSet<RegionId> = units
        .iter()
        .filter_map(|t| dungeon.region_at(dungeon.world_to_cell(t.translation)))
        .collect();
    for (placed, mut vis) in &mut furniture {
        let want = if occupied.contains(&placed.0) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
    }
}

/// Map a manifest entry to an IR candidate (asset key + role + footprint + affordances).
fn to_candidate(item: &ManifestItem) -> Candidate {
    Candidate {
        asset: item.key.clone(),
        role: item.role.clone(),
        // Footprints in rendered (scaled) units so the layout solver reasons at the size we draw.
        footprint: [item.footprint.0 * FURNITURE_SCALE, item.footprint.1 * FURNITURE_SCALE],
        dof: Dof { translate: true, rotate_quarter: true, rotate_free: false },
        affordances: item.affordances.clone(),
    }
}

