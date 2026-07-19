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
use serde::{Deserialize, Serialize};

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
    /// Steering gain a *wounded* creature feels climbing the water gradient (foraging taxis). Scales the
    /// world-space push added to locomotion; tune against creature speed (cf. `lighting.photophilic_gain`).
    pub forage_gain: f32,
    /// Health fraction at or below which a creature forages toward water. Above it, a healthy creature
    /// ignores the field (no cost when full).
    pub forage_wounded_frac: f32,

    // --- Belief / inversion (gameplay; the water does what the population *believes* it does) ---
    /// The prior belief every pool relaxes toward, in [0,1]: **0 = pre-drift cyanide (poison), 1 = post-drift
    /// heal.** The population's baseline reading of the water before any local rumor bends it (lore §6). Belief
    /// is a per-cell field that spreads and relaxes like a reaction-diffusion of rumor — belief *is* a
    /// stigmergic medium (Heylighen 2015).
    pub belief_prior: f32,
    /// Rate/sec each cell's belief relaxes back toward its seeded base (`belief_base`) — a rumor shift fades,
    /// so a transiently-poisoned heal pool recovers while a seeded cyanide pocket stays dangerous.
    pub belief_relax: f32,
    /// Blend weight [0,1] toward the 4-neighbour belief average each tick — rumor spreads between nearby pools.
    pub belief_diffuse: f32,
    /// Fraction [0,1] of floor cells seeded as **cyanide** (base belief 0 = poison) at the bake; the rest read
    /// as heal (base `belief_prior`). A deterministic per-cell hash picks them, so it stays out of
    /// `snapshot_hash`. This is the pre-drift reading surviving in pockets.
    pub belief_poison_frac: f32,
    /// Belief at/above which a pool HEALS (post-drift reading). The band `(belief_flip_lo, belief_flip_hi)` is
    /// an inert deadband — neither heal nor poison — so a pool near the flip point doesn't oscillate.
    pub belief_flip_hi: f32,
    /// Belief at/below which a pool POISONS (pre-drift cyanide reading). Must be ≤ `belief_flip_hi`.
    pub belief_flip_lo: f32,
    /// HP/sec a creature loses drinking a fully-poison (cyanide) pool — the signed twin of `heal_rate`. Drinks
    /// the cell down exactly as a heal does (you drink the poison), so a dry cyanide cell can't hurt you.
    pub poison_rate: f32,
    /// Belief nudge/sec a SAFE drink deposits toward the heal reading — a pool that heals earns its good name.
    pub rumor_gain: f32,
    /// Belief nudge/sec a POISONING deposits toward the cyanide reading — "someone was poisoned here" spreads.
    pub death_rumor_gain: f32,

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
    /// How much the raw thin-film reflectance is pulled toward its channel mean, [0,1] — real oil/water films
    /// read as a muted golds/teals/magentas mixture, not a clean rainbow. 1 = full spectral, 0 = grey. (Was a
    /// hardcoded 0.6 in the shader; exposed for tuning.)
    pub iridescence_mute: f32,
    /// Base tint (linear-RGB) of a pool the population reads as **cyanide** (belief 0). The puddle lerps from
    /// this sickly hue at belief 0 to `almond_tint` at belief 1 — the diegetic "this pool is wrong" tell (that
    /// an anosmic creature still can't smell). Visual-only.
    pub poison_tint: [f32; 3],
}

/// The **evolvable** slice of the Almond Water gameplay dynamics — the 16 knobs the offline world search
/// (`squad_ai::world_genome`) tunes and the harness installs per rollout, so the RL/QD search can co-evolve
/// the water (seep/heal/poison/belief) alongside combat. `Copy` + `Serialize` so an evolved world decodes to a
/// readable RON diff (the reward-hacking guard). Excludes the structural (`pool_spacing`, `capacity`) and
/// visual knobs — those stay fixed. `Default` MUST mirror the shipped `config.ron` values (guarded by
/// `authored_world_config_override_is_a_noop`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlmondWaterDynamics {
    pub strong_seep: f32,
    pub evaporate: f32,
    pub diffuse: f32,
    pub heal_rate: f32,
    pub heal_per_unit_water: f32,
    pub poison_rate: f32,
    pub belief_prior: f32,
    pub belief_relax: f32,
    pub belief_diffuse: f32,
    pub belief_poison_frac: f32,
    pub belief_flip_hi: f32,
    pub belief_flip_lo: f32,
    pub rumor_gain: f32,
    pub death_rumor_gain: f32,
    pub forage_gain: f32,
    pub forage_wounded_frac: f32,
}

