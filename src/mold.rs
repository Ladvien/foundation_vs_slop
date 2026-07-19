//! **Deterministic CPU mold field** — a reaction-diffusion biomass that grows in damp habitat, spreads to
//! neighbours, and recedes from the squad's light. This is the *gameplay* mold: unlike [`crate::mycelia`]
//! (GPU compute, cosmetic, windowed-only, non-bit-reproducible), it runs on `FixedUpdate` in **both** the
//! game and the headless harness, so it is bit-reproducible and the offline QD search can tune it. The GPU
//! mycelia becomes the hi-res cosmetic mirror of this field.
//!
//! # Model
//!
//! A logistic-growth reaction-diffusion (Fisher, "The wave of advance of advantageous genes", 1937; the
//! Fisher–KPP equation) with a light-recoil sink, over the 192² dungeon-cell grid `V ∈ [0,1]`:
//!
//! ```text
//!   V ← V  +  growth·V·(K − V)  +  diffuse·∇²V  −  light_recoil·light01·V     (clamped to [0,1])
//! ```
//!
//! - `K(cell) ∈ {0,1}` is the geometry-derived **habitat capacity** — the same deterministic, GPU-free
//!   `mycelia::habitat::infested_cells` mask almond-water already reads (Tero et al. 2010 give the
//!   biological grounding for habitat-directed spread; Turing 1952 for the reaction-diffusion form).
//! - **Logistic** growth (bounded toward `K`) is chosen over Gray–Scott so an *evolved* parameter set can
//!   never blow the field up or extinguish it — the optimizer explores a robust, always-stable regime.
//! - Diffusion is a 4-neighbour Laplacian in **fixed N,E,S,W order** with a no-flux (reflective) wall
//!   boundary; `diffuse < 0.25` keeps the explicit Euler step stable.
//! - `light01` is the normalised [`crate::light::LightField`] illuminance the squad's flashlights raise —
//!   the **photophobia** the GPU mold already shows, now a gameplay force: shine light, push the mold back.
//!
//! # Determinism
//!
//! Row-major `f32` grids, fixed cell iteration and fixed neighbour order, seeded only by geometry, no
//! `HashMap` — the same discipline the stigmergy / almond-water fields keep. Folded into
//! `sim_harness::field_hash` so the replay gate covers it. Registered in BOTH `lib::run` and the harness
//! (like `LightFieldPlugin`), on `FixedUpdate`, ordered `.after(LightFieldWritten)` so it recoils from the
//! *current* tick's light; its own gameplay couplings (dim light / occlude LOS / boost seep) feed the next
//! tick, the standard one-tick-lag the codebase uses to break the mold↔light feedback cycle.

use bevy::prelude::*;

use crate::config::GameConfig;
use crate::dungeon::Dungeon;
use crate::light::{LightField, LightFieldWritten};
use crate::util::row_major;

/// The evolvable mold parameters (the `mold:` config slice). Per-substep coefficients (the fixed 60 Hz
/// `FixedUpdate` × `substeps` sets the wall-clock rate), so nothing here depends on `dt`.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MoldConfig {
    /// Logistic growth coefficient per substep — how fast biomass climbs toward the habitat capacity.
    pub growth: f32,
    /// Diffusion lerp weight per substep. Must be `< 0.25` for explicit-Euler stability on a 2-D grid.
    pub diffuse: f32,
    /// Reaction-diffusion substeps per `FixedUpdate` tick (integer; more = faster/smoother spread).
    pub substeps: u32,
    /// Initial biomass seeded into every habitat cell at bake time.
    pub seed_v: f32,
    /// Recoil sink per unit normalised illuminance per substep — the photophobia strength.
    pub light_recoil: f32,
    /// Illuminance that reads as "fully lit" (`light01 = min(1, light/light_ref)`).
    pub light_ref: f32,

    // ── Gameplay couplings (consumed by the light / fog / almond-water read-edges; see their systems) ──
    /// How strongly biomass attenuates the gameplay `LightField` (0 = no dimming, 1 = a full mat blacks out).
    pub dim_light: f32,
    /// How strongly dense biomass occludes line-of-sight (soft cover).
    pub occlude_los: f32,
    /// Multiplier applied to almond-water seep under dense mold (mold-cracked concrete weeps more).
    pub seep_boost: f32,
    /// Biomass at or above which a cell counts as "dense" mold (for the LOS occlusion + seep couplings).
    pub dense_v: f32,
}

