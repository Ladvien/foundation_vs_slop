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
mod tests {
    use super::*;
    use crate::placement::ir::{PropertyBag, Rect2, Region};

    /// Room footprint used by [`fixture`], in cells.
    const ROOM: i32 = 10;
    /// Cells of corridor in the fixture's single run (edge 0).
    const RUN: std::ops::Range<usize> = 15..75;

    /// A 192² dungeon: twelve 10×10 rooms on a 4×3 lattice, plus one corridor run (edge 0) clear of them all.
    ///
    /// Twelve rooms rather than two, deliberately: the greedy's granularity is one whole room, so a fixture
    /// with a couple of huge rooms cannot land anywhere near a 25% target and would tell us nothing about the
    /// selection. The shipped dungeon has ~24 rooms.
    fn fixture() -> Dungeon {
        let n = CONTROL_SIZE as usize;
        let mut walkable = vec![false; n * n];
        let mut corridor_of = vec![u32::MAX; n * n];

        // Alternating damp/dry so the susceptibility ordering has something to bite on.
        const TAGS: [&str; 4] = ["bathroom", "office", "kitchen", "bedroom"];
        let mut regions = Vec::new();
        for (i, (gx, gy)) in (0..3).flat_map(|y| (0..4).map(move |x| (x, y))).enumerate() {
            let (x0, y0) = (10 + gx * 20, 10 + gy * 20);
            for y in y0..y0 + ROOM {
                for x in x0..x0 + ROOM {
                    walkable[y as usize * n + x as usize] = true;
                }
            }
            regions.push(Region {
                id: i as u32,
                rect: Rect2 { min: [x0, y0], max: [x0 + ROOM, y0 + ROOM] },
                openings: Vec::new(),
                adjacency: Vec::new(),
                props: PropertyBag { tags: vec!["room".into(), TAGS[i % TAGS.len()].into()] },
            });
        }

        // One corridor run along y = 75, below every room (rooms end at y = 60).
        for x in RUN {
            walkable[75 * n + x] = true;
            corridor_of[75 * n + x] = 0;
        }
        Dungeon::from_parts(n, n, walkable, regions, corridor_of)
    }

    /// How many of the fixture's rooms ended up with any habitat at all?
    fn infested_rooms(d: &Dungeon, bytes: &[u8], field: usize) -> usize {
        d.regions
            .iter()
            .filter(|r| {
                (r.rect.min[1]..r.rect.max[1]).any(|y| {
                    (r.rect.min[0]..r.rect.max[0]).any(|x| at_cell(bytes, field, x, y) > 0)
                })
            })
            .count()
    }

    /// A config with a small field so the tests stay fast, and no corridor infestation unless a test wants it.
    fn cfg(field_size: u32, coverage: f32, corridor_chance: f32) -> MyceliaConfig {
        let mut c = crate::mycelia::tests::valid();
        c.field_size = field_size;
        c.habitat_coverage = coverage;
        c.corridor_infest_chance = corridor_chance;
        c
    }

    /// Read the mask byte at the field texel containing cell `(x, y)`'s centre.
    fn at_cell(bytes: &[u8], field: usize, x: i32, y: i32) -> u8 {
        let cells_per_texel = CONTROL_SIZE as f32 / field as f32;
        let tx = ((x as f32 + 0.5) / cells_per_texel) as usize;
        let ty = ((y as f32 + 0.5) / cells_per_texel) as usize;
        bytes[ty * field + tx]
    }

    fn covered(bytes: &[u8], field: usize, x: i32, y: i32) -> bool {
        at_cell(bytes, field, x, y) >= (COVERED * 255.0).round() as u8
    }

    /// Measured coverage of a built mask, as a fraction of walkable floor.
    fn coverage(d: &Dungeon, bytes: &[u8], field: usize) -> f32 {
        let floor = d.floor_cells().count();
        let cov = d.floor_cells().filter(|c| covered(bytes, field, c.x, c.y)).count();
        cov as f32 / floor as f32
    }