/// MUST match the `almond_water:` gameplay knobs in `assets/config/config.ron`.
///
/// **Never put comments inside `fn default()`.** `train apply --dim world` rewrites that body verbatim from
/// the baked elite (`regen_default` in `src/bin/train.rs` brace-matches and replaces the whole body), so
/// anything in there is deleted by the first bake. Document above this impl instead.
impl Default for AlmondWaterDynamics {
    fn default() -> Self {
        Self {
            strong_seep: 8.0,
            evaporate: 0.05,
            diffuse: 0.02,
            heal_rate: 5.0,
            heal_per_unit_water: 1.5,
            poison_rate: 5.0,
            belief_prior: 1.0,
            belief_relax: 0.05,
            belief_diffuse: 0.02,
            belief_poison_frac: 0.15,
            belief_flip_hi: 0.6,
            belief_flip_lo: 0.4,
            rumor_gain: 0.5,
            death_rumor_gain: 1.0,
            forage_gain: 10.0,
            forage_wounded_frac: 0.6,
        }
    }
}

impl AlmondWaterDynamics {
    /// Read the evolvable slice out of a full config.
    pub fn from_config(c: &AlmondWaterConfig) -> Self {
        Self {
            strong_seep: c.strong_seep,
            evaporate: c.evaporate,
            diffuse: c.diffuse,
            heal_rate: c.heal_rate,
            heal_per_unit_water: c.heal_per_unit_water,
            poison_rate: c.poison_rate,
            belief_prior: c.belief_prior,
            belief_relax: c.belief_relax,
            belief_diffuse: c.belief_diffuse,
            belief_poison_frac: c.belief_poison_frac,
            belief_flip_hi: c.belief_flip_hi,
            belief_flip_lo: c.belief_flip_lo,
            rumor_gain: c.rumor_gain,
            death_rumor_gain: c.death_rumor_gain,
            forage_gain: c.forage_gain,
            forage_wounded_frac: c.forage_wounded_frac,
        }
    }

    /// Overwrite the evolvable gameplay knobs of a full config, leaving structural + visual knobs untouched.
    pub fn apply_to(&self, c: &mut AlmondWaterConfig) {
        c.strong_seep = self.strong_seep;
        c.evaporate = self.evaporate;
        c.diffuse = self.diffuse;
        c.heal_rate = self.heal_rate;
        c.heal_per_unit_water = self.heal_per_unit_water;
        c.poison_rate = self.poison_rate;
        c.belief_prior = self.belief_prior;
        c.belief_relax = self.belief_relax;
        c.belief_diffuse = self.belief_diffuse;
        c.belief_poison_frac = self.belief_poison_frac;
        c.belief_flip_hi = self.belief_flip_hi;
        c.belief_flip_lo = self.belief_flip_lo;
        c.rumor_gain = self.rumor_gain;
        c.death_rumor_gain = self.death_rumor_gain;
        c.forage_gain = self.forage_gain;
        c.forage_wounded_frac = self.forage_wounded_frac;
    }
}