/// A stable spot-and-spread regime; couplings moderate. Calibrated to leave the field sparse (mold
/// fills its damp habitat over ~tens of seconds and is pushed back by the squad's light).
///
/// **These MUST mirror the `mold:` block in `assets/config/config.ron`** — guarded by
/// `authored_world_config_override_is_a_noop`.
///
/// **The couplings are SHIPPED defaults.** The mold is load-bearing (Phase 3): it dims light (crabs are
/// less light-pushed) and, where dense (biomass >= `dense_v`/`occlude_los` ~= 0.83), hides crabs from the
/// squad's line of sight — a real tactical cost, not cosmetic. Adding the mold also shifts the
/// deterministic trajectory (every new FixedUpdate producer re-bakes the golden, cf. `tests/replay.rs`),
/// which tipped two knife's-edge *held-in* worlds (0xA11CE, 0xBEEF) into squad wipes. Those were
/// re-selected: the held-in set is now `[0x5C09191, 0x1CE5, 0xD00D]`, where the shipped squad produces a
/// real encounter — it survives with margin AND the swarm survives (neither side is wiped). The couplings
/// stay LIVE and tunable: the optimizer (`world_genome` BOUNDS: `dim_light` 0..1, `occlude_los` 0..1.5,
/// `seep_boost` 0.5..6) can push them and weigh the "mold makes it scarier" cost.
///
/// `seep_boost: 1.5` mirrors the old static `almond_water.mold_seep_mult`: the live ramp `1 + 0.5·mold01`
/// stays <= that at all times, so moldy pools keep the sparse, fog-preserving footprint the
/// `almond_pools_stay_small_and_isolated` liveness test guards. The optimizer may push it up to 6×
/// (`world_genome` BOUNDS) and weigh the "moldy zones flood into a healing sheet" trade-off itself.
///
/// **Never put comments inside `fn default()`.** `train apply --dim world` rewrites that body verbatim
/// from the baked elite (`regen_default` in `src/bin/train.rs` brace-matches and replaces the whole
/// body), so anything in there is deleted by the first bake. Document above this impl instead.
impl Default for MoldConfig {
    fn default() -> Self {
        MoldConfig {
            growth: 0.08,
            diffuse: 0.12,
            substeps: 4,
            seed_v: 0.15,
            light_recoil: 0.05,
            light_ref: 6.0,
            dim_light: 0.5,
            occlude_los: 0.6,
            seep_boost: 1.5,
            dense_v: 0.5,
        }
    }
}

/// Validate the `mold:` slice loudly at the door — one `Err` per violation, no fallback (matches
/// `light::validate_config` / `almond_water::validate_config`).
pub fn validate_config(c: &MoldConfig) -> Result<(), String> {
    for (name, v) in [
        ("growth", c.growth),
        ("seed_v", c.seed_v),
        ("light_recoil", c.light_recoil),
        ("dim_light", c.dim_light),
        ("occlude_los", c.occlude_los),
        ("seep_boost", c.seep_boost),
        ("dense_v", c.dense_v),
    ] {
        if !v.is_finite() || v < 0.0 {
            return Err(format!("mold.{name} must be finite and >= 0 (got {v})"));
        }
    }
    if !(c.diffuse >= 0.0 && c.diffuse < 0.25) {
        return Err(format!("mold.diffuse must be in [0, 0.25) for a stable step (got {})", c.diffuse));
    }
    if !(c.light_ref > 0.0 && c.light_ref.is_finite()) {
        return Err(format!("mold.light_ref must be finite and > 0 (got {})", c.light_ref));
    }
    if c.substeps == 0 {
        return Err("mold.substeps must be >= 1".into());
    }
    if !(c.seed_v <= 1.0) {
        return Err(format!("mold.seed_v must be <= 1 (got {})", c.seed_v));
    }
    if !(c.dense_v <= 1.0) {
        return Err(format!("mold.dense_v must be <= 1 (got {})", c.dense_v));
    }
    Ok(())
}

