//! Almond Water — a finite, self-regenerating substance that seeps up from the concrete, pools on the
//! floor, and heals biological creatures that stand in it. Backrooms Object 1, the post-drift "healing"
//! reading (lore: `docs/lore/2026-07-13-backrooms-almond-water.md`; idea capture:
//! `slop/ideas/2026-07-13-almond-water-resource.md`).
//!
//! **Architecture — one clean path.** It borrows [`crate::light::LightField`]'s *query interface*
//! (`sample`/`gradient`, a per-cell scalar over the shared 192² dungeon grid, its own config slice + loud
//! validator, and a plugin registered in BOTH the game and the headless harness) but, unlike light, it is
//! *dynamic and consumable*, so its per-tick update copies [`crate::ai::field::Stig`]'s
//! accumulate→evaporate→diffuse kernel (a reaction-diffusion process on the floor surface — Painter &
//! Maini, *J. Chem. Soc. Faraday Trans.* 1997, doi:10.1039/a702602a). The emergent payoff is stigmergic
//! foraging over a regenerating resource: wounded creatures descend toward richer water, drain it, and a
//! depletion front moves with no coordination code (Heylighen, *Cognitive Systems Research* 2015,
//! doi:10.1016/j.cogsys.2015.12.002; Parunak 2005).
//!
//! **What is pinned.** The field grid is CPU gameplay state that crab locomotion reads for foraging taxis,
//! and the heal writes `Health`, so both fold into the deterministic replay gate (this module is registered
//! in `sim_harness`, exactly like [`crate::light::LightFieldPlugin`]). The *visual* — the iridescent puddle
//! and the mold moisture-feed — is cosmetic, windowed-only, and lives in [`visual`] behind the `RenderApp`
//! firewall; it never reaches the harness and never perturbs a golden.

use bevy::prelude::*;
use serde::Deserialize;

use crate::config::GameConfig;
use crate::dungeon::Dungeon;
use crate::health::{Biological, Health};
use crate::util::{in_grid, row_major};

pub mod visual;

/// The `almond_water:` slice of `assets/config/config.ron` — every knob, one source of truth (see
/// [`GameConfig`]). Gameplay knobs drive the field + heal; the trailing visual knobs are read only by the
/// windowed [`visual::AlmondWaterVisualPlugin`]. No fallback: a missing/invalid slice is a loud startup
/// panic via [`validate_config`].
#[derive(Deserialize, Clone, Debug)]
pub struct AlmondWaterConfig {
    /// Seep rate (volume/sec) baked into a **spring** — a sparse, spaced-out floor cell that borders a wall
    /// (or is mold-colonised), where the water "bubbles up from the concrete." Only springs seep; every
    /// other cell stays dry, so the field reads as discrete pools rather than one continuous sheet.
    pub strong_seep: f32,
    /// Minimum spacing (in tiles) between springs — the greedy scatter in [`bake_almond_sources`] rejects a
    /// candidate spring within this Chebyshev distance of one already placed. Larger ⇒ fewer, more isolated
    /// pools; this is what caps a pool's footprint (a spring's puddle spreads only a tile or two before the
    /// next spring is too far to merge with it), keeping each pool to ~1–10 tiles.
    pub pool_spacing: f32,
    /// Per-cell volume ceiling. Accumulation clamps here so a rich seep pools but never runs away.
    pub capacity: f32,
    /// Fraction of a cell's volume lost per second (drying) — the same evaporation term `Stig` uses.
    pub evaporate: f32,
    /// Blend weight [0,1] toward the 4-neighbour average each tick — spreads a pool outward so the field
    /// pools and patterns rather than sitting in point sources.
    pub diffuse: f32,
    /// HP restored per second to a biological creature standing in water (rate-limited, like `medic_heal`).
    pub heal_rate: f32,
    /// Water volume consumed per HP healed — the consumable coupling. Higher ⇒ each heal drains more water.
    pub heal_per_unit_water: f32,
    /// Multiplier applied to a cell's seep where the mold habitat lives. Default > 1: hyphae crack/porous
    /// the concrete so more water bubbles up (mutual reinforcement — water also feeds the mold visually).
    /// Set < 1 to flip the reading to "biofilm seals the concrete" — a config-only change, no code path.
    pub mold_seep_mult: f32,
    /// Steering gain a *wounded* creature feels climbing the water gradient (foraging taxis). Scales the
    /// world-space push added to locomotion; tune against creature speed (cf. `lighting.photophilic_gain`).
    pub forage_gain: f32,
    /// Health fraction at or below which a creature forages toward water. Above it, a healthy creature
    /// ignores the field (no cost when full).
    pub forage_wounded_frac: f32,

