//! WHERE the mold may live — the placement decision the colony never used to make.
//!
//! Before this module the mold grew on `dungeon.is_floor(cell)`: every room, every corridor, the whole
//! dungeon coated to the same depth. That is not how fungal colonies distribute. Substrate, moisture, and
//! competition are patchy at the scale of a room, and a colony is patchy with them (Boddy, "Saprotrophic
//! cord-forming fungi", *Mycologia* 91(1):13, doi:10.2307/3761190 — mycelial networks forage across a
//! resource-heterogeneous floor rather than tiling it).
//!
//! So this builds a **habitat mask** once, at startup, from the realized dungeon:
//!
//! - **Rooms get patches.** A subset of rooms is infested; each infested room carries blue-noise patch
//!   nuclei with fbm-ragged borders. Most rooms are left entirely clean, which is what makes walking into a
//!   moldy one an event rather than the background.
//! - **Corridors get all or nothing.** A corridor *run* — one adjacency edge, room to room — is either fully
//!   infested or bare. A passage you dread is a passage, not a spot.
//! - **Damp rooms rot.** A room's susceptibility is scaled by its type tag, so a bathroom rots and an office
//!   rarely does. This reuses the same `Region::props` hook `placement::furnish` reads.
//!
//! # Why the coverage target is met by selection, not by tuning
//!
//! The caller asks for a fraction of walkable floor ([`MyceliaConfig::habitat_coverage`]). Hitting that by
//! bisecting a patch radius would work, but it would trade away the look: on a dungeon with few rooms the
//! radius would inflate until patches merged and each infested room became uniformly coated — precisely the
//! thing being fixed. So patch **geometry is drawn first and never touched**, and only the **number of
//! infested rooms** varies: rooms are ranked by susceptibility and accepted greedily until the cell budget is
//! spent. An infested room therefore always looks heavily patched, whatever the dungeon.
//!
//! Achieved coverage is measured from the finished mask and always logged. It is never silently clamped: a
//! dungeon whose corridors alone overshoot the budget gets a `warn!`, not a quietly-rewritten target.
//!
//! # The coverage target and the corridor dial are coupled
//!
//! Worth knowing before you turn either. This dungeon is only about a **third room floor** by area (1179 room
//! cells against 2349 corridor cells at the shipped seed), so a coverage target expressed as a fraction of
//! *walkable floor* is mostly a claim on corridors. Raise `habitat_coverage` without raising
//! `corridor_infest_chance` and the greedy has nowhere to find the cells but rooms, so it infests nearly all
//! of them and nothing is left clean (measured: 17 of 24 rooms at `0.25 / 0.12`). Raise
//! `corridor_infest_chance` to compensate and the colony migrates into the halls, which is the one thing this
//! module exists to prevent (measured: 57% of the mold in corridors at `0.25 / 0.30`).
//!
//! `the_shipped_config_delivers_the_intended_level` asserts all three properties at once — coverage, clean
//! rooms, and mold-lives-in-rooms — precisely because satisfying any two of them is easy.
//!
//! # Determinism
//!
//! Every draw flows from [`HABITAT_SEED`] through `rng::seeded` (ChaCha8), split per region and per corridor
//! edge with `splitmix64` — the `placement::furnish` idiom, so the result is independent of iteration order.
//! The mask is pure CPU, computed once, and touches no pinned state, so it stays outside `snapshot_hash`.
//!
//! # Resolution
//!
//! The mask is built at **field** resolution (`cfg.field_size`, 1024² ≈ 5.3 texels/tile), not cell
//! resolution. A cell-resolution mask would give every patch a tile-blocky border. It is quantized to `u8`
//! here, because `u8` is what crosses to the GPU in the static control texture's `G` channel — and the CPU
//! agent seeder must threshold *exactly* the bytes the shader's hard block will threshold, or an agent could
//! be seeded on a texel the GPU then refuses to let it leave.

use bevy::prelude::*;

use crate::dungeon::Dungeon;
use crate::geom::poisson_disk;
use crate::placement::splitmix64;
use crate::rng::{seeded, DetRng};