/// The gameplay mold biomass grid. Row-major `y*width + x` over dungeon cells, `V ∈ [0,1]`.
#[derive(Resource)]
pub struct MoldField {
    width: usize,
    height: usize,
    /// Biomass per cell, `[0,1]`. The dynamic state the reaction-diffusion evolves.
    v: Vec<f32>,
    /// Habitat capacity per cell, `{0,1}` — geometry-derived, static after bake.
    k: Vec<f32>,
    /// Floor mask (no-flux boundary + growth gate), static after bake.
    floor: Vec<bool>,
    /// Floor cells in row-major order `(index, cell)` — the fixed iteration list the update walks.
    cells: Vec<(usize, IVec2)>,
    /// Per-tick normalised illuminance, refilled each tick before the substeps (reused to avoid realloc).
    light01: Vec<f32>,
    /// Reused floor-indexed output buffer for the *parallel* reaction-diffusion map: one slot per `cells`
    /// entry, same order. Each substep writes disjoint slots here (no cross-cell reduction), then a serial
    /// scatter copies it back into `v`. Lazily sized to `cells.len()` after bake; never allocates per tick.
    react_out: Vec<f32>,
    /// Precomputed floor-neighbour grid indices per `cells` entry, in the fixed **N, E, S, W** Laplacian
    /// order — `-1` where that neighbour is off-grid or wall (no-flux). The floor mask is static after bake,
    /// so hoisting the per-neighbour bounds + `floor[]` test out of the per-tick stencil is **exact** (same
    /// terms, same fixed order → bit-identical `v`); it just removes the branches and lets the hot loop over
    /// the millions of training-rollout ticks vectorise (pbrt SIMD; Turk 1991 grid stencil). Built at bake.
    nbr: Vec<[i32; 4]>,
}

impl MoldField {
    fn new(width: usize, height: usize) -> Self {
        let n = width * height;
        MoldField {
            width,
            height,
            v: vec![0.0; n],
            k: vec![0.0; n],
            floor: vec![false; n],
            cells: Vec::new(),
            light01: vec![0.0; n],
            react_out: Vec::new(), // lazily sized to cells.len() on first diffuse_react after bake
            nbr: Vec::new(),       // filled at bake once `floor` + `cells` exist
        }
    }

    /// Biomass at a dungeon cell (`0.0` off-grid). The gameplay read used by the light/fog/water couplings.
    pub fn biomass_at(&self, c: IVec2) -> f32 {
        if c.x < 0 || c.y < 0 || c.x as usize >= self.width || c.y as usize >= self.height {
            return 0.0;
        }
        self.v[row_major(c, self.width)]
    }

    /// Biomass sampled at a world position (`0.0` off-grid).
    pub fn sample(&self, dungeon: &Dungeon, pos: Vec3) -> f32 {
        self.biomass_at(dungeon.world_to_cell(pos))
    }

