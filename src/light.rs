//! Diegetic lighting — fluorescent fixtures that actually light the Backrooms, the queryable
//! [`LightField`] gameplay grid (Phase 1), and the light-response markers `Photophobic` /
//! `Phototropic` / `Photophilic` (Phase 2+) that let creatures develop emergent behaviour around
//! light and its absence.
//!
//! Design + literature review: `slop/research/2026-07-11-backrooms-lighting-review-and-design.md`.
//! Bevy's raster renderer does **not** let an emissive `StandardMaterial` illuminate other surfaces
//! (there is no baked GI here), so each fixture lights the scene with a real clustered [`PointLight`];
//! GTAO (Bevy's SSAO *is* GTAO — Jimenez et al., "Practical Real-Time Strategies for Accurate Indirect
//! Occlusion", SIGGRAPH 2016) plus 0.19 contact shadows carve depth into the otherwise flat wash.
//!
//! **Split by concern so the deterministic core stays clean:**
//! - Environment fill (ambient + directional key) lives in [`crate::world`] — pure light *data*, safe
//!   in the headless harness, and config-driven from the same `lighting:` slice.
//! - Fixtures + camera screen-space FX (real lights, GTAO, contact shadows) are cosmetic/GPU and live in
//!   [`LightingPlugin`], registered **only** in the windowed game (never `sim_harness`), so the
//!   exact-hash core never depends on a GPU.
//! - [`LightField`] (Phase 1) is CPU gameplay state read by creature AI, so it *is* harness-visible.

use std::collections::HashMap;

use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use serde::{Deserialize, Serialize};

use crate::config::GameConfig;
use crate::dungeon::Dungeon;
use crate::placement::{ir::RegionId, PlacedIn};

/// The `lighting:` slice of `assets/config/config.ron` — every light knob, one source of truth
/// (see [`GameConfig`]). Read by both [`crate::world`] (environment fill) and [`LightingPlugin`]
/// (fixtures). No fallback: a missing/invalid slice is a loud startup panic via [`validate_config`].
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct LightingConfig {
    /// Residual uniform fill left *under* the environment map — the only remaining use of Bevy's flat
    /// `GlobalAmbientLight`. It ran the whole ambient path at 500, then 200; it is now a small floor, and
    /// [`env_brightness`] carries the fill instead. The reason is structural, not a taste call: a uniform
    /// ambient term is added identically to every surface whatever its normal, so it can never shade
    /// anything — it is precisely what made the scene read as clay. Kept non-zero only so a surface facing
    /// away from every light does not crush to pure black. Read by [`crate::world::WorldPlugin`].
    pub ambient_brightness: f32,
    /// Tint of that residual fill (sRGB triple) — warm fluorescent.
    pub ambient_color: [f32; 3],
    /// Directional key-light illuminance (lux) — a weak steep fill so low-poly tiles keep some shading.
    pub key_illuminance: f32,
    /// Per-fixture real-light luminous power (lumens). Bevy's default `PointLight` is a 1e6-lm cinema
    /// light at range 20; a fluorescent fixture is a fraction of that, tuned against the camera exposure.
    pub fixture_intensity: f32,
    /// Per-fixture light range (metres) — the area-of-effect cut-off, tuned with `fixture_intensity`.
    pub fixture_range: f32,
    /// Per-fixture light colour — cool white with a faint green cast (the low-CRI halophosphate tint that
    /// makes the Backrooms look uneasy: green channel highest, magenta-deficient — Klipstein's fluorescent
    /// spectra).
    pub fixture_color: [f32; 3],
    /// Emissive strength for the fixture *mesh* glow (linear-RGB multiplier on `fixture_color`). LDR, so
    /// values ~1.5–3 read as a lit tube/panel without HDR bloom. This is what the player sees glowing;
    /// the real illumination is the paired [`PointLight`] (Bevy raster: emissive ≠ light).
    pub fixture_emissive: f32,
    /// **Gameplay** illuminance each fixture contributes at its centre in the [`LightField`] (peaks here,
    /// falls to 0 at `fixture_range`). A *gameplay* scalar in the field's own units — deliberately separate
    /// from the render `fixture_intensity` (lumens): the AI wants "how lit is this point", not photometry.
    /// The field's physical reach reuses `fixture_range` (so render pool and gameplay reach agree).
    pub field_intensity: f32,
    /// Steering strength for a [`Photophobic`] creature descending the light gradient (toward the dark).
    /// Scales the world-space push added to locomotion; tune against creature speed.
    pub photophobic_gain: f32,
    /// Steering strength for a [`Photophilic`] creature climbing the light gradient (toward the light).
    pub photophilic_gain: f32,
    /// Max fractional size increase a [`Phototropic`] fruit body reaches in full light — real fungal
    /// photomorphogenesis (light-gated fruiting-body enlargement, Zhang et al., PLoS ONE 2015). 0.5 = up
    /// to 50% larger cap under a bright lamp; 0 disables the effect. Read by `mycelia::grow_fruit_bodies`.
    pub mushroom_light_size_bonus: f32,
    /// How fast that size bonus eases in, in mesh scale-units per second. Kept slow so the enlargement
    /// stays below motion perception (the mold's speed-limit ethos), accruing over the fruit body's life.
    pub mushroom_light_size_rate: f32,
    /// Depth of the fixtures' steady mains-hum flicker, `0..1` (a few percent reads as a fluorescent
    /// shimmer). Purely cosmetic — modulates the real point lights only, never the gameplay `LightField`.
    pub flicker_hum_depth: f32,
    /// Fraction of fixtures that are *failing* tubes — stochastic dropouts / strobe instead of a steady
    /// hum (the classic Backrooms dying-fluorescent). Cosmetic; the gameplay field is unaffected.
    pub flicker_fail_ratio: f32,
    /// Hard cap on how many fixtures in the same room may be `failing` at once, regardless of how many
    /// independently roll under `flicker_fail_ratio`. Without this, a dense room (a high `wall_lights_
    /// per_room` roll, or a naturally notchy/small room with many wall runs) can have several tubes
    /// strobing at once by sheer chance — the intended "occasional dying tube" ambience reads as chaos
    /// instead. `1` is the classic single-dying-fluorescent look; `0` disables failing tubes entirely for
    /// a calmer room without touching `flicker_fail_ratio`'s per-fixture odds. A fixture with no room
    /// association (nothing outside `placement::furnish` currently emits one) is unaffected — it always
    /// gets its own independent roll, as if it were alone in a room of one.
    pub flicker_max_failing_per_room: usize,

    // --- The Researcher's flashlight (a moving directional emitter in the LightField) ---
    /// **Gameplay** peak illuminance the flashlight adds at the Researcher's own cell, in the field's own
    /// units (same scale as [`field_intensity`]). Falls linearly to 0 at `flashlight_range`. This is what
    /// repels photophobic creatures — tune against `photophobic_gain`.
    pub flashlight_intensity: f32,
    /// Beam reach in dungeon cells (the cone's radial cut-off, wall-occluded like a fixture).
    pub flashlight_range: f32,
    /// Cosine of the beam's half-angle (the wedge width). `cos(35°) ≈ 0.819` is a tight torch; lower =
    /// wider. Cells whose direction from the source dots `forward` above this are inside the beam.
    pub flashlight_cone_cos: f32,
    /// Soft-edge ramp width, in cosine units past `flashlight_cone_cos`, over which the cone fades 0→1.
    /// Keeps the illuminance gradient smooth at the rim so creature steering doesn't hit a cliff.
    pub flashlight_edge_softness: f32,
    /// Cosmetic (windowed-only) real `SpotLight` on the flashlight model — luminous power (lumens).
    pub flashlight_spot_intensity: f32,
    /// Cosmetic spot light reach (metres).
    pub flashlight_spot_range: f32,
    /// Cosmetic spot light colour (sRGB triple) — a warm torch beam.
    pub flashlight_spot_color: [f32; 3],
    /// Cosmetic spot light outer cone half-angle (radians) — the visible beam spread.
    pub flashlight_spot_outer_angle: f32,

    // --- Image-based ambient + HDR display (see `crate::world`) ---
    /// Intensity (cd/m²) of the generated environment map that supplies **normal-aware** ambient.
    /// This replaced a flat [`ambient_brightness`] of 500→200: a uniform `GlobalAmbientLight` adds the
    /// same term to every surface regardless of which way it faces, so nothing acquires form from the
    /// fill and everything reads as clay. An irradiance environment map does depend on the normal, and
    /// Ramamoorthi & Hanrahan 2001 (`10.1145/383259.383317`) is why a tiny one suffices: irradiance from
    /// *any* environment is captured almost entirely by a low-order spherical-harmonic expansion, so a
    /// 64² gradient cubemap carries essentially the same ambient signal as a full HDRI would.
    pub env_brightness: f32,
    /// Upward (+Y) end of the environment gradient, sRGB — the low-CRI fluorescent ceiling. Warm-neutral
    /// by the palette rule (`docs/lore/2026-07-12-scp-color-language.md` §6: "Desaturation = reality").
    pub env_sky_color: [f32; 3],
    /// Downward (−Y) end of the environment gradient, sRGB — bounce off dark carpet/concrete. Darker and
    /// less warm than the ceiling; the *difference* between the two ends is what shades a normal.
    pub env_ground_color: [f32; 3],
    /// Bloom strength on the HDR camera. The whole emissive layer (fixtures, TV static, laser bolts,
    /// mycelia glow) was tuned *down* to survive an LDR camera that clipped anything above mid-grey;
    /// this is what those values are now free to exceed.
    pub bloom_intensity: f32,
    /// Far edge of the directional key's shadow cascades, in metres. Interior scale — the Bevy default is
    /// built for outdoor draw distances and would spend the whole shadow map on empty space.
    pub shadow_max_distance: f32,
    /// Depth of the first (sharpest) cascade, in metres. Roughly one large room, so the rooms the player
    /// is actually looking at get the texel density.
    pub shadow_first_cascade: f32,
}