    /// The whole point: a quarter of the floor, not all of it.
    #[test]
    fn coverage_lands_near_the_target() {
        let d = fixture();
        let c = cfg(768, 0.25, 0.0);
        let bytes = build(&d, &c).expect("fixture builds");
        let achieved = coverage(&d, &bytes, 768);
        // The greedy's granularity is one room, so it cannot do better than half a room's worth of error.
        assert!((achieved - 0.25).abs() <= 0.06, "coverage {achieved} should be near 0.25");
        assert!(achieved > 0.0, "some floor must be infested");
    }

    /// The aesthetic requirement, made testable: most rooms are untouched, so walking into a moldy one is an
    /// event. If a future falloff change thins the patches, the greedy will start infesting every room to
    /// spend its budget — and this fails, rather than the mold quietly going back to coating everything.
    #[test]
    fn most_rooms_are_left_completely_clean() {
        let d = fixture();
        let bytes = build(&d, &cfg(768, 0.25, 0.0)).expect("builds");
        let dirty = infested_rooms(&d, &bytes, 768);
        assert!(dirty >= 1, "at least one room must rot");
        assert!(
            dirty * 2 < d.regions.len(),
            "{dirty} of {} rooms infested — a subset must stay clean",
            d.regions.len()
        );
    }

    /// An infested room is *heavily* patched, not lightly speckled — the other half of the contrast.
    #[test]
    fn an_infested_room_is_heavily_patched() {
        let d = fixture();
        let bytes = build(&d, &cfg(768, 0.25, 0.0)).expect("builds");
        let worst = d
            .regions
            .iter()
            .map(|r| {
                (r.rect.min[1]..r.rect.max[1])
                    .flat_map(|y| (r.rect.min[0]..r.rect.max[0]).map(move |x| (x, y)))
                    .filter(|&(x, y)| covered(&bytes, 768, x, y))
                    .count()
            })
            .max()
            .unwrap_or(0);
        let area = (ROOM * ROOM) as usize;
        assert!(
            worst * 100 / area >= 40,
            "the most-infested room covers only {worst}/{area} of its floor; patches are too thin"
        );
    }

    /// Rock is never habitat — the rasterizer clips to owned floor, and nothing bleeds outside a room.
    #[test]
    fn rock_is_never_habitat() {
        let d = fixture();
        let c = cfg(768, 0.60, 0.0);
        let bytes = build(&d, &c).expect("builds");
        let n = CONTROL_SIZE as usize;
        for i in 0..n * n {
            let (x, y) = ((i % n) as i32, (i / n) as i32);
            if !d.is_floor(IVec2::new(x, y)) && at_cell(&bytes, 768, x, y) != 0 {
                panic!("rock cell ({x},{y}) has habitat {}", at_cell(&bytes, 768, x, y));
            }
        }
    }

    /// A corridor run is infested end to end, or not at all. Never half.
    #[test]
    fn corridor_runs_are_all_or_nothing() {
        let d = fixture();
        // Chance 1.0 → the single run must be solid across its whole length.
        let bytes = build(&d, &cfg(768, 0.9, 1.0)).expect("builds");
        for x in RUN {
            assert!(covered(&bytes, 768, x as i32, 75), "corridor cell ({x},75) must be infested");
        }
        // Chance 0.0 → not one cell of it.
        let bytes = build(&d, &cfg(768, 0.9, 0.0)).expect("builds");
        for x in RUN {
            assert_eq!(at_cell(&bytes, 768, x as i32, 75), 0, "corridor cell ({x},75) must be bare");
        }
    }

    /// Same seed, same mask — the colony layout is a pure function of the dungeon.
    #[test]
    fn build_is_deterministic() {
        let d = fixture();
        let c = cfg(384, 0.25, 0.5);
        assert_eq!(build(&d, &c).expect("a"), build(&d, &c).expect("b"));
    }

