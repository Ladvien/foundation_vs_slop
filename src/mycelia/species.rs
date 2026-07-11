//! The data-driven mushroom species table.
//!
//! The game grows many mushroom species from one shared simulation. Everything a species varies —
//! the growth mesh, its measured geometry (which feeds the perceptual speed limit in
//! [`super::perceptual`]), and its on-screen size — lives here as **data**, one row per species,
//! loaded from the `mycelia.species` slice of the RON config. The death cap is simply row `0`; no
//! system special-cases it. This mirrors the `Vec<DampWeight>` idiom already used for per-room
//! rot susceptibility (`super::DampWeight` + `validate_damp_coverage`).
//!
//! # Why the geometry is authored data, not read from the glb
//!
//! The perceptual module is pure arithmetic so the motion-threshold invariant can be *proved* in a
//! unit test with no ECS/GPU/async. The measured numbers here (`stage_max_disp`, `radius_profile`,
//! …) are produced offline by the asset framework's `_lib/inspect_glb.py`, which rebuilds each
//! morph stage from `basis + delta` and measures it, then pasted into the RON. A CI job re-runs the
//! sidecar and diffs it against the RON, so a regenerated asset that drifts fails loudly. Loading
//! the numbers from the `Handle<WorldAsset>` at runtime would be async (the scene is absent for the
//! first frames) and would make the speed limit un-provable at test time — a second path, and a
//! panic risk on frame one. One source of truth: the RON.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::perceptual::{segment_index, MAX_TILT, STAGE_T};

/// Dense index into the species table. The death cap is `SpeciesId(0)`. Kept a `Copy` newtype so
/// [`super::fruit::FruitBody`] stays cheap to clone and carries no large per-species arrays.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct SpeciesId(pub u16);

impl Default for SpeciesId {
    /// The death cap — the reference species — is always row 0.
    fn default() -> Self {
        Self(0)
    }
}

/// The measured geometry block for one species, in **metres at the asset's native scale**, exactly
/// as authored in RON. Derived quantities (adult height, bend fractions, …) are computed once into
/// [`SpeciesGeometry`]; only the *measured* numbers live here so the RON carries a single fact per
/// field. See the module header for how these are produced and kept honest.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SpeciesGeometryData {
    /// Longest vertex chord across each of the six morph segments — the basis of the speed limit.
    pub stage_max_disp: [f32; 6],
    /// Apex height at each baked stage (`STAGE_T` index for index).
    pub stage_height_m: [f32; 7],
    /// Height of the sealed spawn state — the distance the body rises out of the mat.
    pub egg_height_m: f32,
    /// Adult cap (pileus) radius.
    pub cap_radius_m: f32,
    /// Adult base/volva radius — the body's footprint on the floor and sibling-spacing basis.
    pub volva_radius_m: f32,
    /// Adult silhouette: the widest radius in each of 16 equal height slices (wall clearance solve).
    pub radius_profile: [f32; 16],
    /// The stipe's bending zone `[bend_lo_m, bend_hi_m]` (differential-elongation upper third).
    pub bend_lo_m: f32,
    pub bend_hi_m: f32,
}

/// One row of [`super::MyceliaConfig::species`] — a species as configuration.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SpeciesConfig {
    /// Human-readable name, e.g. `"Death Cap"`. For diagnostics and error messages.
    pub name: String,
    /// Asset path of the growth glb (six morph targets), relative to `assets/`.
    pub growth_glb: String,
    /// Uniform scale applied to the native-scale mesh. The death cap ships at 4.0 (13.9 cm → 56 cm).
    pub body_scale: f32,
    /// The measured geometry, resolved into a [`SpeciesGeometry`] at load.
    pub geom: SpeciesGeometryData,
}