/// The evolvable **gameplay** subset of [`LightingConfig`], as one value — so the world search can
/// co-evolve the light the ecosystem steers on instead of holding it frozen while it evolves the mold's
/// *response* to it (`mold.light_recoil` / `mold.dim_light`). `Copy` + `Serialize` so an evolved world
/// decodes to a readable RON diff (the reward-hacking guard). Mirrors [`crate::almond_water::
/// AlmondWaterDynamics`], which is the established pattern for this.
///
/// **Only knobs that are both gameplay-affecting and non-visual are here**, and that is a short list:
///
/// - [`LightingConfig::field_intensity`] — the gameplay illuminance baked into [`LightField`] on
///   `FixedUpdate`. Deliberately separate from the render `fixture_intensity` (lumens), so evolving it
///   cannot change how the game looks.
/// - [`LightingConfig::photophobic_gain`] — the push a [`Photophobic`] creature takes down the light
///   gradient. Pure steering; nothing renders it. `tests/replay.rs`'s `photophobia_pulls_crabs_into_shadow`
///   pins that it moves the trajectory.
///
/// **Deliberately excluded, each for a measured reason:**
///
/// - `fixture_range` and the `flashlight_*` knobs feed the FixedUpdate field *and* the renderer. Evolving
///   them would let the search restyle the game's look, which is an authored decision, not a search's.
/// - `photophilic_gain` is a **no-op in rollouts**: its only reader (`crab_locomotion`) is gated on a
///   `Photophilic` component no crab carries — `config.ron` calls it a "toolkit; no carrier yet". The only
///   inserter is windowed-only.
/// - `mushroom_light_size_*` are read by `mycelia::grow_fruit_bodies`, which runs on `Update` inside the
///   windowed-only `MyceliaPlugin` and never reaches the harness; they scale a mesh with no `Health`, so
///   they are not even in `snapshot_hash`.
/// - `ambient_*` / `key_illuminance` / `fixture_intensity` / `fixture_*` / `flicker_*` are cosmetic.
///
/// A knob that cannot move fitness must not be in the genome: it spends a search dimension and an RNG draw
/// per mutation to buy nothing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LightingDynamics {
    pub field_intensity: f32,
    pub photophobic_gain: f32,
}

/// MUST match the `lighting:` gameplay knobs in `assets/config/config.ron`.
///
/// **Never put comments inside `fn default()`.** `train apply --dim world` rewrites that body verbatim from
/// the baked elite (`regen_default` in `src/bin/train.rs` brace-matches and replaces the whole body), so
/// anything in there is deleted by the first bake. Document above this impl instead.
impl Default for LightingDynamics {
    fn default() -> Self {
        Self { field_intensity: 1.0, photophobic_gain: 6.0 }
    }
}

impl LightingDynamics {
    /// Read the evolvable slice out of a full config.
    pub fn from_config(c: &LightingConfig) -> Self {
        Self { field_intensity: c.field_intensity, photophobic_gain: c.photophobic_gain }
    }

    /// Overwrite the evolvable gameplay knobs of a full config, leaving the visual knobs untouched.
    pub fn apply_to(&self, c: &mut LightingConfig) {
        c.field_intensity = self.field_intensity;
        c.photophobic_gain = self.photophobic_gain;
    }
}

/// Loud, one-path validation (mirrors `config::validate_density` and the other `validate_*` checks).
pub fn validate_config(c: &LightingConfig) -> Result<(), String> {
    for (name, v) in [
        ("ambient_brightness", c.ambient_brightness),
        ("key_illuminance", c.key_illuminance),
        ("fixture_intensity", c.fixture_intensity),
        ("fixture_emissive", c.fixture_emissive),
        ("field_intensity", c.field_intensity),
        ("photophobic_gain", c.photophobic_gain),
        ("photophilic_gain", c.photophilic_gain),
        ("mushroom_light_size_bonus", c.mushroom_light_size_bonus),
        ("mushroom_light_size_rate", c.mushroom_light_size_rate),
        ("flicker_hum_depth", c.flicker_hum_depth),
        ("flicker_fail_ratio", c.flicker_fail_ratio),
        ("flashlight_intensity", c.flashlight_intensity),
        ("flashlight_edge_softness", c.flashlight_edge_softness),
        ("flashlight_spot_intensity", c.flashlight_spot_intensity),
        ("flashlight_spot_outer_angle", c.flashlight_spot_outer_angle),
        ("env_brightness", c.env_brightness),
        ("bloom_intensity", c.bloom_intensity),
    ] {
        if !(v.is_finite() && v >= 0.0) {
            return Err(format!("lighting.{name} must be finite and >= 0 (got {v})"));
        }
    }
    // A typo guard, same spirit as `config::validate_density`'s `MAX_PER_ROOM`: any real room in this
    // game never exceeds `WALL_LIGHTS`'s evolved ceiling (`squad_ai::level_genome`, 16), so a configured
    // cap above that is certainly a mistake, not an intentional "never limit" — `usize::MAX` would silently
    // reproduce the exact pile-up bug this knob exists to prevent.
    const MAX_FAILING_PER_ROOM: usize = 16;
    if c.flicker_max_failing_per_room > MAX_FAILING_PER_ROOM {
        return Err(format!(
            "lighting.flicker_max_failing_per_room = {} exceeds the {MAX_FAILING_PER_ROOM} ceiling",
            c.flicker_max_failing_per_room
        ));
    }
    if !(c.fixture_range.is_finite() && c.fixture_range > 0.0) {
        return Err(format!("lighting.fixture_range must be finite and > 0 (got {})", c.fixture_range));
    }
    if !(c.flashlight_range.is_finite() && c.flashlight_range > 0.0) {
        return Err(format!("lighting.flashlight_range must be finite and > 0 (got {})", c.flashlight_range));
    }
    if !(c.flashlight_spot_range.is_finite() && c.flashlight_spot_range > 0.0) {
        return Err(format!(
            "lighting.flashlight_spot_range must be finite and > 0 (got {})",
            c.flashlight_spot_range
        ));
    }
    // A cosine must be in [-1, 1]; outside that the beam is either everything or nothing (a config typo).
    if !(c.flashlight_cone_cos.is_finite() && (-1.0..=1.0).contains(&c.flashlight_cone_cos)) {
        return Err(format!(
            "lighting.flashlight_cone_cos must be a cosine in [-1, 1] (got {})",
            c.flashlight_cone_cos
        ));
    }
    for (name, col) in [
        ("ambient_color", c.ambient_color),
        ("fixture_color", c.fixture_color),
        ("flashlight_spot_color", c.flashlight_spot_color),
        ("env_sky_color", c.env_sky_color),
        ("env_ground_color", c.env_ground_color),
    ] {
        if col.iter().any(|ch| !ch.is_finite() || *ch < 0.0) {
            return Err(format!("lighting.{name} channels must be finite and >= 0 (got {col:?})"));
        }
    }
    // Cascade splits: both positive, and the first cascade must sit inside the far edge. A first cascade
    // beyond `shadow_max_distance` is not a degraded look, it is an inverted range — reject it at the door
    // rather than let Bevy silently resolve it into a shadow map that covers nothing.
    if !(c.shadow_max_distance.is_finite() && c.shadow_max_distance > 0.0) {
        return Err(format!(
            "lighting.shadow_max_distance must be finite and > 0 (got {})",
            c.shadow_max_distance
        ));
    }
    if !(c.shadow_first_cascade.is_finite() && c.shadow_first_cascade > 0.0) {
        return Err(format!(
            "lighting.shadow_first_cascade must be finite and > 0 (got {})",
            c.shadow_first_cascade
        ));
    }
    if c.shadow_first_cascade > c.shadow_max_distance {
        return Err(format!(
            "lighting.shadow_first_cascade ({}) exceeds shadow_max_distance ({})",
            c.shadow_first_cascade, c.shadow_max_distance
        ));
    }
    Ok(())
}

/// Marker: a placed furniture piece that emits light — `affords("emit")`, i.e. ceiling tubes, wall
/// sconces, desk lamps, glowing screens (kit-agnostic, per `placement::manifest`). Tagged at
/// furniture-spawn time in [`crate::placement::furnish`], so it is present in the headless harness too
/// (inert there — only the windowed [`LightingPlugin`] consumes it). Its world `Transform` is the single
/// source of fixture position for BOTH the real point light below and the [`LightField`] bake (Phase 1).
#[derive(Component)]
pub struct LightEmitter;

// ---------------------------------------------------------------------------------------------------
// LightField — the queryable gameplay illuminance grid (Phase 1). Single source of truth for "how lit
// is this point", read by creature light-response (Phase 2) and mushroom growth (Phase 3).
// ---------------------------------------------------------------------------------------------------