use super::{MyceliaConfig, CONTROL_SIZE};

/// Base seed for every habitat draw. Fixed, so a dungeon seed maps to one colony layout.
const HABITAT_SEED: u64 = 0xB105_FEED_C0DE;

/// Salt separating the susceptibility draw from the patch-geometry draw on the same region id.
const SCORE_SALT: u64 = 0xA55E_5510;

/// Salt separating the corridor-run roll from every other draw.
const RUN_SALT: u64 = 0xC077_1D02;

/// Habitat at or above which a cell counts as *covered* when measuring against
/// [`MyceliaConfig::habitat_coverage`]. This is the solid core of a patch, not its faint fringe — which is
/// why it sits well above `agent_hab_min` (agents may wander into the fringe; the player does not see mold
/// there).
const COVERED: f32 = 0.5;

/// Candidate attempts per active sample in the Bridson sampler. The `geom` default used elsewhere.
const POISSON_K: usize = 30;

/// Octaves of value noise in the border fbm. Three gives a ragged edge with a ~2-texel finest feature at the
/// shipped `edge_noise_scale`; more is invisible under the material's own `margin_roughness`.
const FBM_OCTAVES: u32 = 3;

/// Fraction of a nucleus's radius given over to its soft margin. Inside `r * (1 - EDGE_BAND)` a patch is
/// solid; beyond it the value ramps to zero and the fbm displaces the contour.
///
/// This is a **shape** constant, not an aesthetic dial — `patch_radius_*` sizes a patch and `edge_noise_amp`
/// roughens it. It exists because the obvious falloff, a linear cone `1 - d/r` fed straight to a smoothstep,
/// is wrong: it only crosses 0.5 at `d = r/2`, so every patch renders at *half* its nominal radius and a room
/// reads as light speckle rather than a colony. (Measured: the cone gave 20.2% coverage with **every** room
/// infested — the greedy exhausted the rooms before it could spend the budget, and the "most rooms are clean"
/// contrast this module exists to create never appeared.) A solid core with a ragged rim is both what a mat
/// actually looks like and what lets a few rooms carry the whole quota.
const EDGE_BAND: f32 = 0.30;

// ── Deterministic value noise (CPU) ───────────────────────────────────────────────────────────────────
//
// The codebase has fbm in WGSL only; nothing on the CPU could perturb a border. This is the smallest thing
// that works: a `splitmix64`-hashed integer lattice, smoothstep-faded bilinear interpolation, summed over
// octaves. Reusing `splitmix64` (the placement hash of record) keeps one hash in the codebase rather than
// two. Output is in `[0, 1)`.

/// A `[0, 1)` float from a 64-bit key. Takes the top 24 bits, which is all an `f32` mantissa can hold.
fn hash01(key: u64) -> f32 {
    (splitmix64(key) >> 40) as f32 / (1u64 << 24) as f32
}