/// Loud, one-path validation (mirrors [`crate::light::validate_config`]). Every knob finite and in range;
/// one `Err` per violation, no fallback.
pub fn validate_config(c: &AlmondWaterConfig) -> Result<(), String> {
    for (name, v) in [
        ("strong_seep", c.strong_seep),
        ("evaporate", c.evaporate),
        ("heal_rate", c.heal_rate),
        ("belief_relax", c.belief_relax),
        ("poison_rate", c.poison_rate),
        ("rumor_gain", c.rumor_gain),
        ("death_rumor_gain", c.death_rumor_gain),
        ("forage_gain", c.forage_gain),
        ("min_visible_level", c.min_visible_level),
        ("film_thickness_nm", c.film_thickness_nm),
        ("iridescence_strength", c.iridescence_strength),
        ("moisture_feed_gain", c.moisture_feed_gain),
        ("iridescence_mute", c.iridescence_mute),
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
    // Blend weights, a health fraction, and belief (a 0=poison..1=heal reading) all must sit in [0, 1] or the
    // field never diffuses / always saturates, every creature forages regardless of health, or belief runs
    // off its poison↔heal axis.
    for (name, v) in [
        ("diffuse", c.diffuse),
        ("forage_wounded_frac", c.forage_wounded_frac),
        ("belief_prior", c.belief_prior),
        ("belief_diffuse", c.belief_diffuse),
        ("belief_poison_frac", c.belief_poison_frac),
        ("belief_flip_hi", c.belief_flip_hi),
        ("belief_flip_lo", c.belief_flip_lo),
    ] {
        if !(v.is_finite() && (0.0..=1.0).contains(&v)) {
            return Err(format!("almond_water.{name} must be in [0, 1] (got {v})"));
        }
    }
    if c.belief_flip_lo > c.belief_flip_hi {
        return Err(format!(
            "almond_water.belief_flip_lo ({}) must be <= belief_flip_hi ({}) — an inverted deadband",
            c.belief_flip_lo, c.belief_flip_hi
        ));
    }
    for (name, tint) in [("almond_tint", c.almond_tint), ("poison_tint", c.poison_tint)] {
        if tint.iter().any(|ch| !ch.is_finite() || *ch < 0.0) {
            return Err(format!("almond_water.{name} channels must be finite and >= 0 (got {tint:?})"));
        }
    }
    Ok(())
}

/// System set for the field writers ([`accumulate_evaporate_diffuse`] then [`almond_water_effect`], chained).
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
    /// Reused floor-indexed output buffer for the *parallel* diffusion maps. The level and belief passes run
    /// sequentially within a tick, so they share it: the map writes disjoint slots (one per `floor_cells`
    /// entry, no cross-cell reduction), then a serial scatter copies into the grid-indexed double-buffer.
    /// Lazily sized to `floor_cells.len()` after the bake; never allocates per tick.
    diffuse_out: Vec<f32>,
    /// Per-cell **belief** in [0,1]: 0 = the water reads as pre-drift cyanide (poison), 1 = post-drift heal.
    /// A rumor field over the floor — seeded from `belief_base` at the bake, then each tick relaxed toward its
    /// base + diffused, and nudged by the effect system (safe drink → +heal reading; a poisoning →
    /// +cyanide reading). This is what makes the water do what the population believes (lore §6).
    belief: Vec<f32>,
    /// Per-cell **base belief** — the pool's seeded identity that `belief` relaxes back toward (a
    /// `belief_poison_frac` slice of floor cells are seeded to 0 = cyanide, the rest to `belief_prior`).
    /// Static after the bake, like `sources`, so a rumor shift is transient (a massacred heal-pool reads
    /// poison for a while, then recovers) while seeded cyanide pockets stay dangerous.
    belief_base: Vec<f32>,
    /// Double-buffer for the belief diffusion pass (kept separate from `scratch` so level and belief can
    /// diffuse in the same tick without aliasing).
    belief_scratch: Vec<f32>,
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
            diffuse_out: Vec::new(), // lazily sized to floor_cells.len() on first tick after bake
            belief: vec![0.0; n],
            belief_base: vec![0.0; n],
            belief_scratch: vec![0.0; n],
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

    /// The population's belief at `cell` in [0,1] (0 = cyanide/poison reading, 1 = heal). Off-grid reads 0.
    /// The effect system turns this into a signed heal/poison rate.
    pub fn belief_at(&self, cell: IVec2) -> f32 {
        if in_grid(cell, self.width, self.height) {
            self.belief[row_major(cell, self.width)]
        } else {
            0.0
        }
    }

    /// Nudge a cell's belief by `delta`, clamped to [0,1] — the rumor deposit (a safe drink pushes toward the
    /// heal reading, a poisoning toward cyanide). The caller applies these in a sorted order so the
    /// non-associative `+=` stays reproducible, exactly like `drink` contention.
    fn nudge_belief(&mut self, cell: IVec2, delta: f32) {
        if delta != 0.0 && in_grid(cell, self.width, self.height) {
            let idx = row_major(cell, self.width);
            self.belief[idx] = (self.belief[idx] + delta).clamp(0.0, 1.0);
        }
    }

    /// One accumulate → evaporate → diffuse step, floor cells only, in `Stig::evaporate_diffuse`'s order and
    /// with its determinism discipline (fixed E/W/S/N neighbour order — float add is non-associative — and a
    /// double-buffered diffuse so rock cells stay invariantly 0). `dt` in seconds.
    #[allow(clippy::too_many_arguments)]
    fn tick(
        &mut self,
        dt: f32,
        evaporate: f32,
        diffuse: f32,
        capacity: f32,
        // Mold→seep coupling: the LIVE mold biomass grid + its knobs. A spring's seep is scaled by the mold
        // over it — `boost = 1 + (seep_boost − 1)·min(1, biomass/dense_v)` — so a spring in thick mold weeps
        // more and one where the mold has receded (pushed back by the squad's light) dries toward its base
        // rate. Deterministic: read-only over `mold`, applied in the same fixed floor-cell order.
        mold: &[f32],
        seep_boost: f32,
        dense_v: f32,
        // Belief (rumor) dynamics: relax each cell toward its seeded base, then diffuse. Same fixed-order,
        // double-buffered discipline as the water diffuse, so it stays bit-reproducible.
        belief_relax: f32,
        belief_diffuse: f32,
    ) {
        // The two diffusion passes below are pure stencils — each cell's next value is a function of the
        // PREVIOUS grid with the fixed E/W/S/N order preserved — so they parallelise bit-identically for any
        // thread count (no cross-cell reduction; Dourvas et al. 2019 exploit the same for a Physarum CA). Size
        // the shared floor-indexed scratch once after the bake fills `floor_cells`.
        use rayon::prelude::*;
        if self.diffuse_out.len() != self.floor_cells.len() {
            self.diffuse_out.resize(self.floor_cells.len(), 0.0);
        }
        // 1+2. Accumulate the (mold-boosted) seep, then evaporate, in one in-place pass (an evaporation of
        //      the just-added seep is exactly a steady-state cap; the two commute up to O(dt²), same as `Stig`).
        let retain = (1.0 - evaporate * dt).clamp(0.0, 1.0);
        let inv_dense = 1.0 / dense_v.max(0.01);
        for &(idx, _) in &self.floor_cells {
            let m01 = (mold.get(idx).copied().unwrap_or(0.0) * inv_dense).clamp(0.0, 1.0);
            let boost = 1.0 + (seep_boost - 1.0) * m01;
            let filled = (self.level[idx] + self.sources[idx] * boost * dt).min(capacity);
            self.level[idx] = filled * retain;
        }

        // 3. Diffuse: blend each floor cell toward the average of its floor neighbours (double-buffered).
        //    Parallel: the map reads the OLD `level`, the barrier completes, then a serial scatter + swap.
        if diffuse > 0.0 {
            let (w, h) = (self.width, self.height);
            let Self { level, scratch, floor_cells, floor_mask, diffuse_out, .. } = self;
            diffuse_out
                .par_iter_mut()
                .zip(floor_cells.par_iter())
                .for_each(|(out, &(idx, pos))| {
                    let (x, y) = (pos.x, pos.y);
                    let mut sum = 0.0;
                    let mut n = 0.0;
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let nb = IVec2::new(x + dx, y + dy);
                        if nb.x >= 0 && nb.y >= 0 && (nb.x as usize) < w && (nb.y as usize) < h {
                            let nidx = (nb.y as usize) * w + nb.x as usize;
                            if floor_mask[nidx] {
                                sum += level[nidx];
                                n += 1.0;
                            }
                        }
                    }
                    let avg = if n > 0.0 { sum / n } else { level[idx] };
                    *out = level[idx] * (1.0 - diffuse) + avg * diffuse;
                });
            for (&(idx, _), &v) in floor_cells.iter().zip(diffuse_out.iter()) {
                scratch[idx] = v;
            }
            std::mem::swap(level, scratch);
        }

        // Peak for the visual's 0..1 normalisation. Order-independent (`max`), so it never perturbs the sim.
        self.peak = self.floor_cells.iter().map(|&(idx, _)| self.level[idx]).fold(0.0f32, f32::max);

        // 4. Belief relax: each floor cell drifts back toward its seeded base (a rumor shift fades). The
        //    effect-system deposits (rumor / poison) push it away between ticks; this pulls it home.
        if belief_relax > 0.0 {
            let k = (belief_relax * dt).clamp(0.0, 1.0);
            for &(idx, _) in &self.floor_cells {
                self.belief[idx] += (self.belief_base[idx] - self.belief[idx]) * k;
            }
        }

        // 5. Belief diffuse: rumor spreads to floor neighbours (double-buffered, fixed E/W/S/N order). Blending
        //    two [0,1] values stays in [0,1], so belief never leaves its poison↔heal axis.
        if belief_diffuse > 0.0 {
            let (w, h) = (self.width, self.height);
            let Self { belief, belief_scratch, floor_cells, floor_mask, diffuse_out, .. } = self;
            diffuse_out
                .par_iter_mut()
                .zip(floor_cells.par_iter())
                .for_each(|(out, &(idx, pos))| {
                    let (x, y) = (pos.x, pos.y);
                    let mut sum = 0.0;
                    let mut n = 0.0;
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let nb = IVec2::new(x + dx, y + dy);
                        if nb.x >= 0 && nb.y >= 0 && (nb.x as usize) < w && (nb.y as usize) < h {
                            let nidx = (nb.y as usize) * w + nb.x as usize;
                            if floor_mask[nidx] {
                                sum += belief[nidx];
                                n += 1.0;
                            }
                        }
                    }
                    let avg = if n > 0.0 { sum / n } else { belief[idx] };
                    *out = belief[idx] * (1.0 - belief_diffuse) + avg * belief_diffuse;
                });
            for (&(idx, _), &v) in floor_cells.iter().zip(diffuse_out.iter()) {
                belief_scratch[idx] = v;
            }
            std::mem::swap(belief, belief_scratch);
        }
    }

    /// FNV-1a-fold the exact bit pattern of every `level`, `sources`, and `belief` cell (the **full** grid, so
    /// the rock-cells-stay-0 invariant is pinned too) plus `peak`, into `hash`. The determinism oracle for the
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
        for &v in &self.belief {
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
        // SORT-OK: bare sizes from a seeded bake, not an ECS query.
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

    /// Force every floor cell's belief (and its base, so `relax` holds it) to `belief` — a harness helper so a
    /// poison/heal/flip test can pin the population's reading without waiting for rumor dynamics. Test-only.
    #[cfg(feature = "test-harness")]
    pub fn test_set_belief(&mut self, belief: f32) {
        for &(idx, _) in &self.floor_cells {
            self.belief[idx] = belief;
            self.belief_base[idx] = belief;
        }
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
/// of one continuous sheet. A mold spring's extra seep is applied LIVE in [`AlmondWater::tick`] (scaled by
/// the current mold biomass via `mold.seep_boost`), not baked here. Every non-spring cell stays dry.
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
        // Base seep at the spring. The mold-cracked boost is applied LIVE in `tick` (scaled by the current
        // `MoldField` biomass over the cell via `mold.seep_boost`), not baked once — so a spring weeps more as
        // mold thickens and dries back as the squad's light pushes it away. A mold-habitat cell is still
        // eligible above, so the moldy regions have springs to boost.
        sources[idx] = cfg.strong_seep;
    }

    // Seed each floor cell's base belief: a deterministic `belief_poison_frac` slice reads as cyanide (base 0),
    // the rest as heal (base `belief_prior`). A per-cell splitmix hash picks them — pure/deterministic, like
    // `sources`, so it stays out of `snapshot_hash`; only the per-tick belief evolution folds into `field_hash`.
    // `belief` starts at its base (the population's initial reading of each pool).
    let mut belief_base = vec![0.0f32; w * h];
    let mut belief = vec![0.0f32; w * h];
    for &(idx, _) in &floor_cells {
        let base = if cell_hash01(idx) < cfg.belief_poison_frac { 0.0 } else { cfg.belief_prior };
        belief_base[idx] = base;
        belief[idx] = base;
    }

    field.sources = sources;
    field.belief = belief;
    field.belief_base = belief_base;
    field.belief_scratch = vec![0.0f32; w * h];
    field.floor_cells = floor_cells;
    field.floor_mask = floor_mask;
    Ok(())
}