/// System set for `bake_light_field`, the sole writer of [`LightField`]. Creature readers (Phase 2:
/// photophobic/-tropic/-philic steering) order themselves `.after(LightFieldWritten)` on `FixedUpdate`
/// so they read the current tick's field — mirroring `fog::LosWritten`.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LightFieldWritten;

/// A CPU-side scalar **illuminance grid over dungeon cells** — the gameplay light field. Row-major
/// `y*width + x` (the project-wide indexing), 0 in full dark. Baked from [`LightEmitter`] fixture
/// positions with linear radial falloff **and wall occlusion** (`Dungeon::line_of_sight`, so light never
/// leaks through walls), summed over fixtures. Its `sample`/`gradient` copy the shape of
/// `ai::field::Stig` so creature steering reuses that idiom (`FollowGradient` = `+`, `FleeGradient` = `-`).
///
/// **Its own resource, not a `Stig` channel:** light is *static, environmental, occlusion-shadowed*;
/// `Stig` channels are *dynamic, decaying, creature-emitted* pheromones. Folding light into the decaying
/// model would be a hidden second path (re-deposit every tick, or a zero-evaporation special case).
/// Semantically it belongs with the static habitat mask, at dungeon-cell resolution but with `Stig`'s
/// query interface. One path: one `LightField`.
///
/// Research: Greger et al., "The Irradiance Volume" (IEEE CG&A 1998) — a queryable spatial illumination
/// field for dynamic agents in static geometry; leak-suppression here is a cheap `line_of_sight` (cf.
/// DDGI's visibility moments, Majercik et al. JCGT 2019). A photophobic crab descending this field's
/// gradient does photophobic taxis (local descent of the illuminance gradient) — a light-avoidance
/// direction consistent with Nakagaki et al.'s Physarum photoavoidance (PRL 2007), but not their
/// minimum-risk routing (a global path integral between two fixed endpoints, not local gradient descent).
/// A thin **facade** over `bevy_light_grid::LightGrid`.
///
/// The grid and its two passes live in that crate, which works in CELL space and takes occlusion as a
/// closure. This type gives them the game's vocabulary back: it owns the `Dungeon` (world<->cell, the
/// floor set, `line_of_sight`) and keeps the exact method signatures the 18 modules reading `light`
/// already use, so the extraction moved no caller. Same argument as `ai::field`'s `Stig`.
///
/// **`dirty` stays here, not in the crate.** It is bake-gating shell state — "has a fixture changed
/// since the last bake" is a question about this game's fixtures, and a grid that tracked it would be
/// guessing at a schedule it does not own.
#[derive(Resource)]
pub struct LightField {
    core: bevy_light_grid::LightGrid,
    /// Recompute pending for the static base. True at startup (bake once fixtures exist) and whenever a
    /// fixture changes state, gated like `fog::FogGrid::dirty`. Does NOT gate the per-tick dynamic pass,
    /// which always runs — a moving light can never be dirty-gated.
    dirty: bool,
}

impl LightField {
    /// Empty field sized to the dungeon; starts `dirty` so the first `FixedUpdate` bakes the static base.
    pub fn new(dungeon: &Dungeon) -> Self {
        Self {
            core: bevy_light_grid::LightGrid::new(dungeon.width, dungeon.height, dungeon.floor_cells()),
            dirty: true,
        }
    }

    /// Point read at a world position (query). Off-grid reads as 0 — the same contract as `Stig::sample`.
    #[inline]
    pub fn sample(&self, dungeon: &Dungeon, pos: Vec3) -> f32 {
        self.core.sample_cell(dungeon.world_to_cell(pos))
    }

    /// World-XZ direction of *increasing* illuminance (central differences), magnitude ≈ the local slope.
    /// A photophobic creature steers along `-gradient` (toward the dark), a phototropic/-philic one
    /// along `+gradient`.
    #[inline]
    pub fn gradient(&self, dungeon: &Dungeon, pos: Vec3) -> Vec2 {
        self.core.gradient_cell(dungeon.world_to_cell(pos))
    }

    /// Peak illuminance from the last bake (0 before the first bake).
    pub fn peak(&self) -> f32 {
        self.core.peak()
    }

    /// Recompute every cell from the fixture list — the bake. Walls cast shadow via
    /// `Dungeon::line_of_sight`, handed to the crate as a closure.
    fn bake(&mut self, dungeon: &Dungeon, fixtures: &[(IVec2, f32, f32)]) {
        self.core.bake(fixtures, |a, b| dungeon.line_of_sight(a, b));
        self.dirty = false;
    }

    /// Recompose `cells = base + Σ dynamic cones`, then recompute `peak`. Runs EVERY tick, so a walking
    /// flashlight's beam sweeps live.
    fn compose(&mut self, dungeon: &Dungeon, cones: &[FlashlightCone]) {
        self.core.compose(cones, |a, b| dungeon.line_of_sight(a, b));
    }

    /// Mark the field for recompute (a fixture switched on/off/failing).
    #[allow(dead_code)]
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Attenuate the composed illuminance by the gameplay mold: a moldy cell (`biomass` toward 1)
    /// darkens toward `1 - dim_light`. Called each tick AFTER `compose` (so it never accumulates), inside
    /// the `LightFieldWritten` set so every light reader sees the darkened field. This is the mold→light
    /// half of the mold↔light feedback loop: mold self-shades, and the squad's flashlight — strong enough
    /// to overpower the dimming — pushes it back (the recoil half lives in `mold::mold_update`).
    pub fn apply_mold_dim(&mut self, biomass: &[f32], dim_light: f32) {
        self.core.apply_mold_dim(biomass, dim_light);
    }

    /// The composed grid, for the tests below that compare two bakes cell-for-cell.
    #[cfg(test)]
    pub fn cells(&self) -> &[f32] {
        self.core.cells()
    }