/// Runtime per-species geometry: the measured block plus the quantities the perceptual speed limit
/// derives from it. Built once at startup, held in [`SpeciesTable`], looked up by [`SpeciesId`].
#[derive(Clone, Debug)]
pub struct SpeciesGeometry {
    pub stage_max_disp: [f32; 6],
    pub stage_height_m: [f32; 7],
    pub adult_height_m: f32,
    pub egg_height_m: f32,
    pub cap_radius_m: f32,
    pub volva_radius_m: f32,
    pub radius_profile: [f32; 16],
    pub bend_lo_m: f32,
    pub bend_hi_m: f32,
    /// Hard ceiling on apex deflection — 35% of adult height. Past this a stipe reads as broken.
    pub max_bend_m: f32,
    /// Fraction of total bend laid down during each morph segment (derived from `bend_profile`).
    pub stage_bend_fraction: [f32; 6],
    /// `|Δheight|` across each morph segment — a tilted body's sideways drift as it grows.
    pub stage_height_delta: [f32; 6],
}

impl SpeciesGeometry {
    /// Resolve measured data into runtime geometry, computing the derived quantities once.
    pub fn from_data(d: &SpeciesGeometryData) -> Self {
        let adult_height_m = d.stage_height_m[6];
        let max_bend_m = 0.35 * adult_height_m;
        // The bend profile for this species' own zone; used to derive per-segment bend fractions.
        let bend_profile = |y: f32| -> f32 {
            let denom = (d.bend_hi_m - d.bend_lo_m).max(f32::MIN_POSITIVE);
            let u = ((y - d.bend_lo_m) / denom).clamp(0.0, 1.0);
            u * u * (3.0 - 2.0 * u)
        };
        let mut stage_bend_fraction = [0.0; 6];
        let mut stage_height_delta = [0.0; 6];
        for k in 0..6 {
            stage_bend_fraction[k] = bend_profile(d.stage_height_m[k + 1]) - bend_profile(d.stage_height_m[k]);
            stage_height_delta[k] = (d.stage_height_m[k + 1] - d.stage_height_m[k]).abs();
        }
        Self {
            stage_max_disp: d.stage_max_disp,
            stage_height_m: d.stage_height_m,
            adult_height_m,
            egg_height_m: d.egg_height_m,
            cap_radius_m: d.cap_radius_m,
            volva_radius_m: d.volva_radius_m,
            radius_profile: d.radius_profile,
            bend_lo_m: d.bend_lo_m,
            bend_hi_m: d.bend_hi_m,
            max_bend_m,
            stage_bend_fraction,
            stage_height_delta,
        }
    }

    /// Centre height of `radius_profile[i]`.
    pub fn radius_slice_height(&self, i: usize) -> f32 {
        (i as f32 + 0.5) * self.adult_height_m / self.radius_profile.len() as f32
    }

    /// Fraction of a body's apex deflection applied at stipe height `y` (native-scale metres).
    /// Smoothstep over the bend zone: zero (with zero slope) below it, one above it. Duplicated in
    /// `mycelia_fruit.wgsl` for the death cap; per-species bend zones become shader uniforms later.
    pub fn bend_profile(&self, y: f32) -> f32 {
        let denom = (self.bend_hi_m - self.bend_lo_m).max(f32::MIN_POSITIVE);
        let u = ((y - self.bend_lo_m) / denom).clamp(0.0, 1.0);
        u * u * (3.0 - 2.0 * u)
    }

    /// Vertex travel charged to segment `k` (native-scale metres): the morph's own chord, plus the
    /// bend laid down while `growth` crosses the segment, plus a tilted body's sideways drift. The
    /// three need not point the same way, so their sum bounds the fastest vertex (triangle ineq.).
    fn segment_travel(&self, k: usize, bend_m: f32, tilt: f32) -> f32 {
        self.stage_max_disp[k]
            + self.stage_bend_fraction[k] * bend_m.abs().min(self.max_bend_m)
            + self.stage_height_delta[k] * tilt.abs().min(MAX_TILT)
    }