/// Deterministic per-cell hash → [0,1). A splitmix64 finalizer over the row-major index, so the cyanide-pocket
/// seeding (and any future per-cell coin-flip) is reproducible and arch-stable — no RNG in the hash path.
fn cell_hash01(idx: usize) -> f32 {
    let mut z = (idx as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // Top 24 bits → a uniform [0,1) with exact f32 precision.
    ((z >> 40) as f32) / ((1u64 << 24) as f32)
}

/// Field update: accumulate the seep, evaporate, diffuse. `FixedUpdate`, in [`AlmondWaterWritten`], before
/// the heal. Writes `level` → folds into the field replay hash.
fn accumulate_evaporate_diffuse(
    mut field: ResMut<AlmondWater>,
    config: Res<GameConfig>,
    time: Res<Time>,
    // The live mold field boosts seep where it is dense (mold-cracked concrete weeps more). Reads last-tick
    // biomass — `mold_update` is ordered `.after(AlmondWaterWritten)` so the read/write pair is acyclic.
    mold: Res<crate::mold::MoldField>,
) {
    // Profiling span: read the per-system cost under `--features bevy/trace_tracy` (see `perf_hud`).
    let _span = info_span!("almond_water_diffuse").entered();
    let cfg = &config.almond_water;
    let (evaporate, diffuse, capacity) = (cfg.evaporate, cfg.diffuse, cfg.capacity);
    let (belief_relax, belief_diffuse) = (cfg.belief_relax, cfg.belief_diffuse);
    field.tick(
        time.delta_secs(),
        evaporate,
        diffuse,
        capacity,
        mold.biomass_grid(),
        config.mold.seep_boost,
        config.mold.dense_v,
        belief_relax,
        belief_diffuse,
    );
}

/// Apply the water's effect to every biological standing in it — **the belief/inversion mechanic**: the pool
/// does what the population believes it does. Belief at the cell selects one signed path: heal (post-drift
/// reading), poison (pre-drift cyanide reading), or inert (the unsettled deadband). Both heal and poison drink
/// the cell down. `FixedUpdate`, after the field update and after `medic_heal` (both take `&mut Health`, so the
/// order is pinned for deterministic composition when two `Health` writers land on one unit in one tick).
/// Mirrors `squad_ai::actions::medic_heal`'s rate-based idiom, minus the medic gate — the water *is* the source.
///
/// **Drink contention determinism:** several drinkers can share a cell, and both `drink` (clamped at 0) and
/// the rumor `nudge_belief` (clamped to [0,1]) mutate the cell with a non-associative, *clamped* `f32 ±`, so
/// the candidates are sorted before any mutation — the same sorted-application discipline `snapshot_hash`'s
/// rows and `Stig`'s deposits use. A clamp is what makes this bite even when the magnitudes are equal: on a
/// nearly-dry cell the first drinker takes what is left and the second gets nothing.
///
/// The key ends with [`CyanideSmell::id`], and that tiebreak is load-bearing, not decoration. This sort was
/// keyed `(cell, current, pos.x, pos.z)` and *documented as* a total order. It was not — two crabs
/// `clamp_to_patch`-ed against the same wall hold BIT-IDENTICAL coordinates, and at equal health every
/// component of the key matches, so `sort_unstable` fell through to the ECS query order the sort exists to
/// erase. Measured on held-in world `0xA11CE`: **6 fully-tied pairs at tick 1580**, all at
/// `pos=(77.94, 12.94) hp=25/25`, and the episode diverged ~15 ticks later. Tied drinkers are *not*
/// interchangeable (different `anosmic`, mode, carry phase), so the swap is observable: a wounded crab's
/// forage push is a ±1 sign flip on `belief` crossing a threshold, which turns a lost drink into a
/// 0.2-unit-per-tick lurch the other way.
fn almond_water_effect(
    mut field: ResMut<AlmondWater>,
    dungeon: Res<Dungeon>,
    config: Res<GameConfig>,
    time: Res<Time>,
    mut drinkers: Query<(Entity, &Transform, &mut Health, &crate::health::CyanideSmell), With<Biological>>,
) {
    let cfg = &config.almond_water;
    let dt = time.delta_secs();

    // Immutable pass: collect every living biological with its stable sort key. Not just the wounded — a pool
    // the population reads as cyanide poisons a full-health drinker too. `to_bits` on a deterministic-core
    // float is a total, reproducible order; the position tie-break decorrelates two co-located drinkers.
    let mut cands: Vec<(u32, u32, u32, u32, u32, u64, Entity, IVec2)> = drinkers
        .iter()
        .filter(|(_, _, h, _)| h.max > 0.0)
        .map(|(e, t, h, smell)| {
            let p = t.translation;
            let cell = dungeon.world_to_cell(p);
            (
                cell.y as u32,
                cell.x as u32,
                h.current.to_bits(),
                p.x.to_bits(),
                p.z.to_bits(),
                smell.id,
                e,
                cell,
            )
        })
        .collect();
    if cands.is_empty() {
        return;
    }
    // TOTAL order: `smell.id` is the tiebreak, and without it this is not one — see the note above.
    crate::sort_total!(&mut cands, |k: &(u32, u32, u32, u32, u32, u64, Entity, IVec2)| (k.0, k.1, k.2, k.3, k.4, k.5));

    // Mutable pass, in the sorted order. ONE signed path: belief at the cell selects heal (+), poison (−), or
    // inert (the deadband). Both heal and poison DRINK the cell down — you drink the water either way, so a dry
    // cell can neither heal nor hurt. `want`/`current` are read fresh so this composes on top of `medic_heal`
    // and the same-tick damage set. Rumor deposits (a safe drink reinforces the heal reading, a poisoning the
    // cyanide reading) land in the same sorted order — the non-associative belief `+=` stays reproducible.
    for (.., e, cell) in cands {
        let Ok((_, _, mut health, _)) = drinkers.get_mut(e) else {
            continue;
        };
        let belief = field.belief_at(cell);
        if belief >= cfg.belief_flip_hi {
            // Heal reading: restore HP, drink the cell down, reinforce the pool's good name.
            let want = (cfg.heal_rate * dt).min(health.max - health.current);
            if want <= 0.0 {
                continue;
            }
            let got = field.drink(cell, want / cfg.heal_per_unit_water);
            if got > 0.0 {
                health.current = (health.current + got * cfg.heal_per_unit_water).min(health.max);
                field.nudge_belief(cell, cfg.rumor_gain * dt);
            }
        } else if belief <= cfg.belief_flip_lo {
            // Cyanide reading: drink the poison and take damage (can kill), spread the "poisoned here" rumor.
            let intended = cfg.poison_rate * dt;
            let got = field.drink(cell, intended / cfg.heal_per_unit_water);
            if got > 0.0 {
                health.current = (health.current - got * cfg.heal_per_unit_water).max(0.0);
                field.nudge_belief(cell, -cfg.death_rumor_gain * dt);
            }
        }
        // else: the inert deadband — the pool's reading is unsettled, so it neither heals nor poisons.
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
                almond_water_effect
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
            forage_gain: 10.0,
            forage_wounded_frac: 0.6,
            belief_prior: 1.0,
            belief_relax: 0.05,
            belief_diffuse: 0.02,
            belief_poison_frac: 0.15,
            belief_flip_hi: 0.6,
            belief_flip_lo: 0.4,
            poison_rate: 5.0,
            rumor_gain: 0.5,
            death_rumor_gain: 1.0,
            almond_tint: [0.92, 0.85, 0.70],
            min_visible_level: 1.0,
            film_thickness_nm: 320.0,
            film_ior: 1.33,
            iridescence_strength: 0.25,
            moisture_feed_gain: 0.3,
            iridescence_mute: 0.6,
            poison_tint: [0.55, 0.70, 0.40],
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
            f.tick(dt, e, 0.0, cap, &[], 1.0, 0.5, 0.0, 0.0); // diffuse 0: isolated cell; no mold; no belief dynamics
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
        f.tick(1.0 / 60.0, 0.0, 0.0, 42.0, &[], 1.0, 0.5, 0.0, 0.0);
        assert_eq!(f.level[0], 42.0);
        assert_eq!(f.peak(), 42.0);
    }

    #[test]
    fn diffuse_spreads_to_a_neighbour_and_conserves_between_two_cells() {
        // Two adjacent floor cells, one full, one dry; no seep, no drying → diffusion only.
        let mut f = grid(2, 1, &[(0, 0), (1, 0)]);
        f.level[0] = 100.0;
        f.tick(1.0 / 60.0, 0.0, 0.5, 1000.0, &[], 1.0, 0.5, 0.0, 0.0);
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

        // Belief / inversion knobs.
        let mut c = valid_cfg();
        c.belief_poison_frac = 1.5; // a fraction must be in [0, 1]
        assert!(validate_config(&c).is_err());

        let mut c = valid_cfg();
        c.poison_rate = -1.0; // a rate can't be negative
        assert!(validate_config(&c).is_err());

        let mut c = valid_cfg();
        (c.belief_flip_lo, c.belief_flip_hi) = (0.8, 0.4); // an inverted deadband
        assert!(validate_config(&c).is_err());
    }

    #[test]
    fn belief_relaxes_toward_base_then_diffuses() {
        // Relax only: a cell poisoned below its heal base drifts back toward the base (the rumor fades).
        let mut f = grid(2, 1, &[(0, 0), (1, 0)]);
        f.belief_base[0] = 1.0;
        f.belief[0] = 0.0;
        f.belief_base[1] = 1.0;
        f.belief[1] = 1.0;
        f.tick(1.0 / 60.0, 0.0, 0.0, 100.0, &[], 1.0, 0.5, 0.5, 0.0); // relax 0.5, no belief diffuse
        assert!(f.belief[0] > 0.0 && f.belief[0] < 1.0, "belief relaxed toward base: {}", f.belief[0]);
        assert!((f.belief[1] - 1.0).abs() < 1.0e-6, "a cell already at base stays put");

        // Diffuse only: a poison cell (0) and a heal cell (1) blend halfway toward each other, conserving sum.
        let mut g = grid(2, 1, &[(0, 0), (1, 0)]);
        g.belief_base[0] = 0.0;
        g.belief[0] = 0.0;
        g.belief_base[1] = 1.0;
        g.belief[1] = 1.0;
        g.tick(1.0 / 60.0, 0.0, 0.0, 100.0, &[], 1.0, 0.5, 0.0, 0.5); // no relax, belief diffuse 0.5
        assert!((g.belief[0] - 0.5).abs() < 1.0e-4, "belief diffused: {}", g.belief[0]);
        assert!((g.belief[1] - 0.5).abs() < 1.0e-4, "belief diffused: {}", g.belief[1]);
    }

    #[test]
    fn nudge_belief_deposits_and_clamps() {
        let mut f = grid(1, 1, &[(0, 0)]);
        f.belief[0] = 0.5;
        f.nudge_belief(IVec2::new(0, 0), 0.3);
        assert!((f.belief_at(IVec2::new(0, 0)) - 0.8).abs() < 1.0e-6);
        f.nudge_belief(IVec2::new(0, 0), 5.0); // clamps at 1
        assert_eq!(f.belief_at(IVec2::new(0, 0)), 1.0);
        f.nudge_belief(IVec2::new(0, 0), -9.0); // clamps at 0
        assert_eq!(f.belief_at(IVec2::new(0, 0)), 0.0);
        // Off-grid is a no-op (and reads 0).
        f.nudge_belief(IVec2::new(99, 99), 1.0);
        assert_eq!(f.belief_at(IVec2::new(99, 99)), 0.0);
    }

    #[test]
    fn cell_hash01_is_deterministic_and_roughly_uniform() {
        for idx in [0usize, 1, 42, 1000, 36_863] {
            assert_eq!(cell_hash01(idx).to_bits(), cell_hash01(idx).to_bits(), "hash is a pure fn");
            assert!((0.0..1.0).contains(&cell_hash01(idx)), "hash in [0,1)");
        }
        // A `belief_poison_frac`-style slice should be about that fraction of cells (uniform hash).
        let n = 20_000usize;
        let below = (0..n).filter(|&i| cell_hash01(i) < 0.15).count();
        let frac = below as f32 / n as f32;
        assert!((frac - 0.15).abs() < 0.02, "hash not ~uniform (0.15 slice got {frac})");
    }
}