    // --- Visual (windowed-only; read by `visual`, never by the gameplay field) ---
    /// Almond base tint of the puddle (linear-RGB).
    pub almond_tint: [f32; 3],
    /// Level below which no puddle is drawn (a dry cell reads as bare concrete).
    pub min_visible_level: f32,
    /// Thin-film thickness in nanometres — sets the iridescence hue via optical path difference.
    pub film_thickness_nm: f32,
    /// Index of refraction of the water film (~1.33). Must be ≥ 1.
    pub film_ior: f32,
    /// Overall strength of the oil-slick iridescent sheen, [0,1]-ish. Higher = more colour.
    pub iridescence_strength: f32,
    /// How strongly water level boosts the mold's Gray-Scott feed where the two overlap (cosmetic).
    pub moisture_feed_gain: f32,
}

/// Loud, one-path validation (mirrors [`crate::light::validate_config`]). Every knob finite and in range;
/// one `Err` per violation, no fallback.
pub fn validate_config(c: &AlmondWaterConfig) -> Result<(), String> {
    for (name, v) in [
        ("strong_seep", c.strong_seep),
        ("evaporate", c.evaporate),
        ("heal_rate", c.heal_rate),
        ("mold_seep_mult", c.mold_seep_mult),
        ("forage_gain", c.forage_gain),
        ("min_visible_level", c.min_visible_level),
        ("film_thickness_nm", c.film_thickness_nm),
        ("iridescence_strength", c.iridescence_strength),
        ("moisture_feed_gain", c.moisture_feed_gain),
    ] {
        if !(v.is_finite() && v >= 0.0) {
            return Err(format!("almond_water.{name} must be finite and >= 0 (got {v})"));
        }
    }
    if !(c.capacity.is_finite() && c.capacity > 0.0) {
        return Err(format!("almond_water.capacity must be finite and > 0 (got {})", c.capacity));
    }
    if !(c.pool_spacing.is_finite() && c.pool_spacing >= 1.0) {
        return Err(format!(
            "almond_water.pool_spacing must be finite and >= 1 tile (got {})",
            c.pool_spacing
        ));
    }
    if !(c.heal_per_unit_water.is_finite() && c.heal_per_unit_water > 0.0) {
        return Err(format!(
            "almond_water.heal_per_unit_water must be finite and > 0 (got {})",
            c.heal_per_unit_water
        ));
    }
    if !(c.film_ior.is_finite() && c.film_ior >= 1.0) {
        return Err(format!("almond_water.film_ior must be finite and >= 1.0 (got {})", c.film_ior));
    }
    // A blend weight and a health fraction are both cosines-of-nothing: they must sit in [0, 1] or the
    // field either never diffuses / always saturates, or every creature forages regardless of health.
    for (name, v) in [("diffuse", c.diffuse), ("forage_wounded_frac", c.forage_wounded_frac)] {
        if !(v.is_finite() && (0.0..=1.0).contains(&v)) {
            return Err(format!("almond_water.{name} must be in [0, 1] (got {v})"));
        }
    }
    if c.almond_tint.iter().any(|ch| !ch.is_finite() || *ch < 0.0) {
        return Err(format!(
            "almond_water.almond_tint channels must be finite and >= 0 (got {:?})",
            c.almond_tint
        ));
    }
    Ok(())
}