    /// The damp table must actually order rooms: a bathroom outscores an office at equal geometry.
    #[test]
    fn damp_rooms_outscore_dry_ones() {
        let c = cfg(384, 0.25, 0.0);
        let bath = hash01(HABITAT_SEED ^ splitmix64(0) ^ SCORE_SALT)
            * c.damp_weight(&["room".into(), "bathroom".into()]).expect("listed");
        let office = hash01(HABITAT_SEED ^ splitmix64(0) ^ SCORE_SALT)
            * c.damp_weight(&["room".into(), "office".into()]).expect("listed");
        assert!(bath > office, "bathroom ({bath}) must outrank office ({office}) at equal hash");
    }

    /// An unlisted room type is a loud error, never a silent middling weight.
    #[test]
    fn unlisted_room_type_fails_loudly() {
        let c = cfg(384, 0.25, 0.0);
        let err = c.damp_weight(&["room".into(), "dungeon_of_doom".into()]).expect_err("unlisted");
        assert!(err.contains("damp_weights"), "error should name the table: {err}");
    }

    /// Overshooting the target is a usable colony, so it returns `Ok` and reports the truth.
    #[test]
    fn corridor_overshoot_is_ok_not_err() {
        let d = fixture();
        // Tiny target, every run infested: the corridors alone blow the budget.
        let bytes = build(&d, &cfg(384, 0.01, 1.0)).expect("overshoot is still a usable mask");
        for x in RUN {
            assert!(covered(&bytes, 384, x as i32, 75), "the run is still fully infested");
        }
        assert!(coverage(&d, &bytes, 384) > 0.01, "the report must admit the overshoot");
    }

    /// A dungeon with no floor is a generation bug, not something to paper over.
    #[test]
    fn no_floor_fails_loudly() {
        let n = CONTROL_SIZE as usize;
        let d = Dungeon::from_parts(n, n, vec![false; n * n], Vec::new(), vec![u32::MAX; n * n]);
        let err = build(&d, &cfg(384, 0.25, 0.0)).expect_err("no floor must error");
        assert!(err.contains("no walkable floor"), "got: {err}");
    }

    /// A patch must be SOLID across its core, not a cone that fades from the centre.
    ///
    /// The regression this pins: with a linear `1 - d/r` falloff the value crosses 0.5 at `d = r/2`, so every
    /// patch covered a quarter of its nominal area, every room needed infesting to meet the budget, and no
    /// room was ever clean. Here the value must still be ~1 at 60% of the radius and dead by the radius.
    #[test]
    fn a_patch_has_a_solid_core_and_a_soft_rim() {
        let mut c = crate::mycelia::tests::valid();
        c.edge_noise_amp = 0.0; // isolate the radial profile from the border noise
        let n = Nucleus { x: 0.0, y: 0.0, radius: 10.0 };

        assert!(nucleus_value(&n, 0.0, 0.0, &c, 1) >= 0.999, "the centre must be solid");
        assert!(nucleus_value(&n, 6.0, 0.0, &c, 1) >= 0.999, "60% of the radius must still be solid");
        let rim = nucleus_value(&n, 8.5, 0.0, &c, 1);
        assert!((0.05..0.95).contains(&rim), "the rim must be a ramp, got {rim}");
        assert_eq!(nucleus_value(&n, 10.0, 0.0, &c, 1), 0.0, "nothing at the nominal radius");

        // ...and the covered radius (value >= COVERED) must be most of `r`, not half of it.
        assert!(nucleus_value(&n, 8.0, 0.0, &c, 1) >= COVERED, "80% of the radius must count as covered");
    }

    /// The rasterizer walks `nucleus_reach`; if that bound understates the contour, patches get square edges.
    #[test]
    fn nucleus_reach_bounds_the_noisy_contour() {
        let c = crate::mycelia::tests::valid();
        let n = Nucleus { x: 0.0, y: 0.0, radius: 6.0 };
        let reach = nucleus_reach(&n, &c);
        // Sample a ring just outside the claimed reach; nothing may be alive there, whatever the noise does.
        for i in 0..720 {
            let a = i as f32 * std::f32::consts::TAU / 720.0;
            let (x, y) = ((reach + 0.01) * a.cos(), (reach + 0.01) * a.sin());
            assert_eq!(nucleus_value(&n, x, y, &c, 7), 0.0, "value beyond reach at angle {a}");
        }
    }