/// The pseudo-random value at integer lattice point `(ix, iy)`.
fn lattice(ix: i32, iy: i32, seed: u64) -> f32 {
    let kx = (ix as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let ky = (iy as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    hash01(kx ^ ky ^ seed)
}

/// Bilinear value noise with a smoothstep fade, in `[0, 1)`.
fn vnoise(x: f32, y: f32, seed: u64) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (i, j) = (fx as i32, fy as i32);
    let (tx, ty) = (x - fx, y - fy);
    // Smoothstep fade — a linear fade makes the lattice grid visible as creases.
    let ux = tx * tx * (3.0 - 2.0 * tx);
    let uy = ty * ty * (3.0 - 2.0 * ty);

    let a = lattice(i, j, seed);
    let b = lattice(i + 1, j, seed);
    let c = lattice(i, j + 1, seed);
    let d = lattice(i + 1, j + 1, seed);
    let top = a + (b - a) * ux;
    let bot = c + (d - c) * ux;
    top + (bot - top) * uy
}

/// Fractional Brownian motion over [`vnoise`]: `FBM_OCTAVES` octaves, lacunarity 2, gain 0.5, normalized to
/// `[0, 1)`.
fn fbm(x: f32, y: f32, seed: u64) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0f32, 0.5f32, 1.0f32, 0.0f32);
    for o in 0..FBM_OCTAVES {
        sum += amp * vnoise(x * freq, y * freq, seed ^ u64::from(o).wrapping_mul(0x9E37_79B9));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

// ── Patch geometry ────────────────────────────────────────────────────────────────────────────────────

/// One patch nucleus inside a room, in **cell** coordinates (absolute, not rect-relative).
#[derive(Clone, Copy)]
struct Nucleus {
    x: f32,
    y: f32,
    radius: f32,
}

/// Habitat contributed by one nucleus at cell-space point `(x, y)`, in `[0, 1]`.
///
/// Solid inside `r * (1 - EDGE_BAND)`, ramping to zero at `r`. The fbm is added *before* the smoothstep, so
/// the noise displaces the **contour** rather than merely dimming the fill — that is what turns a disc into a
/// colony margin instead of a blurry dot.
fn nucleus_value(n: &Nucleus, x: f32, y: f32, cfg: &MyceliaConfig, seed: u64) -> f32 {
    let d = ((x - n.x).powi(2) + (y - n.y).powi(2)).sqrt();
    // `t` is 0 at the nominal radius and 1 at the inner edge of the margin, so it saturates across the core.
    let t = (1.0 - d / n.radius) / EDGE_BAND;
    let noise = fbm(x * cfg.edge_noise_scale, y * cfg.edge_noise_scale, seed) - 0.5;
    smoothstep01(t + cfg.edge_noise_amp * noise)
}

/// `smoothstep(0, 1, t)`.
fn smoothstep01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The furthest a nucleus can reach once fbm has displaced its contour outward.
///
/// `nucleus_value` is positive while `(1 - d/r)/EDGE_BAND + amp*(fbm - 0.5) > 0`, and `fbm - 0.5 < 0.5`, so
/// the contour never escapes `r * (1 + EDGE_BAND * amp / 2)`. Rasterizing that bound is exact, not a guess —
/// which matters, because a bound that is too tight would square off the patch at the rasterizer's edge.
fn nucleus_reach(n: &Nucleus, cfg: &MyceliaConfig) -> f32 {
    n.radius * (1.0 + EDGE_BAND * cfg.edge_noise_amp * 0.5)
}

/// Draw a room's patch nuclei. Geometry only — this says nothing about whether the room is *selected*.
fn room_nuclei(rect: &crate::placement::ir::Rect2, cfg: &MyceliaConfig, id: u32) -> Vec<Nucleus> {
    let mut rng = seeded(HABITAT_SEED ^ splitmix64(u64::from(id)));
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    if w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }
    poisson_disk(w, h, f64::from(cfg.patch_spacing), POISSON_K, &mut rng)
        .into_iter()
        .map(|p| {
            let t = rng.unit() as f32;
            Nucleus {
                x: rect.min[0] as f32 + p[0] as f32,
                y: rect.min[1] as f32 + p[1] as f32,
                radius: cfg.patch_radius_min + t * (cfg.patch_radius_max - cfg.patch_radius_min),
            }
        })
        .collect()
}

/// Habitat at cell-space point `(x, y)` from a room's whole nucleus set — the union, as a max.
fn room_value(nuclei: &[Nucleus], x: f32, y: f32, cfg: &MyceliaConfig, seed: u64) -> f32 {
    let mut best = 0.0f32;
    for n in nuclei {
        let dx = x - n.x;
        let dy = y - n.y;
        let reach = nucleus_reach(n, cfg);
        if dx * dx + dy * dy > reach * reach {
            continue;
        }
        best = best.max(nucleus_value(n, x, y, cfg, seed));
        if best >= 1.0 {
            break;
        }
    }
    best
}

// ── Build ─────────────────────────────────────────────────────────────────────────────────────────────