    /// FNV-1a-fold every **composed** cell's bit pattern into `hash` — the determinism oracle for the
    /// whole field, mirroring `Stig::fold_fingerprint`.
    ///
    /// **Folds `cells` (base + cones), not just `base`.** This once folded `base` alone, because the
    /// cone's beam direction was derived from the unit's slerped `Transform.rotation` — glam quaternion
    /// transcendentals that are NOT bit-identical across architectures — so an ARM-pinned cone-inclusive
    /// golden failed on x86 CI (issue #46). Now `apply_dynamic_lights` builds the cone `forward` from
    /// deterministic gameplay state (FacingOverride/AimTarget/velocity) with arch-stable ops (subtract +
    /// `normalize_or`), never from `rotation`, so `cells` is a cross-arch-stable oracle again.
    #[cfg(feature = "test-harness")]
    pub fn fold_fingerprint(&self, hash: &mut u64) {
        for &v in self.core.cells() {
            for &b in &v.to_bits().to_le_bytes() {
                *hash ^= b as u64;
                *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
}

/// One moving directional light contributed to the [`LightField`] each tick — the Researcher's
/// flashlight. `source` is its dungeon cell, `forward` the world-XZ beam direction (unit length), the
/// rest the beam's reach/brightness/shape (see [`LightingConfig`]). Sorted by `source` before compose
/// for determinism. Defined in `bevy_light_grid`.
pub use bevy_light_grid::FlashlightCone;

/// Bake the STATIC base when dirty: collect [`LightEmitter`] fixture cells (stable-sorted for a
/// deterministic float sum), then recompute [`LightField::base`]. Runs on `FixedUpdate` in
/// [`LightFieldWritten`], **chained before** [`apply_dynamic_lights`]. Uses fixture `Transform`
/// (world-space at spawn — furniture never moves), not `GlobalTransform`, to avoid propagation-timing on
/// the first tick. If no fixtures exist yet (spawn not flushed) it stays dirty and retries next tick.
fn bake_light_field(
    mut field: ResMut<LightField>,
    dungeon: Res<Dungeon>,
    config: Res<GameConfig>,
    fixtures: Query<&Transform, With<LightEmitter>>,
) {
    if !field.dirty {
        return;
    }
    let intensity = config.lighting.field_intensity;
    let range = config.lighting.fixture_range;
    let mut fx: Vec<(IVec2, f32, f32)> = fixtures
        .iter()
        .map(|t| (dungeon.world_to_cell(t.translation), intensity, range))
        .collect();
    if fx.is_empty() {
        return; // fixtures not spawned yet — stay dirty, retry next tick
    }
    // Stable order so the per-cell float summation in `bake` is reproducible across runs/threads.
    //
    // Sorted by the WHOLE value, not just the cell. Keying on `(c.x, c.y)` alone was a prefix of the value:
    // two fixtures in one cell with different intensity/range tied, and `sort_unstable` then ordered them by
    // the ECS query order this sort exists to erase — feeding `bake`'s non-associative per-cell sum in a
    // run-dependent order. With the full value in the key a tie means the entries are IDENTICAL, hence
    // interchangeable, which is exactly the claim `sort_value_canonical` makes.
    crate::util::sort_value_canonical(&mut fx, |(c, i, r)| (c.x, c.y, i.to_bits(), r.to_bits()));
    field.bake(&dungeon, &fx);
}

/// Recompose the field every tick: `cells = base + Σ flashlight cones`. The Researcher (the "Scientist")
/// carries the only moving light — its beam points at the same target `unit_facing` turns the body toward
/// (the `Mode::Ward` aim via `FacingOverride`, else `AimTarget`, else its travel direction), so the AI's
/// warding aim is exactly what steers the beam. Photophobic crabs/mancas already flee this field's
/// gradient, so the cone repels them with no per-creature code. Runs in [`LightFieldWritten`], chained
/// AFTER [`bake_light_field`] and ordered AFTER `squad::unit_facing`, in BOTH the windowed game and the
/// headless harness (the field is hashed).
/// **Determinism:** cones are sorted by source cell before compose (the `bake` float-sum discipline); the
/// beam `forward` is built from the unit's deterministic gameplay state with arch-stable ops, never from
/// the slerped `Transform.rotation` (see [`LightField::fold_fingerprint`]). Ref: Björk & Michelsen, FDG 2014.
pub(crate) fn apply_dynamic_lights(
    mut field: ResMut<LightField>,
    dungeon: Res<Dungeon>,
    config: Res<GameConfig>,
    researchers: Query<
        (
            &Transform,
            &crate::squad::Velocity,
            &crate::squad::AimTarget,
            &crate::squad::FacingOverride,
            &crate::squad_ai::role::RoleId,
        ),
        With<crate::squad::Unit>,
    >,
    mut cones: Local<Vec<FlashlightCone>>,
) {
    // Profiling span: read the per-system cost under `--features bevy/trace_tracy` (see `perf_hud`). Inspection
    // shows this recompose is a cheap `copy_from_slice` + max-scan plus a small (≈1-cone) scatter — deliberately
    // left serial; this span lets that be confirmed rather than assumed.
    let _span = info_span!("light_recompose").entered();
    let c = &config.lighting;
    // Reused buffer (`Local`), cleared each tick, instead of a fresh `Vec` — usually 0-1 researchers, but
    // this runs every `FixedUpdate` tick on the deterministic path, so a per-tick heap alloc is pure waste.
    cones.clear();
    cones.extend(researchers
        .iter()
        .filter(|(.., role)| **role == crate::squad_ai::role::RoleId::Researcher)
        .map(|(t, velocity, aim, facing_override, _)| {
            // Beam direction = the SAME target `squad::unit_facing` turns the body toward, but built here
            // with arch-stable ops (subtract + `normalize_or` = mul/add/sqrt/div) instead of reading the
            // rendered `Transform.rotation`. That rotation is accumulated through `looking_at`/`slerp`
            // (acos/sin), which are NOT bit-identical across architectures — reading it leaked that
            // divergence into the hashed positions of the photophobic crabs/mancae this cone steers (the
            // same hazard #46 fixed for the field oracle, still live for the actor hash). The visible
            // `SpotLight` is a child of the unit and still follows the smooth slerped rotation, so only this
            // CPU gameplay cone snaps to the target. Precedence mirrors `unit_facing`: FacingOverride (the
            // warding aim) → AimTarget → travel direction → world -Z.
            let target = facing_override
                .0
                .or(aim.0)
                .map(|p| Vec3::new(p.x, t.translation.y, p.z))
                .or_else(|| {
                    let v = Vec3::new(velocity.0.x, 0.0, velocity.0.y);
                    (v.length_squared() > 1.0e-6).then_some(t.translation + v)
                });
            let forward = target
                .map(|tg| Vec2::new(tg.x - t.translation.x, tg.z - t.translation.z))
                .unwrap_or(Vec2::new(0.0, -1.0))
                .normalize_or(Vec2::new(0.0, -1.0));
            FlashlightCone {
                source: dungeon.world_to_cell(t.translation),
                forward,
                intensity: c.flashlight_intensity,
                range: c.flashlight_range,
                cone_cos: c.flashlight_cone_cos,
                edge_softness: c.flashlight_edge_softness,
            }
        }));
    // Stable order so the per-cell float summation in `compose` is reproducible across runs/threads.
    //
    // Keyed on the WHOLE cone, not just `source`. `(source.x, source.y)` was a PREFIX of the value: two
    // flashlights in one cell with different dir/range/cone tied, and `sort_unstable` then ordered them by
    // ECS query order — which `compose`'s non-associative per-cell sum then folds. Full value in the key ⇒
    // a tie means the cones are identical ⇒ interchangeable.
    crate::util::sort_value_canonical(&mut cones, |k| {
        (
            k.source.x,
            k.source.y,
            k.forward.x.to_bits(),
            k.forward.y.to_bits(),
            k.intensity.to_bits(),
            k.range.to_bits(),
            k.cone_cos.to_bits(),
            k.edge_softness.to_bits(),
        )
    });
    field.compose(&dungeon, &cones);
}

/// Owns the gameplay [`LightField`]. Registered in BOTH the windowed game and the headless harness
/// (unlike [`LightingPlugin`]) because the field is CPU gameplay state creature AI reads — so the
/// deterministic replay gate must cover its bake. Requires `Dungeon` at build (DungeonPlugin precedes it).
/// Size this run's illuminance grid to its dungeon.
fn size_light_field(mut commands: Commands, dungeon: Res<Dungeon>) {
    commands.insert_resource(LightField::new(&dungeon));
}

pub struct LightFieldPlugin;

impl Plugin for LightFieldPlugin {
    fn build(&self, app: &mut App) {
        // Sized per run — see the note in `fog::FogPlugin` (FVS-A-5).
        app.add_systems(
            OnEnter(crate::session::RunState::Active),
            size_light_field.in_set(crate::session::RunBuild::Grids),
        )
        .add_systems(
            FixedUpdate,
            // Static base first, then the moving cones layered on top — one field, one query interface.
            // Ordered AFTER `squad::unit_facing` so the cone reads settled, current-tick unit facing/position:
            // both `apply_dynamic_lights` (shared `&Transform`) and `unit_facing` (`&mut Transform`) touch the
            // `Unit` archetype, and without this the mut-vs-shared conflict was resolved by an unspecified
            // Bevy tie-break — leaving the actor golden implicitly pinned to schedule insertion order (D2).
            (bake_light_field, apply_dynamic_lights)
                .chain()
                .in_set(LightFieldWritten)
                .after(crate::squad::unit_facing).distributive_run_if(in_state(crate::session::RunState::Active)),
            );
    }
}

/// Idempotency guard: set once a [`LightEmitter`] has been given its real point-light child, so
/// `attach_fixture_lights` never double-lights a fixture as furniture streams in on room reveal.
#[derive(Component)]
struct FixtureLit;

/// Idempotency guard for `glow_fixtures`: set once the fixture's GLB mesh materials have been made
/// emissive. Separate from [`FixtureLit`] because the glow needs the async GLB scene to have *loaded*
/// its mesh descendants, whereas the point-light child does not — so the two run at different times.
#[derive(Component)]
struct FixtureGlowing;

/// Stylised mains-hum shimmer rate. A real ballast flickers at ~100–120 Hz — invisible at 60 fps — so
/// this is a slower, perceptible shimmer for effect.
const FLICKER_HUM_HZ: f32 = 7.0;

/// A decorrelated flicker phase in `[0, τ)` derived from `seed`, for every emitter that shimmers.
///
/// **Wrapping into one turn is not tidiness — it is the difference between a shimmer and a step
/// function.** The previous form, `seed as f32 * 2.399_963` (the golden angle in radians), lands around
/// 1e9–1e10 for a real seed, and f32's ULP up there is 128 radians or more — some twenty whole sine
/// periods. `(t * hz + phase)` is then *bit-identical* for seconds of wall-clock at a time and, when it
/// finally ticks, jumps to a value uncorrelated with the one before: measured for a fixture at
/// (12.5, 2.7, 30.25), phase = 1.58e9 and `t * 7.0 + phase` does not move until t ≈ 9 s. So a fluorescent
/// held one fixed brightness for ~18 s at a stretch, and a failing tube latched into its near-off branch
/// for 20 s+ instead of strobing — the flicker was quantized out of existence by the phase's own
/// magnitude. Perceptually this is the whole ballgame: flicker visibility is a function of temporal
/// frequency against the spatio-temporal CSF, and a ~0.05 Hz staircase is not the ~7 Hz shimmer intended.
///
/// The golden-angle rotation only ever needed the FRACTIONAL turn, so take it in u32 space
/// (`0x9E37_79B9` ≈ 2³²·φ⁻¹ — exact, no rounding) and convert once, into a range where f32's ULP is ~1e-7.
fn flicker_phase(seed: u32) -> f32 {
    seed.wrapping_mul(0x9E37_79B9) as f32 / u32::MAX as f32 * std::f32::consts::TAU
}

/// Per-fixture flicker state, carried on the real point-light child (cosmetic, windowed-only).
/// `base_intensity` is the unflickered lumens; `phase` decorrelates the hum so tubes don't shimmer in
/// lockstep; `failing` tubes drop out / strobe like dying Backrooms fluorescents.
#[derive(Component)]
struct FixtureLight {
    base_intensity: f32,
    phase: f32,
    failing: bool,
}

/// Marker for a screen prop (a TV) — inserted at spawn by `placement::furnish` when the manifest item
/// `affords("screen")`. The windowed `attach_screen_lights` gives it an eery cool-cyan flickering LOS
/// spotlight *instead* of the generic fixture light, so a TV reads as a dead-channel CRT glowing into the
/// room rather than a lamp. Cosmetic only: the TV also keeps `LightEmitter` (via `affords("emit")`), so
/// the gameplay `LightField` is unchanged and this marker never touches the deterministic sim.
#[derive(Component)]
pub struct ScreenEmitter;

/// Idempotency guard: set once a [`ScreenEmitter`] has been given its cosmetic screen spotlight.
#[derive(Component)]
struct ScreenLit;

/// Idempotency guard: set once a [`ScreenEmitter`]'s CRT face mesh has been swapped to an emissive
/// material (the self-lit "the screen is on" cue), so `glow_screens` runs once per TV.
#[derive(Component)]
struct ScreenGlowing;

/// Flicker state for a TV's screen spotlight (cosmetic, windowed-only). `base_intensity` is the
/// un-flickered lumens; `phase` decorrelates one TV from another.
#[derive(Component)]
struct ScreenLight {
    base_intensity: f32,
    phase: f32,
}

/// Eery CRT screen glow — a cool cyan, the classic dead-channel cast. Held as a code constant (a
/// cosmetic rendering-fit value, like [`FLICKER_HUM_HZ`]); the gameplay light stays config-driven.
const SCREEN_COLOR: [f32; 3] = [0.40, 0.78, 0.92];
/// Spotlight lumens for the screen cast — enough to throw a visible cool wash onto the dresser/floor in
/// front of the TV (a shadowless 55k cast read too faintly in an already-lit room), still dimmer than a
/// room fixture so it broods rather than lights.
const SCREEN_INTENSITY: f32 = 90_000.0;
/// How far the screen glow spills into the room (metres).
const SCREEN_RANGE: f32 = 6.5;
/// Overall glow/brightness multiplier for the animated CRT-static face (the `a` of the material tint).
/// The material is unlit, so its output IS the emitted colour — a modest multiplier on the cool snow
/// reads as a bright, self-lit dead-channel screen. The spotlight above supplies the actual cast light.
const SCREEN_EMISSIVE: f32 = 2.6;

/// GPU uniform for [`TvStaticMaterial`] — mirrors the `TvStatic` struct in `tv_static.wgsl`.
#[derive(Clone, ShaderType)]
struct TvStaticUniform {
    /// rgb = cool CRT tint applied to the snow; a = overall glow multiplier.
    tint: Vec4,
}

/// The animated CRT "dead channel" static material for a TV screen mesh (see `tv_static.wgsl`). Unlit —
/// the fragment output is emitted directly, so the screen self-glows — and driven by `globals.time`, so
/// one shared instance animates every TV. Windowed-only (registered in [`LightingPlugin`], never the
/// headless harness). Replaces the flat teal screen material so the TV shows moving snow, scanlines, and
/// vertical roll like a real untuned set (player request).
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct TvStaticMaterial {
    #[uniform(0)]
    settings: TvStaticUniform,
}

impl Material for TvStaticMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/tv_static.wgsl".into()
    }
}

impl Default for TvStaticMaterial {
    fn default() -> Self {
        TvStaticMaterial {
            settings: TvStaticUniform {
                tint: Vec4::new(SCREEN_COLOR[0], SCREEN_COLOR[1], SCREEN_COLOR[2], SCREEN_EMISSIVE),
            },
        }
    }
}
/// Cone half-angle of the screen cast (radians) — wide, so the glow washes the near floor/wall.
const SCREEN_OUTER_ANGLE: f32 = 0.75;
/// Base flicker rate of the screen (Hz) — faster and less regular than a fluorescent hum: a CRT's
/// restless scan roll.
const SCREEN_FLICKER_HZ: f32 = 11.0;

// ---------------------------------------------------------------------------------------------------
// Light-response markers — the composable toolkit. Any creature can carry one to gain emergent behaviour
// around light and its absence; the generic `light_push` (below) reads the shared LightField gradient.
// The photophobic/-philic duality is the FleeGradient/FollowGradient pair from `ai::field`, for light.
// Research: crustacean noxious-stimulus avoidance (Cano et al. 2011); the light-avoidance direction is
// photophobic/photophilic taxis (down/up the illuminance gradient), consistent with Nakagaki et al.'s
// Physarum photoavoidance (PRL 2007) — but NOT their minimum-risk routing (a global path integral over
// two fixed endpoints, which this local gradient step is not).
// ---------------------------------------------------------------------------------------------------

/// The light-response toolkit, defined in `bevy_light_grid` and re-exported at the paths every
/// consumer already uses.
///
/// * `Photophobic` — steers **down** the gradient (toward the dark), strength
///   `lighting.photophobic_gain`. Carried by crabs, which pool in shadow and cede the lit rooms.
/// * `Photophilic` — steers **up** it, strength `lighting.photophilic_gain`. The ready toolkit
///   component for a light-seeking creature; the same push, opposite sign.
/// * `Phototropic` — grows or orients toward light, a *tropism* rather than steering. Carried by
///   mushroom fruit bodies, where light both enlarges the cap and leans it toward the brightest
///   neighbour; its consumer lives in `mycelia::fruit`.
pub use bevy_light_grid::{Photophilic, Photophobic, Phototropic};

/// World-XZ steering push a light-response creature feels at `pos`: `signed_gain · ∇illuminance`.
///
/// A photophobic creature passes `-photophobic_gain` (descends toward the dark), a photophilic one
/// `+photophilic_gain` (climbs toward the light). Zero where the field is flat (deep dark, or the middle
/// of a uniform pool), so a creature far from any light gradient is unbiased — the graceful "no cost off
/// in the dark" property. Pure: the caller projects the result onto the locomotion surface and scales by
/// `dt` (see `crab::crab_locomotion`). This wrapper is what supplies the world→cell conversion.
pub fn light_push(field: &LightField, dungeon: &Dungeon, pos: Vec3, signed_gain: f32) -> Vec3 {
    bevy_light_grid::light_push_at(&field.core, dungeon.world_to_cell(pos), signed_gain)
}

/// The next rendered scale for a [`Phototropic`] fruit body easing toward its light-scaled target size
/// `base·(1 + bonus·light01)`, approached from `current` by at most `max_step` this tick — rate-limited
/// so the enlargement stays sub-perceptual (the mold's speed-limit ethos). `light01` is the illuminance
/// normalised to the field peak. Photomorphogenesis — fungal fruiting is light-gated (Zhang et al.,
/// PLoS ONE 10:e0123025, 2015).
pub use bevy_light_grid::phototropic_scale;

/// Windowed-game lighting: real fixture lights. **Never** registered in the headless harness
/// (GPU/cosmetic only — the deterministic core must not depend on it).
///
/// No camera screen-space FX component is attached. Bevy's `ContactShadows` used to be inserted here and
/// was pure cost: the raymarch is gated per-light on `contact_shadows_enabled`, which defaults `false` and
/// is set nowhere in this project (`bevy_pbr::render::light` checks it before emitting any contact-shadow
/// work), so no ray was ever marched — while the component's `#[require]`d depth prepass ran every frame
/// and quietly falsified `MoldFruitExt`'s documented "no camera carries a `DepthPrepass`/`NormalPrepass`"
/// invariant. Enabling contact shadows for real is a per-light opt-in and a look change, so it is the
/// user's call, not a silent side effect of a camera component.
pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<TvStaticMaterial>::default());
        app.add_systems(
            Update,
            (
                attach_fixture_lights,
                glow_fixtures,
                flicker_lights,
                attach_flashlight_spots,
                attach_screen_lights,
                glow_screens,
                flicker_screens,
            ).distributive_run_if(in_state(crate::session::RunState::Active)),
            );
    }
}