    /// Advance the reaction-diffusion `substeps` times over the floor cells: logistic growth toward the
    /// habitat capacity `k`, a fixed-order 4-neighbour Laplacian (no-flux walls), and a light-recoil sink
    /// from the per-cell `light01` filled by the caller. Pure over the field's own grids (row-major, fixed
    /// order) → bit-reproducible. Extracted from [`mold_update`] so it is unit-testable without the sim.
    fn diffuse_react(&mut self, c: &MoldConfig) {
        // Each substep is a pure stencil: every floor cell's next value is a function of the CURRENT `v`
        // with the fixed N,E,S,W Laplacian order preserved inside the cell, so the result is identical for
        // any thread count (no cross-cell reduction). Parallelised across `cells` — the same data-parallel
        // property Dourvas et al. (2019) exploit to accelerate a Physarum reaction-diffusion CA. The harness
        // pins rayon to one thread and `fold_fingerprint` hashes `v`, so any divergence fails loudly.
        use rayon::prelude::*;
        let Self { v, k, floor, cells, light01, react_out, nbr, width, height, .. } = self;
        if react_out.len() != cells.len() {
            react_out.resize(cells.len(), 0.0); // one-time after bake fills `cells`; a no-op thereafter
        }
        // Branch-free neighbour table: each cell's 4 floor-neighbour grid indices in the fixed N,E,S,W order
        // (`-1` = wall/off-grid = no-flux), precomputed from the STATIC floor mask. Recomputed only when
        // `cells` changes (bake, or a unit test that sets `cells` directly); a cheap length check every real
        // tick. It lets the per-substep stencil below skip the per-neighbour bounds + `floor[]` test — exact
        // (same terms, same order → bit-identical `v`), just branch-free and easier to vectorise.
        if nbr.len() != cells.len() {
            let (w, h) = (*width, *height);
            *nbr = cells
                .iter()
                .map(|&(_, c)| {
                    let mut n = [-1i32; 4];
                    for (slot, (dx, dy)) in [(0, 1), (1, 0), (0, -1), (-1, 0)].into_iter().enumerate() {
                        let (nx, ny) = (c.x + dx, c.y + dy);
                        if nx >= 0
                            && ny >= 0
                            && (nx as usize) < w
                            && (ny as usize) < h
                            && floor[ny as usize * w + nx as usize]
                        {
                            n[slot] = (ny as usize * w + nx as usize) as i32;
                        }
                    }
                    n
                })
                .collect();
        }
        for _ in 0..c.substeps {
            react_out
                .par_iter_mut()
                .zip(cells.par_iter())
                .zip(nbr.par_iter())
                .for_each(|((out, &(idx, _cell)), n)| {
                    let v0 = v[idx];
                    // 4-neighbour Laplacian in the fixed N,E,S,W order; a `-1` slot is a wall/off-grid
                    // neighbour = no-flux (contributes 0), so mold does not leak through walls. The neighbours
                    // are precomputed at bake from the static floor mask, so this is the same sum in the same
                    // order as the branchy version — bit-identical, just branch-free (see `MoldField::nbr`).
                    let mut lap = 0.0f32;
                    for &ni in n {
                        if ni >= 0 {
                            lap += v[ni as usize] - v0;
                        }
                    }
                    let growth = c.growth * v0 * (k[idx] - v0);
                    let recoil = c.light_recoil * light01[idx] * v0;
                    *out = (v0 + growth + c.diffuse * lap - recoil).clamp(0.0, 1.0);
                });
            // Serial scatter: floor-indexed results → grid-indexed `v` (the double-buffer swap).
            for (&(idx, _), &nv) in cells.iter().zip(react_out.iter()) {
                v[idx] = nv;
            }
        }
    }

    /// Total biomass over all cells — a cheap read-out for behavioural tests / diagnostics.
    pub fn total_biomass(&self) -> f32 {
        self.v.iter().sum()
    }

    /// The row-major biomass grid (same layout as `LightField`/`Stig`/`AlmondWater`) — for the couplings
    /// that scale another field by mold (`light::LightField::apply_mold_dim`).
    pub fn biomass_grid(&self) -> &[f32] {
        &self.v
    }