/// System set for the field writers ([`accumulate_evaporate_diffuse`] then [`almond_water_heal`], chained).
/// Foraging readers (crab locomotion) order `.after(AlmondWaterWritten)` so they read the current tick's
/// water level — mirroring [`crate::light::LightFieldWritten`].
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AlmondWaterWritten;

/// A CPU-side scalar **water grid over dungeon cells** — the gameplay Almond Water field. Row-major
/// `y*width + x` (the project-wide indexing), 0 on dry / rock cells. `sample`/`gradient` copy the shape of
/// [`crate::light::LightField`] so creature steering reuses that idiom; the per-tick
/// [`AlmondWater::tick`] copies [`crate::ai::field::Stig`]'s evaporate/diffuse discipline.
///
/// Its own resource, not a `Stig` channel: like light it is environmental (sourced from geometry, not
/// creature deposits) and named for future extractors — but unlike light it is dynamic and consumable, so
/// it borrows `Stig`'s update kernel. One path: one `AlmondWater`.
#[derive(Resource)]
pub struct AlmondWater {
    width: usize,
    height: usize,
    /// Per-cell seep rate, baked once from geometry (+ the static mold habitat). Static after the bake, so
    /// it stays out of `snapshot_hash` (only `level` evolves).
    sources: Vec<f32>,
    /// Current water volume per cell — the depletable quantity the whole game reads (`sample`/`gradient`).
    level: Vec<f32>,
    /// Reused double-buffer for the diffusion pass (avoids per-tick allocation) — copies `Stig::scratch`.
    scratch: Vec<f32>,
    /// The floor cells (the only cells that ever carry value), precomputed so the per-tick passes skip
    /// rock. `(row_major index, cell)`. Filled by [`bake_almond_sources`].
    floor_cells: Vec<(usize, IVec2)>,
    /// Row-major floor membership, so the diffusion neighbour check is a local array read instead of a
    /// `Dungeon` borrow — the mask is exactly the floor set, so it is bit-identical to `dungeon.is_floor`,
    /// and it keeps [`AlmondWater::tick`] pure (unit-testable without building a `Dungeon`).
    floor_mask: Vec<bool>,
    /// Peak `level` after the last tick — lets the visual normalise to 0..1.
    peak: f32,
}

impl AlmondWater {
    /// Empty field sized to the dungeon. `sources`/`floor_cells` stay empty until [`bake_almond_sources`]
    /// runs at `Startup` (the bake is fallible — it reads the mold habitat — so it cannot happen at
    /// plugin-build time; an unbaked field simply seeps nothing until then).
    fn new(width: usize, height: usize) -> Self {
        let n = width * height;
        Self {
            width,
            height,
            sources: vec![0.0; n],
            level: vec![0.0; n],
            scratch: vec![0.0; n],
            floor_cells: Vec::new(),
            floor_mask: vec![false; n],
            peak: 0.0,
        }
    }

    /// Point read at a world position (query). Off-grid reads as 0 — the same contract as
    /// `LightField::sample` / `Stig::sample`.
    pub fn sample(&self, dungeon: &Dungeon, pos: Vec3) -> f32 {
        let c = dungeon.world_to_cell(pos);
        if in_grid(c, self.width, self.height) {
            self.level[row_major(c, self.width)]
        } else {
            0.0
        }
    }

    /// World-XZ direction of *increasing* water (central differences), magnitude ≈ the local slope — copied
    /// from `LightField::gradient`. A wounded forager steers along `+gradient` (toward richer water).
    pub fn gradient(&self, dungeon: &Dungeon, pos: Vec3) -> Vec2 {
        let c = dungeon.world_to_cell(pos);
        let at = |dx: i32, dy: i32| -> f32 {
            let n = c + IVec2::new(dx, dy);
            if in_grid(n, self.width, self.height) {
                self.level[row_major(n, self.width)]
            } else {
                0.0
            }
        };
        Vec2::new(at(1, 0) - at(-1, 0), at(0, 1) - at(0, -1))
    }