/// Idempotency guard: set on a [`crate::squad::FlashlightModel`] once its cosmetic `SpotLight` has been
/// attached, so `attach_flashlight_spots` lights each Researcher's flashlight exactly once.
#[derive(Component)]
struct FlashlightLit;

/// Give the Researcher's flashlight a real (windowed-only) [`SpotLight`] so the beam is visible — the
/// **cosmetic** counterpart to the gameplay [`LightField`] cone in [`apply_dynamic_lights`]. The spot is a
/// child of the **unit** (the flashlight model's parent), not the model, so it points straight down the
/// unit's forward (`−Z`, Bevy's spot axis) regardless of how the model is cosmetically pitched in the hand
/// — the same forward the gameplay cone uses, so glow and gameplay agree. First `SpotLight` in the
/// codebase; shadowless like the fixture point lights (clustered, cheap). Runs in [`LightingPlugin`],
/// never the headless harness.
fn attach_flashlight_spots(
    mut commands: Commands,
    config: Res<GameConfig>,
    flashlights: Query<(Entity, &ChildOf), (With<crate::squad::FlashlightModel>, Without<FlashlightLit>)>,
) {
    let c = &config.lighting;
    let color =
        Color::srgb(c.flashlight_spot_color[0], c.flashlight_spot_color[1], c.flashlight_spot_color[2]);
    for (model, child_of) in &flashlights {
        // Mark the model (not the unit) so the guard is one-per-flashlight; spawn the light on the unit.
        commands.entity(model).insert(FlashlightLit);
        commands.entity(child_of.parent()).with_child((
            SpotLight {
                color,
                intensity: c.flashlight_spot_intensity,
                range: c.flashlight_spot_range,
                outer_angle: c.flashlight_spot_outer_angle,
                inner_angle: c.flashlight_spot_outer_angle * 0.6, // soft-edged cone
                shadow_maps_enabled: false,
                ..default()
            },
            // Chest height, slightly ahead of the body; identity rotation ⇒ beams along the unit's −Z
            // forward (the direction `unit_facing` turns to, hence where the gameplay cone points).
            Transform::from_xyz(0.15, 0.35, -0.3),
        ));
    }
}