    /// `d(growth)/dt` that holds the fastest-moving vertex at exactly `v_max`. Always finite:
    /// every `stage_max_disp` entry is strictly positive and `body_scale` is validated `> 0`. The
    /// rate is unsigned — callers apply the biology gate (which may be negative).
    pub fn growth_rate(&self, growth: f32, body_scale: f32, bend_m: f32, tilt: f32, v_max: f32) -> f32 {
        let k = segment_index(growth);
        let span = STAGE_T[k + 1] - STAGE_T[k];
        let duration = self.segment_travel(k, bend_m, tilt) * body_scale / v_max;
        span / duration
    }

    /// Virtual seconds from sealed egg to adult at a fixed `v_max`, ignoring the rise. Diagnostics
    /// and tests only — the live clock re-evaluates `v_max` every frame against the zoom.
    pub fn egg_to_adult_secs(&self, body_scale: f32, bend_m: f32, tilt: f32, v_max: f32) -> f32 {
        (0..6).map(|k| self.segment_travel(k, bend_m, tilt) * body_scale / v_max).sum()
    }

    /// Smallest centre-to-centre spacing two bodies of `body_scale` may have (volvas touching).
    pub fn min_sibling_spacing(&self, body_scale: f32) -> f32 {
        2.0 * self.volva_radius_m * body_scale
    }
}

/// The death cap's measured geometry — the reference species and row 0 of the table. These are the
/// numbers previously carried as `perceptual::STAGE_MAX_DISP` etc., measured from the shipped
/// `death_cap_growth.glb` over all 1,379 vertices. Sums to 11.40 cm of vertex travel egg → adult.
/// Used to build the row-0 runtime geometry test fixture and to match the RON row 0.
pub fn death_cap_data() -> SpeciesGeometryData {
    SpeciesGeometryData {
        stage_max_disp: [0.00060, 0.01978, 0.03057, 0.02778, 0.02397, 0.01134],
        stage_height_m: [0.0485, 0.0484, 0.0627, 0.0933, 0.1192, 0.1345, 0.1393],
        egg_height_m: 0.0485,
        cap_radius_m: 0.0560,
        volva_radius_m: 0.0230,
        radius_profile: [
            0.0184, 0.0225, 0.0230, 0.0142, 0.0123, 0.0106, 0.0099, 0.0092, 0.0082, 0.0103, 0.0124,
            0.0088, 0.0070, 0.0560, 0.0533, 0.0396,
        ],
        bend_lo_m: 0.0891,
        bend_hi_m: 0.1180,
    }
}

/// The death cap as a full config row — row 0 of the table. Used by test config builders that
/// construct [`super::MyceliaConfig`] literally (the shipped RON carries the same row). The glb path
/// matches the currently-shipped `death_cap_growth.glb`, so row 0 is byte-identical to today.
pub fn death_cap_config_row() -> SpeciesConfig {
    SpeciesConfig {
        name: "Death Cap".to_string(),
        growth_glb: "death_cap/death_cap_growth.glb".to_string(),
        body_scale: 4.0,
        geom: death_cap_data(),
    }
}

/// Runtime table of per-species geometry, indexed by [`SpeciesId`]`.0`. Built once at startup from
/// `MyceliaConfig::species`. Row 0 is the death cap.
#[derive(Resource)]
pub struct SpeciesTable(pub Vec<SpeciesGeometry>);

impl SpeciesTable {
    /// The geometry for a species. Infallible: `validate_species_config` proves the table dense and
    /// every spawned `FruitBody.species` is chosen from it, so an out-of-range id is a contract
    /// violation, not a runtime possibility.
    pub fn get(&self, id: SpeciesId) -> &SpeciesGeometry {
        &self.0[id.0 as usize]
    }
}

/// Loaded growth scenes, parallel to [`SpeciesTable`]. Row 0 is the death cap. `WorldAssetRoot`
/// instantiates the chosen scene asynchronously beneath each spawned body.
#[derive(Resource)]
pub struct SpeciesScenes(pub Vec<Handle<WorldAsset>>);

impl SpeciesScenes {
    /// A clone of a species' scene handle.
    pub fn handle(&self, id: SpeciesId) -> Handle<WorldAsset> {
        self.0[id.0 as usize].clone()
    }
}