    /// Peak water level from the last tick (0 before the first tick).
    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// Remove up to `amount` water from `cell`, clamped ≥ 0, returning the volume actually removed. The
    /// consumable coupling: the heal drinks the local cell down as it restores HP.
    fn drink(&mut self, cell: IVec2, amount: f32) -> f32 {
        if amount <= 0.0 || !in_grid(cell, self.width, self.height) {
            return 0.0;
        }
        let idx = row_major(cell, self.width);
        let taken = amount.min(self.level[idx]).max(0.0);
        self.level[idx] -= taken;
        taken
    }

    /// One accumulate → evaporate → diffuse step, floor cells only, in `Stig::evaporate_diffuse`'s order and
    /// with its determinism discipline (fixed E/W/S/N neighbour order — float add is non-associative — and a
    /// double-buffered diffuse so rock cells stay invariantly 0). `dt` in seconds.
    fn tick(&mut self, dt: f32, evaporate: f32, diffuse: f32, capacity: f32) {
        // 1+2. Accumulate the seep, then evaporate, in one in-place pass (an evaporation of the just-added
        //      seep is exactly a steady-state cap; the two commute up to O(dt²), same as `Stig`).
        let retain = (1.0 - evaporate * dt).clamp(0.0, 1.0);
        for &(idx, _) in &self.floor_cells {
            let filled = (self.level[idx] + self.sources[idx] * dt).min(capacity);
            self.level[idx] = filled * retain;
        }

        // 3. Diffuse: blend each floor cell toward the average of its floor neighbours (double-buffered).
        if diffuse > 0.0 {
            let (w, h) = (self.width, self.height);
            for &(idx, pos) in &self.floor_cells {
                let (x, y) = (pos.x, pos.y);
                let mut sum = 0.0;
                let mut n = 0.0;
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let nb = IVec2::new(x + dx, y + dy);
                    if nb.x >= 0 && nb.y >= 0 && (nb.x as usize) < w && (nb.y as usize) < h {
                        let nidx = (nb.y as usize) * w + nb.x as usize;
                        if self.floor_mask[nidx] {
                            sum += self.level[nidx];
                            n += 1.0;
                        }
                    }
                }
                let avg = if n > 0.0 { sum / n } else { self.level[idx] };
                self.scratch[idx] = self.level[idx] * (1.0 - diffuse) + avg * diffuse;
            }
            std::mem::swap(&mut self.level, &mut self.scratch);
        }

        // Peak for the visual's 0..1 normalisation. Order-independent (`max`), so it never perturbs the sim.
        self.peak = self.floor_cells.iter().map(|&(idx, _)| self.level[idx]).fold(0.0f32, f32::max);
    }

    /// FNV-1a-fold the exact bit pattern of every `level` and `sources` cell (the **full** grid, so the
    /// rock-cells-stay-0 invariant is pinned too) plus `peak`, into `hash`. The determinism oracle for the
    /// field: `snapshot_hash` folds only actor Transform+Health, so a reordered diffusion neighbour sum or a
    /// broken floor mask that doesn't happen to relocate an agent would ship silently without this. Copies
    /// `Stig::fold_fingerprint`. Test-only.
    #[cfg(feature = "test-harness")]
    pub fn fold_fingerprint(&self, hash: &mut u64) {
        let fold = |v: f32, h: &mut u64| {
            for b in v.to_bits().to_le_bytes() {
                *h ^= u64::from(b);
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for &v in &self.level {
            fold(v, hash);
        }
        for &v in &self.sources {
            fold(v, hash);
        }
        fold(self.peak, hash);
    }

    /// Connected-component (4-neighbour) tile counts of every **pool** — a maximal run of floor cells whose
    /// `level` exceeds `threshold`. Sorted descending. Lets a harness test assert the sparse springs stay
    /// small, isolated puddles (the "1–10 tiles per pool" contract) rather than merging into a sheet.
    /// Test-only.
    #[cfg(feature = "test-harness")]
    pub fn pool_sizes(&self, threshold: f32) -> Vec<usize> {
        let (w, h) = (self.width, self.height);
        let wet = |c: IVec2| -> bool {
            in_grid(c, w, h) && self.level[row_major(c, w)] > threshold
        };
        let mut seen = vec![false; w * h];
        let mut sizes: Vec<usize> = Vec::new();
        for &(idx, c) in &self.floor_cells {
            if seen[idx] || !wet(c) {
                continue;
            }
            let mut stack = vec![c];
            seen[idx] = true;
            let mut size = 0usize;
            while let Some(p) = stack.pop() {
                size += 1;
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let n = p + IVec2::new(dx, dy);
                    if wet(n) {
                        let ni = row_major(n, w);
                        if !seen[ni] {
                            seen[ni] = true;
                            stack.push(n);
                        }
                    }
                }
            }
            sizes.push(size);
        }
        sizes.sort_unstable_by(|a, b| b.cmp(a));
        sizes
    }

    /// Flood every floor cell to `level` — a harness helper so a heal test can put water under every
    /// biological without waiting for the seeps to pool. Test-only.
    #[cfg(feature = "test-harness")]
    pub fn test_flood(&mut self, level: f32) {
        for &(idx, _) in &self.floor_cells {
            self.level[idx] = level;
        }
        self.peak = level;
    }
}