    /// The determinism fingerprint (test-harness only) — folded into `sim_harness::field_hash`.
    #[cfg(feature = "test-harness")]
    pub fn fold_fingerprint(&self, hash: &mut u64) {
        for &v in &self.v {
            for b in v.to_bits().to_le_bytes() {
                *hash ^= u64::from(b);
                *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
}

/// Bake the static habitat + seed the initial biomass, once, at `Startup` — from the same deterministic,
/// GPU-free `infested_cells` mask almond-water reads. Fallible (loud), like `bake_almond_sources`.
fn bake_mold(
    mut field: ResMut<MoldField>,
    dungeon: Res<Dungeon>,
    config: Res<GameConfig>,
) -> Result<(), BevyError> {
    let infested = crate::mycelia::habitat::infested_cells(&dungeon, &config.mycelia)?;
    let w = field.width;
    let seed_v = config.mold.seed_v;
    // Floor mask + habitat capacity + fixed iteration list, all row-major so the field is a pure function
    // of geometry.
    let cells: Vec<(usize, IVec2)> = dungeon.floor_cells().map(|c| (row_major(c, w), c)).collect();
    for &(idx, _) in &cells {
        field.floor[idx] = true;
    }
    for &(idx, _) in &cells {
        if infested.get(idx).copied().unwrap_or(false) {
            field.k[idx] = 1.0;
            field.v[idx] = seed_v;
        }
    }
    field.cells = cells;
    // `nbr` (the branch-free neighbour table) is computed lazily on the first `diffuse_react` after `cells`
    // is set — one compute site, so a test that sets `cells` directly and the real bake can never diverge.
    Ok(())
}

/// Advance the reaction-diffusion one `FixedUpdate` tick: `substeps` of logistic growth + fixed-order
/// diffusion + light recoil. Reads the current-tick `LightField` (ordered `.after(LightFieldWritten)`), so
/// the squad's light pushes the mold back this tick; the mold's own gameplay couplings feed the next tick.
fn mold_update(
    mut field: ResMut<MoldField>,
    dungeon: Res<Dungeon>,
    config: Res<GameConfig>,
    light: Res<LightField>,
) {
    // Profiling span: read the per-system cost under `--features bevy/trace_tracy` (see `perf_hud`).
    let _span = info_span!("mold_update").entered();
    // Fill the per-cell normalised illuminance once (reused across substeps). Split the borrow: read
    // `cells` while writing `light01`, then hand the list back before the reaction-diffusion.
    let light_ref = config.mold.light_ref;
    let cells = std::mem::take(&mut field.cells);
    for &(idx, cell) in &cells {
        let lit = light.sample(&dungeon, dungeon.cell_center(cell)) / light_ref;
        field.light01[idx] = lit.clamp(0.0, 1.0);
    }
    field.cells = cells;
    field.diffuse_react(&config.mold);
}

/// **Mold → light dimming** — the mold's write-edge onto the [`LightField`]. Runs inside the
/// [`LightFieldWritten`] set, AFTER the cones are composed and BEFORE any light reader, so crab photophobia
/// and the mold's own recoil both see the darkened field the same tick. Reads last-tick biomass (the
/// standard one-tick lag), so the mold↔light feedback stays acyclic and deterministic.
fn mold_dim_light(mut light: ResMut<LightField>, mold: Res<MoldField>, config: Res<GameConfig>) {
    light.apply_mold_dim(mold.biomass_grid(), config.mold.dim_light);
}

/// Owns the gameplay [`MoldField`]. Registered in BOTH the windowed game and the headless harness (like
/// [`crate::light::LightFieldPlugin`]) because the field is CPU gameplay state the replay gate must cover.
/// Requires `Dungeon` at build (DungeonPlugin precedes it).
pub struct MoldPlugin;

impl Plugin for MoldPlugin {
    fn build(&self, app: &mut App) {
        let dungeon = app
            .world()
            .get_resource::<Dungeon>()
            .expect("MoldPlugin requires DungeonPlugin to be registered first");
        let field = MoldField::new(dungeon.width, dungeon.height);
        app.insert_resource(field)
            .add_systems(Startup, bake_mold)
            .add_systems(
                FixedUpdate,
                (
                    // Dim the light INSIDE `LightFieldWritten`, after the cones (`apply_dynamic_lights`), so
                    // every `.after(LightFieldWritten)` reader — crab photophobia and `mold_update` below —
                    // deterministically sees the darkened field.
                    mold_dim_light
                        .in_set(LightFieldWritten)
                        .after(crate::light::apply_dynamic_lights),
                    // Then advance the reaction-diffusion, recoiling from the (now mold-dimmed) illuminance.
                    // Ordered AFTER every mold READER — `update_los` (LOS occlusion) and
                    // `accumulate_evaporate_diffuse` (seep boost) — so each reads last-tick biomass and the
                    // MoldField read/write pairs stay acyclic and deterministic.
                    mold_update
                        .after(LightFieldWritten)
                        .after(crate::fog::LosWritten)
                        .after(crate::almond_water::AlmondWaterWritten),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fully-floored w×h field with the fixed row-major cell list, ready for `diffuse_react`.
    fn floored(w: usize, h: usize) -> MoldField {
        let mut f = MoldField::new(w, h);
        for i in 0..w * h {
            f.floor[i] = true;
        }
        f.cells = (0..h)
            .flat_map(|y| (0..w).map(move |x| (y * w + x, IVec2::new(x as i32, y as i32))))
            .collect();
        f
    }

    fn seed_all(f: &mut MoldField, k: f32, v: f32) {
        for (idx, _) in f.cells.clone() {
            f.k[idx] = k;
            f.v[idx] = v;
        }
    }

    #[test]
    fn mold_grows_toward_habitat_capacity() {
        let mut f = floored(5, 5);
        seed_all(&mut f, 1.0, 0.15);
        let before = f.total_biomass();
        for _ in 0..80 {
            f.diffuse_react(&MoldConfig::default());
        }
        let after = f.total_biomass();
        assert!(after > before, "mold must grow in habitat: {before} -> {after}");
        assert!(after > 15.0, "should approach capacity (25 cells → ~25), got {after}");
    }

    #[test]
    fn light_pushes_mold_back() {
        // Isolate recoil (growth off): an identically-seeded lit field must lose more biomass than a dark one.
        let mut c = MoldConfig::default();
        c.growth = 0.0;
        c.light_recoil = 0.2;
        let mut dark = floored(5, 5);
        seed_all(&mut dark, 1.0, 0.5);
        let mut lit = floored(5, 5);
        seed_all(&mut lit, 1.0, 0.5);
        for (idx, _) in lit.cells.clone() {
            lit.light01[idx] = 1.0;
        }
        for _ in 0..20 {
            dark.diffuse_react(&c);
            lit.diffuse_react(&c);
        }
        assert!(
            lit.total_biomass() < dark.total_biomass(),
            "light must push mold back: lit {} !< dark {}",
            lit.total_biomass(),
            dark.total_biomass()
        );
    }

    #[test]
    fn mold_diffuses_to_neighbours() {
        let mut c = MoldConfig::default();
        c.growth = 0.0; // isolate diffusion
        let mut f = floored(5, 5);
        for (idx, _) in f.cells.clone() {
            f.k[idx] = 1.0;
        }
        f.v[2 * 5 + 2] = 1.0; // seed the centre only
        f.diffuse_react(&c);
        assert!(f.v[2 * 5 + 1] > 0.0, "diffusion must spread to a neighbour, got {}", f.v[2 * 5 + 1]);
    }

    #[test]
    fn walls_are_no_flux() {
        // One floor cell walled off on all sides: diffusion has no floor neighbour, so biomass can't leak.
        let mut c = MoldConfig::default();
        c.growth = 0.0;
        let mut f = MoldField::new(3, 3);
        f.floor[1 * 3 + 1] = true;
        f.cells = vec![(1 * 3 + 1, IVec2::new(1, 1))];
        f.k[1 * 3 + 1] = 1.0;
        f.v[1 * 3 + 1] = 0.3;
        f.diffuse_react(&c);
        assert!((f.v[1 * 3 + 1] - 0.3).abs() < 1e-6, "a no-flux wall must not leak biomass");
    }

    #[test]
    fn biomass_stays_bounded() {
        let mut f = floored(4, 4);
        seed_all(&mut f, 1.0, 0.9);
        for _ in 0..100 {
            f.diffuse_react(&MoldConfig::default());
        }
        for (idx, _) in f.cells.clone() {
            assert!((0.0..=1.0).contains(&f.v[idx]), "biomass escaped [0,1]: {}", f.v[idx]);
        }
    }

    #[test]
    fn default_config_validates() {
        assert!(validate_config(&MoldConfig::default()).is_ok());
        let mut bad = MoldConfig::default();
        bad.diffuse = 0.3;
        assert!(validate_config(&bad).is_err(), "diffuse >= 0.25 must be rejected (unstable step)");
    }
}