    /// The three design rules, asserted against the **shipped** config and the **shipped** dungeon seed.
    ///
    /// The other tests here prove the algorithm behaves on a synthetic fixture. This one proves the numbers
    /// actually in `assets/config/config.ron` produce the level the design asked for — a different claim, and
    /// the one that broke first.
    ///
    /// The three rules pull against each other, because this dungeon is only about a THIRD room floor by area.
    /// Fund the coverage quota out of rooms alone and nearly all of them must rot (measured: 17 of 24 at
    /// `coverage 0.25 / corridor 0.12`). Fund it out of corridors instead and the mold stops being a room
    /// phenomenon (measured: 57% of it in halls at `0.25 / 0.30`). Only a lower `habitat_coverage` satisfies
    /// all three at once. So assert all three together — tuning one dial otherwise breaks another in silence.
    #[test]
    fn the_shipped_config_delivers_the_intended_level() {
        let game = crate::config::load_game_config().expect("the shipped config must load and validate");
        let d = Dungeon::generate(&game.dungeon).expect("the shipped seed must generate");
        let bytes = build(&d, &game.mycelia).expect("the shipped dungeon must be habitable");
        let field = game.mycelia.field_size as usize;

        // 1. It covers about what it was asked to cover.
        let achieved = coverage(&d, &bytes, field);
        assert!(
            (achieved - game.mycelia.habitat_coverage).abs() <= 0.03,
            "shipped coverage {achieved} misses the configured {}",
            game.mycelia.habitat_coverage
        );

        // 2. Most rooms stay clean, so walking into a moldy one is an event.
        let dirty = infested_rooms(&d, &bytes, field);
        let total = d.regions.len();
        assert!(
            dirty * 2 <= total,
            "{dirty} of {total} rooms infested — most rooms must stay clean. Lower \
             mycelia.habitat_coverage, or raise corridor_infest_chance and accept mold in the halls."
        );
        assert!(dirty >= 2, "only {dirty} rooms rot; the mold should still be a presence");

        // 3. The mold is a ROOM phenomenon. This is the rule the corridor dial quietly destroys: raise
        //    `corridor_infest_chance` to fund a high coverage target and the colony moves into the halls,
        //    while rules 1 and 2 both still pass.
        let n = CONTROL_SIZE as usize;
        let cell = |i: usize| IVec2::new((i % n) as i32, (i / n) as i32);
        let (mut in_rooms, mut in_halls) = (0usize, 0usize);
        for i in 0..n * n {
            let c = cell(i);
            if d.is_floor(c) && covered(&bytes, field, c.x, c.y) {
                if d.is_corridor(c) {
                    in_halls += 1;
                } else {
                    in_rooms += 1;
                }
            }
        }
        let mold = in_rooms + in_halls;
        assert!(
            in_rooms * 100 / mold.max(1) >= 60,
            "only {in_rooms} of {mold} mold cells are in rooms; the mold must live in rooms, not halls"
        );
    }

    /// The noise is deterministic and stays in range — everything downstream assumes both.
    #[test]
    fn value_noise_is_deterministic_and_bounded() {
        for i in 0..500 {
            let (x, y) = (i as f32 * 0.37, i as f32 * -0.11);
            let a = fbm(x, y, 0xABCD);
            let b = fbm(x, y, 0xABCD);
            assert_eq!(a, b, "fbm must be a pure function");
            assert!((0.0..1.0).contains(&a), "fbm out of range at ({x},{y}): {a}");
        }
        // Two different seeds must not agree everywhere (a constant would silently disable the raggedness).
        let differs = (0..64).any(|i| {
            let (x, y) = (i as f32 * 0.7, 3.0);
            (fbm(x, y, 1) - fbm(x, y, 2)).abs() > 1e-6
        });
        assert!(differs, "fbm must depend on its seed");
    }
}