/// World-XZ steering push a wounded forager feels at `pos`: `gain · ∇level`. Zero where the field is flat
/// (deep dry or the middle of a uniform pool), so a creature far from any water gradient is unbiased — the
/// graceful "no cost off the water" property. Pure: the caller projects the result onto the locomotion
/// surface and scales by `dt` (see `crab::crab_locomotion`). Mirrors [`crate::light::light_push`].
pub fn almond_push(field: &AlmondWater, dungeon: &Dungeon, pos: Vec3, gain: f32) -> Vec3 {
    if gain == 0.0 {
        return Vec3::ZERO;
    }
    let g = field.gradient(dungeon, pos);
    Vec3::new(g.x, 0.0, g.y) * gain
}

/// Bake the per-cell seep `sources` once, at `Startup`, from pure geometry plus the static mold habitat.
/// Water bubbles up from **sparse springs**, not every wall: a floor cell is a spring only if it is eligible
/// (borders a wall — "bubbles up from the walls" — or is mold-colonised) AND no spring is already placed
/// within `pool_spacing` tiles. A greedy min-spacing scatter (deterministic row-major order) keeps springs
/// far enough apart that their puddles never merge, so the field reads as discrete 1–10 tile pools instead
/// of one continuous sheet. A mold spring seeps `mold_seep_mult`× more. Every non-spring cell stays dry.
///
/// Fallible because it reads the mold habitat (`mycelia::habitat::infested_cells`) — a dungeon/damp-table
/// contract violation is a loud startup `Err`, never a degraded default. Deterministic (pure geometry + the
/// seeded habitat mask + a fixed iteration order), so it stays out of `snapshot_hash`.
fn bake_almond_sources(
    mut field: ResMut<AlmondWater>,
    dungeon: Res<Dungeon>,
    config: Res<GameConfig>,
) -> Result<(), BevyError> {
    let cfg = &config.almond_water;
    let (w, h) = (field.width, field.height);

    // Where the mold lives, per dungeon cell — the single source of truth for "mold-colonised concrete",
    // sampled from the same field-resolution mask + `COVERED` threshold the mold itself uses.
    let infested = crate::mycelia::habitat::infested_cells(&dungeon, &config.mycelia)?;

    let floor_cells: Vec<(usize, IVec2)> =
        dungeon.floor_cells().map(|c| (row_major(c, w), c)).collect();

    // Chebyshev min-spacing between springs (≥ 1). Rounded from the config tile distance.
    let spacing = cfg.pool_spacing.max(1.0).round() as i32;

    let mut sources = vec![0.0f32; w * h];
    let mut floor_mask = vec![false; w * h];
    // Placed spring cells, for the greedy spacing rejection. Deterministic: `floor_cells` is row-major, so
    // the accepted set is a pure function of geometry + the seeded mold mask.
    let mut springs: Vec<IVec2> = Vec::new();
    for &(idx, c) in &floor_cells {
        floor_mask[idx] = true;

        // Eligible = bubbles up from a wall edge, or from mold-cracked concrete.
        let wall_adjacent = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| {
            let nb = c + IVec2::new(dx, dy);
            !dungeon.is_floor(nb) // off-grid or rock both count as "a wall borders that edge"
        });
        let mold = infested.get(idx).copied().unwrap_or(false);
        if !(wall_adjacent || mold) {
            continue;
        }

        // Greedy min-spacing: reject if any placed spring is within `spacing` tiles (Chebyshev), so pools
        // stay isolated and small.
        let too_close = springs
            .iter()
            .any(|s| (s.x - c.x).abs() <= spacing && (s.y - c.y).abs() <= spacing);
        if too_close {
            continue;
        }

        springs.push(c);
        let mut seep = cfg.strong_seep;
        if mold {
            seep *= cfg.mold_seep_mult;
        }
        sources[idx] = seep;
    }

    field.sources = sources;
    field.floor_cells = floor_cells;
    field.floor_mask = floor_mask;
    Ok(())
}