/// Give a TV (`ScreenEmitter`) its cosmetic eery glow: a cool-cyan `SpotLight` cast forward out of the
/// screen, wall-occluded by range (the same shadowless "LOS by placement" idiom as the flashlight and
/// fixtures). The spot is a child of the TV, so it inherits the TV's yaw (set by the scatter pass to face
/// the room) and the fog-reveal `Visibility`. Windowed-only (`LightingPlugin`), never in the harness, and
/// the TV keeps its `LightEmitter` for the gameplay field — so this is determinism-neutral.
fn attach_screen_lights(
    mut commands: Commands,
    screens: Query<Entity, (With<ScreenEmitter>, Without<ScreenLit>)>,
) {
    let color = Color::srgb(SCREEN_COLOR[0], SCREEN_COLOR[1], SCREEN_COLOR[2]);
    for e in &screens {
        // Golden-angle decorrelation between TVs, wrapped into [0, τ) — see `flicker_phase` for why the
        // unwrapped form froze this shimmer into a multi-second staircase.
        let phase = flicker_phase(e.to_bits() as u32);
        commands.entity(e).insert(ScreenLit).with_child((
            SpotLight {
                color,
                intensity: SCREEN_INTENSITY,
                range: SCREEN_RANGE,
                outer_angle: SCREEN_OUTER_ANGLE,
                inner_angle: SCREEN_OUTER_ANGLE * 0.5, // soft, diffuse screen wash
                // A real TV throws shadows of whatever stands in front of it (player request). Unlike the
                // shadowless room fixtures, this one spotlight casts — one shadow map per TV, affordable
                // since TVs are rare (one per living room with a media surface).
                shadow_maps_enabled: true,
                // `shadow_normal_bias` is deliberately left at Bevy's default. It was briefly raised to 3.0
                // against the player-reported artifacts (`debug_screenshots/region_2026-07-25_16-53-51-436`,
                // "Wtf are these shadow artifacts?"); that could not have worked, and the arithmetic says
                // why. Depth/normal bias is the remedy for *self-shadowing acne* — an error along the light
                // ray (Williams 1978, "Casting curved shadows on curved surfaces"). A jagged, staircased
                // shadow edge is the orthogonal error: LATERAL quantization of the shadow map's texel grid,
                // which no offset along Z can resample away (Scherzer et al. 2011, "A Survey of Real-Time
                // Hard Shadow Mapping Methods", §3.1 separates the two). Measured for this spot:
                // texel_size = 2·tan(outer_angle)/2048 = 9.10e-4, so the world offset moves from 0.0151 to
                // 0.0251 units at the 6.5 range — far too small to have been fixing anything visible, and
                // equally far from Peter-Panning. If the staircase needs to go, the levers are the shadow
                // map resolution and the filter (`ShadowFilteringMethod`), not this number.
                ..default()
            },
            // At screen height, a touch in front of the face. The furniture forward convention is +Z
            // (yaw = atan2(sin, cos)) while Bevy's spot axis is −Z, so a PI yaw-flip beams the glow out of
            // the screen into the room. (If a kit's TV model faces the other way, flip this PI.)
            Transform::from_xyz(0.0, 0.42, 0.18)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
            ScreenLight { base_intensity: SCREEN_INTENSITY, phase },
        ));
    }
}

/// A restless CRT flicker on each TV's screen spotlight — a faster, less regular shimmer than the
/// fluorescent hum, with a slow roll beneath it (a dead channel breathing). **Cosmetic and windowed:**
/// modulates only the rendered `SpotLight` intensity, never the gameplay `LightField`. Runs on `Update`.
fn flicker_screens(time: Res<Time>, mut lights: Query<(&ScreenLight, &mut SpotLight)>) {
    let t = time.elapsed_secs();
    for (sl, mut light) in &mut lights {
        // A fast shimmer times a slow roll — the product gives the irregular scan-roll beat a lone sine
        // can't, with a floor so the screen never fully dies.
        let fast = 0.5 + 0.5 * (t * SCREEN_FLICKER_HZ + sl.phase).sin();
        let roll = 0.5 + 0.5 * (t * 2.7 + sl.phase * 1.7).sin();
        // ∈ [0.665, 0.865]: restless, but a swing half the old [0.62, 1.0] one. This is the *only* light
        // in the game that casts a real shadow map (see `attach_screen_lights`); a hard, unfiltered shadow
        // edge pulsing through a 38-point intensity swing every frame reads as the shadow itself
        // flickering, not just the room's brightness. Halving the swing keeps the restless-CRT character
        // without making its shadow the most visually loud thing in a room it's supposed to be ambient.
        //
        // The floor is 0.665, NOT 0.80, so that narrowing the swing does not silently BRIGHTEN the room.
        // `fast` and `roll` are independent half-rectified sines, each with mean 0.5, so E[fast·roll] =
        // 0.25 and the time-average multiplier is `floor + 0.25·swing`. The old pair averaged
        // 0.62 + 0.38·0.25 = 0.715; a 0.80 floor would average 0.85, a ~19% rise in mean output with
        // `SCREEN_INTENSITY` unchanged. That matters twice over: by the Talbot-Plateau law the perceived
        // brightness of a flickering source *is* its time-average luminance, so the eery cyan wash would
        // have got brighter, and by Ferry-Porter the critical flicker-fusion frequency rises with log
        // luminance — a brighter lamp makes the residual 11 Hz modulation MORE visible, the opposite of
        // the intent. 0.665 + 0.25·0.20 = 0.715 holds the mean exactly where it was.
        let mult = 0.665 + 0.20 * fast * roll;
        let next = sl.base_intensity * mult;
        if light.intensity != next {
            light.intensity = next;
        }
    }
}

/// Groups the per-room failing-tube cap in [`attach_fixture_lights`]: fixtures placed in the same room
/// share a bucket; a fixture with no [`PlacedIn`] (nothing outside `placement::furnish` currently emits
/// one, but the query allows it) gets its own `Solo` bucket keyed on its entity, so it's never capped
/// alongside an unrelated fixture that also happens to lack a room.
#[derive(Clone, PartialEq, Eq, Hash)]
enum FlickerRoom {
    Region(RegionId),
    Solo(Entity),
}

