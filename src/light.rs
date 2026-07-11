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

use bevy::pbr::ContactShadows;
use bevy::prelude::*;
use serde::Deserialize;

use crate::config::GameConfig;
use crate::dungeon::Dungeon;
use crate::util::{in_grid, row_major};

/// The `lighting:` slice of `assets/config/config.ron` — every light knob, one source of truth
/// (see [`GameConfig`]). Read by both [`crate::world`] (environment fill) and [`LightingPlugin`]
/// (fixtures). No fallback: a missing/invalid slice is a loud startup panic via [`validate_config`].
#[derive(Deserialize, Clone, Debug)]
pub struct LightingConfig {
    /// Ambient fill brightness — the flat Backrooms fluorescent wash. Lower than the old hardcoded 500
    /// so fixtures carve real contrast (dread/immersion rise as ambient falls — FDG 2014). Read by
    /// [`crate::world::WorldPlugin`].
    pub ambient_brightness: f32,
    /// Ambient fill colour (sRGB triple) — warm fluorescent.
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
    ] {
        if !(v.is_finite() && v >= 0.0) {
            return Err(format!("lighting.{name} must be finite and >= 0 (got {v})"));
        }
    }
    if !(c.fixture_range.is_finite() && c.fixture_range > 0.0) {
        return Err(format!("lighting.fixture_range must be finite and > 0 (got {})", c.fixture_range));
    }
    for (name, col) in [("ambient_color", c.ambient_color), ("fixture_color", c.fixture_color)] {
        if col.iter().any(|ch| !ch.is_finite() || *ch < 0.0) {
            return Err(format!("lighting.{name} channels must be finite and >= 0 (got {col:?})"));
        }
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
/// gradient is Physarum minimum-risk routing over an illumination field (Nakagaki et al., PRL 2007).
#[derive(Resource)]
pub struct LightField {
    width: usize,
    height: usize,
    /// Illuminance per dungeon cell (gameplay units), row-major. Only floor cells ever carry value.
    cells: Vec<f32>,
    /// Recompute pending. True at startup (bake once fixtures exist) and whenever a fixture changes state
    /// (Phase 4), gated like `fog::FogGrid::dirty` so the bake is event-driven, not per-frame.
    dirty: bool,
    /// Peak cell illuminance from the last bake — lets callers normalise to a 0..1 "brightness".
    peak: f32,
}

impl LightField {
    /// Empty field sized to the dungeon; starts `dirty` so the first `FixedUpdate` bakes it.
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, cells: vec![0.0; width * height], dirty: true, peak: 0.0 }
    }

    /// Point read at a world position (query). Off-grid reads as 0 — the same contract as `Stig::sample`.
    pub fn sample(&self, dungeon: &Dungeon, pos: Vec3) -> f32 {
        let c = dungeon.world_to_cell(pos);
        if in_grid(c, self.width, self.height) {
            self.cells[row_major(c, self.width)]
        } else {
            0.0
        }
    }

    /// World-XZ direction of *increasing* illuminance (central differences), magnitude ≈ the local slope
    /// — copied from `Stig::gradient`. A photophobic creature steers along `-gradient` (toward the dark),
    /// a phototropic/-philic one along `+gradient`.
    pub fn gradient(&self, dungeon: &Dungeon, pos: Vec3) -> Vec2 {
        let c = dungeon.world_to_cell(pos);
        let at = |dx: i32, dy: i32| -> f32 {
            let n = c + IVec2::new(dx, dy);
            if in_grid(n, self.width, self.height) {
                self.cells[row_major(n, self.width)]
            } else {
                0.0
            }
        };
        Vec2::new(at(1, 0) - at(-1, 0), at(0, 1) - at(0, -1))
    }

    /// Peak illuminance from the last bake (0 before the first bake).
    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// Recompute every cell from the fixture list — the bake. Each fixture is `(cell, intensity, range)`
    /// in cells; a cell within `range` of a fixture with an unobstructed `line_of_sight` to it gains
    /// `intensity * (1 - dist/range)`. Walls cast shadow (no LOS ⇒ no light). **Determinism:** `fixtures`
    /// must arrive in a stable order (the caller sorts by cell) so the per-cell float sum is reproducible
    /// — the discipline `Stig`'s sorted deposits use (float add is non-associative).
    fn bake(&mut self, dungeon: &Dungeon, fixtures: &[(IVec2, f32, f32)]) {
        for v in self.cells.iter_mut() {
            *v = 0.0;
        }
        for &(fcell, intensity, range) in fixtures {
            if range <= 0.0 {
                continue;
            }
            let r = range.ceil() as i32;
            for dy in -r..=r {
                for dx in -r..=r {
                    let cell = fcell + IVec2::new(dx, dy);
                    if !in_grid(cell, self.width, self.height) || !dungeon.is_floor(cell) {
                        continue;
                    }
                    let dist = ((dx * dx + dy * dy) as f32).sqrt();
                    if dist > range {
                        continue;
                    }
                    // Walls block light: only cells the fixture can "see" are lit (cheap leak-suppression).
                    if !dungeon.line_of_sight(fcell, cell) {
                        continue;
                    }
                    self.cells[row_major(cell, self.width)] += intensity * (1.0 - dist / range);
                }
            }
        }
        self.peak = self.cells.iter().copied().fold(0.0f32, f32::max);
        self.dirty = false;
    }

    /// Mark the field for recompute (Phase 4: a fixture switched on/off/failing).
    #[allow(dead_code)]
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// FNV-1a-fold every cell's bit pattern into `hash` — the determinism oracle for the bake, mirroring
    /// `Stig::fold_fingerprint`. Once creature steering reads the field (Phase 2) a broken bake/occlusion
    /// that shifts a crab would change the replay hash; this pins the field itself too. Test-only.
    #[cfg(feature = "test-harness")]
    pub fn fold_fingerprint(&self, hash: &mut u64) {
        for &v in &self.cells {
            for &b in &v.to_bits().to_le_bytes() {
                *hash ^= b as u64;
                *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
}

/// Bake the light field when dirty: collect [`LightEmitter`] fixture cells (stable-sorted for a
/// deterministic float sum), then recompute. Runs on `FixedUpdate` in [`LightFieldWritten`] so the pinned
/// creature readers (Phase 2) see a consistent field. Uses fixture `Transform` (world-space at spawn —
/// furniture never moves), not `GlobalTransform`, to avoid propagation-timing on the first tick. If no
/// fixtures exist yet (spawn not flushed) it stays dirty and retries next tick.
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
    fx.sort_unstable_by_key(|(c, _, _)| (c.x, c.y));
    field.bake(&dungeon, &fx);
}

/// Owns the gameplay [`LightField`]. Registered in BOTH the windowed game and the headless harness
/// (unlike [`LightingPlugin`]) because the field is CPU gameplay state creature AI reads — so the
/// deterministic replay gate must cover its bake. Requires `Dungeon` at build (DungeonPlugin precedes it).
pub struct LightFieldPlugin;

impl Plugin for LightFieldPlugin {
    fn build(&self, app: &mut App) {
        let dungeon = app
            .world()
            .get_resource::<Dungeon>()
            .expect("LightFieldPlugin requires DungeonPlugin to be registered first");
        let field = LightField::new(dungeon.width, dungeon.height);
        app.insert_resource(field)
            .add_systems(FixedUpdate, bake_light_field.in_set(LightFieldWritten));
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

/// Per-fixture flicker state, carried on the real point-light child (cosmetic, windowed-only).
/// `base_intensity` is the unflickered lumens; `phase` decorrelates the hum so tubes don't shimmer in
/// lockstep; `failing` tubes drop out / strobe like dying Backrooms fluorescents.
#[derive(Component)]
struct FixtureLight {
    base_intensity: f32,
    phase: f32,
    failing: bool,
}

// ---------------------------------------------------------------------------------------------------
// Light-response markers — the composable toolkit. Any creature can carry one to gain emergent behaviour
// around light and its absence; the generic `light_push` (below) reads the shared LightField gradient.
// The photophobic/-philic duality is the FleeGradient/FollowGradient pair from `ai::field`, for light.
// Research: crustacean noxious-stimulus avoidance (Cano et al. 2011); Physarum photoavoidance as
// minimum-risk routing over an illumination field (Nakagaki et al., PRL 2007).
// ---------------------------------------------------------------------------------------------------

/// Avoids light: the creature steers **down** the [`LightField`] gradient (toward the dark), strength
/// `lighting.photophobic_gain`. Carried by crabs — they pool in shadow and cede the lit rooms.
#[derive(Component)]
pub struct Photophobic;

/// Drawn to light: the creature steers **up** the [`LightField`] gradient (toward the light), strength
/// `lighting.photophilic_gain`. A ready toolkit component for the light-seeking "other creatures" a
/// designer adds; the generic push supports it identically to [`Photophobic`], opposite sign.
#[derive(Component)]
pub struct Photophilic;

/// Grows/orients **toward** light — a *tropism*, not steering. Carried by mushroom fruit bodies (Phase 3),
/// where light both enlarges the cap and leans it toward the brightest neighbour. Defined here with the
/// other light-response markers; its consumer lives in `mycelia::fruit`.
#[derive(Component)]
pub struct Phototropic;

/// World-XZ steering push a light-response creature feels at `pos`: `signed_gain · ∇illuminance`. A
/// photophobic creature passes `-photophobic_gain` (descends toward the dark), a photophilic one
/// `+photophilic_gain` (climbs toward the light). Zero where the field is flat (deep dark or the middle of
/// a uniform pool), so a creature far from any light gradient is unbiased — the graceful "no cost off in
/// the dark" property. Pure: the caller projects the result onto the locomotion surface and scales by `dt`
/// (see `crab::crab_locomotion`).
pub fn light_push(field: &LightField, dungeon: &Dungeon, pos: Vec3, signed_gain: f32) -> Vec3 {
    if signed_gain == 0.0 {
        return Vec3::ZERO;
    }
    let g = field.gradient(dungeon, pos);
    Vec3::new(g.x, 0.0, g.y) * signed_gain
}

/// The next rendered scale for a [`Phototropic`] fruit body easing toward its light-scaled target size
/// `base·(1 + bonus·light01)`, approached from `current` by at most `max_step` this tick — rate-limited so
/// the enlargement stays sub-perceptual (the mold's speed-limit ethos). `light01` is the illuminance
/// normalised to the field peak (`0` = dark ⇒ target is just `base`; `1` = brightest ⇒ full bonus). Pure,
/// so `mycelia::grow_fruit_bodies` stays a thin caller and the growth math is unit-tested here.
/// Photomorphogenesis — fungal fruiting is light-gated (Zhang et al., PLoS ONE 10:e0123025, 2015).
pub fn phototropic_scale(base: f32, current: f32, light01: f32, bonus: f32, max_step: f32) -> f32 {
    let target = base * (1.0 + bonus * light01.clamp(0.0, 1.0));
    (current + (target - current).clamp(-max_step, max_step)).max(0.0)
}

/// Windowed-game lighting: real fixture lights + camera screen-space FX. **Never** registered in the
/// headless harness (GPU/cosmetic only — the deterministic core must not depend on it). The SSAO and
/// contact-shadow *plugins* already ship inside Bevy's default `PbrPlugin`, so we only attach their
/// camera *components* here.
pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup_camera_fx)
            .add_systems(Update, (attach_fixture_lights, glow_fixtures, flicker_lights));
    }
}