/// Field update: accumulate the seep, evaporate, diffuse. `FixedUpdate`, in [`AlmondWaterWritten`], before
/// the heal. Writes `level` → folds into the field replay hash.
fn accumulate_evaporate_diffuse(
    mut field: ResMut<AlmondWater>,
    config: Res<GameConfig>,
    time: Res<Time>,
) {
    let cfg = &config.almond_water;
    let (evaporate, diffuse, capacity) = (cfg.evaporate, cfg.diffuse, cfg.capacity);
    field.tick(time.delta_secs(), evaporate, diffuse, capacity);
}

/// Heal every biological creature standing in water, draining the cell as it heals. `FixedUpdate`, after the
/// field update and after `medic_heal` (both take `&mut Health`, so the order is pinned for deterministic
/// composition when two heal sources land on one unit in one tick). Mirrors `squad_ai::actions::medic_heal`'s
/// rate-based `(current + rate·dt).min(max)` idiom, minus the medic proximity gate — the water *is* the source.
///
/// **Drink contention determinism:** several drinkers can share a cell, and `drink` mutates the cell with a
/// non-associative `f32 -=`, so the candidates are sorted by `(cell, current, pos)` before any drink — the
/// same sorted-application discipline `snapshot_hash`'s rows and `Stig`'s deposits use.
fn almond_water_heal(
    mut field: ResMut<AlmondWater>,
    dungeon: Res<Dungeon>,
    config: Res<GameConfig>,
    time: Res<Time>,
    mut drinkers: Query<(Entity, &Transform, &mut Health), With<Biological>>,
) {
    let cfg = &config.almond_water;
    let dt = time.delta_secs();

    // Immutable pass: collect the wounded, each with its stable sort key. `to_bits` on a deterministic-core
    // float is a total, reproducible order; the position tie-break decorrelates two co-located drinkers.
    let mut cands: Vec<(u32, u32, u32, u32, u32, Entity, IVec2)> = drinkers
        .iter()
        .filter(|(_, _, h)| h.current < h.max && h.max > 0.0)
        .map(|(e, t, h)| {
            let p = t.translation;
            let cell = dungeon.world_to_cell(p);
            (
                cell.y as u32,
                cell.x as u32,
                h.current.to_bits(),
                p.x.to_bits(),
                p.z.to_bits(),
                e,
                cell,
            )
        })
        .collect();
    if cands.is_empty() {
        return;
    }
    cands.sort_unstable_by_key(|k| (k.0, k.1, k.2, k.3, k.4));

    // Mutable pass: drink + heal in the sorted order. `want` is recomputed from the fresh `Health` so this
    // stays correct if `medic_heal` topped the same unit up earlier in the schedule.
    for (.., e, cell) in cands {
        let Ok((_, _, mut health)) = drinkers.get_mut(e) else {
            continue;
        };
        let want = (cfg.heal_rate * dt).min(health.max - health.current);
        if want <= 0.0 {
            continue;
        }
        let got = field.drink(cell, want / cfg.heal_per_unit_water);
        health.current = (health.current + got * cfg.heal_per_unit_water).min(health.max);
    }
}