/// Give each newly-revealed [`LightEmitter`] a real clustered [`PointLight`] child so fixtures actually
/// cast light. The light is a **child**, so it inherits the fixture's fog-reveal `Visibility` — rooms
/// light up as the squad enters them, matching the fog-of-war reveal (`fog`; unexplored tiles stay black
/// void, the eerie part — see the `world` module doc). Shadowless for now: clustered point lights are
/// cheap (Bevy 0.19 clusters on the GPU), and shadow-caster culling is a later phase; GTAO + contact
/// shadows supply the depth cues. "Bake the many, light the few" adapted to raster.
///
/// Two passes, not one: every fixture's own `flicker_fail_ratio` roll is computed first, then capped
/// per room (`flicker_max_failing_per_room`) before any command is issued, so a room that rolls more
/// failing tubes than its cap allows doesn't have that decided by which fixture happened to be visited
/// first in this frame's query order (`tests/determinism_lint.rs`'s "query order decides nothing" rule
/// — see the `sort_total!` below for the total key that replaces query order).
fn attach_fixture_lights(
    mut commands: Commands,
    config: Res<GameConfig>,
    // A TV (`ScreenEmitter`) is excluded — it gets an eery screen spotlight in `attach_screen_lights`
    // instead of this generic room-fixture point light.
    fixtures: Query<
        (Entity, &Transform, Option<&PlacedIn>),
        (With<LightEmitter>, Without<FixtureLit>, Without<ScreenEmitter>),
    >,
    // Failing-tube slots already spent per room, carried BETWEEN runs of this system — see the use site.
    mut room_slots: Local<HashMap<FlickerRoom, usize>>,
    // The expedition those slots belong to. A `Local` survives the whole PROCESS, but fixtures are
    // `run_scoped()` and re-spawned per expedition while `RegionId` is a per-dungeon `u32` that restarts
    // at 0 — so without this, budget spent on run 1's room 3 permanently barred run 2's *different*
    // room 3, and later expeditions got no failing tubes at all.
    mut slots_run: Local<Option<u64>>,
    run_seed: Option<Res<crate::session::RunSeed>>,
) {
    // Reset on a new Branch universe. Keyed on the seed rather than on a state transition because this
    // system has no `OnEnter` to hang off — and the seed is exactly what makes one expedition a
    // different world from the last.
    let this_run = run_seed.map(|s| s.0);
    if *slots_run != this_run {
        room_slots.clear();
        *slots_run = this_run;
    }
    let c = &config.lighting;
    let color = Color::srgb(c.fixture_color[0], c.fixture_color[1], c.fixture_color[2]);

    struct Candidate {
        entity: Entity,
        phase: f32,
        hash: u32,
        /// The fixture's world position, bit-exact. Carried purely to complete the sort key below.
        pos_bits: [u32; 3],
        wants_to_fail: bool,
        room: FlickerRoom,
    }
    let mut candidates: Vec<Candidate> = fixtures
        .iter()
        .map(|(e, tf, placed)| {
            // Per-fixture flicker seed from the fixture's WORLD POSITION, not its entity id.
            // `e.to_bits()` is run-dependent (spawn order/allocator), so which tubes flicker/fail and
            // their phase would vary same-seed run to run — cosmetic only (never touches
            // `LightField`/`snapshot_hash`), but a position is stable, immortal, and level geometry
            // never shares a cell with itself.
            let p = tf.translation;
            let seed = p.x.to_bits() ^ p.y.to_bits().rotate_left(11) ^ p.z.to_bits().rotate_left(22);
            // A golden-angle phase decorrelates the shimmer; a hash of the seed picks the
            // `flicker_fail_ratio` fraction that WANT to fail — subject to the per-room cap below.
            let phase = flicker_phase(seed);
            let mut h = seed.wrapping_mul(0x9E37_79B1);
            h ^= h >> 16;
            let wants_to_fail = (h % 1000) as f32 / 1000.0 < c.flicker_fail_ratio;
            let room = placed.map_or(FlickerRoom::Solo(e), |p| FlickerRoom::Region(p.0));
            let pos_bits = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
            Candidate { entity: e, phase, hash: h, pos_bits, wants_to_fail, room }
        })
        .collect();

    // Lowest hash wins a room's failing slots first. The order is load-bearing — it decides a scarce
    // per-room resource — so it needs a key that is a TOTAL order, and `hash` alone is not one: `seed` is
    // a 3-into-1 XOR fold of the position's bits and is not injective, so two fixtures at different
    // positions can collide, and a raw `sort_by_key` would then resolve them by ECS query order, which is
    // not stable across `App` instances. Completing the key with the position's own bits makes it total
    // (level geometry never puts two fixtures at one point) and `sort_total!` proves that at runtime
    // under `test-harness`/debug instead of asserting it in a comment.
    crate::sort_total!(&mut candidates, |cand| (cand.hash, cand.pos_bits));
    // Persists across invocations. `attach_fixture_lights` runs on `Update` (not `Startup`) and filters on
    // `Without<FixtureLit>`, so fixtures arrive in whatever batches the fog reveal and GLB scene loads
    // produce; a `HashMap` rebuilt per call would hand every later batch a fresh full allowance and cap
    // nothing. Keyed by owned `FlickerRoom`, and each fixture is counted exactly once because it gets
    // `FixtureLit` in the same run.
    let slots_used: &mut HashMap<FlickerRoom, usize> = &mut room_slots;
    let failing: Vec<bool> = candidates
        .iter()
        .map(|cand| {
            if !cand.wants_to_fail {
                return false;
            }
            let used = slots_used.entry(cand.room.clone()).or_insert(0);
            if *used < c.flicker_max_failing_per_room {
                *used += 1;
                true
            } else {
                false
            }
        })
        .collect();

    for (cand, failing) in candidates.into_iter().zip(failing) {
        commands.entity(cand.entity).insert(FixtureLit).with_child((
            PointLight {
                color,
                intensity: c.fixture_intensity,
                range: c.fixture_range,
                shadow_maps_enabled: false,
                ..default()
            },
            // Dropped just below the fixture origin so a ceiling tube pools light onto the floor rather
            // than straight into the ceiling mesh it is flush against.
            Transform::from_xyz(0.0, -0.15, 0.0),
            FixtureLight { base_intensity: c.fixture_intensity, phase: cand.phase, failing },
        ));
    }
}

/// A stylised mains-hum shimmer on every fixture's real point light, with a `flicker_fail_ratio` fraction
/// dropping out like dying Backrooms fluorescents. **Cosmetic and windowed:** it modulates only the
/// rendered `PointLight` intensity, never the gameplay [`LightField`] (which uses the fixtures' steady
/// brightness so AI perception can't jitter at frame rate — research §3). Runs on `Update`.
fn flicker_lights(
    time: Res<Time>,
    config: Res<GameConfig>,
    mut lights: Query<(&FixtureLight, &bevy::camera::visibility::ViewVisibility, &mut PointLight)>,
) {
    let t = time.elapsed_secs();
    let depth = config.lighting.flicker_hum_depth;
    for (fl, vis, mut light) in &mut lights {
        // Off-screen fixtures don't shimmer. This is a purely cosmetic modulation (the gameplay
        // `LightField` reads the steady brightness), and writing an unseen light's intensity marks
        // it `Changed<PointLight>` — which re-runs its bounding-sphere insert and the GPU
        // light-buffer extract for every fixture in the dungeon, every frame. Measured 2026-07-31
        // (flicker_hum_depth 0.06 → 0.0 A/B, same seed/route): 10.38 → 8.69 ms median frame time,
        // with ~120 fixtures resident and ~7 visible. A light scrolling into view resumes its hum
        // on its next rendered frame, which is the first frame anyone could see it.
        if !vis.get() {
            continue;
        }
        // Shallow steady ripple — the fluorescent shimmer.
        let hum = 1.0 - depth * (0.5 + 0.5 * (t * FLICKER_HUM_HZ + fl.phase).sin());
        let mult = if fl.failing {
            // Failing tube: two detuned sines gate it near-off in irregular bursts (the dying-tube strobe).
            let n = ((t * 2.3 + fl.phase).sin() * (t * 5.7 + fl.phase * 1.7).sin()).abs();
            if n < 0.15 { 0.04 } else { hum * (0.35 + 0.65 * n) }
        } else {
            hum
        };
        // Only write — and thereby mark the light `Changed`, forcing a GPU light-buffer re-extract — when
        // the value actually moves. A failing tube clamped near-off (the `0.04` branch) and any fixture with
        // `flicker_hum_depth == 0` hold a constant value across frames, so this skips their per-frame churn
        // with zero visual change.
        let next = fl.base_intensity * mult;
        if light.intensity != next {
            light.intensity = next;
        }
    }
}

/// Make each fixture's GLB mesh **glow** by swapping its material for an emissive one — the visible "the
/// light is on" cue (Bevy raster: an emissive material glows but does not illuminate, so this is purely
/// cosmetic; `attach_fixture_lights` supplies the actual light). Reuses the async-scene-load material walk
/// from `squad::recolor_units`: retry each frame until the GLB has spawned mesh descendants, then tag the
/// fixture `FixtureGlowing` so it never runs again. One fresh material per fixture (not shared).
fn glow_fixtures(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config: Res<GameConfig>,
    // A TV keeps its own screen material (the teal CRT face); its glow is the spotlight cast, not an
    // emissive mesh swap — so exclude `ScreenEmitter` here.
    fixtures: Query<Entity, (With<LightEmitter>, Without<FixtureGlowing>, Without<ScreenEmitter>)>,
    children: Query<&Children>,
    has_material: Query<(), With<MeshMaterial3d<StandardMaterial>>>,
) {
    let c = &config.lighting;
    // Cool fluorescent glow — the tube colour lifted into an emissive HDR-ish value (LDR here, so a
    // modest multiplier reads as lit). Green channel highest for the uneasy low-CRI cast.
    let emissive = LinearRgba::rgb(
        c.fixture_color[0] * c.fixture_emissive,
        c.fixture_color[1] * c.fixture_emissive,
        c.fixture_color[2] * c.fixture_emissive,
    );
    for fixture in &fixtures {
        // Scene not instantiated yet → retry next frame (the async GLB load, exactly as recolor_units).
        let mut stack: Vec<Entity> = match children.get(fixture) {
            Ok(ch) => ch.iter().collect(),
            Err(_) => continue,
        };
        // Mint the emissive material lazily, only once a mesh is actually found — same anti-churn guard as
        // recolor_units (creating it up-front would orphan a throwaway asset every frame while streaming).
        let mut material: Option<Handle<StandardMaterial>> = None;
        while let Some(e) = stack.pop() {
            if has_material.get(e).is_ok() {
                let handle = material.get_or_insert_with(|| {
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(c.fixture_color[0], c.fixture_color[1], c.fixture_color[2]),
                        emissive,
                        ..default()
                    })
                });
                commands.entity(e).insert(MeshMaterial3d(handle.clone()));
            }
            if let Ok(ch) = children.get(e) {
                stack.extend(ch.iter());
            }
        }
        if material.is_some() {
            commands.entity(fixture).insert(FixtureGlowing);
        }
    }
}