/// Attach contact shadows to the camera. Runs once at `PostStartup` (after `camera::setup_camera`'s
/// `Startup` spawn has flushed). Contact shadows re-attach props to the floor "without the cost of full
/// raytracing" and, unlike Bevy's GTAO/SSAO, do **not** require `Msaa::Off` — so the scene keeps its
/// cheap 4× MSAA edge smoothing (this stylized isometric look leans on clean edges, and the VHS
/// post-process already stylizes; GTAO's corner-darkening is not worth losing MSAA here). The component
/// `#[require]`s a depth prepass, which auto-inserts. Kept LDR — no HDR/Bloom (mycelia is LDR-calibrated).
fn setup_camera_fx(mut commands: Commands, cam: Query<Entity, With<Camera3d>>) {
    for e in &cam {
        commands.entity(e).insert(ContactShadows::default());
    }
}

/// Give each newly-revealed [`LightEmitter`] a real clustered [`PointLight`] child so fixtures actually
/// cast light. The light is a **child**, so it inherits the fixture's fog-reveal `Visibility` — rooms
/// light up as the squad enters them, matching the fog-of-war reveal (`fog`; unexplored tiles stay black
/// void, the eerie part — see the `world` module doc). Shadowless for now: clustered point lights are
/// cheap (Bevy 0.19 clusters on the GPU), and shadow-caster culling is a later phase; GTAO + contact
/// shadows supply the depth cues. "Bake the many, light the few" adapted to raster.
fn attach_fixture_lights(
    mut commands: Commands,
    config: Res<GameConfig>,
    fixtures: Query<Entity, (With<LightEmitter>, Without<FixtureLit>)>,
) {
    let c = &config.lighting;
    let color = Color::srgb(c.fixture_color[0], c.fixture_color[1], c.fixture_color[2]);
    for e in &fixtures {
        // Per-fixture flicker seed from the entity id (cosmetic only). A golden-angle phase decorrelates
        // the shimmer; a hash of the id picks the `flicker_fail_ratio` fraction that fail.
        let seed = e.to_bits() as u32;
        let phase = seed as f32 * 2.399_963; // golden angle (radians)
        let mut h = seed.wrapping_mul(0x9E37_79B1);
        h ^= h >> 16;
        let failing = (h % 1000) as f32 / 1000.0 < c.flicker_fail_ratio;
        commands.entity(e).insert(FixtureLit).with_child((
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
            FixtureLight { base_intensity: c.fixture_intensity, phase, failing },
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
    mut lights: Query<(&FixtureLight, &mut PointLight)>,
) {
    let t = time.elapsed_secs();
    let depth = config.lighting.flicker_hum_depth;
    for (fl, mut light) in &mut lights {
        // Shallow steady ripple — the fluorescent shimmer.
        let hum = 1.0 - depth * (0.5 + 0.5 * (t * FLICKER_HUM_HZ + fl.phase).sin());
        let mult = if fl.failing {
            // Failing tube: two detuned sines gate it near-off in irregular bursts (the dying-tube strobe).
            let n = ((t * 2.3 + fl.phase).sin() * (t * 5.7 + fl.phase * 1.7).sin()).abs();
            if n < 0.15 { 0.04 } else { hum * (0.35 + 0.65 * n) }
        } else {
            hum
        };
        light.intensity = fl.base_intensity * mult;
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
    fixtures: Query<Entity, (With<LightEmitter>, Without<FixtureGlowing>)>,
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

    #[test]
    fn fixture_lights_nearby_floor_with_falloff() {
        let d = corridor_with_wall();
        let mut field = LightField::new(7, 1);
        field.bake(&d, &[(IVec2::new(0, 0), 1.0, 6.0)]);
        let at = |x: i32| field.sample(&d, d.cell_center(IVec2::new(x, 0)));
        assert!((at(0) - 1.0).abs() < 1e-6, "peak illuminance at the fixture cell");
        assert!(at(1) > at(2) && at(2) > 0.0, "monotone linear falloff away from the fixture");
        assert_eq!(field.peak(), at(0), "peak() is the brightest cell (the fixture cell)");
    }

    #[test]
    fn walls_cast_light_shadow() {
        let d = corridor_with_wall();
        let mut field = LightField::new(7, 1);
        field.bake(&d, &[(IVec2::new(0, 0), 1.0, 6.0)]);
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
        let mut a = LightField::new(7, 1);
        let mut b = LightField::new(7, 1);
        a.bake(&d, &fixtures);
        b.bake(&d, &fixtures);
        assert_eq!(a.cells, b.cells, "same (sorted) input → bit-identical field");
    }

    #[test]
    fn gradient_points_toward_the_light() {
        let d = corridor_with_wall();
        let mut field = LightField::new(7, 1);
        field.bake(&d, &[(IVec2::new(0, 0), 1.0, 6.0)]);
        // At cell (1,0) the light rises toward the fixture at x=0, so the +gradient (increasing light)
        // has negative x. A photophobic crab steers along -gradient (+x, into the dark); a photophilic
        // one along +gradient (-x, toward the lamp).
        let g = field.gradient(&d, d.cell_center(IVec2::new(1, 0)));
        assert!(g.x < 0.0, "gradient of increasing illuminance points toward the fixture (-x)");
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