/// Owns the gameplay [`AlmondWater`] field + the consuming heal. Registered in BOTH the windowed game and
/// the headless harness (unlike the cosmetic [`visual::AlmondWaterVisualPlugin`]) because the field is CPU
/// gameplay state creature AI reads and the heal writes `Health` — so the deterministic replay gate must
/// cover both. Requires `Dungeon` at build (DungeonPlugin precedes it), like `LightFieldPlugin`.
pub struct AlmondWaterPlugin;

impl Plugin for AlmondWaterPlugin {
    fn build(&self, app: &mut App) {
        let dungeon = app
            .world()
            .get_resource::<Dungeon>()
            .expect("AlmondWaterPlugin requires DungeonPlugin to be registered first");
        let field = AlmondWater::new(dungeon.width, dungeon.height);
        app.insert_resource(field)
            .add_systems(Startup, bake_almond_sources)
            // The FIELD write is what crab foraging reads (`crab_locomotion.after(AlmondWaterWritten)`), so
            // ONLY the field update carries that marker — keeping the heal out of it avoids a cycle
            // (crab_locomotion → crab_jump → heal would otherwise loop back into a set crab_locomotion is
            // after). The heal is a separate, late sink.
            .add_systems(FixedUpdate, accumulate_evaporate_diffuse.in_set(AlmondWaterWritten))
            .add_systems(
                FixedUpdate,
                almond_water_heal
                    // Reads this tick's accumulated water to drink it down.
                    .after(accumulate_evaporate_diffuse)
                    // The heal writes `Health`, which every damage system also mutates. Those writers overlap
                    // in access but rarely touch the same entity the same tick, so their order was never
                    // pinned — until the wounded-crab forage clusters low-HP crabs onto water inside weapon
                    // range, so heal and damage hit the SAME crab the same tick and `min(max)`/`>= 0` clamping
                    // makes the net HP order-dependent → a per-process nondeterministic `snapshot_hash`. Pin
                    // the heal AFTER every `Health` writer (the damage set + the medic) so it composes
                    // deterministically — the water gets the last word (a killing blow can be out-healed while
                    // you stand in it). See [`crate::health::HealthDamage`].
                    .after(crate::health::HealthDamage)
                    .after(crate::squad_ai::actions::medic_heal),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built field over a `w×h` grid whose listed `(x, y)` cells are floor. Bypasses the `Startup`
    /// bake (which needs a real `Dungeon`) so the field math is unit-testable GPU-free — the tests are in
    /// this module, so they see the private grid state.
    fn grid(w: usize, h: usize, floor: &[(usize, usize)]) -> AlmondWater {
        let mut f = AlmondWater::new(w, h);
        for &(x, y) in floor {
            let idx = y * w + x;
            f.floor_mask[idx] = true;
            f.floor_cells.push((idx, IVec2::new(x as i32, y as i32)));
        }
        f
    }

    /// A fully-valid config to mutate one knob at a time in the validator tests.
    fn valid_cfg() -> AlmondWaterConfig {
        AlmondWaterConfig {
            strong_seep: 8.0,
            pool_spacing: 6.0,
            capacity: 100.0,
            evaporate: 0.05,
            diffuse: 0.1,
            heal_rate: 5.0,
            heal_per_unit_water: 1.5,
            mold_seep_mult: 1.5,
            forage_gain: 10.0,
            forage_wounded_frac: 0.6,
            almond_tint: [0.92, 0.85, 0.70],
            min_visible_level: 1.0,
            film_thickness_nm: 320.0,
            film_ior: 1.33,
            iridescence_strength: 0.25,
            moisture_feed_gain: 0.3,
        }
    }

    #[test]
    fn drink_drains_exactly_and_clamps_at_zero() {
        let mut f = grid(3, 1, &[(0, 0), (1, 0), (2, 0)]);
        f.level[1] = 50.0;
        let cell = IVec2::new(1, 0);
        // Partial drink removes exactly `amount`.
        assert_eq!(f.drink(cell, 20.0), 20.0);
        assert_eq!(f.level[1], 30.0);
        // Over-drink removes only what's there and clamps at 0 (never negative).
        assert_eq!(f.drink(cell, 999.0), 30.0);
        assert_eq!(f.level[1], 0.0);
        // A dry / non-positive / off-grid drink is a no-op returning 0.
        assert_eq!(f.drink(cell, 5.0), 0.0);
        assert_eq!(f.drink(cell, -5.0), 0.0);
        assert_eq!(f.drink(IVec2::new(99, 99), 5.0), 0.0);
    }

    #[test]
    fn tick_accumulates_toward_steady_state_and_respects_capacity() {
        // One floor cell, weak source, slow drying, generous cap.
        let mut f = grid(1, 1, &[(0, 0)]);
        let (dt, s, e, cap) = (1.0 / 60.0, 2.0, 0.05, 100.0);
        f.sources[0] = s;
        let mut prev = 0.0;
        // ~20k ticks: the per-tick convergence factor is (1 − e·dt) ≈ 0.99917, so it takes several
        // thousand ticks to settle to the fixed point within tolerance.
        for _ in 0..20_000 {
            f.tick(dt, e, 0.0, cap); // diffuse 0: isolated cell
            // Monotone non-decreasing from empty toward the fixed point; never over capacity.
            assert!(f.level[0] >= prev - 1.0e-6);
            assert!(f.level[0] <= cap + 1.0e-4);
            prev = f.level[0];
        }
        // Discrete fixed point L = (L + s·dt)(1 − e·dt) ⇒ L = s·(1 − e·dt)/e ≈ s/e for small dt.
        let expected = s * (1.0 - e * dt) / e;
        assert!((f.level[0] - expected).abs() < 0.05, "level {} vs {}", f.level[0], expected);
    }

    #[test]
    fn tick_clamps_a_huge_seep_to_capacity() {
        let mut f = grid(1, 1, &[(0, 0)]);
        f.sources[0] = 1.0e9;
        f.tick(1.0 / 60.0, 0.0, 0.0, 42.0);
        assert_eq!(f.level[0], 42.0);
        assert_eq!(f.peak(), 42.0);
    }

    #[test]
    fn diffuse_spreads_to_a_neighbour_and_conserves_between_two_cells() {
        // Two adjacent floor cells, one full, one dry; no seep, no drying → diffusion only.
        let mut f = grid(2, 1, &[(0, 0), (1, 0)]);
        f.level[0] = 100.0;
        f.tick(1.0 / 60.0, 0.0, 0.5, 1000.0);
        // Each blends halfway toward the other; the pair's total is conserved.
        assert!((f.level[0] - 50.0).abs() < 1.0e-4);
        assert!((f.level[1] - 50.0).abs() < 1.0e-4);
    }

    #[test]
    fn validate_config_accepts_valid_and_rejects_out_of_range() {
        assert!(validate_config(&valid_cfg()).is_ok());

        let mut c = valid_cfg();
        c.diffuse = 1.5; // a blend weight must be in [0, 1]
        assert!(validate_config(&c).is_err());

        let mut c = valid_cfg();
        c.film_ior = 0.5; // an index of refraction below air is unphysical
        assert!(validate_config(&c).is_err());

        let mut c = valid_cfg();
        c.capacity = 0.0; // a zero cap would divide-by-zero the visual normalisation
        assert!(validate_config(&c).is_err());

        let mut c = valid_cfg();
        c.forage_wounded_frac = 2.0; // a health fraction must be in [0, 1]
        assert!(validate_config(&c).is_err());

        let mut c = valid_cfg();
        c.strong_seep = -1.0; // seep can't be negative
        assert!(validate_config(&c).is_err());

        let mut c = valid_cfg();
        c.pool_spacing = 0.0; // springs must be at least 1 tile apart
        assert!(validate_config(&c).is_err());
    }
}