/// A room's patch geometry and how much of its own floor that patch would cover.
///
/// The cells a region owns live in the shared `owner` map rather than here: ownership is a *partition* of
/// room floor (lowest region id wins an overlapping rect), which is what stops `Σ covered_cells` from
/// double-counting a cell into the coverage budget twice.
struct RoomPlan {
    id: u32,
    nuclei: Vec<Nucleus>,
    /// Owned cells whose centre would be under a patch. Fixed: it does not depend on selection.
    covered_cells: usize,
    /// Susceptibility. Higher rots first.
    score: f32,
}

/// Build the habitat mask at field resolution, quantized to the `u8` the GPU will read.
///
/// Fails loudly on a dungeon with no floor, or a region whose room type the damp table does not name — both
/// are contract violations upstream, not conditions to degrade around.
pub fn build(dungeon: &Dungeon, cfg: &MyceliaConfig) -> Result<Vec<u8>, String> {
    let cells = CONTROL_SIZE as usize;
    if dungeon.width != cells || dungeon.height != cells {
        return Err(format!(
            "mycelia::habitat: dungeon is {}x{}, expected {cells}x{cells}",
            dungeon.width, dungeon.height
        ));
    }
    let cell_at = |i: usize| IVec2::new((i % cells) as i32, (i / cells) as i32);

    let floor_total = dungeon.floor_cells().count();
    if floor_total == 0 {
        return Err("mycelia::habitat: dungeon has no walkable floor".to_string());
    }

    // ── Corridors: one all-or-nothing roll per run ────────────────────────────────────────────────────
    let mut corridor_cells: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
    for i in 0..cells * cells {
        if let Some(edge) = dungeon.corridor_id(cell_at(i)) {
            corridor_cells.entry(edge).or_default().push(i);
        }
    }
    let infested_runs: Vec<u32> = corridor_cells
        .keys()
        .copied()
        .filter(|&e| hash01(HABITAT_SEED ^ splitmix64(u64::from(e) ^ RUN_SALT)) < cfg.corridor_infest_chance)
        .collect();
    let corridor_budget: usize =
        infested_runs.iter().map(|e| corridor_cells[e].len()).sum();

    // ── Rooms: partition the room floor, then draw each room's patches ────────────────────────────────
    // Lowest region id wins an overlap, so `owned` is a partition however the rects are arranged.
    let mut owner = vec![u32::MAX; cells * cells];
    for region in &dungeon.regions {
        for i in 0..cells * cells {
            let c = cell_at(i);
            if owner[i] == u32::MAX
                && dungeon.is_floor(c)
                && !dungeon.is_corridor(c)
                && region.rect.contains([c.x, c.y])
            {
                owner[i] = region.id;
            }
        }
    }

    let mut plans: Vec<RoomPlan> = Vec::with_capacity(dungeon.regions.len());
    for region in &dungeon.regions {
        let nuclei = room_nuclei(&region.rect, cfg, region.id);
        let seed = HABITAT_SEED ^ splitmix64(u64::from(region.id));
        let covered_cells = (0..cells * cells)
            .filter(|&i| owner[i] == region.id)
            .filter(|&i| {
                let c = cell_at(i);
                room_value(&nuclei, c.x as f32 + 0.5, c.y as f32 + 0.5, cfg, seed) >= COVERED
            })
            .count();
        let damp = cfg.damp_weight(&region.props.tags)?;
        plans.push(RoomPlan {
            id: region.id,
            nuclei,
            covered_cells,
            score: hash01(seed ^ SCORE_SALT) * damp,
        });
    }

    // ── Greedy selection to the cell budget ───────────────────────────────────────────────────────────
    // Rank by susceptibility, tie-break on id so the order is total and reproducible.
    let mut order: Vec<usize> = (0..plans.len()).collect();
    // SORT-OK: seeded habitat bake over grid cells, not an ECS query.
    order.sort_by(|&a, &b| {
        plans[b]
            .score
            .partial_cmp(&plans[a].score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(plans[a].id.cmp(&plans[b].id))
    });

    let target_cells = (cfg.habitat_coverage * floor_total as f32).round() as i64;
    let room_budget = target_cells - corridor_budget as i64;
    let mut selected: Vec<usize> = Vec::new();
    let mut sum: i64 = 0;
    if room_budget > 0 {
        for &p in &order {
            let a = plans[p].covered_cells as i64;
            if a == 0 {
                continue;
            }
            if sum + a <= room_budget {
                selected.push(p);
                sum += a;
                continue;
            }
            // This room crosses the budget. Take it only if landing past the target is *closer* than
            // stopping short of it — otherwise one sprawling hall would swamp the whole quota. Either way
            // the budget is spent, so stop.
            if (sum + a - room_budget) < (room_budget - sum) {
                selected.push(p);
            }
            break;
        }
    }

    // ── Rasterize at field resolution ─────────────────────────────────────────────────────────────────
    let field = cfg.field_size as usize;
    let mut mask = vec![0.0f32; field * field];
    // Cell units per field texel. Texel `t` centres at `(t + 0.5) * cells_per_texel` in cell space, whose
    // integer part is its dungeon cell — the exact inverse of the shader's texel→cell map.
    let cells_per_texel = cells as f32 / field as f32;

    // Rooms: walk each selected nucleus's exact reach, clipped to its own region's owned cells.
    for &p in &selected {
        let plan = &plans[p];
        let seed = HABITAT_SEED ^ splitmix64(u64::from(plan.id));
        for n in &plan.nuclei {
            let reach = nucleus_reach(n, cfg);
            let lo_x = (((n.x - reach) / cells_per_texel).floor() as i64).max(0) as usize;
            let hi_x = (((n.x + reach) / cells_per_texel).ceil() as i64).min(field as i64 - 1) as usize;
            let lo_y = (((n.y - reach) / cells_per_texel).floor() as i64).max(0) as usize;
            let hi_y = (((n.y + reach) / cells_per_texel).ceil() as i64).min(field as i64 - 1) as usize;
            for ty in lo_y..=hi_y {
                let cy = (ty as f32 + 0.5) * cells_per_texel;
                for tx in lo_x..=hi_x {
                    let cx = (tx as f32 + 0.5) * cells_per_texel;
                    let ci = (cy as usize) * cells + (cx as usize);
                    // Clip to this room's own floor. Rock, corridors, and other rooms are not its habitat.
                    if ci >= owner.len() || owner[ci] != plan.id {
                        continue;
                    }
                    let v = nucleus_value(n, cx, cy, cfg, seed);
                    let slot = &mut mask[ty * field + tx];
                    if v > *slot {
                        *slot = v;
                    }
                }
            }
        }
    }

    // Corridors: solid. A run is infested end to end, so there is no border to ragged — the visible edge is
    // the doorway, and a colony stopping at a threshold is exactly right. The material's `margin_roughness`
    // breaks the contour where it meets bare floor.
    for e in &infested_runs {
        for &ci in &corridor_cells[e] {
            let (cx, cy) = (ci % cells, ci / cells);
            let lo_x = ((cx as f32) / cells_per_texel).floor() as usize;
            let hi_x = (((cx + 1) as f32) / cells_per_texel).ceil() as usize;
            let lo_y = ((cy as f32) / cells_per_texel).floor() as usize;
            let hi_y = (((cy + 1) as f32) / cells_per_texel).ceil() as usize;
            for ty in lo_y..hi_y.min(field) {
                for tx in lo_x..hi_x.min(field) {
                    // Guard the cell mapping: a texel straddling the cell border belongs to its own cell.
                    let bx = ((tx as f32 + 0.5) * cells_per_texel) as usize;
                    let by = ((ty as f32 + 0.5) * cells_per_texel) as usize;
                    if bx == cx && by == cy {
                        mask[ty * field + tx] = 1.0;
                    }
                }
            }
        }
    }

    // ── Quantize, measure, report ─────────────────────────────────────────────────────────────────────
    let bytes: Vec<u8> = mask.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect();

    // Measure the achieved coverage from the FINISHED mask, not from the plan's arithmetic — the rasterizer
    // clips to real floor and the quantizer rounds, and a number that skipped both would be a number about
    // a mask that does not exist.
    let covered_byte = (COVERED * 255.0).round() as u8;
    let covered_cells = (0..cells * cells)
        .filter(|&i| {
            let c = cell_at(i);
            if !dungeon.is_floor(c) {
                return false;
            }
            // The field texel containing this cell's centre.
            let tx = ((c.x as f32 + 0.5) / cells_per_texel) as usize;
            let ty = ((c.y as f32 + 0.5) / cells_per_texel) as usize;
            tx < field && ty < field && bytes[ty * field + tx] >= covered_byte
        })
        .count();
    let achieved = covered_cells as f32 / floor_total as f32;

    // The breakdown, not just the total. Room floor and corridor floor are wildly different fractions of a
    // dungeon (corridors dominate at wide `corridor_width`), and that ratio — not any patch dial — is what
    // decides how many rooms must rot to fund the quota. Without these two numbers the only way to understand
    // "17 of 24 rooms" is to go and measure the dungeon by hand.
    let corridor_floor: usize = corridor_cells.values().map(Vec::len).sum();
    let room_floor = floor_total - corridor_floor;
    info!(
        "mycelia::habitat: {:.1}% of floor infested ({covered_cells}/{floor_total} cells) — \
         {} of {} rooms, {} of {} corridor runs. \
         Floor is {room_floor} room + {corridor_floor} corridor; runs claim {}, rooms fund {}.",
        achieved * 100.0,
        selected.len(),
        plans.len(),
        infested_runs.len(),
        corridor_cells.len(),
        corridor_budget,
        room_budget.max(0),
    );
    // Which rooms rotted, and where. Cheap, and the only way to walk to one on purpose — the alternative is
    // wandering the dungeon hoping to find mold.
    debug!(
        "mycelia::habitat: infested rooms {:?}",
        selected
            .iter()
            .filter_map(|&p| {
                let id = plans[p].id;
                dungeon.regions.iter().find(|r| r.id == id).map(|r| (id, r.rect.center_cell()))
            })
            .collect::<Vec<_>>()
    );

    // Loud, not clamped. A dungeon whose corridor runs alone overshoot the quota is a real, usable colony —
    // just not the one that was asked for, and the operator should know which.
    let miss = (achieved - cfg.habitat_coverage).abs();
    if miss > 0.05 {
        warn!(
            "mycelia::habitat: coverage {:.3} misses the requested {:.3} by {:.3}. \
             Corridor runs alone claim {:.3}; rooms could offer {:.3} more.",
            achieved,
            cfg.habitat_coverage,
            miss,
            corridor_budget as f32 / floor_total as f32,
            plans.iter().map(|p| p.covered_cells).sum::<usize>() as f32 / floor_total as f32,
        );
    }

    Ok(bytes)
}

/// A per-**dungeon-cell** "mold lives here" mask, row-major over the 192² grid. Samples the
/// field-resolution [`build`] mask at each floor cell's centre using the exact texel map + [`COVERED`]
/// threshold `build`'s own coverage measurement uses (the coverage loop above), so the two can never
/// disagree. `true` only on infested floor; rock and clean floor are `false`.
///
/// This is the single source of truth for "mold-colonised concrete" outside the mold itself — used by
/// [`crate::almond_water`] to boost seep where the colony has cracked the concrete. Pure and deterministic
/// (it only reads the seeded, geometry-derived mask), so a caller may bake it into static state.
pub fn infested_cells(dungeon: &Dungeon, cfg: &MyceliaConfig) -> Result<Vec<bool>, String> {
    let bytes = build(dungeon, cfg)?;
    let cells = CONTROL_SIZE as usize;
    let field = cfg.field_size as usize;
    let cells_per_texel = cells as f32 / field as f32;
    let covered_byte = (COVERED * 255.0).round() as u8;
    let mut out = vec![false; cells * cells];
    for c in dungeon.floor_cells() {
        let tx = ((c.x as f32 + 0.5) / cells_per_texel) as usize;
        let ty = ((c.y as f32 + 0.5) / cells_per_texel) as usize;
        if tx < field && ty < field && bytes[ty * field + tx] >= covered_byte {
            out[crate::util::row_major(c, cells)] = true;
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "habitat_tests.rs"]
mod tests;