/// Give a TV's CRT face its animated **static** by swapping just the screen sub-mesh's material for the
/// unlit [`TvStaticMaterial`] (moving snow + scanlines + vertical roll; player request). Being unlit it
/// self-glows — the "the screen is on" cue — while the spotlight in [`attach_screen_lights`] supplies the
/// cast light. Kit-agnostic: the screen face is identified by its cool, chromatic base colour (the teal
/// dead-channel face vs. the neutral-grey chassis), never a kit-specific material name, so any kit's TV
/// works with zero code change. Same async-scene-load walk as [`glow_fixtures`]; one shared static
/// material instance drives every TV (minted lazily). Windowed-only, never the harness — cosmetic and
/// determinism-neutral (the gameplay `LightField` reads the TV via its `LightEmitter`, unchanged).
fn glow_screens(
    mut commands: Commands,
    mut static_mats: ResMut<Assets<TvStaticMaterial>>,
    std_mats: Res<Assets<StandardMaterial>>,
    mut shared: Local<Option<Handle<TvStaticMaterial>>>,
    screens: Query<Entity, (With<ScreenEmitter>, Without<ScreenGlowing>)>,
    children: Query<&Children>,
    mats: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    for tv in &screens {
        // Scene not instantiated yet → retry next frame (the async GLB load, exactly as `glow_fixtures`).
        let mut stack: Vec<Entity> = match children.get(tv) {
            Ok(ch) => ch.iter().collect(),
            Err(_) => continue,
        };
        let mut lit_any = false;
        while let Some(e) = stack.pop() {
            if let Ok(mm) = mats.get(e) {
                // The CRT face is the cool, chromatic material (teal); a neutral-grey chassis (r≈g≈b) fails.
                let is_screen = std_mats.get(&mm.0).is_some_and(|base| {
                    let c = base.base_color.to_linear();
                    c.green + c.blue > 3.0 * c.red + 0.05
                });
                if is_screen {
                    // One shared static material animates every TV (its motion comes from `globals.time`).
                    let handle = shared
                        .get_or_insert_with(|| static_mats.add(TvStaticMaterial::default()))
                        .clone();
                    // Replace the StandardMaterial with the custom one — remove the old so the mesh isn't
                    // drawn twice (once per material pipeline).
                    commands
                        .entity(e)
                        .remove::<MeshMaterial3d<StandardMaterial>>()
                        .insert(MeshMaterial3d(handle));
                    lit_any = true;
                }
            }
            if let Ok(ch) = children.get(e) {
                stack.extend(ch.iter());
            }
        }
        // Only guard once a face was actually found; a TV whose screen mesh hasn't streamed in yet retries.
        if lit_any {
            commands.entity(tv).insert(ScreenGlowing);
        }
    }
}

#[cfg(test)]
mod tests {
    //! Pure `LightField` bake/query tests — hand-crafted `Dungeon::from_walkable` layouts, no App/GPU
    //! (the seed-in/assert-out convention of `wfc.rs`). The bake's determinism under sorted input is what
    //! the harness replay-hash test (Phase 2) pins end-to-end; here we pin the field math + occlusion.
    use super::*;

    /// A 7×1 corridor with cell (3,0) walled off (rock), splitting it — so light from one end cannot
    /// reach the far end (occlusion), and cells before the wall fall off with distance.
    fn corridor_with_wall() -> Dungeon {
        let mut walkable = vec![true; 7];
        walkable[3] = false;
        Dungeon::from_walkable(7, 1, walkable)
    }

    /// Bake the static base then compose with no flashlight cones — the production `LightField` write path
    /// (`bake_light_field` chained into `apply_dynamic_lights`) with no dynamic emitters, so `cells`
    /// reflects the furniture-only field the tests assert on.
    fn bake_static(field: &mut LightField, d: &Dungeon, fixtures: &[(IVec2, f32, f32)]) {
        field.bake(d, fixtures);
        field.compose(d, &[]);
    }

    #[test]
    fn fixture_lights_nearby_floor_with_falloff() {
        let d = corridor_with_wall();
        let mut field = LightField::new(&d);
        bake_static(&mut field, &d, &[(IVec2::new(0, 0), 1.0, 6.0)]);
        let at = |x: i32| field.sample(&d, d.cell_center(IVec2::new(x, 0)));
        assert!((at(0) - 1.0).abs() < 1e-6, "peak illuminance at the fixture cell");
        assert!(at(1) > at(2) && at(2) > 0.0, "monotone linear falloff away from the fixture");
        assert_eq!(field.peak(), at(0), "peak() is the brightest cell (the fixture cell)");
    }

    #[test]
    fn walls_cast_light_shadow() {
        let d = corridor_with_wall();
        let mut field = LightField::new(&d);
        bake_static(&mut field, &d, &[(IVec2::new(0, 0), 1.0, 6.0)]);
        let at = |x: i32| field.sample(&d, d.cell_center(IVec2::new(x, 0)));
        assert!(at(2) > 0.0, "cell before the wall is lit");
        assert_eq!(at(3), 0.0, "the wall cell itself carries no light (not floor)");
        assert_eq!(at(4), 0.0, "cell behind the wall is shadowed — line_of_sight blocked (no leak)");
        assert_eq!(at(5), 0.0, "further behind the wall stays dark");
    }

    #[test]
    fn bake_is_deterministic() {
        let d = corridor_with_wall();
        let fixtures = [(IVec2::new(0, 0), 1.0, 6.0), (IVec2::new(6, 0), 0.7, 6.0)];
        let mut a = LightField::new(&d);
        let mut b = LightField::new(&d);
        bake_static(&mut a, &d, &fixtures);
        bake_static(&mut b, &d, &fixtures);
        assert_eq!(a.cells(), b.cells(), "same (sorted) input → bit-identical field");
    }

    #[test]
    fn gradient_points_toward_the_light() {
        let d = corridor_with_wall();
        let mut field = LightField::new(&d);
        bake_static(&mut field, &d, &[(IVec2::new(0, 0), 1.0, 6.0)]);
        // At cell (1,0) the light rises toward the fixture at x=0, so the +gradient (increasing light)
        // has negative x. A photophobic crab steers along -gradient (+x, into the dark); a photophilic
        // one along +gradient (-x, toward the lamp).
        let g = field.gradient(&d, d.cell_center(IVec2::new(1, 0)));
        assert!(g.x < 0.0, "gradient of increasing illuminance points toward the fixture (-x)");
    }

    /// A flashlight cone aimed +x over open floor: lights the cells ahead, leaves those behind and to the
    /// side dark, and layers additively on the cached static base — the "moving deterrent" write path.
    #[test]
    fn flashlight_cone_lights_ahead_not_behind() {
        let d = Dungeon::from_walkable(7, 7, vec![true; 49]);
        let mut field = LightField::new(&d);
        field.bake(&d, &[]); // no fixtures → base is dark
        let cone = FlashlightCone {
            source: IVec2::new(3, 3),
            forward: Vec2::new(1.0, 0.0),
            intensity: 3.0,
            range: 4.0,
            cone_cos: 0.82, // ~35° half-angle
            edge_softness: 0.15,
        };
        field.compose(&d, &[cone]);
        let at = |x: i32, y: i32| field.sample(&d, d.cell_center(IVec2::new(x, y)));
        assert!(at(5, 3) > 0.0, "a cell straight ahead of the beam is lit");
        assert_eq!(at(1, 3), 0.0, "a cell directly behind the beam is dark (outside the cone)");
        assert_eq!(at(3, 6), 0.0, "a cell perpendicular to the beam is dark (outside the cone)");
        assert!(at(4, 3) > at(5, 3), "illuminance falls off with distance along the beam");
    }

    /// The dynamic compose must be bit-reproducible (it folds into the replay hash): same base + same
    /// sorted cones → identical `cells`. Mirrors `bake_is_deterministic` for the moving pass.
    #[test]
    fn flashlight_compose_is_deterministic() {
        let d = Dungeon::from_walkable(7, 7, vec![true; 49]);
        let cone = || FlashlightCone {
            source: IVec2::new(3, 3),
            forward: Vec2::new(0.6, 0.8).normalize(),
            intensity: 2.5,
            range: 4.0,
            cone_cos: 0.7,
            edge_softness: 0.2,
        };
        let mut a = LightField::new(&d);
        let mut b = LightField::new(&d);
        a.bake(&d, &[(IVec2::new(0, 0), 1.0, 3.0)]);
        b.bake(&d, &[(IVec2::new(0, 0), 1.0, 3.0)]);
        a.compose(&d, &[cone()]);
        b.compose(&d, &[cone()]);
        assert_eq!(a.cells(), b.cells(), "same base + same cone → bit-identical composed field");
    }

    #[test]
    fn phototropic_scale_grows_toward_light_and_holds_in_dark() {
        // In the dark (light01 = 0) the target is just the base size, so a body at base scale stays put.
        assert_eq!(phototropic_scale(4.0, 4.0, 0.0, 0.5, 1.0), 4.0);
        // Under full light it eases UP toward base·(1+bonus) = 6.0, but only by `max_step` this tick.
        let after_one = phototropic_scale(4.0, 4.0, 1.0, 0.5, 0.25);
        assert!((after_one - 4.25).abs() < 1e-6, "rate-limited one step toward the lit target");
        // It never overshoots the target even with a huge step.
        assert_eq!(phototropic_scale(4.0, 4.0, 1.0, 0.5, 100.0), 6.0);
        // Half light → half the bonus.
        assert_eq!(phototropic_scale(4.0, 4.0, 0.5, 0.5, 100.0), 5.0);
    }

    #[test]
    fn phototropic_scale_eases_back_down_when_light_leaves() {
        // A cap grown to 6.0 whose lamp fails (light01 = 0) eases back toward base, rate-limited, never
        // below 0. (Symmetric ease — Phase 4 flicker uses a running average so this stays gentle.)
        let shrunk = phototropic_scale(4.0, 6.0, 0.0, 0.5, 0.25);
        assert!((shrunk - 5.75).abs() < 1e-6, "eases back down one rate-limited step");
        assert_eq!(phototropic_scale(4.0, 6.0, 0.0, 0.5, 100.0), 4.0, "returns to base, not below");
    }
}
